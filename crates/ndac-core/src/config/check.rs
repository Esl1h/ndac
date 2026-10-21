//! Per-file checks: unknown keys (driven by the JSON Schema) and semantic
//! validation with source spans.

use std::collections::BTreeMap;

use serde_json::Value;
use toml_edit::{InlineTable, Item, Table};

use super::{
    error::{ConfigError, span},
    schema::object_schema,
    source::{Seg, SourceFile},
    types::{AuthConfig, DurationValue, ItemMap, ItemValue, ProfileFile, ProviderConfig, RootFile},
    values::{duration_secs, normalize_domain, suggest},
};
use crate::paths;

/// `"$inherit"` in an ID array expands to the inherited array.
pub const INHERIT: &str = "$inherit";

#[derive(Clone, Copy)]
enum Node<'a> {
    Table(&'a Table),
    Inline(&'a InlineTable),
}

/// Reports every key of `src` that `schema` does not allow.
pub(crate) fn unknown_keys(src: &SourceFile, schema: &Value) -> Vec<ConfigError> {
    let mut out = Vec::new();
    if let Some(root) = src.doc().as_item().as_table() {
        walk(
            src,
            Node::Table(root),
            schema,
            schema,
            &mut Vec::new(),
            &mut out,
        );
    }
    out
}

fn walk(
    src: &SourceFile,
    node: Node<'_>,
    schema: &Value,
    root: &Value,
    path: &mut Vec<String>,
    out: &mut Vec<ConfigError>,
) {
    let Some(obj) = object_schema(schema, root) else {
        return;
    };
    let props = obj.get("properties").and_then(Value::as_object);
    let additional = obj.get("additionalProperties").filter(|a| a.is_object());
    let children: Vec<(&str, Option<Node<'_>>)> = match node {
        Node::Table(t) => t.iter().map(|(k, v)| (k, as_node_item(v))).collect(),
        Node::Inline(t) => t
            .iter()
            .map(|(k, v)| (k, v.as_inline_table().map(Node::Inline)))
            .collect(),
    };
    for (key, child) in children {
        let child_schema = props.and_then(|p| p.get(key)).or(additional);
        path.push(key.to_owned());
        if let Some(cs) = child_schema {
            if let Some(c) = child {
                walk(src, c, cs, root, path, out);
            }
        } else {
            let segs: Vec<Seg<'_>> = path.iter().map(|k| Seg::Key(k)).collect();
            let known = props.into_iter().flat_map(|p| p.keys().map(String::as_str));
            out.push(ConfigError::UnknownKey {
                key: path.join("."),
                help: suggest(key, known).map(|s| format!("did you mean `{s}`?")),
                src: src.named(),
                span: span(src.key_span(&segs)),
            });
        }
        path.pop();
    }
}

fn as_node_item(item: &Item) -> Option<Node<'_>> {
    match item {
        Item::Table(t) => Some(Node::Table(t)),
        Item::Value(v) => v.as_inline_table().map(Node::Inline),
        _ => None,
    }
}

/// Deserializes `src` into `T`, mapping serde errors to spanned diagnostics.
pub(crate) fn deserialize<T: serde::de::DeserializeOwned>(
    src: &SourceFile,
) -> Result<T, ConfigError> {
    toml::from_str(&src.text).map_err(|e| ConfigError::Syntax {
        message: e.message().trim().to_owned(),
        span: span(e.span()),
        src: src.named(),
    })
}

/// Semantic checks of the root file.
pub(crate) fn root_file(src: &SourceFile, root: &RootFile) -> Vec<ConfigError> {
    let mut out = Vec::new();
    if root.schema != 1 {
        out.push(src.invalid(
            src.value_span(&[Seg::Key("schema")]),
            format!("unsupported schema version {}", root.schema),
            "only 1 is supported",
            None,
        ));
    }
    provider(src, &root.provider, &mut out);
    auth(src, &root.auth, &[Seg::Key("auth")], &mut out);
    out
}

pub(crate) fn provider(src: &SourceFile, p: &ProviderConfig, out: &mut Vec<ConfigError>) {
    if let Some(t) = &p.timeout {
        duration(src, t, &[Seg::Key("provider"), Seg::Key("timeout")], out);
    }
    if p.concurrency == Some(0) {
        out.push(src.invalid(
            src.value_span(&[Seg::Key("provider"), Seg::Key("concurrency")]),
            "concurrency must be at least 1",
            "zero",
            None,
        ));
    }
}

