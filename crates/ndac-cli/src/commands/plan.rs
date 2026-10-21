//! `ndac plan`.

use miette::Report;
use ndac_core::plan::{Target, render_json, render_markdown, render_text};

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

pub(crate) fn run(
    global: &GlobalArgs,
    targets: &[String],
    detailed_exitcode: bool,
    format: PlanFormat,
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
