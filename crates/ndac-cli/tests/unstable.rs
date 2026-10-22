//! The `unstable-endpoints` feature against the fake API: rewrites through
//! import, plan, apply and drift, and IPv6 servers from the setup endpoint.
#![cfg(feature = "unstable-endpoints")]
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

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

fn workspace(api: &FakeApi, profile: Option<&str>) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("ndac.toml"),
        format!(
            "schema = 1\ninclude = [\"profiles/*.toml\"]\n\n[provider]\napi_url = {:?}\nretries = 0\n",
            api.base_url()
        ),
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("profiles")).unwrap();
    if let Some(p) = profile {
        fs::write(dir.path().join("profiles/casa.toml"), p).unwrap();
    }
    dir
}

fn names(api: &FakeApi, id: &str) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = api
        .rewrites(id)
        .iter()
        .map(|r| {
            (
                r["name"].as_str().unwrap().to_owned(),
                r["content"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    v.sort();
    v
}

#[tokio::test(flavor = "multi_thread")]
async fn rewrites_import_plan_apply() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({ "name": "Casa" }));
    api.insert_rewrite(&id, "router.lan", "192.168.1.1");
    api.insert_rewrite(&id, "Old.Lan", "192.168.1.9");

    // Import writes the rewrites, and plan is empty right after.
    let dir = workspace(&api, None);
    let o = run(dir.path(), &["import", "--profile", &id]).await;
    assert!(o.status.success(), "{}", text(&o));
    let file = dir.path().join("profiles/casa.toml");
    let imported = fs::read_to_string(&file).unwrap();
    assert!(
        imported.contains("\"router.lan\" = \"192.168.1.1\""),
        "{imported}"
    );
    let o = run(dir.path(), &["plan", "--detailed-exitcode"]).await;
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));

    // A changed answer is a removal and an addition; an absent name is
    // removed (authoritative).
    fs::write(
        &file,
        format!(
            "id = {id:?}\n\n[rewrites.items]\n\"router.lan\" = \"192.168.1.2\"\n\"nas.lan\" = \"nas.example.com\"\n"
        ),
    )
    .unwrap();
    let o = run(dir.path(), &["plan"]).await;
    let plan = text(&o);
    assert!(plan.contains("+ router.lan → 192.168.1.2"), "{plan}");
    assert!(plan.contains("- router.lan → 192.168.1.1"), "{plan}");
    assert!(plan.contains("- Old.Lan → 192.168.1.9"), "{plan}");
    assert!(plan.contains("+ nas.lan → nas.example.com"), "{plan}");

    let o = run(dir.path(), &["apply", "--auto-approve"]).await;
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(
        names(&api, &id),
        [
            ("nas.lan".into(), "nas.example.com".into()),
            ("router.lan".into(), "192.168.1.2".into()),
        ]
    );
    let o = run(dir.path(), &["plan", "--detailed-exitcode"]).await;
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));

    // A rewrite added in the dashboard is drift; reconcile cannot write it
    // back.
    api.insert_rewrite(&id, "tv.lan", "192.168.1.5");
    let o = run(dir.path(), &["drift"]).await;
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("tv.lan → 192.168.1.5"), "{}", text(&o));
}

#[tokio::test(flavor = "multi_thread")]
async fn rewrites_merge_mode_and_new_profiles() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({ "name": "Casa" }));
    api.insert_rewrite(&id, "keep.lan", "10.0.0.1");
    let dir = workspace(
        &api,
        Some(&format!(
            "id = {id:?}\n\n[rewrites]\nmode = \"merge\"\n\n[rewrites.items]\n\"new.lan\" = \"10.0.0.2\"\n"
        )),
    );
    let o = run(dir.path(), &["apply", "--auto-approve"]).await;
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(names(&api, &id).len(), 2);

    // A new profile is created first, then its rewrites are added.
    fs::write(
        dir.path().join("profiles/novo.toml"),
        "name = \"Novo\"\n\n[rewrites.items]\n\"a.lan\" = \"10.0.0.3\"\n",
    )
    .unwrap();
    let o = run(dir.path(), &["apply", "--auto-approve"]).await;
    assert!(o.status.success(), "{}", text(&o));
    let created = fs::read_to_string(dir.path().join("profiles/novo.toml")).unwrap();
    let new_id = created
        .lines()
        .find_map(|l| l.strip_prefix("id = \""))
        .and_then(|l| l.split('"').next())
        .unwrap()
        .to_owned();
    assert_eq!(names(&api, &new_id), [("a.lan".into(), "10.0.0.3".into())]);
}

#[tokio::test(flavor = "multi_thread")]
async fn endpoint_ipv6_from_setup() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({ "name": "Casa" }));
    api.set_setup(
        &id,
        json!({ "ipv4": [], "ipv6": ["2a07:a8c0::ab:cd12", "2a07:a8c1::ab:cd12"], "dnscrypt": "", "linkedIp": { "updateToken": "secret-token" } }),
    );
    let dir = workspace(&api, Some(&format!("id = {id:?}\n")));
    let o = run(
        dir.path(),
        &[
            "endpoint",
            "render",
            "casa",
            "--target",
            "resolved",
            "--api-ipv6",
        ],
    )
    .await;
    let out = text(&o);
    assert!(o.status.success(), "{out}");
    assert!(
        out.contains(&format!("DNS=2a07:a8c0::ab:cd12#{id}.dns.nextdns.io")),
        "{out}"
    );
    assert!(!out.contains("IPv6 servers are left out"), "{out}");
    assert!(!out.contains("secret-token"), "{out}");

    // Anything but an address is refused, since the networkmanager output
    // is a shell script.
    api.set_setup(&id, json!({ "ipv6": ["::1\"; rm -rf ~; \""] }));
    let o = run(
        dir.path(),
        &[
            "endpoint",
            "render",
            "casa",
            "--target",
            "networkmanager",
            "--api-ipv6",
        ],
    )
    .await;
    assert_eq!(o.status.code(), Some(5), "{}", text(&o));
    assert!(text(&o).contains("which is not an IPv6"), "{}", text(&o));
}
