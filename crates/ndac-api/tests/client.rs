//! Client behavior against the fake API from `ndac-testkit`.
#![allow(clippy::unwrap_used, reason = "test helpers outside #[test] functions")]

use std::time::{Duration, Instant};

use ndac_api::{
    ActiveItem, ApiKey, Client, Error, IdItem, ListPath, ObjectPath, Performance, Profile,
    RetryPolicy,
};
use ndac_testkit::{FakeApi, Fault, TEST_KEY};
use reqwest::Method;
use serde_json::{Value, json};

fn fast_retry(max_retries: u32) -> RetryPolicy {
    RetryPolicy {
        max_retries,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(20),
    }
}

fn client(api: &FakeApi) -> Client {
    Client::builder(ApiKey::new(TEST_KEY))
        .base_url(api.base_url())
        .retry_policy(fast_retry(3))
        .build()
        .unwrap()
}

#[tokio::test]
async fn lists_profiles_across_pages() {
    let api = FakeApi::builder().page_size(2).start().await.unwrap();
    for i in 0..5 {
        let _ = api.insert_profile(&json!({ "name": format!("p{i}") }));
    }
    let profiles = client(&api).list_profiles().await.unwrap();
    let names: Vec<_> = profiles.iter().filter_map(|p| p.name.clone()).collect();
    assert_eq!(names, ["p0", "p1", "p2", "p3", "p4"]);
    let queries: Vec<_> = api.requests().into_iter().map(|r| r.query).collect();
    assert_eq!(
        queries,
        [None, Some("cursor=o2".into()), Some("cursor=o4".into())]
    );
}

#[tokio::test]
async fn page_limit_is_sent() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({ "denylist": [
        { "id": "a.example", "active": true },
        { "id": "b.example", "active": true },
        { "id": "c.example", "active": false },
    ]}));
    let c = Client::builder(ApiKey::new(TEST_KEY))
        .base_url(api.base_url())
        .page_limit(2)
        .build()
        .unwrap();
    let items: Vec<ActiveItem> = c.get_list(&id, ListPath::Denylist).await.unwrap();
    assert_eq!(items.len(), 3);
    assert!(!items[2].active);
    assert_eq!(
        api.requests()[1].query.as_deref(),
        Some("limit=2&cursor=o2")
    );
}

#[tokio::test]
async fn get_profile_preserves_unknown_fields() {
    let api = FakeApi::builder()
        .profile_extra("fingerprint", json!("fp0000"))
        .start()
        .await
        .unwrap();
    let id = api.insert_profile(&json!({ "name": "Casa" }));
    let p = client(&api).get_profile(&id).await.unwrap();
    assert_eq!(p.name.as_deref(), Some("Casa"));
    assert_eq!(p.extra["fingerprint"], json!("fp0000"));
    assert_eq!(p.extra["id"], json!(id));
    assert_eq!(p.unknown_fields(), ["fingerprint", "id"]);
}

#[tokio::test]
async fn create_then_read_back() {
    let api = FakeApi::start().await.unwrap();
    let c = client(&api);
    let desired = Profile {
        name: Some("Trabalho".into()),
        denylist: Some(vec![ActiveItem::new("tiktokv.com", true)]),
        ..Profile::default()
    };
    let id = c.create_profile(&desired).await.unwrap();
    let live = c.get_profile(&id).await.unwrap();
    assert_eq!(live.name.as_deref(), Some("Trabalho"));
    assert_eq!(
        live.denylist,
        Some(vec![ActiveItem::new("tiktokv.com", true)])
    );
}

#[tokio::test]
async fn user_error_with_200_is_a_failure() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({}));
    let err = client(&api)
        .add_item(
            &id,
            ListPath::Denylist,
            "not a domain",
            &ActiveItem::new("not a domain", true),
        )
        .await
        .unwrap_err();
    match err {
        Error::Api {
            status,
            errors,
            endpoint,
        } => {
            assert_eq!(status, 200);
            assert_eq!(errors[0].code, "invalid");
            assert_eq!(endpoint, "/profiles/:profile/denylist");
        }
        other => panic!("unexpected {other:?}"),
    }
    // User errors are not retried.
    assert_eq!(api.requests().len(), 1);
}

