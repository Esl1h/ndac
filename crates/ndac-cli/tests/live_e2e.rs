//! Live end-to-end test against a real, dedicated throwaway NextDNS
//! profile. Never run on pull requests; see `.github/workflows/e2e.yml`.
//!
//! ```sh
//! NEXTDNS_E2E_API_KEY=... NEXTDNS_E2E_PROFILE_ID=... \
//!   cargo test -p ndac --features live-e2e --test live_e2e -- --ignored
//! ```
//!
//! It imports the profile, adds one denylist item, applies, and rolls back
//! to the pre-apply snapshot, checking the golden invariants on the way.
#![cfg(feature = "live-e2e")]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests")]

use std::{env, fs, path::Path, process::Output};

use assert_cmd::Command;
use ndac_core::{
    config::types::ProviderConfig,
    snapshot::{SnapshotKind, SnapshotStore},
};

const ITEM: &str = "ndac-e2e.example";

fn ndac(dir: &Path) -> Command {
    let key = env::var("NEXTDNS_E2E_API_KEY").expect("NEXTDNS_E2E_API_KEY");
    let mut cmd = Command::cargo_bin("ndac").unwrap();
    cmd.current_dir(dir)
        .env("NO_COLOR", "1")
        .env("NEXTDNS_API_KEY", key)
        .env("NDAC_AUTH_SOURCE", "env:NEXTDNS_API_KEY")
        .env_remove("NDAC_CONFIG");
    cmd
}

fn check(o: &Output) -> String {
    let text =
        String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{text}");
    text
}

fn plan_code(dir: &Path) -> Option<i32> {
    ndac(dir)
        .args(["plan", "--detailed-exitcode"])
        .output()
        .unwrap()
        .status
        .code()
}

#[test]
#[ignore = "needs a throwaway NextDNS profile; see the module docs"]
fn import_apply_rollback() {
    let id = env::var("NEXTDNS_E2E_PROFILE_ID").expect("NEXTDNS_E2E_PROFILE_ID");

    // A: the whole profile, imported.
    let imported = tempfile::tempdir().unwrap();
    check(&ndac(imported.path()).arg("init").output().unwrap());
    check(
        &ndac(imported.path())
            .args(["import", "--profile", &id])
            .output()
            .unwrap(),
    );
    for f in fs::read_dir(imported.path().join("profiles")).unwrap() {
        let text = fs::read_to_string(f.unwrap().path()).unwrap();
        assert!(!text.contains(ITEM), "remove {ITEM} left by an earlier run");
    }
    assert_eq!(plan_code(imported.path()), Some(0), "import then plan");

    // B: only the change, with the denylist in merge mode.
    let change = tempfile::tempdir().unwrap();
    check(&ndac(change.path()).arg("init").output().unwrap());
    fs::write(
        change.path().join("profiles/e2e.toml"),
        format!(
            "id = {id:?}\n\n[denylist]\nmode = \"merge\"\n\n[denylist.items]\n{ITEM:?} = true\n"
        ),
    )
    .unwrap();
    assert_eq!(plan_code(change.path()), Some(2));
    check(
        &ndac(change.path())
            .args(["apply", "--auto-approve"])
            .output()
            .unwrap(),
    );
    assert_eq!(plan_code(change.path()), Some(0), "apply then plan");
    // The imported, authoritative copy now sees the new item.
    assert_eq!(plan_code(imported.path()), Some(2));

    let store = SnapshotStore::new(change.path(), &ProviderConfig::default());
    let pre = store
        .list(Some("e2e"))
        .unwrap()
        .into_iter()
        .find(|e| store.load(e).unwrap().kind == SnapshotKind::PreApply)
        .expect("pre-apply snapshot");
    check(
        &ndac(change.path())
            .args(["rollback", &pre.id, "--auto-approve"])
            .output()
            .unwrap(),
    );
    assert_eq!(plan_code(imported.path()), Some(0), "rollback restores");
}
