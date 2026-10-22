//! Diff engine: compares canonical desired state with live state.
//!
//! The diff works on the JSON form of both profiles and only visits the
//! paths in [`crate::paths::FIELD_PATHS`]: anything not set in configuration
//! is unmanaged, unknown live fields are ignored, and paths listed in
//! `ignore` are pruned from both sides.

mod drift;
mod render;
mod target;

use std::collections::{BTreeMap, BTreeSet};

use ndac_api::Profile;
use serde::Serialize;
use serde_json::Value;

pub use drift::{drift_only, drift_sections, render_github, render_prom};
pub use render::{
    MARKDOWN_LIMIT, PLAN_FORMAT_VERSION, render_json, render_markdown, render_text, summary_line,
};
pub use target::Target;

use crate::{
    config::{DesiredProfile, ListMode, normalize_domain, normalize_tld},
    paths::{FIELD_PATHS, covers},
};

/// Paths that hold lists, and whether their items carry `active`.
pub const LIST_PATHS: &[(&str, bool)] = &[
    ("security.tlds", false),
    ("privacy.blocklists", false),
    ("privacy.natives", false),
    ("parentalControl.services", true),
    ("parentalControl.categories", true),
    ("denylist", true),
    ("allowlist", true),
];

/// Who changed what since the last successful apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// Configuration changed (or there is no snapshot to compare with).
    Pending,
    /// The live profile changed, e.g. in the dashboard.
    Drift,
    /// Both changed the same path.
    Conflict,
}

/// One difference between desired and live state.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Change {
    /// A scalar field differs.
    SetField {
        /// Dotted path, e.g. `settings.logs.retention`.
        path: String,
        /// Live value, if any.
        from: Option<Value>,
        /// Desired value.
        to: Value,
        /// Who changed it.
        origin: Origin,
    },
    /// An item is desired but not live.
    AddItem {
        /// List path, e.g. `denylist`.
        list: String,
        /// Item ID.
        id: String,
        /// `active` flag, for lists that have one.
        active: Option<bool>,
        /// Who changed it.
        origin: Origin,
    },
    /// A live item is not desired (authoritative lists only).
    RemoveItem {
        /// List path.
        list: String,
        /// Item ID.
        id: String,
        /// Who changed it.
        origin: Origin,
    },
    /// An item's `active` flag differs.
    SetItemActive {
        /// List path.
        list: String,
        /// Item ID.
        id: String,
        /// Live flag.
        from: bool,
        /// Desired flag.
        to: bool,
        /// Who changed it.
        origin: Origin,
    },
}

impl Change {
    /// The field path or list path this change belongs to.
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::SetField { path, .. } => path,
            Self::AddItem { list, .. }
            | Self::RemoveItem { list, .. }
            | Self::SetItemActive { list, .. } => list,
        }
    }

    /// Who changed it.
    #[must_use]
    pub fn origin(&self) -> Origin {
        match self {
            Self::SetField { origin, .. }
            | Self::AddItem { origin, .. }
            | Self::RemoveItem { origin, .. }
            | Self::SetItemActive { origin, .. } => *origin,
        }
    }

    fn set_origin(&mut self, o: Origin) {
        match self {
            Self::SetField { origin, .. }
            | Self::AddItem { origin, .. }
            | Self::RemoveItem { origin, .. }
            | Self::SetItemActive { origin, .. } => *origin = o,
        }
    }

    /// Item ID for list changes.
    #[must_use]
    pub fn item(&self) -> Option<&str> {
        match self {
            Self::SetField { .. } => None,
            Self::AddItem { id, .. }
            | Self::RemoveItem { id, .. }
            | Self::SetItemActive { id, .. } => Some(id),
        }
    }
}

/// Changes for one profile.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProfilePlan {
    /// Profile key (file stem).
    pub key: String,
    /// NextDNS ID; `None` when the profile will be created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The profile does not exist yet and will be created.
    pub create: bool,
    /// Mode of the domain lists present in configuration.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub list_modes: BTreeMap<String, ListMode>,
    /// Differences, in [`FIELD_PATHS`] order.
    pub changes: Vec<Change>,
}

impl ProfilePlan {
    /// True when there is nothing to do.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.create && self.changes.is_empty()
    }
}

/// A whole plan.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Plan {
    /// Per-profile plans, in key order.
    pub profiles: Vec<ProfilePlan>,
    /// Profiles with snapshots but no configuration file any more.
    pub removed: Vec<RemovedProfile>,
}

/// A profile whose file was removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemovedProfile {
    /// Profile key.
    pub key: String,
    /// Last known NextDNS ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// Totals shown at the end of a plan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// Items and profiles to add.
    pub add: usize,
    /// Fields and flags to change.
    pub change: usize,
    /// Items to remove.
    pub remove: usize,
    /// Profiles with at least one change.
    pub profiles: usize,
    /// Sections (field or list paths) with drift or conflict.
    pub drift_sections: usize,
}

impl Plan {
    /// True when no profile has changes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.profiles.iter().all(ProfilePlan::is_empty)
    }

    /// Totals.
    #[must_use]
    pub fn summary(&self) -> Summary {
        let mut s = Summary::default();
        for p in &self.profiles {
            if !p.is_empty() {
                s.profiles += 1;
            }
            if p.create {
                s.add += 1;
            }
            let mut drift = BTreeSet::new();
            for c in &p.changes {
                match c {
                    Change::AddItem { .. } => s.add += 1,
                    Change::RemoveItem { .. } => s.remove += 1,
                    Change::SetField { .. } | Change::SetItemActive { .. } => s.change += 1,
                }
                if c.origin() != Origin::Pending {
                    drift.insert(c.path());
                }
            }
            s.drift_sections += drift.len();
        }
        s
    }

    /// Keeps only changes matched by `targets` (all when empty).
    #[must_use]
    pub fn filtered(mut self, targets: &[Target]) -> Self {
        if targets.is_empty() {
            return self;
        }
        self.profiles
            .retain(|p| targets.iter().any(|t| t.matches_profile(&p.key)));
        for p in &mut self.profiles {
            let key = p.key.clone();
            p.changes
                .retain(|c| targets.iter().any(|t| t.matches(&key, c.path())));
        }
        self
    }
}

