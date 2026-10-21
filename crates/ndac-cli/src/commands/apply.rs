//! `ndac apply` and `ndac rollback`.

use miette::Report;
use ndac_api::Profile;
use ndac_core::{config::DesiredProfile, plan, snapshot::SnapshotKind};

use super::{
    CmdResult, Failure,
    engine::{self, Options, Work},
    load,
    plan::parse_targets,
    session::Session,
};
use crate::{cli::GlobalArgs, output::Output};

pub(crate) fn run(
    global: &GlobalArgs,
    targets: &[String],
    auto_approve: bool,
    allow_destroy: bool,
    out: Output,
) -> CmdResult {
    let targets = parse_targets(targets)?;
    let ws = load(global.config.as_deref())?;
    let mut desired = ws.resolve_all()?;
    if !targets.is_empty() {
        desired.retain(|d| targets.iter().any(|t| t.matches_profile(&d.key)));
    }
    let mut session = Session::new(ws, global)?;
    let live = session.fetch_live(&desired)?;
    let full = session.plan(&desired, &live)?.filtered(&targets);
    let work = desired
        .into_iter()
        .zip(live)
        .zip(full.profiles)
        .map(|((desired, live), plan)| Work {
            desired,
            plan,
            live,
        })
        .collect();
    let opts = Options {
        auto_approve,
        allow_destroy,
        write_back: true,
    };
    engine::run(&mut session, work, full.removed, &opts, out)
}

pub(crate) fn rollback(
    global: &GlobalArgs,
    snapshot: &str,
    profile: Option<&str>,
    auto_approve: bool,
    out: Output,
) -> CmdResult {
    let ws = load(global.config.as_deref())?;
    let mut session = Session::new(ws, global)?;
    let entry = session
        .store
        .find(snapshot, profile)
        .map_err(|e| Failure::Other(Report::msg(e.to_string())))?;
    let snap = session
        .store
        .load(&entry)
        .map_err(|e| Failure::Other(Report::msg(e.to_string())))?;
    if snap.kind == SnapshotKind::FailedApply {
        out.status("note: rolling back to the state recorded after a failed apply");
    }
    let target: Profile = serde_json::from_value(snap.data.clone())
        .map_err(|e| Failure::Other(Report::msg(format!("snapshot is not a profile: {e}"))))?;
    // The snapshot is the whole profile, so every list is authoritative.
    let desired = DesiredProfile {
        key: snap.profile.clone(),
        id: Some(snap.id.clone()),
        list_modes: std::collections::BTreeMap::new(),
        ignore: Vec::new(),
        auth: None,
        desired: target,
        source: std::path::PathBuf::new(),
    };
    let live = session.fetch_live(std::slice::from_ref(&desired))?;
    let live = live.into_iter().next().flatten();
    let p = plan::diff(&desired, live.as_ref());
    let opts = Options {
        auto_approve,
        allow_destroy: true,
        write_back: false,
    };
    out.status(&format!(
        "rolling back profile \"{}\" ({}) to snapshot {}",
        snap.profile, snap.id, entry.id
    ));
    engine::run(
        &mut session,
        vec![Work {
            desired,
            plan: p,
            live,
        }],
        Vec::new(),
        &opts,
        out,
    )
}
