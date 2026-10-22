//! Local snapshots of live profiles.
//!
//! A snapshot is the full live profile JSON at a point in time, stored as
//! `<snapshot_dir>/<profile>/<UTC timestamp>-<short hash>.json` with mode
//! `0600` in `0700` directories. `pre-apply` snapshots allow rollback;
//! `post-apply` snapshots record the state after a verified apply and are
//! the baseline for drift classification.

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{auth::store::write_private, config::types::ProviderConfig};

/// Version of the snapshot file format.
pub const SNAPSHOT_FORMAT_VERSION: u32 = 1;
/// Default directory, relative to the configuration root.
pub const DEFAULT_DIR: &str = ".ndac/snapshots";
/// Default number of snapshots kept per profile.
pub const DEFAULT_KEEP: usize = 50;

/// Why a snapshot was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapshotKind {
    /// Live state just before an apply.
    PreApply,
    /// Live state after a verified apply.
    PostApply,
    /// Live state after an apply that failed verification.
    FailedApply,
}

/// A stored snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// File format version.
    pub format_version: u32,
    /// Profile key.
    pub profile: String,
    /// NextDNS profile ID.
    pub id: String,
    /// Why it was taken.
    pub kind: SnapshotKind,
    /// UTC time, RFC 3339.
    pub taken_at: String,
    /// The live profile.
    pub data: Value,
}

/// A snapshot file on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotEntry {
    /// Profile key (directory name).
    pub profile: String,
    /// Snapshot ID (file stem), sortable by time.
    pub id: String,
    /// File path.
    pub path: PathBuf,
}

/// The snapshot directory of a configuration.
#[derive(Debug, Clone)]
pub struct SnapshotStore {
    dir: PathBuf,
    keep: usize,
}

impl SnapshotStore {
    /// Store at `provider.snapshot_dir` (default [`DEFAULT_DIR`]) under
    /// `root_dir`.
    #[must_use]
    pub fn new(root_dir: &Path, provider: &ProviderConfig) -> Self {
        let dir = provider.snapshot_dir.as_deref().unwrap_or(DEFAULT_DIR);
        Self {
            dir: root_dir.join(dir),
            keep: provider.snapshot_keep.unwrap_or(DEFAULT_KEEP).max(1),
        }
    }

    /// The directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Saves a snapshot and prunes old ones for that profile.
    ///
    /// A top-level `setup` object is left out: it is not documented, and
    /// other clients show it carrying the linked IP update token, a
    /// credential that must not end up on disk or in CI artifacts.
    ///
    /// # Errors
    ///
    /// I/O errors.
    pub fn save(
        &self,
        profile: &str,
        id: &str,
        kind: SnapshotKind,
        data: &Value,
    ) -> io::Result<SnapshotEntry> {
        let mut data = data.clone();
        if let Some(m) = data.as_object_mut() {
            m.remove("setup");
        }
        let snap = Snapshot {
            format_version: SNAPSHOT_FORMAT_VERSION,
            profile: profile.to_owned(),
            id: id.to_owned(),
            kind,
            taken_at: rfc3339(SystemTime::now()),
            data,
        };
        let text = serde_json::to_string_pretty(&snap).map_err(io::Error::other)?;
        let pdir = self.profile_dir(profile)?;
        private_dir(&self.dir)?;
        private_dir(&pdir)?;
        let stamp = compact(SystemTime::now());
        let name = format!("{stamp}-{:08x}", fnv1a(text.as_bytes()) >> 32);
        let path = pdir.join(format!("{name}.json"));
        write_private(&path, &format!("{text}\n"))?;
        self.prune(profile)?;
        Ok(SnapshotEntry {
            profile: profile.to_owned(),
            id: name,
            path,
        })
    }

    fn profile_dir(&self, profile: &str) -> io::Result<PathBuf> {
        if profile.is_empty() || profile.contains(['/', '\\']) || profile.starts_with('.') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid profile key",
            ));
        }
        Ok(self.dir.join(profile))
    }

    /// Snapshots of `profile` (or all profiles), oldest first.
    ///
    /// # Errors
    ///
    /// I/O errors other than a missing directory.
    pub fn list(&self, profile: Option<&str>) -> io::Result<Vec<SnapshotEntry>> {
        let profiles = match profile {
            Some(p) => vec![p.to_owned()],
            None => self.profiles()?,
        };
        let mut out = Vec::new();
        for p in profiles {
            let dir = self.profile_dir(&p)?;
            let entries = match fs::read_dir(&dir) {
                Ok(e) => e,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            let mut found: Vec<SnapshotEntry> = entries
                .flatten()
                .filter_map(|e| {
                    let path = e.path();
                    if path.extension()? != "json" {
                        return None;
                    }
                    let id = path.file_stem()?.to_string_lossy().into_owned();
                    Some(SnapshotEntry {
                        profile: p.clone(),
                        id,
                        path,
                    })
                })
                .collect();
            found.sort_by(|a, b| a.id.cmp(&b.id));
            out.extend(found);
        }
        Ok(out)
    }

    /// Profile keys that have snapshots.
    ///
    /// # Errors
    ///
    /// I/O errors other than a missing directory.
    pub fn profiles(&self) -> io::Result<Vec<String>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut out: Vec<String> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with('.'))
            .collect();
        out.sort();
        Ok(out)
    }

    /// Reads a snapshot file.
    ///
    /// # Errors
    ///
    /// I/O or JSON errors.
    pub fn load(&self, entry: &SnapshotEntry) -> io::Result<Snapshot> {
        let text = fs::read_to_string(&entry.path)?;
        serde_json::from_str(&text).map_err(io::Error::other)
    }

    /// Finds a snapshot by ID (or unique ID prefix), in `profile` or any.
    ///
    /// # Errors
    ///
    /// Not found, ambiguous, or I/O errors.
    pub fn find(&self, id: &str, profile: Option<&str>) -> io::Result<SnapshotEntry> {
        let matches: Vec<SnapshotEntry> = self
            .list(profile)?
            .into_iter()
            .filter(|e| e.id.starts_with(id))
            .collect();
        let n = matches.len();
        let mut iter = matches.into_iter();
        match (iter.next(), n) {
            (Some(only), 1) => Ok(only),
            (None, _) => Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no snapshot `{id}`"),
            )),
            (Some(_), n) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("`{id}` matches {n} snapshots; use a longer ID or --profile"),
            )),
        }
    }

    /// The most recent snapshot of `kind` for `profile`.
    ///
    /// # Errors
    ///
    /// I/O or JSON errors.
    pub fn latest(&self, profile: &str, kind: SnapshotKind) -> io::Result<Option<Snapshot>> {
        for entry in self.list(Some(profile))?.iter().rev() {
            let snap = self.load(entry)?;
            if snap.kind == kind {
                return Ok(Some(snap));
            }
        }
        Ok(None)
    }

    fn prune(&self, profile: &str) -> io::Result<()> {
        let all = self.list(Some(profile))?;
        let excess = all.len().saturating_sub(self.keep);
        for e in all.into_iter().take(excess) {
            fs::remove_file(e.path)?;
        }
        Ok(())
    }
}

