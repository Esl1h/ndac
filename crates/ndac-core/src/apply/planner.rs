//! Turns a profile's changes into the smallest set of API calls.

use std::collections::BTreeMap;

use ndac_api::{ListPath, ObjectPath, Profile};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::{
    config::{DesiredProfile, ListMode},
    plan::{Change, ProfilePlan, get},
};

/// One API request.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "call", rename_all = "snake_case")]
pub enum ApiCall {
    /// `POST /profiles` with the full desired profile.
    CreateProfile {
        /// Profile body.
        body: Box<Profile>,
    },
    /// `DELETE /profiles/:profile`.
    DeleteProfile,
    /// `PATCH /profiles/:profile` (top-level fields such as `name`).
    PatchProfile {
        /// Partial body.
        body: Value,
    },
    /// `PATCH` on an object endpoint.
    PatchObject {
        /// Endpoint.
        #[serde(serialize_with = "ser_object")]
        path: ObjectPath,
        /// Partial body.
        body: Value,
    },
    /// `PUT` the whole array.
    PutList {
        /// Endpoint.
        #[serde(serialize_with = "ser_list")]
        list: ListPath,
        /// Desired items.
        items: Vec<Value>,
    },
    /// `POST` one item.
    AddItem {
        /// Endpoint.
        #[serde(serialize_with = "ser_list")]
        list: ListPath,
        /// Item ID.
        id: String,
        /// Item body.
        body: Value,
    },
    /// `PATCH` one item.
    PatchItem {
        /// Endpoint.
        #[serde(serialize_with = "ser_list")]
        list: ListPath,
        /// Item ID.
        id: String,
        /// Partial body.
        body: Value,
    },
    /// `DELETE` one item.
    DeleteItem {
        /// Endpoint.
        #[serde(serialize_with = "ser_list")]
        list: ListPath,
        /// Item ID.
        id: String,
    },
    /// `POST` one rewrite. UNVERIFIED endpoint.
    #[cfg(feature = "unstable-endpoints")]
    AddRewrite {
        /// Domain.
        name: String,
        /// Answer.
        content: String,
    },
    /// `DELETE` one rewrite, found by name and answer. UNVERIFIED endpoint.
    #[cfg(feature = "unstable-endpoints")]
    DeleteRewrite {
        /// Domain.
        name: String,
        /// Answer.
        content: String,
    },
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde serialize_with signature"
)]
fn ser_object<S: serde::Serializer>(p: &ObjectPath, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(p.template())
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde serialize_with signature"
)]
fn ser_list<S: serde::Serializer>(p: &ListPath, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(p.template())
}

impl ApiCall {
    /// `METHOD /endpoint/template` for logs and tests.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::CreateProfile { .. } => "POST /profiles".into(),
            Self::DeleteProfile => "DELETE /profiles/:profile".into(),
            Self::PatchProfile { .. } => "PATCH /profiles/:profile".into(),
            Self::PatchObject { path, .. } => format!("PATCH {}", path.template()),
            Self::PutList { list, .. } => format!("PUT {}", list.template()),
            Self::AddItem { list, .. } => format!("POST {}", list.template()),
            Self::PatchItem { list, .. } => format!("PATCH {}", list.item_template()),
            Self::DeleteItem { list, .. } => format!("DELETE {}", list.item_template()),
            #[cfg(feature = "unstable-endpoints")]
            Self::AddRewrite { .. } => "POST /profiles/:profile/rewrites".into(),
            #[cfg(feature = "unstable-endpoints")]
            Self::DeleteRewrite { .. } => "DELETE /profiles/:profile/rewrites/:id".into(),
        }
    }
}

/// Section order of execution within a profile.
const ORDER: &[&str] = &[
    "name",
    "security",
    "privacy",
    "parentalControl",
    "denylist",
    "allowlist",
    "rewrites",
    "settings",
];

fn section_rank(path: &str) -> usize {
    let first = path.split('.').next().unwrap_or(path);
    ORDER
        .iter()
        .position(|s| *s == first)
        .unwrap_or(ORDER.len())
}

/// The list endpoint for a list path.
#[must_use]
pub fn list_path(path: &str) -> Option<ListPath> {
    Some(match path {
        "security.tlds" => ListPath::SecurityTlds,
        "privacy.blocklists" => ListPath::PrivacyBlocklists,
        "privacy.natives" => ListPath::PrivacyNatives,
        "parentalControl.services" => ListPath::ParentalControlServices,
        "parentalControl.categories" => ListPath::ParentalControlCategories,
        "denylist" => ListPath::Denylist,
        "allowlist" => ListPath::Allowlist,
        _ => return None,
    })
}

/// The object endpoint holding a field, and the field's key in it.
fn object_of(path: &str) -> Option<(Option<ObjectPath>, &str)> {
    let (parent, key) = path.rsplit_once('.').unwrap_or(("", path));
    let obj = match parent {
        "" => None,
        "security" => Some(ObjectPath::Security),
        "privacy" => Some(ObjectPath::Privacy),
        "parentalControl" => Some(ObjectPath::ParentalControl),
        "settings" => Some(ObjectPath::Settings),
        "settings.logs" => Some(ObjectPath::SettingsLogs),
        "settings.logs.drop" => Some(ObjectPath::SettingsLogsDrop),
        "settings.blockPage" => Some(ObjectPath::SettingsBlockPage),
        "settings.performance" => Some(ObjectPath::SettingsPerformance),
        _ => return None,
    };
    Some((obj, key))
}

