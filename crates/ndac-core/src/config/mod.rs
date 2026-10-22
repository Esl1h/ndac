//! Configuration: TOML files, `extends`, normalization and validation.

// Diagnostics carry the source text for miette; they are only built on the
// error path, so their size does not matter.
#![allow(clippy::result_large_err)]

mod check;
mod desired;
mod error;
mod fmt;
mod load;
mod merge;
mod schema;
mod source;
pub mod types;
mod values;

pub use check::INHERIT;
pub(crate) use desired::normalize_tld;
pub use desired::{DesiredProfile, RENDER_FORMAT_VERSION};
pub use error::{ConfigError, ConfigErrors};
pub use fmt::format_toml;
pub use load::{
    Entry, LOCAL_FILE, ROOT_FILE, TEMPLATES_DIR, Workspace, color_preference, config_files,
    discover, fold_layers, include_files, user_config_path,
};
pub use merge::{ids as merge_ids, items as merge_items, merge};
pub use schema::{SchemaKind, json_schema, json_schema_text};
pub use source::SourceFile;
pub use types::ListMode;
pub use values::{duration_secs, normalize_domain, suggest};
