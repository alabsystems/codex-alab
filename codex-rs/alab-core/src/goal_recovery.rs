// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One cancellable, positively paced recheck after a transient goal-admission failure.
//! This owns no goal policy: the callback must re-enter the normal admission path.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::watch;

pub type Recheck = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

tokio::task_local! {
    static CURRENT: Arc<Ticket>;
}

struct Ticket {
    owner: Weak<GoalAdmissionRecovery>,
    cancel: watch::Sender<bool>,
    failed_again: AtomicBool,
}

/// Stored with one goal runtime. A task holds only a weak reference to that runtime.
#[derive(Default)]
pub struct GoalAdmissionRecovery {
    pending: Mutex<Option<Arc<Ticket>>>,
}

impl GoalAdmissionRecovery {
    /// Repeated failures coalesce; a new callback never replaces an owned attempt.
    pub fn failed(self: &Arc<Self>, recheck: Recheck) {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if CURRENT
            .try_with(|ticket| {
                Weak::ptr_eq(&ticket.owner, &Arc::downgrade(self))
                    && !pending
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, ticket))
            })
            .unwrap_or(false)
        {
            // A stopped callback finishes its admission/accounting, but cannot
            // resurrect its retired recovery when that admission returns an error.
            return;
        }
        if let Some(ticket) = pending.as_ref() {
            ticket.failed_again.store(true, Ordering::SeqCst);
            return;
        }
        let (cancel, receiver) = watch::channel(false);
        let ticket = Arc::new(Ticket {
            owner: Arc::downgrade(self),
            cancel,
            failed_again: AtomicBool::new(true),
        });
        *pending = Some(Arc::clone(&ticket));
        drop(pending);
        tokio::spawn(run(ticket, receiver, recheck));
    }

    /// Goal mutation and thread teardown retire the exact outstanding recovery.
    pub fn cancel(&self) {
        self.retire(false);
    }

    /// A permanent result retires recovery, but a stale callback owns no newer ticket.
    pub fn permanent_failure(&self) {
        self.retire(true);
    }

    fn retire(&self, respect_current: bool) {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if respect_current
            && CURRENT
                .try_with(|ticket| {
                    std::ptr::eq(ticket.owner.as_ptr(), self)
                        && !pending
                            .as_ref()
                            .is_some_and(|current| Arc::ptr_eq(current, ticket))
                })
                .unwrap_or(false)
        {
            return;
        }
        let ticket = pending.take();
        drop(pending);
        if let Some(ticket) = ticket {
            ticket.cancel.send_replace(true);
        }
    }

    /// Called under the goal-mutation permit. Ordinary admissions are unchanged.
    pub fn permits_current_attempt(self: &Arc<Self>) -> bool {
        CURRENT
            .try_with(|ticket| {
                !Weak::ptr_eq(&ticket.owner, &Arc::downgrade(self))
                    || self
                        .pending
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, ticket))
            })
            .unwrap_or(true)
    }

    pub fn pending(&self) -> bool {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    }

    fn finish_attempt(&self, ticket: &Arc<Ticket>) -> bool {
        // Failure publication and retiring its recovery share one linearization point.
        // A late failure either keeps this owner or starts a new one after removal.
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !pending
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, ticket))
        {
            return false;
        }
        if ticket.failed_again.load(Ordering::SeqCst) {
            return true;
        }
        *pending = None;
        false
    }
}

struct Retire(Arc<Ticket>);

impl Drop for Retire {
    fn drop(&mut self) {
        if let Some(owner) = self.0.owner.upgrade() {
            let mut pending = owner
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if pending
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &self.0))
            {
                *pending = None;
            }
        }
    }
}

