//! Reconcile write-back: importing live values into configuration files.
//!
//! Each import edits a single key, array element or item with `toml_edit`,
//! so comments, ordering and whitespace survive. Edits are kept in memory
//! until [`WriteBack::write`].

use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
};

use ndac_api::Profile;
use serde_json::Value;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, TableLike, Value as TomlValue};

use crate::{
    apply::{duration, replace_preserving},
    config::{DesiredProfile, Entry, INHERIT, fold_layers, merge_ids, normalize_domain},
    plan::{Change, Origin, get, is_list, items},
};

/// The `$remove` marker of item maps.
const REMOVE: &str = "$remove";

/// Why a live value cannot be imported.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{path}: {reason}")]
pub struct ImportError {
    /// Field or list path.
    pub path: String,
    /// What went wrong.
    pub reason: String,
}

fn fail(path: &str, reason: impl Into<String>) -> ImportError {
    ImportError {
        path: path.to_owned(),
        reason: reason.into(),
    }
}

/// Pending edits to configuration files.
#[derive(Debug, Default)]
pub struct WriteBack {
    /// Text of the trailing comment, without `#`, e.g. `imported 2026-10-21`;
    /// `None` writes no comments.
    note: Option<String>,
    docs: BTreeMap<PathBuf, (String, DocumentMut)>,
}

impl WriteBack {
    /// `note` is the trailing comment added to edited lines, e.g.
    /// `imported 2026-10-21`; the change's origin is appended.
    #[must_use]
    pub fn new(note: Option<String>) -> Self {
        Self {
            note,
            docs: BTreeMap::new(),
        }
    }

    /// Writes the live value of what `change` touches into `layers[target]`.
    /// `layers` are the files the profile is built from, in order (see
    /// [`crate::config::Workspace::layer_entries`]); the target is usually
    /// the last one, the profile itself.
    ///
    /// # Errors
    ///
    /// The value cannot be expressed in that file.
    pub fn import(
        &mut self,
        layers: &[&Entry],
        target: usize,
        change: &Change,
        live: &Profile,
    ) -> Result<(), ImportError> {
        let path = change.path();
        if path == "rewrites" {
            return Err(fail(
                path,
                "rewrites cannot be written back; edit [rewrites.items] by hand",
            ));
        }
        let entry = layers
            .get(target)
            .ok_or_else(|| fail(path, "no such file"))?;
        let base = resolved_json(layers.get(..target).unwrap_or_default());
        let have = serde_json::to_value(live).unwrap_or(Value::Null);
        let comment = self.note.as_ref().map(|n| {
            let origin = match change.origin() {
                Origin::Conflict => "conflict",
                _ => "drift",
            };
            format!(" # {n} ({origin})")
        });
        let doc = self.doc(entry).map_err(|e| fail(path, e))?;
        let edit = Edit {
            doc,
            comment: comment.as_deref(),
        };
        match change.item() {
            None => edit.field(path, get(&have, path), get(&base, path)),
            Some(id) => {
                let has_active = is_list(path).ok_or_else(|| fail(path, "not a list"))?;
                let live_item = items(get(&have, path), has_active).get(id).copied();
                let base_items = items(get(&base, path), has_active);
                if has_active {
                    edit.map_item(path, id, live_item.map(|a| a.unwrap_or(true)), &base_items)
                } else {
                    edit.array_item(path, id, live_item.is_some(), &base_items)
                }
            }
        }
    }

    fn doc(&mut self, entry: &Entry) -> Result<&mut DocumentMut, String> {
        let key = entry.src.path.clone();
        if !self.docs.contains_key(&key) {
            let doc: DocumentMut = entry
                .src
                .text
                .parse()
                .map_err(|e| format!("cannot parse {}: {e}", entry.src.display))?;
            self.docs.insert(key.clone(), (entry.src.text.clone(), doc));
        }
        self.docs
            .get_mut(&key)
            .map(|(_, d)| d)
            .ok_or_else(|| "internal: document missing".to_owned())
    }

    /// Changed files and their new text.
    #[must_use]
    pub fn changed(&self) -> Vec<(&Path, String)> {
        self.docs
            .iter()
            .map(|(p, (orig, doc))| (p.as_path(), orig, doc.to_string()))
            .filter(|(_, orig, new)| *orig != new)
            .map(|(p, _, new)| (p, new))
            .collect()
    }

    /// Original text of every file in [`Self::changed`], to undo a write.
    #[must_use]
    pub fn originals(&self) -> Vec<(PathBuf, String)> {
        self.changed()
            .into_iter()
            .filter_map(|(p, _)| Some((p.to_path_buf(), self.docs.get(p)?.0.clone())))
            .collect()
    }

