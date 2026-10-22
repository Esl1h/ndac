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
    #[arg(
        long,
        global = true,
        env = "NDAC_CONFIG",
        hide_env_values = true,
        value_name = "PATH"
    )]
    pub(crate) config: Option<PathBuf>,

    /// API key source reference(s), e.g. `keyring:nextdns/default`, tried in
    /// order. Overrides `[auth].sources`. Never a key value.
    #[arg(
        long,
        global = true,
        env = "NDAC_AUTH_SOURCE",
        hide_env_values = true,
        value_name = "REF",
        value_delimiter = ','
    )]
    pub(crate) auth_source: Vec<String>,

    /// Use a literal key from a file that git tracks, for this run only.
    #[arg(long, global = true)]
    pub(crate) allow_tracked_secret: bool,

    /// More logging on stderr (-v info, -vv debug, -vvv trace).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub(crate) verbose: u8,

    /// Only print errors and the requested output.
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub(crate) quiet: bool,

    /// When to use colors [default: `[output].color` from configuration,
    /// else auto]. `NO_COLOR`, `CLICOLOR_FORCE` and `TERM=dumb` are honored
    /// in auto mode.
    #[arg(long, global = true, value_enum)]
    pub(crate) color: Option<ColorArg>,

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

    /// Show what apply would change, and drift since the last apply.
    Plan {
        /// Limit to profiles or paths: `casa`, `casa.denylist`,
        /// `*.privacy.blocklists`. Repeatable.
        #[arg(long = "target", value_name = "PATH")]
        targets: Vec<String>,
        /// Exit with 2 when there are changes (0 when none).
        #[arg(long)]
        detailed_exitcode: bool,
        /// Output format.
        #[arg(long, value_enum, default_value_t = PlanFormat::Text)]
        format: PlanFormat,
    },

    /// Apply the plan: snapshot, change, verify.
    Apply {
        /// Limit to profiles or paths, as for `plan`. Repeatable.
        #[arg(long = "target", value_name = "PATH")]
        targets: Vec<String>,
        /// Do not ask for confirmation.
        #[arg(long)]
        auto_approve: bool,
        /// Allow emptying authoritative lists and deleting profiles whose
        /// file was removed.
        #[arg(long)]
        allow_destroy: bool,
    },

    /// Report changes made outside ndac since the last apply. Exits with 2
    /// when there is drift.
    Drift {
        /// Limit to profiles or paths, as for `plan`. Repeatable.
        #[arg(long = "target", value_name = "PATH")]
        targets: Vec<String>,
        /// Output format.
        #[arg(long, value_enum, default_value_t = DriftFormat::Text)]
        format: DriftFormat,
        /// With `--format prom`, write `ndac_drift.prom` atomically into this
        /// directory (`node_exporter` textfile collector) instead of stdout.
        #[arg(long, value_name = "DIR")]
        textfile_dir: Option<std::path::PathBuf>,
    },

    /// Resolve drift: overwrite the live value, import it into the
    /// configuration, or skip, item by item.
    Reconcile {
        /// Limit to profiles or paths, as for `plan`. Repeatable.
        #[arg(long = "target", value_name = "PATH")]
        targets: Vec<String>,
        /// Answer every item this way: `code` overwrites NextDNS, `live`
        /// imports into the configuration.
        #[arg(long, value_enum)]
        prefer: Option<Prefer>,
        /// Never prompt; imports go into the profile file. Requires
        /// `--prefer`.
        #[arg(long, requires = "prefer")]
        non_interactive: bool,
        /// Do not add `# imported <date> (drift)` comments to edited lines.
        #[arg(long)]
        no_comment: bool,
    },

    /// Write live profiles to TOML files under profiles/.
    Import {
        /// Profile ID to import. Repeatable.
        #[arg(long = "profile", value_name = "ID", required_unless_present = "all")]
        profiles: Vec<String>,
        /// Import every profile of the account.
        #[arg(long, conflicts_with = "profiles")]
        all: bool,
        /// Overwrite existing files for the same profile.
        #[arg(long)]
        force: bool,
    },

    /// Re-apply a snapshot as the desired state of its profile.
    Rollback {
        /// Snapshot ID or a unique prefix.
        snapshot: String,
        /// Look for the snapshot only in this profile.
        #[arg(long)]
        profile: Option<String>,
        /// Do not ask for confirmation.
        #[arg(long)]
        auto_approve: bool,
    },

    /// Scaffold a configuration repository.
    Init {
        /// Directory to initialize.
        #[arg(default_value = ".")]
        dir: std::path::PathBuf,
        /// Also write a GitHub Actions workflow: plan on pull requests,
        /// apply on main, daily drift check.
        #[arg(long)]
        github_actions: bool,
    },

    /// Print configuration for devices and resolvers. Never installs
    /// anything.
    #[command(subcommand)]
    Endpoint(EndpointCommand),

    /// Check whether this host's DNS queries reach NextDNS, using
    /// test.nextdns.io. Exits with 2 on a mismatch. The API key is not used.
    Doctor {
        /// Profile key or NextDNS profile ID the queries should use.
        #[arg(long, value_name = "PROFILE")]
        expect_profile: Option<String>,
        /// Device name the queries should carry.
        #[arg(long, value_name = "NAME")]
        expect_device: Option<String>,
        /// Test service URL.
        #[arg(long, hide = true, env = "NDAC_DOCTOR_URL", default_value = ndac_api::TEST_URL)]
        url: String,
    },

    /// Print shell completions.
    Completions {
        /// Target shell.
        shell: clap_complete::Shell,
    },

    /// Print the man page (roff).
    Man,

    /// Inspect local snapshots.
    #[command(subcommand)]
    Snapshot(SnapshotCommand),

    /// Inspect and store the NextDNS API key.
    #[command(subcommand)]
    Auth(AuthCommand),

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
    /// The per-user configuration file.
    User,
}

