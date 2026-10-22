//! Loading, `extends` resolution and diagnostics of whole configurations.
#![allow(clippy::unwrap_used, reason = "tests")]

mod common;

use common::{ROOT, render, workspace};
use ndac_api::{ActiveItem, IdItem};
use ndac_core::config::{
    ConfigError, ListMode, Workspace, color_preference, discover,
    types::{ColorChoice, OnSourceError},
    user_config_path,
};

const BASE: &str = r#"
[security]
threatIntelligenceFeeds = true
nrd  = true
dga  = true
tlds = ["ru", "cn", "zip", "mov"]

[privacy]
blocklists        = ["nextdns-recommended", "oisd"]
natives           = ["samsung", "xiaomi", "windows"]
disguisedTrackers = true
allowAffiliate    = false

[settings.logs]
enabled   = true
retention = "30d"
location  = "eu"
drop      = { ip = false, domain = false }

[settings.performance]
ecs = true
cacheBoost = false
cnameFlattening = true
"#;

const PARANOID: &str = r#"
[security]
tlds = ["$inherit", "xyz"]
typosquatting = true

[privacy]
blocklists = ["$inherit", "hagezi-pro"]

[denylist.items]
"doubleclick.net" = true
"#;

const CASA: &str = r#"
id      = "abc123"
name    = "Casa"
extends = ["base", "paranoid"]
ignore  = ["parentalControl.services"]

[auth]
sources = ["cmd:pass show nextdns/work-api-key"]

[settings.logs]
retention = "90d"

[parentalControl]
safeSearch = true
categories = { gambling = true, porn = true }
services   = { tiktok = true }

[denylist]
mode = "authoritative"

[denylist.items]
"tiktokv.com"          = true
"AdService.Google.com." = true
"foo.example"          = false

[allowlist]
mode = "merge"

[allowlist.items]
"s.youtube.com" = true
"#;

fn spec_example() -> (tempfile::TempDir, std::path::PathBuf) {
    workspace(&[
        ("ndac.toml", ROOT),
        ("templates/base.toml", BASE),
        ("templates/paranoid.toml", PARANOID),
        ("profiles/casa.toml", CASA),
    ])
}

#[test]
fn resolves_the_documented_example() {
    let (_dir, root) = spec_example();
    let ws = Workspace::load(&root).unwrap();
    let p = ws.resolve("casa").unwrap();

    assert_eq!(p.id.as_deref(), Some("abc123"));
    assert_eq!(p.ignore, ["parentalControl.services"]);
    assert_eq!(p.list_modes["denylist"], ListMode::Authoritative);
    assert_eq!(p.list_modes["allowlist"], ListMode::Merge);
    let d = &p.desired;
    assert_eq!(d.name.as_deref(), Some("Casa"));

    let sec = d.security.as_ref().unwrap();
    assert_eq!(sec.typosquatting, Some(true));
    assert_eq!(sec.ai_threat_detection, None, "unset fields stay unmanaged");
    let tlds: Vec<_> = sec
        .tlds
        .as_ref()
        .unwrap()
        .iter()
        .map(|t| t.id.as_str())
        .collect();
    assert_eq!(tlds, ["cn", "mov", "ru", "xyz", "zip"]);

    let blocklists = d.privacy.as_ref().unwrap().blocklists.clone().unwrap();
    assert_eq!(
        blocklists,
        vec![
            IdItem::new("hagezi-pro"),
            IdItem::new("nextdns-recommended"),
            IdItem::new("oisd")
        ]
    );

    let logs = d.settings.as_ref().unwrap().logs.as_ref().unwrap();
    assert_eq!(logs.retention, Some(7_776_000));
    assert_eq!(logs.location.as_deref(), Some("eu"));

    assert_eq!(
        d.denylist.clone().unwrap(),
        vec![
            ActiveItem::new("adservice.google.com", true),
            ActiveItem::new("doubleclick.net", true),
            ActiveItem::new("foo.example", false),
            ActiveItem::new("tiktokv.com", true),
        ]
    );
    let auth = p.auth.unwrap();
    assert_eq!(
        auth.sources.unwrap(),
        ["cmd:pass show nextdns/work-api-key"]
    );
}

#[test]
fn render_outputs_are_stable() {
    let (_dir, root) = spec_example();
    let p = Workspace::load(&root).unwrap().resolve("casa").unwrap();
    insta::assert_snapshot!("render_casa_json", p.to_json());
    insta::assert_snapshot!("render_casa_toml", p.to_toml());
}

#[test]
fn remove_marker_deletes_inherited_items() {
    let (_dir, root) = workspace(&[
        ("ndac.toml", ROOT),
        (
            "templates/base.toml",
            "[denylist.items]\n\"a.example\" = true\n\"b.example\" = true\n",
        ),
        (
            "profiles/p.toml",
            "extends = [\"base\"]\n[denylist.items]\n\"B.example\" = \"$remove\"\n",
        ),
    ]);
    let p = Workspace::load(&root).unwrap().resolve("p").unwrap();
    assert_eq!(
        p.desired.denylist.unwrap(),
        vec![ActiveItem::new("a.example", true)]
    );
}

