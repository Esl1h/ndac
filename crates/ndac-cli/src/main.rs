//! `ndac`: NextDNS-as-Code, NextDNS profiles managed from TOML files.

// Not `forbid`: the Linux `prctl(PR_SET_DUMPABLE)` hardening call will live in
// a single module of this crate with a scoped `allow`.
#![deny(unsafe_code)]

use std::{io, process::ExitCode};

use clap::Parser;
use tracing_subscriber::{EnvFilter, fmt};

mod cli;
mod commands;
mod output;

use cli::{Cli, Command, GlobalArgs, LogFormat};
use output::{Exit, Output};

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            // clap uses 2 for usage errors, which is reserved for "changes
            // pending" here.
            return if e.use_stderr() {
                Exit::Error
            } else {
                Exit::Success
            }
            .into();
        }
    };
    let out = Output::new(cli.global.quiet, cli.global.color);
    init_logging(&cli.global, out.color);
    install_miette_hook(out.color);

    let config = cli.global.config.as_deref();
    let result = match cli.command {
        Command::Validate => commands::validate::run(config, out),
        Command::Render { profile, format } => commands::render::run(config, &profile, format),
        Command::Fmt { check } => commands::fmt::run(config, check, out),
        Command::Schema { kind } => commands::schema::run(kind),
    };
    match result {
        Ok(code) => code.into(),
        Err(failure) => {
            let code = failure.exit();
            eprintln!("{:?}", failure.report());
            code.into()
        }
    }
}

fn init_logging(global: &GlobalArgs, color: bool) {
    let level = match (global.quiet, global.verbose) {
        (true, _) => "error",
        (false, 0) => "warn",
        (false, 1) => "info",
        (false, 2) => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_env("NDAC_LOG").unwrap_or_else(|_| {
        EnvFilter::new(format!(
            "warn,ndac={level},ndac_core={level},ndac_api={level}"
        ))
    });
    let builder = fmt().with_env_filter(filter).with_writer(io::stderr);
    match global.log_format {
        LogFormat::Text => builder.with_ansi(color).without_time().init(),
        LogFormat::Json => builder.json().init(),
    }
}

fn install_miette_hook(color: bool) {
    let _ = miette::set_hook(Box::new(move |_| {
        Box::new(
            miette::MietteHandlerOpts::new()
                .color(color)
                .unicode(color)
                .build(),
        )
    }));
}
