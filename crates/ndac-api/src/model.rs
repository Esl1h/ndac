//! Profile models, following the documented profile shape.
//!
//! Every field is optional so the same types serve as `PATCH` bodies and
//! tolerate partial responses. Fields the API returns but this crate does not
//! know are kept in `extra` and serialized back unchanged.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Fields not modeled by this crate, preserved as-is.
pub type Extra = BTreeMap<String, Value>;

/// A full NextDNS profile.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// Display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Security settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security: Option<Security>,
    /// Privacy settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub privacy: Option<Privacy>,
    /// Parental control settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parental_control: Option<ParentalControl>,
    /// Blocked domains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub denylist: Option<Vec<ActiveItem>>,
    /// Allowed domains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowlist: Option<Vec<ActiveItem>>,
    /// General settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Settings>,
    /// Unknown fields.
    #[serde(flatten)]
    pub extra: Extra,
}

/// `security` object.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools, reason = "mirrors the API shape")]
pub struct Security {
    /// Threat intelligence feeds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threat_intelligence_feeds: Option<bool>,
    /// AI-driven threat detection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_threat_detection: Option<bool>,
    /// Google Safe Browsing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub google_safe_browsing: Option<bool>,
    /// Cryptojacking protection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cryptojacking: Option<bool>,
    /// DNS rebinding protection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns_rebinding: Option<bool>,
    /// IDN homograph attack protection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idn_homographs: Option<bool>,
    /// Typosquatting protection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typosquatting: Option<bool>,
    /// Domain generation algorithm protection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dga: Option<bool>,
    /// Block newly registered domains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nrd: Option<bool>,
    /// Block dynamic DNS hostnames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ddns: Option<bool>,
    /// Block parked domains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parking: Option<bool>,
    /// Block child sexual abuse material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub csam: Option<bool>,
    /// Blocked top-level domains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tlds: Option<Vec<IdItem>>,
    /// Unknown fields.
    #[serde(flatten)]
    pub extra: Extra,
}

/// `privacy` object.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Privacy {
    /// Enabled blocklists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocklists: Option<Vec<IdItem>>,
    /// Blocked native tracking (vendor telemetry).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub natives: Option<Vec<IdItem>>,
    /// Block disguised third-party trackers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disguised_trackers: Option<bool>,
    /// Allow affiliate and tracking links.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_affiliate: Option<bool>,
    /// Unknown fields.
    #[serde(flatten)]
    pub extra: Extra,
}

/// `parentalControl` object.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParentalControl {
    /// Blocked services.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub services: Option<Vec<ActiveItem>>,
    /// Blocked categories.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<ActiveItem>>,
    /// Enforce SafeSearch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_search: Option<bool>,
    /// Enforce YouTube restricted mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub youtube_restricted_mode: Option<bool>,
    /// Block bypass methods.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_bypass: Option<bool>,
    /// Unknown fields.
    #[serde(flatten)]
    pub extra: Extra,
}

/// `settings` object.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Query logging.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logs: Option<Logs>,
    /// Block page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_page: Option<BlockPage>,
    /// Performance options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub performance: Option<Performance>,
    /// Web3 domain resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web3: Option<bool>,
    /// Unknown fields.
    #[serde(flatten)]
    pub extra: Extra,
}

/// `settings.logs` object.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Logs {
    /// Logging enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Data dropped from logs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop: Option<LogsDrop>,
    /// Retention in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention: Option<u64>,
    /// Storage location, e.g. `eu`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Unknown fields.
    #[serde(flatten)]
    pub extra: Extra,
}

/// `settings.logs.drop` object.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogsDrop {
    /// Do not log client IPs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<bool>,
    /// Do not log domains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<bool>,
    /// Unknown fields.
    #[serde(flatten)]
    pub extra: Extra,
}

/// `settings.blockPage` object.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockPage {
    /// Block page enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Unknown fields.
    #[serde(flatten)]
    pub extra: Extra,
}

/// `settings.performance` object.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Performance {
    /// EDNS Client Subnet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ecs: Option<bool>,
    /// Cache boost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_boost: Option<bool>,
    /// CNAME flattening.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cname_flattening: Option<bool>,
    /// Unknown fields.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A list item identified only by `id` (TLDs, blocklists, natives).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdItem {
    /// Item identifier.
    pub id: String,
    /// Unknown fields, e.g. descriptive metadata returned on `GET`.
    #[serde(flatten)]
    pub extra: Extra,
}

impl IdItem {
    /// Creates an item with no extra fields.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            extra: Extra::new(),
        }
    }
}

