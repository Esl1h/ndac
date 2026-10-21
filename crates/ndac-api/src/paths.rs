//! Child endpoints of a profile.
//!
//! Only objects and arrays from the documented profile shape are listed.
//! Each path knows its URL segments and an endpoint template used in logs
//! and errors, so profile IDs never reach `info`-level output.

/// An object endpoint under `/profiles/:profile`, supporting `GET` and
/// `PATCH`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ObjectPath {
    /// `/security`
    Security,
    /// `/privacy`
    Privacy,
    /// `/parentalControl`
    ParentalControl,
    /// `/settings`
    Settings,
    /// `/settings/logs`
    SettingsLogs,
    /// `/settings/logs/drop`
    SettingsLogsDrop,
    /// `/settings/blockPage`
    SettingsBlockPage,
    /// `/settings/performance`
    SettingsPerformance,
}

impl ObjectPath {
    /// Every object path, parents before children.
    pub const ALL: [Self; 8] = [
        Self::Security,
        Self::Privacy,
        Self::ParentalControl,
        Self::Settings,
        Self::SettingsLogs,
        Self::SettingsLogsDrop,
        Self::SettingsBlockPage,
        Self::SettingsPerformance,
    ];

    /// URL segments below `/profiles/:profile`.
    #[must_use]
    pub fn segments(self) -> &'static [&'static str] {
        match self {
            Self::Security => &["security"],
            Self::Privacy => &["privacy"],
            Self::ParentalControl => &["parentalControl"],
            Self::Settings => &["settings"],
            Self::SettingsLogs => &["settings", "logs"],
            Self::SettingsLogsDrop => &["settings", "logs", "drop"],
            Self::SettingsBlockPage => &["settings", "blockPage"],
            Self::SettingsPerformance => &["settings", "performance"],
        }
    }

    /// Endpoint template, e.g. `/profiles/:profile/settings/logs`.
    #[must_use]
    pub fn template(self) -> &'static str {
        match self {
            Self::Security => "/profiles/:profile/security",
            Self::Privacy => "/profiles/:profile/privacy",
            Self::ParentalControl => "/profiles/:profile/parentalControl",
            Self::Settings => "/profiles/:profile/settings",
            Self::SettingsLogs => "/profiles/:profile/settings/logs",
            Self::SettingsLogsDrop => "/profiles/:profile/settings/logs/drop",
            Self::SettingsBlockPage => "/profiles/:profile/settings/blockPage",
            Self::SettingsPerformance => "/profiles/:profile/settings/performance",
        }
    }
}

/// An array endpoint under `/profiles/:profile`, supporting `GET`, `PUT`
/// and `POST`; its items support `PATCH` and `DELETE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ListPath {
    /// `/security/tlds`
    SecurityTlds,
    /// `/privacy/blocklists`
    PrivacyBlocklists,
    /// `/privacy/natives`
    PrivacyNatives,
    /// `/parentalControl/services`
    ParentalControlServices,
    /// `/parentalControl/categories`
    ParentalControlCategories,
    /// `/denylist`
    Denylist,
    /// `/allowlist`
    Allowlist,
}

impl ListPath {
    /// Every list path.
    pub const ALL: [Self; 7] = [
        Self::SecurityTlds,
        Self::PrivacyBlocklists,
        Self::PrivacyNatives,
        Self::ParentalControlServices,
        Self::ParentalControlCategories,
        Self::Denylist,
        Self::Allowlist,
    ];

    /// URL segments below `/profiles/:profile`.
    #[must_use]
    pub fn segments(self) -> &'static [&'static str] {
        match self {
            Self::SecurityTlds => &["security", "tlds"],
            Self::PrivacyBlocklists => &["privacy", "blocklists"],
            Self::PrivacyNatives => &["privacy", "natives"],
            Self::ParentalControlServices => &["parentalControl", "services"],
            Self::ParentalControlCategories => &["parentalControl", "categories"],
            Self::Denylist => &["denylist"],
            Self::Allowlist => &["allowlist"],
        }
    }

    /// Endpoint template, e.g. `/profiles/:profile/denylist`.
    #[must_use]
    pub fn template(self) -> &'static str {
        match self {
            Self::SecurityTlds => "/profiles/:profile/security/tlds",
            Self::PrivacyBlocklists => "/profiles/:profile/privacy/blocklists",
            Self::PrivacyNatives => "/profiles/:profile/privacy/natives",
            Self::ParentalControlServices => "/profiles/:profile/parentalControl/services",
            Self::ParentalControlCategories => "/profiles/:profile/parentalControl/categories",
            Self::Denylist => "/profiles/:profile/denylist",
            Self::Allowlist => "/profiles/:profile/allowlist",
        }
    }

    /// Endpoint template of one item, e.g. `/profiles/:profile/denylist/:id`.
    #[must_use]
    pub fn item_template(self) -> &'static str {
        match self {
            Self::SecurityTlds => "/profiles/:profile/security/tlds/:id",
            Self::PrivacyBlocklists => "/profiles/:profile/privacy/blocklists/:id",
            Self::PrivacyNatives => "/profiles/:profile/privacy/natives/:id",
            Self::ParentalControlServices => "/profiles/:profile/parentalControl/services/:id",
            Self::ParentalControlCategories => "/profiles/:profile/parentalControl/categories/:id",
            Self::Denylist => "/profiles/:profile/denylist/:id",
            Self::Allowlist => "/profiles/:profile/allowlist/:id",
        }
    }

    /// True if items carry an `active` flag (`{ id, active }`), false for
    /// plain `{ id }` items.
    #[must_use]
    pub fn has_active_flag(self) -> bool {
        matches!(
            self,
            Self::ParentalControlServices
                | Self::ParentalControlCategories
                | Self::Denylist
                | Self::Allowlist
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_match_segments() {
        for p in ObjectPath::ALL {
            assert_eq!(
                p.template(),
                format!("/profiles/:profile/{}", p.segments().join("/"))
            );
        }
        for p in ListPath::ALL {
            let base = format!("/profiles/:profile/{}", p.segments().join("/"));
            assert_eq!(p.template(), base);
            assert_eq!(p.item_template(), format!("{base}/:id"));
        }
    }
}