fn private_dir(dir: &Path) -> io::Result<()> {
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        b.mode(0o700);
        b.create(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    b.create(dir)
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (
        y,
        u32::try_from(m).unwrap_or(1),
        u32::try_from(d).unwrap_or(1),
    )
}

fn parts(t: SystemTime) -> (i64, u32, u32, u64, u64, u64, u32) {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    let (y, m, day) = civil(i64::try_from(secs / 86_400).unwrap_or(0));
    let rem = secs % 86_400;
    (
        y,
        m,
        day,
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60,
        d.subsec_millis(),
    )
}

/// RFC 3339 UTC timestamp with milliseconds.
#[must_use]
pub fn rfc3339(t: SystemTime) -> String {
    let (y, m, d, hh, mm, ss, ms) = parts(t);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}.{ms:03}Z")
}

/// File-name friendly UTC timestamp, sortable: `20261021T151535123Z`.
fn compact(t: SystemTime) -> String {
    let (y, m, d, hh, mm, ss, ms) = parts(t);
    format!("{y:04}{m:02}{d:02}T{hh:02}{mm:02}{ss:02}{ms:03}Z")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;

    #[test]
    fn timestamps() {
        let t = UNIX_EPOCH + Duration::from_millis(1_792_598_135_123);
        assert_eq!(rfc3339(t), "2026-10-21T15:55:35.123Z");
        assert_eq!(compact(t), "20261021T155535123Z");
        assert_eq!(rfc3339(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        // Leap day.
        let leap = UNIX_EPOCH + Duration::from_secs(951_782_400);
        assert_eq!(rfc3339(leap), "2000-02-29T00:00:00.000Z");
    }

    fn store(dir: &Path, keep: usize) -> SnapshotStore {
        SnapshotStore::new(
            dir,
            &ProviderConfig {
                snapshot_keep: Some(keep),
                ..ProviderConfig::default()
            },
        )
    }

    #[test]
    fn save_list_latest_and_prune() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path(), 3);
        for i in 0..5 {
            let kind = if i == 3 {
                SnapshotKind::PostApply
            } else {
                SnapshotKind::PreApply
            };
            s.save("casa", "abc123", kind, &json!({ "n": i })).unwrap();
            std::thread::sleep(Duration::from_millis(3));
        }
        let all = s.list(Some("casa")).unwrap();
        assert_eq!(all.len(), 3, "pruned to snapshot_keep");
        let latest = s.latest("casa", SnapshotKind::PostApply).unwrap().unwrap();
        assert_eq!(latest.data, json!({ "n": 3 }));
        assert_eq!(s.latest("casa", SnapshotKind::FailedApply).unwrap(), None);
        assert_eq!(s.profiles().unwrap(), ["casa"]);
        let found = s.find(&all[0].id[..10], None);
        assert!(found.is_err(), "ambiguous prefix");
        assert_eq!(s.find(&all[2].id, None).unwrap(), all[2]);
    }

    #[cfg(unix)]
    #[test]
    fn files_and_dirs_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path(), 5);
        let e = s
            .save("casa", "abc123", SnapshotKind::PreApply, &json!({}))
            .unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&e.path), 0o600);
        assert_eq!(mode(e.path.parent().unwrap()), 0o700);
        assert_eq!(mode(s.dir()), 0o700);
    }

    #[test]
    fn setup_is_never_stored() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path(), 5);
        let data = json!({
            "name": "Casa",
            "setup": { "linkedIp": { "updateToken": "token-0000" } }
        });
        let e = s
            .save("casa", "abc123", SnapshotKind::PreApply, &data)
            .unwrap();
        let text = fs::read_to_string(&e.path).unwrap();
        assert!(!text.contains("token-0000"), "{text}");
        assert_eq!(s.load(&e).unwrap().data, json!({ "name": "Casa" }));
    }

    #[test]
    fn rejects_path_like_profile_keys() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path(), 5);
        assert!(
            s.save("../x", "id", SnapshotKind::PreApply, &json!({}))
                .is_err()
        );
        assert!(s.list(Some(".hidden")).is_err());
    }
}
