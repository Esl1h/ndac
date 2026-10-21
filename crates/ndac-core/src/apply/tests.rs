use std::{fmt::Write as _, sync::atomic::AtomicBool};

use ndac_api::{ApiKey, Client, Profile, RetryPolicy};
use ndac_testkit::{FakeApi, TEST_KEY};
use serde_json::{Value, json};

use super::*;
use crate::{
    config::{DesiredProfile, types::ProfileFile},
    plan::diff,
};

fn desired(key: &str, toml_text: &str) -> DesiredProfile {
    let file: ProfileFile = toml::from_str(toml_text).unwrap();
    DesiredProfile::from_resolved(key, file)
}

fn client(api: &FakeApi) -> Client {
    Client::builder(ApiKey::new(TEST_KEY))
        .base_url(api.base_url())
        .retry_policy(RetryPolicy {
            max_retries: 0,
            ..RetryPolicy::default()
        })
        .build()
        .unwrap()
}

const CASA: &str = r#"
name = "Casa"
[security]
nrd = true
dga = true
tlds = ["ru", "zip"]
[privacy]
blocklists = ["nextdns-recommended", "oisd"]
[parentalControl]
safeSearch = true
categories = { porn = true }
[settings]
web3 = false
[settings.logs]
retention = "30d"
location = "eu"
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

fn seeded_live() -> Value {
    json!({
        "name": "Old",
        "security": { "nrd": false, "dga": true, "tlds": [{ "id": "ru" }] },
        "privacy": { "blocklists": [{ "id": "nextdns-recommended" }] },
        "denylist": [
            { "id": "tiktokv.com", "active": true },
            { "id": "bad.example", "active": true },
            { "id": "foo.example", "active": true }
        ],
        "allowlist": [{ "id": "keep.example", "active": true }],
        "settings": { "logs": { "retention": 7_776_000, "location": "eu" }, "web3": true }
    })
}

#[test]
fn planner_groups_fields_and_picks_list_strategies() {
    let mut d = desired("casa", CASA);
    d.id = Some("abc123".into());
    let live: Profile = serde_json::from_value(seeded_live()).unwrap();
    let plan = diff(&d, Some(&live));
    let described: Vec<String> = calls(&plan, &d).iter().map(ApiCall::describe).collect();
    assert_eq!(
        described,
        [
            "PATCH /profiles/:profile",
            "PATCH /profiles/:profile/security",
            "POST /profiles/:profile/security/tlds",
            "POST /profiles/:profile/privacy/blocklists",
            "POST /profiles/:profile/parentalControl/categories",
            "PATCH /profiles/:profile/parentalControl",
            "PUT /profiles/:profile/denylist",
            "POST /profiles/:profile/allowlist",
            "PATCH /profiles/:profile/settings/logs",
            "PATCH /profiles/:profile/settings",
        ]
    );
}

#[test]
fn single_item_changes_avoid_put() {
    let d = desired("p", "[denylist.items]\n\"a.example\" = false\n");
    let live: Profile =
        serde_json::from_value(json!({ "denylist": [{ "id": "a.example", "active": true }] }))
            .unwrap();
    let c = calls(&diff(&d, Some(&live)), &d);
    assert_eq!(
        c,
        vec![ApiCall::PatchItem {
            list: ndac_api::ListPath::Denylist,
            id: "a.example".into(),
            body: json!({ "active": false }),
        }]
    );
}

#[test]
fn emptying_an_authoritative_list_is_detected() {
    let d = desired("p", "[denylist]\nmode = \"authoritative\"\n");
    let live: Profile =
        serde_json::from_value(json!({ "denylist": [{ "id": "a.example", "active": true }] }))
            .unwrap();
    assert_eq!(
        empties_a_list(&diff(&d, Some(&live)), &d).as_deref(),
        Some("denylist")
    );
}

#[tokio::test]
async fn apply_then_plan_is_empty_with_exact_calls() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&seeded_live());
    let c = client(&api);
    let mut d = desired("casa", CASA);
    d.id = Some(id.clone());

    let live = c.get_profile(&id).await.unwrap();
    let plan = diff(&d, Some(&live));
    let planned = calls(&plan, &d);
    api.clear_requests();
    let outcome = execute(&c, Some(&id), &planned, &AtomicBool::new(false)).await;
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert_eq!(outcome.done, planned.len());

    let p = format!("/profiles/{id}");
    assert_eq!(
        api.request_lines(),
        [
            format!("PATCH {p}"),
            format!("PATCH {p}/security"),
            format!("POST {p}/security/tlds"),
            format!("POST {p}/privacy/blocklists"),
            format!("POST {p}/parentalControl/categories"),
            format!("PATCH {p}/parentalControl"),
            format!("PUT {p}/denylist"),
            format!("POST {p}/allowlist"),
            format!("PATCH {p}/settings/logs"),
            format!("PATCH {p}/settings"),
        ]
    );

    let after = c.get_profile(&id).await.unwrap();
    assert_eq!(diff(&d, Some(&after)).changes, vec![]);
    // Merge-mode list kept the live-only item.
    let allow = api.profile(&id).unwrap()["allowlist"].clone();
    assert!(
        allow
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["id"] == "keep.example")
    );
}

