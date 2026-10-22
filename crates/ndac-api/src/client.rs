//! HTTP client for the NextDNS API.

use std::{
    collections::{BTreeSet, HashSet},
    fmt,
    net::IpAddr,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant, SystemTime},
};

use reqwest::{
    Method, StatusCode,
    header::{CONTENT_TYPE, HeaderName, HeaderValue, RETRY_AFTER},
    redirect,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use tokio::sync::Semaphore;
use url::{Host, Url};

use crate::{
    ApiErrorDetail, ApiKey, Error, ListPath, ObjectPath, Profile, ProfileSummary, RetryPolicy,
    retry::parse_retry_after,
};

/// Production API base URL.
pub const DEFAULT_BASE_URL: &str = "https://api.nextdns.io";

#[cfg(feature = "unstable-endpoints")]
mod unstable;

const X_API_KEY: HeaderName = HeaderName::from_static("x-api-key");
const MAX_PAGES: usize = 10_000;

/// Builder for [`Client`].
#[derive(Debug)]
#[must_use]
pub struct ClientBuilder {
    key: ApiKey,
    base_url: String,
    user_agent: String,
    timeout: Duration,
    connect_timeout: Duration,
    retry: RetryPolicy,
    concurrency: usize,
    deadline: Option<Duration>,
    page_limit: Option<u32>,
}

impl ClientBuilder {
    /// Starts a builder with production defaults.
    pub fn new(key: ApiKey) -> Self {
        Self {
            key,
            base_url: DEFAULT_BASE_URL.to_owned(),
            user_agent: format!(
                "ndac/{} (+https://github.com/Esl1h/ndac)",
                env!("CARGO_PKG_VERSION")
            ),
            timeout: Duration::from_secs(15),
            connect_timeout: Duration::from_secs(10),
            retry: RetryPolicy::default(),
            concurrency: 4,
            deadline: None,
            page_limit: None,
        }
    }

    /// Overrides the API base URL. Only `https` is accepted, except plain
    /// `http` to a loopback address for tests.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Sets the `User-Agent` header.
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    /// Per-request timeout, also used as the read timeout. Default 15s.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// TCP and TLS connect timeout. Default 10s.
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Retry policy. Default: 5 retries.
    pub fn retry_policy(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// Maximum requests in flight across all clones of the client.
    /// Default 4; values below 1 are raised to 1.
    pub fn concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency.max(1);
        self
    }

    /// Overall deadline, counted from [`build`](Self::build). No retry is
    /// started that would end after it.
    pub fn deadline(mut self, deadline: Duration) -> Self {
        self.deadline = Some(deadline);
        self
    }

    /// `limit` query parameter sent on paginated endpoints. Default: the
    /// server's default.
    pub fn page_limit(mut self, limit: u32) -> Self {
        self.page_limit = Some(limit);
        self
    }

    /// Builds the client.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidBaseUrl`] for a disallowed base URL,
    /// [`Error::InvalidApiKey`] if the key is not a valid header value, and
    /// [`Error::Build`] if the TLS stack cannot be initialized.
    pub fn build(self) -> Result<Client, Error> {
        let base = validate_base_url(&self.base_url)?;
        let key = self.key.header_value()?;

        // reqwest is built without a default crypto provider; an already
        // installed provider (e.g. by an embedding application) is kept.
        let _ = rustls::crypto::ring::default_provider().install_default();

        let http = reqwest::Client::builder()
            .user_agent(self.user_agent)
            .connect_timeout(self.connect_timeout)
            .read_timeout(self.timeout)
            .https_only(base.scheme() == "https")
            // A redirect could forward X-Api-Key to another host.
            .redirect(redirect::Policy::none())
            .build()
            .map_err(Error::Build)?;

        Ok(Client {
            inner: Arc::new(Inner {
                http,
                base,
                key,
                timeout: self.timeout,
                retry: self.retry,
                concurrency: self.concurrency,
                semaphore: Semaphore::new(self.concurrency),
                deadline: self.deadline.map(|d| Instant::now() + d),
                page_limit: self.page_limit,
                reported_unknown: Mutex::new(BTreeSet::new()),
            }),
        })
    }
}

/// NextDNS API client. Cheap to clone; clones share the connection pool and
/// the concurrency limit.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

struct Inner {
    http: reqwest::Client,
    base: Url,
    key: HeaderValue,
    timeout: Duration,
    retry: RetryPolicy,
    concurrency: usize,
    semaphore: Semaphore,
    deadline: Option<Instant>,
    page_limit: Option<u32>,
    reported_unknown: Mutex<BTreeSet<String>>,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("base_url", &self.inner.base.as_str())
            .field("api_key", &"<redacted>")
            .field("timeout", &self.inner.timeout)
            .field("retry", &self.inner.retry)
            .field("concurrency", &self.inner.concurrency)
            .finish_non_exhaustive()
    }
}

