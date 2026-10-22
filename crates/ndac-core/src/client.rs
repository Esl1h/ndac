//! Builds the API client from `[provider]`.

use std::time::Duration;

use ndac_api::{ApiKey, Client, Error, Profile, RetryPolicy};

use crate::config::{duration_secs, types::ProviderConfig};

/// A client configured from `[provider]`, identifying as `user_agent`.
///
/// # Errors
///
/// An invalid `api_url` or TLS initialization failure.
pub fn api_client(
    provider: &ProviderConfig,
    key: ApiKey,
    user_agent: &str,
) -> Result<Client, Error> {
    let mut b = Client::builder(key).user_agent(user_agent);
    if let Some(url) = &provider.api_url {
        b = b.base_url(url.clone());
    }
    if let Some(t) = provider
        .timeout
        .as_ref()
        .and_then(|t| duration_secs(t).ok())
    {
        b = b.timeout(Duration::from_secs(t));
    }
    if let Some(r) = provider.retries {
        b = b.retry_policy(RetryPolicy {
            max_retries: r,
            ..RetryPolicy::default()
        });
    }
    if let Some(c) = provider.concurrency {
        b = b.concurrency(c);
    }
    b.build()
}

/// The live state of a profile, as plans and snapshots use it. With
/// `unstable-endpoints`, rewrites are read from their own endpoint.
///
/// # Errors
///
/// Any [`Error`] from the requests.
pub async fn live_profile(client: &Client, id: &str) -> Result<Profile, Error> {
    #[cfg_attr(not(feature = "unstable-endpoints"), allow(unused_mut))]
    let mut p = client.get_profile(id).await?;
    // UNVERIFIED: whether the profile response includes rewrites, and in
    // which shape, is undocumented.
    #[cfg(feature = "unstable-endpoints")]
    {
        p.extra.remove("rewrites");
        p.rewrites = Some(client.rewrites(id).await?);
    }
    Ok(p)
}
