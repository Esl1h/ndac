//! `ndac completions` and `ndac man`.

use clap::CommandFactory;
use clap_complete::Shell;
use miette::Report;

use super::{CmdResult, Failure};
use crate::{
    cli::Cli,
    output::{Exit, Output},
};

/// Writes generated text through [`Output::artifact`], which tolerates a
/// closed pipe (`ndac man | head`).
fn emit(buf: &[u8]) -> CmdResult {
    Output::artifact(&String::from_utf8_lossy(buf))
        .map_err(|e| Failure::Other(Report::msg(format!("cannot write output: {e}"))))?;
    Ok(Exit::Success)
}

pub(crate) fn completions(shell: Shell) -> CmdResult {
    let mut buf = Vec::new();
    clap_complete::generate(shell, &mut Cli::command(), "ndac", &mut buf);
    emit(&buf)
}

pub(crate) fn man() -> CmdResult {
    let mut buf = Vec::new();
    clap_mangen::Man::new(Cli::command())
        .render(&mut buf)
        .map_err(|e| Failure::Other(Report::msg(format!("cannot render the man page: {e}"))))?;
    emit(&buf)
}
