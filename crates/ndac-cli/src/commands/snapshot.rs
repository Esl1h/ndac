//! `ndac snapshot list|show|diff`.

use std::{collections::BTreeMap, fmt::Write as _};

use miette::Report;
use ndac_api::Profile;
use ndac_core::{
    config::DesiredProfile,
    plan::{self, Plan},
    snapshot::{Snapshot, SnapshotStore},
};

use super::{CmdResult, Failure, load};
use crate::output::{Exit, Output};

fn io(e: &std::io::Error) -> Failure {
    Failure::Other(Report::msg(e.to_string()))
}

fn store(config: Option<&std::path::Path>) -> Result<SnapshotStore, Failure> {
    let ws = load(config)?;
    Ok(SnapshotStore::new(&ws.root_dir, &ws.provider()))
}

fn write(text: &str) -> Result<(), Failure> {
    Output::artifact(text)
        .map_err(|e| Failure::Other(Report::msg(format!("cannot write output: {e}"))))
}

pub(crate) fn list(config: Option<&std::path::Path>, profile: Option<&str>) -> CmdResult {
    let store = store(config)?;
    let entries = store.list(profile).map_err(|e| io(&e))?;
    if entries.is_empty() {
        write("no snapshots\n")?;
        return Ok(Exit::Success);
    }
    let mut text = String::new();
    for e in &entries {
        let snap = store.load(e).map_err(|e| io(&e))?;
        let kind = serde_json::to_value(snap.kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let _ = writeln!(
            text,
            "{}  {}  {:<12} {}  {}",
            e.id, snap.taken_at, kind, e.profile, snap.id
        );
    }
    write(&text)?;
    Ok(Exit::Success)
}

fn find(store: &SnapshotStore, id: &str, profile: Option<&str>) -> Result<Snapshot, Failure> {
    let entry = store.find(id, profile).map_err(|e| io(&e))?;
    store.load(&entry).map_err(|e| io(&e))
}

pub(crate) fn show(config: Option<&std::path::Path>, id: &str, profile: Option<&str>) -> CmdResult {
    let store = store(config)?;
    let snap = find(&store, id, profile)?;
    let mut text = serde_json::to_string_pretty(&snap).unwrap_or_default();
    text.push('\n');
    write(&text)?;
    Ok(Exit::Success)
}

/// Shows what changed from snapshot `a` to snapshot `b`, as a plan that
/// would turn `a` into `b`.
pub(crate) fn diff(
    config: Option<&std::path::Path>,
    a: &str,
    b: &str,
    profile: Option<&str>,
    out: Output,
) -> CmdResult {
    let store = store(config)?;
    let (from, to) = (find(&store, a, profile)?, find(&store, b, profile)?);
    let parse = |s: &Snapshot| -> Result<Profile, Failure> {
        serde_json::from_value(s.data.clone())
            .map_err(|e| Failure::Other(Report::msg(format!("snapshot is not a profile: {e}"))))
    };
    let target = DesiredProfile {
        key: to.profile.clone(),
        id: Some(to.id.clone()),
        list_modes: BTreeMap::new(),
        ignore: Vec::new(),
        auth: None,
        desired: parse(&to)?,
        source: std::path::PathBuf::new(),
    };
    let p = plan::diff(&target, Some(&parse(&from)?));
    let plan = Plan {
        profiles: vec![p],
        removed: Vec::new(),
    };
    write(&plan::render_text(&plan, out.stdout_color))?;
    Ok(Exit::Success)
}