/// Items of the desired list at `path`, as API bodies.
fn desired_items(desired: &Value, path: &str) -> Vec<Value> {
    get(desired, path)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// A list that would end up empty after removing live items; requires
/// `--allow-destroy`.
#[must_use]
pub fn empties_a_list(plan: &ProfilePlan, desired: &DesiredProfile) -> Option<String> {
    let want = serde_json::to_value(&desired.desired).unwrap_or(Value::Null);
    plan.changes.iter().find_map(|c| match c {
        Change::RemoveItem { list, .. } if desired_items(&want, list).is_empty() => {
            Some(list.clone())
        }
        _ => None,
    })
}

/// Plans the calls for one profile. A new profile is a single
/// `POST /profiles`.
#[must_use]
pub fn calls(plan: &ProfilePlan, desired: &DesiredProfile) -> Vec<ApiCall> {
    if plan.create {
        #[cfg_attr(not(feature = "unstable-endpoints"), allow(unused_mut))]
        let mut body = desired.desired.clone();
        // Rewrites have their own endpoint; whether `POST /profiles`
        // accepts them is unknown.
        #[cfg(feature = "unstable-endpoints")]
        let rewrites = body.rewrites.take().unwrap_or_default();
        #[cfg_attr(not(feature = "unstable-endpoints"), allow(unused_mut))]
        let mut out = vec![ApiCall::CreateProfile {
            body: Box::new(body),
        }];
        #[cfg(feature = "unstable-endpoints")]
        out.extend(rewrites.into_iter().map(|r| ApiCall::AddRewrite {
            name: r.name,
            content: r.content,
        }));
        return out;
    }
    let want = serde_json::to_value(&desired.desired).unwrap_or(Value::Null);

    let mut changes: Vec<&Change> = plan.changes.iter().collect();
    changes.sort_by_key(|c| section_rank(c.path()));

    let mut out = Vec::new();
    let mut profile_body = Map::new();
    let mut objects: BTreeMap<ObjectPath, Map<String, Value>> = BTreeMap::new();
    let mut lists: BTreeMap<String, Vec<&Change>> = BTreeMap::new();
    let mut order: Vec<(usize, Key)> = Vec::new();

    for c in changes {
        let rank = section_rank(c.path());
        if let Change::SetField { path, to, .. } = c {
            match object_of(path) {
                Some((None, key)) => {
                    if profile_body.is_empty() {
                        order.push((rank, Key::Profile));
                    }
                    profile_body.insert(key.to_owned(), to.clone());
                }
                Some((Some(obj), key)) => {
                    let entry = objects.entry(obj).or_default();
                    if entry.is_empty() {
                        order.push((rank, Key::Object(obj)));
                    }
                    entry.insert(key.to_owned(), to.clone());
                }
                None => {}
            }
        } else {
            let entry = lists.entry(c.path().to_owned()).or_default();
            if entry.is_empty() {
                order.push((rank, Key::List(c.path().to_owned())));
            }
            entry.push(c);
        }
    }

    for (_, key) in order {
        match key {
            Key::Profile => out.push(ApiCall::PatchProfile {
                body: Value::Object(std::mem::take(&mut profile_body)),
            }),
            Key::Object(obj) => {
                if let Some(body) = objects.remove(&obj) {
                    out.push(ApiCall::PatchObject {
                        path: obj,
                        body: Value::Object(body),
                    });
                }
            }
            #[cfg(feature = "unstable-endpoints")]
            Key::List(path) if path == "rewrites" => {
                out.extend(rewrite_calls(&lists.remove(&path).unwrap_or_default()));
            }
            Key::List(path) => {
                let Some(list) = list_path(&path) else {
                    continue;
                };
                let changes = lists.remove(&path).unwrap_or_default();
                let authoritative = desired.list_modes.get(&path) != Some(&ListMode::Merge);
                if authoritative && changes.len() > 1 {
                    out.push(ApiCall::PutList {
                        list,
                        items: desired_items(&want, &path),
                    });
                } else {
                    out.extend(changes.into_iter().filter_map(|c| item_call(list, c)));
                }
            }
        }
    }
    out
}

/// Removals first: the API keeps one rewrite per name, so a changed
/// answer must be deleted before the new one is added.
#[cfg(feature = "unstable-endpoints")]
fn rewrite_calls(changes: &[&Change]) -> Vec<ApiCall> {
    use crate::plan::rewrite_parts;
    let parts = |id: &str| rewrite_parts(id).map(|(n, c)| (n.to_owned(), c.to_owned()));
    let deletes = changes.iter().filter_map(|c| match c {
        Change::RemoveItem { id, .. } => {
            parts(id).map(|(name, content)| ApiCall::DeleteRewrite { name, content })
        }
        _ => None,
    });
    let adds = changes.iter().filter_map(|c| match c {
        Change::AddItem { id, .. } => {
            parts(id).map(|(name, content)| ApiCall::AddRewrite { name, content })
        }
        _ => None,
    });
    deletes.chain(adds).collect()
}

enum Key {
    Profile,
    Object(ObjectPath),
    List(String),
}

fn item_call(list: ListPath, c: &Change) -> Option<ApiCall> {
    Some(match c {
        Change::AddItem { id, active, .. } => {
            let mut body = Map::new();
            body.insert("id".into(), Value::String(id.clone()));
            if let Some(a) = active {
                body.insert("active".into(), Value::Bool(*a));
            }
            ApiCall::AddItem {
                list,
                id: id.clone(),
                body: Value::Object(body),
            }
        }
        Change::RemoveItem { id, .. } => ApiCall::DeleteItem {
            list,
            id: id.clone(),
        },
        Change::SetItemActive { id, to, .. } => ApiCall::PatchItem {
            list,
            id: id.clone(),
            body: serde_json::json!({ "active": to }),
        },
        Change::SetField { .. } => return None,
    })
}
