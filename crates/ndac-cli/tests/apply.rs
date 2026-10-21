//! `ndac init`, `import`, `apply` and `rollback` against the fake API,
//! including the golden invariants: import then plan is empty, apply then
//! plan is empty.
#![allow(clippy::unwrap_used, reason = "tests")]

use std::{fs, path::Path, process::Output};

use assert_cmd::Command;
use ndac_core::{
    config::types::ProviderConfig,
    snapshot::{SnapshotKind, SnapshotStore},
};
use ndac_testkit::{FakeApi, TEST_KEY};
use serde_json::json;
use tempfile::TempDir;

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

/// `ndac init` plus a provider pointing at the fake.
fn init(api: &FakeApi) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    ndac(dir.path()).arg("init").assert().success();
    let root = dir.path().join("ndac.toml");
    let text = fs::read_to_string(&root).unwrap();
    let text = text.replace(
        "[provider]\n",
        &format!("[provider]\napi_url = {:?}\n", api.base_url()),
    );
    fs::write(
        &root,
        text.replace("retries       = 5", "retries       = 0"),
    )
    .unwrap();
    dir
}

async fn plan_exit(dir: &Path) -> (Option<i32>, String) {
    let mut cmd = ndac(dir);
    cmd.args(["plan", "--detailed-exitcode"]);
    let o = run(cmd).await;
    (o.status.code(), out(&o) + &err(&o))
}

