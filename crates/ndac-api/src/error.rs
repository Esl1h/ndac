//! Error types for the NextDNS API client.

use std::fmt;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

/// Errors returned by [`Client`](crate::Client).
///
/// No variant ever contains the API key.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The API answered with an `errors` array. NextDNS uses `400` for
    /// malformed requests and `200` for user errors such as an invalid
    /// domain, so `status` may be a success code.
    #[error("{endpoint}: NextDNS API returned an error (HTTP {status}): {}", ApiErrors(.errors))]
    Api {
        /// Endpoint template, e.g. `/profiles/:profile/denylist`.
        endpoint: &'static str,
        /// HTTP status of the response.
        status: StatusCode,
        /// The errors reported by the API, at least one.
        errors: Vec<ApiErrorDetail>,
    },

    /// The API key was rejected (`401` or `403`).
    #[error("{endpoint}: API key rejected by NextDNS (HTTP {status})")]
    Unauthorized {
        /// Endpoint template.
        endpoint: &'static str,
        /// `401` or `403`.
        status: StatusCode,
    },

    /// The profile or list item does not exist (`404`).
    #[error("{endpoint}: not found (HTTP 404)")]
    NotFound {
        /// Endpoint template.
        endpoint: &'static str,
    },

    /// Any other unsuccessful status without an `errors` array.
    #[error("{endpoint}: unexpected HTTP {status}")]
    Status {
        /// Endpoint template.
        endpoint: &'static str,
        /// HTTP status of the response.
        status: StatusCode,
    },

    /// Connection, TLS or timeout failure.
    #[error("{endpoint}: request failed: {source}")]
    Transport {
        /// Endpoint template.
        endpoint: &'static str,
        /// Underlying error.
        #[source]
        source: reqwest::Error,
    },

    /// The response body could not be decoded.
    #[error("{endpoint}: invalid response body: {source}")]
    Decode {
        /// Endpoint template.
        endpoint: &'static str,
        /// Underlying error.
        #[source]
        source: serde_json::Error,
    },

    /// A paginated endpoint returned the same cursor twice or too many pages.
    #[error("{endpoint}: pagination did not terminate")]
    Pagination {
        /// Endpoint template.
        endpoint: &'static str,
    },

    /// Retrying would exceed the command deadline.
    #[error("deadline exceeded after {attempts} attempt(s); last error: {last}")]
    DeadlineExceeded {
        /// Attempts made before giving up.
        attempts: u32,
        /// The error of the last attempt.
        #[source]
        last: Box<Error>,
    },

    /// Every retry failed.
    #[error("giving up after {attempts} attempt(s): {last}")]
    RetriesExhausted {
        /// Attempts made, including the first one.
        attempts: u32,
        /// The error of the last attempt.
        #[source]
        last: Box<Error>,
    },

    /// The API key contains characters not allowed in an HTTP header.
    #[error("API key contains invalid characters")]
    InvalidApiKey,

    /// The base URL is malformed or not allowed.
    #[error("invalid API base URL: {0}")]
    InvalidBaseUrl(String),

    /// The HTTP client could not be built.
    #[error("cannot build HTTP client: {0}")]
    Build(#[source] reqwest::Error),
}

impl Error {
    /// True if the API key was rejected.
    #[must_use]
    pub fn is_auth(&self) -> bool {
        match self {
            Self::Unauthorized { .. } => true,
            Self::RetriesExhausted { last, .. } | Self::DeadlineExceeded { last, .. } => {
                last.is_auth()
            }
            _ => false,
        }
    }

    /// True for failures worth retrying: `429`, `5xx` and transport errors.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Status { status, .. } | Self::Api { status, .. } => {
                *status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
            }
            Self::Transport { source, .. } => {
                source.is_connect() || source.is_timeout() || source.is_request()
            }
            _ => false,
        }
    }
}

/// One entry of the `errors` array in an API response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiErrorDetail {
    /// Machine-readable code, e.g. `invalid`.
    pub code: String,
    /// Human-readable explanation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Where the error is located in the request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ErrorSource>,
}

/// Location of an API error in the request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorSource {
    /// Query string parameter name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter: Option<String>,
    /// JSON pointer into the request body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
}

struct ApiErrors<'a>(&'a [ApiErrorDetail]);

impl fmt::Display for ApiErrors<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, e) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("; ")?;
            }
            f.write_str(&e.code)?;
            if let Some(detail) = &e.detail {
                write!(f, ": {detail}")?;
            }
            if let Some(source) = &e.source
                && let Some(p) = source.pointer.as_deref().or(source.parameter.as_deref())
            {
                write!(f, " (at {p})")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_display_lists_every_error() {
        let err = Error::Api {
            endpoint: "/profiles/:profile/denylist",
            status: StatusCode::OK,
            errors: vec![
                ApiErrorDetail {
                    code: "invalid".into(),
                    detail: Some("bad domain".into()),
                    source: Some(ErrorSource {
                        parameter: None,
                        pointer: Some("/id".into()),
                    }),
                },
                ApiErrorDetail {
                    code: "duplicate".into(),
                    detail: None,
                    source: None,
                },
            ],
        };
        assert_eq!(
            err.to_string(),
            "/profiles/:profile/denylist: NextDNS API returned an error (HTTP 200 OK): \
             invalid: bad domain (at /id); duplicate"
        );
        assert!(!err.is_retryable());
    }

    #[test]
    fn retryable_statuses() {
        let status = |s| Error::Status {
            endpoint: "/profiles",
            status: s,
        };
        assert!(status(StatusCode::TOO_MANY_REQUESTS).is_retryable());
        assert!(status(StatusCode::BAD_GATEWAY).is_retryable());
        assert!(!status(StatusCode::CONFLICT).is_retryable());
    }

    #[test]
    fn auth_is_detected_through_wrappers() {
        let inner = Error::Unauthorized {
            endpoint: "/profiles",
            status: StatusCode::FORBIDDEN,
        };
        let wrapped = Error::RetriesExhausted {
            attempts: 1,
            last: Box::new(inner),
        };
        assert!(wrapped.is_auth());
        assert!(!wrapped.is_retryable());
    }
}
