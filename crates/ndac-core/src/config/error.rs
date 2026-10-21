//! Configuration diagnostics.

use std::{io, ops::Range, path::PathBuf};

use miette::{Diagnostic, NamedSource, SourceSpan};

/// A configuration problem, with source location when known.
#[derive(Debug, thiserror::Error, Diagnostic)]
#[non_exhaustive]
pub enum ConfigError {
    /// No root file was found.
    #[error("no ndac.toml found in {} or any parent directory", start.display())]
    #[diagnostic(
        code(ndac::config::not_found),
        help("run `ndac init`, or pass --config / set NDAC_CONFIG")
    )]
    NotFound {
        /// Where the search started.
        start: PathBuf,
    },

    /// A file could not be read.
    #[error("cannot read {}", path.display())]
    #[diagnostic(code(ndac::config::io))]
    Io {
        /// The file.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: io::Error,
    },

    /// TOML syntax or type error.
    #[error("{message}")]
    #[diagnostic(code(ndac::config::syntax))]
    Syntax {
        /// What is wrong.
        message: String,
        /// The file.
        #[source_code]
        src: NamedSource<String>,
        /// Where.
        #[label("here")]
        span: Option<SourceSpan>,
    },

    /// A key that is not part of the schema.
    #[error("unknown key `{key}`")]
    #[diagnostic(code(ndac::config::unknown_key))]
    UnknownKey {
        /// The key.
        key: String,
        /// Suggestion, if a known key is close.
        #[help]
        help: Option<String>,
        /// The file.
        #[source_code]
        src: NamedSource<String>,
        /// Where.
        #[label("not valid here")]
        span: Option<SourceSpan>,
    },

    /// A value that parses but is not acceptable.
    #[error("{message}")]
    #[diagnostic(code(ndac::config::invalid))]
    Invalid {
        /// What is wrong.
        message: String,
        /// How to fix it.
        #[help]
        help: Option<String>,
        /// The file.
        #[source_code]
        src: NamedSource<String>,
        /// Where.
        #[label("{label}")]
        span: Option<SourceSpan>,
        /// Label text.
        label: String,
    },

    /// An `include` glob that is unsafe or malformed.
    #[error("include pattern `{pattern}`: {reason}")]
    #[diagnostic(code(ndac::config::include))]
    Include {
        /// The pattern.
        pattern: String,
        /// Why it was rejected.
        reason: String,
    },

    /// A file found by `include` resolves outside the root directory.
    #[error("{} is outside the configuration root", path.display())]
    #[diagnostic(
        code(ndac::config::escape),
        help("included files and symlink targets must stay inside the directory of ndac.toml")
    )]
    Escape {
        /// The offending file.
        path: PathBuf,
    },
}

/// One or more configuration problems.
#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("configuration has {} error(s)", errors.len())]
#[diagnostic(code(ndac::config))]
pub struct ConfigErrors {
    /// The problems, in file order.
    #[related]
    pub errors: Vec<ConfigError>,
}

impl From<ConfigError> for ConfigErrors {
    fn from(e: ConfigError) -> Self {
        Self { errors: vec![e] }
    }
}

pub(crate) fn span(range: Option<Range<usize>>) -> Option<SourceSpan> {
    range.map(SourceSpan::from)
}
