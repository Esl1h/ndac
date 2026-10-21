//! Command-line definition.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// NextDNS-as-Code: manage NextDNS profiles declaratively from TOML files.
///
/// There is deliberately no `--api-key` flag: command-line arguments are
/// visible to other users through the process list and end up in shell
/// history. See `ndac auth sources` for the supported key sources.
#[derive(Debug, Parser)]
#[command(name = "ndac", version, propagate_version = true, max_term_width = 100)]
pub(crate) struct Cli {
    #[command(flatten)]
    pub(crate) global: GlobalArgs,

    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Args)]
pub(crate) struct GlobalArgs {
    /// Path to ndac.toml. Default: the nearest one in this or a parent
    /// directory.
    #[arg(long, global = true, env = "NDAC_CONFIG", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,

    /// More logging on stderr (-v info, -vv debug, -vvv trace).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub(crate) verbose: u8,

    /// Only print errors and the requested output.
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub(crate) quiet: bool,

    /// When to use colors. `NO_COLOR`, `CLICOLOR_FORCE` and `TERM=dumb` are
    /// honored in auto mode.
    #[arg(long, global = true, value_enum, default_value_t = ColorArg::Auto)]
    pub(crate) color: ColorArg,

    /// Log line format.
    #[arg(long, global = true, value_enum, default_value_t = LogFormat::Text)]
    pub(crate) log_format: LogFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ColorArg {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum LogFormat {
    Text,
    Json,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Check every configuration file: syntax, unknown keys, values and
    /// template references.
    Validate,

    /// Print the fully resolved desired state of a profile.
    Render {
        /// Profile key (file stem under profiles/).
        profile: String,
        /// Output format.
        #[arg(long, value_enum, default_value_t = RenderFormat::Toml)]
        format: RenderFormat,
    },

    /// Format configuration files canonically. Comments are preserved.
    Fmt {
        /// Report files that need formatting instead of rewriting them.
        /// Exits with 1 if any.
        #[arg(long)]
        check: bool,
    },

    /// Print the JSON Schema of a configuration file kind.
    Schema {
        /// Which file the schema describes.
        #[arg(long, value_enum, default_value_t = SchemaArg::Profile)]
        kind: SchemaArg,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum RenderFormat {
    Toml,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum SchemaArg {
    /// ndac.toml
    Root,
    /// ndac.local.toml
    Local,
    /// Profile and template files.
    Profile,
}
