use std::fs;

use serde_json::json;

use super::*;
use crate::{config::Workspace, plan::diff};

const BASE: &str = "\
[security]
tlds = [\"zip\"]

[privacy]
blocklists        = [\"oisd\", \"nextdns-recommended\"]
disguisedTrackers = true

[parentalControl.services]
tiktok = true
";

const CASA: &str = "\
# Casa
id      = \"abc123\"
name    = \"Casa\" # the house
extends = [\"base\"]

[settings]
web3 = false # keep off

[denylist]
mode = \"authoritative\"

[denylist.items]
\"Foo.example\" = true
\"bar.example\" = true
";

fn workspace(casa: &str) -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ndac.toml");
    fs::write(
        &root,
        "schema = 1\ninclude = [\"templates/*.toml\", \"profiles/*.toml\"]\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("templates")).unwrap();
    fs::create_dir_all(dir.path().join("profiles")).unwrap();
    fs::write(dir.path().join("templates/base.toml"), BASE).unwrap();
    fs::write(dir.path().join("profiles/casa.toml"), casa).unwrap();
    let ws = Workspace::load(&root).unwrap();
    (dir, ws)
}

fn live() -> Profile {
    serde_json::from_value(json!({
        "name": "Casa",
        "security": { "tlds": [{ "id": "zip" }, { "id": "ru" }] },
        "privacy": { "blocklists": [{ "id": "oisd" }], "disguisedTrackers": false },
        "parentalControl": { "services": [] },
        "denylist": [
            { "id": "foo.example", "active": false },
            { "id": "new.example", "active": true }
        ],
        "settings": { "web3": true }
    }))
    .unwrap()
}

/// Imports every change of `casa` into the file `pick` chooses, writes, and
/// returns the reloaded workspace.
fn import_all(
    dir: &tempfile::TempDir,
    ws: &Workspace,
    note: Option<&str>,
    pick: impl Fn(&[&Entry], &Change) -> usize,
) -> Workspace {
    let layers = ws.layer_entries("casa").unwrap();
    let plan = diff(&ws.resolve("casa").unwrap(), Some(&live()));
    assert_ne!(plan.changes, Vec::new());
    let mut wb = WriteBack::new(note.map(str::to_owned));
    for c in &plan.changes {
        wb.import(&layers, pick(&layers, c), c, &live()).unwrap();
    }
    wb.write().unwrap();
    Workspace::load(&dir.path().join("ndac.toml")).unwrap()
}

#[test]
fn prefer_live_into_the_profile_empties_the_plan() {
    let (dir, ws) = workspace(CASA);
    let ws = import_all(&dir, &ws, Some("imported 2026-10-21"), |l, _| l.len() - 1);
    // Golden invariant: reconcile --prefer live → plan is empty.
    let after = diff(&ws.resolve("casa").unwrap(), Some(&live()));
    assert_eq!(after.changes, Vec::new());
    assert_eq!(
        fs::read_to_string(dir.path().join("templates/base.toml")).unwrap(),
        BASE
    );
    insta::assert_snapshot!(
        "reconcile_profile",
        fs::read_to_string(dir.path().join("profiles/casa.toml")).unwrap()
    );
}

#[test]
fn template_target_edits_the_template() {
    let (dir, ws) = workspace(CASA);
    let ws = import_all(&dir, &ws, Some("imported 2026-10-21"), |l, c| {
        template_origin(l, c).unwrap_or(l.len() - 1)
    });
    let after = diff(&ws.resolve("casa").unwrap(), Some(&live()));
    assert_eq!(after.changes, Vec::new());
    insta::assert_snapshot!(
        "reconcile_template",
        fs::read_to_string(dir.path().join("templates/base.toml")).unwrap()
    );
    let casa = fs::read_to_string(dir.path().join("profiles/casa.toml")).unwrap();
    assert!(!casa.contains("[privacy]"), "{casa}");
    assert!(casa.contains("\"new.example\" = true"), "{casa}");
    assert!(casa.contains("tlds = [\"$inherit\", \"ru\"]"), "{casa}");
}

#[test]
fn no_note_writes_no_comment() {
    let (dir, ws) = workspace(CASA);
    let _ = import_all(&dir, &ws, None, |l, _| l.len() - 1);
    let casa = fs::read_to_string(dir.path().join("profiles/casa.toml")).unwrap();
    assert!(!casa.contains("imported"), "{casa}");
    assert!(casa.contains("# keep off"), "{casa}");
}

#[test]
fn inline_tables_stay_valid() {
    let casa = "id = \"abc123\"\nsettings = { web3 = false }\ndenylist = { items = { \"foo.example\" = true } }\n";
    let (dir, ws) = workspace(casa);
    let ws = import_all(&dir, &ws, Some("imported 2026-10-21"), |l, _| l.len() - 1);
    let after = diff(&ws.resolve("casa").unwrap(), Some(&live()));
    assert_eq!(after.changes, Vec::new());
    assert_eq!(
        fs::read_to_string(dir.path().join("profiles/casa.toml")).unwrap(),
        "id = \"abc123\"\nsettings = { web3 = true }\ndenylist = { items = { \"foo.example\" = false , \"new.example\" = true } }\n"
    );
}

#[test]
fn template_origin_only_for_inherited_values() {
    let (_dir, ws) = workspace(CASA);
    let layers = ws.layer_entries("casa").unwrap();
    let plan = diff(&ws.resolve("casa").unwrap(), Some(&live()));
    let origin = |path: &str, item: Option<&str>| {
        let c = plan
            .changes
            .iter()
            .find(|c| c.path() == path && c.item() == item)
            .unwrap();
        template_origin(&layers, c)
    };
    assert_eq!(origin("privacy.disguisedTrackers", None), Some(0));
    assert_eq!(
        origin("privacy.blocklists", Some("nextdns-recommended")),
        Some(0)
    );
    assert_eq!(origin("parentalControl.services", Some("tiktok")), Some(0));
    assert_eq!(origin("settings.web3", None), None);
    assert_eq!(origin("denylist", Some("bar.example")), None);
    // Live-only items are new: nothing to edit in a template.
    assert_eq!(origin("denylist", Some("new.example")), None);
    assert_eq!(origin("security.tlds", Some("ru")), None);
}

#[test]
fn unset_live_field_inherited_from_a_template_is_an_error() {
    let (_dir, ws) = workspace(CASA);
    let layers = ws.layer_entries("casa").unwrap();
    let mut live = live();
    live.privacy.as_mut().unwrap().disguised_trackers = None;
    let plan = diff(&ws.resolve("casa").unwrap(), Some(&live));
    let c = plan
        .changes
        .iter()
        .find(|c| c.path() == "privacy.disguisedTrackers")
        .unwrap();
    let mut wb = WriteBack::new(None);
    let err = wb.import(&layers, layers.len() - 1, c, &live).unwrap_err();
    assert!(err.reason.contains("template sets it"), "{err}");
}

#[test]
fn canonical_files_stay_canonical() {
    let casa = crate::config::format_toml(CASA).unwrap();
    let (dir, ws) = workspace(&casa);
    let _ = import_all(&dir, &ws, Some("imported 2026-10-21"), |l, _| l.len() - 1);
    let after = fs::read_to_string(dir.path().join("profiles/casa.toml")).unwrap();
    assert_eq!(crate::config::format_toml(&after).unwrap(), after);
}

#[test]
fn other_files_are_not_reformatted() {
    let casa = CASA.replace("id      =", "id =");
    assert_ne!(crate::config::format_toml(&casa).unwrap(), casa);
    let (dir, ws) = workspace(&casa);
    let _ = import_all(&dir, &ws, None, |l, _| l.len() - 1);
    let after = fs::read_to_string(dir.path().join("profiles/casa.toml")).unwrap();
    assert!(after.starts_with("# Casa\nid = \"abc123\"\n"), "{after}");
}