    /// Writes every changed file atomically, keeping permissions.
    ///
    /// # Errors
    ///
    /// The first I/O error; files written before it stay written.
    pub fn write(&self) -> io::Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        for (path, text) in self.changed() {
            replace_preserving(path, &text)?;
            out.push(path.to_path_buf());
        }
        Ok(out)
    }
}

/// The template among `layers` that sets what `change` touches, when the
/// profile itself (the last layer) does not: the place a user may prefer to
/// import into. `None` for values only the profile can hold (`name`, new
/// items).
#[must_use]
pub fn template_origin(layers: &[&Entry], change: &Change) -> Option<usize> {
    let (profile, templates) = layers.split_last()?;
    if change.path() == "name" || sets(profile, change) {
        return None;
    }
    (0..templates.len())
        .rev()
        .find(|&i| templates.get(i).is_some_and(|e| sets(e, change)))
}

/// True if the file alone sets the field, or contains the item.
fn sets(entry: &Entry, change: &Change) -> bool {
    let own = resolved_json(&[entry]);
    let path = change.path();
    match change.item() {
        None => get(&own, path).is_some(),
        Some(id) => {
            let has_active = is_list(path).unwrap_or(false);
            items(get(&own, path), has_active).contains_key(id)
        }
    }
}

fn resolved_json(layers: &[&Entry]) -> Value {
    let merged = fold_layers(layers);
    serde_json::to_value(DesiredProfile::from_resolved("", merged).desired).unwrap_or(Value::Null)
}

/// Canonical form of a list key as written in a file, for comparison with
/// API IDs.
fn norm(path: &str, key: &str) -> String {
    match path {
        "denylist" | "allowlist" => normalize_domain(key).unwrap_or_else(|_| key.to_owned()),
        "security.tlds" => key.trim().trim_start_matches('.').to_lowercase(),
        _ => key.to_owned(),
    }
}

/// TOML keys of a list: domain lists keep items under `items`.
fn list_keys(path: &str) -> Vec<&str> {
    let mut keys: Vec<&str> = path.split('.').collect();
    if matches!(path, "denylist" | "allowlist") {
        keys.push("items");
    }
    keys
}

/// A JSON field value as written in configuration files.
fn to_toml(path: &str, v: &Value) -> Result<TomlValue, ImportError> {
    match v {
        Value::Bool(b) => Ok((*b).into()),
        Value::String(s) => Ok(s.as_str().into()),
        Value::Number(n) => {
            if path == "settings.logs.retention"
                && let Some(d) = n.as_u64().and_then(duration)
            {
                return Ok(d.into());
            }
            n.as_i64()
                .map(TomlValue::from)
                .ok_or_else(|| fail(path, format!("unsupported number {n}")))
        }
        other => Err(fail(path, format!("unsupported value {other}"))),
    }
}

struct Edit<'a> {
    doc: &'a mut DocumentMut,
    comment: Option<&'a str>,
}

/// A table or inline table reached by key path.
struct Slot<'d> {
    table: &'d mut dyn TableLike,
    inline: bool,
}

impl Slot<'_> {
    fn find(&self, path: &str, id: &str) -> Option<String> {
        self.table
            .iter()
            .map(|(k, _)| k)
            .find(|k| norm(path, k) == id)
            .map(str::to_owned)
    }

    /// Sets `key`, keeping the old value's decor; adds `comment` unless the
    /// line already has one (comments cannot go inside inline tables).
    fn set(&mut self, key: &str, v: TomlValue, comment: Option<&str>) {
        match self.table.get_mut(key) {
            Some(Item::Value(old)) => {
                let decor = old.decor().clone();
                *old = v;
                *old.decor_mut() = decor;
            }
            _ => {
                self.table.insert(key, Item::Value(v));
            }
        }
        if let (false, Some(c), Some(Item::Value(val))) =
            (self.inline, comment, self.table.get_mut(key))
        {
            let has_comment = val
                .decor()
                .suffix()
                .and_then(|s| s.as_str())
                .is_some_and(|s| s.contains('#'));
            if !has_comment {
                val.decor_mut().set_suffix(c);
            }
        }
    }
}

