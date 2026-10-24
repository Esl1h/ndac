//! `ndac reconcile`.

use std::{
    collections::BTreeMap,
    fs,
    io::{self, BufRead, IsTerminal, Write},
    time::SystemTime,
};

use miette::Report;
use ndac_api::Profile;
use ndac_core::{
    config::{Entry, Workspace},
    plan::{Change, Origin, ProfilePlan, drift_only, render_text},
    reconcile::{WriteBack, template_origin},
    snapshot::rfc3339,
};
use serde_json::Value;

use super::{
    CmdResult, Failure,
    engine::{self, Options, Work},
    load,
    plan::parse_targets,
    session::Session,
};
use crate::{
    cli::{GlobalArgs, Prefer},
    output::{Exit, Output},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Overwrite,
    Import,
    Skip,
}

fn other(msg: impl Into<String>) -> Failure {
    Failure::Other(Report::msg(msg.into()))
}

fn prompt(text: &str) -> Result<String, Failure> {
    let mut err = io::stderr().lock();
    let _ = write!(err, "{text}");
    let _ = err.flush();
    let mut line = String::new();
    let n = io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| other(format!("cannot read the answer: {e}")))?;
    if n == 0 {
        return Err(other("no answer (end of input); nothing was changed"));
    }
    Ok(line.trim().to_owned())
}

fn shown(v: Option<&Value>) -> String {
    v.map_or_else(|| "(unset)".to_owned(), Value::to_string)
}

fn describe(c: &Change) -> String {
    let tag = match c.origin() {
        Origin::Conflict => "[conflict]",
        _ => "[drift]",
    };
    let what = match c {
        Change::SetField { path, from, to, .. } => format!(
            "{path}: configuration {to}, NextDNS {}",
            shown(from.as_ref())
        ),
        Change::AddItem { list, id, .. } => {
            format!("{list}: {id} is in the configuration, not in NextDNS")
        }
        Change::RemoveItem { list, id, .. } => {
            format!("{list}: {id} is in NextDNS, not in the configuration")
        }
        Change::SetItemActive {
            list, id, from, to, ..
        } => format!("{list}: {id} active: configuration {to}, NextDNS {from}"),
    };
    format!("{what} {tag}")
}

/// Asks what to do with one item. The second value is true for "all
/// remaining".
fn ask_item(key: &str, c: &Change) -> Result<(Choice, bool), Failure> {
    let text = format!(
        "\nprofile \"{key}\": {}\n[o]verwrite live  [i]mport into code  [s]kip  [O]/[I] all remaining: ",
        describe(c)
    );
    loop {
        match prompt(&text)?.as_str() {
            "o" => return Ok((Choice::Overwrite, false)),
            "i" => return Ok((Choice::Import, false)),
            "s" | "" => return Ok((Choice::Skip, false)),
            "O" => return Ok((Choice::Overwrite, true)),
            "I" => return Ok((Choice::Import, true)),
            _ => {}
        }
    }
}

/// Profiles whose layers include the file of `template`.
fn used_by(ws: &Workspace, template: &Entry) -> Vec<String> {
    ws.profiles
        .keys()
        .filter(|k| {
            ws.layer_entries(k)
                .is_ok_and(|l| l.iter().any(|e| e.src.path == template.src.path))
        })
        .cloned()
        .collect()
}

/// Asks whether to import into the profile or the template the value
/// comes from.
fn ask_where(ws: &Workspace, layers: &[&Entry], template: usize) -> Result<usize, Failure> {
    let profile = layers.len() - 1;
    let Some(t) = layers.get(template) else {
        return Ok(profile);
    };
    let text = format!(
        "  this value comes from {} (used by: {}).\n  Write to [p]rofile (an override, default) or [t]emplate? ",
        t.src.display,
        used_by(ws, t).join(", ")
    );
    loop {
        match prompt(&text)?.as_str() {
            "p" | "" => return Ok(profile),
            "t" => return Ok(template),
            _ => {}
        }
    }
}

struct Decisions {
    writes: WriteBack,
    overwrite: BTreeMap<String, Vec<Change>>,
    imported: usize,
    skipped: usize,
    failed: bool,
}

