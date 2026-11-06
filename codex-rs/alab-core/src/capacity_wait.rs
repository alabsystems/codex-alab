// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Live presentation evidence for the known capacity sleep, never retry authority.
//! The active turn owns this slot; no history or crash-pacing record is consulted.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

/// Shared by the live capacity error and an exact active-turn reconnect snapshot.
pub const CAPACITY_RETRY_NOTICE: &str =
    "Model is at capacity; retrying automatically (interrupt to stop)...";

/// The exact capacity failure that owns a live wait, independent of any API crate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CapacityReason {
    #[default]
    ServerOverloaded,
    PolicyCapacityUnavailable,
}

impl CapacityReason {
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "server_is_overloaded" => Some(Self::ServerOverloaded),
            "policy_capacity_unavailable" => Some(Self::PolicyCapacityUnavailable),
            _ => None,
        }
    }

    pub const fn notice(self) -> &'static str {
        match self {
            Self::ServerOverloaded => CAPACITY_RETRY_NOTICE,
            Self::PolicyCapacityUnavailable => {
                "Policy-restricted capacity is unavailable; retrying automatically (interrupt to stop)..."
            }
        }
    }
}

#[derive(Default)]
struct Wait {
    generation: u64,
    deadline: Option<Instant>,
    reason: CapacityReason,
}

/// Installed in the existing turn extension store only by the capacity-wait path.
#[derive(Default)]
pub struct LiveCapacityWait(Mutex<Wait>);

impl LiveCapacityWait {
    /// A newer wait supersedes an older guard without letting its drop clear the slot.
    /// Exhausting the generation space refuses presentation evidence, never the retry.
    pub fn begin(self: &Arc<Self>, deadline: Instant) -> LiveCapacityWaitGuard {
        self.begin_with_reason(deadline, CapacityReason::ServerOverloaded)
    }

    /// Keep the failure's reason and deadline in the same guarded generation.
    pub fn begin_with_reason(
        self: &Arc<Self>,
        deadline: Instant,
        reason: CapacityReason,
    ) -> LiveCapacityWaitGuard {
        let mut wait = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let generation = wait.generation.checked_add(1);
        wait.deadline = generation.map(|_| deadline);
        wait.reason = reason;
        if let Some(generation) = generation {
            wait.generation = generation;
        }
        LiveCapacityWaitGuard {
            state: Arc::clone(self),
            generation,
        }
    }

    /// Check and publish without suspension while the slot still proves a live sleep.
    /// The caller reserves its outgoing queue capacity before entering this method.
    pub fn publish_if_current<R>(
        &self,
        clock: impl FnOnce() -> Instant,
        publish: impl FnOnce() -> R,
    ) -> Option<R> {
        self.publish_reason_if_current(clock, |_| publish())
    }

    /// Publish the exact current reason under the same lock as its deadline check.
    pub fn publish_reason_if_current<R>(
        &self,
        clock: impl FnOnce() -> Instant,
        publish: impl FnOnce(CapacityReason) -> R,
    ) -> Option<R> {
        let wait = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = clock();
        wait.deadline.filter(|deadline| *deadline > now)?;
        Some(publish(wait.reason))
    }
}

/// Ending or cancelling the sleep retires only its own generation.
pub struct LiveCapacityWaitGuard {
    state: Arc<LiveCapacityWait>,
    generation: Option<u64>,
}

impl LiveCapacityWaitGuard {
    /// Revise only the live wait this guard owns, without reviving a retired or refused slot.
    #[must_use]
    pub fn update(&self, deadline: Instant) -> bool {
        let mut wait = self
            .state
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.generation != Some(wait.generation) || wait.deadline.is_none() {
            return false;
        }
        wait.deadline = Some(deadline);
        true
    }
}

