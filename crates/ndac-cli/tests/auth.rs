//! `ndac auth` against the fake API, and the literal-key guardrails.
#![allow(clippy::unwrap_used, reason = "tests")]

use std::{fs, path::Path, process::Output};

use assert_cmd::Command;
use ndac_testkit::{FakeApi, TEST_KEY};
use predicates::prelude::*;
use serde_json::json;
use tempfile::TempDir;

const WRONG_KEY: &str = "test-key-1111111111111111111111111111111111";

fn workspace(api: &FakeApi, extra_root: &str, files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = format!(
        "schema = 1\ninclude = [\"profiles/*.toml\"]\n{extra_root}\n[provider]\napi_url = {:?}\nretries = 0\n",
        api.base_url()
    );
    fs::write(dir.path().join("ndac.toml"), root).unwrap();
    for (rel, text) in files {
        let p = dir.path().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, text).unwrap();
    }
    dir
}

fn ndac(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("ndac").unwrap();
    cmd.current_dir(dir)
        .env("NO_COLOR", "1")
        .env("HOME", dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    for k in [
        "NDAC_CONFIG",
        "NDAC_AUTH_SOURCE",
        "NDAC_LOG",
        "NEXTDNS_API_KEY",
        "NEXTDNS_API_KEY_FILE",
        "CREDENTIALS_DIRECTORY",
        "DBUS_SESSION_BUS_ADDRESS",
        "NDAC_ALLOW_INSECURE_PERMISSIONS",
    ] {
        cmd.env_remove(k);
    }
    cmd
}

/// Runs the blocking CLI while the async fake keeps serving.
async fn run(mut cmd: Command) -> Output {
    tokio::task::spawn_blocking(move || cmd.output().unwrap())
        .await
        .unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn check_reports_source_and_profiles_without_the_key() {
    let api = FakeApi::start().await.unwrap();
    let _ = api.insert_profile(&json!({ "name": "Casa" }));
    let dir = workspace(&api, "", &[]);
    let mut cmd = ndac(dir.path());
    cmd.env("NEXTDNS_API_KEY", TEST_KEY).args(["auth", "check"]);
    let out = run(cmd).await;
    let (stdout, stderr) = (text(&out.stdout), text(&out.stderr));
    assert!(out.status.success(), "{stderr}");
    assert!(stdout.contains("✓ source: env:NEXTDNS_API_KEY"), "{stdout}");
    assert!(
        stdout.contains("✓ valid: 1 profile(s) reachable (a00001 \"Casa\")"),
        "{stdout}"
    );
    assert!(!stdout.contains(TEST_KEY) && !stderr.contains(TEST_KEY));
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_key_exits_3() {
    let api = FakeApi::start().await.unwrap();
    let dir = workspace(&api, "", &[]);
    let mut cmd = ndac(dir.path());
    cmd.env("NEXTDNS_API_KEY", WRONG_KEY)
        .args(["-vv", "auth", "check"]);
    let out = run(cmd).await;
    assert_eq!(out.status.code(), Some(3));
    let all = text(&out.stdout) + &text(&out.stderr);
    assert!(all.contains("API key rejected"), "{all}");
    assert!(!all.contains(WRONG_KEY), "{all}");
}

#[test]
fn no_key_exits_3_with_the_tried_sources() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("ndac.toml"), "schema = 1\n").unwrap();
    ndac(dir.path())
        .args(["--auth-source", "env:NDAC_TEST_NOPE", "auth", "check"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("no NextDNS API key found"))
        .stderr(predicate::str::contains("NDAC_TEST_NOPE is not set"));
}

#[test]
fn a_key_passed_as_source_is_rejected_and_not_echoed() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("ndac.toml"), "schema = 1\n").unwrap();
    ndac(dir.path())
        .args(["--auth-source", TEST_KEY, "auth", "check"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("not a source reference"))
        .stderr(predicate::str::contains(TEST_KEY).not());
}

#[tokio::test(flavor = "multi_thread")]
async fn sources_lists_the_chain() {
    let api = FakeApi::start().await.unwrap();
    let dir = workspace(
        &api,
        "[auth]\nsources = [\"env:NDAC_TEST_A\", \"env:NDAC_TEST_B\", \"config\"]",
        &[],
    );
    let mut cmd = ndac(dir.path());
    cmd.env("NDAC_TEST_B", TEST_KEY).args(["auth", "sources"]);
    let out = run(cmd).await;
    let stdout = text(&out.stdout);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(stdout.contains("chain from ndac.toml:"), "{stdout}");
    assert!(stdout.contains("· env:NDAC_TEST_A  missing: NDAC_TEST_A is not set"));
    assert!(stdout.contains("✓ env:NDAC_TEST_B  key found (used)"));
    assert!(stdout.contains("- config  not tried"));
}

#[tokio::test(flavor = "multi_thread")]
async fn profile_auth_overrides_root() {
    let api = FakeApi::start().await.unwrap();
    let dir = workspace(
        &api,
        "[auth]\nsources = [\"env:NDAC_TEST_ROOT\"]",
        &[(
            "profiles/work.toml",
            "[auth]\nsources = [\"env:NDAC_TEST_WORK\"]\n",
        )],
    );
    let mut cmd = ndac(dir.path());
    cmd.env("NDAC_TEST_WORK", TEST_KEY)
        .args(["auth", "check", "--profile", "work"]);
    let out = run(cmd).await;
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("source: env:NDAC_TEST_WORK"));
}

