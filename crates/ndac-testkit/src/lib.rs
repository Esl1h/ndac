//! In-memory fake of the NextDNS API and shared test fixtures.
//!
//! [`FakeApi`] serves the documented profile endpoints over HTTP on a
//! loopback port. It is a black box with respect to `ndac-api`: it works on
//! raw JSON and only knows the semantics described in the API docs:
//!
//! - object endpoints support `GET` and `PATCH` (deep merge, unknown keys
//!   rejected with `400`);
//! - array endpoints support `GET` (paginated), `PUT` and `POST`, and their
//!   items `PATCH` and `DELETE`, addressed by `id`;
//! - user errors (invalid domain, duplicate name or item) are `200` with an
//!   `errors` array;
//! - faults (`429` with `Retry-After`, `5xx`) and latency can be injected,
//!   and every request is recorded.
//!
//! Behavior the docs leave open (status codes for successful writes,
//! duplicate items, error codes) follows the assumptions listed in
//! `docs/src/known-limitations.md`.
#![forbid(unsafe_code)]
// Handlers return `Err(Response)` to short-circuit; boxing them would only
// add noise in a test fake.
#![allow(clippy::result_large_err)]

use std::{
    collections::{BTreeMap, VecDeque},
    io,
    net::SocketAddr,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use axum::{
    Router,
    body::Bytes,
    extract::State as AxumState,
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use serde_json::{Map, Value, json};
use tokio::{net::TcpListener, task::JoinHandle};

/// Obviously fake API key accepted by default.
pub const TEST_KEY: &str = "test-key-0000000000000000000000000000000000";

/// A request received by the fake, in arrival order.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedRequest {
    /// HTTP method, e.g. `POST`.
    pub method: String,
    /// Path as sent, still percent-encoded.
    pub path: String,
    /// Raw query string, if any.
    pub query: Option<String>,
    /// JSON body, if any.
    pub body: Option<Value>,
}

impl RecordedRequest {
    /// `"METHOD /path"`, convenient for asserting call sequences.
    #[must_use]
    pub fn line(&self) -> String {
        format!("{} {}", self.method, self.path)
    }
}

/// A failure returned instead of (or after) handling a request.
#[derive(Debug, Clone)]
pub struct Fault {
    /// Status to return, e.g. `429` or `503`.
    pub status: u16,
    /// `Retry-After` header value.
    pub retry_after: Option<String>,
    /// Only match requests with this method.
    pub method: Option<Method>,
    /// Apply the request before failing, simulating a lost response.
    pub apply: bool,
}

impl Fault {
    /// Fails the next matching request with `status` without applying it.
    #[must_use]
    pub fn status(status: u16) -> Self {
        Self {
            status,
            retry_after: None,
            method: None,
            apply: false,
        }
    }

    /// Adds a `Retry-After` header.
    #[must_use]
    pub fn retry_after(mut self, value: impl Into<String>) -> Self {
        self.retry_after = Some(value.into());
        self
    }

    /// Restricts the fault to one method.
    #[must_use]
    pub fn on(mut self, method: Method) -> Self {
        self.method = Some(method);
        self
    }

    /// Applies the request, then returns the failure.
    #[must_use]
    pub fn after_apply(mut self) -> Self {
        self.apply = true;
        self
    }
}

#[derive(Debug)]
struct State {
    api_key: String,
    profiles: BTreeMap<String, Value>,
    next_id: u32,
    page_size: Option<usize>,
    faults: VecDeque<Fault>,
    latency: Duration,
    requests: Vec<RecordedRequest>,
    profile_extra: Map<String, Value>,
}

type Shared = Arc<Mutex<State>>;

fn lock(state: &Shared) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Builder for [`FakeApi`].
#[derive(Debug, Default)]
#[must_use]
pub struct FakeApiBuilder {
    api_key: Option<String>,
    page_size: Option<usize>,
    profile_extra: Map<String, Value>,
}

impl FakeApiBuilder {
    /// Accepted API key. Default [`TEST_KEY`].
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Maximum items per page on paginated endpoints. Default: unlimited.
    pub fn page_size(mut self, size: usize) -> Self {
        self.page_size = Some(size.max(1));
        self
    }

    /// Extra top-level field added to every `GET /profiles/:profile`
    /// response, to exercise unknown-field handling.
    pub fn profile_extra(mut self, key: impl Into<String>, value: Value) -> Self {
        self.profile_extra.insert(key.into(), value);
        self
    }

    /// Starts the server on `127.0.0.1` with an ephemeral port.
    ///
    /// # Errors
    ///
    /// If the listener cannot be bound.
    pub async fn start(self) -> io::Result<FakeApi> {
        let state = Arc::new(Mutex::new(State {
            api_key: self.api_key.unwrap_or_else(|| TEST_KEY.to_owned()),
            profiles: BTreeMap::new(),
            next_id: 0,
            page_size: self.page_size,
            faults: VecDeque::new(),
            latency: Duration::ZERO,
            requests: Vec::new(),
            profile_extra: self.profile_extra,
        }));
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = listener.local_addr()?;
        let app = Router::new()
            .fallback(handle)
            .with_state(Arc::clone(&state));
        let task = tokio::spawn(async move {
            // Serving only stops when the task is aborted on drop.
            let _ = axum::serve(listener, app).await;
        });
        Ok(FakeApi { addr, state, task })
    }
}

/// A running fake NextDNS API. Stops when dropped.
#[derive(Debug)]
pub struct FakeApi {
    addr: SocketAddr,
    state: Shared,
    task: JoinHandle<()>,
}

impl Drop for FakeApi {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FakeApi {
    /// Starts a [`FakeApiBuilder`].
    pub fn builder() -> FakeApiBuilder {
        FakeApiBuilder::default()
    }

    /// Starts a fake with default settings.
    ///
    /// # Errors
    ///
    /// If the listener cannot be bound.
    pub async fn start() -> io::Result<Self> {
        Self::builder().start().await
    }

    /// Base URL, e.g. `http://127.0.0.1:41234`.
    #[must_use]
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Stores a profile directly, filling missing fields with defaults.
    /// Returns its ID.
    #[must_use]
    pub fn insert_profile(&self, profile: &Value) -> String {
        let mut s = lock(&self.state);
        let id = s.allocate_id();
        let mut full = default_profile();
        merge_lenient(&mut full, profile);
        s.profiles.insert(id.clone(), full);
        id
    }

    /// The stored profile, without `id`.
    #[must_use]
    pub fn profile(&self, id: &str) -> Option<Value> {
        lock(&self.state).profiles.get(id).cloned()
    }

    /// Every request received so far.
    #[must_use]
    pub fn requests(&self) -> Vec<RecordedRequest> {
        lock(&self.state).requests.clone()
    }

    /// Request lines (`"METHOD /path"`) received so far.
    #[must_use]
    pub fn request_lines(&self) -> Vec<String> {
        lock(&self.state)
            .requests
            .iter()
            .map(RecordedRequest::line)
            .collect()
    }

    /// Forgets recorded requests.
    pub fn clear_requests(&self) {
        lock(&self.state).requests.clear();
    }

    /// Queues a fault for the next matching request.
    pub fn inject(&self, fault: Fault) {
        lock(&self.state).faults.push_back(fault);
    }

    /// Delay added before every response.
    pub fn set_latency(&self, latency: Duration) {
        lock(&self.state).latency = latency;
    }
}

impl State {
    fn allocate_id(&mut self) -> String {
        self.next_id += 1;
        format!("{:06x}", 0x00a0_0000 + self.next_id)
    }

    fn take_fault(&mut self, method: &Method) -> Option<Fault> {
        let i = self
            .faults
            .iter()
            .position(|f| f.method.as_ref().is_none_or(|m| m == method))?;
        self.faults.remove(i)
    }

    fn name_taken(&self, name: &str, except: Option<&str>) -> bool {
        self.profiles
            .iter()
            .any(|(id, p)| Some(id.as_str()) != except && p["name"] == name)
    }
}

async fn handle(
    AxumState(state): AxumState<Shared>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let latency = lock(&state).latency;
    if !latency.is_zero() {
        tokio::time::sleep(latency).await;
    }

    let mut s = lock(&state);
    let parsed_body = if body.is_empty() {
        None
    } else {
        serde_json::from_slice::<Value>(&body).ok()
    };
    s.requests.push(RecordedRequest {
        method: method.to_string(),
        path: uri.path().to_owned(),
        query: uri.query().map(str::to_owned),
        body: parsed_body.clone(),
    });

    let key_ok = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|k| k == s.api_key);
    if !key_ok {
        return errors(StatusCode::FORBIDDEN, "forbidden", None, None);
    }

    let fault = s.take_fault(&method);
    if let Some(f) = fault.as_ref().filter(|f| !f.apply) {
        return fault_response(f);
    }
    if !body.is_empty() && parsed_body.is_none() {
        return errors(
            StatusCode::BAD_REQUEST,
            "invalid",
            Some("malformed JSON"),
            Some("/"),
        );
    }

    let segments: Vec<String> = uri
        .path()
        .trim_matches('/')
        .split('/')
        .map(percent_decode)
        .collect();
    let query = parse_query(uri.query());
    let response = route(&mut s, &method, &segments, &query, parsed_body);
    match fault {
        Some(f) => fault_response(&f),
        None => response,
    }
}

fn route(
    s: &mut State,
    method: &Method,
    segments: &[String],
    query: &BTreeMap<String, String>,
    body: Option<Value>,
) -> Response {
    match segments {
        [p] if p == "profiles" => match *method {
            Method::GET => {
                let items: Vec<Value> = s
                    .profiles
                    .iter()
                    .map(|(id, p)| json!({ "id": id, "name": p["name"] }))
                    .collect();
                page(&items, query, s.page_size)
            }
            Method::POST => create_profile(s, body),
            _ => method_not_allowed(),
        },
        [p, id] if p == "profiles" => {
            if !s.profiles.contains_key(id) {
                return not_found();
            }
            match *method {
                Method::GET => {
                    let mut out = s.profiles[id].clone();
                    if let Value::Object(m) = &mut out {
                        m.insert("id".into(), json!(id));
                        m.extend(s.profile_extra.clone());
                    }
                    data(out)
                }
                Method::PATCH => patch_profile(s, id, body),
                Method::DELETE => {
                    s.profiles.remove(id);
                    no_content()
                }
                _ => method_not_allowed(),
            }
        }
        [p, id, rest @ ..] if p == "profiles" => {
            let page_size = s.page_size;
            match s.profiles.get_mut(id) {
                Some(profile) => child(profile, method, rest, query, body, page_size),
                None => not_found(),
            }
        }
        _ => not_found(),
    }
}

fn create_profile(s: &mut State, body: Option<Value>) -> Response {
    let Some(body @ Value::Object(_)) = body else {
        return errors(
            StatusCode::BAD_REQUEST,
            "invalid",
            Some("expected an object"),
            Some("/"),
        );
    };
    if let Some(name) = body["name"].as_str()
        && s.name_taken(name, None)
    {
        return user_error("duplicate", "a profile with this name exists", "/name");
    }
    for list in ["denylist", "allowlist"] {
        if let Some(items) = body[list].as_array()
            && let Err(r) = validate_items(list, items, &format!("/{list}"))
        {
            return r;
        }
    }
    let mut full = default_profile();
    if let Err(r) = merge_strict(&mut full, &body, "") {
        return r;
    }
    let id = s.allocate_id();
    s.profiles.insert(id.clone(), full);
    data(json!({ "id": id }))
}

fn patch_profile(s: &mut State, id: &str, body: Option<Value>) -> Response {
    let Some(body @ Value::Object(_)) = body else {
        return errors(
            StatusCode::BAD_REQUEST,
            "invalid",
            Some("expected an object"),
            Some("/"),
        );
    };
    if let Some(name) = body["name"].as_str()
        && s.name_taken(name, Some(id))
    {
        return user_error("duplicate", "a profile with this name exists", "/name");
    }
    let Some(profile) = s.profiles.get_mut(id) else {
        return not_found();
    };
    let mut updated = profile.clone();
    if let Err(r) = merge_strict(&mut updated, &body, "") {
        return r;
    }
    *profile = updated;
    no_content()
}

fn child(
    profile: &mut Value,
    method: &Method,
    rest: &[String],
    query: &BTreeMap<String, String>,
    body: Option<Value>,
    page_size: Option<usize>,
) -> Response {
    let mut cur = profile;
    let mut list_name = "";
    for (i, seg) in rest.iter().enumerate() {
        match cur {
            Value::Object(m) => match m.get_mut(seg) {
                Some(next) => {
                    list_name = seg;
                    cur = next;
                }
                None => return not_found(),
            },
            Value::Array(items) if i + 1 == rest.len() => {
                return item(items, list_name, seg, method, body);
            }
            _ => return not_found(),
        }
    }
    match cur {
        Value::Object(_) => match *method {
            Method::GET => data(cur.clone()),
            Method::PATCH => {
                let Some(body @ Value::Object(_)) = body else {
                    return errors(
                        StatusCode::BAD_REQUEST,
                        "invalid",
                        Some("expected an object"),
                        Some("/"),
                    );
                };
                let mut updated = cur.clone();
                if let Err(r) = merge_strict(&mut updated, &body, "") {
                    return r;
                }
                *cur = updated;
                no_content()
            }
            _ => method_not_allowed(),
        },
        Value::Array(items) => match *method {
            Method::GET => page(items, query, page_size),
            Method::PUT => {
                let Some(Value::Array(new)) = body else {
                    return errors(
                        StatusCode::BAD_REQUEST,
                        "invalid",
                        Some("expected an array"),
                        Some("/"),
                    );
                };
                if let Err(r) = validate_items(list_name, &new, "") {
                    return r;
                }
                *items = new;
                no_content()
            }
            Method::POST => {
                let Some(new) = body else {
                    return errors(
                        StatusCode::BAD_REQUEST,
                        "invalid",
                        Some("expected an object"),
                        Some("/"),
                    );
                };
                if let Err(r) = validate_items(list_name, std::slice::from_ref(&new), "") {
                    return r;
                }
                if items.iter().any(|i| i["id"] == new["id"]) {
                    return user_error("duplicate", "item already exists", "/id");
                }
                items.push(new);
                no_content()
            }
            _ => method_not_allowed(),
        },
        _ => not_found(),
    }
}

fn item(
    items: &mut Vec<Value>,
    list_name: &str,
    id: &str,
    method: &Method,
    body: Option<Value>,
) -> Response {
    let Some(i) = items.iter().position(|v| v["id"] == id) else {
        return not_found();
    };
    match *method {
        Method::PATCH => {
            let Some(body @ Value::Object(_)) = body else {
                return errors(
                    StatusCode::BAD_REQUEST,
                    "invalid",
                    Some("expected an object"),
                    Some("/"),
                );
            };
            let mut updated = items[i].clone();
            if let Err(r) = merge_strict(&mut updated, &body, "") {
                return r;
            }
            if let Err(r) = validate_items(list_name, std::slice::from_ref(&updated), "") {
                return r;
            }
            items[i] = updated;
            no_content()
        }
        Method::DELETE => {
            items.remove(i);
            no_content()
        }
        _ => method_not_allowed(),
    }
}

/// Validates list items: every item needs a string `id`; domain lists need
/// valid domains (reported as a `200` user error, like the real API).
fn validate_items(list: &str, items: &[Value], prefix: &str) -> Result<(), Response> {
    for (i, item) in items.iter().enumerate() {
        let pointer = if items.len() == 1 && prefix.is_empty() {
            "/id".to_owned()
        } else {
            format!("{prefix}/{i}/id")
        };
        let Some(id) = item["id"].as_str() else {
            return Err(errors(
                StatusCode::BAD_REQUEST,
                "invalid",
                Some("id is required"),
                Some(&pointer),
            ));
        };
        if matches!(list, "denylist" | "allowlist") && !is_domain(id) {
            return Err(user_error(
                "invalid",
                &format!("{id:?} is not a valid domain"),
                &pointer,
            ));
        }
    }
    Ok(())
}

fn is_domain(s: &str) -> bool {
    let s = s.strip_suffix('.').unwrap_or(s);
    s.len() <= 253
        && s.contains('.')
        && s.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

/// Deep merge that rejects keys absent from `target`, like a strict API.
fn merge_strict(target: &mut Value, patch: &Value, pointer: &str) -> Result<(), Response> {
    let (Value::Object(fields), Value::Object(changes)) = (&mut *target, patch) else {
        *target = patch.clone();
        return Ok(());
    };
    for (k, v) in changes {
        let here = format!("{pointer}/{k}");
        match fields.get_mut(k) {
            Some(existing @ Value::Object(_)) if v.is_object() => merge_strict(existing, v, &here)?,
            Some(existing) => *existing = v.clone(),
            None => {
                return Err(errors(
                    StatusCode::BAD_REQUEST,
                    "invalid",
                    Some("unknown field"),
                    Some(&here),
                ));
            }
        }
    }
    Ok(())
}

/// Deep merge that accepts any key; used for test setup only.
fn merge_lenient(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(t), Value::Object(p)) => {
            for (k, v) in p {
                match t.get_mut(k) {
                    Some(existing) if existing.is_object() && v.is_object() => {
                        merge_lenient(existing, v);
                    }
                    _ => {
                        t.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (t, p) => *t = p.clone(),
    }
}

/// The documented profile shape with every flag off and lists empty.
#[must_use]
pub fn default_profile() -> Value {
    json!({
      "name": "",
      "security": {
        "threatIntelligenceFeeds": false, "aiThreatDetection": false,
        "googleSafeBrowsing": false, "cryptojacking": false, "dnsRebinding": false,
        "idnHomographs": false, "typosquatting": false, "dga": false, "nrd": false,
        "ddns": false, "parking": false, "csam": false, "tlds": []
      },
      "privacy": {
        "blocklists": [], "natives": [], "disguisedTrackers": false, "allowAffiliate": false
      },
      "parentalControl": {
        "services": [], "categories": [],
        "safeSearch": false, "youtubeRestrictedMode": false, "blockBypass": false
      },
      "denylist": [],
      "allowlist": [],
      "settings": {
        "logs": {
          "enabled": false, "drop": { "ip": false, "domain": false },
          "retention": 2_592_000, "location": "us"
        },
        "blockPage": { "enabled": false },
        "performance": { "ecs": false, "cacheBoost": false, "cnameFlattening": false },
        "web3": false
      }
    })
}

fn page(items: &[Value], query: &BTreeMap<String, String>, page_size: Option<usize>) -> Response {
    let start = match query.get("cursor") {
        None => 0,
        Some(c) => match c.strip_prefix('o').and_then(|n| n.parse::<usize>().ok()) {
            Some(n) if n <= items.len() => n,
            _ => return errors(StatusCode::BAD_REQUEST, "invalid", Some("bad cursor"), None),
        },
    };
    let limit = match query.get("limit").map(|l| l.parse::<usize>()) {
        None => None,
        Some(Ok(l)) if l >= 1 => Some(l),
        Some(_) => {
            return errors_param(StatusCode::BAD_REQUEST, "invalid", "limit");
        }
    };
    let size = match (limit, page_size) {
        (Some(l), Some(p)) => l.min(p),
        (Some(n), None) | (None, Some(n)) => n,
        (None, None) => usize::MAX,
    };
    let end = start.saturating_add(size).min(items.len());
    let cursor = (end < items.len()).then(|| format!("o{end}"));
    let body = json!({
        "data": &items[start..end],
        "meta": { "pagination": { "cursor": cursor } }
    });
    (StatusCode::OK, axum::Json(body)).into_response()
}

fn data(value: Value) -> Response {
    let body = Map::from_iter([("data".to_owned(), value)]);
    (StatusCode::OK, axum::Json(Value::Object(body))).into_response()
}

fn no_content() -> Response {
    StatusCode::NO_CONTENT.into_response()
}

fn user_error(code: &str, detail: &str, pointer: &str) -> Response {
    errors(StatusCode::OK, code, Some(detail), Some(pointer))
}

fn not_found() -> Response {
    errors(StatusCode::NOT_FOUND, "notFound", None, None)
}

fn method_not_allowed() -> Response {
    errors(
        StatusCode::METHOD_NOT_ALLOWED,
        "methodNotAllowed",
        None,
        None,
    )
}

fn errors(status: StatusCode, code: &str, detail: Option<&str>, pointer: Option<&str>) -> Response {
    let mut e = json!({ "code": code });
    if let Some(d) = detail {
        e["detail"] = json!(d);
    }
    if let Some(p) = pointer {
        e["source"] = json!({ "pointer": p });
    }
    (status, axum::Json(json!({ "errors": [e] }))).into_response()
}

fn errors_param(status: StatusCode, code: &str, parameter: &str) -> Response {
    let body = json!({ "errors": [{ "code": code, "source": { "parameter": parameter } }] });
    (status, axum::Json(body)).into_response()
}

fn fault_response(f: &Fault) -> Response {
    let status = StatusCode::from_u16(f.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut resp = (status, "injected fault").into_response();
    if let Some(ra) = f
        .retry_after
        .as_deref()
        .and_then(|v| HeaderValue::from_str(v).ok())
    {
        resp.headers_mut().insert(header::RETRY_AFTER, ra);
    }
    resp
}

fn parse_query(query: Option<&str>) -> BTreeMap<String, String> {
    query
        .unwrap_or_default()
        .split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| char::from(b).to_digit(16);
        if bytes[i] == b'%'
            && let (Some(&h), Some(&l)) = (bytes.get(i + 1), bytes.get(i + 2))
            && let (Some(h), Some(l)) = (hex(h), hex(l))
        {
            // Two hex digits always fit in a byte.
            out.push(u8::try_from(h * 16 + l).unwrap_or(b'?'));
            i += 3;
        } else {
            out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_validation() {
        assert!(is_domain("example.com"));
        assert!(is_domain("a-b.example.co.uk."));
        assert!(!is_domain("localhost"));
        assert!(!is_domain("Example.com"));
        assert!(!is_domain("bad domain.com"));
        assert!(!is_domain("-x.com"));
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%2Fb%3fc"), "a/b?c");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn strict_merge_rejects_unknown_keys() {
        let mut t = json!({ "a": { "b": 1 } });
        assert!(merge_strict(&mut t, &json!({ "a": { "b": 2 } }), "").is_ok());
        assert_eq!(t, json!({ "a": { "b": 2 } }));
        assert!(merge_strict(&mut t, &json!({ "a": { "c": 2 } }), "").is_err());
    }

    #[test]
    fn pagination_cursors() {
        let items: Vec<Value> = (0..5).map(|i| json!({ "id": i })).collect();
        let q = BTreeMap::from([("cursor".to_owned(), "o4".to_owned())]);
        let resp = page(&items, &q, Some(2));
        assert_eq!(resp.status(), StatusCode::OK);
        let bad = BTreeMap::from([("cursor".to_owned(), "zzz".to_owned())]);
        assert_eq!(
            page(&items, &bad, Some(2)).status(),
            StatusCode::BAD_REQUEST
        );
    }
}
