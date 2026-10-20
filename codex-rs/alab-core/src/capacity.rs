// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Pure backoff for retrying the selected model while it is at capacity.
//! The caller owns error classification, cancellation and the source of jitter.

use std::time::Duration;

/// Capacity waits grow independently of the ordinary stream retry budget.
#[derive(Debug, Default)]
pub struct CapacityBackoff {
    attempts: u64,
}

impl CapacityBackoff {
    /// Select the next wait using caller-supplied jitter, then count the attempt.
    ///
    /// The upper bound grows through 1, 2, 4, 8, 16, 32 and 60 seconds, remaining
    /// at 60 seconds thereafter. Each wait lies within the inclusive interval
    /// from half that bound to the bound, so even the first wait is positive.
    pub fn next_delay(&mut self, jitter: u64) -> Duration {
        let upper_ms = (1_000_u64 << self.attempts.min(6)).min(60_000);
        self.attempts = self.attempts.saturating_add(1);
        let lower_ms = upper_ms / 2;
        let jitter_ms = jitter % (upper_ms - lower_ms + 1);
        Duration::from_millis(lower_ms + jitter_ms)
    }

    /// How many waits have been selected, saturating rather than wrapping.
    pub fn attempts(&self) -> u64 {
        self.attempts
    }
}

#[cfg(test)]
mod tests {
    use super::CapacityBackoff;
    use std::time::Duration;

    #[test]
    fn the_first_wait_is_positive_and_jitter_reaches_both_bounds() {
        let mut lower = CapacityBackoff::default();
        assert_eq!(lower.attempts(), 0);
        assert_eq!(lower.next_delay(0), Duration::from_millis(500));
        assert_eq!(lower.attempts(), 1);

        let mut upper = CapacityBackoff::default();
        assert_eq!(upper.next_delay(500), Duration::from_secs(1));
        assert_eq!(upper.attempts(), 1);

        let mut large_jitter = CapacityBackoff::default();
        assert!(
            (Duration::from_millis(500)..=Duration::from_secs(1))
                .contains(&large_jitter.next_delay(u64::MAX))
        );
    }

    #[test]
    fn capacity_waits_grow_then_stay_capped_across_continued_retries() {
        let bounds_ms = [
            (500, 1_000),
            (1_000, 2_000),
            (2_000, 4_000),
            (4_000, 8_000),
            (8_000, 16_000),
            (16_000, 32_000),
            (30_000, 60_000),
        ];
        for jitter in [0, 37, u64::MAX] {
            let mut backoff = CapacityBackoff::default();
            for (index, (lower, upper)) in bounds_ms.into_iter().enumerate() {
                assert!(
                    (Duration::from_millis(lower)..=Duration::from_millis(upper))
                        .contains(&backoff.next_delay(jitter))
                );
                assert_eq!(backoff.attempts(), index as u64 + 1);
            }
            for _ in 0..64 {
                assert!(
                    (Duration::from_secs(30)..=Duration::from_secs(60))
                        .contains(&backoff.next_delay(jitter))
                );
            }
            assert_eq!(backoff.attempts(), 71);
        }
    }

    #[test]
    fn a_saturated_attempt_count_keeps_the_same_bounded_wait() {
        let mut backoff = CapacityBackoff {
            attempts: u64::MAX - 1,
        };
        assert_eq!(backoff.next_delay(0), Duration::from_secs(30));
        assert_eq!(backoff.attempts(), u64::MAX);
        assert_eq!(backoff.next_delay(30_000), Duration::from_secs(60));
        assert_eq!(backoff.attempts(), u64::MAX);
        assert!(
            (Duration::from_secs(30)..=Duration::from_secs(60))
                .contains(&backoff.next_delay(u64::MAX))
        );
        assert_eq!(backoff.attempts(), u64::MAX);
    }
}
