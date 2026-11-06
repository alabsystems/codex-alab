// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Backoff for model capacity and turn-scoped provider retry advice.
//! The caller owns error classification, cancellation and the source of jitter.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

#[path = "capacity_wait.rs"]
mod live_wait;
pub use live_wait::CAPACITY_RETRY_NOTICE;
pub use live_wait::CapacityReason;
pub use live_wait::LiveCapacityWait;
pub use live_wait::LiveCapacityWaitGuard;

/// The latest Responses failure's server deadline, scoped to its turn.
///
/// The error lifecycle input carries semantic details but omits Retry-After. The
/// retry hooks for sampling and compaction write this alongside those details so a
/// continuation can keep the provider's absolute deadline without restarting it.
#[derive(Debug, Clone, Copy)]
pub struct SamplingRetryAdvice {
    deadline: Option<crate::wake::Deadline>,
}

impl SamplingRetryAdvice {
    /// Capture the current sleep count when the failure's deadline is retained.
    pub fn new(retry_at: Option<Instant>) -> Self {
        Self {
            deadline: retry_at.map(crate::wake::Deadline::new),
        }
    }

    /// Capture the deadline with the same reading its caller uses for the handoff.
    pub fn with_reading(retry_at: Option<Instant>, reading: Option<crate::wake::Reading>) -> Self {
        Self {
            deadline: retry_at.map(|at| crate::wake::Deadline::with_reading(at, reading)),
        }
    }

    /// Carry receipt advice without converting it through an instant and a second reading.
    pub fn from_deadline_with_reading(
        retry_at: Option<crate::wake::Deadline>,
        reading: Option<crate::wake::Reading>,
    ) -> Self {
        Self {
            deadline: retry_at.map(|deadline| deadline.with_baseline(reading)),
        }
    }

    /// The original real-time deadline, including sleep since it was retained.
    pub fn current_deadline(&self) -> Option<Instant> {
        self.deadline.map(crate::wake::Deadline::current)
    }

    /// Evaluate against the reading already captured by the error lifecycle.
    pub fn deadline_at(
        &self,
        reading: Option<crate::wake::Reading>,
        now: Instant,
    ) -> Option<Instant> {
        self.deadline.map(|at| at.at_reading(reading, now))
    }
}

/// Installed in a root turn's extension store. The retry hook reports its selected
/// wait through this seam; it knows nothing about the session record or autopilot.
pub struct InFlightRetryObserver {
    observation: Arc<RetryObservation>,
}

struct RetryObservation {
    current: Mutex<u64>,
    changed: Box<dyn Fn(Option<Instant>) + Send + Sync>,
}

impl InFlightRetryObserver {
    pub fn new(changed: impl Fn(Option<Instant>) + Send + Sync + 'static) -> Self {
        Self {
            observation: Arc::new(RetryObservation {
                current: Mutex::new(0),
                changed: Box::new(changed),
            }),
        }
    }

    pub fn observe(&self, deadline: Option<Instant>) {
        let mut current = self
            .observation
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *current = current.wrapping_add(1);
        (self.observation.changed)(deadline);
    }

    /// Cancellation clears the current wait, unless the session's signal teardown
    /// explicitly retains that exact native turn's pacing fact for aterm's relaunch.
    pub fn waiting(&self, deadline: Instant) -> InFlightRetryGuard {
        let mut current = self
            .observation
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *current = current.wrapping_add(1);
        (self.observation.changed)(Some(deadline));
        InFlightRetryGuard {
            observation: Arc::clone(&self.observation),
            generation: *current,
        }
    }
}

pub struct InFlightRetryGuard {
    observation: Arc<RetryObservation>,
    generation: u64,
}

impl InFlightRetryGuard {
    /// Publish a revised deadline only while this exact wait still owns the observation.
    /// A later observation or wait cannot be replaced by a delayed wake update.
    #[must_use]
    pub fn update(&self, deadline: Instant) -> bool {
        let current = self
            .observation
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *current != self.generation {
            return false;
        }
        (self.observation.changed)(Some(deadline));
        true
    }
}

impl Drop for InFlightRetryGuard {
    fn drop(&mut self) {
        let current = self
            .observation
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *current == self.generation {
            (self.observation.changed)(None);
        }
    }
}

