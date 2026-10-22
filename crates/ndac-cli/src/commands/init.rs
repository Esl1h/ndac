//! `ndac init`.

use std::{fs, path::Path};

use miette::Report;

use super::{CmdResult, Failure};
use crate::output::{Exit, Output};

const ROOT: &str = r#"schema  = 1
include = ["profiles/*.toml", "templates/*.toml"]

[provider]
timeout       = "15s"
retries       = 5
concurrency   = 4
snapshot_dir  = ".ndac/snapshots"
snapshot_keep = 50

[auth]
# Where to find the NextDNS API key, in order. See `ndac auth sources`.
# Never put the key itself here: use ndac.local.toml (git-ignored) or one
# of the sources below.
sources = ["env:NEXTDNS_API_KEY", "keyring:nextdns/default"]
"#;

const BASE: &str = r#"# Settings shared by every profile that lists "base" in `extends`.

[security]
threatIntelligenceFeeds = true
googleSafeBrowsing      = true
cryptojacking           = true
dnsRebinding            = true
idnHomographs           = true
typosquatting           = true

[privacy]
blocklists        = ["nextdns-recommended"]
disguisedTrackers = true
"#;

const TAPLO: &str = r#"include = ["ndac.toml", "ndac.local.toml", "templates/*.toml", "profiles/*.toml"]

[formatting]
align_entries = true
reorder_keys  = false

[[rule]]
include = ["ndac.toml"]
[rule.schema]
path = "https://raw.githubusercontent.com/Esl1h/ndac/main/schema/ndac.schema.json"

[[rule]]
include = ["templates/*.toml", "profiles/*.toml"]
[rule.schema]
path = "https://raw.githubusercontent.com/Esl1h/ndac/main/schema/ndac-profile.schema.json"
"#;

const IGNORE: &[&str] = &["ndac.local.toml", ".env", ".ndac/"];

fn other(msg: impl Into<String>) -> Failure {
    Failure::Other(Report::msg(msg.into()))
}

fn write_new(dir: &Path, rel: &str, text: &str, out: Output) -> Result<(), Failure> {
    let path = dir.join(rel);
    if path.exists() {
        out.status(&format!("- {rel} exists, kept"));
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| other(format!("cannot create {}: {e}", parent.display())))?;
    }
    fs::write(&path, text).map_err(|e| other(format!("cannot write {rel}: {e}")))?;
    out.status(&format!("✓ created {rel}"));
    Ok(())
}

pub(crate) fn run(dir: &Path, out: Output) -> CmdResult {
    fs::create_dir_all(dir).map_err(|e| other(format!("cannot create {}: {e}", dir.display())))?;
    write_new(dir, "ndac.toml", ROOT, out)?;
    write_new(dir, "templates/base.toml", BASE, out)?;
    write_new(dir, ".taplo.toml", TAPLO, out)?;
    fs::create_dir_all(dir.join("profiles"))
        .map_err(|e| other(format!("cannot create profiles/: {e}")))?;

    let gi = dir.join(".gitignore");
    let current = fs::read_to_string(&gi).unwrap_or_default();
    let missing: Vec<&str> = IGNORE
        .iter()
        .copied()
        .filter(|e| !current.lines().any(|l| l.trim() == *e))
        .collect();
    if !missing.is_empty() {
        let mut text = current;
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        for e in &missing {
            text.push_str(e);
            text.push('\n');
        }
        fs::write(&gi, text).map_err(|e| other(format!("cannot write .gitignore: {e}")))?;
        out.status(&format!("✓ .gitignore: added {}", missing.join(", ")));
    }
    out.status("next: `ndac auth set --backend keyring`, then `ndac import --all` and `ndac plan`");
    Ok(Exit::Success)
}
