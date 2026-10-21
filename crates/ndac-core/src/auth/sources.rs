//! Reading each kind of source.

use std::{
    fs, io,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use super::{AuthError, Context, Env, SourceRef};

/// Environment passed to `cmd:` and `age:` children; everything else is
/// cleared so the child never sees unrelated secrets.
const PASSTHROUGH: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LANG",
    "GNUPGHOME",
    "GPG_TTY",
    "PASSWORD_STORE_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    "XDG_RUNTIME_DIR",
];

/// Variable looked up in `.env` files.
const DOTENV_VAR: &str = "NEXTDNS_API_KEY";

pub(super) struct Found {
    pub(super) value: String,
    pub(super) origin: Option<PathBuf>,
    /// The key sits in a file the user edits, so guardrails apply.
    pub(super) literal: bool,
}

impl Found {
    fn plain(value: String, origin: Option<PathBuf>) -> Self {
        Self {
            value,
            origin,
            literal: false,
        }
    }
}

pub(super) fn missing_reason(source: &SourceRef) -> String {
    match source {
        SourceRef::Config => "no api_key in configuration".into(),
        SourceRef::Env(v) | SourceRef::EnvFile(v) => format!("{v} is not set"),
        SourceRef::File(p) | SourceRef::Age(p) => format!("{} does not exist", p.display()),
        SourceRef::Dotenv(_) => format!("no .env with {DOTENV_VAR}"),
        SourceRef::Keyring { .. } => "no keyring entry or keyring unavailable".into(),
        SourceRef::SystemdCred(_) => "no systemd credential".into(),
        SourceRef::Cmd(_) => "command produced nothing".into(),
    }
}

pub(super) fn read(
    source: &SourceRef,
    ctx: &Context,
    env: &dyn Env,
) -> Result<Option<Found>, AuthError> {
    let fail = |reason: String| AuthError::Source {
        source_ref: source.to_string(),
        reason,
    };
    match source {
        SourceRef::Config => Ok(ctx.literals.first().map(|l| Found {
            value: l.key.expose().trim().to_owned(),
            origin: Some(l.path.clone()),
            literal: true,
        })),
        SourceRef::Env(var) => Ok(env
            .var(var)
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .map(|v| Found::plain(v, None))),
        SourceRef::EnvFile(var) => match env.var(var).filter(|v| !v.is_empty()) {
            None => Ok(None),
            Some(path) => {
                let path = PathBuf::from(path);
                read_file(&path)
                    .map(|v| Some(Found::plain(v, Some(path))))
                    .map_err(fail)
            }
        },
        SourceRef::File(path) => {
            if !path.exists() {
                return Ok(None);
            }
            read_file(path)
                .map(|v| Some(Found::plain(v, Some(path.clone()))))
                .map_err(fail)
        }
        SourceRef::SystemdCred(name) => {
            let Some(dir) = env.var("CREDENTIALS_DIRECTORY").filter(|d| !d.is_empty()) else {
                return Ok(None);
            };
            let path = Path::new(&dir).join(name);
            if !path.exists() {
                return Ok(None);
            }
            read_file(&path)
                .map(|v| Some(Found::plain(v, Some(path))))
                .map_err(fail)
        }
        SourceRef::Dotenv(explicit) => dotenv(explicit.as_deref(), ctx).map_err(fail),
        SourceRef::Keyring { service, user } => keyring(service, user).map_err(fail),
        SourceRef::Cmd(cmdline) => {
            let words = shell_words::split(cmdline)
                .map_err(|e| fail(format!("cannot parse command: {e}")))?;
            let Some((program, args)) = words.split_first() else {
                return Err(fail("empty command".into()));
            };
            run(program, args, env, ctx.cmd_timeout)
                .map(|v| Some(Found::plain(v, None)))
                .map_err(fail)
        }
        SourceRef::Age(path) => age(path, ctx, env).map_err(fail),
    }
}