async fn run(ticket: Arc<Ticket>, mut cancel: watch::Receiver<bool>, recheck: Recheck) {
    let _retire = Retire(Arc::clone(&ticket));
    let mut delay = Duration::from_secs(1);
    let mut attempts = 0_u64;
    loop {
        attempts = attempts.saturating_add(1);
        let wait = crate::wake::ArmedWait::new(tokio::time::Instant::now() + delay);
        tracing::warn!(
            attempts,
            ?delay,
            "automatic goal admission failed transiently; rechecking after backoff"
        );
        tokio::select! {
            biased;
            _ = cancel.changed() => return,
            _ = wait.wait() => {}
        }
        if *cancel.borrow() || ticket.owner.upgrade().is_none() {
            return;
        }
        ticket.failed_again.store(false, Ordering::SeqCst);
        CURRENT.scope(Arc::clone(&ticket), recheck()).await;
        if *cancel.borrow()
            || !ticket
                .owner
                .upgrade()
                .is_some_and(|owner| owner.finish_attempt(&ticket))
        {
            return;
        }
        delay = delay.saturating_mul(2).min(Duration::from_secs(30));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn no_recheck() -> Recheck {
        Arc::new(|| Box::pin(async { panic!("a coalesced callback must not replace its owner") }))
    }

    #[tokio::test(start_paused = true)]
    async fn a_failure_rechecks_once_after_a_positive_delay_and_coalesces() {
        let recovery = Arc::new(GoalAdmissionRecovery::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        recovery.failed(Arc::new(move || {
            let seen = Arc::clone(&seen);
            Box::pin(async move {
                seen.fetch_add(1, Ordering::SeqCst);
            })
        }));
        recovery.failed(no_recheck());
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(999)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        tokio::time::advance(Duration::from_millis(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!recovery.pending());
    }

    #[tokio::test(start_paused = true)]
    async fn actual_transient_failures_continue_past_three_with_capped_positive_backoff() {
        let recovery = Arc::new(GoalAdmissionRecovery::default());
        let weak = Arc::downgrade(&recovery);
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        recovery.failed(Arc::new(move || {
            let weak = weak.clone();
            let seen = Arc::clone(&seen);
            Box::pin(async move {
                if seen.fetch_add(1, Ordering::SeqCst) < 7 {
                    weak.upgrade().expect("runtime stands").failed(no_recheck());
                }
            })
        }));
        tokio::task::yield_now().await;
        for (index, seconds) in [1, 2, 4, 8, 16, 30, 30, 30].into_iter().enumerate() {
            tokio::time::advance(Duration::from_secs(seconds - 1)).await;
            assert_eq!(calls.load(Ordering::SeqCst), index);
            tokio::time::advance(Duration::from_secs(1)).await;
            tokio::task::yield_now().await;
            assert_eq!(calls.load(Ordering::SeqCst), index + 1);
        }
        assert!(!recovery.pending());
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_retires_a_wait_without_touching_a_new_ticket() {
        let recovery = Arc::new(GoalAdmissionRecovery::default());
        recovery.failed(no_recheck());
        tokio::task::yield_now().await;
        recovery.cancel();
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        recovery.failed(Arc::new(move || {
            let seen = Arc::clone(&seen);
            Box::pin(async move {
                seen.fetch_add(1, Ordering::SeqCst);
            })
        }));
        tokio::task::yield_now().await;
        assert!(recovery.pending());
        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!recovery.pending());
    }

    #[tokio::test]
    async fn a_stale_attempt_is_rejected_after_the_mutation_permit() {
        let recovery = Arc::new(GoalAdmissionRecovery::default());
        let (cancel, _) = watch::channel(false);
        let ticket = Arc::new(Ticket {
            owner: Arc::downgrade(&recovery),
            cancel,
            failed_again: AtomicBool::new(false),
        });
        *recovery.pending.lock().expect("pending") = Some(Arc::clone(&ticket));
        CURRENT
            .scope(Arc::clone(&ticket), async {
                assert!(recovery.permits_current_attempt());
                recovery.cancel();
                assert!(!recovery.permits_current_attempt());
            })
            .await;
        assert!(
            recovery.permits_current_attempt(),
            "ordinary admission remains unchanged"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_failure_after_atomic_retirement_gets_a_new_owner() {
        let recovery = Arc::new(GoalAdmissionRecovery::default());
        let (cancel, _) = watch::channel(false);
        let old = Arc::new(Ticket {
            owner: Arc::downgrade(&recovery),
            cancel,
            failed_again: AtomicBool::new(true),
        });
        *recovery.pending.lock().expect("pending") = Some(Arc::clone(&old));
        assert!(
            recovery.finish_attempt(&old),
            "a failure before retirement keeps its owner"
        );
        old.failed_again.store(false, Ordering::SeqCst);
        assert!(!recovery.finish_attempt(&old));
        assert!(
            !recovery.pending(),
            "retirement is complete before releasing the decision lock"
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        recovery.failed(Arc::new(move || {
            let seen = Arc::clone(&seen);
            Box::pin(async move {
                seen.fetch_add(1, Ordering::SeqCst);
            })
        }));
        drop(Retire(old));
        assert!(
            recovery.pending(),
            "old cleanup cannot erase the late failure's new owner"
        );
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_stopped_callback_cannot_resurrect_its_cancelled_recovery() {
        let recovery = Arc::new(GoalAdmissionRecovery::default());
        let weak = Arc::downgrade(&recovery);
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        recovery.failed(Arc::new(move || {
            let weak = weak.clone();
            let seen = Arc::clone(&seen);
            Box::pin(async move {
                seen.fetch_add(1, Ordering::SeqCst);
                let recovery = weak.upgrade().expect("runtime stands");
                recovery.cancel();
                recovery.failed(no_recheck());
                assert!(
                    !recovery.pending(),
                    "late failure from a cancelled callback cannot restart it"
                );
            })
        }));
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!recovery.pending());
    }

    #[tokio::test(start_paused = true)]
    async fn an_ordinary_permanent_failure_retires_an_earlier_transient_wait() {
        let recovery = Arc::new(GoalAdmissionRecovery::default());
        recovery.failed(no_recheck());
        recovery.permanent_failure();
        tokio::time::advance(Duration::from_secs(60)).await;
        assert!(!recovery.pending());
    }

    #[tokio::test(start_paused = true)]
    async fn a_stale_permanent_failure_cannot_erase_a_new_owner() {
        let recovery = Arc::new(GoalAdmissionRecovery::default());
        recovery.failed(no_recheck());
        let old = recovery
            .pending
            .lock()
            .expect("pending")
            .clone()
            .expect("ticket");
        recovery.cancel();
        recovery.failed(no_recheck());
        CURRENT
            .scope(old, async {
                recovery.permanent_failure();
                assert!(recovery.pending(), "the newer recovery retains custody");
            })
            .await;
        recovery.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn a_recheck_does_not_take_authority_over_another_runtime() {
        let first = Arc::new(GoalAdmissionRecovery::default());
        first.failed(no_recheck());
        let current = first
            .pending
            .lock()
            .expect("pending")
            .clone()
            .expect("ticket");
        let second = Arc::new(GoalAdmissionRecovery::default());
        CURRENT
            .scope(current, async {
                assert!(second.permits_current_attempt());
                second.failed(no_recheck());
                assert!(second.pending());
                second.permanent_failure();
                assert!(!second.pending());
                assert!(first.pending());
            })
            .await;
        first.cancel();
    }
}
