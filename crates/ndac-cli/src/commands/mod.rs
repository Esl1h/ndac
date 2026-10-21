//! Command implementations.

use std::{
    env,
    path::{Path, PathBuf},
};

use miette::Report;
use ndac_core::config::{ConfigErrors, Workspace, discover};

use crate::output::Exit;

pub(crate) mod fmt;
pub(crate) mod render;
pub(crate) mod schema;
pub(crate) mod validate;

/// Why a command failed, and the exit code to use.
#[derive(Debug)]
pub(crate) enum Failure {
    Config(ConfigErrors),
    Other(Report),
}

impl Failure {
    pub(crate) fn exit(&self) -> Exit {
        match self {
            Self::Config(_) => Exit::Config,
            Self::Other(_) => Exit::Error,
        }
    }

    pub(crate) fn report(self) -> Report {
        match self {
            Self::Config(e) => Report::new(e),
            Self::Other(r) => r,
        }
    }
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
    let cwd = env::current_dir().map_err(|e| {
        Failure::Other(Report::msg(format!(
            "cannot read the current directory: {e}"
        )))
    })?;
    discover(explicit, &cwd).map_err(|e| Failure::Config(e.into()))
}

pub(crate) fn load(explicit: Option<&Path>) -> Result<Workspace, Failure> {
    let root = root_path(explicit)?;
    Ok(Workspace::load(&root)?)
}