fn read_file(path: &Path) -> Result<String, String> {
    fs::read_to_string(path)
        .map(|s| s.trim().to_owned())
        .map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn dotenv(explicit: Option<&Path>, ctx: &Context) -> Result<Option<Found>, String> {
    let candidates: Vec<PathBuf> = if let Some(p) = explicit {
        vec![p.to_path_buf()]
    } else {
        let mut c = vec![ctx.cwd.join(".env")];
        let at_root = ctx.root_dir.join(".env");
        if !c.contains(&at_root) {
            c.push(at_root);
        }
        c
    };
    let Some(path) = candidates.into_iter().find(|p| p.is_file()) else {
        return Ok(None);
    };
    // from_path_iter parses without touching the process environment.
    let iter = dotenvy::from_path_iter(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    for item in iter {
        let (k, v) = item.map_err(|_| format!("cannot parse {}", path.display()))?;
        if k == DOTENV_VAR {
            let v = v.trim().to_owned();
            return Ok((!v.is_empty()).then_some(Found {
                value: v,
                origin: Some(path),
                literal: true,
            }));
        }
    }
    Ok(None)
}

#[cfg(feature = "keyring")]
fn keyring(service: &str, user: &str) -> Result<Option<Found>, String> {
    use keyring::Error as K;
    if keyring::Entry::store_status().is_err() {
        tracing::debug!("no OS keyring store available");
        return Ok(None);
    }
    let entry = keyring::Entry::new(service, user).map_err(|e| keyring_error(&e))?;
    match entry.get_password() {
        Ok(v) => Ok(Some(Found::plain(v.trim().to_owned(), None))),
        Err(K::NoEntry) => Ok(None),
        // No Secret Service on this machine (servers, CI): treat as absent.
        Err(K::PlatformFailure(e)) => {
            tracing::debug!(error = %e, "keyring platform failure");
            Ok(None)
        }
        Err(e) => Err(keyring_error(&e)),
    }
}

#[cfg(feature = "keyring")]
fn keyring_error(e: &keyring::Error) -> String {
    use keyring::Error as K;
    // Some variants carry the secret bytes; never format those.
    match e {
        K::NoStorageAccess(_) => "keyring is locked or access was denied".into(),
        K::BadEncoding(_) | K::BadDataFormat(..) => "keyring entry is not valid text".into(),
        K::Ambiguous(_) => "more than one keyring entry matches".into(),
        K::NoEntry => "no keyring entry".into(),
        other => format!("keyring error: {}", short_kind(other)),
    }
}

#[cfg(feature = "keyring")]
fn short_kind(e: &keyring::Error) -> &'static str {
    use keyring::Error as K;
    match e {
        K::PlatformFailure(_) => "platform failure",
        K::TooLong(..) => "attribute too long",
        K::Invalid(..) => "invalid entry",
        K::BadStoreFormat(_) => "unexpected store format",
        _ => "unexpected error",
    }
}

#[cfg(not(feature = "keyring"))]
fn keyring(_service: &str, _user: &str) -> Result<Option<Found>, String> {
    Err("ndac was built without keyring support".into())
}

#[cfg(feature = "age")]
fn age(path: &Path, ctx: &Context, env: &dyn Env) -> Result<Option<Found>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let identity = env
        .var("NDAC_AGE_IDENTITY")
        .filter(|v| !v.is_empty())
        .ok_or("set NDAC_AGE_IDENTITY to an identity file (or a plugin identity)")?;
    let args = vec![
        "--decrypt".to_owned(),
        "--identity".to_owned(),
        identity,
        path.display().to_string(),
    ];
    run("age", &args, env, ctx.cmd_timeout).map(|v| Some(Found::plain(v, Some(path.to_path_buf()))))
}

#[cfg(not(feature = "age"))]
fn age(_path: &Path, _ctx: &Context, _env: &dyn Env) -> Result<Option<Found>, String> {
    Err("ndac was built without the `age` feature".into())
}

/// Runs `program` without a shell, with a cleared environment, inherited
/// stdin (for pinentry or a security key touch) and stderr, and returns the
/// first line of stdout.
pub(super) fn run(
    program: &str,
    args: &[String],
    env: &dyn Env,
    timeout: Duration,
) -> Result<String, String> {
    #[allow(
        clippy::disallowed_methods,
        reason = "the only place that runs user-configured commands; no shell is involved"
    )]
    let mut command = Command::new(program);
    command
        .args(args)
        .env_clear()
        .envs(
            PASSTHROUGH
                .iter()
                .filter_map(|k| env.var(k).map(|v| (*k, v))),
        )
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = command.spawn().map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => format!("`{program}` not found in PATH"),
        _ => format!("cannot run `{program}`: {e}"),
    })?;

    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "`{program}` timed out after {}s",
                    timeout.as_secs()
                ));
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("cannot wait for `{program}`: {e}")),
        }
    };
    let output = rx.recv_timeout(Duration::from_secs(5)).unwrap_or_default();
    if !status.success() {
        return Err(format!("`{program}` exited with {status}"));
    }
    let text =
        String::from_utf8(output).map_err(|_| format!("`{program}` printed invalid UTF-8"))?;
    let first = text.lines().next().unwrap_or_default().trim().to_owned();
    if first.is_empty() {
        return Err(format!("`{program}` printed nothing"));
    }
    Ok(first)
}