#[tokio::test(flavor = "multi_thread")]
async fn set_config_then_use_it() {
    let api = FakeApi::start().await.unwrap();
    let dir = workspace(&api, "", &[]);
    ndac(dir.path())
        .args(["auth", "set", "--backend", "config"])
        .write_stdin(format!("{TEST_KEY}\n"))
        .assert()
        .success()
        .stderr(predicate::str::contains("stored in config"));
    let local = dir.path().join("ndac.local.toml");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&local).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(
        fs::read_to_string(dir.path().join(".gitignore"))
            .unwrap()
            .contains("ndac.local.toml")
    );

    let mut cmd = ndac(dir.path());
    cmd.args(["--auth-source", "config", "auth", "check"]);
    let out = run(cmd).await;
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains("literal in ndac.local.toml"));
}

#[test]
fn set_rejects_garbage_and_never_takes_argv() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("ndac.toml"), "schema = 1\n").unwrap();
    ndac(dir.path())
        .args(["auth", "set", "--backend", "config"])
        .write_stdin("has space\n")
        .assert()
        .code(1);
    ndac(dir.path())
        .args(["auth", "set", "--backend", "config", TEST_KEY])
        .assert()
        .code(1);
    assert!(!dir.path().join("ndac.local.toml").exists());
}

#[cfg(unix)]
#[test]
fn literal_key_guardrails() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ndac.toml");
    fs::write(
        &root,
        format!("schema = 1\n[auth]\napi_key = {TEST_KEY:?}\n"),
    )
    .unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o644)).unwrap();
    ndac(dir.path())
        .args(["--auth-source", "config", "auth", "sources"])
        .assert()
        .code(3)
        .stdout(predicate::str::contains("readable by other users"))
        .stdout(predicate::str::contains("chmod 600").or(predicate::str::contains("mode 644")));

    fs::set_permissions(&root, fs::Permissions::from_mode(0o600)).unwrap();
    let git = |args: &[&str]| {
        #[allow(clippy::disallowed_methods, reason = "test setup")]
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .status()
            .unwrap()
            .success();
        assert!(ok);
    };
    git(&["init", "-q"]);
    git(&["add", "ndac.toml"]);
    ndac(dir.path())
        .args(["--auth-source", "config", "auth", "sources"])
        .assert()
        .code(3)
        .stdout(predicate::str::contains("is tracked by git"));
    ndac(dir.path())
        .args([
            "--auth-source",
            "config",
            "--allow-tracked-secret",
            "auth",
            "sources",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("WARNING:"))
        .stdout(predicate::str::contains(TEST_KEY).not());
}