/// A list item with an `active` flag (services, categories, domains).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveItem {
    /// Item identifier, e.g. a domain.
    pub id: String,
    /// Whether the item is in effect. The documented shape always includes
    /// it; a missing value is assumed to be `true`.
    #[serde(default = "default_active")]
    pub active: bool,
    /// Unknown fields.
    #[serde(flatten)]
    pub extra: Extra,
}

impl ActiveItem {
    /// Creates an item with no extra fields.
    #[must_use]
    pub fn new(id: impl Into<String>, active: bool) -> Self {
        Self {
            id: id.into(),
            active,
            extra: Extra::new(),
        }
    }
}

fn default_active() -> bool {
    true
}

/// An entry of `GET /profiles`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileSummary {
    /// Profile ID.
    pub id: String,
    /// Display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Unknown fields.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Profile {
    /// Dotted paths of every unknown field, e.g. `settings.logs.foo`.
    #[must_use]
    pub fn unknown_fields(&self) -> Vec<String> {
        let mut out = Vec::new();
        push_keys(&mut out, "", &self.extra);
        if let Some(s) = &self.security {
            push_keys(&mut out, "security.", &s.extra);
        }
        if let Some(p) = &self.privacy {
            push_keys(&mut out, "privacy.", &p.extra);
        }
        if let Some(p) = &self.parental_control {
            push_keys(&mut out, "parentalControl.", &p.extra);
        }
        if let Some(s) = &self.settings {
            push_keys(&mut out, "settings.", &s.extra);
            if let Some(l) = &s.logs {
                push_keys(&mut out, "settings.logs.", &l.extra);
                if let Some(d) = &l.drop {
                    push_keys(&mut out, "settings.logs.drop.", &d.extra);
                }
            }
            if let Some(b) = &s.block_page {
                push_keys(&mut out, "settings.blockPage.", &b.extra);
            }
            if let Some(p) = &s.performance {
                push_keys(&mut out, "settings.performance.", &p.extra);
            }
        }
        out
    }
}

fn push_keys(out: &mut Vec<String>, prefix: &str, extra: &Extra) {
    out.extend(extra.keys().map(|k| format!("{prefix}{k}")));
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn documented_example() -> Value {
        json!({
          "name": "My profile",
          "security": {
            "threatIntelligenceFeeds": true, "aiThreatDetection": true,
            "googleSafeBrowsing": true, "cryptojacking": true, "dnsRebinding": true,
            "idnHomographs": true, "typosquatting": true, "dga": true, "nrd": true,
            "ddns": true, "parking": true, "csam": true,
            "tlds": [{ "id": "ru" }, { "id": "cn" }]
          },
          "privacy": {
            "blocklists": [{ "id": "nextdns-recommended" }, { "id": "oisd" }],
            "natives": [{ "id": "huawei" }, { "id": "samsung" }],
            "disguisedTrackers": true, "allowAffiliate": true
          },
          "parentalControl": {
            "services": [{ "id": "tiktok", "active": true }],
            "categories": [{ "id": "porn", "active": true }],
            "safeSearch": true, "youtubeRestrictedMode": true, "blockBypass": false
          },
          "denylist": [{ "id": "badwebsite.com", "active": true }],
          "allowlist": [{ "id": "nytimes.com", "active": false }],
          "settings": {
            "logs": {
              "enabled": true, "drop": { "ip": false, "domain": false },
              "retention": 7_776_000, "location": "eu"
            },
            "blockPage": { "enabled": true },
            "performance": { "ecs": true, "cacheBoost": false, "cnameFlattening": true },
            "web3": true
          }
        })
    }

    #[test]
    fn documented_profile_round_trips() {
        let value = documented_example();
        let profile: Profile = serde_json::from_value(value.clone()).expect("deserialize");
        assert!(profile.unknown_fields().is_empty());
        assert_eq!(serde_json::to_value(&profile).expect("serialize"), value);
    }

    #[test]
    fn unknown_fields_are_preserved_and_reported() {
        let mut value = documented_example();
        value["fingerprint"] = json!("fp0000");
        value["settings"]["logs"]["future"] = json!({ "x": 1 });
        value["denylist"][0]["note"] = json!("kept");
        let profile: Profile = serde_json::from_value(value.clone()).expect("deserialize");
        assert_eq!(
            profile.unknown_fields(),
            ["fingerprint", "settings.logs.future"]
        );
        assert_eq!(serde_json::to_value(&profile).expect("serialize"), value);
    }

    #[test]
    fn partial_profile_serializes_only_set_fields() {
        let patch = Settings {
            web3: Some(false),
            ..Settings::default()
        };
        assert_eq!(
            serde_json::to_value(patch).expect("serialize"),
            json!({ "web3": false })
        );
    }

    #[test]
    fn missing_active_defaults_to_true() {
        let item: ActiveItem = serde_json::from_value(json!({ "id": "a.example" })).expect("ok");
        assert!(item.active);
    }
}
