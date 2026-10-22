//! `ndac drift` and `ndac reconcile` against the fake API, including the
//! golden invariant: reconcile --prefer live then plan is empty.
#![allow(clippy::unwrap_used, reason = "tests")]

use std::{fs, path::Path, process::Output};

use assert_cmd::Command;
use ndac_testkit::{FakeApi, TEST_KEY};
use serde_json::json;
use tempfile::TempDir;

fn ndac(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("ndac").unwrap();
    cmd.current_dir(dir)
        .env("NO_COLOR", "1")
        .env("XDG_CONFIG_HOME", dir)
        .env("NEXTDNS_API_KEY", TEST_KEY)
        .env("NDAC_AUTH_SOURCE", "env:NEXTDNS_API_KEY")
        .env_remove("NDAC_CONFIG")
        .env_remove("NDAC_LOG");
    cmd
}

async fn run(dir: &Path, args: &[&str]) -> Output {
    let mut cmd = ndac(dir);
    cmd.args(args);
    tokio::task::spawn_blocking(move || cmd.output().unwrap())
        .await
        .unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// An imported profile `casa`, then a dashboard edit: one denylist item
/// added and `web3` turned on.
async fn drifted() -> (FakeApi, TempDir, String) {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({
        "name": "Casa",
        "denylist": [{ "id": "foo.example", "active": true }],
        "settings": { "web3": false }
    }));
    let dir = tempfile::tempdir().unwrap();
    ndac(dir.path()).arg("init").assert().success();
    let root = dir.path().join("ndac.toml");
    let text = fs::read_to_string(&root)
        .unwrap()
        .replace(
            "[provider]\n",
            &format!("[provider]\napi_url = {:?}\n", api.base_url()),
        )
        .replace("retries       = 5", "retries       = 0");
    fs::write(&root, text).unwrap();
    let o = run(dir.path(), &["import", "--all"]).await;
    assert!(o.status.success(), "{}", err(&o));

    let o = run(dir.path(), &["drift"]).await;
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(out(&o).contains("No drift"), "{}", out(&o));

    assert!(api.edit_profile(&id, |p| {
        p["denylist"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "id": "bar.example", "active": true }));
        p["settings"]["web3"] = json!(true);
    }));
    (api, dir, id)
}

async fn plan_exit(dir: &Path) -> (Option<i32>, String) {
    let o = run(dir, &["plan", "--detailed-exitcode"]).await;
    (o.status.code(), out(&o) + &err(&o))
}

#[tokio::test(flavor = "multi_thread")]
async fn drift_formats() {
    let (_api, dir, _id) = drifted().await;
    let file = fs::read_to_string(dir.path().join("profiles/casa.toml")).unwrap();
    let line = |key: &str| {
        file.lines()
            .position(|l| l.starts_with(key))
            .map(|i| i + 1)
            .unwrap()
    };

    let o = run(dir.path(), &["drift"]).await;
    assert_eq!(o.status.code(), Some(2));
    assert!(out(&o).contains("[drift]"), "{}", out(&o));

    let o = run(dir.path(), &["drift", "--format", "github"]).await;
    assert_eq!(o.status.code(), Some(2));
    let text = out(&o);
    assert!(
        text.contains(&format!(
            "::warning file=profiles/casa.toml,line={},title=NextDNS drift::profile \"casa\": denylist changed in NextDNS",
            line("[denylist]")
        )),
        "{text}"
    );
    assert!(text.contains(&format!("line={},", line("web3"))), "{text}");

    let metrics = tempfile::tempdir().unwrap();
    let m = metrics.path().to_str().unwrap();
    let o = run(
        dir.path(),
        &["drift", "--format", "prom", "--textfile-dir", m],
    )
    .await;
    assert_eq!(o.status.code(), Some(2), "{}", err(&o));
    let prom = fs::read_to_string(metrics.path().join("ndac_drift.prom")).unwrap();
    assert!(
        prom.contains("ndac_drift{profile=\"casa\",section=\"denylist\"} 1"),
        "{prom}"
    );
    assert!(prom.contains("ndac_drift_last_run_timestamp_seconds "));
    assert_eq!(fs::read_dir(metrics.path()).unwrap().count(), 1);

    let o = run(dir.path(), &["drift", "--textfile-dir", m]).await;
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("requires --format prom"), "{}", err(&o));
}

#[tokio::test(flavor = "multi_thread")]
async fn reconcile_prefer_live_imports_and_empties_the_plan() {
    let (api, dir, _id) = drifted().await;
    api.clear_requests();
    let o = run(
        dir.path(),
        &["reconcile", "--non-interactive", "--prefer", "live"],
    )
    .await;
    assert!(o.status.success(), "{}", err(&o));
    assert!(
        err(&o).contains("✓ updated profiles/casa.toml"),
        "{}",
        err(&o)
    );
    assert!(
        api.request_lines().iter().all(|l| l.starts_with("GET ")),
        "{:?}",
        api.request_lines()
    );
    let file = fs::read_to_string(dir.path().join("profiles/casa.toml")).unwrap();
    assert!(
        file.contains("\"bar.example\" = true # imported "),
        "{file}"
    );
    assert!(file.contains("(drift)"), "{file}");

    // Invariant: reconcile --prefer live → plan is empty.
    let (code, text) = plan_exit(dir.path()).await;
    assert_eq!(code, Some(0), "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn reconcile_prefer_code_overwrites_live() {
    let (api, dir, id) = drifted().await;
    let before = fs::read_to_string(dir.path().join("profiles/casa.toml")).unwrap();
    let o = run(
        dir.path(),
        &["reconcile", "--non-interactive", "--prefer", "code"],
    )
    .await;
    assert!(o.status.success(), "{}{}", out(&o), err(&o));
    assert!(err(&o).contains("applied and verified"), "{}", err(&o));
    let live = api.profile(&id).unwrap();
    assert_eq!(live["settings"]["web3"], false);
    assert_eq!(live["denylist"].as_array().unwrap().len(), 1);
    assert_eq!(
        fs::read_to_string(dir.path().join("profiles/casa.toml")).unwrap(),
        before
    );
    let (code, text) = plan_exit(dir.path()).await;
    assert_eq!(code, Some(0), "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn reconcile_needs_a_terminal_or_prefer() {
    let (_api, dir, _id) = drifted().await;
    let o = run(dir.path(), &["reconcile"]).await;
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).contains("--non-interactive --prefer"),
        "{}",
        err(&o)
    );

    let o = run(dir.path(), &["reconcile", "--non-interactive"]).await;
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("--prefer"), "{}", err(&o));
}