struct Request<'a> {
    method: Method,
    url: Url,
    endpoint: &'static str,
    body: Option<&'a [u8]>,
    retry: bool,
}

#[derive(Debug, Deserialize)]
struct RawEnvelope {
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    meta: Option<Meta>,
    #[serde(default)]
    errors: Option<Vec<ApiErrorDetail>>,
}

#[derive(Debug, Default, Deserialize)]
struct Meta {
    #[serde(default)]
    pagination: Option<Pagination>,
}

#[derive(Debug, Deserialize)]
struct Pagination {
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Default)]
struct Envelope {
    data: Option<Value>,
    meta: Option<Meta>,
}

#[derive(Deserialize)]
struct Created {
    id: String,
}

#[derive(Deserialize)]
struct ItemId {
    id: String,
}

impl Client {
    /// Starts a [`ClientBuilder`].
    pub fn builder(key: ApiKey) -> ClientBuilder {
        ClientBuilder::new(key)
    }

    /// The API base URL in use.
    #[must_use]
    pub fn base_url(&self) -> &Url {
        &self.inner.base
    }

    /// `GET /profiles`, following pagination.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from the request or response decoding.
    pub async fn list_profiles(&self) -> Result<Vec<ProfileSummary>, Error> {
        self.get_all(self.url(&["profiles"]), "/profiles").await
    }

    /// `GET /profiles/:profile`.
    ///
    /// Unknown fields are preserved in the returned model and logged once per
    /// client at `debug` level.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from the request or response decoding.
    pub async fn get_profile(&self, profile: &str) -> Result<Profile, Error> {
        const ENDPOINT: &str = "/profiles/:profile";
        let env = self
            .send(&Self::get(self.url(&["profiles", profile]), ENDPOINT))
            .await?;
        let p: Profile = decode(env.data, ENDPOINT)?;
        self.report_unknown(&p.unknown_fields());
        Ok(p)
    }

    /// `POST /profiles`. Returns the new profile ID.
    ///
    /// Never retried: a lost response could otherwise create duplicates.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from the request or response decoding.
    pub async fn create_profile(&self, profile: &Profile) -> Result<String, Error> {
        const ENDPOINT: &str = "/profiles";
        let body = encode(profile)?;
        let env = self
            .send(&Request {
                method: Method::POST,
                url: self.url(&["profiles"]),
                endpoint: ENDPOINT,
                body: Some(&body),
                retry: false,
            })
            .await?;
        Ok(decode::<Created>(env.data, ENDPOINT)?.id)
    }

    /// `PATCH /profiles/:profile`, e.g. to rename it.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from the request.
    pub async fn patch_profile(&self, profile: &str, body: &impl Serialize) -> Result<(), Error> {
        let body = encode(body)?;
        self.send(&Self::write(
            Method::PATCH,
            self.url(&["profiles", profile]),
            "/profiles/:profile",
            &body,
        ))
        .await
        .map(drop)
    }

    /// `DELETE /profiles/:profile`.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from the request.
    pub async fn delete_profile(&self, profile: &str) -> Result<(), Error> {
        self.delete(self.url(&["profiles", profile]), "/profiles/:profile")
            .await
    }

