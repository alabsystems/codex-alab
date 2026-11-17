// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Retry deadlines retain receipt-time sleep and fit the timer's millisecond rounding.

use super::Duration;
use super::Instant;
use super::RETRY_AFTER;
use super::SystemTime;
use http::HeaderMap;

/// Select repeated advice from one receipt; each field remains a whole value.
/// Inactive consumers retain upstream's exact first-field behavior.
pub(super) fn from_headers(headers: &HeaderMap) -> Option<super::RetryAfter> {
    if !codex_alab_core::active() {
        return super::RetryAfter::from_header(headers.get(RETRY_AFTER)?.to_str().ok()?);
    }
    // Match from_header's wall-before-monotonic ordering. Every candidate shares
    // this receipt, and only the selected deadline captures the wake observer.
    let now = SystemTime::now();
    let received_at = Instant::now();
    let deadline = headers
        .get_all(RETRY_AFTER)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| header_delay(value, now))
        .filter_map(|delay| received_at.checked_add(delay))
        .filter(timer_safe)
        .max()?;
    Some(super::RetryAfter(capture(deadline)))
}

fn header_delay(value: &str, now: SystemTime) -> Option<Duration> {
    let value = value.trim();
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        Some(Duration::from_secs(value.parse().ok()?))
    } else {
        Some(
            httpdate::parse_http_date(value)
                .ok()?
                .duration_since(now)
                .unwrap_or_default(),
        )
    }
}

pub(super) type Deadline = codex_alab_core::wake::Deadline;

pub(super) fn capture(deadline: Instant) -> Deadline {
    let active = codex_alab_core::active();
    let reading = active.then(codex_alab_core::wake::read).flatten();
    capture_with(deadline, active, reading)
}

fn capture_with(
    deadline: Instant,
    active: bool,
    reading: Option<codex_alab_core::wake::Reading>,
) -> Deadline {
    if active {
        Deadline::with_reading(deadline.into_std(), reading)
    } else {
        Deadline::untracked(deadline.into_std())
    }
}

pub(super) fn deadline(deadline: Deadline) -> Instant {
    Instant::from_std(deadline.current())
}

impl super::RetryAfter {
    /// Retain the original receipt deadline without normalizing or recapturing its origin.
    #[must_use]
    pub fn sleep_deadline(self) -> Deadline {
        self.0
    }

    /// Select the provider wait and any fresh local minimum from one accepted reading.
    #[must_use]
    pub fn arm_sleep_wait(self, minimum: Option<Instant>) -> codex_alab_core::wake::ArmedWait {
        codex_alab_core::wake::ArmedWait::from_deadline(self.0, minimum)
    }
}

#[cfg(any(test, debug_assertions))]
impl super::RetryAfter {
    /// Select from a supplied sleep clock independently of this host's platform support.
    pub fn arm_sleep_wait_with_clock(
        self,
        minimum: Option<Instant>,
        read: impl FnOnce() -> Option<codex_alab_core::wake::Reading>,
    ) -> codex_alab_core::wake::ArmedWait {
        codex_alab_core::wake::ArmedWait::from_deadline_with_clock(self.0, minimum, true, read)
    }

    /// Capture active advice with a supplied receipt reading, including a rejected sample.
    pub fn from_delay_with_sleep_reading(
        delay: Duration,
        reading: Option<codex_alab_core::wake::Reading>,
    ) -> Option<Self> {
        Instant::now()
            .checked_add(delay)
            .filter(timer_safe)
            .map(|at| Self(capture_with(at, true, reading)))
    }
}

