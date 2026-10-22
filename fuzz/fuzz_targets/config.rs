//! Configuration parsing and normalization. The input is a template and a
//! profile file separated by a NUL byte; both are loaded from a scratch
//! directory, validated and resolved. Also checks that `format_toml` is
//! idempotent.
#![no_main]

use std::{fs, sync::OnceLock};

use libfuzzer_sys::fuzz_target;
use ndac_core::config::{Workspace, format_toml};
use tempfile::TempDir;

const ROOT: &str = "schema = 1\ninclude = [\"templates/*.toml\", \"profiles/*.toml\"]\n";

fn scratch() -> &'static TempDir {
    static DIR: OnceLock<TempDir> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = tempfile::tempdir().expect("scratch directory");
        fs::write(dir.path().join("ndac.toml"), ROOT).expect("root file");
        fs::create_dir(dir.path().join("templates")).expect("templates/");
        fs::create_dir(dir.path().join("profiles")).expect("profiles/");
        dir
    })
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let (template, profile) = text.split_once('\0').unwrap_or(("", text));

    for part in [template, profile] {
        if let Ok(once) = format_toml(part) {
            let twice = format_toml(&once).expect("formatted output parses");
            assert_eq!(once, twice, "format_toml is not idempotent");
        }
    }

    let dir = scratch().path();
    fs::write(dir.join("templates/t.toml"), template).expect("write template");
    fs::write(dir.join("profiles/p.toml"), profile).expect("write profile");
    if let Ok(ws) = Workspace::load(&dir.join("ndac.toml"))
        && let Ok(all) = ws.resolve_all()
    {
        for p in all {
            let _ = p.to_json();
            let _ = p.to_toml();
        }
    }
});
