//! `ndac drift`.

use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use miette::Report;
use ndac_core::{
    config::Workspace,
    plan::{drift_only, render_github, render_json, render_prom, render_text},
};

use super::{CmdResult, Failure, cwd, load, plan::parse_targets, session::Session};
use crate::{
    cli::{DriftFormat, GlobalArgs},
    output::{Exit, Output},
};

/// File name inside `--textfile-dir`; `node_exporter` reads `*.prom`.
const PROM_FILE: &str = "ndac_drift.prom";

fn other(msg: impl Into<String>) -> Failure {
    Failure::Other(Report::msg(msg.into()))
}

/// The file that sets `section` for `profile` (the profile, else the last
/// template that does), relative to the working directory so annotations
/// land on the right file in a checkout.
fn locate(ws: &Workspace, base: &Path, profile: &str, section: &str) -> (String, Option<usize>) {
    let layers = ws.layer_entries(profile).unwrap_or_default();
    let rel = |p: &Path| p.strip_prefix(base).unwrap_or(p).display().to_string();
    layers
        .iter()
        .rev()
        .find_map(|e| e.src.line_of(section).map(|l| (rel(&e.src.path), Some(l))))
        .or_else(|| layers.last().map(|e| (rel(&e.src.path), None)))
        .unwrap_or_default()
}

/// Writes `text` to `dir/ndac_drift.prom` via a temporary file and a
/// rename, so the collector never reads a partial file.
fn write_textfile(dir: &Path, text: &str) -> Result<(), Failure> {
    let path = dir.join(PROM_FILE);
    let tmp = dir.join(format!(".{PROM_FILE}.{}.tmp", std::process::id()));
    fs::write(&tmp, text)
        .and_then(|()| fs::rename(&tmp, &path))
        .map_err(|e| {
            let _ = fs::remove_file(&tmp);
            other(format!("cannot write {}: {e}", path.display()))
        })
}

pub(crate) fn run(
    global: &GlobalArgs,
    targets: &[String],
    format: DriftFormat,
    textfile_dir: Option<&Path>,
    out: Output,
) -> CmdResult {
    if textfile_dir.is_some() && format != DriftFormat::Prom {
        return Err(other("--textfile-dir requires --format prom"));
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
    let found = drift.profiles.iter().any(|p| !p.changes.is_empty());

    let text = match format {
        DriftFormat::Text if !found => "No drift since the last apply.\n".to_owned(),
        DriftFormat::Text => render_text(&drift, out.stdout_color),
        DriftFormat::Json => render_json(&drift),
        DriftFormat::Github => {
            let base = cwd()?;
            render_github(&drift, |p, s| locate(&session.ws, &base, p, s))
        }
        DriftFormat::Prom => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            render_prom(&drift, now)
        }
    };
    if let Some(dir) = textfile_dir {
        write_textfile(dir, &text)?;
        out.status(&format!("wrote {}", dir.join(PROM_FILE).display()));
    } else {
        Output::artifact(&text).map_err(|e| other(format!("cannot write output: {e}")))?;
    }
    Ok(if found { Exit::Changes } else { Exit::Success })
}
