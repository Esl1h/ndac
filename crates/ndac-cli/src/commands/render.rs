//! `ndac render`.

use std::path::Path;

use miette::Report;

use super::{CmdResult, Failure, load};
use crate::{
    cli::RenderFormat,
    output::{Exit, Output},
};

pub(crate) fn run(config: Option<&Path>, profile: &str, format: RenderFormat) -> CmdResult {
    let ws = load(config)?;
    let p = ws.resolve(profile)?;
    let text = match format {
        RenderFormat::Toml => p.to_toml(),
        RenderFormat::Json => p.to_json(),
    };
    Output::artifact(&text)
        .map_err(|e| Failure::Other(Report::msg(format!("cannot write output: {e}"))))?;
    Ok(Exit::Success)
}
