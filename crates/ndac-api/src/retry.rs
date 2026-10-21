//! Retry policy: exponential backoff with full jitter and `Retry-After`.
//!
//! NextDNS does not document rate limits, so the client backs off on `429`,
//! `5xx` and transport errors and lets the server dictate the delay when it
//! sends `Retry-After`.

use std::time::{Duration, SystemTime};

use reqwest::header::HeaderValue;

/// How failed requests are retried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Retries after the first attempt. `0` disables retrying.
    pub max_retries: u32,
    /// Backoff ceiling for the first retry; doubles on each retry.
    pub base_delay: Duration,
    /// Upper bound for a single backoff delay, including `Retry-After`.
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 5,
            base_delay: Duration::from_millis(250),
            max_delay: Duration::from_secs(30),
        }
    }
}

impl RetryPolicy {
    /// Delay before retry number `retry` (0-based): uniformly random in
    /// `[0, min(max_delay, base_delay * 2^retry)]`.
    #[must_use]
    pub fn backoff(&self, retry: u32) -> Duration {
        let ceiling = self
            .base_delay
            .checked_mul(2u32.saturating_pow(retry))
            .unwrap_or(self.max_delay)
            .min(self.max_delay);
        let nanos = u64::try_from(ceiling.as_nanos()).unwrap_or(u64::MAX);
        Duration::from_nanos(fastrand::u64(0..=nanos))
    }

    /// Delay for a retry, preferring the server's `Retry-After`.
    pub(crate) fn delay(&self, retry: u32, retry_after: Option<Duration>) -> Duration {
        retry_after.map_or_else(|| self.backoff(retry), |d| d.min(self.max_delay))
    }
}

/// Parses `Retry-After` as delay-seconds or an HTTP date.
pub(crate) fn parse_retry_after(value: &HeaderValue, now: SystemTime) -> Option<Duration> {
    let s = value.to_str().ok()?.trim();
    if let Ok(secs) = s.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let at = httpdate::parse_http_date(s).ok()?;
    Some(at.duration_since(now).unwrap_or(Duration::ZERO))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_stays_within_full_jitter_bounds() {
        let p = RetryPolicy {
            max_retries: 5,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(1),
        };
        for retry in 0..40 {
            let ceiling = Duration::from_millis(100)
                .saturating_mul(2u32.saturating_pow(retry))
                .min(Duration::from_secs(1));
            for _ in 0..50 {
                assert!(p.backoff(retry) <= ceiling);
            }
        }
    }

    #[test]
    fn retry_after_wins_but_is_capped() {
        let p = RetryPolicy::default();
        assert_eq!(
            p.delay(0, Some(Duration::from_secs(3))),
            Duration::from_secs(3)
        );
        assert_eq!(p.delay(0, Some(Duration::from_secs(3600))), p.max_delay);
    }

    #[test]
    fn parses_delay_seconds() {
        let v = HeaderValue::from_static(" 7 ");
        assert_eq!(
            parse_retry_after(&v, SystemTime::now()),
            Some(Duration::from_secs(7))
        );
    }

    #[test]
    fn parses_http_date() {
        // 784_111_777 is Sun, 06 Nov 1994 08:49:37 GMT (RFC 9110 example).
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(784_111_777);
        let v = HeaderValue::from_static("Sun, 06 Nov 1994 08:49:47 GMT");
        assert_eq!(parse_retry_after(&v, now), Some(Duration::from_secs(10)));
        let later = now + Duration::from_secs(60);
        assert_eq!(parse_retry_after(&v, later), Some(Duration::ZERO));
    }

    #[test]
    fn ignores_garbage() {
        let v = HeaderValue::from_static("soon");
        assert_eq!(parse_retry_after(&v, SystemTime::now()), None);
    }
}
