//! API key resolution.
//!
//! A key is resolved from an ordered chain of source references. The chain
//! comes from, in order of precedence: `--auth-source` /
//! `NDAC_AUTH_SOURCE`, the profile's `[auth].sources`, the root (and local)
//! `[auth].sources`, or [`DEFAULT_CHAIN`]. The first source that yields a
//! key wins. Missing sources are skipped; a source that exists but fails is
//! an error unless `on_source_error = "skip"`.
//!
//! The key is never accepted as a command-line value, never logged, and
//! literal keys in files are only used when the file is private and not
//! tracked by git.

mod error;
mod guard;
mod redact;
mod source;
mod sources;
pub mod store;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

pub use error::AuthError;
pub use guard::{GitTracking, git_tracking};
use ndac_api::ApiKey;
pub use redact::{redact, register_secret};
pub use source::SourceRef;

use crate::config::{
    Workspace, duration_secs,
    types::{AuthConfig, OnSourceError, SecretValue},
};

/// Chain used when nothing is configured.
pub const DEFAULT_CHAIN: &[&str] = &[
    "systemd-cred:nextdns-api-key",
    "env-file:NEXTDNS_API_KEY_FILE",
    "env:NEXTDNS_API_KEY",
    "dotenv",
    "keyring:nextdns/default",
    "config",
];

/// Default timeout for `cmd:` sources.
pub const DEFAULT_CMD_TIMEOUT: Duration = Duration::from_secs(30);

/// Read access to environment variables, injectable for tests.
pub trait Env {
    /// The variable's value, if set and valid UTF-8.
    fn var(&self, key: &str) -> Option<String>;
}

/// The process environment.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemEnv;

impl Env for SystemEnv {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

impl Env for BTreeMap<String, String> {
    fn var(&self, key: &str) -> Option<String> {
        self.get(key).cloned()
    }
}

/// A literal `api_key` and the file it was written in.
#[derive(Debug, Clone)]
pub struct Literal {
    /// The key.
    pub key: SecretValue,
    /// The file holding it.
    pub path: PathBuf,
}

/// Everything resolution needs besides the chain.
#[derive(Debug, Clone)]
pub struct Context {
    /// Directory of `ndac.toml`; searched for `.env` after the working
    /// directory.
    pub root_dir: PathBuf,
    /// Working directory.
    pub cwd: PathBuf,
    /// Literal keys in precedence order: local file, root file, profile.
    pub literals: Vec<Literal>,
    /// What to do when a source fails.
    pub on_source_error: OnSourceError,
    /// Downgrade insecure permissions to a warning. Only honored from
    /// `ndac.local.toml` or `NDAC_ALLOW_INSECURE_PERMISSIONS=1`.
    pub allow_insecure_permissions: bool,
    /// Allow a literal key in a file that git tracks (CLI flag only).
    pub allow_tracked_secret: bool,
    /// Do not warn when a literal key is used.
    pub quiet_literal: bool,
    /// Timeout for `cmd:` sources.
    pub cmd_timeout: Duration,
}

impl Context {
    /// A context with defaults for `root_dir` and `cwd`.
    #[must_use]
    pub fn new(root_dir: impl Into<PathBuf>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            root_dir: root_dir.into(),
            cwd: cwd.into(),
            literals: Vec::new(),
            on_source_error: OnSourceError::Fail,
            allow_insecure_permissions: false,
            allow_tracked_secret: false,
            quiet_literal: false,
            cmd_timeout: DEFAULT_CMD_TIMEOUT,
        }
    }
}

/// A resolved key and where it came from.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// The key. Already registered for log redaction.
    pub key: ApiKey,
    /// The source that produced it.
    pub source: SourceRef,
    /// File the key was read from, for file-based sources.
    pub origin: Option<PathBuf>,
    /// Warnings to show the user once.
    pub warnings: Vec<String>,
}

/// Outcome of trying one source.
#[derive(Debug)]
pub enum Probe {
    /// The source produced a key.
    Found(Box<Resolved>),
    /// The source is not configured here (unset variable, absent file).
    Missing(String),
    /// The source exists but failed.
    Failed(AuthError),
}

/// Builds the chain to use.
///
/// `cli` holds `--auth-source` values (clap also fills it from
/// `NDAC_AUTH_SOURCE`); the profile's and the root's `[auth].sources` follow.
///
/// # Errors
///
/// [`AuthError::InvalidRef`] for a malformed reference.
pub fn chain(
    cli: &[String],
    profile: Option<&AuthConfig>,
    root: &AuthConfig,
) -> Result<Vec<SourceRef>, AuthError> {
    let refs: Vec<String> = if !cli.is_empty() {
        cli.to_vec()
    } else if let Some(s) = profile.and_then(|p| p.sources.clone()) {
        s
    } else if let Some(s) = root.sources.clone() {
        s
    } else {
        DEFAULT_CHAIN.iter().map(|s| (*s).to_owned()).collect()
    };
    refs.iter().map(|r| r.parse()).collect()
}

