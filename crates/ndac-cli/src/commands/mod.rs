//! Command implementations.

use std::{
    env,
    path::{Path, PathBuf},
};

use miette::Report;
use ndac_core::config::{ConfigErrors, Workspace, discover};

use crate::output::Exit;

pub(crate) mod apply;
pub(crate) mod auth;
pub(crate) mod engine;
pub(crate) mod fmt;
pub(crate) mod import;
pub(crate) mod init;
pub(crate) mod plan;
pub(crate) mod render;
pub(crate) mod schema;
pub(crate) mod session;
pub(crate) mod snapshot;
pub(crate) mod validate;

/// Why a command failed, and the exit code to use.
#[derive(Debug)]
pub(crate) enum Failure {
    Config(ConfigErrors),
    Auth(Report),
    Api(Report),
    Other(Report),
}

impl Failure {
    pub(crate) fn exit(&self) -> Exit {
        match self {
            Self::Config(_) => Exit::Config,
            Self::Auth(_) => Exit::Auth,
            Self::Api(_) => Exit::Api,
            Self::Other(_) => Exit::Error,
        }
    }

    pub(crate) fn report(self) -> Report {
        match self {
            Self::Config(e) => Report::new(e),
            Self::Auth(r) | Self::Api(r) | Self::Other(r) => r,
        }
    }

    /// Maps an API error: rejected keys are auth failures (exit 3), the
    /// rest API failures (exit 5).
    #[allow(
        clippy::needless_pass_by_value,
        reason = "used as map_err(Failure::api)"
    )]
    pub(crate) fn api(e: ndac_api::Error) -> Self {
        if e.is_auth() {
            Self::Auth(Report::msg(e.to_string()))
        } else {
            Self::Api(Report::msg(e.to_string()))
        }
    }
}

impl From<ndac_core::auth::AuthError> for Failure {
    fn from(e: ndac_core::auth::AuthError) -> Self {
        Self::Auth(Report::new(e))
    }
}

/// `User-Agent` sent to the API.
pub(crate) fn user_agent() -> String {
    format!(
        "ndac/{} (+https://github.com/Esl1h/ndac)",
        env!("CARGO_PKG_VERSION")
    )
}

/// A single-threaded runtime: commands make few, mostly sequential calls.
pub(crate) fn runtime() -> Result<tokio::runtime::Runtime, Failure> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| Failure::Other(Report::msg(format!("cannot start the async runtime: {e}"))))
}

pub(crate) fn cwd() -> Result<PathBuf, Failure> {
    env::current_dir().map_err(|e| {
        Failure::Other(Report::msg(format!(
            "cannot read the current directory: {e}"
        )))
    })
}

impl From<ConfigErrors> for Failure {
    fn from(e: ConfigErrors) -> Self {
        Self::Config(e)
    }
}

pub(crate) type CmdResult = Result<Exit, Failure>;

/// Locates `ndac.toml` from `--config`/`NDAC_CONFIG` or the working
/// directory.
pub(crate) fn root_path(explicit: Option<&Path>) -> Result<PathBuf, Failure> {
    discover(explicit, &cwd()?).map_err(|e| Failure::Config(e.into()))
}

pub(crate) fn load(explicit: Option<&Path>) -> Result<Workspace, Failure> {
    let root = root_path(explicit)?;
    Ok(Workspace::load(&root)?)
}
