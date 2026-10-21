use std::fmt::Write as _;

use ndac_api::Profile;
use serde_json::{Value, json};

use super::*;
use crate::config::{DesiredProfile, types::ProfileFile};

fn desired(key: &str, toml_text: &str) -> DesiredProfile {
    let file: ProfileFile = toml::from_str(toml_text).unwrap();
    DesiredProfile::from_resolved(key, file)
}

fn live(v: &Value) -> Profile {
    serde_json::from_value(v.clone()).unwrap()
}

const CASA: &str = r#"
id = "abc123"
name = "Casa"
ignore = ["parentalControl.services"]

[security]
nrd = true

[privacy]
blocklists = ["nextdns-recommended", "hagezi-pro"]

[parentalControl]
services = { tiktok = true }

[settings.logs]
retention = "30d"

[denylist]
mode = "authoritative"
[denylist.items]
"tiktokv.com" = true
"bad.example" = false

[allowlist]
mode = "merge"
[allowlist.items]
"s.youtube.com" = true
"#;

fn casa_live() -> Value {
    json!({
        "id": "abc123",
        "name": "Casa",
        "security": { "nrd": true, "dga": true, "tlds": [] },
        "privacy": { "blocklists": [{ "id": "nextdns-recommended", "name": "NextDNS" }], "natives": [] },
        "parentalControl": { "services": [{ "id": "facebook", "active": true }] },
        "denylist": [
            { "id": "tiktokv.com", "active": true },
            { "id": "bad.example", "active": true },
            { "id": "foo.example", "active": true }
        ],
        "allowlist": [{ "id": "other.example", "active": true }],
        "settings": { "logs": { "enabled": true, "retention": 7_776_000 }, "web3": true }
    })
}

#[test]
fn identical_state_has_no_changes() {
    let d = desired("casa", CASA);
    let same = d.desired.clone();
    let plan = diff(&d, Some(&same));
    assert_eq!(plan.changes, vec![]);
    assert!(plan.is_empty());
}

#[test]
fn detects_fields_items_and_respects_modes() {
    let d = desired("casa", CASA);
    let plan = diff(&d, Some(&live(&casa_live())));
    let lines: Vec<String> = plan
        .changes
        .iter()
        .map(|c| match c {
            Change::SetField { path, from, to, .. } => format!("set {path} {from:?} {to}"),
            Change::AddItem { list, id, .. } => format!("add {list} {id}"),
            Change::RemoveItem { list, id, .. } => format!("remove {list} {id}"),
            Change::SetItemActive { list, id, to, .. } => format!("active {list} {id} {to}"),
        })
        .collect();
    assert_eq!(
        lines,
        [
            "add privacy.blocklists hagezi-pro",
            "active denylist bad.example false",
            "remove denylist foo.example",
            "add allowlist s.youtube.com",
            "set settings.logs.retention Some(Number(7776000)) 2592000",
        ]
    );
    // Unmanaged (security.dga, settings.web3), ignored (services) and
    // merge-mode extras (allowlist other.example) are untouched.
    assert!(
        plan.changes
            .iter()
            .all(|c| !c.path().starts_with("parentalControl"))
    );
}

#[test]
fn new_profile_is_a_create_with_every_desired_item() {
    let d = desired(
        "novo",
        "name = \"Novo\"\n[denylist.items]\n\"a.example\" = true\n",
    );
    let plan = diff(&d, None);
    assert!(plan.create);
    assert_eq!(plan.changes.len(), 2);
}

#[test]
fn drift_classification() {
    let d = desired(
        "casa",
        "[settings]\nweb3 = true\n[denylist.items]\n\"a.example\" = true\n\"b.example\" = true\n",
    );
    let snapshot = json!({
        "settings": { "web3": false },
        "denylist": [{ "id": "a.example", "active": true }, { "id": "c.example", "active": true }]
    });
    // Live: web3 flipped in dashboard to... still false; a.example removed
    // in the dashboard; c.example still there; d.example added live.
    let l = live(&json!({
        "settings": { "web3": false },
        "denylist": [{ "id": "c.example", "active": true }, { "id": "d.example", "active": true }]
    }));
    let mut plan = diff(&d, Some(&l));
    classify(&mut plan, &d, Some(&l), Some(&snapshot));
    let origins: Vec<(String, Origin)> = plan
        .changes
        .iter()
        .map(|c| {
            (
                format!("{}:{}", c.path(), c.item().unwrap_or("")),
                c.origin(),
            )
        })
        .collect();
    assert_eq!(
        origins,
        [
            ("denylist:a.example".into(), Origin::Drift),
            ("denylist:b.example".into(), Origin::Pending),
            ("denylist:c.example".into(), Origin::Pending),
            ("denylist:d.example".into(), Origin::Drift),
            ("settings.web3:".into(), Origin::Pending),
        ]
    );

    // Both sides changed web3 differently: conflict.
    let l2 = live(&json!({ "settings": { "web3": null }, "denylist": [] }));
    let snap2 = json!({ "settings": { "web3": false }, "denylist": [] });
    let d2 = desired("casa", "[settings]\nweb3 = true\n");
    let mut p2 = diff(&d2, Some(&l2));
    classify(&mut p2, &d2, Some(&l2), Some(&snap2));
    assert_eq!(p2.changes[0].origin(), Origin::Conflict);

    // No snapshot: everything pending.
    let mut p3 = diff(&d, Some(&l));
    classify(&mut p3, &d, Some(&l), None);
    assert!(p3.changes.iter().all(|c| c.origin() == Origin::Pending));
}

