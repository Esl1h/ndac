//! The NextDNS API key type.

use std::fmt;

use reqwest::header::HeaderValue;
use secrecy::{ExposeSecret, SecretString};

use crate::Error;

/// A NextDNS API key.
///
/// The key grants full access to the account, including DNS logs. It is
/// zeroized on drop and its `Debug` and `Display` output show at most the
/// last four characters.
#[derive(Clone)]
pub struct ApiKey(SecretString);

impl ApiKey {
    /// Wraps a key, trimming surrounding whitespace such as the trailing
    /// newline left by `pass show` or a key file.
    #[must_use]
    pub fn new(key: impl Into<String>) -> Self {
        let key: String = key.into();
        let trimmed = key.trim();
        if trimmed.len() == key.len() {
            Self(SecretString::from(key))
        } else {
            Self(SecretString::from(trimmed.to_owned()))
        }
    }

    /// Exposes the key. Only the HTTP layer and auth validation call this.
    #[must_use]
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }

    /// The key as an `X-Api-Key` header value, marked sensitive so it is
    /// never printed by the HTTP stack.
    pub(crate) fn header_value(&self) -> Result<HeaderValue, Error> {
        let mut value = HeaderValue::from_str(self.expose()).map_err(|_| Error::InvalidApiKey)?;
        value.set_sensitive(true);
        Ok(value)
    }

    fn redacted(&self) -> String {
        let key = self.expose();
        let tail: String = key
            .chars()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        // Short keys would be revealed almost entirely by their tail.
        if key.chars().count() < 16 {
            "****".to_owned()
        } else {
            format!("****{tail}")
        }
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ApiKey").field(&self.redacted()).finish()
    }
}

impl fmt::Display for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.redacted())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "test-key-00000000000000000000000000001234";

    #[test]
    fn debug_and_display_redact() {
        let key = ApiKey::new(KEY);
        assert_eq!(format!("{key}"), "****1234");
        assert_eq!(format!("{key:?}"), r#"ApiKey("****1234")"#);
    }

    #[test]
    fn short_keys_are_fully_masked() {
        assert_eq!(ApiKey::new("abc12345").to_string(), "****");
    }

    #[test]
    fn trims_whitespace() {
        assert_eq!(ApiKey::new(format!("  {KEY}\n")).expose(), KEY);
    }

    #[test]
    fn header_value_is_sensitive() {
        let value = ApiKey::new(KEY).header_value().expect("valid header");
        assert!(value.is_sensitive());
        assert!(!format!("{value:?}").contains("1234"));
    }

    #[test]
    fn rejects_non_header_characters() {
        let err = ApiKey::new("bad\u{7f}key").header_value().unwrap_err();
        assert!(matches!(err, Error::InvalidApiKey));
        assert!(!err.to_string().contains("bad"));
    }
}