#[tokio::test(flavor = "multi_thread")]
async fn init_import_plan_apply_plan() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({
        "name": "Casa",
        "privacy": { "blocklists": [{ "id": "oisd" }] },
        "denylist": [{ "id": "foo.example", "active": true }],
        "settings": { "logs": { "retention": 7_776_000 } }
    }));
    let dir = init(&api);
    assert!(dir.path().join(".taplo.toml").exists());
    assert!(
        fs::read_to_string(dir.path().join(".gitignore"))
            .unwrap()
            .contains("ndac.local.toml")
    );

    let mut cmd = ndac(dir.path());
    cmd.args(["import", "--all"]);
    let o = run(cmd).await;
    assert!(o.status.success(), "{}", err(&o));
    assert!(err(&o).contains("→ profiles/casa.toml"), "{}", err(&o));
    let file = dir.path().join("profiles/casa.toml");
    assert!(
        fs::read_to_string(&file)
            .unwrap()
            .contains("retention = \"90d\"")
    );

    // Invariant: import → plan is empty.
    let (code, text) = plan_exit(dir.path()).await;
    assert_eq!(code, Some(0), "{text}");

    // Change the configuration and apply.
    let text = fs::read_to_string(&file).unwrap();
    let text = text
        .replace(
            "\"foo.example\" = true",
            "\"foo.example\" = false\n\"tiktokv.com\" = true",
        )
        .replace("retention = \"90d\"", "retention = \"30d\"");
    fs::write(&file, text).unwrap();
    let (code, _) = plan_exit(dir.path()).await;
    assert_eq!(code, Some(2));

    api.clear_requests();
    let mut cmd = ndac(dir.path());
    cmd.args(["apply", "--auto-approve"]);
    let o = run(cmd).await;
    assert!(o.status.success(), "{}{}", out(&o), err(&o));
    assert!(err(&o).contains("applied and verified"), "{}", err(&o));
    let p = format!("/profiles/{id}");
    assert_eq!(
        api.request_lines(),
        [
            format!("GET {p}"),
            format!("PUT {p}/denylist"),
            format!("PATCH {p}/settings/logs"),
            format!("GET {p}"),
        ]
    );

    // Invariant: apply → plan is empty.
    let (code, text) = plan_exit(dir.path()).await;
    assert_eq!(code, Some(0), "{text}");

    let store = SnapshotStore::new(dir.path(), &ProviderConfig::default());
    assert!(
        store
            .latest("casa", SnapshotKind::PreApply)
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .latest("casa", SnapshotKind::PostApply)
            .unwrap()
            .is_some()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn create_writes_the_id_back() {
    let api = FakeApi::start().await.unwrap();
    let dir = init(&api);
    let file = dir.path().join("profiles/novo.toml");
    fs::write(
        &file,
        "# new profile\nname = \"Novo\"\nextends = [\"base\"]\n",
    )
    .unwrap();
    let mut cmd = ndac(dir.path());
    cmd.args(["apply", "--auto-approve"]);
    let o = run(cmd).await;
    assert!(o.status.success(), "{}", err(&o));
    let text = fs::read_to_string(&file).unwrap();
    assert!(
        text.starts_with("# new profile\nname = \"Novo\"\n"),
        "{text}"
    );
    assert!(text.contains("id = \"a00001\""), "{text}");
    assert_eq!(
        api.profile("a00001").unwrap()["privacy"]["blocklists"],
        json!([{ "id": "nextdns-recommended" }])
    );
    let (code, text) = plan_exit(dir.path()).await;
    assert_eq!(code, Some(0), "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn confirmation_and_destroy_guards() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({ "denylist": [{ "id": "a.example", "active": true }] }));
    let dir = init(&api);
    fs::write(
        dir.path().join("profiles/p.toml"),
        format!("id = {id:?}\n[denylist]\nmode = \"authoritative\"\n"),
    )
    .unwrap();

    // No terminal and no --auto-approve.
    let mut cmd = ndac(dir.path());
    cmd.arg("apply").write_stdin("yes\n");
    let o = run(cmd).await;
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("would remove every item"), "{}", err(&o));

    let mut cmd = ndac(dir.path());
    cmd.args(["apply", "--auto-approve"]);
    let o = run(cmd).await;
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("would remove every item"), "{}", err(&o));
    assert_eq!(
        api.profile(&id).unwrap()["denylist"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let mut cmd = ndac(dir.path());
    cmd.args(["apply", "--auto-approve", "--allow-destroy"]);
    assert!(run(cmd).await.status.success());
    assert_eq!(api.profile(&id).unwrap()["denylist"], json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn no_terminal_requires_auto_approve() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({ "name": "A" }));
    let dir = init(&api);
    fs::write(
        dir.path().join("profiles/p.toml"),
        format!("id = {id:?}\nname = \"B\"\n"),
    )
    .unwrap();
    let mut cmd = ndac(dir.path());
    cmd.arg("apply");
    let o = run(cmd).await;
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("pass --auto-approve"), "{}", err(&o));
    assert_eq!(api.profile(&id).unwrap()["name"], "A");
}

#[tokio::test(flavor = "multi_thread")]
async fn rollback_restores_the_pre_apply_state() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({ "name": "Before", "settings": { "web3": true } }));
    let dir = init(&api);
    fs::write(
        dir.path().join("profiles/p.toml"),
        format!("id = {id:?}\nname = \"After\"\n[settings]\nweb3 = false\n"),
    )
    .unwrap();
    let mut cmd = ndac(dir.path());
    cmd.args(["apply", "--auto-approve"]);
    assert!(run(cmd).await.status.success());
    assert_eq!(api.profile(&id).unwrap()["name"], "After");

    let store = SnapshotStore::new(dir.path(), &ProviderConfig::default());
    let pre = store
        .list(Some("p"))
        .unwrap()
        .into_iter()
        .find(|e| store.load(e).unwrap().kind == SnapshotKind::PreApply)
        .unwrap();
    let mut cmd = ndac(dir.path());
    cmd.args(["rollback", &pre.id, "--auto-approve"]);
    let o = run(cmd).await;
    assert!(o.status.success(), "{}", err(&o));
    let live = api.profile(&id).unwrap();
    assert_eq!(live["name"], "Before");
    assert_eq!(live["settings"]["web3"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn removed_profile_is_deleted_only_with_allow_destroy() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({ "name": "Old" }));
    let dir = init(&api);
    let file = dir.path().join("profiles/old.toml");
    fs::write(&file, format!("id = {id:?}\nname = \"Old\"\n")).unwrap();
    let mut cmd = ndac(dir.path());
    cmd.args(["import", "--profile", &id, "--force"]);
    assert!(run(cmd).await.status.success());
    fs::remove_file(&file).unwrap();

    let mut cmd = ndac(dir.path());
    cmd.args(["apply", "--auto-approve"]);
    let o = run(cmd).await;
    assert!(o.status.success());
    assert!(
        out(&o).contains("configuration file removed"),
        "{}",
        out(&o)
    );
    assert!(api.profile(&id).is_some());

    let mut cmd = ndac(dir.path());
    cmd.args(["apply", "--auto-approve", "--allow-destroy"]);
    let o = run(cmd).await;
    assert!(o.status.success(), "{}", err(&o));
    assert!(err(&o).contains("old: profile deleted"), "{}", err(&o));
    assert!(api.profile(&id).is_none());
    let (code, text) = plan_exit(dir.path()).await;
    assert_eq!(code, Some(0), "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_apply_is_locked_out() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({ "name": "A" }));
    let dir = init(&api);
    fs::write(
        dir.path().join("profiles/p.toml"),
        format!("id = {id:?}\nname = \"B\"\n"),
    )
    .unwrap();
    let snap_dir = dir.path().join(".ndac/snapshots");
    fs::create_dir_all(&snap_dir).unwrap();
    let held = fs::File::create(snap_dir.join(".lock")).unwrap();
    held.lock().unwrap();
    let mut cmd = ndac(dir.path());
    cmd.args(["apply", "--auto-approve"]);
    let o = run(cmd).await;
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("another apply is running"), "{}", err(&o));
    drop(held);
}

#[tokio::test(flavor = "multi_thread")]
async fn targeted_apply_verifies_only_the_target() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({ "name": "A", "settings": { "web3": false } }));
    let dir = init(&api);
    fs::write(
        dir.path().join("profiles/p.toml"),
        format!("id = {id:?}\nname = \"B\"\n[settings]\nweb3 = true\n"),
    )
    .unwrap();
    let mut cmd = ndac(dir.path());
    cmd.args(["apply", "--auto-approve", "--target", "p.name"]);
    let o = run(cmd).await;
    assert!(o.status.success(), "{}", err(&o));
    assert!(err(&o).contains("applied and verified"), "{}", err(&o));
    let live = api.profile(&id).unwrap();
    assert_eq!(live["name"], "B");
    assert_eq!(live["settings"]["web3"], false);
}
