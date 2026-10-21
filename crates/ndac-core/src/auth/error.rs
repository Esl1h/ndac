//! Authentication errors. None of them ever contains a key.

use std::path::PathBuf;

use miette::Diagnostic;

/// Why no key could be resolved.
#[derive(Debug, thiserror::Error, Diagnostic)]
#[non_exhaustive]
pub enum AuthError {
    /// Every source in the chain was missing.
    #[error("no NextDNS API key found")]
    #[diagnostic(
        code(ndac::auth::no_key),
        help(
            "tried: {}\nset NEXTDNS_API_KEY, run `ndac auth set`, or configure [auth].sources",
            tried.join(", ")
        )
    )]
    NoKey {
        /// Sources tried, with the reason each was skipped.
        tried: Vec<String>,
    },

    /// A source reference could not be parsed.
    #[error("invalid auth source `{input}`: {reason}")]
    #[diagnostic(
        code(ndac::auth::invalid_ref),
        help(
            "valid forms: config, env:VAR, env-file:VAR, file:PATH, dotenv[:PATH], keyring:SERVICE/USER, cmd:PROGRAM ARGS, systemd-cred:NAME, age:PATH"
        )
    )]
    InvalidRef {
        /// The reference.
        input: String,
        /// What is wrong.
        reason: String,
    },

    /// A source exists but could not produce a key.
    #[error("auth source `{source_ref}` failed: {reason}")]
    #[diagnostic(code(ndac::auth::source))]
    Source {
        /// The source reference.
        source_ref: String,
        /// What went wrong.
        reason: String,
    },

    /// A file holding a literal key is readable by other users.
    #[error("{} holds an API key but is readable by other users (mode {mode:o})", path.display())]
    #[diagnostic(
        code(ndac::auth::permissions),
        help("run `chmod 600 {}`, or set auth.allow_insecure_permissions = true in ndac.local.toml", path.display())
    )]
    InsecurePermissions {
        /// The file.
        path: PathBuf,
        /// Its permission bits.
        mode: u32,
    },

    /// A file holding a literal key belongs to another user.
    #[error("{} holds an API key but is owned by another user", path.display())]
    #[diagnostic(
        code(ndac::auth::owner),
        help("take ownership of the file or use another auth source")
    )]
    NotOwned {
        /// The file.
        path: PathBuf,
    },

    /// A file holding a literal key is tracked by git or not ignored.
    #[error("{} holds an API key but {reason}", path.display())]
    #[diagnostic(
        code(ndac::auth::tracked),
        help(
            "move the key to ndac.local.toml (git-ignored) or another source; --allow-tracked-secret overrides this check for one run"
        )
    )]
    TrackedSecret {
        /// The file.
        path: PathBuf,
        /// "is tracked by git" or "is not ignored by git".
        reason: &'static str,
    },
}

impl AuthError {
    /// Failures that `on_source_error = "skip"` may skip. Guardrail
    /// violations are never skipped: they mean a key is exposed.
    #[must_use]
    pub fn is_skippable(&self) -> bool {
        matches!(self, Self::Source { .. })
    }
}
