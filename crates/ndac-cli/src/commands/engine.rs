//! The apply engine shared by `apply` and `rollback`: confirmation, lock,
//! snapshots, execution, ID write-back and verification.

use std::{
    fs::{self, File, TryLockError},
    io::{self, BufRead, IsTerminal, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use miette::Report;
use ndac_api::Profile;
use ndac_core::{
    apply::{self, ApiCall, Outcome},
    config::DesiredProfile,
    plan::{self, Plan, ProfilePlan, RemovedProfile},
    snapshot::SnapshotKind,
};
use serde_json::Value;
use tokio::task::JoinSet;

use super::{Failure, session::Session};
use crate::output::{Exit, Output};

/// One profile to apply.
pub(crate) struct Work {
    pub(crate) desired: DesiredProfile,
    pub(crate) plan: ProfilePlan,
    pub(crate) live: Option<Profile>,
}

pub(crate) struct Options {
    pub(crate) auto_approve: bool,
    pub(crate) allow_destroy: bool,
    /// Write a created profile's ID back into its file.
    pub(crate) write_back: bool,
}

fn other(msg: impl Into<String>) -> Failure {
    Failure::Other(Report::msg(msg.into()))
}

/// Asks for confirmation on the terminal. Without a terminal, only
/// `--auto-approve` proceeds.
fn confirm(summary: &str) -> Result<bool, Failure> {
    if !io::stdin().is_terminal() {
        return Err(other(
            "stdin is not a terminal; review the plan and pass --auto-approve to apply without confirmation",
        ));
    }
    let mut err = io::stderr().lock();
    let _ = write!(
        err,
        "\n{summary}\nApply these changes? Only 'yes' is accepted: "
    );
    let _ = err.flush();
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| other(format!("cannot read the answer: {e}")))?;
    Ok(line.trim() == "yes")
}

/// Holds `<snapshot_dir>/.lock` for the lifetime of the value.
struct Lock(#[allow(dead_code, reason = "held for its lock")] File);

fn lock(dir: &Path) -> Result<Lock, Failure> {
    fs::create_dir_all(dir).map_err(|e| other(format!("cannot create {}: {e}", dir.display())))?;
    let path = dir.join(".lock");
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| other(format!("cannot open {}: {e}", path.display())))?;
    match file.try_lock() {
        Ok(()) => Ok(Lock(file)),
        Err(TryLockError::WouldBlock) => Err(other(format!(
            "another apply is running in this checkout (lock held on {})",
            path.display()
        ))),
        Err(TryLockError::Error(e)) => Err(other(format!("cannot lock {}: {e}", path.display()))),
    }
}

fn to_json(p: &Profile) -> Value {
    serde_json::to_value(p).unwrap_or(Value::Null)
}

/// Applies `work` (and deletes `removed` profiles when allowed). Prints the
/// plan, asks for confirmation, and reports per profile.
#[allow(
    clippy::too_many_lines,
    reason = "one linear procedure, easier to audit in one place"
)]
pub(crate) fn run(
    session: &mut Session<'_>,
    work: Vec<Work>,
    removed: Vec<RemovedProfile>,
    opts: &Options,
    out: Output,
) -> Result<Exit, Failure> {
    let mut work: Vec<Work> = work.into_iter().filter(|w| !w.plan.is_empty()).collect();
    // Removed profiles are always shown; they are only deleted with
    // --allow-destroy.
    let shown = Plan {
        profiles: work.iter().map(|w| w.plan.clone()).collect(),
        removed: removed.clone(),
    };
    let removed: Vec<RemovedProfile> = if opts.allow_destroy {
        removed.into_iter().filter(|r| r.id.is_some()).collect()
    } else {
        Vec::new()
    };
    if work.is_empty() && removed.is_empty() {
        Output::artifact(&plan::render_text(&shown, out.stdout_color))
            .map_err(|e| other(e.to_string()))?;
        return Ok(Exit::Success);
    }

    if !opts.allow_destroy {
        for w in &work {
            if let Some(list) = apply::empties_a_list(&w.plan, &w.desired) {
                return Err(other(format!(
                    "profile \"{}\": this would remove every item of `{list}`; pass --allow-destroy if that is intended",
                    w.desired.key
                )));
            }
        }
    }

    Output::artifact(&plan::render_text(&shown, out.stdout_color))
        .map_err(|e| other(e.to_string()))?;
    if !removed.is_empty() {
        out.status(&format!(
            "{} profile(s) will be DELETED from NextDNS.",
            removed.len()
        ));
    }
    if !opts.auto_approve && !confirm(&plan::summary_line(shown.summary()))? {
        out.status("Apply cancelled.");
        return Ok(Exit::Error);
    }

    let _lock = lock(session.store.dir())?;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let cancel = Arc::clone(&cancel);
        session.rt.spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                cancel.store(true, Ordering::SeqCst);
                let _ = writeln!(
                    io::stderr(),
                    "\ninterrupted: finishing the request in flight, then stopping"
                );
            }
        });
    }

    // Snapshot before touching anything.
    for w in &work {
        if let (Some(id), Some(live)) = (&w.desired.id, &w.live) {
            session
                .store
                .save(&w.desired.key, id, SnapshotKind::PreApply, &to_json(live))
                .map_err(|e| other(format!("cannot write the pre-apply snapshot: {e}")))?;
        }
    }

    // Each profile runs its calls in order; profiles run concurrently and
    // the client caps requests in flight.
    let mut jobs = Vec::new();
    for (i, w) in work.iter().enumerate() {
        let client = session.client(&w.desired.key)?;
        jobs.push((
            i,
            client,
            w.desired.id.clone(),
            apply::calls(&w.plan, &w.desired),
        ));
    }
    for r in &removed {
        let client = session.client(&r.key)?;
        jobs.push((
            usize::MAX,
            client,
            r.id.clone(),
            vec![ApiCall::DeleteProfile],
        ));
    }
    let job_count = jobs.len();
    let outcomes: Vec<(usize, Option<String>, Outcome, usize)> = session.rt.block_on({
        let cancel = Arc::clone(&cancel);
        async move {
            let mut set = JoinSet::new();
            for (i, client, id, calls) in jobs {
                let cancel = Arc::clone(&cancel);
                set.spawn(async move {
                    let o = apply::execute(&client, id.as_deref(), &calls, &cancel).await;
                    (i, id, o, calls.len())
                });
            }
            let mut out = Vec::with_capacity(job_count);
            while let Some(Ok(r)) = set.join_next().await {
                out.push(r);
            }
            out
        }
    });

    let mut failed = false;
    let mut auth_failed = false;
    let mut verify_failed = false;
    let mut by_index: Vec<Option<(Outcome, usize)>> = (0..work.len()).map(|_| None).collect();
    for (i, id, outcome, total) in outcomes {
        if i == usize::MAX {
            let key = removed
                .iter()
                .find(|r| r.id == id)
                .map_or_else(String::new, |r| r.key.clone());
            match &outcome.error {
                None if outcome.done == total => {
                    out.status(&format!("✓ {key}: profile deleted"));
                    retire_snapshots(session, &key);
                }
                Some((call, e)) => {
                    failed = true;
                    auth_failed |= e.is_auth();
                    out.status(&format!("✗ {key}: {call} failed: {e}"));
                }
                None => out.status(&format!("- {key}: not deleted (interrupted)")),
            }
            continue;
        }
        if let Some(slot) = by_index.get_mut(i) {
            *slot = Some((outcome, total));
        }
    }

    for (w, result) in work.iter_mut().zip(by_index) {
        let key = w.desired.key.clone();
        let Some((outcome, total)) = result else {
            continue;
        };
        if let Some(new_id) = &outcome.created_id {
            w.desired.id = Some(new_id.clone());
            if opts.write_back && w.desired.source.is_file() {
                apply::write_id(&w.desired.source, new_id).map_err(|e| {
                    other(format!(
                        "profile created as {new_id}, but writing the ID to {} failed: {e}",
                        w.desired.source.display()
                    ))
                })?;
                out.status(&format!(
                    "  wrote id = \"{new_id}\" to {}",
                    w.desired.source.display()
                ));
            }
        }
        if let Some((call, e)) = &outcome.error {
            failed = true;
            auth_failed |= e.is_auth();
            out.status(&format!(
                "✗ {key}: {} of {total} call(s) applied; {call} failed: {e}",
                outcome.done
            ));
        } else if outcome.done < total {
            out.status(&format!(
                "- {key}: {} of {total} call(s) applied before the interrupt",
                outcome.done
            ));
        }
        let Some(id) = w.desired.id.clone() else {
            continue;
        };

        // Verify: the live profile must now match.
        let client = session.client(&key)?;
        let after = session
            .rt
            .block_on(client.get_profile(&id))
            .map_err(Failure::api)?;
        let remaining = plan::diff(&w.desired, Some(&after));
        let complete = outcome.error.is_none() && outcome.done == total;
        let kind = if remaining.changes.is_empty() && complete {
            SnapshotKind::PostApply
        } else {
            SnapshotKind::FailedApply
        };
        session
            .store
            .save(&key, &id, kind, &to_json(&after))
            .map_err(|e| other(format!("cannot write the snapshot: {e}")))?;
        if complete && remaining.changes.is_empty() {
            out.status(&format!(
                "✓ {key} ({id}): {total} call(s) applied and verified"
            ));
        } else if complete {
            verify_failed = true;
            out.status(&format!(
                "✗ {key} ({id}): apply verification failed, {} difference(s) remain; see `ndac plan` and the failed-apply snapshot",
                remaining.changes.len()
            ));
        }
    }

    Ok(if cancel.load(Ordering::SeqCst) {
        Exit::Interrupted
    } else if auth_failed {
        Exit::Auth
    } else if failed {
        Exit::Api
    } else if verify_failed {
        Exit::Error
    } else {
        Exit::Success
    })
}

/// Moves a deleted profile's snapshots aside so plans stop reporting it,
/// while keeping them for reference.
fn retire_snapshots(session: &Session<'_>, key: &str) {
    let from = session.store.dir().join(key);
    let stamp = ndac_core::snapshot::rfc3339(std::time::SystemTime::now()).replace(':', "");
    let to = session.store.dir().join(format!(".deleted-{key}-{stamp}"));
    let _ = fs::rename(from, to);
}
