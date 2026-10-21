//! Writing configuration files: ID write-back after a create, and new
//! profile files from `import`.

use std::{fs, io, path::Path};

use ndac_api::{ActiveItem, IdItem, Profile};
use toml_edit::{Array, DocumentMut, Item, Table, value};

use crate::config::format_toml;

/// Sets `id` in a profile file, keeping comments, order and permissions.
///
/// # Errors
///
/// I/O errors or a file that is not valid TOML.
pub fn write_id(path: &Path, id: &str) -> io::Result<()> {
    let text = fs::read_to_string(path)?;
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e}")))?;
    match doc.get_mut("id") {
        Some(Item::Value(v)) => {
            let decor = v.decor().clone();
            *v = toml_edit::Value::from(id);
            *v.decor_mut() = decor;
        }
        _ => {
            doc.insert("id", value(id));
        }
    }
    replace_preserving(path, &doc.to_string())
}

/// Replaces `path` atomically, keeping its permissions.
///
/// # Errors
///
/// I/O errors.
pub fn replace_preserving(path: &Path, text: &str) -> io::Result<()> {
    let perms = fs::metadata(path)?.permissions();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.ndac.tmp"));
    fs::write(&tmp, text)?;
    fs::set_permissions(&tmp, perms)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// A file key from a profile name: lowercase ASCII letters, digits and
/// `-`; empty when nothing usable remains.
#[must_use]
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

/// Seconds as the shortest exact duration string, e.g. `7776000` → `"90d"`.
fn duration(secs: u64) -> Option<String> {
    [
        ("y", 31_536_000),
        ("w", 604_800),
        ("d", 86_400),
        ("h", 3_600),
        ("m", 60),
    ]
    .into_iter()
    .find(|(_, n)| secs >= *n && secs.is_multiple_of(*n))
    .map(|(u, n)| format!("{}{u}", secs / n))
}

fn ids(items: &[IdItem]) -> Item {
    let mut v: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    v.sort_unstable();
    let mut a = Array::new();
    for id in v {
        a.push(id);
    }
    value(a)
}

fn item_table(items: &[ActiveItem]) -> Table {
    let mut t = Table::new();
    let mut sorted: Vec<&ActiveItem> = items.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    for i in sorted {
        t.insert(&i.id, value(i.active));
    }
    t
}

fn set<T: Into<toml_edit::Value>>(t: &mut Table, key: &str, v: Option<T>) {
    if let Some(v) = v {
        t.insert(key, value(v));
    }
}

/// Renders a live profile as a profile file in the canonical layout, so that
/// `plan` right after `import` is empty. Lists are written authoritative.
#[must_use]
pub fn profile_toml(id: &str, live: &Profile) -> String {
    let mut doc = DocumentMut::new();
    doc.insert("id", value(id));
    if let Some(n) = &live.name {
        doc.insert("name", value(n.as_str()));
    }
    if let Some(s) = &live.security {
        let mut t = Table::new();
        set(
            &mut t,
            "threatIntelligenceFeeds",
            s.threat_intelligence_feeds,
        );
        set(&mut t, "aiThreatDetection", s.ai_threat_detection);
        set(&mut t, "googleSafeBrowsing", s.google_safe_browsing);
        set(&mut t, "cryptojacking", s.cryptojacking);
        set(&mut t, "dnsRebinding", s.dns_rebinding);
        set(&mut t, "idnHomographs", s.idn_homographs);
        set(&mut t, "typosquatting", s.typosquatting);
        set(&mut t, "dga", s.dga);
        set(&mut t, "nrd", s.nrd);
        set(&mut t, "ddns", s.ddns);
        set(&mut t, "parking", s.parking);
        set(&mut t, "csam", s.csam);
        if let Some(l) = &s.tlds {
            t.insert("tlds", ids(l));
        }
        doc.insert("security", Item::Table(t));
    }
    if let Some(p) = &live.privacy {
        let mut t = Table::new();
        if let Some(l) = &p.blocklists {
            t.insert("blocklists", ids(l));
        }
        if let Some(l) = &p.natives {
            t.insert("natives", ids(l));
        }
        set(&mut t, "disguisedTrackers", p.disguised_trackers);
        set(&mut t, "allowAffiliate", p.allow_affiliate);
        doc.insert("privacy", Item::Table(t));
    }
    if let Some(pc) = &live.parental_control {
        let mut t = Table::new();
        set(&mut t, "safeSearch", pc.safe_search);
        set(&mut t, "youtubeRestrictedMode", pc.youtube_restricted_mode);
        set(&mut t, "blockBypass", pc.block_bypass);
        if let Some(l) = &pc.services {
            t.insert("services", Item::Table(item_table(l)));
        }
        if let Some(l) = &pc.categories {
            t.insert("categories", Item::Table(item_table(l)));
        }
        doc.insert("parentalControl", Item::Table(t));
    }
    for (key, list) in [("denylist", &live.denylist), ("allowlist", &live.allowlist)] {
        if let Some(l) = list {
            let mut t = Table::new();
            t.insert("mode", value("authoritative"));
            t.insert("items", Item::Table(item_table(l)));
            doc.insert(key, Item::Table(t));
        }
    }
    if let Some(s) = &live.settings {
        let mut t = Table::new();
        set(&mut t, "web3", s.web3);
        if let Some(l) = &s.logs {
            let mut lt = Table::new();
            set(&mut lt, "enabled", l.enabled);
            if let Some(r) = l.retention {
                match duration(r) {
                    Some(d) => lt.insert("retention", value(d)),
                    None => lt.insert("retention", value(i64::try_from(r).unwrap_or(i64::MAX))),
                };
            }
            set(&mut lt, "location", l.location.as_deref());
            if let Some(d) = &l.drop {
                let mut dt = Table::new();
                set(&mut dt, "ip", d.ip);
                set(&mut dt, "domain", d.domain);
                lt.insert("drop", Item::Table(dt));
            }
            t.insert("logs", Item::Table(lt));
        }
        if let Some(b) = &s.block_page {
            let mut bt = Table::new();
            set(&mut bt, "enabled", b.enabled);
            t.insert("blockPage", Item::Table(bt));
        }
        if let Some(p) = &s.performance {
            let mut pt = Table::new();
            set(&mut pt, "ecs", p.ecs);
            set(&mut pt, "cacheBoost", p.cache_boost);
            set(&mut pt, "cnameFlattening", p.cname_flattening);
            t.insert("performance", Item::Table(pt));
        }
        doc.insert("settings", Item::Table(t));
    }
    let text = doc.to_string();
    format_toml(&text).unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slug("Casa"), "casa");
        assert_eq!(slug("Kids' iPad (2nd)"), "kids-ipad-2nd");
        assert_eq!(slug("--"), "");
        assert_eq!(slug("Trabalho Ágil"), "trabalho-gil");
    }

    #[test]
    fn durations() {
        assert_eq!(duration(7_776_000).as_deref(), Some("90d"));
        assert_eq!(duration(604_800).as_deref(), Some("1w"));
        assert_eq!(duration(31_536_000).as_deref(), Some("1y"));
        assert_eq!(duration(3_601), None);
    }

    #[test]
    fn write_id_keeps_comments_and_replaces_existing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("p.toml");
        fs::write(
            &p,
            "# my profile\nname = \"X\" # shown\n\n[settings]\nweb3 = true\n",
        )
        .unwrap();
        write_id(&p, "abc123").unwrap();
        let text = fs::read_to_string(&p).unwrap();
        assert!(
            text.starts_with("# my profile\nname = \"X\" # shown\nid = \"abc123\"\n"),
            "{text}"
        );
        fs::write(&p, "id = \"\" # fill me\n").unwrap();
        write_id(&p, "def456").unwrap();
        assert_eq!(
            fs::read_to_string(&p).unwrap(),
            "id = \"def456\" # fill me\n"
        );
    }

    #[test]
    fn imported_profile_layout() {
        let live: Profile = serde_json::from_value(json!({
            "name": "Casa",
            "security": { "nrd": true, "tlds": [{ "id": "zip" }, { "id": "ru" }] },
            "parentalControl": { "safeSearch": true, "categories": [{ "id": "porn", "active": true }] },
            "denylist": [{ "id": "b.example", "active": false }, { "id": "a.example", "active": true }],
            "settings": { "logs": { "retention": 7_776_000, "drop": { "ip": false } }, "web3": true }
        }))
        .unwrap();
        let text = profile_toml("abc123", &live);
        let expected = "\
id   = \"abc123\"
name = \"Casa\"

[security]
nrd  = true
tlds = [\"ru\", \"zip\"]

[parentalControl]
safeSearch = true

[parentalControl.categories]
porn = true

[denylist]
mode = \"authoritative\"

[denylist.items]
\"a.example\" = true
\"b.example\" = false

[settings]
web3 = true

[settings.logs]
retention = \"90d\"

[settings.logs.drop]
ip = false
";
        assert_eq!(text, expected);
    }
}
