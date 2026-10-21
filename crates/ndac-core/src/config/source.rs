//! A loaded configuration file with span lookup for diagnostics.

use std::{
    ops::Range,
    path::{Path, PathBuf},
};

use miette::NamedSource;
use toml_edit::{Document, Item, Value};

use super::error::{ConfigError, span};

/// One step of a path into a TOML document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Seg<'a> {
    Key(&'a str),
    Index(usize),
}

/// A configuration file: its path, text and parsed document.
#[derive(Debug)]
pub struct SourceFile {
    /// Path as given or discovered.
    pub path: PathBuf,
    /// Path shown in diagnostics, relative to the root directory.
    pub display: String,
    /// File contents.
    pub text: String,
    doc: Document<String>,
}

impl SourceFile {
    /// Parses `text`. Syntax errors carry the location reported by the
    /// parser.
    pub(crate) fn parse(path: &Path, display: String, text: String) -> Result<Self, ConfigError> {
        match Document::parse(text.clone()) {
            Ok(doc) => Ok(Self {
                path: path.to_path_buf(),
                display,
                text,
                doc,
            }),
            Err(e) => Err(ConfigError::Syntax {
                message: e.message().to_owned(),
                span: span(e.span()),
                src: NamedSource::new(display, text),
            }),
        }
    }

    /// The parsed document.
    pub(crate) fn doc(&self) -> &Document<String> {
        &self.doc
    }

    /// Named source for diagnostics.
    pub(crate) fn named(&self) -> NamedSource<String> {
        NamedSource::new(self.display.clone(), self.text.clone())
    }

    /// Span of the key at `path` (the last segment must be a key), falling
    /// back to the closest enclosing location.
    pub(crate) fn key_span(&self, path: &[Seg<'_>]) -> Option<Range<usize>> {
        self.locate(path, true)
    }

    /// Span of the value at `path`, falling back to the closest enclosing
    /// location.
    pub(crate) fn value_span(&self, path: &[Seg<'_>]) -> Option<Range<usize>> {
        self.locate(path, false)
    }

    fn locate(&self, path: &[Seg<'_>], want_key: bool) -> Option<Range<usize>> {
        let mut best = None;
        let mut item = self.doc.as_item();
        for (i, seg) in path.iter().enumerate() {
            let last = i + 1 == path.len();
            match *seg {
                Seg::Key(k) => {
                    let (key, next) = match item {
                        Item::Table(t) => t.get_key_value(k)?,
                        Item::Value(Value::InlineTable(t)) => t.get_key_value(k)?,
                        _ => return best,
                    };
                    best = key.span().or(best);
                    if last && want_key {
                        return best;
                    }
                    item = next;
                }
                Seg::Index(n) => {
                    // Paths never descend into array elements.
                    let Item::Value(Value::Array(a)) = item else {
                        return best;
                    };
                    return a.get(n).and_then(Value::span).or(best);
                }
            }
            best = item.span().or(best);
        }
        best
    }

    /// An [`ConfigError::Invalid`] pointing at `range`.
    pub(crate) fn invalid(
        &self,
        range: Option<Range<usize>>,
        message: impl Into<String>,
        label: impl Into<String>,
        help: Option<String>,
    ) -> ConfigError {
        ConfigError::Invalid {
            message: message.into(),
            help,
            src: self.named(),
            span: span(range),
            label: label.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(text: &str) -> SourceFile {
        SourceFile::parse(Path::new("x.toml"), "x.toml".into(), text.into()).unwrap()
    }

    #[test]
    fn finds_keys_and_values() {
        let text =
            "[denylist.items]\n\"a.example\" = true\n[privacy]\nblocklists = [\"x\", \"y\"]\n";
        let s = src(text);
        let k = s
            .key_span(&[
                Seg::Key("denylist"),
                Seg::Key("items"),
                Seg::Key("a.example"),
            ])
            .unwrap();
        assert_eq!(&text[k], "\"a.example\"");
        let v = s
            .value_span(&[Seg::Key("privacy"), Seg::Key("blocklists"), Seg::Index(1)])
            .unwrap();
        assert_eq!(text[v].trim(), "\"y\"");
    }

    #[test]
    fn inline_tables_are_searched() {
        let text = "[parentalControl]\nservices = { tiktok = true }\n";
        let s = src(text);
        let k = s
            .key_span(&[
                Seg::Key("parentalControl"),
                Seg::Key("services"),
                Seg::Key("tiktok"),
            ])
            .unwrap();
        assert_eq!(&text[k], "tiktok");
    }

    #[test]
    fn syntax_errors_have_a_span() {
        let err = SourceFile::parse(Path::new("x"), "x".into(), "a = \n".into()).unwrap_err();
        assert!(matches!(err, ConfigError::Syntax { span: Some(_), .. }));
    }
}
