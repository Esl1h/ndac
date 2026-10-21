//! Plan output: text for terminals, versioned JSON for machines, Markdown
//! for pull request comments.

use std::fmt::Write as _;

use serde::Serialize;
use serde_json::Value;

use super::{Change, Origin, Plan, ProfilePlan, RemovedProfile, Summary};
use crate::config::ListMode;

/// Version of the `plan --format json` document.
pub const PLAN_FORMAT_VERSION: u32 = 1;

/// Maximum size of Markdown output; GitHub rejects comments over 65536
/// characters.
pub const MARKDOWN_LIMIT: usize = 60_000;

struct Style {
    color: bool,
}

impl Style {
    fn paint(&self, code: &str, s: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_owned()
        }
    }

    fn sym(&self, c: char) -> String {
        let code = match c {
            '+' => "32",
            '-' => "31",
            '~' => "33",
            '!' => "35",
            _ => "0",
        };
        self.paint(code, &c.to_string())
    }
}

fn value(v: Option<&Value>) -> String {
    match v {
        None => "(unset)".into(),
        Some(v) => v.to_string(),
    }
}

/// `7776000` as `90d`, for retention-like fields.
fn human_secs(v: &Value) -> Option<String> {
    let mut s = v.as_u64()?;
    let mut out = String::new();
    for (unit, size) in [
        ("y", 31_536_000),
        ("d", 86_400),
        ("h", 3_600),
        ("m", 60),
        ("s", 1),
    ] {
        if s >= size {
            let _ = write!(out, "{}{unit}", s / size);
            s %= size;
        }
    }
    Some(if out.is_empty() { "0s".into() } else { out })
}

fn tag(o: Origin) -> &'static str {
    match o {
        Origin::Pending => "",
        Origin::Drift => "[drift]",
        Origin::Conflict => "[conflict]",
    }
}

fn header(p: &ProfilePlan) -> String {
    match (&p.id, p.create) {
        (_, true) => format!("profile \"{}\" (to be created)", p.key),
        (Some(id), false) => format!("profile \"{}\" ({id})", p.key),
        (None, false) => format!("profile \"{}\"", p.key),
    }
}

/// Groups changes by path, keeping their order.
fn sections(p: &ProfilePlan) -> Vec<(&str, Vec<&Change>)> {
    let mut out: Vec<(&str, Vec<&Change>)> = Vec::new();
    for c in &p.changes {
        match out.last_mut() {
            Some((path, v)) if *path == c.path() => v.push(c),
            _ => out.push((c.path(), vec![c])),
        }
    }
    out
}

fn profile_text(p: &ProfilePlan, st: &Style, out: &mut String) {
    if p.is_empty() {
        let _ = writeln!(out, "{}: no changes", header(p));
        return;
    }
    let _ = writeln!(out, "{}", header(p));
    for (path, changes) in sections(p) {
        let worst = changes
            .iter()
            .map(|c| c.origin())
            .max()
            .unwrap_or(Origin::Pending);
        let sym = if worst == Origin::Pending { '~' } else { '!' };
        if let [Change::SetField { from, to, .. }] = changes.as_slice() {
            let mut line = format!(
                "  {} {path}: {} → {}",
                st.sym(sym),
                value(from.as_ref()),
                value(Some(to))
            );
            if path.ends_with("retention")
                && let (Some(a), Some(b)) = (from.as_ref().and_then(human_secs), human_secs(to))
            {
                let _ = write!(line, " ({a} → {b})");
            }
            let _ = writeln!(out, "{}{}", pad(&line, worst), st.paint("33", tag(worst)));
            continue;
        }
        let mode = match p.list_modes.get(path) {
            Some(ListMode::Merge) => " (merge)",
            _ => "",
        };
        let line = format!("  {} {path}{mode}", st.sym(sym));
        let _ = writeln!(out, "{}{}", pad(&line, worst), st.paint("33", tag(worst)));
        for c in changes {
            let item = match c {
                Change::AddItem { id, active, .. } => format!(
                    "      {} {id}{}",
                    st.sym('+'),
                    if *active == Some(false) {
                        " (inactive)"
                    } else {
                        ""
                    }
                ),
                Change::RemoveItem { id, .. } => format!(
                    "      {} {id}   (live only; authoritative → will be removed)",
                    st.sym('-')
                ),
                Change::SetItemActive { id, from, to, .. } => format!(
                    "      {} {id}: {} → {}",
                    st.sym('~'),
                    if *from { "active" } else { "inactive" },
                    if *to { "active" } else { "inactive" }
                ),
                Change::SetField { .. } => continue,
            };
            let _ = writeln!(out, "{item}");
        }
    }
}

