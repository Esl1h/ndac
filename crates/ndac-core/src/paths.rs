//! Dotted paths of every managed field, as used by `ignore`, `--target` and
//! plan output. They follow the API's camelCase names.

/// Every leaf field and list of a profile.
pub const FIELD_PATHS: &[&str] = &[
    "name",
    "security.threatIntelligenceFeeds",
    "security.aiThreatDetection",
    "security.googleSafeBrowsing",
    "security.cryptojacking",
    "security.dnsRebinding",
    "security.idnHomographs",
    "security.typosquatting",
    "security.dga",
    "security.nrd",
    "security.ddns",
    "security.parking",
    "security.csam",
    "security.tlds",
    "privacy.blocklists",
    "privacy.natives",
    "privacy.disguisedTrackers",
    "privacy.allowAffiliate",
    "parentalControl.services",
    "parentalControl.categories",
    "parentalControl.safeSearch",
    "parentalControl.youtubeRestrictedMode",
    "parentalControl.blockBypass",
    "denylist",
    "allowlist",
    // UNVERIFIED endpoint; only set with the `unstable-endpoints` feature.
    "rewrites",
    "settings.logs.enabled",
    "settings.logs.drop.ip",
    "settings.logs.drop.domain",
    "settings.logs.retention",
    "settings.logs.location",
    "settings.blockPage.enabled",
    "settings.performance.ecs",
    "settings.performance.cacheBoost",
    "settings.performance.cnameFlattening",
    "settings.web3",
];

/// True if `path` is a field path or a group of them, e.g. `settings.logs`.
#[must_use]
pub fn is_known(path: &str) -> bool {
    FIELD_PATHS.iter().any(|p| covers(path, p))
}

/// True if `prefix` is `path` itself or one of its parent groups.
#[must_use]
pub fn covers(prefix: &str, path: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('.'))
}

/// Every field path and group, for suggestions.
#[must_use]
pub fn all_paths_and_groups() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in FIELD_PATHS {
        let mut acc = String::new();
        for (i, seg) in p.split('.').enumerate() {
            if i > 0 {
                acc.push('.');
            }
            acc.push_str(seg);
            if !out.contains(&acc) {
                out.push(acc.clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_and_leaves() {
        assert!(is_known("settings.logs"));
        assert!(is_known("settings.logs.retention"));
        assert!(is_known("denylist"));
        assert!(!is_known("settings.log"));
        assert!(!is_known("settings.logs.retention.x"));
        assert!(covers("settings", "settings.web3"));
        assert!(!covers("set", "settings.web3"));
        assert!(all_paths_and_groups().contains(&"settings.logs.drop".to_owned()));
    }
}
