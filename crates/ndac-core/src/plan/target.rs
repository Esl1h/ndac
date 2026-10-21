//! `--target` filters: `casa`, `casa.denylist`, `*.privacy.blocklists`.

use std::str::FromStr;

use globset::{Glob, GlobMatcher};

use crate::paths::covers;

/// A profile glob and an optional path prefix.
#[derive(Debug, Clone)]
pub struct Target {
    profile: GlobMatcher,
    path: Option<String>,
    raw: String,
}

impl Target {
    /// True if the target selects `profile` at all.
    #[must_use]
    pub fn matches_profile(&self, profile: &str) -> bool {
        self.profile.is_match(profile)
    }

    /// True if the target selects `path` within `profile`. A list target
    /// (`casa.denylist`) selects every item of the list.
    #[must_use]
    pub fn matches(&self, profile: &str, path: &str) -> bool {
        self.matches_profile(profile) && self.path.as_deref().is_none_or(|p| covers(p, path))
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.raw)
    }
}

impl FromStr for Target {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let (profile, path) = match s.split_once('.') {
            Some((p, rest)) => (p, Some(rest.to_owned())),
            None => (s, None),
        };
        if profile.is_empty() {
            return Err(format!("invalid target `{s}`: missing profile"));
        }
        if let Some(p) = &path
            && !crate::paths::is_known(p)
        {
            return Err(format!("invalid target `{s}`: unknown path `{p}`"));
        }
        let glob = Glob::new(profile).map_err(|e| format!("invalid target `{s}`: {e}"))?;
        Ok(Self {
            profile: glob.compile_matcher(),
            path,
            raw: s.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_and_path_targets() {
        let t: Target = "casa".parse().unwrap();
        assert!(t.matches("casa", "denylist"));
        assert!(!t.matches("trabalho", "denylist"));

        let t: Target = "casa.denylist".parse().unwrap();
        assert!(t.matches("casa", "denylist"));
        assert!(!t.matches("casa", "allowlist"));

        let t: Target = "*.privacy.blocklists".parse().unwrap();
        assert!(t.matches("x", "privacy.blocklists"));
        assert!(!t.matches("x", "privacy.natives"));

        let t: Target = "casa.settings".parse().unwrap();
        assert!(t.matches("casa", "settings.logs.retention"));

        assert!("casa.nope".parse::<Target>().is_err());
        assert!(".denylist".parse::<Target>().is_err());
    }
}
