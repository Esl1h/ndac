//! `ndac endpoint render`.

use std::path::Path;

use miette::Report;
use ndac_core::endpoint::{self, Target};

use super::{CmdResult, Failure, load};
use crate::{
    cli::EndpointTarget,
    output::{Exit, Output},
};

pub(crate) fn render(
    config: Option<&Path>,
    profile: &str,
    target: EndpointTarget,
    device: Option<&str>,
) -> CmdResult {
    let ws = load(config)?;
    let target = match target {
        EndpointTarget::NextdnsCli => Target::NextdnsCli,
        EndpointTarget::Resolved => Target::Resolved,
        EndpointTarget::Networkmanager => Target::NetworkManager,
        EndpointTarget::Android => Target::Android,
        EndpointTarget::DohUrl => Target::DohUrl,
    };
    let text = endpoint::render(&ws, profile, target, device)
        .map_err(|e| Failure::Other(Report::msg(e.to_string())))?;
    Output::artifact(&text)
        .map_err(|e| Failure::Other(Report::msg(format!("cannot write output: {e}"))))?;
    Ok(Exit::Success)
}
