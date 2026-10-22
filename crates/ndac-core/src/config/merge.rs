//! `extends` merge semantics.
//!
//! - tables merge recursively and scalars take the last value;
//! - ID arrays replace the inherited array, except that `"$inherit"`
//!   expands to it in place; duplicates keep their first occurrence;
//! - item maps merge by key, and `"$remove"` deletes an inherited key;
//! - `id`, `name`, `ignore`, `auth`, `endpoint` and list `mode` are never
//!   inherited.
//!
//! A profile resolves by applying layers in order onto an empty file: each
//! template in `extends` (its own `extends` first, recursively), then the
//! profile. Markers refer to everything applied before them, and every merge
//! produces marker-free output.

use super::{
    check::INHERIT,
    types::{
        BlockPageConfig, DomainListConfig, ItemMap, ItemValue, LogsConfig, LogsDropConfig,
        ParentalControlConfig, PerformanceConfig, PrivacyConfig, ProfileFile, SecurityConfig,
        SettingsConfig,
    },
};

/// Merges `over` onto `base`. Non-inheritable fields come from `over` only.
#[must_use]
pub fn merge(base: ProfileFile, over: ProfileFile) -> ProfileFile {
    ProfileFile {
        id: over.id,
        name: over.name,
        extends: Vec::new(),
        ignore: over.ignore,
        auth: over.auth,
        security: opt(base.security, over.security, security),
        privacy: opt(base.privacy, over.privacy, privacy),
        parental_control: opt(base.parental_control, over.parental_control, parental),
        denylist: domain_list(base.denylist, over.denylist),
        allowlist: domain_list(base.allowlist, over.allowlist),
        #[cfg(feature = "unstable-endpoints")]
        rewrites: rewrites(base.rewrites, over.rewrites),
        settings: opt(base.settings, over.settings, settings),
        endpoint: over.endpoint,
    }
}

fn opt<T>(base: Option<T>, over: Option<T>, f: fn(T, T) -> T) -> Option<T> {
    match (base, over) {
        (Some(b), Some(o)) => Some(f(b, o)),
        (b, o) => o.or(b),
    }
}

