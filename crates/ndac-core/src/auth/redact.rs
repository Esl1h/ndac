//! Defense-in-depth redaction of resolved keys in log output.
//!
//! Every resolved key is registered here; the CLI's log writer passes all
//! output through [`redact`], so a key that reaches a log line by mistake
//! is still masked.

use std::{
    borrow::Cow,
    sync::{Mutex, PoisonError},
};

static SECRETS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Registers a secret for redaction. Very short values are ignored to avoid
/// masking ordinary words.
pub fn register_secret(secret: &str) {
    if secret.len() < 8 {
        return;
    }
    let mut secrets = SECRETS.lock().unwrap_or_else(PoisonError::into_inner);
    if !secrets.iter().any(|s| s == secret) {
        secrets.push(secret.to_owned());
    }
}

/// `text` with every registered secret replaced by `****`.
#[must_use]
pub fn redact(text: &str) -> Cow<'_, str> {
    let secrets = SECRETS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut out = Cow::Borrowed(text);
    for s in secrets.iter() {
        if out.contains(s.as_str()) {
            out = Cow::Owned(out.replace(s.as_str(), "****"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_registered_secrets() {
        register_secret("redact-me-0000000000000000000000000000abcd");
        register_secret("short");
        assert_eq!(
            redact("key=redact-me-0000000000000000000000000000abcd; short"),
            "key=****; short"
        );
        assert!(matches!(redact("nothing here"), Cow::Borrowed(_)));
    }
}