pub(super) fn timer_safe(deadline: &Instant) -> bool {
    // Tokio rounds a deadline up by 999,999 ns before converting it to ticks.
    deadline.checked_add(Duration::from_millis(1)).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retry_after::RetryAfter;
    use std::future::Future;
    use std::task::Poll;

    #[tokio::test(start_paused = true)]
    async fn selected_advice_and_its_copies_keep_the_receipt_origin() {
        use codex_alab_core::wake::Reading;
        let start = Instant::now();
        let base = Duration::from_secs(5000);
        let reading = |asleep| Some(Reading { asleep, last: None });
        let original = start + Duration::from_secs(60);
        let advice = RetryAfter(capture_with(original, true, reading(base)));
        let copied = advice;
        let wait = advice.arm_sleep_wait_with_clock(Some(start + Duration::from_secs(10)), || {
            reading(base + Duration::from_secs(120))
        });
        assert_eq!(wait.deadline(), start + Duration::from_secs(10));
        assert_eq!(advice, copied);
        assert_eq!(
            copied
                .sleep_deadline()
                .at_reading(reading(base), start.into_std()),
            original.into_std(),
        );
        assert_eq!(
            copied
                .sleep_deadline()
                .at_reading(reading(base + Duration::from_secs(120)), start.into_std()),
            (original - Duration::from_secs(120)).into_std(),
        );
    }

    /// Advice can spend a host sleep in a synchronous post-request hook before a
    /// retry caller sees it. Its captured origin survives copying and repeated reads.
    #[tokio::test(start_paused = true)]
    async fn received_advice_counts_sleep_before_the_error_handler_reads_it() {
        use codex_alab_core::wake::Reading;
        let start = Instant::now();
        let base = Duration::from_secs(5000);
        let reading = |asleep| Some(Reading { asleep, last: None });
        let original = start + Duration::from_secs(3600);
        let advice = RetryAfter(capture_with(original, true, reading(base)));
        let copied = advice;
        let now = start + Duration::from_secs(300);
        assert_eq!(
            advice.0.at_reading(reading(base), now.into_std()),
            original.into_std(),
            "no sleep leaves the provider's full wait intact"
        );
        let after = advice
            .0
            .at_reading(reading(base + Duration::from_secs(7200)), now.into_std());
        assert!(
            after <= now.into_std(),
            "the provider wait expired during sleep"
        );
        assert_eq!(
            copied
                .0
                .at_reading(reading(base + Duration::from_secs(7200)), now.into_std()),
            after,
            "copies and repeated reads keep one original receipt baseline"
        );
    }

    /// An inactive upstream consumer and an active receipt after an old sleep both
    /// retain their newly created full deadline.
    #[tokio::test(start_paused = true)]
    async fn inactive_consumers_and_sleep_before_receipt_keep_the_full_wait() {
        use codex_alab_core::wake::Reading;
        let now = Instant::now();
        let before = Duration::from_secs(5000);
        let after = before + Duration::from_secs(7200);
        let reading = |asleep| Some(Reading { asleep, last: None });
        let original = now + Duration::from_secs(3600);
        for (active, receipt) in [(false, before), (true, after)] {
            let advice = RetryAfter(capture_with(original, active, reading(receipt)));
            assert_eq!(
                advice.0.at_reading(reading(after), now.into_std()),
                original.into_std(),
                "active={active}: only sleep after an active receipt counts"
            );
        }
    }

    fn duration_from_nanos(nanos: u128) -> Duration {
        Duration::new(
            u64::try_from(nanos / 1_000_000_000).expect("duration seconds fit"),
            u32::try_from(nanos % 1_000_000_000).expect("subsecond nanoseconds fit"),
        )
    }

    fn maximum_delay(now: Instant) -> Duration {
        let mut lower = 0;
        let mut upper = Duration::MAX.as_nanos();
        while lower < upper {
            let candidate = lower + (upper - lower).div_ceil(2);
            if now.checked_add(duration_from_nanos(candidate)).is_some() {
                lower = candidate;
            } else {
                upper = candidate - 1;
            }
        }
        duration_from_nanos(lower)
    }

    #[tokio::test(start_paused = true)]
    async fn extreme_delay_is_rejected_but_a_roundable_deadline_can_be_polled() {
        let maximum = maximum_delay(Instant::now());
        assert_eq!(RetryAfter::from_delay(maximum), None);

        let safe = RetryAfter::from_delay(maximum - Duration::from_millis(1))
            .expect("deadline with rounding headroom");
        let sleep = tokio::time::sleep_until(safe.deadline());
        tokio::pin!(sleep);
        std::future::poll_fn(|cx| {
            assert!(sleep.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn extreme_integer_header_reserves_rounding_headroom() {
        let maximum = maximum_delay(Instant::now());
        let shift = (maximum.subsec_nanos() + 1_000_000_000 - 500_000) % 1_000_000_000;
        tokio::time::advance(Duration::from_nanos(u64::from(shift))).await;

        let seconds = maximum_delay(Instant::now()).as_secs();
        let unsafe_deadline = Instant::now()
            .checked_add(Duration::from_secs(seconds))
            .expect("representable deadline");
        assert!(
            unsafe_deadline
                .checked_add(Duration::from_nanos(999_999))
                .is_none()
        );
        assert_eq!(RetryAfter::from_header(&seconds.to_string()), None);
    }
}
