//! `ndac validate`.

use std::path::Path;

use super::{CmdResult, load};
use crate::output::{Exit, Output};

pub(crate) fn run(config: Option<&Path>, out: Output) -> CmdResult {
    let ws = load(config)?;
    let profiles = ws.resolve_all()?;
    out.status(&format!(
        "✓ configuration is valid: {} profile(s), {} template(s)",
        profiles.len(),
        ws.templates.len()
    ));
    Ok(Exit::Success)
}
