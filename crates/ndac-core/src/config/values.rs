//! Parsing and normalization of individual values.

use super::types::DurationValue;

/// Parses a duration into seconds: an integer, or a string of
/// `<number><unit>` groups with units `s`, `m`, `h`, `d`, `w` and `y`
/// (365 days), e.g. `"90d"` or `"1h30m"`.
///
/// # Errors
///
/// A human-readable reason when the value is malformed or overflows.
pub fn duration_secs(value: &DurationValue) -> Result<u64, String> {
    let s = match value {
        DurationValue::Seconds(n) => return Ok(*n),
        DurationValue::Text(s) => s.trim(),
    };
    if s.is_empty() {
        return Err("empty duration".into());
    }
    if let Ok(n) = s.parse::<u64>() {
        return Ok(n);
    }
    let mut total: u64 = 0;
    let mut digits = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let unit: u64 = match c {
            's' => 1,
            'm' => 60,
            'h' => 3_600,
            'd' => 86_400,
            'w' => 604_800,
            'y' => 31_536_000,
            _ => return Err(format!("unknown unit `{c}` (use s, m, h, d, w or y)")),
        };
        let n: u64 = digits
            .parse()
            .map_err(|_| format!("missing number before `{c}`"))?;
        digits.clear();
        total = n
            .checked_mul(unit)
            .and_then(|v| total.checked_add(v))
            .ok_or("duration is too large")?;
    }
    if !digits.is_empty() {
        return Err(format!("missing unit after `{digits}`"));
    }
    Ok(total)
}

/// Normalizes a domain: lowercase, IDNA to ASCII (punycode), trailing dot
/// removed. Requires at least two labels.
///
/// # Errors
///
/// A human-readable reason when the input is not a valid domain.
pub fn normalize_domain(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    let no_dot = trimmed.strip_suffix('.').unwrap_or(trimmed);
    if no_dot.is_empty() {
        return Err("empty domain".into());
    }
    if no_dot.contains(['*', '/', ':', ' ', '@']) {
        return Err("only plain domain names are allowed (no wildcards, schemes, paths or ports); subdomains are covered automatically".into());
    }
    let ascii = idna::domain_to_ascii(no_dot).map_err(|_| "not a valid domain name".to_owned())?;
    if ascii.len() > 253 {
        return Err("domain is longer than 253 characters".into());
    }
    if !ascii.contains('.') {
        return Err("a domain needs at least two labels, e.g. `example.com`".into());
    }
    for label in ascii.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err("each label must have 1 to 63 characters".into());
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err(format!("label `{label}` starts or ends with `-`"));
        }
        if !label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(format!("label `{label}` has invalid characters"));
        }
    }
    Ok(ascii)
}

/// Closest candidate to `input`, for "did you mean" hints. Ignores case,
/// `_` and `-` so `parental_control` suggests `parentalControl`.
#[must_use]
pub fn suggest<'a>(input: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let squash = |s: &str| {
        s.chars()
            .filter(|c| *c != '_' && *c != '-')
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let target = squash(input);
    let max = (input.len() / 3).max(2);
    candidates
        .into_iter()
        .map(|c| {
            let d = if squash(c) == target {
                0
            } else {
                strsim::levenshtein(&target, &squash(c))
            };
            (d, c)
        })
        .filter(|(d, _)| *d <= max)
        .min_by_key(|(d, c)| (*d, *c))
        .map(|(_, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> DurationValue {
        DurationValue::Text(s.into())
    }

    #[test]
    fn durations() {
        assert_eq!(duration_secs(&DurationValue::Seconds(5)), Ok(5));
        assert_eq!(duration_secs(&text("30d")), Ok(2_592_000));
        assert_eq!(duration_secs(&text("90d")), Ok(7_776_000));
        assert_eq!(duration_secs(&text("1h30m")), Ok(5_400));
        assert_eq!(duration_secs(&text("2y")), Ok(63_072_000));
        assert_eq!(duration_secs(&text(" 15s ")), Ok(15));
        assert_eq!(duration_secs(&text("42")), Ok(42));
        assert!(duration_secs(&text("")).is_err());
        assert!(duration_secs(&text("10")).is_ok());
        assert!(
            duration_secs(&text("10x"))
                .unwrap_err()
                .contains("unknown unit")
        );
        assert!(
            duration_secs(&text("d"))
                .unwrap_err()
                .contains("missing number")
        );
        assert!(
            duration_secs(&text("1d2"))
                .unwrap_err()
                .contains("missing unit")
        );
        assert!(duration_secs(&text("99999999999999999999y")).is_err());
    }

    #[test]
    fn domains() {
        assert_eq!(normalize_domain("Example.COM."), Ok("example.com".into()));
        assert_eq!(
            normalize_domain("münchen.de"),
            Ok("xn--mnchen-3ya.de".into())
        );
        assert_eq!(normalize_domain(" a-b.example "), Ok("a-b.example".into()));
        for bad in [
            "",
            ".",
            "localhost",
            "*.example.com",
            "https://x.com",
            "a b.com",
            "-x.com",
            "x..com",
        ] {
            assert!(normalize_domain(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn suggestions() {
        let keys = ["parentalControl", "privacy", "security"];
        assert_eq!(suggest("parental_control", keys), Some("parentalControl"));
        assert_eq!(suggest("privcy", keys), Some("privacy"));
        assert_eq!(suggest("zzzzzz", keys), None);
    }
}