#[derive(Debug, Subcommand)]
pub(crate) enum AuthCommand {
    /// Resolve the key and verify it against the API. Never prints the key.
    Check {
        /// Use this profile's `[auth]` instead of the root configuration.
        #[arg(long)]
        profile: Option<String>,
    },

    /// Show the source chain and which source resolves. May run `cmd:`
    /// sources, which can prompt.
    Sources {
        /// Use this profile's `[auth]` instead of the root configuration.
        #[arg(long)]
        profile: Option<String>,
    },

    /// Store a key, read from a hidden prompt or from stdin.
    Set {
        /// Where to store the key.
        #[arg(long, value_enum)]
        backend: Backend,
        /// Keyring entry as SERVICE/USER (keyring backend).
        #[arg(long, default_value = "nextdns/default", value_name = "SERVICE/USER")]
        keyring: String,
        /// Destination file (file backend).
        #[arg(long, value_name = "PATH", required_if_eq("backend", "file"))]
        path: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Backend {
    /// OS keyring (Secret Service, macOS Keychain).
    Keyring,
    /// `api_key` in ndac.local.toml (mode 0600, git-ignored).
    Config,
    /// A file with mode 0600, for `file:` or `systemd-cred:` sources.
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum DriftFormat {
    Text,
    Json,
    /// `::warning` workflow annotations.
    Github,
    /// Prometheus text format.
    Prom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Prefer {
    /// Keep the configuration: overwrite NextDNS.
    Code,
    /// Keep NextDNS: import into the configuration.
    Live,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum PlanFormat {
    Text,
    Json,
    Markdown,
}

#[derive(Debug, Subcommand)]
pub(crate) enum EndpointCommand {
    /// Print configuration text for one profile to stdout.
    Render {
        /// Profile key (file stem under profiles/).
        profile: String,
        /// What to generate.
        #[arg(long, value_enum)]
        target: EndpointTarget,
        /// Device name, so queries show up under it in NextDNS logs.
        #[arg(long, value_name = "NAME")]
        device: Option<String>,
        /// Add the profile's IPv6 servers to the resolved and networkmanager
        /// targets, read from the API. UNVERIFIED: uses the undocumented
        /// setup endpoint.
        #[cfg(feature = "unstable-endpoints")]
        #[arg(long)]
        api_ipv6: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum EndpointTarget {
    /// /etc/nextdns.conf for the nextdns client.
    NextdnsCli,
    /// A systemd-resolved drop-in (DNS over TLS).
    Resolved,
    /// nmcli commands (DNS over TLS through systemd-resolved).
    Networkmanager,
    /// The Android Private DNS hostname.
    Android,
    /// The DNS-over-HTTPS URL.
    DohUrl,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SnapshotCommand {
    /// List snapshots, oldest first.
    List {
        /// Only this profile.
        #[arg(long)]
        profile: Option<String>,
    },
    /// Print a snapshot as JSON.
    Show {
        /// Snapshot ID or a unique prefix.
        id: String,
        /// Look only in this profile.
        #[arg(long)]
        profile: Option<String>,
    },
    /// Show the changes from snapshot A to snapshot B.
    Diff {
        /// Older snapshot.
        a: String,
        /// Newer snapshot.
        b: String,
        /// Look only in this profile.
        #[arg(long)]
        profile: Option<String>,
    },
}