impl Drop for LiveCapacityWaitGuard {
    fn drop(&mut self) {
        let mut wait = self
            .state
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.generation == Some(wait.generation) {
            wait.deadline = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn capacity_codes_preserve_their_exact_reason() {
        assert_eq!(
            CapacityReason::from_code("server_is_overloaded"),
            Some(CapacityReason::ServerOverloaded)
        );
        assert_eq!(
            CapacityReason::from_code("policy_capacity_unavailable"),
            Some(CapacityReason::PolicyCapacityUnavailable)
        );
        for other in [
            "cyber_policy",
            "bio_policy",
            "misalignment_policy_violation",
            "POLICY_CAPACITY_UNAVAILABLE",
            "policy_capacity_unavailable ",
            "overloaded",
        ] {
            assert_eq!(CapacityReason::from_code(other), None);
        }
    }

    #[test]
    fn replacing_a_wait_replaces_its_reason_under_the_same_generation() {
        let state = Arc::new(LiveCapacityWait::default());
        let now = Instant::now();
        let old = state.begin(now + Duration::from_secs(5));
        let current = state.begin_with_reason(
            now + Duration::from_secs(10),
            CapacityReason::PolicyCapacityUnavailable,
        );
        assert!(!old.update(now + Duration::from_secs(60)));
        assert!(current.update(now + Duration::from_secs(20)));
        assert_eq!(
            state.publish_reason_if_current(|| now, |reason| reason),
            Some(CapacityReason::PolicyCapacityUnavailable)
        );
        drop(old);
        assert_eq!(
            state.publish_reason_if_current(|| now, |reason| reason),
            Some(CapacityReason::PolicyCapacityUnavailable)
        );
        drop(current);
        assert_eq!(
            state.publish_reason_if_current(|| now, |reason| reason),
            None
        );
    }

    #[test]
    fn only_a_future_live_guard_allows_synchronous_publication() {
        let state = Arc::new(LiveCapacityWait::default());
        let now = Instant::now();
        assert_eq!(state.publish_if_current(|| now, || true), None);
        let guard = state.begin(now + Duration::from_secs(5));
        assert_eq!(state.publish_if_current(|| now, || true), Some(true));
        assert_eq!(
            state.publish_if_current(|| now + Duration::from_secs(5), || true),
            None
        );
        drop(guard);
        assert_eq!(state.publish_if_current(|| now, || true), None);
    }

    #[test]
    fn an_old_guard_cannot_update_or_clear_its_replacement() {
        let state = Arc::new(LiveCapacityWait::default());
        let now = Instant::now();
        let old = state.begin(now + Duration::from_secs(5));
        let new = state.begin(now + Duration::from_secs(10));
        assert!(!old.update(now + Duration::from_secs(60)));
        assert!(new.update(now + Duration::from_secs(2)));
        assert_eq!(
            state.publish_if_current(|| now + Duration::from_secs(3), || true),
            None
        );
        drop(old);
        assert_eq!(
            state.publish_if_current(|| now + Duration::from_secs(1), || true),
            Some(true)
        );
        drop(new);
        assert_eq!(state.publish_if_current(|| now, || true), None);
    }

    #[test]
    fn the_clock_is_sampled_under_the_slot_lock_and_can_refuse_an_expired_wait() {
        let state = Arc::new(LiveCapacityWait::default());
        let now = Instant::now();
        let _guard = state.begin(now + Duration::from_secs(5));
        let mut published = false;
        let result = state.publish_if_current(
            || {
                assert!(matches!(
                    state.0.try_lock(),
                    Err(std::sync::TryLockError::WouldBlock)
                ));
                now + Duration::from_secs(5)
            },
            || {
                published = true;
            },
        );
        assert_eq!(result, None);
        assert!(
            !published,
            "the sampled expired deadline must refuse publication"
        );
    }

    #[test]
    fn generation_exhaustion_refuses_a_new_notice_without_ending_the_task() {
        let state = Arc::new(LiveCapacityWait(Mutex::new(Wait {
            generation: u64::MAX - 1,
            deadline: None,
            reason: CapacityReason::ServerOverloaded,
        })));
        let now = Instant::now();
        let old = state.begin(now + Duration::from_secs(5));
        assert_eq!(state.publish_if_current(|| now, || true), Some(true));
        let guard = state.begin(now + Duration::from_secs(5));
        assert_eq!(state.publish_if_current(|| now, || true), None);
        assert!(
            !old.update(now + Duration::from_secs(60)),
            "an older maximum-generation guard cannot revive the refused slot"
        );
        assert!(!guard.update(now + Duration::from_secs(60)));
        assert_eq!(state.publish_if_current(|| now, || true), None);
        drop(old);
        drop(guard);
    }
}
