//! The configurations under `examples/` stay valid and formatted, and
//! `ndac init --github-actions` writes the published workflow example.
#![allow(clippy::unwrap_used, reason = "tests")]

use std::{fs, path::Path, path::PathBuf};

use assert_cmd::Command;

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn ndac(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("ndac").unwrap();
    cmd.current_dir(dir)
        .env("NO_COLOR", "1")
        .env("XDG_CONFIG_HOME", dir)
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

#[test]
fn init_writes_the_github_workflow() {
    let dir = tempfile::tempdir().unwrap();
    ndac(dir.path()).arg("init").assert().success();
    assert!(!dir.path().join(".github").exists());
    ndac(dir.path()).args(["fmt", "--check"]).assert().success();

    ndac(dir.path())
        .args(["init", "--github-actions"])
        .assert()
        .success();
    let written =
        fs::read_to_string(dir.path().join(".github/workflows/nextdns-gitops.yml")).unwrap();
    let published =
        fs::read_to_string(examples().join("github-actions/nextdns-gitops.yml")).unwrap();
    assert_eq!(written, published);
}
