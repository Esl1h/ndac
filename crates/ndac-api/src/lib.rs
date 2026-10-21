//! Typed HTTP client for the [NextDNS API](https://nextdns.github.io/api/).
//!
//! This crate has no CLI dependencies so it can be reused by other tools.
//! It covers the documented profile endpoints only; the API is in beta and
//! may change without notice.
//!
//! - Responses carrying an `errors` array are failures, including those
//!   returned with HTTP `200`.
//! - Array endpoints are paginated transparently.
//! - `429`, `5xx` and transport errors are retried with exponential backoff
//!   and full jitter, honoring `Retry-After`. Only idempotent requests are
//!   retried blindly; adding a list item re-reads the list first.
//! - The API key is never part of `Debug` output, errors or logs.
#![forbid(unsafe_code)]

mod client;
mod error;
mod key;
mod model;
mod paths;
mod resolver;
mod retry;

pub use client::{Client, ClientBuilder, DEFAULT_BASE_URL};
pub use error::{ApiErrorDetail, Error, ErrorSource};
pub use key::ApiKey;
pub use model::{
    ActiveItem, BlockPage, Extra, IdItem, Logs, LogsDrop, ParentalControl, Performance, Privacy,
    Profile, ProfileSummary, Security, Settings,
};
pub use paths::{ListPath, ObjectPath};
pub use resolver::{ResolverTest, TEST_URL, test_resolver};
pub use retry::RetryPolicy;