/// Merges an ID array. Always returns a marker-free list when `over` is set.
#[must_use]
pub fn ids(base: Option<Vec<String>>, over: Option<Vec<String>>) -> Option<Vec<String>> {
    let Some(over) = over else {
        return base;
    };
    let inherited = base.unwrap_or_default();
    let mut out: Vec<String> = Vec::with_capacity(over.len() + inherited.len());
    for id in over {
        if id == INHERIT {
            out.extend(inherited.iter().cloned());
        } else {
            out.push(id);
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|id| seen.insert(id.clone()));
    Some(out)
}

/// Merges an item map. `"$remove"` entries are consumed.
#[must_use]
pub fn items(base: Option<ItemMap>, over: Option<ItemMap>) -> Option<ItemMap> {
    let Some(over) = over else {
        return base;
    };
    let mut out = base.unwrap_or_default();
    for (k, v) in over {
        match v {
            ItemValue::Active(_) => {
                out.insert(k, v);
            }
            ItemValue::Marker(_) => {
                out.remove(&k);
            }
        }
    }
    Some(out)
}

fn security(b: SecurityConfig, o: SecurityConfig) -> SecurityConfig {
    SecurityConfig {
        threat_intelligence_feeds: o.threat_intelligence_feeds.or(b.threat_intelligence_feeds),
        ai_threat_detection: o.ai_threat_detection.or(b.ai_threat_detection),
        google_safe_browsing: o.google_safe_browsing.or(b.google_safe_browsing),
        cryptojacking: o.cryptojacking.or(b.cryptojacking),
        dns_rebinding: o.dns_rebinding.or(b.dns_rebinding),
        idn_homographs: o.idn_homographs.or(b.idn_homographs),
        typosquatting: o.typosquatting.or(b.typosquatting),
        dga: o.dga.or(b.dga),
        nrd: o.nrd.or(b.nrd),
        ddns: o.ddns.or(b.ddns),
        parking: o.parking.or(b.parking),
        csam: o.csam.or(b.csam),
        tlds: ids(b.tlds, o.tlds),
    }
}

fn privacy(b: PrivacyConfig, o: PrivacyConfig) -> PrivacyConfig {
    PrivacyConfig {
        blocklists: ids(b.blocklists, o.blocklists),
        natives: ids(b.natives, o.natives),
        disguised_trackers: o.disguised_trackers.or(b.disguised_trackers),
        allow_affiliate: o.allow_affiliate.or(b.allow_affiliate),
    }
}

fn parental(b: ParentalControlConfig, o: ParentalControlConfig) -> ParentalControlConfig {
    ParentalControlConfig {
        services: items(b.services, o.services),
        categories: items(b.categories, o.categories),
        safe_search: o.safe_search.or(b.safe_search),
        youtube_restricted_mode: o.youtube_restricted_mode.or(b.youtube_restricted_mode),
        block_bypass: o.block_bypass.or(b.block_bypass),
    }
}

fn domain_list(
    b: Option<DomainListConfig>,
    o: Option<DomainListConfig>,
) -> Option<DomainListConfig> {
    match (b, o) {
        (None, None) => None,
        (b, o) => {
            let (b, o) = (b.unwrap_or_default(), o.unwrap_or_default());
            Some(DomainListConfig {
                mode: o.mode,
                items: items(b.items, o.items),
            })
        }
    }
}

#[cfg(feature = "unstable-endpoints")]
fn rewrites(
    b: Option<super::types::RewritesConfig>,
    o: Option<super::types::RewritesConfig>,
) -> Option<super::types::RewritesConfig> {
    match (b, o) {
        (None, None) => None,
        (b, o) => {
            let (b, o) = (b.unwrap_or_default(), o.unwrap_or_default());
            let items = match (b.items, o.items) {
                (b, None) => b,
                (b, Some(o)) => {
                    let mut out = b.unwrap_or_default();
                    for (k, v) in o {
                        if v == super::check::REMOVE {
                            out.remove(&k);
                        } else {
                            out.insert(k, v);
                        }
                    }
                    Some(out)
                }
            };
            Some(super::types::RewritesConfig {
                mode: o.mode,
                items,
            })
        }
    }
}

fn settings(b: SettingsConfig, o: SettingsConfig) -> SettingsConfig {
    SettingsConfig {
        logs: opt(b.logs, o.logs, logs),
        block_page: opt(b.block_page, o.block_page, |b: BlockPageConfig, o| {
            BlockPageConfig {
                enabled: o.enabled.or(b.enabled),
            }
        }),
        performance: opt(b.performance, o.performance, |b: PerformanceConfig, o| {
            PerformanceConfig {
                ecs: o.ecs.or(b.ecs),
                cache_boost: o.cache_boost.or(b.cache_boost),
                cname_flattening: o.cname_flattening.or(b.cname_flattening),
            }
        }),
        web3: o.web3.or(b.web3),
    }
}

fn logs(b: LogsConfig, o: LogsConfig) -> LogsConfig {
    LogsConfig {
        enabled: o.enabled.or(b.enabled),
        drop: opt(b.drop, o.drop, |b: LogsDropConfig, o| LogsDropConfig {
            ip: o.ip.or(b.ip),
            domain: o.domain.or(b.domain),
        }),
        retention: o.retention.or(b.retention),
        location: o.location.or(b.location),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::config::types::Marker;

    #[allow(clippy::unnecessary_wraps, reason = "builds Option arguments")]
    fn s(v: &[&str]) -> Option<Vec<String>> {
        Some(v.iter().map(|x| (*x).to_owned()).collect())
    }

    #[test]
    fn id_arrays_replace_unless_inherit() {
        assert_eq!(ids(s(&["a", "b"]), s(&["c"])), s(&["c"]));
        assert_eq!(
            ids(s(&["a", "b"]), s(&["c", "$inherit"])),
            s(&["c", "a", "b"])
        );
        assert_eq!(ids(s(&["a", "b"]), s(&["b", "$inherit"])), s(&["b", "a"]));
        assert_eq!(ids(None, s(&["$inherit", "x"])), s(&["x"]));
        assert_eq!(ids(s(&["a"]), None), s(&["a"]));
        assert_eq!(ids(s(&["a"]), s(&[])), s(&[]));
    }

    #[test]
    fn item_maps_merge_and_remove() {
        let base = BTreeMap::from([
            ("a".to_owned(), ItemValue::Active(true)),
            ("b".to_owned(), ItemValue::Active(true)),
        ]);
        let over = BTreeMap::from([
            ("b".to_owned(), ItemValue::Marker(Marker::Remove)),
            ("c".to_owned(), ItemValue::Active(false)),
            ("zz".to_owned(), ItemValue::Marker(Marker::Remove)),
        ]);
        let merged = items(Some(base), Some(over)).unwrap();
        assert_eq!(
            merged,
            BTreeMap::from([
                ("a".to_owned(), ItemValue::Active(true)),
                ("c".to_owned(), ItemValue::Active(false)),
            ])
        );
    }

    #[test]
    fn non_inheritable_fields_come_from_over() {
        let base = ProfileFile {
            id: Some("base".into()),
            name: Some("Base".into()),
            ignore: vec!["denylist".into()],
            denylist: Some(DomainListConfig {
                mode: Some(super::super::types::ListMode::Merge),
                items: None,
            }),
            ..ProfileFile::default()
        };
        let over = ProfileFile {
            denylist: Some(DomainListConfig::default()),
            ..ProfileFile::default()
        };
        let m = merge(base, over);
        assert_eq!(m.id, None);
        assert_eq!(m.name, None);
        assert_eq!(m.ignore, Vec::<String>::new());
        assert_eq!(m.denylist.unwrap().mode, None);
    }

    #[test]
    fn scalars_last_wins_and_tables_deep_merge() {
        let base = ProfileFile {
            settings: Some(SettingsConfig {
                web3: Some(true),
                logs: Some(LogsConfig {
                    enabled: Some(true),
                    location: Some("eu".into()),
                    ..LogsConfig::default()
                }),
                ..SettingsConfig::default()
            }),
            ..ProfileFile::default()
        };
        let over = ProfileFile {
            settings: Some(SettingsConfig {
                logs: Some(LogsConfig {
                    location: Some("us".into()),
                    ..LogsConfig::default()
                }),
                ..SettingsConfig::default()
            }),
            ..ProfileFile::default()
        };
        let s = merge(base, over).settings.unwrap();
        assert_eq!(s.web3, Some(true));
        let logs = s.logs.unwrap();
        assert_eq!(logs.enabled, Some(true));
        assert_eq!(logs.location.as_deref(), Some("us"));
    }
}
