//! JSON Schemas of the configuration files.
//!
//! The schemas drive editor completion (via Taplo) and the unknown-key check,
//! so the Rust types are the single source of truth.

use serde_json::Value;

use super::types::{LocalFile, ProfileFile, RootFile, UserFile};

/// Which file a schema describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaKind {
    /// `ndac.toml`.
    Root,
    /// `ndac.local.toml`.
    Local,
    /// Profile and template files.
    Profile,
    /// The per-user configuration file.
    User,
}

/// The JSON Schema for `kind`.
#[must_use]
pub fn json_schema(kind: SchemaKind) -> Value {
    let schema = match kind {
        SchemaKind::Root => schemars::schema_for!(RootFile),
        SchemaKind::Local => schemars::schema_for!(LocalFile),
        SchemaKind::Profile => schemars::schema_for!(ProfileFile),
        SchemaKind::User => schemars::schema_for!(UserFile),
    };
    schema.to_value()
}

/// Pretty-printed schema with a trailing newline, as committed under
/// `schema/`.
#[must_use]
pub fn json_schema_text(kind: SchemaKind) -> String {
    let mut s = serde_json::to_string_pretty(&json_schema(kind)).unwrap_or_default();
    s.push('\n');
    s
}

/// Resolves `$ref` and nullable `anyOf` wrappers down to an object schema.
pub(crate) fn object_schema<'a>(schema: &'a Value, root: &'a Value) -> Option<&'a Value> {
    let mut cur = schema;
    for _ in 0..16 {
        if let Some(r) = cur.get("$ref").and_then(Value::as_str) {
            let name = r.strip_prefix("#/$defs/")?;
            cur = root.get("$defs")?.get(name)?;
            continue;
        }
        if let Some(branches) = cur.get("anyOf").or_else(|| cur.get("oneOf")) {
            cur = branches
                .as_array()?
                .iter()
                .find(|b| b.get("type").and_then(Value::as_str) != Some("null"))?;
            continue;
        }
        let is_object = cur.get("properties").is_some()
            || cur
                .get("additionalProperties")
                .is_some_and(Value::is_object);
        return is_object.then_some(cur);
    }
    None
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::*;

    /// The committed schema files must match the types. Regenerate with
    /// `UPDATE_SCHEMA=1 cargo test -p ndac-core schema` (or `just schema`).
    #[test]
    fn schema_files_are_up_to_date() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema");
        for (kind, file) in [
            (SchemaKind::Root, "ndac.schema.json"),
            (SchemaKind::Profile, "ndac-profile.schema.json"),
        ] {
            let path = dir.join(file);
            let expected = json_schema_text(kind);
            if std::env::var_os("UPDATE_SCHEMA").is_some() {
                fs::create_dir_all(&dir).unwrap();
                fs::write(&path, &expected).unwrap();
            }
            let actual = fs::read_to_string(&path).unwrap_or_default();
            assert!(
                actual == expected,
                "{} is out of date; run `just schema`",
                path.display()
            );
        }
    }

    #[test]
    fn resolves_nested_objects() {
        let root = json_schema(SchemaKind::Profile);
        let settings = &root["properties"]["settings"];
        let obj = object_schema(settings, &root).unwrap();
        assert!(obj["properties"].get("logs").is_some());
    }
}
