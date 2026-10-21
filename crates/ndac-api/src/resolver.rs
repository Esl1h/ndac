//! The NextDNS resolver test at <https://test.nextdns.io>, which reports
//! whether and how this host's DNS queries reach NextDNS. It is not part of
//! the API and never receives the API key.

use std::{collections::BTreeMap, time::Duration};

use reqwest::redirect;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::{Error, client::validate_base_url};

/// Production test URL.
pub const TEST_URL: &str = "https://test.nextdns.io";

const ENDPOINT: &str = "test.nextdns.io";

/// The test result. Fields were checked against live responses; all but
/// `status` are optional because they are not documented.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolverTest {
    /// `ok` when queries reach NextDNS.
    pub status: String,
    /// Transport, e.g. `DOH`, `DOT` or `UDP`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    /// Profile, as NextDNS chooses to show it (an ID or a `fp…`
    /// fingerprint).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Client address seen by NextDNS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    /// Device name sent with the queries, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    /// NextDNS server that answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    /// Whether the anycast network was used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anycast: Option<bool>,
    /// Fields not modeled above, such as `srcIP` and `destIP`.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Runs the test. Each run asks a fresh random subdomain, so the lookup
/// cannot be answered from a cache; the bare host only redirects there for
/// some clients and serves a script page to others. Loopback URLs (tests)
/// are used as given. Only redirects to the same host or its subdomains are
/// followed.
///
/// # Errors
///
/// Invalid URL, transport failure, non-success status or a body that is
/// not a test result.
pub async fn test_resolver(
    url: &str,
    user_agent: &str,
    timeout: Duration,
) -> Result<ResolverTest, Error> {
    let mut base = validate_base_url(url)?;
    let host = base.host_str().unwrap_or_default().to_ascii_lowercase();
    if base.scheme() == "https" {
        let label: String = std::iter::repeat_with(fastrand::lowercase)
            .take(20)
            .collect();
        base.set_host(Some(&format!("{label}.{host}")))
            .map_err(|e| Error::InvalidBaseUrl(format!("{url}: {e}")))?;
    }
    let scheme = base.scheme().to_owned();
    let policy = redirect::Policy::custom(move |attempt| {
        let ok = allowed(attempt.url(), &scheme, &host);
        if attempt.previous().len() >= 3 {
            attempt.error("too many redirects")
        } else if ok {
            attempt.follow()
        } else {
            attempt.stop()
        }
    });
    let _ = rustls::crypto::ring::default_provider().install_default();
    let http = reqwest::Client::builder()
        .user_agent(user_agent)
        .timeout(timeout)
        .redirect(policy)
        .build()
        .map_err(Error::Build)?;
    let transport = |source| Error::Transport {
        endpoint: ENDPOINT,
        source,
    };
    let resp = http.get(base).send().await.map_err(transport)?;
    let status = resp.status();
    if !status.is_success() {
        return Err(Error::Status {
            endpoint: ENDPOINT,
            status,
        });
    }
    let body = resp.bytes().await.map_err(transport)?;
    serde_json::from_slice(&body).map_err(|source| Error::Decode {
        endpoint: ENDPOINT,
        source,
    })
}

fn allowed(url: &Url, scheme: &str, host: &str) -> bool {
    let target = url.host_str().unwrap_or_default().to_ascii_lowercase();
    url.scheme() == scheme
        && (target == host
            || target
                .strip_suffix(host)
                .is_some_and(|rest| rest.ends_with('.')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_targets() {
        let ok = |u: &str| allowed(&Url::parse(u).unwrap(), "https", "test.nextdns.io");
        assert!(ok("https://abc123.test.nextdns.io/"));
        assert!(ok("https://test.nextdns.io/"));
        assert!(!ok("http://abc.test.nextdns.io/"));
        assert!(!ok("https://eviltest.nextdns.io/"));
        assert!(!ok("https://test.nextdns.io.evil.example/"));
    }

    #[test]
    fn parses_a_live_shaped_response() {
        let r: ResolverTest = serde_json::from_str(
            r#"{"status":"ok","protocol":"DOH","profile":"fp0000000000000000","client":"192.0.2.1",
               "srcIP":"2001:db8::1","destIP":"198.51.100.1","anycast":false,"server":"test-1",
               "clientName":"dnscrypt"}"#,
        )
        .unwrap();
        assert_eq!(r.status, "ok");
        assert_eq!(r.client_name.as_deref(), Some("dnscrypt"));
        assert_eq!(
            r.extra.get("srcIP").and_then(Value::as_str),
            Some("2001:db8::1")
        );
        let unconfigured: ResolverTest =
            serde_json::from_str(r#"{"status":"unconfigured"}"#).unwrap();
        assert_eq!(unconfigured.protocol, None);
    }
}
