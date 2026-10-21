use std::{collections::BTreeMap, fs, path::Path, process::Command};

use super::*;

const KEY: &str = "test-key-0000000000000000000000000000000001";

fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    let mut m: BTreeMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    if let Ok(path) = std::env::var("PATH") {
        m.entry("PATH".into()).or_insert(path);
    }
    m
}

fn refs(v: &[&str]) -> Vec<SourceRef> {
    v.iter().map(|s| s.parse().unwrap()).collect()
}

#[cfg(unix)]
fn chmod(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn private_file(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
    let p = dir.join(name);
    fs::write(&p, text).unwrap();
    #[cfg(unix)]
    chmod(&p, 0o600);
    p
}

fn literal(path: &Path) -> Literal {
    let key: SecretValue = toml::from_str::<BTreeMap<String, SecretValue>>(&format!("k = {KEY:?}"))
        .unwrap()
        .remove("k")
        .unwrap();
    Literal {
        key,
        path: path.to_path_buf(),
    }
}

#[test]
fn env_source_and_missing_chain() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = Context::new(dir.path(), dir.path());
    let r = resolve(
        &refs(&["env:NEXTDNS_API_KEY"]),
        &ctx,
        &env(&[("NEXTDNS_API_KEY", KEY)]),
    )
    .unwrap();
    assert_eq!(r.key.expose(), KEY);
    assert_eq!(r.source.to_string(), "env:NEXTDNS_API_KEY");
    assert_eq!(r.warnings, Vec::<String>::new());

    let err = resolve(&refs(&["env:A", "env:B"]), &ctx, &env(&[("A", "  ")])).unwrap_err();
    let AuthError::NoKey { tried } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(tried.len(), 2);
}

#[test]
fn first_available_source_wins() {
    let dir = tempfile::tempdir().unwrap();
    let f = private_file(dir.path(), "key", &format!("{KEY}\n"));
    let ctx = Context::new(dir.path(), dir.path());
    let chain = refs(&["env:MISSING", &format!("file:{}", f.display()), "env:LATER"]);
    let r = resolve(&chain, &ctx, &env(&[("LATER", "other-key-000000000000")])).unwrap();
    assert_eq!(r.key.expose(), KEY, "trailing newline trimmed");
    assert_eq!(r.origin.as_deref(), Some(f.as_path()));
}

#[test]
fn env_file_and_systemd_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let f = private_file(dir.path(), "nextdns-api-key", KEY);
    let ctx = Context::new(dir.path(), dir.path());
    let e = env(&[
        ("NEXTDNS_API_KEY_FILE", f.to_str().unwrap()),
        ("CREDENTIALS_DIRECTORY", dir.path().to_str().unwrap()),
    ]);
    for chain in [
        &["env-file:NEXTDNS_API_KEY_FILE"][..],
        &["systemd-cred:nextdns-api-key"],
    ] {
        assert_eq!(resolve(&refs(chain), &ctx, &e).unwrap().key.expose(), KEY);
    }
    // env-file pointing to a missing file is a failure, not "missing".
    let bad = env(&[("NEXTDNS_API_KEY_FILE", "/nonexistent/ndac/key")]);
    assert!(matches!(
        resolve(&refs(&["env-file:NEXTDNS_API_KEY_FILE"]), &ctx, &bad),
        Err(AuthError::Source { .. })
    ));
}

#[test]
fn dotenv_is_parsed_without_touching_the_environment() {
    let dir = tempfile::tempdir().unwrap();
    private_file(
        dir.path(),
        ".env",
        &format!("# c\nOTHER=1\nNEXTDNS_API_KEY=\"{KEY}\"\n"),
    );
    let ctx = Context::new(dir.path(), dir.path());
    let r = resolve(&refs(&["dotenv"]), &ctx, &env(&[])).unwrap();
    assert_eq!(r.key.expose(), KEY);
    assert!(std::env::var("OTHER").is_err());
    assert!(
        r.warnings.iter().any(|w| w.contains("literal")),
        "{:?}",
        r.warnings
    );
}

#[cfg(unix)]
#[test]
fn literal_key_in_world_readable_file_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let p = private_file(dir.path(), ".env", &format!("NEXTDNS_API_KEY={KEY}\n"));
    chmod(&p, 0o644);
    let mut ctx = Context::new(dir.path(), dir.path());
    let err = resolve(&refs(&["dotenv"]), &ctx, &env(&[])).unwrap_err();
    assert!(
        matches!(err, AuthError::InsecurePermissions { mode: 0o644, .. }),
        "{err:?}"
    );
    assert!(err.to_string().contains("mode 644"));

    ctx.allow_insecure_permissions = true;
    let r = resolve(&refs(&["dotenv"]), &ctx, &env(&[])).unwrap();
    assert!(
        r.warnings
            .iter()
            .any(|w| w.contains("allow_insecure_permissions"))
    );
}