impl Edit<'_> {
    /// The table at `keys`, created (as implicit tables) when missing.
    fn slot(&mut self, path: &str, keys: &[&str]) -> Result<Slot<'_>, ImportError> {
        let mut table: &mut dyn TableLike = self.doc.as_table_mut();
        let mut inline = false;
        for k in keys {
            let item = table.entry(k).or_insert_with(|| {
                if inline {
                    Item::Value(TomlValue::InlineTable(InlineTable::new()))
                } else {
                    let mut t = Table::new();
                    t.set_implicit(true);
                    Item::Table(t)
                }
            });
            inline = item.is_inline_table();
            table = item
                .as_table_like_mut()
                .ok_or_else(|| fail(path, format!("`{k}` is not a table")))?;
        }
        Ok(Slot { table, inline })
    }

    fn field(
        mut self,
        path: &str,
        live: Option<&Value>,
        inherited: Option<&Value>,
    ) -> Result<(), ImportError> {
        let keys: Vec<&str> = path.split('.').collect();
        let (last, parents) = keys.split_last().ok_or_else(|| fail(path, "empty path"))?;
        let comment = self.comment;
        match live {
            Some(v) => {
                let v = to_toml(path, v)?;
                self.slot(path, parents)?.set(last, v, comment);
            }
            None if inherited.is_some() => {
                return Err(fail(
                    path,
                    "not set in NextDNS, but a template sets it; remove it there",
                ));
            }
            None => {
                self.slot(path, parents)?.table.remove(last);
            }
        }
        Ok(())
    }

    /// An entry of an item map (`denylist`, `parentalControl.services`, …).
    fn map_item(
        mut self,
        path: &str,
        id: &str,
        live: Option<bool>,
        inherited: &BTreeMap<String, Option<bool>>,
    ) -> Result<(), ImportError> {
        let comment = self.comment;
        let mut slot = self.slot(path, &list_keys(path))?;
        let existing = slot.find(path, id);
        let key = existing.clone().unwrap_or_else(|| id.to_owned());
        match live {
            Some(active) => slot.set(&key, active.into(), comment),
            None if inherited.contains_key(id) => slot.set(&key, REMOVE.into(), comment),
            None => {
                if let Some(k) = existing {
                    slot.table.remove(&k);
                }
            }
        }
        Ok(())
    }

    /// An element of an ID array (`privacy.blocklists`, `security.tlds`, …).
    fn array_item(
        mut self,
        path: &str,
        id: &str,
        live: bool,
        inherited: &BTreeMap<String, Option<bool>>,
    ) -> Result<(), ImportError> {
        let keys = list_keys(path);
        let (last, parents) = keys.split_last().ok_or_else(|| fail(path, "empty path"))?;
        let comment = self.comment;
        let mut slot = self.slot(path, parents)?;
        let base: Vec<String> = inherited.keys().cloned().collect();
        let current: Option<Vec<String>> = match slot.table.get(last) {
            None => None,
            Some(item) => Some(
                item.as_array()
                    .ok_or_else(|| fail(path, "is not an array"))?
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect(),
            ),
        };

        let next: Array = if live {
            if let Some(Item::Value(TomlValue::Array(a))) = slot.table.get_mut(last) {
                if !a
                    .iter()
                    .any(|v| v.as_str().is_some_and(|s| norm(path, s) == id))
                {
                    a.push(id);
                }
                a.clone()
            } else if base.is_empty() {
                Array::from_iter([id])
            } else {
                Array::from_iter([INHERIT, id])
            }
        } else {
            match slot.table.get_mut(last) {
                Some(Item::Value(TomlValue::Array(a))) => {
                    a.retain(|v| v.as_str().is_none_or(|s| norm(path, s) != id));
                    let own: Vec<String> = a
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect();
                    let still = merge_ids(Some(base.clone()), Some(own.clone()))
                        .unwrap_or_default()
                        .iter()
                        .any(|s| norm(path, s) == id);
                    if still {
                        // Inherited through `$inherit`: list the rest
                        // explicitly instead.
                        explicit(path, id, &base, &own)
                    } else {
                        a.clone()
                    }
                }
                _ => explicit(path, id, &base, &current.unwrap_or_default()),
            }
        };
        slot.set(last, TomlValue::Array(next), comment);
        Ok(())
    }
}

/// The resolved list without `id` and without `$inherit`.
fn explicit(path: &str, id: &str, base: &[String], own: &[String]) -> Array {
    let own = if own.is_empty() {
        vec![INHERIT.to_owned()]
    } else {
        own.to_vec()
    };
    merge_ids(Some(base.to_vec()), Some(own))
        .unwrap_or_default()
        .into_iter()
        .filter(|s| norm(path, s) != id)
        .collect()
}

#[cfg(test)]
mod tests;