/// Reads the value at a dotted path.
#[must_use]
pub fn get<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(v, |cur, k| cur.get(k))
        .filter(|v| !v.is_null())
}

pub(crate) fn is_list(path: &str) -> Option<bool> {
    LIST_PATHS.iter().find(|(p, _)| *p == path).map(|(_, a)| *a)
}

/// Items of a list value: ID to `active` (`None` for ID-only lists).
pub(crate) fn items(v: Option<&Value>, has_active: bool) -> BTreeMap<String, Option<bool>> {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?.to_owned();
            let active =
                has_active.then(|| item.get("active").and_then(Value::as_bool).unwrap_or(true));
            Some((id, active))
        })
        .collect()
}

/// Compares `desired` with `live` (`None` when the profile does not exist
/// yet). Every change starts as [`Origin::Pending`]; see [`classify`].
#[must_use]
pub fn diff(desired: &DesiredProfile, live: Option<&Profile>) -> ProfilePlan {
    let want = serde_json::to_value(&desired.desired).unwrap_or(Value::Null);
    let have = live
        .and_then(|l| serde_json::to_value(l).ok())
        .unwrap_or(Value::Null);
    let ignored = |p: &str| desired.ignore.iter().any(|i| covers(i, p));
    let mut changes = Vec::new();

    for &path in FIELD_PATHS {
        if ignored(path) {
            continue;
        }
        let Some(target) = get(&want, path) else {
            continue;
        };
        let current = get(&have, path);
        match is_list(path) {
            None => {
                if current != Some(target) {
                    changes.push(Change::SetField {
                        path: path.to_owned(),
                        from: current.cloned(),
                        to: target.clone(),
                        origin: Origin::Pending,
                    });
                }
            }
            Some(has_active) => {
                let authoritative = desired.list_modes.get(path) != Some(&ListMode::Merge);
                list_changes(
                    path,
                    has_active,
                    authoritative,
                    target,
                    current,
                    &mut changes,
                );
            }
        }
    }

    ProfilePlan {
        key: desired.key.clone(),
        id: desired.id.clone(),
        create: live.is_none(),
        list_modes: desired.list_modes.clone(),
        changes,
    }
}

fn list_changes(
    path: &str,
    has_active: bool,
    authoritative: bool,
    target: &Value,
    current: Option<&Value>,
    out: &mut Vec<Change>,
) {
    let want = items(Some(target), has_active);
    let have = items(current, has_active);
    // Configured domains and TLDs are normalized (lowercase, punycode);
    // match live entries in another form by their normalized ID, but keep
    // the live ID for the API calls that address them.
    let canonical = |id: &str| match path {
        "denylist" | "allowlist" => normalize_domain(id).unwrap_or_else(|_| id.to_owned()),
        "security.tlds" => normalize_tld(id),
        _ => id.to_owned(),
    };
    let live_by_canonical: BTreeMap<String, (&String, &Option<bool>)> = have
        .iter()
        .map(|(id, active)| (canonical(id), (id, active)))
        .collect();
    for (id, active) in &want {
        match live_by_canonical.get(id) {
            None => out.push(Change::AddItem {
                list: path.to_owned(),
                id: id.clone(),
                active: *active,
                origin: Origin::Pending,
            }),
            Some((live_id, live_active)) => {
                if let (Some(from), Some(to)) = (live_active, active)
                    && from != to
                {
                    out.push(Change::SetItemActive {
                        list: path.to_owned(),
                        id: (*live_id).clone(),
                        from: *from,
                        to: *to,
                        origin: Origin::Pending,
                    });
                }
            }
        }
    }
    if authoritative {
        for id in have.keys().filter(|id| !want.contains_key(&canonical(id))) {
            out.push(Change::RemoveItem {
                list: path.to_owned(),
                id: id.clone(),
                origin: Origin::Pending,
            });
        }
    }
}

/// What a change touches, as a comparable value in a given profile: the
/// field value, or an item's `active` flag (`Bool(true)` for ID-only items).
fn probe(v: &Value, change: &Change) -> Option<Value> {
    match change.item() {
        None => get(v, change.path()).cloned(),
        Some(id) => {
            let has_active = is_list(change.path()).unwrap_or(false);
            items(get(v, change.path()), has_active)
                .get(id)
                .map(|a| Value::Bool(a.unwrap_or(true)))
        }
    }
}

/// Sets each change's [`Origin`] by comparing with `snapshot`, the live
/// state recorded after the last successful apply. Without a snapshot every
/// change is pending.
pub fn classify(
    plan: &mut ProfilePlan,
    desired: &DesiredProfile,
    live: Option<&Profile>,
    snapshot: Option<&Value>,
) {
    let Some(snap) = snapshot else { return };
    let want = serde_json::to_value(&desired.desired).unwrap_or(Value::Null);
    let have = live
        .and_then(|l| serde_json::to_value(l).ok())
        .unwrap_or(Value::Null);
    for c in &mut plan.changes {
        let before = probe(snap, c);
        let code_changed = probe(&want, c) != before;
        let live_changed = probe(&have, c) != before;
        c.set_origin(match (code_changed, live_changed) {
            (true, true) => Origin::Conflict,
            (false, true) => Origin::Drift,
            _ => Origin::Pending,
        });
    }
}

#[cfg(test)]
mod tests;