pub(crate) fn auth(src: &SourceFile, a: &AuthConfig, at: &[Seg<'_>], out: &mut Vec<ConfigError>) {
    if let Some(t) = &a.cmd_timeout {
        let mut p = at.to_vec();
        p.push(Seg::Key("cmd_timeout"));
        duration(src, t, &p, out);
    }
}

fn duration(src: &SourceFile, v: &DurationValue, at: &[Seg<'_>], out: &mut Vec<ConfigError>) {
    match duration_secs(v) {
        Ok(0) => out.push(src.invalid(
            src.value_span(at),
            "duration must be positive",
            "zero",
            None,
        )),
        Ok(_) => {}
        Err(why) => out.push(src.invalid(
            src.value_span(at),
            format!("invalid duration: {why}"),
            "here",
            Some("use seconds or units like \"30d\", \"1h30m\"".into()),
        )),
    }
}

/// Semantic checks of a profile or template file.
pub(crate) fn profile_file(
    src: &SourceFile,
    f: &ProfileFile,
    is_template: bool,
) -> Vec<ConfigError> {
    let mut out = Vec::new();
    // A template's markers apply to whatever it is stacked on.
    let inherits = is_template || !f.extends.is_empty();
    if is_template {
        template_only(src, f, &mut out);
    }
    header(src, f, &mut out);
    lists(src, f, inherits, &mut out);
    out
}

/// Fields that templates may not set.
fn template_only(src: &SourceFile, f: &ProfileFile, out: &mut Vec<ConfigError>) {
    let forbidden: [(&str, bool); 5] = [
        ("id", f.id.is_some()),
        ("name", f.name.is_some()),
        ("ignore", !f.ignore.is_empty()),
        ("auth", f.auth.is_some()),
        ("endpoint", f.endpoint.is_some()),
    ];
    for (key, present) in forbidden {
        if present {
            out.push(src.invalid(
                src.key_span(&[Seg::Key(key)]),
                format!("`{key}` is not allowed in a template"),
                "profiles only",
                Some(
                    "`id`, `name`, `ignore`, `auth`, `endpoint` and list `mode` are not inherited"
                        .into(),
                ),
            ));
        }
    }
    for list in ["denylist", "allowlist"] {
        let l = if list == "denylist" {
            &f.denylist
        } else {
            &f.allowlist
        };
        if l.as_ref().is_some_and(|l| l.mode.is_some()) {
            out.push(src.invalid(
                src.key_span(&[Seg::Key(list), Seg::Key("mode")]),
                format!("`{list}.mode` is not allowed in a template"),
                "profiles only",
                None,
            ));
        }
    }
}

/// `id`, `ignore` and `auth`.
fn header(src: &SourceFile, f: &ProfileFile, out: &mut Vec<ConfigError>) {
    if let Some(id) = f.id.as_deref().filter(|s| !s.is_empty())
        && !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        out.push(src.invalid(
            src.value_span(&[Seg::Key("id")]),
            format!("invalid profile id {id:?}"),
            "lowercase letters and digits only",
            Some("copy the ID shown in the NextDNS dashboard URL".into()),
        ));
    }

    for (i, path) in f.ignore.iter().enumerate() {
        if !paths::is_known(path) {
            let all = paths::all_paths_and_groups();
            out.push(
                src.invalid(
                    src.value_span(&[Seg::Key("ignore"), Seg::Index(i)]),
                    format!("unknown path `{path}` in ignore"),
                    "not a profile field",
                    suggest(path, all.iter().map(String::as_str))
                        .map(|s| format!("did you mean `{s}`?")),
                ),
            );
        }
    }

    if let Some(a) = &f.auth {
        auth(src, a, &[Seg::Key("auth")], out);
    }
}

/// ID arrays, item maps and durations.
fn lists(src: &SourceFile, f: &ProfileFile, inherits: bool, out: &mut Vec<ConfigError>) {
    if let Some(s) = &f.security {
        id_array(src, s.tlds.as_deref(), &["security", "tlds"], inherits, out);
    }
    if let Some(p) = &f.privacy {
        id_array(
            src,
            p.blocklists.as_deref(),
            &["privacy", "blocklists"],
            inherits,
            out,
        );
        id_array(
            src,
            p.natives.as_deref(),
            &["privacy", "natives"],
            inherits,
            out,
        );
    }
    if let Some(pc) = &f.parental_control {
        item_map(
            src,
            pc.services.as_ref(),
            &["parentalControl", "services"],
            inherits,
            false,
            out,
        );
        item_map(
            src,
            pc.categories.as_ref(),
            &["parentalControl", "categories"],
            inherits,
            false,
            out,
        );
    }
    if let Some(d) = &f.denylist {
        item_map(
            src,
            d.items.as_ref(),
            &["denylist", "items"],
            inherits,
            true,
            out,
        );
    }
    if let Some(a) = &f.allowlist {
        item_map(
            src,
            a.items.as_ref(),
            &["allowlist", "items"],
            inherits,
            true,
            out,
        );
    }
    if let Some(r) = f
        .settings
        .as_ref()
        .and_then(|s| s.logs.as_ref())
        .and_then(|l| l.retention.as_ref())
    {
        duration(
            src,
            r,
            &[
                Seg::Key("settings"),
                Seg::Key("logs"),
                Seg::Key("retention"),
            ],
            out,
        );
    }
}

fn segs<'a>(path: &[&'a str]) -> Vec<Seg<'a>> {
    path.iter().map(|k| Seg::Key(k)).collect()
}