/// Tries every source in order and returns the first key.
///
/// # Errors
///
/// The first failing source (unless failures are skipped), a guardrail
/// violation, or [`AuthError::NoKey`] when every source is missing.
pub fn resolve(chain: &[SourceRef], ctx: &Context, env: &dyn Env) -> Result<Resolved, AuthError> {
    let mut tried = Vec::new();
    for source in chain {
        match probe(source, ctx, env) {
            Probe::Found(r) => return Ok(*r),
            Probe::Missing(why) => {
                tracing::debug!(source = %source, reason = %why, "auth source not available");
                tried.push(format!("{source} ({why})"));
            }
            Probe::Failed(e) if ctx.on_source_error == OnSourceError::Skip && e.is_skippable() => {
                tracing::warn!(source = %source, error = %e, "auth source failed, trying the next one");
                tried.push(format!("{source} (failed)"));
            }
            Probe::Failed(e) => return Err(e),
        }
    }
    Err(AuthError::NoKey { tried })
}

/// Tries one source.
pub fn probe(source: &SourceRef, ctx: &Context, env: &dyn Env) -> Probe {
    match sources::read(source, ctx, env) {
        Ok(Some(found)) => {
            let mut warnings = Vec::new();
            if found.literal {
                if let Some(path) = &found.origin {
                    match guard::check_secret_file(path, ctx) {
                        Ok(w) => warnings.extend(w),
                        Err(e) => return Probe::Failed(e),
                    }
                }
                if !ctx.quiet_literal {
                    warnings.push(format!(
                        "auth source: {source} (literal in {}); consider keyring, pass or systemd-cred",
                        found.origin.as_deref().map_or_else(String::new, |p| display(p, &ctx.root_dir))
                    ));
                }
            }
            if found.value.is_empty() {
                return Probe::Failed(AuthError::Source {
                    source_ref: source.to_string(),
                    reason: "the source returned an empty key".into(),
                });
            }
            register_secret(&found.value);
            Probe::Found(Box::new(Resolved {
                key: ApiKey::new(found.value),
                source: source.clone(),
                origin: found.origin,
                warnings,
            }))
        }
        Ok(None) => Probe::Missing(sources::missing_reason(source)),
        Err(e) => Probe::Failed(e),
    }
}

/// Builds the resolution context for `profile` (or the root configuration
/// when `None`): literal keys in precedence order, and settings where the
/// profile's `[auth]` overrides the root's.
///
/// `allow_insecure_permissions` is only honored from `ndac.local.toml` or
/// `NDAC_ALLOW_INSECURE_PERMISSIONS=1`, so a committed file cannot weaken
/// the check for everyone who clones it.
#[must_use]
pub fn context(
    ws: &Workspace,
    profile: Option<&str>,
    cwd: &Path,
    allow_tracked_secret: bool,
    env: &dyn Env,
) -> Context {
    let mut ctx = Context::new(&ws.root_dir, cwd);
    let local = ws.local.as_ref();
    let entry = profile.and_then(|k| ws.profiles.get(k));
    let profile_auth = entry.and_then(|e| e.file.auth.as_ref());

    if let Some((src, l)) = local
        && let Some(k) = &l.auth.api_key
    {
        ctx.literals.push(Literal {
            key: k.clone(),
            path: src.path.clone(),
        });
    }
    if let Some(k) = &ws.root.auth.api_key {
        ctx.literals.push(Literal {
            key: k.clone(),
            path: ws.root_src.path.clone(),
        });
    }
    if let (Some(e), Some(k)) = (entry, profile_auth.and_then(|a| a.api_key.as_ref())) {
        ctx.literals.push(Literal {
            key: k.clone(),
            path: e.src.path.clone(),
        });
    }

    let root = ws.auth();
    let pick = |f: fn(&AuthConfig) -> Option<bool>| profile_auth.and_then(f).or_else(|| f(&root));
    ctx.on_source_error = profile_auth
        .and_then(|a| a.on_source_error)
        .or(root.on_source_error)
        .unwrap_or(OnSourceError::Fail);
    ctx.quiet_literal = pick(|a| a.quiet_literal).unwrap_or(false);
    ctx.allow_insecure_permissions = local
        .and_then(|(_, l)| l.auth.allow_insecure_permissions)
        .unwrap_or(false)
        || env.var("NDAC_ALLOW_INSECURE_PERMISSIONS").as_deref() == Some("1");
    ctx.allow_tracked_secret = allow_tracked_secret;
    if let Some(t) = profile_auth
        .and_then(|a| a.cmd_timeout.as_ref())
        .or(root.cmd_timeout.as_ref())
        .and_then(|t| duration_secs(t).ok())
    {
        ctx.cmd_timeout = Duration::from_secs(t);
    }
    ctx
}

/// The chain for `profile` (or the root configuration).
///
/// # Errors
///
/// [`AuthError::InvalidRef`] for a malformed reference.
pub fn chain_for(
    ws: &Workspace,
    profile: Option<&str>,
    cli: &[String],
) -> Result<Vec<SourceRef>, AuthError> {
    let profile_auth = profile
        .and_then(|k| ws.profiles.get(k))
        .and_then(|e| e.file.auth.as_ref());
    chain(cli, profile_auth, &ws.auth())
}

/// `path` relative to `root` when possible, for messages.
pub(crate) fn display(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
mod tests;
