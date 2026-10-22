//! Types of the configuration files, as written by the user.
//!
//! Profile and template files use the API's camelCase keys so they read like
//! the NextDNS documentation; the root file uses `snake_case` keys.

use std::{collections::BTreeMap, fmt};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize, Serializer};

/// Root file, `ndac.toml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "ndac root configuration (ndac.toml)")]
pub struct RootFile {
    /// Configuration format version. Only `1` exists.
    pub schema: u32,
    /// Globs, relative to the root directory, of template and profile files.
    /// Files under `templates/` are templates; all others are profiles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    /// API client settings.
    #[serde(default)]
    pub provider: ProviderConfig,
    /// API key resolution.
    #[serde(default)]
    pub auth: AuthConfig,
    /// Terminal output.
    #[serde(default)]
    pub output: OutputConfig,
}

/// Local override file, `ndac.local.toml`. Must not be committed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalFile {
    /// API client settings.
    #[serde(default)]
    pub provider: ProviderConfig,
    /// API key resolution.
    #[serde(default)]
    pub auth: AuthConfig,
    /// Terminal output.
    #[serde(default)]
    pub output: OutputConfig,
}

/// Per-user defaults, `$XDG_CONFIG_HOME/ndac/config.toml`. Repository files
/// take precedence over it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "ndac user configuration")]
pub struct UserFile {
    /// API key resolution.
    #[serde(default)]
    pub auth: AuthConfig,
    /// Terminal output.
    #[serde(default)]
    pub output: OutputConfig,
}

/// `[provider]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    /// API base URL. Only meant for tests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
    /// Per-request timeout, e.g. `"15s"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<DurationValue>,
    /// Retries after a failed request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retries: Option<u32>,
    /// Maximum requests in flight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<usize>,
    /// Directory for snapshots, relative to the root directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_dir: Option<String>,
    /// Snapshots kept per profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_keep: Option<usize>,
}

/// `[auth]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    /// Ordered key sources, e.g. `["env:NEXTDNS_API_KEY", "keyring:nextdns/default"]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<String>>,
    /// Literal API key. Prefer `ndac.local.toml` or another source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<SecretValue>,
    /// What to do when a source exists but fails.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_source_error: Option<OnSourceError>,
    /// Downgrade insecure file permissions from an error to a warning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_insecure_permissions: Option<bool>,
    /// Silence the warning printed when a literal key is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet_literal: Option<bool>,
    /// Timeout for `cmd:` sources, e.g. `"30s"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmd_timeout: Option<DurationValue>,
}

/// Behavior when an auth source exists but fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum OnSourceError {
    /// Abort (default).
    Fail,
    /// Try the next source.
    Skip,
}

/// `[output]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    /// Colored output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorChoice>,
}

/// Color mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ColorChoice {
    /// Color when writing to a terminal and `NO_COLOR` is unset.
    #[default]
    Auto,
    /// Always color.
    Always,
    /// Never color.
    Never,
}

/// A secret string. Serializes and prints as `<redacted>`.
#[derive(Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct SecretValue(#[schemars(with = "String")] String);

impl SecretValue {
    /// The secret value.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl Serialize for SecretValue {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str("<redacted>")
    }
}

/// A duration: integer seconds or a string such as `"90d"` or `"1h30m"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum DurationValue {
    /// Seconds.
    Seconds(u64),
    /// Units `s`, `m`, `h`, `d`, `w`, `y` (365 days), combinable.
    Text(String),
}

/// A template or profile file. The file stem is the key.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[schemars(title = "ndac profile or template")]
pub struct ProfileFile {
    /// NextDNS profile ID. Empty or absent: created on apply. Profiles only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Profile name. Profiles only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Templates to deep-merge first, in order; later ones win.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extends: Vec<String>,
    /// Dotted paths never planned, e.g. `parentalControl.services`. Profiles only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<String>,
    /// Per-profile auth override, e.g. for a separate account. Profiles only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthConfig>,
    /// Security settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security: Option<SecurityConfig>,
    /// Privacy settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub privacy: Option<PrivacyConfig>,
    /// Parental control settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parental_control: Option<ParentalControlConfig>,
    /// Blocked domains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub denylist: Option<DomainListConfig>,
    /// Allowed domains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowlist: Option<DomainListConfig>,
    /// General settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<SettingsConfig>,
    /// Settings for `ndac endpoint render`; never sent to NextDNS. Profiles
    /// only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<EndpointConfig>,
}