    /// `GET` an object endpoint.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from the request or response decoding.
    pub async fn get_object<T: DeserializeOwned>(
        &self,
        profile: &str,
        path: ObjectPath,
    ) -> Result<T, Error> {
        let env = self
            .send(&Self::get(
                self.profile_url(profile, path.segments(), None),
                path.template(),
            ))
            .await?;
        decode(env.data, path.template())
    }

    /// `PATCH` an object endpoint with a partial body.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from the request.
    pub async fn patch_object(
        &self,
        profile: &str,
        path: ObjectPath,
        body: &impl Serialize,
    ) -> Result<(), Error> {
        let body = encode(body)?;
        self.send(&Self::write(
            Method::PATCH,
            self.profile_url(profile, path.segments(), None),
            path.template(),
            &body,
        ))
        .await
        .map(drop)
    }

    /// `GET` an array endpoint, following pagination.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from the request or response decoding.
    pub async fn get_list<T: DeserializeOwned>(
        &self,
        profile: &str,
        path: ListPath,
    ) -> Result<Vec<T>, Error> {
        self.get_all(
            self.profile_url(profile, path.segments(), None),
            path.template(),
        )
        .await
    }

    /// `PUT` an array endpoint, replacing the whole array.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from the request.
    pub async fn put_list<T: Serialize>(
        &self,
        profile: &str,
        path: ListPath,
        items: &[T],
    ) -> Result<(), Error> {
        let body = encode(&items)?;
        self.send(&Self::write(
            Method::PUT,
            self.profile_url(profile, path.segments(), None),
            path.template(),
            &body,
        ))
        .await
        .map(drop)
    }

    /// `POST` one item to an array endpoint.
    ///
    /// `POST` is not idempotent, so after a retryable failure the list is
    /// read back and the item is posted again only if it is absent.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from the requests.
    pub async fn add_item(
        &self,
        profile: &str,
        path: ListPath,
        item_id: &str,
        item: &impl Serialize,
    ) -> Result<(), Error> {
        let body = encode(item)?;
        let req = Request {
            method: Method::POST,
            url: self.profile_url(profile, path.segments(), None),
            endpoint: path.template(),
            body: Some(&body),
            retry: false,
        };
        let mut retry = 0;
        loop {
            let (result, retry_after) = self.attempt(&req).await;
            let err = match result {
                Ok(_) => return Ok(()),
                Err(e) if e.is_retryable() => e,
                Err(e) => return Err(e),
            };
            let present = self
                .get_list::<ItemId>(profile, path)
                .await?
                .iter()
                .any(|i| i.id == item_id);
            if present {
                tracing::debug!(endpoint = req.endpoint, "item present after failed POST");
                return Ok(());
            }
            self.wait_before_retry(req.endpoint, retry, retry_after, err)
                .await?;
            retry += 1;
        }
    }

    /// `PATCH` one item of an array endpoint.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from the request.
    pub async fn patch_item(
        &self,
        profile: &str,
        path: ListPath,
        item_id: &str,
        body: &impl Serialize,
    ) -> Result<(), Error> {
        let body = encode(body)?;
        self.send(&Self::write(
            Method::PATCH,
            self.profile_url(profile, path.segments(), Some(item_id)),
            path.item_template(),
            &body,
        ))
        .await
        .map(drop)
    }

    /// `DELETE` one item of an array endpoint.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from the request.
    pub async fn delete_item(
        &self,
        profile: &str,
        path: ListPath,
        item_id: &str,
    ) -> Result<(), Error> {
        self.delete(
            self.profile_url(profile, path.segments(), Some(item_id)),
            path.item_template(),
        )
        .await
    }

    fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.inner.base.clone();
        #[allow(
            clippy::expect_used,
            reason = "validate_base_url only accepts http(s) URLs, which can be a base"
        )]
        url.path_segments_mut()
            .expect("base URL can be a base")
            .pop_if_empty()
            .extend(segments);
        url
    }

    fn profile_url(&self, profile: &str, segments: &[&str], item: Option<&str>) -> Url {
        let mut all = Vec::with_capacity(segments.len() + 3);
        all.push("profiles");
        all.push(profile);
        all.extend_from_slice(segments);
        all.extend(item);
        // Segments are percent-encoded, so an item ID cannot add path levels.
        self.url(&all)
    }

    fn get(url: Url, endpoint: &'static str) -> Request<'static> {
        Request {
            method: Method::GET,
            url,
            endpoint,
            body: None,
            retry: true,
        }
    }

    fn write<'a>(method: Method, url: Url, endpoint: &'static str, body: &'a [u8]) -> Request<'a> {
        Request {
            method,
            url,
            endpoint,
            body: Some(body),
            retry: true,
        }
    }

    async fn delete(&self, url: Url, endpoint: &'static str) -> Result<(), Error> {
        let req = Request {
            method: Method::DELETE,
            url,
            endpoint,
            body: None,
            retry: true,
        };
        let mut retry = 0;
        loop {
            let (result, retry_after) = self.attempt(&req).await;
            match result {
                Ok(_) => return Ok(()),
                // An earlier attempt may have succeeded without us seeing
                // the response.
                Err(Error::NotFound { .. }) if retry > 0 => return Ok(()),
                Err(e) if e.is_retryable() => {
                    self.wait_before_retry(endpoint, retry, retry_after, e)
                        .await?;
                    retry += 1;
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn get_all<T: DeserializeOwned>(
        &self,
        url: Url,
        endpoint: &'static str,
    ) -> Result<Vec<T>, Error> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        let mut seen = HashSet::new();
        for _ in 0..MAX_PAGES {
            let mut page_url = url.clone();
            if self.inner.page_limit.is_some() || cursor.is_some() {
                let mut q = page_url.query_pairs_mut();
                if let Some(limit) = self.inner.page_limit {
                    q.append_pair("limit", &limit.to_string());
                }
                if let Some(c) = &cursor {
                    q.append_pair("cursor", c);
                }
            }
            let env = self.send(&Self::get(page_url, endpoint)).await?;
            let next = env
                .meta
                .and_then(|m| m.pagination)
                .and_then(|p| p.cursor)
                .filter(|c| !c.is_empty());
            out.extend(decode::<Vec<T>>(env.data, endpoint)?);
            match next {
                None => return Ok(out),
                Some(c) if !seen.insert(c.clone()) => return Err(Error::Pagination { endpoint }),
                Some(c) => cursor = Some(c),
            }
        }
        Err(Error::Pagination { endpoint })
    }

    async fn send(&self, req: &Request<'_>) -> Result<Envelope, Error> {
        let mut retry = 0;
        loop {
            let (result, retry_after) = self.attempt(req).await;
            match result {
                Err(e) if req.retry && e.is_retryable() => {
                    self.wait_before_retry(req.endpoint, retry, retry_after, e)
                        .await?;
                    retry += 1;
                }
                other => return other,
            }
        }
    }

    /// Sleeps before retry number `retry`, or returns the error to give up
    /// with if the retry budget or the deadline does not allow another try.
    async fn wait_before_retry(
        &self,
        endpoint: &'static str,
        retry: u32,
        retry_after: Option<Duration>,
        err: Error,
    ) -> Result<(), Error> {
        let attempts = retry + 1;
        if retry >= self.inner.retry.max_retries {
            return Err(if retry == 0 {
                err
            } else {
                Error::RetriesExhausted {
                    attempts,
                    last: Box::new(err),
                }
            });
        }
        let delay = self.inner.retry.delay(retry, retry_after);
        if let Some(deadline) = self.inner.deadline
            && Instant::now() + delay >= deadline
        {
            return Err(Error::DeadlineExceeded {
                attempts,
                last: Box::new(err),
            });
        }
        tracing::info!(
            endpoint,
            attempt = attempts,
            delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
            error = %err,
            "retrying"
        );
        tokio::time::sleep(delay).await;
        Ok(())
    }

    /// One HTTP exchange. Returns the server's `Retry-After`, if any.
    async fn attempt(&self, req: &Request<'_>) -> (Result<Envelope, Error>, Option<Duration>) {
        let mut timeout = self.inner.timeout;
        if let Some(deadline) = self.inner.deadline {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                let err = Error::DeadlineExceeded {
                    attempts: 0,
                    last: Box::new(Error::Status {
                        endpoint: req.endpoint,
                        status: StatusCode::REQUEST_TIMEOUT,
                    }),
                };
                return (Err(err), None);
            }
            timeout = timeout.min(left);
        }

        // The semaphore is never closed, so acquiring cannot fail.
        let _permit = self.inner.semaphore.acquire().await.ok();
        let mut builder = self
            .inner
            .http
            .request(req.method.clone(), req.url.clone())
            .header(X_API_KEY, self.inner.key.clone())
            .timeout(timeout);
        if let Some(body) = req.body {
            builder = builder
                .header(CONTENT_TYPE, "application/json")
                .body(body.to_vec());
        }

        let started = Instant::now();
        let transport = |source| Error::Transport {
            endpoint: req.endpoint,
            source: reqwest::Error::without_url(source),
        };
        let resp = match builder.send().await {
            Ok(r) => r,
            Err(e) => return (Err(transport(e)), None),
        };
        let status = resp.status();
        let retry_after = resp
            .headers()
            .get(RETRY_AFTER)
            .and_then(|v| parse_retry_after(v, SystemTime::now()));
        let body = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => return (Err(transport(e)), retry_after),
        };
        tracing::debug!(
            method = %req.method,
            endpoint = req.endpoint,
            status = status.as_u16(),
            elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "api call"
        );
        (parse_response(req.endpoint, status, &body), retry_after)
    }

    fn report_unknown(&self, fields: &[String]) {
        if fields.is_empty() {
            return;
        }
        let mut reported = self
            .inner
            .reported_unknown
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for f in fields {
            if reported.insert(f.clone()) {
                tracing::debug!(field = %f, "ignoring unknown field in API response");
            }
        }
    }
}

fn parse_response(
    endpoint: &'static str,
    status: StatusCode,
    body: &[u8],
) -> Result<Envelope, Error> {
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(Error::Unauthorized { endpoint, status });
    }
    if status == StatusCode::NOT_FOUND {
        return Err(Error::NotFound { endpoint });
    }
    if body.iter().all(u8::is_ascii_whitespace) {
        return if status.is_success() {
            Ok(Envelope::default())
        } else {
            Err(Error::Status { endpoint, status })
        };
    }
    let raw: RawEnvelope = match serde_json::from_slice(body) {
        Ok(raw) => raw,
        Err(_) if !status.is_success() => return Err(Error::Status { endpoint, status }),
        Err(source) => return Err(Error::Decode { endpoint, source }),
    };
    // NextDNS reports user errors as 200 with an `errors` array.
    if let Some(errors) = raw.errors.filter(|e| !e.is_empty()) {
        return Err(Error::Api {
            endpoint,
            status,
            errors,
        });
    }
    if !status.is_success() {
        return Err(Error::Status { endpoint, status });
    }
    Ok(Envelope {
        data: raw.data,
        meta: raw.meta,
    })
}