#[tokio::test]
async fn create_then_plan_is_empty() {
    let api = FakeApi::start().await.unwrap();
    let c = client(&api);
    let d = desired("novo", CASA);
    let plan = diff(&d, None);
    let planned = calls(&plan, &d);
    assert_eq!(planned.len(), 1);
    let outcome = execute(&c, None, &planned, &AtomicBool::new(false)).await;
    let id = outcome.created_id.unwrap();
    let after = c.get_profile(&id).await.unwrap();
    assert_eq!(diff(&d, Some(&after)).changes, vec![]);
    assert_eq!(
        api.request_lines(),
        ["POST /profiles".to_owned(), format!("GET /profiles/{id}")]
    );
}

#[tokio::test]
async fn import_then_plan_is_empty() {
    let api = FakeApi::start().await.unwrap();
    let mut seeded = seeded_live();
    seeded["parentalControl"] = json!({
        "safeSearch": true, "youtubeRestrictedMode": false, "blockBypass": true,
        "services": [{ "id": "tiktok", "active": false }],
        "categories": [{ "id": "porn", "active": true }]
    });
    let id = api.insert_profile(&seeded);
    let c = client(&api);
    let live = c.get_profile(&id).await.unwrap();
    let text = profile_toml(&id, &live);
    let d = desired("imported", &text);
    let plan = diff(&d, Some(&live));
    assert_eq!(plan.changes, vec![], "{text}");
    assert!(!plan.create);
}

#[tokio::test]
async fn cancel_stops_between_calls_and_errors_stop_the_profile() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&seeded_live());
    let c = client(&api);
    let mut d = desired("casa", CASA);
    d.id = Some(id.clone());
    let live = c.get_profile(&id).await.unwrap();
    let planned = calls(&diff(&d, Some(&live)), &d);

    api.clear_requests();
    let o = execute(&c, Some(&id), &planned, &AtomicBool::new(true)).await;
    assert!(o.cancelled);
    assert_eq!(o.done, 0);
    assert_eq!(api.requests().len(), 0);

    api.inject(ndac_testkit::Fault::status(500));
    let o = execute(&c, Some(&id), &planned, &AtomicBool::new(false)).await;
    let (call, _) = o.error.unwrap();
    assert_eq!(call, "PATCH /profiles/:profile");
    assert_eq!(o.done, 0);
}

mod props {
    use proptest::prelude::*;

    use super::*;

    fn id() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9]{0,5}"
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]

        #[test]
        fn apply_reaches_desired_state(
            want_lists in prop::collection::vec(id(), 0..4),
            have_lists in prop::collection::vec(id(), 0..4),
            want_items in prop::collection::btree_map(id(), any::<bool>(), 0..4),
            have_items in prop::collection::btree_map(id(), any::<bool>(), 0..4),
            web3 in any::<bool>(),
            merge in any::<bool>(),
        ) {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(async {
                let api = FakeApi::start().await.unwrap();
                let live_json = json!({
                    "privacy": { "blocklists": have_lists.iter().map(|i| json!({ "id": i })).collect::<Vec<_>>() },
                    "denylist": have_items.iter().map(|(k, v)| json!({ "id": format!("{k}.example"), "active": v })).collect::<Vec<_>>(),
                    "settings": { "web3": !web3 }
                });
                let id = api.insert_profile(&live_json);
                let mut text = format!(
                    "[privacy]\nblocklists = {want_lists:?}\n[settings]\nweb3 = {web3}\n[denylist]\nmode = \"{}\"\n[denylist.items]\n",
                    if merge { "merge" } else { "authoritative" }
                );
                for (k, v) in &want_items {
                    writeln!(text, "\"{k}.example\" = {v}").unwrap();
                }
                let mut d = desired("p", &text);
                d.id = Some(id.clone());
                let c = client(&api);
                let live = c.get_profile(&id).await.unwrap();
                let planned = calls(&diff(&d, Some(&live)), &d);
                let o = execute(&c, Some(&id), &planned, &AtomicBool::new(false)).await;
                assert!(o.error.is_none(), "{:?}", o.error);
                let after = c.get_profile(&id).await.unwrap();
                assert_eq!(diff(&d, Some(&after)).changes, vec![]);
            });
        }
    }
}