#[tokio::test]
async fn duplicate_profile_name_is_reported() {
    let api = FakeApi::start().await.unwrap();
    let _ = api.insert_profile(&json!({ "name": "Casa" }));
    let id = api.insert_profile(&json!({ "name": "Other" }));
    let err = client(&api)
        .patch_profile(&id, &json!({ "name": "Casa" }))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, Error::Api { errors, .. } if errors[0].code == "duplicate"),
        "{err}"
    );
}

#[tokio::test]
async fn bad_request_reports_pointer() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({}));
    let err = client(&api)
        .patch_object(&id, ObjectPath::Settings, &json!({ "nope": true }))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("(at /nope)"), "{err}");
    assert!(matches!(err, Error::Api { status, .. } if status == 400));
}

#[tokio::test]
async fn object_and_list_writes_hit_documented_paths() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({
        "denylist": [{ "id": "old.example", "active": true }]
    }));
    let c = client(&api);
    c.patch_object(
        &id,
        ObjectPath::SettingsPerformance,
        &Performance {
            ecs: Some(true),
            ..Performance::default()
        },
    )
    .await
    .unwrap();
    c.put_list(&id, ListPath::PrivacyBlocklists, &[IdItem::new("oisd")])
        .await
        .unwrap();
    c.patch_item(
        &id,
        ListPath::Denylist,
        "old.example",
        &json!({ "active": false }),
    )
    .await
    .unwrap();
    c.add_item(
        &id,
        ListPath::Allowlist,
        "s.youtube.com",
        &ActiveItem::new("s.youtube.com", true),
    )
    .await
    .unwrap();
    c.delete_item(&id, ListPath::Denylist, "old.example")
        .await
        .unwrap();

    assert_eq!(
        api.request_lines(),
        [
            format!("PATCH /profiles/{id}/settings/performance"),
            format!("PUT /profiles/{id}/privacy/blocklists"),
            format!("PATCH /profiles/{id}/denylist/old.example"),
            format!("POST /profiles/{id}/allowlist"),
            format!("DELETE /profiles/{id}/denylist/old.example"),
        ]
    );
    let p = api.profile(&id).unwrap();
    assert_eq!(p["settings"]["performance"]["ecs"], json!(true));
    assert_eq!(p["privacy"]["blocklists"], json!([{ "id": "oisd" }]));
    assert_eq!(
        p["allowlist"],
        json!([{ "id": "s.youtube.com", "active": true }])
    );
    assert_eq!(p["denylist"], json!([]));

    let logs: Value = c.get_object(&id, ObjectPath::SettingsLogs).await.unwrap();
    assert_eq!(logs["location"], json!("us"));
}

#[tokio::test]
async fn retries_429_honoring_retry_after() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({}));
    api.inject(Fault::status(429).retry_after("0"));
    api.inject(Fault::status(503));
    let p = client(&api).get_profile(&id).await.unwrap();
    assert!(p.security.is_some());
    assert_eq!(api.requests().len(), 3);
}

#[tokio::test]
async fn gives_up_after_max_retries() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({}));
    for _ in 0..10 {
        api.inject(Fault::status(502));
    }
    let err = client(&api).get_profile(&id).await.unwrap_err();
    assert!(
        matches!(err, Error::RetriesExhausted { attempts: 4, .. }),
        "{err}"
    );
    assert_eq!(api.requests().len(), 4);
}

#[tokio::test]
async fn no_retry_policy_returns_the_raw_error() {
    let api = FakeApi::start().await.unwrap();
    api.inject(Fault::status(500));
    let c = Client::builder(ApiKey::new(TEST_KEY))
        .base_url(api.base_url())
        .retry_policy(fast_retry(0))
        .build()
        .unwrap();
    let err = c.list_profiles().await.unwrap_err();
    assert!(
        matches!(err, Error::Status { status, .. } if status == 500),
        "{err}"
    );
}

#[tokio::test]
async fn create_profile_is_never_retried() {
    let api = FakeApi::start().await.unwrap();
    api.inject(Fault::status(503).on(Method::POST));
    let err = client(&api)
        .create_profile(&Profile::default())
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Status { status, .. } if status == 503));
    assert_eq!(api.request_lines(), ["POST /profiles"]);
}

