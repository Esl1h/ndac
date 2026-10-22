//! `ndac endpoint render`.

use miette::Report;
use ndac_core::endpoint::{self, Target};

use super::{CmdResult, Failure, load};
use crate::{
    cli::{EndpointCommand, EndpointTarget, GlobalArgs},
    output::{Exit, Output},
};

pub(crate) fn run(global: &GlobalArgs, cmd: EndpointCommand) -> CmdResult {
    let EndpointCommand::Render {
        profile,
        target,
        device,
        #[cfg(feature = "unstable-endpoints")]
        api_ipv6,
    } = cmd;
    let ws = load(global.config.as_deref())?;
    let target = match target {
        EndpointTarget::NextdnsCli => Target::NextdnsCli,
        EndpointTarget::Resolved => Target::Resolved,
        EndpointTarget::Networkmanager => Target::NetworkManager,
        EndpointTarget::Android => Target::Android,
        EndpointTarget::DohUrl => Target::DohUrl,
    };
    #[cfg(feature = "unstable-endpoints")]
    let (ws, ipv6) = if api_ipv6 {
        api::ipv6(ws, global, &profile)?
    } else {
        (ws, Vec::new())
    };
    #[cfg(not(feature = "unstable-endpoints"))]
    let ipv6 = Vec::new();
    let text = endpoint::render_with_ipv6(&ws, &profile, target, device.as_deref(), &ipv6)
        .map_err(|e| Failure::Other(Report::msg(e.to_string())))?;
    Output::artifact(&text)
        .map_err(|e| Failure::Other(Report::msg(format!("cannot write output: {e}"))))?;
    Ok(Exit::Success)
}

#[cfg(feature = "unstable-endpoints")]
mod api {
    use std::net::Ipv6Addr;

    use miette::Report;
    use ndac_core::config::Workspace;

    use super::super::{Failure, session::Session};
    use crate::cli::GlobalArgs;

    /// The profile's IPv6 servers from the setup endpoint. Each one is
    /// parsed, so nothing but an address reaches the generated text.
    pub(super) fn ipv6(
        ws: Workspace,
        global: &GlobalArgs,
        key: &str,
    ) -> Result<(Workspace, Vec<Ipv6Addr>), Failure> {
        let id = ws
            .profiles
            .get(key)
            .and_then(|e| e.file.id.clone())
            .filter(|i| !i.is_empty())
            .ok_or_else(|| {
                Failure::Other(Report::msg(format!(
                    "profile \"{key}\" has no `id` yet; apply it first"
                )))
            })?;
        let mut session = Session::new(ws, global)?;
        let client = session.client(key)?;
        let setup = session
            .rt
            .block_on(client.setup(&id))
            .map_err(Failure::api)?;
        let addrs = setup
            .ipv6
            .iter()
            .map(|a| {
                a.parse::<Ipv6Addr>().map_err(|_| {
                    Failure::Api(Report::msg(format!(
                        "the setup endpoint returned {a:?}, which is not an IPv6 address"
                    )))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((session.ws, addrs))
    }
}