/// The exact streamed quota code already recognized by the HTTP error path.
/// Other codes and upstream-inactive consumers keep their existing classification.
pub fn streamed_quota_alias(code: &str) -> bool {
    streamed_quota_alias_for(crate::active(), code)
}

fn streamed_quota_alias_for(active: bool, code: &str) -> bool {
    active && code == "organization_usage_limit_exceeded"
}

/// Client refusals that require a corrected request or account access. The transient
/// timeout, conflict, early-data and rate-limit statuses keep normal retry handling.
pub fn permanent_client_status(code: u16) -> bool {
    (400..500).contains(&code) && !matches!(code, 408 | 409 | 425 | 429)
}

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
    fn retained_sampling_advice_preserves_known_and_establishes_missing_origins() {
        use crate::wake::Deadline;
        use crate::wake::Reading;
        let at = std::time::Instant::now() + Duration::from_secs(60);
        let reading = |seconds| {
            Some(Reading {
                asleep: Duration::from_secs(seconds),
                last: None,
            })
        };
        let known = Deadline::with_reading(at, reading(5000));
        let kept =
            super::SamplingRetryAdvice::from_deadline_with_reading(Some(known), reading(5120));
        assert_eq!(
            kept.deadline_at(reading(5120), at),
            Some(at - Duration::from_secs(120))
        );
        assert_eq!(
            kept.deadline_at(reading(5130), at),
            Some(at - Duration::from_secs(130))
        );
        assert_eq!(
            known.at_reading(reading(5000), at),
            at,
            "retaining advice never mutates its receipt or copies"
        );
        let fresh = super::SamplingRetryAdvice::from_deadline_with_reading(
            Some(Deadline::untracked(at)),
            reading(5120),
        );
        assert_eq!(
            fresh.deadline_at(reading(5120), at),
            Some(at),
            "sleep before a missing origin is established is not inferred"
        );
        assert_eq!(
            fresh.deadline_at(reading(5130), at),
            Some(at - Duration::from_secs(10))
        );
        let absent = super::SamplingRetryAdvice::from_deadline_with_reading(None, reading(5120));
        assert_eq!(absent.deadline_at(reading(5130), at), None);
    }

    #[test]
    fn only_the_exact_additional_quota_code_changes_when_active() {
        for active in [false, true] {
            assert_eq!(
                super::streamed_quota_alias_for(active, "organization_usage_limit_exceeded"),
                active
            );
            for code in [
                "",
                "insufficient_quota",
                "server_is_overloaded",
                "rate_limit_exceeded",
                "unknown_error",
                " organization_usage_limit_exceeded ",
                "ORGANIZATION_USAGE_LIMIT_EXCEEDED",
            ] {
                assert!(!super::streamed_quota_alias_for(active, code), "{code}");
            }
        }
    }

    #[test]
    fn an_old_wait_guard_cannot_update_or_clear_a_newer_retry_observation() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = std::sync::Arc::clone(&seen);
        let observer = super::InFlightRetryObserver::new(move |deadline| {
            captured
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(deadline);
        });
        let at = std::time::Instant::now();
        let old = observer.waiting(at);
        let new = observer.waiting(at + Duration::from_secs(1));
        assert!(!old.update(at + Duration::from_secs(60)));
        assert!(new.update(at + Duration::from_millis(500)));
        drop(old);
        assert_eq!(
            *seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            vec![
                Some(at),
                Some(at + Duration::from_secs(1)),
                Some(at + Duration::from_millis(500))
            ]
        );
        drop(new);
        assert_eq!(
            *seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            vec![
                Some(at),
                Some(at + Duration::from_secs(1)),
                Some(at + Duration::from_millis(500)),
                None
            ]
        );
    }

    #[test]
    fn a_replaced_observation_refuses_the_old_waits_deadline_update_and_cleanup() {
        for replacement in [None, Some(std::time::Instant::now())] {
            let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let captured = std::sync::Arc::clone(&seen);
            let observer = super::InFlightRetryObserver::new(move |deadline| {
                captured
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(deadline);
            });
            let at = std::time::Instant::now();
            let old = observer.waiting(at);
            observer.observe(replacement);
            assert!(!old.update(at + Duration::from_secs(60)));
            drop(old);
            assert_eq!(
                *seen
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
                vec![Some(at), replacement]
            );
        }
    }

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
