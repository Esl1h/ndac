//! Helpers shared by integration tests.
#![allow(dead_code, clippy::unwrap_used, reason = "test helpers")]

use std::{fs, path::PathBuf};

use miette::{GraphicalReportHandler, GraphicalTheme};
use ndac_core::config::ConfigErrors;
use tempfile::TempDir;

/// Writes `files` (relative path, content) into a temp dir and returns it
/// with the path of `ndac.toml`.
pub(crate) fn workspace(files: &[(&str, &str)]) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    for (rel, text) in files {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
    }
    let root = dir.path().join("ndac.toml");
    (dir, root)
}

pub(crate) const ROOT: &str = "schema = 1\ninclude = [\"templates/*.toml\", \"profiles/*.toml\"]\n";

/// Renders diagnostics without color, as a user would see them.
pub(crate) fn render(errors: &ConfigErrors) -> String {
    let mut out = String::new();
    GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor())
        .with_width(100)
        .render_report(&mut out, errors)
        .unwrap();
    out
}
