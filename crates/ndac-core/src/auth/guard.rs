//! Guardrails for literal keys in files: private permissions, owned by the
//! current user, and not tracked by git.

use std::{
    path::Path,
    process::{Command, Stdio},
};

use super::{AuthError, Context};

/// Whether git would commit a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitTracking {
    /// Not in a git work tree, or git is not installed.
    NotInRepo,
    /// Ignored by `.gitignore` (or another exclude file).
    Ignored,
    /// In a work tree, neither tracked nor ignored.
    NotIgnored,
    /// Tracked by git.
    Tracked,
}

/// Checks a file holding a literal key. Returns warnings for violations
/// that the context allows.
pub(super) fn check_secret_file(path: &Path, ctx: &Context) -> Result<Vec<String>, AuthError> {
    let mut warnings = Vec::new();

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::metadata(path).map_err(|e| AuthError::Source {
            source_ref: path.display().to_string(),
            reason: format!("cannot stat: {e}"),
        })?;
        let mode = meta.mode() & 0o777;
        if mode & 0o077 != 0 {
            let err = AuthError::InsecurePermissions {
                path: path.to_path_buf(),
                mode,
            };
            if !ctx.allow_insecure_permissions {
                return Err(err);
            }
            warnings.push(format!("{err} (allowed by allow_insecure_permissions)"));
        }
        if meta.uid() != nix::unistd::getuid().as_raw() {
            let err = AuthError::NotOwned {
                path: path.to_path_buf(),
            };
            if !ctx.allow_insecure_permissions {
                return Err(err);
            }
            warnings.push(format!("{err} (allowed by allow_insecure_permissions)"));
        }
    }

    let reason = match git_tracking(path) {
        GitTracking::Tracked => Some("is tracked by git"),
        GitTracking::NotIgnored => Some("is not ignored by git"),
        GitTracking::Ignored | GitTracking::NotInRepo => None,
    };
    if let Some(reason) = reason {
        let err = AuthError::TrackedSecret {
            path: path.to_path_buf(),
            reason,
        };
        if !ctx.allow_tracked_secret {
            return Err(err);
        }
        warnings.push(format!(
            "WARNING: {err}. Allowed for this run by --allow-tracked-secret; the key may already be in git history, rotate it."
        ));
    }
    Ok(warnings)
}

/// Asks git whether `path` is tracked or ignored. Never fails: without git
/// or outside a repository the answer is [`GitTracking::NotInRepo`].
#[must_use]
pub fn git_tracking(path: &Path) -> GitTracking {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return GitTracking::NotInRepo;
    };
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    let git = |args: &[&str]| -> Option<i32> {
        #[allow(
            clippy::disallowed_methods,
            reason = "runs git with fixed arguments to check whether a secret file is tracked"
        )]
        let mut cmd = Command::new("git");
        cmd.arg("-C")
            .arg(dir)
            .args(args)
            .arg("--")
            .arg(name)
            .env_clear()
            .envs(
                ["PATH", "HOME", "XDG_CONFIG_HOME"]
                    .iter()
                    .filter_map(|k| std::env::var_os(k).map(|v| (*k, v))),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cmd.status().ok().and_then(|s| s.code())
    };
    match git(&["ls-files", "--error-unmatch"]) {
        None => GitTracking::NotInRepo,
        Some(0) => GitTracking::Tracked,
        Some(_) => match git(&["check-ignore", "-q"]) {
            Some(0) => GitTracking::Ignored,
            Some(1) => GitTracking::NotIgnored,
            _ => GitTracking::NotInRepo,
        },
    }
}
