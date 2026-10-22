//! End-to-end tests of the `ndac` binary.
#![allow(clippy::unwrap_used, reason = "tests")]

use std::{fs, path::Path};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

const ROOT: &str = "schema = 1\ninclude = [\"templates/*.toml\", \"profiles/*.toml\"]\n";

fn workspace(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (rel, text) in files {
        let p = dir.path().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }
    dir
}

fn ndac(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("ndac").unwrap();
    cmd.current_dir(dir)
        .env_remove("NDAC_CONFIG")
        .env_remove("NDAC_LOG")
        .env("NO_COLOR", "1")
        .env("XDG_CONFIG_HOME", dir);
    cmd
}

#[test]
fn help_and_version() {
    let dir = tempfile::tempdir().unwrap();
    ndac(dir.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("NextDNS-as-Code"))
        // Mentioned in the help text, but never offered as an option.
        .stdout(
            predicate::str::is_match(r"(?m)^\s+--api-key")
                .unwrap()
                .not(),
        );
    ndac(dir.path())
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("ndac "));
}

#[test]
fn usage_errors_exit_1_not_2() {
    let dir = tempfile::tempdir().unwrap();
    ndac(dir.path()).arg("nope").assert().code(1);
    ndac(dir.path())
        .args(["--api-key", "x", "validate"])
        .assert()
        .code(1);
}

#[test]
fn validate_ok_and_from_subdirectory() {
    let dir = workspace(&[
        ("ndac.toml", ROOT),
        (
            "templates/base.toml",
            "[privacy]\nblocklists = [\"oisd\"]\n",
        ),
        (
            "profiles/casa.toml",
            "id = \"abc123\"\nextends = [\"base\"]\n",
        ),
    ]);
    ndac(dir.path())
        .arg("validate")
        .assert()
        .success()
        .stderr(predicate::str::contains("1 profile(s), 1 template(s)"));
    ndac(&dir.path().join("profiles"))
        .arg("validate")
        .assert()
        .success();
}

#[test]
fn validate_reports_config_errors_with_exit_4() {
    let dir = workspace(&[
        ("ndac.toml", ROOT),
        ("profiles/p.toml", "[privacy]\nblocklist = [\"oisd\"]\n"),
    ]);
    ndac(dir.path())
        .arg("validate")
        .assert()
        .code(4)
        .stderr(predicate::str::contains("unknown key `privacy.blocklist`"))
        .stderr(predicate::str::contains("did you mean `blocklists`?"))
        .stderr(predicate::str::contains("profiles/p.toml"));
}

#[test]
fn missing_config_is_exit_4() {
    let dir = tempfile::tempdir().unwrap();
    ndac(dir.path())
        .arg("validate")
        .assert()
        .code(4)
        .stderr(predicate::str::contains("no ndac.toml found"));
}

#[test]
fn explicit_config_via_env() {
    let dir = workspace(&[("cfg/ndac.toml", "schema = 1\n")]);
    let other = tempfile::tempdir().unwrap();
    ndac(other.path())
        .env("NDAC_CONFIG", dir.path().join("cfg/ndac.toml"))
        .arg("validate")
        .assert()
        .success();
}

#[test]
fn render_json_and_toml() {
    let dir = workspace(&[
        ("ndac.toml", ROOT),
        (
            "profiles/casa.toml",
            "name = \"Casa\"\n[settings.logs]\nretention = \"30d\"\n[auth]\napi_key = \"test-key-0000000000000000000000000000000000\"\n",
        ),
    ]);
    let out = ndac(dir.path())
        .args(["render", "casa", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["format_version"], 1);
    assert_eq!(v["desired"]["settings"]["logs"]["retention"], 2_592_000);
    assert_eq!(v["auth"]["api_key"], "<redacted>");

    ndac(dir.path())
        .args(["render", "casa"])
        .assert()
        .success()
        .stdout(predicate::str::contains("retention = 2592000"))
        .stdout(predicate::str::contains("test-key").not());
}

#[test]
fn render_unknown_profile() {
    let dir = workspace(&[("ndac.toml", ROOT), ("profiles/casa.toml", "")]);
    ndac(dir.path())
        .args(["render", "cassa"])
        .assert()
        .code(4)
        .stderr(predicate::str::contains("did you mean `casa`?"));
}

#[test]
fn fmt_check_then_fix() {
    let dir = workspace(&[
        ("ndac.toml", ROOT),
        (
            "profiles/p.toml",
            "# keep me\nname = \"P\"\nid = \"abc\"\n[privacy]\nblocklists = [\"oisd\", \"a\"]\n",
        ),
    ]);
    ndac(dir.path())
        .args(["fmt", "--check"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("would reformat profiles/p.toml"));
    ndac(dir.path()).arg("fmt").assert().success();
    let text = fs::read_to_string(dir.path().join("profiles/p.toml")).unwrap();
    assert_eq!(
        text,
        "# keep me\nname = \"P\"\nid   = \"abc\"\n[privacy]\nblocklists = [\"a\", \"oisd\"]\n"
    );
    ndac(dir.path()).args(["fmt", "--check"]).assert().success();
}

#[cfg(unix)]
#[test]
fn fmt_keeps_file_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = workspace(&[("ndac.toml", "schema=1\n")]);
    let local = dir.path().join("ndac.local.toml");
    fs::write(&local, "[provider]\nretries=3\n").unwrap();
    fs::set_permissions(&local, fs::Permissions::from_mode(0o600)).unwrap();
    ndac(dir.path()).arg("fmt").assert().success();
    let mode = fs::metadata(&local).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    assert_eq!(
        fs::read_to_string(&local).unwrap(),
        "[provider]\nretries = 3\n"
    );
}

#[test]
fn fmt_reports_syntax_errors() {
    let dir = workspace(&[("ndac.toml", ROOT), ("profiles/p.toml", "[x\n")]);
    ndac(dir.path()).arg("fmt").assert().code(4);
}

#[test]
fn schema_is_json() {
    let dir = tempfile::tempdir().unwrap();
    for kind in ["root", "local", "profile"] {
        let out = ndac(dir.path())
            .args(["schema", "--kind", kind])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert!(v["properties"].is_object(), "{kind}");
    }
}

#[test]
fn color_comes_from_flag_then_repository_then_user_file() {
    const ESC: &str = "\u{1b}[";
    let dir = tempfile::tempdir().unwrap();
    let root = "schema = 1\ninclude = [\"profiles/*.toml\"]\n";
    fs::create_dir_all(dir.path().join("profiles")).unwrap();
    // A broken profile, so that validate prints a diagnostic.
    fs::write(
        dir.path().join("profiles/home.toml"),
        "extends = [\"nope\"]\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("ndac")).unwrap();
    let stderr = |args: &[&str]| {
        let out = ndac(dir.path()).args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(4));
        String::from_utf8_lossy(&out.stderr).into_owned()
    };

    fs::write(dir.path().join("ndac.toml"), root).unwrap();
    assert!(!stderr(&["validate"]).contains(ESC), "auto with NO_COLOR");

    fs::write(
        dir.path().join("ndac/config.toml"),
        "[output]\ncolor = \"always\"\n",
    )
    .unwrap();
    assert!(stderr(&["validate"]).contains(ESC), "user file");

    fs::write(
        dir.path().join("ndac.toml"),
        format!("{root}\n[output]\ncolor = \"never\"\n"),
    )
    .unwrap();
    assert!(!stderr(&["validate"]).contains(ESC), "repository wins");
    assert!(
        stderr(&["--color", "always", "validate"]).contains(ESC),
        "flag wins"
    );
}