fn decide(
    ws: &Workspace,
    drift: &[ProfilePlan],
    live: &BTreeMap<String, Profile>,
    prefer: Option<Prefer>,
    ask_template: bool,
    note: Option<String>,
) -> Result<Decisions, Failure> {
    let mut d = Decisions {
        writes: WriteBack::new(note),
        overwrite: BTreeMap::new(),
        imported: 0,
        skipped: 0,
        failed: false,
    };
    let mut all = prefer.map(|p| match p {
        Prefer::Code => Choice::Overwrite,
        Prefer::Live => Choice::Import,
    });
    for p in drift {
        let Some(live) = live.get(&p.key) else {
            continue;
        };
        let layers = ws.layer_entries(&p.key)?;
        for c in &p.changes {
            let choice = if let Some(a) = all {
                a
            } else {
                let (choice, sticky) = ask_item(&p.key, c)?;
                if sticky {
                    all = Some(choice);
                }
                choice
            };
            match choice {
                Choice::Skip => d.skipped += 1,
                Choice::Overwrite => d
                    .overwrite
                    .entry(p.key.clone())
                    .or_default()
                    .push(c.clone()),
                Choice::Import => {
                    let target = match template_origin(&layers, c) {
                        Some(t) if ask_template => ask_where(ws, &layers, t)?,
                        _ => layers.len() - 1,
                    };
                    match d.writes.import(&layers, target, c, live) {
                        Ok(()) => d.imported += 1,
                        Err(e) => {
                            Output::warn(&format!(
                                "profile \"{}\": cannot import {e}; skipped",
                                p.key
                            ));
                            d.skipped += 1;
                            d.failed = true;
                        }
                    }
                }
            }
        }
    }
    Ok(d)
}

/// Writes the imports, then reloads the configuration; if it no longer
/// loads, the files are restored.
fn write_imports(ws: &Workspace, writes: &WriteBack, out: Output) -> Result<(), Failure> {
    let originals = writes.originals();
    let written = writes
        .write()
        .map_err(|e| other(format!("cannot write the configuration: {e}")))?;
    let check = Workspace::load(&ws.root_src.path).and_then(|w| w.resolve_all().map(|_| ()));
    if let Err(e) = check {
        for (path, text) in originals {
            let _ = fs::write(path, text);
        }
        return Err(Failure::Config(e));
    }
    for p in written {
        let rel = p.strip_prefix(&ws.root_dir).unwrap_or(&p);
        out.status(&format!("✓ updated {}", rel.display()));
    }
    Ok(())
}

pub(crate) fn run(
    global: &GlobalArgs,
    targets: &[String],
    prefer: Option<Prefer>,
    non_interactive: bool,
    no_comment: bool,
    allow_destroy: bool,
    out: Output,
) -> CmdResult {
    let tty = io::stdin().is_terminal();
    if prefer.is_none() && !tty {
        return Err(other(
            "stdin is not a terminal; pass --non-interactive --prefer code|live",
        ));
    }
    let targets = parse_targets(targets)?;
    let ws = load(global.config.as_deref())?;
    let mut desired = ws.resolve_all()?;
    if !targets.is_empty() {
        desired.retain(|d| targets.iter().any(|t| t.matches_profile(&d.key)));
    }
    let mut session = Session::new(ws, global)?;
    let live = session.fetch_live(&desired)?;
    let drift = drift_only(session.plan(&desired, &live)?.filtered(&targets));
    if drift.profiles.iter().all(|p| p.changes.is_empty()) {
        out.status("No drift since the last apply.");
        return Ok(Exit::Success);
    }
    Output::artifact(&render_text(&drift, out.stdout_color))
        .map_err(|e| other(format!("cannot write output: {e}")))?;

    let live: BTreeMap<String, Profile> = desired
        .iter()
        .zip(live)
        .filter_map(|(d, l)| Some((d.key.clone(), l?)))
        .collect();
    let note = (!no_comment).then(|| {
        let today = rfc3339(SystemTime::now());
        format!("imported {}", today.get(..10).unwrap_or(&today))
    });
    let d = decide(
        &session.ws,
        &drift.profiles,
        &live,
        prefer,
        tty && !non_interactive,
        note,
    )?;
    write_imports(&session.ws, &d.writes, out)?;
    out.status(&format!(
        "{} imported, {} to overwrite, {} skipped",
        d.imported,
        d.overwrite.values().map(Vec::len).sum::<usize>(),
        d.skipped
    ));

    let mut exit = Exit::Success;
    if !d.overwrite.is_empty() {
        let work: Vec<Work> = desired
            .into_iter()
            .filter_map(|desired| {
                let changes = d.overwrite.get(&desired.key)?.clone();
                let live = live.get(&desired.key).cloned();
                let plan = ProfilePlan {
                    key: desired.key.clone(),
                    id: desired.id.clone(),
                    create: false,
                    list_modes: desired.list_modes.clone(),
                    changes,
                };
                Some(Work {
                    desired,
                    plan,
                    live,
                })
            })
            .collect();
        // Choosing "overwrite" per item is the confirmation.
        let opts = Options {
            auto_approve: true,
            allow_destroy,
            write_back: false,
        };
        exit = engine::run(&mut session, work, Vec::new(), &opts, out)?;
    }
    Ok(if d.failed && exit == Exit::Success {
        Exit::Error
    } else {
        exit
    })
}