#[tokio::test]
async fn add_item_retries_only_when_absent() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({}));
    let c = client(&api);
    let item = ActiveItem::new("a.example", true);

    // Lost before applying: the item is absent, so the POST is repeated.
    api.inject(Fault::status(503).on(Method::POST));
    c.add_item(&id, ListPath::Denylist, "a.example", &item)
        .await
        .unwrap();
    let path = format!("/profiles/{id}/denylist");
    assert_eq!(
        api.request_lines(),
        [
            format!("POST {path}"),
            format!("GET {path}"),
            format!("POST {path}")
        ]
    );

    // Applied but the response was lost: the read-back finds it, no repost.
    api.clear_requests();
    let item = ActiveItem::new("b.example", true);
    api.inject(Fault::status(503).on(Method::POST).after_apply());
    c.add_item(&id, ListPath::Denylist, "b.example", &item)
        .await
        .unwrap();
    assert_eq!(
        api.request_lines(),
        [format!("POST {path}"), format!("GET {path}")]
    );
    assert_eq!(
        api.profile(&id).unwrap()["denylist"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn delete_after_lost_response_is_success() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({ "denylist": [{ "id": "a.example", "active": true }] }));
    api.inject(Fault::status(502).on(Method::DELETE).after_apply());
    client(&api)
        .delete_item(&id, ListPath::Denylist, "a.example")
        .await
        .unwrap();
    assert_eq!(api.requests().len(), 2);
}

#[tokio::test]
async fn delete_missing_item_is_not_found() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({}));
    let err = client(&api)
        .delete_item(&id, ListPath::Denylist, "missing.example")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotFound { .. }));
}

#[tokio::test]
async fn delete_profile_removes_it() {
    let api = FakeApi::start().await.unwrap();
    let id = api.insert_profile(&json!({}));
    let c = client(&api);
    c.delete_profile(&id).await.unwrap();
    assert!(api.profile(&id).is_none());
    assert!(matches!(
        c.get_profile(&id).await,
        Err(Error::NotFound { .. })
    ));
}

#[tokio::test]
async fn wrong_key_is_an_auth_error_without_retry() {
    let api = FakeApi::start().await.unwrap();
    let c = Client::builder(ApiKey::new("test-key-1111111111111111111111111111111111"))
        .base_url(api.base_url())
        .build()
        .unwrap();
    let err = c.list_profiles().await.unwrap_err();
    assert!(err.is_auth());
    assert!(!err.to_string().contains("1111"));
    assert_eq!(api.requests().len(), 1);
}

#[tokio::test]
async fn deadline_stops_retrying() {
    let api = FakeApi::start().await.unwrap();
    api.inject(Fault::status(429).retry_after("10"));
    let c = Client::builder(ApiKey::new(TEST_KEY))
        .base_url(api.base_url())
        .retry_policy(RetryPolicy {
            max_retries: 5,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_secs(60),
        })
        .deadline(Duration::from_secs(2))
        .build()
        .unwrap();
    let started = Instant::now();
    let err = c.list_profiles().await.unwrap_err();
    assert!(
        matches!(err, Error::DeadlineExceeded { attempts: 1, .. }),
        "{err}"
    );
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn timeout_is_a_retryable_transport_error() {
    let api = FakeApi::start().await.unwrap();
    api.set_latency(Duration::from_millis(500));
    let c = Client::builder(ApiKey::new(TEST_KEY))
        .base_url(api.base_url())
        .timeout(Duration::from_millis(50))
        .retry_policy(fast_retry(1))
        .build()
        .unwrap();
    let err = c.list_profiles().await.unwrap_err();
    let Error::RetriesExhausted { last, attempts: 2 } = err else {
        panic!("unexpected {err:?}");
    };
    assert!(matches!(*last, Error::Transport { .. }));
}

#[tokio::test]
async fn concurrency_limit_serializes_requests() {
    let api = FakeApi::start().await.unwrap();
    api.set_latency(Duration::from_millis(100));
    let c = Client::builder(ApiKey::new(TEST_KEY))
        .base_url(api.base_url())
        .concurrency(1)
        .build()
        .unwrap();
    let started = Instant::now();
    let (a, b, d) = tokio::join!(c.list_profiles(), c.list_profiles(), c.list_profiles());
    a.unwrap();
    b.unwrap();
    d.unwrap();
    assert!(started.elapsed() >= Duration::from_millis(300));
}

#[test]
fn rejects_plain_http_to_remote_hosts() {
    let err = Client::builder(ApiKey::new(TEST_KEY))
        .base_url("http://api.nextdns.io")
        .build()
        .unwrap_err();
    assert!(matches!(err, Error::InvalidBaseUrl(_)));
}
