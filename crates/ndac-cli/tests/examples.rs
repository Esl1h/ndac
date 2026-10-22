//! The configurations under `examples/` stay valid and formatted.
#![allow(clippy::unwrap_used, reason = "tests")]

use std::path::{Path, PathBuf};

use assert_cmd::Command;

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn ndac(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("ndac").unwrap();
    cmd.current_dir(dir)
        .env("NO_COLOR", "1")
        .env_remove("NDAC_CONFIG");
    cmd
}

#[test]
fn examples_are_valid_and_formatted() {
    for name in ["minimal", "multi-profile", "paranoid"] {
        let dir = examples().join(name);
        ndac(&dir).arg("validate").assert().success();
        ndac(&dir).args(["fmt", "--check"]).assert().success();
    }
}
