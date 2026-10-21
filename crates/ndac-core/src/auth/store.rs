//! Storing a key for `ndac auth set`.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use toml_edit::{DocumentMut, Item, Table, value};

use super::AuthError;
use crate::config::LOCAL_FILE;

fn fail(target: &str, reason: impl Into<String>) -> AuthError {
    AuthError::Source {
        source_ref: target.to_owned(),
        reason: reason.into(),
    }
}

/// Stores the key in the OS keyring.
///
/// # Errors
///
/// If the keyring is unavailable or refuses the write.
#[cfg(feature = "keyring")]
pub fn keyring(service: &str, user: &str, key: &str) -> Result<(), AuthError> {
    let target = format!("keyring:{service}/{user}");
    let entry = keyring::Entry::new(service, user)
        .map_err(|_| fail(&target, "cannot open the keyring entry"))?;
    entry.set_password(key).map_err(|e| {
        fail(
            &target,
            format!("cannot store the key: {}", keyring_reason(&e)),
        )
    })
}

#[cfg(feature = "keyring")]
fn keyring_reason(e: &keyring::Error) -> &'static str {
    match e {
        keyring::Error::NoStorageAccess(_) => "keyring is locked or access was denied",
        keyring::Error::PlatformFailure(_) => "no keyring service is available",
        _ => "keyring error",
    }
}

/// Stores the key in the OS keyring.
///
/// # Errors
///
/// Always: this build has no keyring support.
#[cfg(not(feature = "keyring"))]
pub fn keyring(service: &str, user: &str, _key: &str) -> Result<(), AuthError> {
    Err(fail(
        &format!("keyring:{service}/{user}"),
        "ndac was built without keyring support",
    ))
}

/// Writes `api_key` into `ndac.local.toml` (created with mode `0600`,
/// existing comments kept) and makes sure `.gitignore` lists it.
///
/// # Errors
///
/// If the file cannot be parsed or written.
pub fn local_config(root_dir: &Path, key: &str) -> Result<PathBuf, AuthError> {
    let path = root_dir.join(LOCAL_FILE);
    let mut doc: DocumentMut = if path.exists() {
        fs::read_to_string(&path)
            .map_err(|e| fail(LOCAL_FILE, format!("cannot read: {e}")))?
            .parse()
            .map_err(|_| fail(LOCAL_FILE, "is not valid TOML"))?
    } else {
        DocumentMut::new()
    };
    let auth = doc
        .entry("auth")
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| fail(LOCAL_FILE, "`auth` is not a table"))?;
    auth["api_key"] = value(key);
    write_private(&path, &doc.to_string())
        .map_err(|e| fail(LOCAL_FILE, format!("cannot write: {e}")))?;
    ensure_ignored(root_dir, &[LOCAL_FILE, ".env"])
        .map_err(|e| fail(".gitignore", format!("cannot update: {e}")))?;
    Ok(path)
}

/// Writes the key to `path` with mode `0600`.
///
/// # Errors
///
/// If the file cannot be written.
pub fn file(path: &Path, key: &str) -> Result<(), AuthError> {
    write_private(path, &format!("{key}\n")).map_err(|e| {
        fail(
            &format!("file:{}", path.display()),
            format!("cannot write: {e}"),
        )
    })
}

/// Replaces `path` atomically with a `0600` file.
pub(crate) fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.ndac.tmp"));
    let _ = fs::remove_file(&tmp);
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&tmp)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// Appends missing entries to `<dir>/.gitignore`.
pub(crate) fn ensure_ignored(dir: &Path, entries: &[&str]) -> std::io::Result<()> {
    let path = dir.join(".gitignore");
    let current = fs::read_to_string(&path).unwrap_or_default();
    let present = |e: &str| {
        current
            .lines()
            .map(str::trim)
            .any(|l| l == e || l.strip_prefix('/') == Some(e))
    };
    let missing: Vec<&str> = entries.iter().copied().filter(|e| !present(e)).collect();
    if missing.is_empty() {
        return Ok(());
    }
    let mut text = current;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    for e in missing {
        text.push_str(e);
        text.push('\n');
    }
    fs::write(path, text)
}
