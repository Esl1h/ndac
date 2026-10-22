//! Endpoints missing from the NextDNS API documentation (feature
//! `unstable-endpoints`).
//!
//! UNVERIFIED: paths and shapes follow the `nextdns-go` client
//! (<https://github.com/amalucelli/nextdns-go>, `rewrites.go` and
//! `setup.go`), not the API documentation. They may change or disappear
//! without notice.

use reqwest::Method;
use serde_json::json;

use super::{Client, Request, decode, encode};
use crate::{Error, Rewrite, Setup};

const REWRITES: &str = "/profiles/:profile/rewrites";
const REWRITE: &str = "/profiles/:profile/rewrites/:id";
const SETUP: &str = "/profiles/:profile/setup";

impl Client {
    /// `GET /profiles/:profile/rewrites`. UNVERIFIED.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from the request or response decoding.
    pub async fn rewrites(&self, profile: &str) -> Result<Vec<Rewrite>, Error> {
        self.get_all(self.profile_url(profile, &["rewrites"], None), REWRITES)
            .await
    }

    /// `POST /profiles/:profile/rewrites` with `{ name, content }`.
    /// UNVERIFIED.
    ///
    /// Like [`Client::add_item`], a retryable failure reads the rewrites back
    /// and posts again only if the rewrite is absent.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from the requests.
    pub async fn add_rewrite(&self, profile: &str, name: &str, content: &str) -> Result<(), Error> {
        let body = encode(&json!({ "name": name, "content": content }))?;
        let req = Request {
            method: Method::POST,
            url: self.profile_url(profile, &["rewrites"], None),
            endpoint: REWRITES,
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
                .rewrites(profile)
                .await?
                .iter()
                .any(|r| r.name == name && r.content == content);
            if present {
                tracing::debug!(endpoint = REWRITES, "rewrite present after failed POST");
                return Ok(());
            }
            self.wait_before_retry(REWRITES, retry, retry_after, err)
                .await?;
            retry += 1;
        }
    }

    /// Deletes the rewrite of `name` to `content`, if present. Rewrites are
    /// addressed by an ID the API assigns, so they are read first.
    /// UNVERIFIED.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from the requests.
    pub async fn delete_rewrite(
        &self,
        profile: &str,
        name: &str,
        content: &str,
    ) -> Result<(), Error> {
        let found = self
            .rewrites(profile)
            .await?
            .into_iter()
            .find(|r| r.name == name && r.content == content)
            .and_then(|r| r.id);
        let Some(id) = found else {
            return Ok(());
        };
        self.delete(self.profile_url(profile, &["rewrites"], Some(&id)), REWRITE)
            .await
    }

    /// `GET /profiles/:profile/setup`. UNVERIFIED.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from the request or response decoding.
    pub async fn setup(&self, profile: &str) -> Result<Setup, Error> {
        let env = self
            .send(&Self::get(
                self.profile_url(profile, &["setup"], None),
                SETUP,
            ))
            .await?;
        decode(env.data, SETUP)
    }
}
