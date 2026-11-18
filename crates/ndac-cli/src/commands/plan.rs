//! `ndac plan`.

use miette::Report;
use ndac_core::{
    config::{DesiredProfile, Workspace},
    plan::{Target, render_json, render_markdown, render_text},
};

use super::{CmdResult, Failure, load, session::Session};
use crate::{
    cli::{GlobalArgs, PlanFormat},
    output::{Exit, Output},
};

pub(crate) fn parse_targets(raw: &[String]) -> Result<Vec<Target>, Failure> {
    raw.iter()
        .map(|t| {
            t.parse::<Target>()
                .map_err(|e| Failure::Other(Report::msg(e)))
        })
        .collect()
}

/// Resolves every profile and keeps those selected by `targets`. A target
/// that selects nothing is an error: otherwise a typo in a CI job reads as
/// "No changes".
pub(crate) fn select(ws: &Workspace, targets: &[Target]) -> Result<Vec<DesiredProfile>, Failure> {
    let mut desired = ws.resolve_all()?;
    if targets.is_empty() {
        return Ok(desired);
    }
    if let Some(t) = targets
        .iter()
        .find(|t| !desired.iter().any(|d| t.matches_profile(&d.key)))
    {
        return Err(Failure::Target(Report::msg(format!(
            "target `{t}` matches no profile"
        ))));
    }
    desired.retain(|d| targets.iter().any(|t| t.matches_profile(&d.key)));
    Ok(desired)
}

pub(crate) fn run(
    global: &GlobalArgs,
    targets: &[String],
    detailed_exitcode: bool,
    format: PlanFormat,
    out: Output,
) -> CmdResult {
    let targets = parse_targets(targets)?;
    let ws = load(global.config.as_deref())?;
    let desired = select(&ws, &targets)?;
    let mut session = Session::new(ws, global)?;
    let live = session.fetch_live(&desired)?;
    let plan = session.plan(&desired, &live)?.filtered(&targets);

    let text = match format {
        PlanFormat::Text => render_text(&plan, out.stdout_color),
        PlanFormat::Json => render_json(&plan),
        PlanFormat::Markdown => render_markdown(&plan),
    };
    Output::artifact(&text)
        .map_err(|e| Failure::Other(Report::msg(format!("cannot write output: {e}"))))?;
    Ok(if detailed_exitcode && !plan.is_empty() {
        Exit::Changes
    } else {
        Exit::Success
    })
}
