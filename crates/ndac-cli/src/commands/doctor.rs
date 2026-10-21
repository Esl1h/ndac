//! `ndac doctor`.

use std::{path::Path, time::Duration};

use miette::Report;
use ndac_api::{ResolverTest, test_resolver};
use ndac_core::config::Workspace;

use super::{CmdResult, Failure, root_path, runtime, user_agent};
use crate::output::{Exit, Output};

const TIMEOUT: Duration = Duration::from_secs(10);

/// The profile ID for `name`: the `id` of a configured profile with that
/// key, else `name` itself. A missing or broken configuration is not an
/// error here; doctor works without one.
fn expected_id(config: Option<&Path>, name: &str) -> String {
    root_path(config)
        .ok()
        .and_then(|p| Workspace::load(&p).ok())
        .and_then(|ws| ws.profiles.get(name).and_then(|e| e.file.id.clone()))
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| name.to_owned())
}

/// Device names travel as `John--Router` in DNS-over-TLS hostnames.
fn same_device(seen: &str, expected: &str) -> bool {
    seen == expected || seen.replace("--", " ") == expected.replace("--", " ")
}

/// Checklist lines, and whether every check passed.
fn checklist(r: &ResolverTest, profile: Option<&str>, device: Option<&str>) -> (Vec<String>, bool) {
    let mut lines = Vec::new();
    let mut ok = true;
    let protocol = r.protocol.as_deref().unwrap_or("unknown");
    if r.status == "ok" {
        let server = r.server.as_deref().unwrap_or("unknown");
        lines.push(format!(
            "✓ queries reach NextDNS (protocol {protocol}, server {server})"
        ));
        if protocol.eq_ignore_ascii_case("udp") || protocol.eq_ignore_ascii_case("tcp") {
            lines.push(format!(
                "! protocol {protocol}: queries are not encrypted; use DoH or DoT"
            ));
        }
    } else {
        ok = false;
        lines.push(format!(
            "✗ queries do not reach NextDNS (status {:?})",
            r.status
        ));
    }

    let seen = r.profile.as_deref();
    match (profile, seen) {
        (Some(want), Some(got)) if got == want => lines.push(format!("✓ profile {got}")),
        (Some(_), Some(got)) if got.starts_with("fp") => lines.push(format!(
            "? profile shown as fingerprint {got}, not as an ID; cannot compare"
        )),
        (Some(want), Some(got)) => {
            ok = false;
            lines.push(format!("✗ profile {got}, expected {want}"));
        }
        (Some(want), None) => {
            ok = false;
            lines.push(format!("✗ no profile reported, expected {want}"));
        }
        (None, Some(got)) => lines.push(format!("  profile {got}")),
        (None, None) => {}
    }

    let name = r.client_name.as_deref();
    match (device, name) {
        (Some(want), Some(got)) if same_device(got, want) => {
            lines.push(format!("✓ device name {got:?}"));
        }
        (Some(want), Some(got)) => {
            ok = false;
            lines.push(format!("✗ device name {got:?}, expected {want:?}"));
        }
        (Some(want), None) => {
            ok = false;
            lines.push(format!("✗ no device name sent, expected {want:?}"));
        }
        (None, Some(got)) => lines.push(format!("  device name {got:?}")),
        (None, None) => {}
    }
    (lines, ok)
}

pub(crate) fn run(
    config: Option<&Path>,
    expect_profile: Option<&str>,
    expect_device: Option<&str>,
    url: &str,
) -> CmdResult {
    let profile = expect_profile.map(|p| expected_id(config, p));
    let result = runtime()?
        .block_on(test_resolver(url, &user_agent(), TIMEOUT))
        .map_err(Failure::api)?;
    let (lines, ok) = checklist(&result, profile.as_deref(), expect_device);
    let mut text = lines.join("\n");
    text.push('\n');
    Output::artifact(&text)
        .map_err(|e| Failure::Other(Report::msg(format!("cannot write output: {e}"))))?;
    Ok(if ok { Exit::Success } else { Exit::Changes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(json: &str) -> ResolverTest {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn checks() {
        let r = result(
            r#"{"status":"ok","protocol":"DOT","profile":"abc123","server":"s1","clientName":"John Router"}"#,
        );
        let (lines, ok) = checklist(&r, Some("abc123"), Some("John--Router"));
        assert!(ok, "{lines:?}");
        assert_eq!(lines.len(), 3);

        let (lines, ok) = checklist(&r, Some("def456"), None);
        assert!(!ok);
        assert_eq!(lines[1], "✗ profile abc123, expected def456");

        let fp = result(r#"{"status":"ok","protocol":"UDP","profile":"fp00"}"#);
        let (lines, ok) = checklist(&fp, Some("abc123"), None);
        assert!(ok, "{lines:?}");
        assert!(lines[1].starts_with("! protocol UDP"), "{lines:?}");
        assert!(
            lines[2].starts_with("? profile shown as fingerprint"),
            "{lines:?}"
        );

        let off = result(r#"{"status":"unconfigured"}"#);
        let (lines, ok) = checklist(&off, None, Some("x"));
        assert!(!ok);
        assert_eq!(lines.len(), 2);
    }
}