fn decode<T: DeserializeOwned>(data: Option<Value>, endpoint: &'static str) -> Result<T, Error> {
    serde_json::from_value(data.unwrap_or(Value::Null))
        .map_err(|source| Error::Decode { endpoint, source })
}

fn encode(body: &impl Serialize) -> Result<Vec<u8>, Error> {
    serde_json::to_vec(body).map_err(|source| Error::Decode {
        endpoint: "<request body>",
        source,
    })
}

pub(crate) fn validate_base_url(raw: &str) -> Result<Url, Error> {
    let invalid = |why: &str| Error::InvalidBaseUrl(format!("{raw}: {why}"));
    let url = Url::parse(raw).map_err(|e| invalid(&e.to_string()))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid("credentials are not allowed"));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(invalid("query and fragment are not allowed"));
    }
    if !matches!(url.path(), "" | "/") {
        return Err(invalid("a path is not allowed"));
    }
    match url.scheme() {
        "https" => Ok(url),
        "http" if is_loopback(&url) => Ok(url),
        "http" => Err(invalid("plain http is only allowed to a loopback address")),
        _ => Err(invalid("scheme must be https")),
    }
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
        Some(Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
        Some(Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_rules() {
        assert!(validate_base_url("https://api.nextdns.io").is_ok());
        assert!(validate_base_url("https://api.nextdns.io/").is_ok());
        assert!(validate_base_url("http://127.0.0.1:8080").is_ok());
        assert!(validate_base_url("http://[::1]:8080").is_ok());
        assert!(validate_base_url("http://localhost:1").is_ok());
        for bad in [
            "http://api.nextdns.io",
            "http://10.0.0.1",
            "ftp://api.nextdns.io",
            "https://user:pw@api.nextdns.io",
            "https://api.nextdns.io/v1",
            "https://api.nextdns.io/?x=1",
            "not a url",
        ] {
            assert!(
                matches!(validate_base_url(bad), Err(Error::InvalidBaseUrl(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn errors_array_fails_even_with_200() {
        let body = br#"{"errors":[{"code":"invalid","source":{"pointer":"/id"}}]}"#;
        let err = parse_response("/e", StatusCode::OK, body).unwrap_err();
        assert!(matches!(
            err,
            Error::Api {
                status: StatusCode::OK,
                ..
            }
        ));
    }

    #[test]
    fn empty_errors_array_is_success() {
        let env = parse_response("/e", StatusCode::OK, br#"{"data":1,"errors":[]}"#).expect("ok");
        assert_eq!(env.data, Some(Value::from(1)));
    }

    #[test]
    fn status_mapping() {
        let err = |s, b: &[u8]| parse_response("/e", s, b).unwrap_err();
        assert!(matches!(
            err(StatusCode::FORBIDDEN, b"{}"),
            Error::Unauthorized { .. }
        ));
        assert!(matches!(
            err(StatusCode::UNAUTHORIZED, b""),
            Error::Unauthorized { .. }
        ));
        assert!(matches!(
            err(StatusCode::NOT_FOUND, b""),
            Error::NotFound { .. }
        ));
        assert!(matches!(
            err(StatusCode::BAD_GATEWAY, b"<html>"),
            Error::Status { .. }
        ));
        assert!(matches!(
            err(StatusCode::OK, b"<html>"),
            Error::Decode { .. }
        ));
        assert!(matches!(
            err(
                StatusCode::BAD_REQUEST,
                br#"{"errors":[{"code":"invalid"}]}"#
            ),
            Error::Api {
                status: StatusCode::BAD_REQUEST,
                ..
            }
        ));
    }

    #[test]
    fn empty_body_on_success_has_no_data() {
        let env = parse_response("/e", StatusCode::NO_CONTENT, b" \n").expect("ok");
        assert!(env.data.is_none());
    }

    #[test]
    fn item_ids_are_percent_encoded_as_one_segment() {
        let client = Client::builder(ApiKey::new("test-key-0000000000000000000000000000000000"))
            .build()
            .expect("client");
        let url = client.profile_url("abc123", ListPath::Denylist.segments(), Some("a/b?c"));
        assert_eq!(
            url.as_str(),
            "https://api.nextdns.io/profiles/abc123/denylist/a%2Fb%3Fc"
        );
    }

    #[test]
    fn debug_never_shows_the_key() {
        let key = "test-key-0000000000000000000000000000009876";
        let client = Client::builder(ApiKey::new(key)).build().expect("client");
        let dbg = format!("{client:?}");
        assert!(!dbg.contains("9876"), "{dbg}");
        assert!(dbg.contains("<redacted>"));
    }
}