/// `[endpoint]`: options for the NextDNS client configuration that
/// `ndac endpoint render --target nextdns-cli` prints.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EndpointConfig {
    /// Other profiles for some clients, first match wins: a CIDR
    /// (`"10.0.3.0/24"`), MAC address, interface name or `@user`, mapped to
    /// a profile key or NextDNS profile ID.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub conditional: BTreeMap<String, String>,
    /// Client cache size, e.g. `"10MB"`; `"0"` disables the cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_size: Option<String>,
    /// Send client names and models with queries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_client_info: Option<bool>,
}

/// `[security]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools, reason = "mirrors the API shape")]
pub struct SecurityConfig {
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
    /// Blocked TLDs, e.g. `["ru", "zip"]`. `"$inherit"` expands to the
    /// inherited list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tlds: Option<Vec<String>>,
}

/// `[privacy]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PrivacyConfig {
    /// Blocklist IDs, e.g. `["nextdns-recommended", "oisd"]`. `"$inherit"`
    /// expands to the inherited list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocklists: Option<Vec<String>>,
    /// Native tracking IDs, e.g. `["samsung"]`. `"$inherit"` expands to the
    /// inherited list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub natives: Option<Vec<String>>,
    /// Block disguised third-party trackers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disguised_trackers: Option<bool>,
    /// Allow affiliate and tracking links.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_affiliate: Option<bool>,
}

/// `[parentalControl]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ParentalControlConfig {
    /// Service ID to active flag, e.g. `{ tiktok = true }`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub services: Option<ItemMap>,
    /// Category ID to active flag, e.g. `{ gambling = true }`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<ItemMap>,
    /// Enforce SafeSearch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_search: Option<bool>,
    /// Enforce YouTube restricted mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub youtube_restricted_mode: Option<bool>,
    /// Block bypass methods.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_bypass: Option<bool>,
}

/// `[denylist]` or `[allowlist]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DomainListConfig {
    /// `authoritative` (default) removes live items absent here; `merge`
    /// only manages the listed items. Profiles only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ListMode>,
    /// Domain to active flag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<ItemMap>,
}

/// How a list is reconciled.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum ListMode {
    /// The configuration is the complete list.
    #[default]
    Authoritative,
    /// Only listed items are managed.
    Merge,
}

/// ID to active flag. `"$remove"` deletes an inherited key.
pub type ItemMap = BTreeMap<String, ItemValue>;

/// Value of an [`ItemMap`] entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ItemValue {
    /// Present, active or inactive.
    Active(bool),
    /// Remove the inherited entry.
    Marker(Marker),
}

/// Merge markers usable in item maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Marker {
    /// Delete the inherited key.
    #[serde(rename = "$remove")]
    Remove,
}

/// `[settings]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SettingsConfig {
    /// Query logging.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logs: Option<LogsConfig>,
    /// Block page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_page: Option<BlockPageConfig>,
    /// Performance options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub performance: Option<PerformanceConfig>,
    /// Web3 domain resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web3: Option<bool>,
}

/// `[settings.logs]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LogsConfig {
    /// Logging enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Data dropped from logs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop: Option<LogsDropConfig>,
    /// Retention: integer seconds or e.g. `"30d"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention: Option<DurationValue>,
    /// Storage location, e.g. `"eu"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

/// `[settings.logs.drop]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LogsDropConfig {
    /// Do not log client IPs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<bool>,
    /// Do not log domains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<bool>,
}

/// `[settings.blockPage]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BlockPageConfig {
    /// Block page enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

/// `[settings.performance]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PerformanceConfig {
    /// EDNS Client Subnet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ecs: Option<bool>,
    /// Cache boost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_boost: Option<bool>,
    /// CNAME flattening.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cname_flattening: Option<bool>,
}