#[allow(clippy::disallowed_methods, reason = "test setup")]
fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

#[test]
fn literal_key_in_tracked_or_unignored_file_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    let root = private_file(dir.path(), "ndac.toml", "schema = 1\n");
    let mut ctx = Context::new(dir.path(), dir.path());
    ctx.literals.push(literal(&root));

    // Untracked and not ignored.
    let err = resolve(&refs(&["config"]), &ctx, &env(&[])).unwrap_err();
    assert!(
        matches!(
            err,
            AuthError::TrackedSecret {
                reason: "is not ignored by git",
                ..
            }
        ),
        "{err:?}"
    );

    git(dir.path(), &["add", "ndac.toml"]);
    let err = resolve(&refs(&["config"]), &ctx, &env(&[])).unwrap_err();
    assert!(matches!(
        err,
        AuthError::TrackedSecret {
            reason: "is tracked by git",
            ..
        }
    ));

    ctx.allow_tracked_secret = true;
    let r = resolve(&refs(&["config"]), &ctx, &env(&[])).unwrap();
    assert!(r.warnings.iter().any(|w| w.starts_with("WARNING:")));
}

#[test]
fn literal_key_in_ignored_file_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    fs::write(dir.path().join(".gitignore"), "ndac.local.toml\n").unwrap();
    let local = private_file(dir.path(), "ndac.local.toml", "");
    let mut ctx = Context::new(dir.path(), dir.path());
    ctx.literals.push(literal(&local));
    let r = resolve(&refs(&["config"]), &ctx, &env(&[])).unwrap();
    assert_eq!(r.key.expose(), KEY);
    assert_eq!(r.warnings.len(), 1, "one literal-key warning");
    ctx.quiet_literal = true;
    let quiet = resolve(&refs(&["config"]), &ctx, &env(&[])).unwrap();
    assert_eq!(quiet.warnings, Vec::<String>::new());
}

#[cfg(unix)]
#[test]
fn cmd_runs_without_a_shell_and_with_a_clean_env() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = Context::new(dir.path(), dir.path());
    let e = env(&[("SECRET_ELSEWHERE", "do-not-leak")]);

    let r = resolve(&refs(&[&format!("cmd:echo {KEY}")]), &ctx, &e).unwrap();
    assert_eq!(r.key.expose(), KEY);

    // Shell syntax is passed literally to the program, not interpreted.
    let r = resolve(&refs(&["cmd:echo $HOME;ignored-key-000000000"]), &ctx, &e).unwrap();
    assert_eq!(r.key.expose(), "$HOME;ignored-key-000000000");

    // Only passthrough variables reach the child: printenv fails for an
    // unset variable.
    let err = resolve(&refs(&["cmd:printenv SECRET_ELSEWHERE"]), &ctx, &e).unwrap_err();
    assert!(err.to_string().contains("exited with"), "{err}");
    let path = sources::run("printenv", &["PATH".to_owned()], &e, DEFAULT_CMD_TIMEOUT).unwrap();
    assert_eq!(path, e["PATH"].lines().next().unwrap_or_default().trim());
}

#[cfg(unix)]
#[test]
fn cmd_failures() {
    let dir = tempfile::tempdir().unwrap();
    let mut ctx = Context::new(dir.path(), dir.path());
    let e = env(&[]);
    for (cmd, needle) in [
        ("cmd:false", "exited with"),
        ("cmd:ndac-no-such-program-xyz", "not found in PATH"),
        ("cmd:true", "printed nothing"),
        ("cmd:echo 'unterminated", "cannot parse"),
    ] {
        let err = resolve(&refs(&[cmd]), &ctx, &e).unwrap_err();
        assert!(err.to_string().contains(needle), "{cmd}: {err}");
    }
    ctx.cmd_timeout = Duration::from_millis(200);
    let err = resolve(&refs(&["cmd:sleep 5"]), &ctx, &e).unwrap_err();
    assert!(err.to_string().contains("timed out"), "{err}");

    // on_source_error = skip moves on to the next source.
    ctx.on_source_error = OnSourceError::Skip;
    let r = resolve(&refs(&["cmd:false", "env:K"]), &ctx, &env(&[("K", KEY)])).unwrap();
    assert_eq!(r.key.expose(), KEY);
}

