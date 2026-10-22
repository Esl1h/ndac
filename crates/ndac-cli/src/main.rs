//! `ndac`: NextDNS-as-Code, NextDNS profiles managed from TOML files.

#![forbid(unsafe_code)]

use std::process::ExitCode;

use clap::Parser;
use tracing_subscriber::{EnvFilter, fmt};

mod cli;
mod commands;
mod hardening;
mod output;

use cli::{AuthCommand, Cli, Command, EndpointCommand, GlobalArgs, LogFormat, SnapshotCommand};
use output::{Exit, Output, RedactingStderr};

fn main() -> ExitCode {
    hardening::apply();
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

    match dispatch(cli, out) {
        Ok(code) => code.into(),
        Err(failure) => {
            let code = failure.exit();
            let text = format!("{:?}", failure.report());
            eprintln!("{}", ndac_core::auth::redact(&text));
            code.into()
        }
    }
}

fn dispatch(cli: Cli, out: Output) -> commands::CmdResult {
    let config = cli.global.config.as_deref();
    match cli.command {
        Command::Validate => commands::validate::run(config, out),
        Command::Render { profile, format } => commands::render::run(config, &profile, format),
        Command::Fmt { check } => commands::fmt::run(config, check, out),
        Command::Schema { kind } => commands::schema::run(kind),
        Command::Plan {
            targets,
            detailed_exitcode,
            format,
        } => commands::plan::run(&cli.global, &targets, detailed_exitcode, format, out),
        Command::Drift {
            targets,
            format,
            textfile_dir,
        } => commands::drift::run(&cli.global, &targets, format, textfile_dir.as_deref(), out),
        Command::Reconcile {
            targets,
            prefer,
            non_interactive,
            no_comment,
        } => commands::reconcile::run(
            &cli.global,
            &targets,
            prefer,
            non_interactive,
            no_comment,
            out,
        ),
        Command::Apply {
            targets,
            auto_approve,
            allow_destroy,
        } => commands::apply::run(&cli.global, &targets, auto_approve, allow_destroy, out),
        Command::Rollback {
            snapshot,
            profile,
            auto_approve,
        } => commands::apply::rollback(
            &cli.global,
            &snapshot,
            profile.as_deref(),
            auto_approve,
            out,
        ),
        Command::Import {
            profiles,
            all,
            force,
        } => commands::import::run(&cli.global, &profiles, all, force, out),
        Command::Init {
            dir,
            github_actions,
        } => commands::init::run(&dir, github_actions, out),
        Command::Endpoint(EndpointCommand::Render {
            profile,
            target,
            device,
        }) => commands::endpoint::render(config, &profile, target, device.as_deref()),
        Command::Doctor {
            expect_profile,
            expect_device,
            url,
        } => commands::doctor::run(
            config,
            expect_profile.as_deref(),
            expect_device.as_deref(),
            &url,
        ),
        Command::Completions { shell } => commands::generate::completions(shell),
        Command::Man => commands::generate::man(),
        Command::Snapshot(SnapshotCommand::List { profile }) => {
            commands::snapshot::list(config, profile.as_deref())
        }
        Command::Snapshot(SnapshotCommand::Show { id, profile }) => {
            commands::snapshot::show(config, &id, profile.as_deref())
        }
        Command::Snapshot(SnapshotCommand::Diff { a, b, profile }) => {
            commands::snapshot::diff(config, &a, &b, profile.as_deref(), out)
        }
        Command::Auth(AuthCommand::Check { profile }) => {
            commands::auth::check(&cli.global, profile.as_deref())
        }
        Command::Auth(AuthCommand::Sources { profile }) => {
            commands::auth::sources(&cli.global, profile.as_deref())
        }
        Command::Auth(AuthCommand::Set {
            backend,
            keyring,
            path,
        }) => commands::auth::set(&cli.global, backend, &keyring, path.as_deref(), out),
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
    let builder = fmt().with_env_filter(filter).with_writer(RedactingStderr);
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