#[test]
fn literal_key_is_redacted_in_render() {
    let (_dir, root) = workspace(&[
        ("ndac.toml", ROOT),
        (
            "profiles/p.toml",
            "[auth]\napi_key = \"test-key-0000000000000000000000000000000000\"\n",
        ),
    ]);
    let p = Workspace::load(&root).unwrap().resolve("p").unwrap();
    for out in [p.to_json(), p.to_toml()] {
        assert!(out.contains("<redacted>"), "{out}");
        assert!(!out.contains("test-key-0000"), "{out}");
    }
}

#[test]
fn local_override_wins() {
    let (_dir, root) = workspace(&[
        (
            "ndac.toml",
            "schema = 1\n[provider]\nretries = 2\nconcurrency = 8\n[auth]\non_source_error = \"fail\"\n",
        ),
        (
            "ndac.local.toml",
            "[provider]\nretries = 9\n[auth]\non_source_error = \"skip\"\n",
        ),
    ]);
    let ws = Workspace::load(&root).unwrap();
    assert_eq!(ws.provider().retries, Some(9));
    assert_eq!(ws.provider().concurrency, Some(8));
    assert_eq!(ws.auth().on_source_error, Some(OnSourceError::Skip));
}

#[test]
fn user_file_is_below_the_repository() {
    let (dir, root) = workspace(&[
        ("ndac.toml", "schema = 1\n[auth]\nquiet_literal = true\n"),
        (
            "user/ndac/config.toml",
            "[auth]\nquiet_literal = false\nsources = [\"env:X\"]\non_source_error = \"skip\"\n",
        ),
    ]);
    let mut ws = Workspace::load(&root).unwrap();
    ws.load_user(&dir.path().join("user/ndac/config.toml"))
        .unwrap();
    let auth = ws.auth();
    assert_eq!(auth.quiet_literal, Some(true));
    assert_eq!(auth.sources, Some(vec!["env:X".to_owned()]));
    assert_eq!(auth.on_source_error, Some(OnSourceError::Skip));
    // A missing user file is not an error.
    ws.load_user(&dir.path().join("absent.toml")).unwrap();
}

#[test]
fn user_file_path_follows_xdg() {
    assert_eq!(
        user_config_path(Some("/x".into()), Some("/home/u".into())),
        Some("/x/ndac/config.toml".into())
    );
    // A relative XDG_CONFIG_HOME is invalid and ignored.
    assert_eq!(
        user_config_path(Some("rel".into()), Some("/home/u".into())),
        Some("/home/u/.config/ndac/config.toml".into())
    );
    assert_eq!(user_config_path(None, None), None);
}

#[test]
fn color_preference_order() {
    let (dir, root) = workspace(&[
        (
            "ndac.toml",
            "schema = 1\nbogus = 1\n[output]\ncolor = \"never\"\n",
        ),
        ("user.toml", "[output]\ncolor = \"always\"\n"),
    ]);
    let user = dir.path().join("user.toml");
    // Other keys, even invalid ones, do not hide the preference.
    assert_eq!(
        color_preference(Some(&root), Some(&user)),
        Some(ColorChoice::Never)
    );
    std::fs::write(
        dir.path().join("ndac.local.toml"),
        "[output]\ncolor = \"auto\"\n",
    )
    .unwrap();
    assert_eq!(
        color_preference(Some(&root), Some(&user)),
        Some(ColorChoice::Auto)
    );
    assert_eq!(
        color_preference(None, Some(&user)),
        Some(ColorChoice::Always)
    );
    assert_eq!(color_preference(None, None), None);
}

#[test]
fn discovery_walks_up() {
    let (dir, root) = workspace(&[("ndac.toml", ROOT), ("profiles/sub/x.txt", "")]);
    let found = discover(None, &dir.path().join("profiles/sub")).unwrap();
    assert_eq!(found, root);
    let empty = tempfile::tempdir().unwrap();
    assert!(discover(None, empty.path()).is_err());
}

fn errors(files: &[(&str, &str)]) -> String {
    let (_dir, root) = workspace(files);
    let err = match Workspace::load(&root) {
        Ok(ws) => ws.resolve_all().unwrap_err(),
        Err(e) => e,
    };
    render(&err)
}

#[test]
fn diagnostic_unknown_key_suggests() {
    let out = errors(&[
        ("ndac.toml", ROOT),
        (
            "profiles/p.toml",
            "[privacy]\nblocklist = [\"oisd\"]\n[parental_control]\nsafeSearch = true\n",
        ),
    ]);
    insta::assert_snapshot!(out);
}

#[test]
fn diagnostic_invalid_values() {
    let out = errors(&[
        ("ndac.toml", ROOT),
        (
            "profiles/p.toml",
            r#"id = "ABC 1"
ignore = ["settings.log"]

[security]
tlds = ["ru", "ru"]

[denylist.items]
"*.example.com" = true
"Example.com" = true
"example.com." = false

[settings.logs]
retention = "30x"
"#,
        ),
    ]);
    insta::assert_snapshot!(out);
}

