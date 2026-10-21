//! Drift reports: the part of a plan changed outside ndac since the last
//! apply, as GitHub annotations or Prometheus textfile metrics.

use std::{collections::BTreeSet, fmt::Write as _};

use super::{Origin, Plan};

/// Keeps only drift and conflict changes, and drops profiles to create and
/// removed profiles: neither has a baseline to drift from.
#[must_use]
pub fn drift_only(mut plan: Plan) -> Plan {
    plan.removed.clear();
    plan.profiles.retain(|p| !p.create);
    for p in &mut plan.profiles {
        p.changes.retain(|c| c.origin() != Origin::Pending);
    }
    plan
}

/// Sections (field or list paths) with drift, per profile, as
/// `(profile, section, origin)`. Conflict wins over drift in a section.
#[must_use]
pub fn drift_sections(plan: &Plan) -> Vec<(&str, &str, Origin)> {
    let mut out: Vec<(&str, &str, Origin)> = Vec::new();
    for p in &plan.profiles {
        for c in p.changes.iter().filter(|c| c.origin() != Origin::Pending) {
            match out
                .iter_mut()
                .find(|(k, s, _)| *k == p.key && *s == c.path())
            {
                Some(entry) => entry.2 = entry.2.max(c.origin()),
                None => out.push((&p.key, c.path(), c.origin())),
            }
        }
    }
    out
}

/// Escapes a workflow command property (`file`, `title`).
fn gh_property(s: &str) -> String {
    gh_data(s).replace(':', "%3A").replace(',', "%2C")
}

/// Escapes a workflow command message.
fn gh_data(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// One `::warning` annotation per drifted section. `locate` maps a profile
/// key and section to the file to annotate and, when known, its line.
#[must_use]
pub fn render_github(
    plan: &Plan,
    locate: impl Fn(&str, &str) -> (String, Option<usize>),
) -> String {
    let mut out = String::new();
    for (key, section, origin) in drift_sections(plan) {
        let (file, line) = locate(key, section);
        let mut props = format!("file={}", gh_property(&file));
        if let Some(l) = line {
            let _ = write!(props, ",line={l}");
        }
        let _ = write!(props, ",title={}", gh_property("NextDNS drift"));
        let what = match origin {
            Origin::Conflict => "changed both in NextDNS and in configuration",
            _ => "changed in NextDNS",
        };
        let msg = format!(
            "profile \"{key}\": {section} {what} since the last apply; run `ndac reconcile`"
        );
        let _ = writeln!(out, "::warning {props}::{}", gh_data(&msg));
    }
    out
}

/// Escapes a Prometheus label value.
fn prom_label(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// `node_exporter` textfile metrics. Only drifted sections get a series;
/// `ndac_drift_last_run_timestamp_seconds` tells a clean run from a stale
/// one.
#[must_use]
pub fn render_prom(plan: &Plan, now_secs: u64) -> String {
    let mut out = String::from(
        "# HELP ndac_drift Section changed outside ndac since the last apply (1).\n\
         # TYPE ndac_drift gauge\n",
    );
    let mut seen = BTreeSet::new();
    for (key, section, _) in drift_sections(plan) {
        if seen.insert((key, section)) {
            let _ = writeln!(
                out,
                "ndac_drift{{profile=\"{}\",section=\"{}\"}} 1",
                prom_label(key),
                prom_label(section)
            );
        }
    }
    let _ = write!(
        out,
        "# HELP ndac_drift_last_run_timestamp_seconds When the drift check last completed.\n\
         # TYPE ndac_drift_last_run_timestamp_seconds gauge\n\
         ndac_drift_last_run_timestamp_seconds {now_secs}\n"
    );
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::plan::{Change, ProfilePlan, RemovedProfile};

    fn plan() -> Plan {
        let set = |path: &str, origin| Change::SetField {
            path: path.into(),
            from: Some(json!(false)),
            to: json!(true),
            origin,
        };
        Plan {
            profiles: vec![
                ProfilePlan {
                    key: "casa".into(),
                    id: Some("abc123".into()),
                    create: false,
                    list_modes: std::collections::BTreeMap::default(),
                    changes: vec![
                        set("settings.web3", Origin::Pending),
                        Change::RemoveItem {
                            list: "denylist".into(),
                            id: "a.example".into(),
                            origin: Origin::Drift,
                        },
                        Change::AddItem {
                            list: "denylist".into(),
                            id: "b.example".into(),
                            active: Some(true),
                            origin: Origin::Conflict,
                        },
                        set("privacy.disguisedTrackers", Origin::Drift),
                    ],
                },
                ProfilePlan {
                    key: "novo".into(),
                    id: None,
                    create: true,
                    list_modes: std::collections::BTreeMap::default(),
                    changes: vec![set("settings.web3", Origin::Pending)],
                },
            ],
            removed: vec![RemovedProfile {
                key: "old".into(),
                id: None,
            }],
        }
    }

    #[test]
    fn drift_only_keeps_drift_and_conflict() {
        let d = drift_only(plan());
        assert_eq!(d.removed, Vec::new());
        assert_eq!(d.profiles.len(), 1);
        let paths: Vec<&str> = d.profiles[0].changes.iter().map(Change::path).collect();
        assert_eq!(paths, ["denylist", "denylist", "privacy.disguisedTrackers"]);
        assert_eq!(
            drift_sections(&d),
            [
                ("casa", "denylist", Origin::Conflict),
                ("casa", "privacy.disguisedTrackers", Origin::Drift)
            ]
        );
    }

    #[test]
    fn github_annotations() {
        let d = drift_only(plan());
        let text = render_github(&d, |k, s| {
            (format!("profiles/{k}.toml"), (s == "denylist").then_some(7))
        });
        assert_eq!(
            text,
            "::warning file=profiles/casa.toml,line=7,title=NextDNS drift::profile \"casa\": denylist changed both in NextDNS and in configuration since the last apply; run `ndac reconcile`\n\
             ::warning file=profiles/casa.toml,title=NextDNS drift::profile \"casa\": privacy.disguisedTrackers changed in NextDNS since the last apply; run `ndac reconcile`\n"
        );
        assert_eq!(gh_property("a,b:c%\n"), "a%2Cb%3Ac%25%0A");
    }

    #[test]
    fn prom_metrics() {
        let text = render_prom(&drift_only(plan()), 1_792_600_000);
        assert_eq!(
            text,
            "# HELP ndac_drift Section changed outside ndac since the last apply (1).\n\
             # TYPE ndac_drift gauge\n\
             ndac_drift{profile=\"casa\",section=\"denylist\"} 1\n\
             ndac_drift{profile=\"casa\",section=\"privacy.disguisedTrackers\"} 1\n\
             # HELP ndac_drift_last_run_timestamp_seconds When the drift check last completed.\n\
             # TYPE ndac_drift_last_run_timestamp_seconds gauge\n\
             ndac_drift_last_run_timestamp_seconds 1792600000\n"
        );
        assert_eq!(prom_label("a\"b\\c"), "a\\\"b\\\\c");
        let empty = render_prom(&Plan::default(), 1);
        assert!(!empty.contains("ndac_drift{"));
    }
}
