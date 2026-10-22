//! `ndac auth check|sources|set`.

use std::{
    io::{self, BufRead, IsTerminal},
    path::{Path, PathBuf},
};

use miette::Report;
use ndac_core::{
    auth::{self, Probe, SystemEnv},
    client::api_client,
    config::{LOCAL_FILE, Workspace},
};

use super::{CmdResult, Failure, cwd, load, root_path, runtime, user_agent};
use crate::{
    cli::{Backend, GlobalArgs},
    output::{Exit, Output},
};

fn write(r: io::Result<()>) -> Result<(), Failure> {
    r.map_err(|e| Failure::Other(Report::msg(format!("cannot write output: {e}"))))
}

fn check_profile(ws: &Workspace, profile: Option<&str>) -> Result<(), Failure> {
    if let Some(p) = profile {
        ws.resolve(p)?;
    }
    Ok(())
}

pub(crate) fn check(global: &GlobalArgs, profile: Option<&str>) -> CmdResult {
    let ws = load(global.config.as_deref())?;
    check_profile(&ws, profile)?;
    let chain = auth::chain_for(&ws, profile, &global.auth_source)?;
    let ctx = auth::context(
        &ws,
        profile,
        &cwd()?,
        global.allow_tracked_secret,
        &SystemEnv,
    );
    let resolved = auth::resolve(&chain, &ctx, &SystemEnv)?;
    for w in &resolved.warnings {
        Output::warn(w);
    }
    write(Output::line(&format!("✓ source: {}", resolved.source)))?;

    let client = api_client(&ws.provider(), resolved.key, &user_agent()).map_err(Failure::api)?;
    let profiles = runtime()?
        .block_on(client.list_profiles())
        .map_err(Failure::api)?;
    let listed: Vec<String> = profiles
        .iter()
        .map(|p| match &p.name {
            Some(n) => format!("{} {n:?}", p.id),
            None => p.id.clone(),
        })
        .collect();
    write(Output::line(&format!(
        "✓ valid: {} profile(s) reachable{}",
        profiles.len(),
        if listed.is_empty() {
            String::new()
        } else {
            format!(" ({})", listed.join(", "))
        }
    )))?;
    Ok(Exit::Success)
}

pub(crate) fn sources(global: &GlobalArgs, profile: Option<&str>) -> CmdResult {
    let ws = load(global.config.as_deref())?;
    check_profile(&ws, profile)?;
    let origin = chain_origin(&ws, profile, global);
    let chain = auth::chain_for(&ws, profile, &global.auth_source)?;
    let ctx = auth::context(
        &ws,
        profile,
        &cwd()?,
        global.allow_tracked_secret,
        &SystemEnv,
    );

    write(Output::line(&format!("chain from {origin}:")))?;
    // Some(true): a key was found; Some(false): a failure ends the chain.
    let mut outcome: Option<bool> = None;
    for source in &chain {
        if outcome.is_some() {
            write(Output::line(&format!("  - {source}  not tried")))?;
            continue;
        }
        let line = match auth::probe(source, &ctx, &SystemEnv) {
            Probe::Found(r) => {
                for w in &r.warnings {
                    Output::warn(w);
                }
                outcome = Some(true);
                format!("  ✓ {source}  key found (used)")
            }
            Probe::Missing(why) => format!("  · {source}  missing: {why}"),
            Probe::Failed(e) => {
                let skipped = e.is_skippable()
                    && ctx.on_source_error == ndac_core::config::types::OnSourceError::Skip;
                if !skipped {
                    outcome = Some(false);
                }
                format!("  ✗ {source}  failed: {e}")
            }
        };
        write(Output::line(&line))?;
    }
    match outcome {
        Some(true) => Ok(Exit::Success),
        Some(false) => Err(Failure::Auth(Report::msg("an auth source failed"))),
        None => Err(Failure::Auth(Report::msg(
            "no source in the chain produced a key",
        ))),
    }
}

fn chain_origin(ws: &Workspace, profile: Option<&str>, global: &GlobalArgs) -> String {
    if !global.auth_source.is_empty() {
        return "--auth-source / NDAC_AUTH_SOURCE".into();
    }
    if let Some(e) = profile.and_then(|p| ws.profiles.get(p))
        && e.file.auth.as_ref().is_some_and(|a| a.sources.is_some())
    {
        return e.src.display.clone();
    }
    if ws
        .local
        .as_ref()
        .is_some_and(|(_, l)| l.auth.sources.is_some())
    {
        return LOCAL_FILE.into();
    }
    if ws.root.auth.sources.is_some() {
        return ws.root_src.display.clone();
    }
    if let Some((src, _)) = ws.user.as_ref().filter(|(_, u)| u.auth.sources.is_some()) {
        return src.display.clone();
    }
    "the built-in default".into()
}

pub(crate) fn set(
    global: &GlobalArgs,
    backend: Backend,
    keyring_ref: &str,
    path: Option<&Path>,
    out: Output,
) -> CmdResult {
    let key = read_key()?;
    let stored = match backend {
        Backend::Keyring => {
            let Some((service, user)) = keyring_ref
                .split_once('/')
                .filter(|(s, u)| !s.is_empty() && !u.is_empty())
            else {
                return Err(Failure::Other(Report::msg(
                    "--keyring must be SERVICE/USER",
                )));
            };
            auth::store::keyring(service, user, &key)?;
            format!("keyring:{service}/{user}")
        }
        Backend::Config => {
            let root = root_path(global.config.as_deref())?;
            let dir = root
                .parent()
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
            let file = auth::store::local_config(&dir, &key)?;
            format!("config ({})", file.display())
        }
        Backend::File => {
            let Some(p) = path else {
                return Err(Failure::Other(Report::msg(
                    "--path is required for the file backend",
                )));
            };
            auth::store::file(p, &key)?;
            format!("file:{}", p.display())
        }
    };
    out.status(&format!("✓ stored in {stored}"));
    out.status("  reference it in [auth].sources, or rely on the default chain");
    Ok(Exit::Success)
}

/// Reads the key from a hidden prompt on a terminal, or from the first line
/// of stdin otherwise. Never from arguments.
fn read_key() -> Result<String, Failure> {
    let key = if io::stdin().is_terminal() {
        rpassword::prompt_password("NextDNS API key (input hidden): ")
            .map_err(|e| Failure::Other(Report::msg(format!("cannot read the key: {e}"))))?
    } else {
        let mut line = String::new();
        io::stdin().lock().read_line(&mut line).map_err(|e| {
            Failure::Other(Report::msg(format!("cannot read the key from stdin: {e}")))
        })?;
        line
    };
    let key = key.trim().to_owned();
    if key.is_empty() {
        return Err(Failure::Other(Report::msg("empty key")));
    }
    if !key.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Failure::Other(Report::msg(
            "the key contains spaces or non-printable characters",
        )));
    }
    auth::register_secret(&key);
    Ok(key)
}