/// Pads a section line so drift tags line up.
fn pad(line: &str, origin: Origin) -> String {
    if origin == Origin::Pending {
        return line.to_owned();
    }
    let visible = strip_ansi(line).chars().count();
    format!("{line}{} ", " ".repeat(48usize.saturating_sub(visible)))
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut in_esc = false;
    for c in s.chars() {
        match (in_esc, c) {
            (false, '\x1b') => in_esc = true,
            (true, 'm') => in_esc = false,
            (false, c) => out.push(c),
            _ => {}
        }
    }
    out
}

fn removed_text(r: &RemovedProfile, st: &Style, out: &mut String) {
    let id =
        r.id.as_deref()
            .map(|i| format!(" ({i})"))
            .unwrap_or_default();
    let _ = writeln!(
        out,
        "{} profile \"{}\"{id}: configuration file removed; `ndac apply --allow-destroy` deletes it",
        st.sym('-'),
        r.key
    );
}

/// One-line totals.
#[must_use]
pub fn summary_line(s: Summary) -> String {
    if s.profiles == 0 {
        return "No changes. Live state matches configuration.".into();
    }
    let mut line = format!(
        "Plan: {} to add, {} to change, {} to remove across {} profile{}.",
        s.add,
        s.change,
        s.remove,
        s.profiles,
        if s.profiles == 1 { "" } else { "s" }
    );
    if s.drift_sections > 0 {
        let _ = write!(
            line,
            " Drift in {} section{}.",
            s.drift_sections,
            if s.drift_sections == 1 { "" } else { "s" }
        );
    }
    line
}

/// Human-readable plan.
#[must_use]
pub fn render_text(plan: &Plan, color: bool) -> String {
    let st = Style { color };
    let mut out = String::new();
    for p in &plan.profiles {
        profile_text(p, &st, &mut out);
        out.push('\n');
    }
    for r in &plan.removed {
        removed_text(r, &st, &mut out);
        out.push('\n');
    }
    out.push_str(&summary_line(plan.summary()));
    out.push('\n');
    out
}

#[derive(Serialize)]
struct JsonPlan<'a> {
    format_version: u32,
    summary: Summary,
    profiles: &'a [ProfilePlan],
    removed: &'a [RemovedProfile],
}

/// Machine-readable plan, `format_version` 1.
#[must_use]
pub fn render_json(plan: &Plan) -> String {
    let doc = JsonPlan {
        format_version: PLAN_FORMAT_VERSION,
        summary: plan.summary(),
        profiles: &plan.profiles,
        removed: &plan.removed,
    };
    let mut s = serde_json::to_string_pretty(&doc).unwrap_or_default();
    s.push('\n');
    s
}

/// Markdown for pull request comments: one collapsible block per profile,
/// truncated to [`MARKDOWN_LIMIT`] characters.
#[must_use]
pub fn render_markdown(plan: &Plan) -> String {
    let st = Style { color: false };
    let mut head = String::from("## ndac plan\n\n");
    let _ = writeln!(head, "**{}**\n", summary_line(plan.summary()));
    let mut blocks = Vec::new();
    for p in &plan.profiles {
        let mut body = String::new();
        if p.is_empty() {
            blocks.push(format!("- `{}`: no changes\n", p.key));
            continue;
        }
        if p.create {
            body.push_str("+ profile will be created\n");
        }
        for c in &p.changes {
            let t = tag(c.origin());
            let t = if t.is_empty() {
                String::new()
            } else {
                format!("  {t}")
            };
            let line = match c {
                Change::SetField { path, from, to, .. } => {
                    format!(
                        "! {path}: {} → {}{t}",
                        value(from.as_ref()),
                        value(Some(to))
                    )
                }
                Change::AddItem {
                    list, id, active, ..
                } => format!(
                    "+ {list}: {id}{}{t}",
                    if *active == Some(false) {
                        " (inactive)"
                    } else {
                        ""
                    }
                ),
                Change::RemoveItem { list, id, .. } => format!("- {list}: {id}{t}"),
                Change::SetItemActive {
                    list, id, from, to, ..
                } => {
                    format!("! {list}: {id} active {from} → {to}{t}")
                }
            };
            body.push_str(&line);
            body.push('\n');
        }
        let mut block = String::new();
        let _ = write!(
            block,
            "<details><summary><code>{}</code>{}: {} change(s)</summary>\n\n```diff\n{body}```\n\n</details>\n",
            p.key,
            p.id.as_deref()
                .map(|i| format!(" ({i})"))
                .unwrap_or_default(),
            p.changes.len() + usize::from(p.create)
        );
        blocks.push(block);
    }
    for r in &plan.removed {
        let mut line = String::new();
        removed_text(r, &st, &mut line);
        blocks.push(format!("\n{line}"));
    }

    let note = format!(
        "\n_Output truncated at {MARKDOWN_LIMIT} characters; run `ndac plan` for the full plan._\n"
    );
    let mut out = head;
    for b in blocks {
        if out.len() + b.len() + note.len() > MARKDOWN_LIMIT {
            out.push_str(&note);
            return out;
        }
        out.push_str(&b);
    }
    out
}
