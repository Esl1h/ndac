//! Canonical formatting of configuration files.
//!
//! Formatting only reorders and re-spaces; comments stay attached to their
//! entries and values are never changed:
//!
//! - ID arrays (`tlds`, `blocklists`, `natives`, `include`) are sorted, with
//!   `"$inherit"` first;
//! - item maps (`items`, `services`, `categories`) are sorted by key;
//! - `=` is aligned among the plain key/value pairs of each table;
//! - the file ends with exactly one newline.

use toml_edit::{Array, DocumentMut, Item, Table, TomlError, Value};

use super::check::INHERIT;

const ID_ARRAYS: &[&[&str]] = &[
    &["include"],
    &["security", "tlds"],
    &["privacy", "blocklists"],
    &["privacy", "natives"],
];

const ITEM_MAPS: &[&[&str]] = &[
    &["parentalControl", "services"],
    &["parentalControl", "categories"],
    &["denylist", "items"],
    &["allowlist", "items"],
];

/// Formats a configuration file.
///
/// # Errors
///
/// If `text` is not valid TOML.
pub fn format_toml(text: &str) -> Result<String, TomlError> {
    let mut doc: DocumentMut = text.parse()?;
    for path in ID_ARRAYS {
        if let Some(Item::Value(Value::Array(a))) = get_mut(doc.as_item_mut(), path) {
            sort_array(a);
        }
    }
    for path in ITEM_MAPS {
        match get_mut(doc.as_item_mut(), path) {
            Some(Item::Table(t)) => t.sort_values(),
            Some(Item::Value(Value::InlineTable(t))) => {
                t.sort_values();
                // TOML 1.1 allows comments in multi-line inline tables;
                // only single-line ones are re-spaced.
                if !t.to_string().contains('\n') {
                    t.fmt();
                }
            }
            _ => {}
        }
    }
    align(doc.as_table_mut());
    let mut out = doc.to_string();
    let trimmed = out.trim_end_matches(['\n', '\r', ' ', '\t']).len();
    out.truncate(trimmed);
    out.push('\n');
    Ok(out)
}

fn get_mut<'a>(mut item: &'a mut Item, path: &[&str]) -> Option<&'a mut Item> {
    for key in path {
        item = match item {
            Item::Table(t) => t.get_mut(key)?,
            _ => return None,
        };
    }
    Some(item)
}

fn sort_array(a: &mut Array) {
    let rank = |v: &Value| {
        let s = v.as_str().unwrap_or_default();
        (s != INHERIT, s.to_owned())
    };
    let sorted = a.iter().map(rank).is_sorted();
    if sorted {
        return;
    }
    let multiline = a.iter().any(|v| {
        v.decor()
            .prefix()
            .and_then(|p| p.as_str())
            .is_some_and(|p| p.contains('\n'))
    });
    a.sort_by(|x, y| rank(x).cmp(&rank(y)));
    if !multiline {
        // Single-line arrays carry no comments, so resetting spacing is safe.
        a.fmt();
    }
}

fn align(table: &mut Table) {
    let width = table
        .iter()
        .filter(|(_, v)| v.is_value())
        .filter_map(|(k, _)| table.key(k).map(|key| key.display_repr().chars().count()))
        .max()
        .unwrap_or(0);
    for (mut key, item) in table.iter_mut() {
        match item {
            Item::Value(v) => {
                let pad = width.saturating_sub(key.display_repr().chars().count());
                key.leaf_decor_mut().set_suffix(" ".repeat(pad + 1));
                v.decor_mut().set_prefix(" ");
            }
            Item::Table(t) => align(t),
            Item::ArrayOfTables(a) => a.iter_mut().for_each(align),
            Item::None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_aligns_and_keeps_comments() {
        let input = "\
# profile for home
id = \"abc123\"
name = \"Casa\" # shown in the dashboard
extends = [\"base\", \"paranoid\"]

[privacy]
blocklists = [\"oisd\", \"$inherit\", \"hagezi-pro\"]

[denylist.items]
\"z.example\" = true
# keep this one
\"a.example\" = false
";
        let out = format_toml(input).unwrap();
        let expected = "\
# profile for home
id      = \"abc123\"
name    = \"Casa\" # shown in the dashboard
extends = [\"base\", \"paranoid\"]

[privacy]
blocklists = [\"$inherit\", \"hagezi-pro\", \"oisd\"]

[denylist.items]
# keep this one
\"a.example\" = false
\"z.example\" = true
";
        assert_eq!(out, expected);
        assert_eq!(format_toml(&out).unwrap(), out);
    }

    #[test]
    fn extends_order_is_preserved() {
        let input = "extends = [\"z\", \"a\"]\n";
        assert_eq!(format_toml(input).unwrap(), input);
    }

    #[test]
    fn inline_maps_are_sorted() {
        let input = "[parentalControl]\ncategories = { porn = true, gambling = true }\n";
        let out = format_toml(input).unwrap();
        assert_eq!(
            out,
            "[parentalControl]\ncategories = { gambling = true, porn = true }\n"
        );
    }

    #[test]
    fn ensures_single_trailing_newline() {
        assert_eq!(format_toml("a = 1").unwrap(), "a = 1\n");
        assert_eq!(format_toml("a = 1\n\n\n").unwrap(), "a = 1\n");
    }
}
