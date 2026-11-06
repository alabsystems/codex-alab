//! Server retry advice captured once so passing an error between callers does not restart it.

use http::HeaderMap;
use http::header::RETRY_AFTER;
use std::time::Duration;
use std::time::SystemTime;
use tokio::time::Instant;

/// The earliest time a server advised making a follow-up request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryAfter(alab_retry_after::Deadline); // ALAB(wake): preserve the sleep count from receipt, including delayed error hooks

impl RetryAfter {
    /// Captures an interval measured from receipt of the server's advice.
    pub fn from_delay(delay: Duration) -> Option<Self> {
        Instant::now().checked_add(delay).filter(alab_retry_after::timer_safe).map(alab_retry_after::capture).map(Self) // ALAB(capacity): reserve rounding headroom and retain the receipt's sleep count
    }

    /// Reads either nonnegative delay seconds or an HTTP date from `Retry-After`.
    pub fn from_headers(headers: &HeaderMap) -> Option<Self> {
        Self::from_header(headers.get(RETRY_AFTER)?.to_str().ok()?)
    }

    /// Reads a `Retry-After` value, including values carried inside a streamed error.
    pub fn from_header(value: &str) -> Option<Self> {
        // Sample wall time first so a scheduling pause cannot shorten the server's deadline.
        let now = SystemTime::now();
        let received_at = Instant::now();
        let value = value.trim();
        let delay = if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
            Duration::from_secs(value.parse().ok()?)
        } else {
            httpdate::parse_http_date(value)
                .ok()?
                .duration_since(now)
                .unwrap_or_default()
        };
        received_at.checked_add(delay).filter(alab_retry_after::timer_safe).map(alab_retry_after::capture).map(Self) // ALAB(capacity): streamed and HTTP advice share timer-safe, sleep-aware admission
    }

    /// Returns the original real-time deadline, including time the host slept.
    pub fn deadline(self) -> Instant {
        alab_retry_after::deadline(self.0) // ALAB(wake): a delayed caller cannot restart or postpone advice across a sleep
    }

    /// Returns the remaining delay, or zero after the deadline has passed.
    pub fn remaining_delay(self) -> Duration {
        self.deadline().saturating_duration_since(Instant::now()) // ALAB(wake): use the same real-time deadline as retry callers
    }
}

#[cfg(test)]
#[path = "retry_after_tests.rs"]
mod tests;

#[path = "../../alab-core/glue/retry_after.rs"]
mod alab_retry_after; // ALAB(capacity): retry advice must fit the timer's deadline rounding
