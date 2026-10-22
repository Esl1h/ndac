//! `ndac schema`.

use miette::Report;
use ndac_core::config::{SchemaKind, json_schema_text};

use super::{CmdResult, Failure};
use crate::{
    cli::SchemaArg,
    output::{Exit, Output},
};

pub(crate) fn run(kind: SchemaArg) -> CmdResult {
    let kind = match kind {
        SchemaArg::Root => SchemaKind::Root,
        SchemaArg::Local => SchemaKind::Local,
        SchemaArg::Profile => SchemaKind::Profile,
        SchemaArg::User => SchemaKind::User,
    };
    Output::artifact(&json_schema_text(kind))
        .map_err(|e| Failure::Other(Report::msg(format!("cannot write output: {e}"))))?;
    Ok(Exit::Success)
}
