//! The canonical desired state of a profile.
//!
//! Only fields set in configuration are present: anything left out is not
//! managed. Durations are seconds, domains are lowercase punycode without a
//! trailing dot, ID arrays become `[{ id }]` and item maps `[{ id, active }]`,
//! all sorted by ID. NextDNS list order is assumed not to matter.

use std::{collections::BTreeMap, path::PathBuf};

use ndac_api::{
    ActiveItem, BlockPage, IdItem, Logs, LogsDrop, ParentalControl, Performance, Privacy, Profile,
    Security, Settings,
};
use serde::Serialize;

use super::{
    check::INHERIT,
    types::{AuthConfig, ItemMap, ItemValue, ListMode, ProfileFile},
    values::{duration_secs, normalize_domain},
};

/// Version of the `render --format json` document.
pub const RENDER_FORMAT_VERSION: u32 = 1;

/// A fully resolved, normalized profile.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DesiredProfile {
    /// File stem, e.g. `casa` for `profiles/casa.toml`.
    pub key: String,
    /// NextDNS profile ID; `None` means the profile is created on apply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Mode of each domain list present in configuration.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub list_modes: BTreeMap<String, ListMode>,
    /// Paths excluded from planning.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<String>,
    /// Per-profile auth override. A literal key renders as `<redacted>`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthConfig>,
    /// Desired state in API shape.
    pub desired: Profile,
    /// File the profile was loaded from.
    #[serde(skip)]
    pub source: PathBuf,
}

#[derive(Serialize)]
struct Rendered<'a> {
    format_version: u32,
    #[serde(flatten)]
    profile: &'a DesiredProfile,
}

impl DesiredProfile {
    /// Normalizes an already merged file (no `extends` left to apply).
    #[must_use]
    pub fn from_resolved(key: &str, file: ProfileFile) -> Self {
        let mut file = file;
        prenormalize(&mut file);
        desired(key, file, PathBuf::new())
    }

    /// `render --format json` output.
    #[must_use]
    pub fn to_json(&self) -> String {
        let doc = Rendered {
            format_version: RENDER_FORMAT_VERSION,
            profile: self,
        };
        let mut s = serde_json::to_string_pretty(&doc).unwrap_or_default();
        s.push('\n');
        s
    }

    /// `render --format toml` output.
    #[must_use]
    pub fn to_toml(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }
}

/// Canonicalizes keys in place before merging so that equal entries written
/// differently (`Example.com` and `example.com.`) merge as one. Invalid
/// values are left as-is; validation reports them.
pub(crate) fn prenormalize(f: &mut ProfileFile) {
    for list in [&mut f.denylist, &mut f.allowlist].into_iter().flatten() {
        if let Some(items) = list.items.take() {
            list.items = Some(
                items
                    .into_iter()
                    .map(|(k, v)| (normalize_domain(&k).unwrap_or(k), v))
                    .collect(),
            );
        }
    }
    if let Some(tlds) = f.security.as_mut().and_then(|s| s.tlds.as_mut()) {
        for t in tlds.iter_mut() {
            *t = normalize_tld(t);
        }
    }
}

/// The form a TLD takes in configuration: no surrounding space or leading
/// dot, lowercase.
pub(crate) fn normalize_tld(t: &str) -> String {
    t.trim().trim_start_matches('.').to_lowercase()
}

/// Builds the canonical profile from a resolved (merged) file.
pub(crate) fn desired(key: &str, f: ProfileFile, source: PathBuf) -> DesiredProfile {
    let mut list_modes = BTreeMap::new();
    if let Some(d) = &f.denylist {
        list_modes.insert("denylist".to_owned(), d.mode.unwrap_or_default());
    }
    if let Some(a) = &f.allowlist {
        list_modes.insert("allowlist".to_owned(), a.mode.unwrap_or_default());
    }
    let desired = Profile {
        name: f.name,
        security: f.security.map(|s| Security {
            threat_intelligence_feeds: s.threat_intelligence_feeds,
            ai_threat_detection: s.ai_threat_detection,
            google_safe_browsing: s.google_safe_browsing,
            cryptojacking: s.cryptojacking,
            dns_rebinding: s.dns_rebinding,
            idn_homographs: s.idn_homographs,
            typosquatting: s.typosquatting,
            dga: s.dga,
            nrd: s.nrd,
            ddns: s.ddns,
            parking: s.parking,
            csam: s.csam,
            tlds: s.tlds.map(id_items),
            extra: BTreeMap::new(),
        }),
        privacy: f.privacy.map(|p| Privacy {
            blocklists: p.blocklists.map(id_items),
            natives: p.natives.map(id_items),
            disguised_trackers: p.disguised_trackers,
            allow_affiliate: p.allow_affiliate,
            extra: BTreeMap::new(),
        }),
        parental_control: f.parental_control.map(|pc| ParentalControl {
            services: pc.services.map(active_items),
            categories: pc.categories.map(active_items),
            safe_search: pc.safe_search,
            youtube_restricted_mode: pc.youtube_restricted_mode,
            block_bypass: pc.block_bypass,
            extra: BTreeMap::new(),
        }),
        denylist: f
            .denylist
            .map(|d| active_items(d.items.unwrap_or_default())),
        allowlist: f
            .allowlist
            .map(|a| active_items(a.items.unwrap_or_default())),
        settings: f.settings.map(|s| Settings {
            logs: s.logs.map(|l| Logs {
                enabled: l.enabled,
                drop: l.drop.map(|d| LogsDrop {
                    ip: d.ip,
                    domain: d.domain,
                    extra: BTreeMap::new(),
                }),
                retention: l.retention.and_then(|r| duration_secs(&r).ok()),
                location: l.location,
                extra: BTreeMap::new(),
            }),
            block_page: s.block_page.map(|b| BlockPage {
                enabled: b.enabled,
                extra: BTreeMap::new(),
            }),
            performance: s.performance.map(|p| Performance {
                ecs: p.ecs,
                cache_boost: p.cache_boost,
                cname_flattening: p.cname_flattening,
                extra: BTreeMap::new(),
            }),
            web3: s.web3,
            extra: BTreeMap::new(),
        }),
        extra: BTreeMap::new(),
    };
    DesiredProfile {
        key: key.to_owned(),
        id: f.id.filter(|id| !id.is_empty()),
        list_modes,
        ignore: f.ignore,
        auth: f.auth,
        desired,
        source,
    }
}

fn id_items(mut ids: Vec<String>) -> Vec<IdItem> {
    ids.retain(|id| id != INHERIT);
    ids.sort();
    ids.dedup();
    ids.into_iter().map(IdItem::new).collect()
}

fn active_items(map: ItemMap) -> Vec<ActiveItem> {
    map.into_iter()
        .filter_map(|(id, v)| match v {
            ItemValue::Active(a) => Some(ActiveItem::new(id, a)),
            ItemValue::Marker(_) => None,
        })
        .collect()
}
