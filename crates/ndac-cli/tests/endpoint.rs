//! `ndac endpoint render`, `doctor`, `completions` and `man`.
#![allow(clippy::unwrap_used, reason = "tests")]

use std::{fs, path::Path, process::Output};

use assert_cmd::Command;
use ndac_testkit::{FakeResolverTest, TEST_KEY};
use serde_json::json;

fn ndac(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("ndac").unwrap();
    cmd.current_dir(dir)
        .env("NO_COLOR", "1")
        .env("XDG_CONFIG_HOME", dir)
        // Set, to show that doctor never sends it.
        .env("NEXTDNS_API_KEY", TEST_KEY)
        .env_remove("NDAC_CONFIG")
        .env_remove("NDAC_DOCTOR_URL");
    cmd
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("ndac.toml"),
        "schema = 1\ninclude = [\"profiles/*.toml\"]\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("profiles")).unwrap();
    fs::write(
        dir.path().join("profiles/casa.toml"),
        "id = \"abc123\"\n[endpoint]\ncacheSize = \"10MB\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("profiles/novo.toml"), "name = \"Novo\"\n").unwrap();
    dir
}

#[test]
fn endpoint_render_targets() {
    let dir = workspace();
    let o = ndac(dir.path())
        .args(["endpoint", "render", "casa", "--target", "nextdns-cli"])
        .output()
        .unwrap();
    assert!(o.status.success());
    assert!(
        out(&o).ends_with("profile abc123\ncache-size 10MB\n"),
        "{}",
        out(&o)
    );

    ndac(dir.path())
        .args(["endpoint", "render", "casa", "--target", "doh-url"])
        .args(["--device", "Pixel 8"])
        .assert()
        .success()
        .stdout("https://dns.nextdns.io/abc123/Pixel%208\n");
    ndac(dir.path())
        .args(["endpoint", "render", "casa", "--target", "android"])
        .assert()
        .success()
        .stdout("abc123.dns.nextdns.io\n");

    let o = ndac(dir.path())
        .args(["endpoint", "render", "novo", "--target", "resolved"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("no `id` yet"));
}

#[test]
fn templates_cannot_set_endpoint() {
    let dir = workspace();
    fs::write(
        dir.path().join("ndac.toml"),
        "schema = 1\ninclude = [\"templates/*.toml\", \"profiles/*.toml\"]\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("templates")).unwrap();
    fs::write(
        dir.path().join("templates/base.toml"),
        "[endpoint]\nreportClientInfo = true\n",
    )
    .unwrap();
    let o = ndac(dir.path()).arg("validate").output().unwrap();
    assert_eq!(o.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&o.stderr).contains("`endpoint` is not allowed in a template"));
}

#[test]
fn completions_and_man() {
    let dir = tempfile::tempdir().unwrap();
    for shell in ["bash", "zsh", "fish", "elvish", "powershell"] {
        let o = ndac(dir.path())
            .args(["completions", shell])
            .output()
            .unwrap();
        assert!(o.status.success(), "{shell}");
        assert!(out(&o).contains("ndac"), "{shell}");
    }
    let o = ndac(dir.path()).arg("man").output().unwrap();
    assert!(o.status.success());
    assert!(out(&o).contains(".TH ndac 1"), "{}", out(&o));
}

async fn doctor(dir: &Path, url: &str, args: &[&str]) -> Output {
    let mut cmd = ndac(dir);
    cmd.arg("doctor").args(args).env("NDAC_DOCTOR_URL", url);
    tokio::task::spawn_blocking(move || cmd.output().unwrap())
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn doctor_checks_profile_and_device() {
    let dir = workspace();
    let fake = FakeResolverTest::start(json!({
        "status": "ok", "protocol": "DOT", "profile": "abc123",
        "server": "test-1", "clientName": "Laptop"
    }))
    .await
    .unwrap();

    let o = doctor(
        dir.path(),
        &fake.url(),
        &["--expect-profile", "casa", "--expect-device", "Laptop"],
    )
    .await;
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    assert!(out(&o).contains("✓ profile abc123"), "{}", out(&o));

    let o = doctor(dir.path(), &fake.url(), &["--expect-profile", "def456"]).await;
    assert_eq!(o.status.code(), Some(2));
    assert!(
        out(&o).contains("✗ profile abc123, expected def456"),
        "{}",
        out(&o)
    );

    let headers = fake.headers();
    assert_eq!(headers.len(), 2);
    assert!(headers.iter().all(|h| !h.contains_key("x-api-key")));
}

#[tokio::test(flavor = "multi_thread")]
async fn doctor_reports_unconfigured() {
    let dir = tempfile::tempdir().unwrap();
    let fake = FakeResolverTest::start(json!({ "status": "unconfigured" }))
        .await
        .unwrap();
    let o = doctor(dir.path(), &fake.url(), &[]).await;
    assert_eq!(o.status.code(), Some(2));
    assert!(
        out(&o).contains("✗ queries do not reach NextDNS"),
        "{}",
        out(&o)
    );
}
