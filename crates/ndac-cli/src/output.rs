//! Terminal output. Requested artifacts (rendered profiles, plans, schemas)
//! go to stdout; everything meant for humans goes to stderr.

use std::{
    env,
    io::{self, IsTerminal, Write},
    process::ExitCode,
};

use ndac_core::auth::redact;
use tracing_subscriber::fmt::MakeWriter;

use crate::cli::ColorArg;

/// stderr writer for logs that masks every resolved key (defense in depth:
/// keys are never logged on purpose).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RedactingStderr;

impl Write for RedactingStderr {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        io::stderr().lock().write_all(redact(&text).as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()
    }
}

impl<'a> MakeWriter<'a> for RedactingStderr {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        *self
    }
}

/// Exit codes shared by every command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Exit {
    Success = 0,
    Error = 1,
    /// Changes or drift pending (`plan --detailed-exitcode`, `drift`).
    #[allow(dead_code, reason = "used from Phase 4 on")]
    Changes = 2,
    Auth = 3,
    Config = 4,
    Api = 5,
}

impl From<Exit> for ExitCode {
    fn from(e: Exit) -> Self {
        Self::from(e as u8)
    }
}

/// Output settings resolved from flags and environment.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Output {
    pub(crate) quiet: bool,
    /// Color on stderr.
    pub(crate) color: bool,
    /// Color in artifacts written to stdout (plan text).
    pub(crate) stdout_color: bool,
}

impl Output {
    pub(crate) fn new(quiet: bool, color: ColorArg) -> Self {
        Self {
            quiet,
            color: use_color(color, io::stderr().is_terminal()),
            stdout_color: use_color(color, io::stdout().is_terminal()),
        }
    }

    /// Writes an artifact to stdout. A closed pipe (`ndac render x | head`)
    /// is not an error.
    pub(crate) fn artifact(text: &str) -> io::Result<()> {
        let mut out = io::stdout().lock();
        match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
            other => other,
        }
    }

    /// A line of the requested result on stdout.
    pub(crate) fn line(text: &str) -> io::Result<()> {
        Self::artifact(&format!("{text}\n"))
    }

    /// A warning on stderr; shown even with `--quiet`.
    pub(crate) fn warn(text: &str) {
        let _ = writeln!(io::stderr().lock(), "warning: {}", redact(text));
    }

    /// A status line on stderr, suppressed by `--quiet`.
    pub(crate) fn status(self, text: &str) {
        if !self.quiet {
            let _ = writeln!(io::stderr().lock(), "{text}");
        }
    }
}

fn use_color(arg: ColorArg, is_tty: bool) -> bool {
    match arg {
        ColorArg::Always => true,
        ColorArg::Never => false,
        ColorArg::Auto => {
            let set = |k: &str| env::var_os(k).is_some_and(|v| !v.is_empty());
            if set("NO_COLOR") {
                false
            } else if env::var_os("CLICOLOR_FORCE").is_some_and(|v| v != "0") {
                true
            } else {
                is_tty && env::var_os("TERM").is_none_or(|t| t != "dumb")
            }
        }
    }
}