#[test]
fn diagnostic_template_rules() {
    let out = errors(&[
        ("ndac.toml", ROOT),
        (
            "templates/base.toml",
            "id = \"abc\"\n[denylist]\nmode = \"merge\"\n",
        ),
        (
            "profiles/p.toml",
            "extends = [\"bse\"]\n[privacy]\nnatives = [\"$inherit\"]\n",
        ),
        ("profiles/q.toml", "[privacy]\nnatives = [\"$inherit\"]\n"),
    ]);
    insta::assert_snapshot!(out);
}

#[test]
fn diagnostic_cycle() {
    let out = errors(&[
        ("ndac.toml", ROOT),
        ("templates/a.toml", "extends = [\"b\"]\n"),
        ("templates/b.toml", "extends = [\"a\"]\n"),
        ("profiles/p.toml", "extends = [\"a\"]\n"),
    ]);
    insta::assert_snapshot!(out);
}

#[test]
fn diagnostic_syntax_and_types() {
    let out = errors(&[
        ("ndac.toml", ROOT),
        ("profiles/p.toml", "[security]\nnrd = \"yes\"\n"),
        ("profiles/q.toml", "[security\n"),
    ]);
    insta::assert_snapshot!(out);
}

#[test]
fn unsafe_includes_are_rejected() {
    let out = errors(&[(
        "ndac.toml",
        "schema = 1\ninclude = [\"../outside/*.toml\", \"/etc/*.toml\"]\n",
    )]);
    insta::assert_snapshot!(out);
}

#[cfg(unix)]
#[test]
fn symlink_escape_is_rejected() {
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("evil.toml"), "name = \"x\"\n").unwrap();
    let (dir, root) = workspace(&[("ndac.toml", ROOT)]);
    std::fs::create_dir_all(dir.path().join("profiles")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("evil.toml"),
        dir.path().join("profiles/evil.toml"),
    )
    .unwrap();
    let err = Workspace::load(&root).unwrap_err();
    // Checked by variant: rendered text wraps long temp paths (macOS).
    assert!(
        err.errors
            .iter()
            .any(|e| matches!(e, ConfigError::Escape { .. })),
        "{}",
        render(&err)
    );
}

#[test]
fn duplicate_profile_stems_are_rejected() {
    let out = errors(&[
        (
            "ndac.toml",
            "schema = 1\ninclude = [\"profiles/**/*.toml\"]\n",
        ),
        ("profiles/a/casa.toml", ""),
        ("profiles/b/casa.toml", ""),
    ]);
    assert!(out.contains("duplicate profile `casa`"), "{out}");
}

#[cfg(not(feature = "unstable-endpoints"))]
#[test]
fn rewrites_need_the_feature() {
    let out = errors(&[
        ("ndac.toml", ROOT),
        (
            "profiles/casa.toml",
            "[rewrites.items]\n\"a.lan\" = \"10.0.0.1\"\n",
        ),
    ]);
    assert!(out.contains("unknown key `rewrites`"), "{out}");
}

#[cfg(feature = "unstable-endpoints")]
#[test]
fn rewrites_merge_and_validate() {
    let (_dir, root) = workspace(&[
        ("ndac.toml", ROOT),
        (
            "templates/lan.toml",
            "[rewrites.items]\n\"Router.LAN\" = \"192.168.1.1\"\n\"nas.lan\" = \"192.168.1.2\"\n",
        ),
        (
            "profiles/casa.toml",
            "extends = [\"lan\"]\n\n[rewrites]\nmode = \"merge\"\n\n[rewrites.items]\n\"nas.lan\" = \"$remove\"\n\"tv.lan\" = \"tv.example.com\"\n",
        ),
    ]);
    let p = Workspace::load(&root).unwrap().resolve("casa").unwrap();
    let got: Vec<(&str, &str)> = p
        .desired
        .rewrites
        .as_deref()
        .unwrap()
        .iter()
        .map(|r| (r.name.as_str(), r.content.as_str()))
        .collect();
    assert_eq!(
        got,
        [("router.lan", "192.168.1.1"), ("tv.lan", "tv.example.com")]
    );
    assert_eq!(p.list_modes.get("rewrites"), Some(&ListMode::Merge));

    let out = errors(&[
        ("ndac.toml", ROOT),
        ("templates/t.toml", "[rewrites]\nmode = \"merge\"\n"),
        (
            "profiles/casa.toml",
            "[rewrites.items]\n\"a.lan\" = \"not an answer\"\n\"b.lan\" = \"$remove\"\n\"bad name\" = \"10.0.0.1\"\n",
        ),
    ]);
    assert!(
        out.contains("`rewrites.mode` is not allowed in a template"),
        "{out}"
    );
    assert!(out.contains("invalid rewrite answer"), "{out}");
    assert!(out.contains("`$remove` without `extends`"), "{out}");
    assert!(out.contains("invalid domain \"bad name\""), "{out}");
}