#[test]
fn guardrail_violations_are_never_skipped() {
    let err = AuthError::TrackedSecret {
        path: "x".into(),
        reason: "is tracked by git",
    };
    assert!(!err.is_skippable());
}

#[test]
fn chain_precedence() {
    let root = AuthConfig {
        sources: Some(vec!["env:ROOT".into()]),
        ..AuthConfig::default()
    };
    let profile = AuthConfig {
        sources: Some(vec!["env:PROFILE".into()]),
        ..AuthConfig::default()
    };
    let s = |c: Vec<SourceRef>| c.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert_eq!(
        s(chain(&["env:CLI".into()], Some(&profile), &root).unwrap()),
        ["env:CLI"]
    );
    assert_eq!(
        s(chain(&[], Some(&profile), &root).unwrap()),
        ["env:PROFILE"]
    );
    assert_eq!(s(chain(&[], None, &root).unwrap()), ["env:ROOT"]);
    assert_eq!(
        s(chain(&[], None, &AuthConfig::default()).unwrap()),
        DEFAULT_CHAIN
    );
}

#[test]
fn resolved_keys_are_redacted_and_never_debug_printed() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = Context::new(dir.path(), dir.path());
    let key = "test-key-0000000000000000000000000000009999";
    let r = resolve(&refs(&["env:K"]), &ctx, &env(&[("K", key)])).unwrap();
    assert!(!format!("{r:?}").contains(key));
    assert_eq!(
        redact(&format!("header x-api-key: {key}")),
        "header x-api-key: ****"
    );
}

#[test]
fn store_local_config_is_private_and_ignored() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("ndac.local.toml"),
        "# mine\n[provider]\nretries = 2\n",
    )
    .unwrap();
    fs::write(dir.path().join(".gitignore"), "target/").unwrap();
    let p = store::local_config(dir.path(), KEY).unwrap();
    let text = fs::read_to_string(&p).unwrap();
    assert!(
        text.starts_with("# mine\n[provider]\nretries = 2\n"),
        "{text}"
    );
    assert!(text.contains(&format!("api_key = \"{KEY}\"")));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert_eq!(
        fs::read_to_string(dir.path().join(".gitignore")).unwrap(),
        "target/\nndac.local.toml\n.env\n"
    );
    // Idempotent.
    store::local_config(dir.path(), KEY).unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join(".gitignore")).unwrap(),
        "target/\nndac.local.toml\n.env\n"
    );
}

#[test]
fn store_file_is_private() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("key");
    store::file(&p, KEY).unwrap();
    assert_eq!(fs::read_to_string(&p).unwrap(), format!("{KEY}\n"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn dotenv_explicit_and_malformed() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = Context::new(dir.path(), dir.path());
    let missing = dir.path().join("nope.env");
    let err = resolve(
        &refs(&[&format!("dotenv:{}", missing.display())]),
        &ctx,
        &env(&[]),
    )
    .unwrap_err();
    assert!(matches!(err, AuthError::NoKey { .. }), "{err:?}");

    let bad = private_file(dir.path(), "bad.env", "NEXTDNS_API_KEY='unterminated\n");
    let err = resolve(
        &refs(&[&format!("dotenv:{}", bad.display())]),
        &ctx,
        &env(&[]),
    )
    .unwrap_err();
    assert!(err.to_string().contains("cannot parse"), "{err}");

    let other = private_file(dir.path(), "other.env", "SOMETHING_ELSE=1\n");
    let err = resolve(
        &refs(&[&format!("dotenv:{}", other.display())]),
        &ctx,
        &env(&[]),
    )
    .unwrap_err();
    assert!(matches!(err, AuthError::NoKey { .. }));
}

#[test]
fn empty_file_is_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let f = private_file(dir.path(), "key", "\n");
    let ctx = Context::new(dir.path(), dir.path());
    let err = resolve(&refs(&[&format!("file:{}", f.display())]), &ctx, &env(&[])).unwrap_err();
    assert!(err.to_string().contains("empty key"), "{err}");
}

#[cfg(feature = "age")]
#[test]
fn age_source_requires_an_identity() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = Context::new(dir.path(), dir.path());
    let enc = private_file(dir.path(), "key.age", "not really encrypted");
    let err = resolve(&refs(&[&format!("age:{}", enc.display())]), &ctx, &env(&[])).unwrap_err();
    assert!(err.to_string().contains("NDAC_AGE_IDENTITY"), "{err}");
    let missing = dir.path().join("none.age");
    let err = resolve(
        &refs(&[&format!("age:{}", missing.display())]),
        &ctx,
        &env(&[]),
    )
    .unwrap_err();
    assert!(matches!(err, AuthError::NoKey { .. }));
}
