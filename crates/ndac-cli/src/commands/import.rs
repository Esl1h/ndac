//! `ndac import`.

use std::{collections::BTreeMap, fs, path::PathBuf};

use miette::Report;
use ndac_core::{
    apply::{profile_toml, slug},
    config::Workspace,
    snapshot::SnapshotKind,
};

use super::{CmdResult, Failure, load, session::Session};
use crate::{
    cli::GlobalArgs,
    output::{Exit, Output},
};

fn other(msg: impl Into<String>) -> Failure {
    Failure::Other(Report::msg(msg.into()))
}

/// Existing profile files by NextDNS ID.
fn files_by_id(ws: &Workspace) -> BTreeMap<String, PathBuf> {
    ws.profiles
        .values()
        .filter_map(|e| Some((e.file.id.clone()?, e.src.path.clone())))
        .collect()
}

pub(crate) fn run(
    global: &GlobalArgs,
    ids: &[String],
    all: bool,
    force: bool,
    out: Output,
) -> CmdResult {
    let ws = load(global.config.as_deref())?;
    let dir = ws.root_dir.join("profiles");
    let existing = files_by_id(&ws);
    let mut session = Session::new(ws, global)?;
    let client = session.root_client()?;

    let targets: Vec<String> = if all {
        session
            .rt
            .block_on(client.list_profiles())
            .map_err(Failure::api)?
            .into_iter()
            .map(|p| p.id)
            .collect()
    } else {
        ids.to_vec()
    };

    fs::create_dir_all(&dir).map_err(|e| other(format!("cannot create {}: {e}", dir.display())))?;
    let mut written = 0;
    let mut skipped = 0;
    for id in targets {
        let live = session
            .rt
            .block_on(client.get_profile(&id))
            .map_err(Failure::api)?;
        let path = if let Some(p) = existing.get(&id) {
            p.clone()
        } else {
            let base = live
                .name
                .as_deref()
                .map(slug)
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| id.clone());
            let candidate = dir.join(format!("{base}.toml"));
            if candidate.exists() {
                dir.join(format!("{base}-{id}.toml"))
            } else {
                candidate
            }
        };
        let rel = path
            .strip_prefix(&session.ws.root_dir)
            .unwrap_or(&path)
            .display()
            .to_string();
        if path.exists() && !force {
            out.status(&format!(
                "- {id}: {rel} already exists (use --force to overwrite)"
            ));
            skipped += 1;
            continue;
        }
        fs::write(&path, profile_toml(&id, &live))
            .map_err(|e| other(format!("cannot write {rel}: {e}")))?;
        let key = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        // The file now matches the live profile: record it as the drift
        // baseline.
        session
            .store
            .save(
                &key,
                &id,
                SnapshotKind::PostApply,
                &serde_json::to_value(&live).unwrap_or_default(),
            )
            .map_err(|e| other(format!("cannot write the snapshot: {e}")))?;
        let name = live
            .name
            .as_deref()
            .map(|n| format!(" {n:?}"))
            .unwrap_or_default();
        out.status(&format!("✓ imported {id}{name} → {rel}"));
        written += 1;
    }

    // Warn when the root file would not pick up what was written.
    if written > 0 {
        let reloaded = Workspace::load(&session.ws.root_src.path);
        if reloaded.is_ok_and(|ws| ws.profiles.is_empty()) {
            Output::warn(
                "profiles/ is not matched by `include` in ndac.toml; add \"profiles/*.toml\"",
            );
        }
    }
    out.status(&format!("{written} imported, {skipped} skipped"));
    Ok(Exit::Success)
}
