//! `ndac plan` and `ndac snapshot` against the fake API.
#![allow(clippy::unwrap_used, reason = "tests")]

use std::{fs, path::Path, process::Output};

use assert_cmd::Command;
use ndac_core::{
    config::types::ProviderConfig,
    snapshot::{SnapshotKind, SnapshotStore},
};
use ndac_testkit::{FakeApi, TEST_KEY};
use serde_json::{Value, json};
use tempfile::TempDir;

fn workspace(api: &FakeApi, files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = format!(
        "schema = 1\ninclude = [\"profiles/*.toml\"]\n[provider]\napi_url = {:?}\nretries = 0\n",
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
        .env("NEXTDNS_API_KEY", TEST_KEY)
        .env("NDAC_AUTH_SOURCE", "env:NEXTDNS_API_KEY")
        .env_remove("NDAC_CONFIG")
        .env_remove("NDAC_LOG");
    cmd
}

async fn run(mut cmd: Command) -> Output {
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

fn store(dir: &Path) -> SnapshotStore {
    SnapshotStore::new(dir, &ProviderConfig::default())
}

async fn setup() -> (FakeApi, String, TempDir) {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({
        "name": "Casa",
        "settings": { "logs": { "retention": 7_776_000 } },
        "denylist": [{ "id": "foo.example", "active": true }]
    }));
    let profile = format!(
        "id = {id:?}\nname = \"Casa\"\n[settings.logs]\nretention = \"30d\"\n[denylist.items]\n\"tiktokv.com\" = true\n"
    );
    let dir = workspace(&api, &[("profiles/casa.toml", &profile)]);
    (api, id, dir)
}

#[tokio::test(flavor = "multi_thread")]
async fn text_plan_and_detailed_exitcode() {
    let (_api, id, dir) = setup().await;
    let mut cmd = ndac(dir.path());
    cmd.arg("plan");
    let o = run(cmd).await;
    assert!(o.status.success(), "{}", err(&o));
    let text = out(&o);
    assert!(text.contains(&format!("profile \"casa\" ({id})")), "{text}");
    assert!(
        text.contains("  ~ settings.logs.retention: 7776000 → 2592000 (90d → 30d)"),
        "{text}"
    );
    assert!(text.contains("      + tiktokv.com"));
    assert!(text.contains("      - foo.example"));
    assert!(text.contains("Plan: 1 to add, 1 to change, 1 to remove across 1 profile."));

    let mut cmd = ndac(dir.path());
    cmd.args(["plan", "--detailed-exitcode"]);
    assert_eq!(run(cmd).await.status.code(), Some(2));
}

#[tokio::test(flavor = "multi_thread")]
async fn no_changes_exit_0_even_with_detailed_exitcode() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({ "name": "Same" }));
    let dir = workspace(
        &api,
        &[(
            "profiles/same.toml",
            &format!("id = {id:?}\nname = \"Same\"\n"),
        )],
    );
    let mut cmd = ndac(dir.path());
    cmd.args(["plan", "--detailed-exitcode"]);
    let o = run(cmd).await;
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(out(&o).contains("No changes."));
}

#[tokio::test(flavor = "multi_thread")]
async fn json_and_markdown_formats() {
    let (_api, _id, dir) = setup().await;
    let mut cmd = ndac(dir.path());
    cmd.args(["plan", "--format", "json"]);
    let o = run(cmd).await;
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["format_version"], 1);
    assert_eq!(v["summary"]["remove"], 1);
    assert_eq!(v["profiles"][0]["changes"][0]["op"], "add_item");

    let mut cmd = ndac(dir.path());
    cmd.args(["plan", "--format", "markdown"]);
    let md = out(&run(cmd).await);
    assert!(md.starts_with("## ndac plan"));
    assert!(md.contains("<details>"));
}

#[tokio::test(flavor = "multi_thread")]
async fn new_profile_and_missing_id() {
    let api = FakeApi::start().await.unwrap();
    let dir = workspace(&api, &[("profiles/novo.toml", "name = \"Novo\"\n")]);
    let mut cmd = ndac(dir.path());
    cmd.arg("plan");
    let o = run(cmd).await;
    assert!(
        out(&o).contains("profile \"novo\" (to be created)"),
        "{}",
        out(&o)
    );
    assert!(
        api.request_lines().is_empty(),
        "nothing fetched for a new profile"
    );

    fs::write(dir.path().join("profiles/novo.toml"), "id = \"ffffff\"\n").unwrap();
    let mut cmd = ndac(dir.path());
    cmd.arg("plan");
    let o = run(cmd).await;
    assert_eq!(o.status.code(), Some(5));
    assert!(
        err(&o).contains("NextDNS profile ffffff does not exist"),
        "{}",
        err(&o)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn drift_and_removed_profiles_use_snapshots() {
    let (api, id, dir) = setup().await;
    let s = store(dir.path());
    // Last apply left foo.example out; it was added in the dashboard since.
    s.save(
        "casa",
        &id,
        SnapshotKind::PostApply,
        &json!({ "settings": { "logs": { "retention": 2_592_000 } }, "denylist": [] }),
    )
    .unwrap();
    s.save("old", "zzz999", SnapshotKind::PostApply, &json!({}))
        .unwrap();
    let mut cmd = ndac(dir.path());
    cmd.arg("plan");
    let o = run(cmd).await;
    let text = out(&o);
    assert!(text.contains("! denylist"), "{text}");
    assert!(text.contains("[drift]"));
    assert!(text.contains("Drift in 2 sections."), "{text}");
    assert!(text.contains("profile \"old\" (zzz999): configuration file removed"));
    drop(api);
}

#[tokio::test(flavor = "multi_thread")]
async fn targets() {
    let (_api, _id, dir) = setup().await;
    let mut cmd = ndac(dir.path());
    cmd.args(["plan", "--target", "casa.denylist"]);
    let text = out(&run(cmd).await);
    assert!(text.contains("+ tiktokv.com"));
    assert!(!text.contains("retention"), "{text}");

    let mut cmd = ndac(dir.path());
    cmd.args(["plan", "--target", "casa.nope"]);
    let o = run(cmd).await;
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("unknown path"));
}

#[test]
fn snapshot_list_show_diff() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("ndac.toml"), "schema = 1\n").unwrap();
    let s = store(dir.path());
    let a = s
        .save(
            "casa",
            "abc123",
            SnapshotKind::PreApply,
            &json!({ "name": "A", "denylist": [] }),
        )
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let b = s
        .save(
            "casa",
            "abc123",
            SnapshotKind::PostApply,
            &json!({ "name": "B", "denylist": [{ "id": "x.example", "active": true }] }),
        )
        .unwrap();

    let list = ndac(dir.path())
        .args(["snapshot", "list"])
        .output()
        .unwrap();
    let text = out(&list);
    assert!(text.contains(&a.id) && text.contains(&b.id), "{text}");
    assert!(text.contains("post-apply"));

    let show = ndac(dir.path())
        .args(["snapshot", "show", &a.id])
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&show.stdout).unwrap();
    assert_eq!(v["data"]["name"], "A");

    let diff = ndac(dir.path())
        .args(["snapshot", "diff", &a.id, &b.id])
        .output()
        .unwrap();
    let text = out(&diff);
    assert!(text.contains("~ name: \"A\" → \"B\""), "{text}");
    assert!(text.contains("+ x.example"), "{text}");
}