fn id_array(
    src: &SourceFile,
    ids: Option<&[String]>,
    path: &[&str],
    inherits: bool,
    out: &mut Vec<ConfigError>,
) {
    let Some(ids) = ids else { return };
    let base = segs(path);
    let at = |i: usize| {
        let mut p = base.clone();
        p.push(Seg::Index(i));
        src.value_span(&p)
    };
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, id) in ids.iter().enumerate() {
        if id == INHERIT {
            if !inherits {
                out.push(src.invalid(
                    at(i),
                    "`$inherit` without `extends`",
                    "nothing to inherit",
                    Some("add `extends = [\"<template>\"]` or remove `$inherit`".into()),
                ));
            }
        } else if id.trim().is_empty() || id.contains(char::is_whitespace) {
            out.push(src.invalid(
                at(i),
                format!("invalid id {id:?}"),
                "empty or contains spaces",
                None,
            ));
        }
        if let Some(first) = seen.insert(id, i) {
            out.push(src.invalid(
                at(i),
                format!("duplicate `{id}` in {}", path.join(".")),
                format!("already listed at position {}", first + 1),
                None,
            ));
        }
    }
}

fn item_map(
    src: &SourceFile,
    map: Option<&ItemMap>,
    path: &[&str],
    inherits: bool,
    domains: bool,
    out: &mut Vec<ConfigError>,
) {
    let Some(map) = map else { return };
    let base = segs(path);
    let at = |k: &str| {
        let mut p = base.clone();
        p.push(Seg::Key(k));
        src.key_span(&p)
    };
    let mut normalized: BTreeMap<String, &str> = BTreeMap::new();
    for (key, value) in map {
        if matches!(value, ItemValue::Marker(_)) && !inherits {
            out.push(src.invalid(
                at(key),
                "`$remove` without `extends`",
                "nothing to remove",
                Some("only inherited keys can be removed".into()),
            ));
        }
        let norm = if domains {
            match normalize_domain(key) {
                Ok(d) => d,
                Err(why) => {
                    out.push(src.invalid(at(key), format!("invalid domain {key:?}"), why, None));
                    continue;
                }
            }
        } else if key.trim().is_empty() || key.contains(char::is_whitespace) {
            out.push(src.invalid(
                at(key),
                format!("invalid id {key:?}"),
                "empty or contains spaces",
                None,
            ));
            continue;
        } else {
            key.clone()
        };
        if let Some(first) = normalized.insert(norm.clone(), key) {
            out.push(src.invalid(
                at(key),
                format!("duplicate entry `{norm}` in {}", path.join(".")),
                format!("same as `{first}` after normalization"),
                None,
            ));
        }
    }
}
