//! API response deserialization. A profile that deserializes is diffed
//! against an empty configuration, and, when its import loads back, the
//! golden invariant holds: import then plan is empty.
#![no_main]

use std::{fs, sync::OnceLock};

use libfuzzer_sys::fuzz_target;
use ndac_api::{Profile, ProfileSummary};
use ndac_core::{
    apply::profile_toml,
    config::{DesiredProfile, Workspace, types::ProfileFile},
    plan::diff,
};
use tempfile::TempDir;

const ROOT: &str = "schema = 1\ninclude = [\"profiles/*.toml\"]\n";

fn scratch() -> &'static TempDir {
    static DIR: OnceLock<TempDir> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = tempfile::tempdir().expect("scratch directory");
        fs::write(dir.path().join("ndac.toml"), ROOT).expect("root file");
        fs::create_dir(dir.path().join("profiles")).expect("profiles/");
        dir
    })
}

fuzz_target!(|data: &[u8]| {
    let _ = serde_json::from_slice::<Vec<ProfileSummary>>(data);
    let Ok(live) = serde_json::from_slice::<Profile>(data) else {
        return;
    };
    let _ = diff(
        &DesiredProfile::from_resolved("p", ProfileFile::default()),
        Some(&live),
    );

    let dir = scratch().path();
    fs::write(dir.join("profiles/p.toml"), profile_toml("abc123", &live)).expect("write profile");
    let Ok(ws) = Workspace::load(&dir.join("ndac.toml")) else {
        return;
    };
    let Ok(desired) = ws.resolve("p") else {
        return;
    };
    let plan = diff(&desired, Some(&live));
    assert!(
        plan.changes.is_empty(),
        "import then plan: {:?}",
        plan.changes
    );
});