fn sample_plan() -> Plan {
    let d = desired("casa", CASA);
    let l = live(&casa_live());
    let mut casa = diff(&d, Some(&l));
    let snap = json!({ "denylist": [
        { "id": "tiktokv.com", "active": true },
        { "id": "bad.example", "active": true }
    ], "privacy": { "blocklists": [{ "id": "nextdns-recommended" }] },
       "settings": { "logs": { "retention": 7_776_000 } },
       "allowlist": [] });
    classify(&mut casa, &d, Some(&l), Some(&snap));
    let t = desired("trabalho", "id = \"def456\"\n[settings]\nweb3 = true\n");
    let trabalho = diff(&t, Some(&live(&json!({ "settings": { "web3": true } }))));
    let n = desired(
        "novo",
        "name = \"Novo\"\n[privacy]\nblocklists = [\"oisd\"]\n",
    );
    let novo = diff(&n, None);
    Plan {
        profiles: vec![casa, novo, trabalho],
        removed: vec![RemovedProfile {
            key: "old".into(),
            id: Some("zzz999".into()),
        }],
    }
}

#[test]
fn summary_counts() {
    let s = sample_plan().summary();
    assert_eq!(
        (s.add, s.change, s.remove, s.profiles, s.drift_sections),
        (4, 3, 1, 2, 1)
    );
}

#[test]
fn renders_text_json_and_markdown() {
    let plan = sample_plan();
    insta::assert_snapshot!("plan_text", render_text(&plan, false));
    insta::assert_snapshot!("plan_json", render_json(&plan));
    insta::assert_snapshot!("plan_markdown", render_markdown(&plan));
    let colored = render_text(&plan, true);
    assert!(colored.contains("\x1b[32m+\x1b[0m"));
}

#[test]
fn empty_plan_message() {
    let plan = Plan::default();
    assert_eq!(
        render_text(&plan, false),
        "No changes. Live state matches configuration.\n"
    );
}

#[test]
fn markdown_is_truncated() {
    let mut items = String::new();
    for i in 0..3000 {
        writeln!(items, "\"d{i}.example\" = true").unwrap();
    }
    let d = desired("big", &format!("[denylist.items]\n{items}"));
    let mut plan = Plan::default();
    for i in 0..3 {
        let mut p = diff(&d, Some(&Profile::default()));
        p.key = format!("big{i}");
        plan.profiles.push(p);
    }
    let md = render_markdown(&plan);
    assert!(md.len() <= MARKDOWN_LIMIT);
    assert!(md.contains("Output truncated"));
}

#[test]
fn targets_filter_changes() {
    let plan = sample_plan().filtered(&["casa.denylist".parse().unwrap()]);
    assert_eq!(plan.profiles.len(), 1);
    assert!(
        plan.profiles[0]
            .changes
            .iter()
            .all(|c| c.path() == "denylist")
    );
    let all = sample_plan().filtered(&["*".parse().unwrap()]);
    assert_eq!(all.profiles.len(), 3);
}

mod props {
    use proptest::prelude::*;

    use super::*;

    fn id() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9]{0,6}"
    }

    proptest! {
        #[test]
        fn diff_of_identical_state_is_empty(
            blocklists in prop::collection::vec(id(), 0..5),
            items in prop::collection::btree_map(id(), any::<bool>(), 0..5),
            web3 in prop::option::of(any::<bool>()),
            retention in prop::option::of(1u64..100_000_000),
            merge in any::<bool>(),
        ) {
            let mut text = format!("[privacy]\nblocklists = {blocklists:?}\n[denylist]\nmode = \"{}\"\n[denylist.items]\n", if merge { "merge" } else { "authoritative" });
            for (k, v) in &items {
                writeln!(text, "\"{k}.example\" = {v}").unwrap();
            }
            if let Some(w) = web3 {
                writeln!(text, "[settings]\nweb3 = {w}").unwrap();
            }
            if let Some(r) = retention {
                writeln!(text, "[settings.logs]\nretention = {r}").unwrap();
            }
            let d = desired("p", &text);
            let plan = diff(&d, Some(&d.desired.clone()));
            prop_assert_eq!(plan.changes, vec![]);
        }
    }
}
