// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Revocable native admission during a process handoff. Callers retain each
//! admission through its real mutation or queued operation. This state alone
//! grants no execution.

use std::sync::Arc;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    Open,
    Prepared(u64),
    Interrupted(u64),
    Committed(u64),
}

#[derive(Debug, Default)]
struct Inner {
    phase: Phase,
    generation: u64,
    waiting: usize,
    retiring: bool,
    final_capture: FinalCapturePhase,
    retirement: RetirementPhase,
}

#[derive(Debug, Default)]
enum FinalCapturePhase {
    #[default]
    Pending,
    Released,
    Published {
        inventory: bool,
    },
    Abandoned(String),
}

#[derive(Debug, Default)]
enum RetirementPhase {
    #[default]
    None,
    Deferred,
    Prepared(Result<(), String>),
    Complete(Result<(), String>),
}

#[derive(Debug, Default)]
pub struct State {
    inner: Mutex<Inner>,
    changed: tokio::sync::Notify,
}

/// Registration of the one retirement owner for a committed transfer. A
/// repeated source-stop observer joins the already registered owner by doing
/// nothing; it must not start an ordinary retirement beside it.
#[derive(Debug)]
pub enum RetirementRegistration {
    Owner(DeferredRetirement),
    Registered,
}

/// A committed runtime's retirement remains owned while Core reaches final
/// source quiescence. Dropping the owner publishes completion so a capture can
/// never wait forever after the cleanup task itself ends.
#[derive(Debug)]
pub struct DeferredRetirement {
    state: Arc<State>,
    complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Transferred,
    PendingAdmission,
    AlreadyPreparing,
    GenerationExhausted,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Transferred => "the runtime is transferring to another process",
            Self::PendingAdmission => "native admission is pending",
            Self::AlreadyPreparing => "another runtime transfer is being prepared",
            Self::GenerationExhausted => "runtime transfer generation is exhausted",
        })
    }
}

impl State {
    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Register before waiting for the native mutation boundary. An admission that
    /// precedes preparation prevents it; one arriving during preparation
    /// invalidates the transfer, even if that admission is subsequently cancelled.
    pub fn admit(self: &Arc<Self>) -> Result<Admission, Refusal> {
        let mut inner = self.inner();
        if inner.retiring || matches!(inner.phase, Phase::Committed(_)) {
            return Err(Refusal::Transferred);
        }
        let waiting = inner
            .waiting
            .checked_add(1)
            .ok_or(Refusal::PendingAdmission)?;
        let interrupted = if let Phase::Prepared(generation) = inner.phase {
            inner.phase = Phase::Interrupted(generation);
            true
        } else {
            false
        };
        inner.waiting = waiting;
        if interrupted {
            self.changed.notify_waiters();
        }
        Ok(Admission {
            state: Arc::clone(self),
            interrupted,
        })
    }

    /// Called while holding the native mutation boundary and a verified idle state.
    pub fn prepare(self: &Arc<Self>) -> Result<Transfer, Refusal> {
        let mut inner = self.inner();
        if inner.retiring {
            return Err(Refusal::Transferred);
        }
        if inner.phase != Phase::Open {
            return Err(Refusal::AlreadyPreparing);
        }
        if inner.waiting != 0 {
            return Err(Refusal::PendingAdmission);
        }
        let generation = inner
            .generation
            .checked_add(1)
            .ok_or(Refusal::GenerationExhausted)?;
        inner.generation = generation;
        inner.phase = Phase::Prepared(generation);
        Ok(Transfer {
            state: Arc::clone(self),
            generation,
            committed: false,
        })
    }

    pub fn committed(&self) -> bool {
        matches!(self.inner().phase, Phase::Committed(_))
    }

    /// Reserve this committed runtime for its final-capture owner. The source
    /// lifecycle may return after registration, but registry custody and native
    /// persistence remain held until the exact transfer releases this owner. A
    /// retirement which wins before commitment atomically closes all later
    /// admission/preparation and invalidates a prepared generation, so ordinary
    /// cleanup cannot race either a checkpoint or a later commit.
    pub fn defer_retirement(self: &Arc<Self>) -> Option<RetirementRegistration> {
        let mut inner = self.inner();
        inner.retiring = true;
        match inner.phase {
            Phase::Prepared(generation) => {
                inner.phase = Phase::Interrupted(generation);
                self.changed.notify_waiters();
                return None;
            }
            Phase::Committed(_) => {}
            Phase::Open | Phase::Interrupted(_) => return None,
        }
        match &inner.retirement {
            RetirementPhase::None => {
                inner.retirement = RetirementPhase::Deferred;
                Some(RetirementRegistration::Owner(DeferredRetirement {
                    state: Arc::clone(self),
                    complete: false,
                }))
            }
            RetirementPhase::Deferred
            | RetirementPhase::Prepared(_)
            | RetirementPhase::Complete(_) => Some(RetirementRegistration::Registered),
        }
    }

    /// Runtime configuration changes also revoke a prepared transfer.
    pub fn interrupt(&self) -> bool {
        let mut inner = self.inner();
        if let Phase::Prepared(generation) = inner.phase {
            inner.phase = Phase::Interrupted(generation);
            self.changed.notify_waiters();
            true
        } else {
            false
        }
    }
}

#[derive(Debug)]
pub struct Admission {
    state: Arc<State>,
    interrupted: bool,
}

impl Admission {
    pub fn interrupted_transfer(&self) -> bool {
        self.interrupted
    }
}

impl Drop for Admission {
    fn drop(&mut self) {
        self.state.inner().waiting -= 1;
    }
}

#[derive(Debug)]
pub struct Transfer {
    state: Arc<State>,
    generation: u64,
    committed: bool,
}

impl Transfer {
    pub fn belongs_to(&self, state: &Arc<State>) -> bool {
        Arc::ptr_eq(&self.state, state)
    }

    pub fn valid(&self) -> bool {
        let inner = self.state.inner();
        inner.phase == Phase::Prepared(self.generation) && inner.waiting == 0
    }

    /// Wait until this exact preparation is no longer valid. Register before
    /// checking so input accepted before or during the wait cannot lose its wake.
    /// This notification grants no permission to transfer or discard that input.
    pub async fn invalidated(&self) {
        loop {
            let changed = self.state.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if !self.valid() {
                return;
            }
            changed.await;
        }
    }

    /// True only for this exact token's irrevocably committed generation.
    pub fn committed(&self) -> bool {
        self.committed && self.state.inner().phase == Phase::Committed(self.generation)
    }

    /// Release source retirement only after final accounting has consumed this
    /// exact committed runtime. This is a provisional release: cleanup may
    /// prepare, but it retains registry custody until the complete capture
    /// inventory is published synchronously. Repeating the release is harmless.
    pub fn release_final_capture(&self) -> bool {
        let mut inner = self.state.inner();
        if !self.committed || inner.phase != Phase::Committed(self.generation) {
            return false;
        }
        match &inner.final_capture {
            FinalCapturePhase::Pending => {
                inner.final_capture = FinalCapturePhase::Released;
                self.state.changed.notify_waiters();
                true
            }
            FinalCapturePhase::Released | FinalCapturePhase::Published { .. } => true,
            FinalCapturePhase::Abandoned(_) => false,
        }
    }

    /// Wake source retirement when capture cannot produce a transferable
    /// snapshot. Cleanup must carry this failure permanently rather than
    /// publishing a healthy successor without Core/source quiescence proof.
    pub fn abandon_final_capture(&self, reason: impl Into<String>) -> bool {
        let mut inner = self.state.inner();
        if !self.committed || inner.phase != Phase::Committed(self.generation) {
            return false;
        }
        match &inner.final_capture {
            FinalCapturePhase::Pending | FinalCapturePhase::Released => {
                inner.final_capture = FinalCapturePhase::Abandoned(reason.into());
                self.state.changed.notify_waiters();
                true
            }
            FinalCapturePhase::Abandoned(_) => true,
            FinalCapturePhase::Published { .. } => false,
        }
    }

    /// Validate the second phase without changing it. A capture coordinator
    /// checks its whole inventory before publishing any member.
    pub fn final_capture_publishable(&self) -> bool {
        let inner = self.state.inner();
        self.committed
            && inner.phase == Phase::Committed(self.generation)
            && matches!(&inner.final_capture, FinalCapturePhase::Released)
            && matches!(&inner.retirement, RetirementPhase::Prepared(Ok(())))
    }

    /// Commit this member's prepared cleanup to the already-owned published
    /// capture. The coordinator invokes every member synchronously, with no
    /// cancellation point between inventory validation and publication.
    pub fn publish_final_capture(&self) -> bool {
        self.publish_final_capture_with_scope(false)
    }

    /// Inventory-wide publication already removed this exact registry key
    /// under the shared batch locks before committing the barrier.
    #[doc(hidden)]
    pub fn publish_final_capture_inventory(&self) -> bool {
        self.publish_final_capture_with_scope(true)
    }

    /// Roll back an inventory publication which could not publish every member.
    /// The registry coordinator has already forced every removed key to a
    /// permanent carry before releasing its locks.
    #[doc(hidden)]
    pub fn fail_final_capture_inventory(&self, reason: impl Into<String>) -> bool {
        let mut inner = self.state.inner();
        if !self.committed || inner.phase != Phase::Committed(self.generation) {
            return false;
        }
        if matches!(
            &inner.final_capture,
            FinalCapturePhase::Published { inventory: false }
        ) {
            return false;
        }
        let reason = reason.into();
        inner.final_capture = FinalCapturePhase::Abandoned(reason.clone());
        if matches!(
            &inner.retirement,
            RetirementPhase::Prepared(Ok(())) | RetirementPhase::Complete(Ok(()))
        ) {
            inner.retirement = RetirementPhase::Complete(Err(reason));
        }
        self.state.changed.notify_waiters();
        true
    }

    fn publish_final_capture_with_scope(&self, inventory: bool) -> bool {
        let mut inner = self.state.inner();
        if !self.committed || inner.phase != Phase::Committed(self.generation) {
            return false;
        }
        match &inner.final_capture {
            FinalCapturePhase::Published { inventory: current } => *current == inventory,
            FinalCapturePhase::Released
                if matches!(&inner.retirement, RetirementPhase::Prepared(Ok(()))) =>
            {
                inner.final_capture = FinalCapturePhase::Published { inventory };
                self.state.changed.notify_waiters();
                true
            }
            FinalCapturePhase::Pending
            | FinalCapturePhase::Released
            | FinalCapturePhase::Abandoned(_) => false,
        }
    }

    /// Join the failure-prone cleanup phase while registry custody is still
    /// reserved. Publication is a separate synchronous inventory decision.
    pub async fn wait_for_deferred_retirement_prepared(&self) -> Result<(), String> {
        if !self.committed {
            return Err("final capture does not own a committed runtime transfer".into());
        }
        loop {
            let changed = self.state.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let outcome = {
                let inner = self.state.inner();
                if inner.phase != Phase::Committed(self.generation) {
                    return Err("deferred retirement lost its committed runtime transfer".into());
                }
                match &inner.retirement {
                    RetirementPhase::None => Some(Err(
                        "committed runtime retirement was never registered".into(),
                    )),
                    RetirementPhase::Deferred => None,
                    RetirementPhase::Prepared(result) | RetirementPhase::Complete(result) => {
                        Some(result.clone())
                    }
                }
            };
            if let Some(outcome) = outcome {
                return outcome;
            }
            changed.await;
        }
    }

    /// Join cleanup only when source retirement registered an owner. An early
    /// forest failure may have no runtime teardown to join.
    pub async fn wait_for_deferred_retirement(&self) -> Result<(), String> {
        if !self.committed {
            return Err("final capture does not own a committed runtime transfer".into());
        }
        loop {
            let changed = self.state.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let outcome = {
                let inner = self.state.inner();
                if inner.phase != Phase::Committed(self.generation) {
                    return Err("deferred retirement lost its committed runtime transfer".into());
                }
                match &inner.retirement {
                    RetirementPhase::None => Some(Err(
                        "committed runtime retirement was never registered".into(),
                    )),
                    RetirementPhase::Deferred | RetirementPhase::Prepared(_) => None,
                    RetirementPhase::Complete(result) => Some(result.clone()),
                }
            };
            if let Some(outcome) = outcome {
                return outcome;
            }
            changed.await;
        }
    }

    /// The final synchronous handoff decision. Committed runtimes refuse new
    /// admissions instead of leaving a continuation waiting through teardown.
    pub fn commit(&mut self) -> bool {
        self.commit_if(|| true)
    }

    /// Linearize the native TUI/host decision with native admission. A refused
    /// decision leaves rollback available; an invalidated transfer never calls it.
    pub fn commit_if(&mut self, decide: impl FnOnce() -> bool) -> bool {
        let mut inner = self.state.inner();
        if inner.retiring || inner.phase != Phase::Prepared(self.generation) || inner.waiting != 0 {
            return false;
        }
        if !decide() {
            return false;
        }
        inner.phase = Phase::Committed(self.generation);
        self.committed = true;
        self.state.changed.notify_waiters();
        true
    }
}

impl DeferredRetirement {
    /// Wait until final capture has either produced the source goal state or
    /// decided that capture failed. The caller then performs ordinary drain,
    /// persistence retirement and registry removal.
    pub async fn released(&self) -> Result<(), String> {
        loop {
            let changed = self.state.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let outcome = match &self.state.inner().final_capture {
                FinalCapturePhase::Pending => None,
                FinalCapturePhase::Released | FinalCapturePhase::Published { .. } => Some(Ok(())),
                FinalCapturePhase::Abandoned(error) => Some(Err(error.clone())),
            };
            if let Some(outcome) = outcome {
                return outcome;
            }
            changed.await;
        }
    }

    /// Publish the failure-prone cleanup result while the registry key remains
    /// reserved. A successful result still grants no successor authority until
    /// the capture coordinator publishes the complete inventory.
    pub fn prepare(&self, result: Result<(), String>) -> bool {
        let mut inner = self.state.inner();
        match &inner.retirement {
            RetirementPhase::Deferred => {
                inner.retirement = RetirementPhase::Prepared(result);
                self.state.changed.notify_waiters();
                true
            }
            RetirementPhase::Prepared(current) => current.is_ok() == result.is_ok(),
            RetirementPhase::None | RetirementPhase::Complete(_) => false,
        }
    }

    /// Wait for the inventory-wide publication decision. Abandonment can
    /// override the provisional release until successful publication.
    pub async fn published(&self) -> Result<(), String> {
        loop {
            let changed = self.state.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let outcome = match &self.state.inner().final_capture {
                FinalCapturePhase::Pending | FinalCapturePhase::Released => None,
                FinalCapturePhase::Published { .. } => Some(Ok(())),
                FinalCapturePhase::Abandoned(error) => Some(Err(error.clone())),
            };
            if let Some(outcome) = outcome {
                return outcome;
            }
            changed.await;
        }
    }

    /// True only after the shared registry owner published this member as part
    /// of one inventory-wide commit.
    pub fn inventory_published(&self) -> bool {
        self.publication_scope() == Some(true)
    }

    /// `Some(true)` is an inventory batch, `Some(false)` is a standalone
    /// publication, and `None` means publication was absent or revoked.
    #[doc(hidden)]
    pub fn publication_scope(&self) -> Option<bool> {
        match &self.state.inner().final_capture {
            FinalCapturePhase::Published { inventory } => Some(*inventory),
            FinalCapturePhase::Pending
            | FinalCapturePhase::Released
            | FinalCapturePhase::Abandoned(_) => None,
        }
    }

    pub fn complete(mut self, result: Result<(), String>) {
        self.finish(result);
    }

    fn finish(&mut self, result: Result<(), String>) {
        if self.complete {
            return;
        }
        let mut inner = self.state.inner();
        if matches!(
            &inner.retirement,
            RetirementPhase::Deferred | RetirementPhase::Prepared(_)
        ) {
            inner.retirement = RetirementPhase::Complete(result);
            self.state.changed.notify_waiters();
        }
        self.complete = true;
    }
}

impl Drop for DeferredRetirement {
    fn drop(&mut self) {
        if self.complete {
            return;
        }
        let mut inner = self.state.inner();
        if matches!(
            &inner.retirement,
            RetirementPhase::Deferred | RetirementPhase::Prepared(_)
        ) {
            let result = if matches!(
                (&inner.final_capture, &inner.retirement),
                (
                    FinalCapturePhase::Published { inventory: true },
                    RetirementPhase::Prepared(Ok(()))
                )
            ) {
                Ok(())
            } else {
                Err("deferred runtime retirement ended without a cleanup result".into())
            };
            inner.retirement = RetirementPhase::Complete(result);
            self.state.changed.notify_waiters();
        }
        self.complete = true;
    }
}

impl Drop for Transfer {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let mut inner = self.state.inner();
        if matches!(inner.phase, Phase::Prepared(id) | Phase::Interrupted(id) if id == self.generation)
        {
            inner.phase = Phase::Open;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn preflight_waits_wake_for_input_and_configuration_without_losing_early_invalidation() {
        for early in [true, false] {
            for configuration in [true, false] {
                let state = Arc::new(State::default());
                let transfer = Arc::new(state.prepare().unwrap());
                let invalidate = || {
                    if configuration {
                        assert!(state.interrupt());
                    } else {
                        let input = state.admit().unwrap();
                        assert!(input.interrupted_transfer());
                        drop(input); // Cancellation cannot retract accepted invalidation.
                    }
                };
                if early {
                    invalidate();
                }
                let (armed, ready) = tokio::sync::oneshot::channel();
                let watched = transfer.clone();
                let waiter = tokio::spawn(async move {
                    let invalidated = watched.invalidated();
                    tokio::pin!(invalidated);
                    let mut armed = Some(armed);
                    std::future::poll_fn(|cx| {
                        let result = std::future::Future::poll(invalidated.as_mut(), cx);
                        if let Some(armed) = armed.take() {
                            let _ = armed.send(());
                        }
                        result
                    })
                    .await;
                });
                ready.await.unwrap();
                if !early {
                    invalidate();
                }
                tokio::time::timeout(std::time::Duration::from_secs(10), waiter)
                    .await
                    .unwrap()
                    .unwrap();
                assert!(!transfer.valid());
                drop(transfer);
                assert!(
                    state.admit().is_ok(),
                    "revoked preparation leaves ordinary work open"
                );
            }
        }
    }

    #[test]
    fn queued_and_cancelled_admissions_do_not_disappear_from_preparation() {
        let state = Arc::new(State::default());
        let first = state.admit().expect("first admission");
        let cancelled = state.admit().expect("queued admission");
        drop(cancelled);
        assert!(matches!(state.prepare(), Err(Refusal::PendingAdmission)));
        drop(first);
        let transfer = state.prepare().expect("drained admission");
        assert!(transfer.valid());
        drop(transfer);
        assert!(state.admit().is_ok());
    }

    #[test]
    fn a_late_writer_revokes_the_attempt_even_when_it_cancels() {
        let state = Arc::new(State::default());
        let mut transfer = state.prepare().expect("prepare");
        let writer = state.admit().expect("new work has priority");
        assert!(writer.interrupted_transfer());
        assert!(!transfer.valid());
        drop(writer);
        assert!(!transfer.commit());
        drop(transfer);
        let mut next = state.prepare().expect("rollback reopened admission");
        assert!(next.commit());
        drop(next);
        assert!(state.committed());
        assert!(matches!(state.admit(), Err(Refusal::Transferred)));
    }

    #[test]
    fn a_stale_rollback_cannot_reopen_a_later_attempt() {
        let state = Arc::new(State::default());
        let old = state.prepare().expect("old transfer");
        let generation = old.generation;
        drop(old);
        let current = state.prepare().expect("new transfer");
        drop(Transfer {
            state,
            generation,
            committed: false,
        });
        assert!(current.valid());
    }

    #[test]
    fn the_external_decision_is_called_only_for_a_valid_transfer() {
        let state = Arc::new(State::default());
        let mut transfer = state.prepare().expect("prepare");
        assert!(!transfer.commit_if(|| false));
        assert!(transfer.valid());
        assert!(state.interrupt());
        assert!(!transfer.commit_if(|| panic!("an interrupted transfer has no authority")));
    }

    #[test]
    fn concurrent_commit_and_admission_have_exactly_one_winner() {
        for _ in 0..128 {
            let state = Arc::new(State::default());
            let mut transfer = state.prepare().expect("prepare");
            let barrier = std::sync::Barrier::new(2);
            std::thread::scope(|scope| {
                let writer = scope.spawn(|| {
                    barrier.wait();
                    state.admit()
                });
                barrier.wait();
                let committed = transfer.commit();
                let admission = writer.join().expect("writer thread");
                assert_eq!(committed, matches!(admission, Err(Refusal::Transferred)));
            });
        }
    }

    #[test]
    fn exhausted_generations_refuse_transfers_but_keep_normal_work_available() {
        let state = Arc::new(State::default());
        state.inner().generation = u64::MAX;
        assert!(matches!(state.prepare(), Err(Refusal::GenerationExhausted)));
        assert!(state.admit().is_ok());
    }

    #[tokio::test]
    async fn committed_capture_releases_and_joins_its_deferred_retirement() {
        let state = Arc::new(State::default());
        let mut transfer = state.prepare().expect("prepare");
        assert!(transfer.commit());
        let RetirementRegistration::Owner(retirement) =
            state.defer_retirement().expect("committed retirement")
        else {
            panic!("first retirement registration must own cleanup");
        };
        assert!(matches!(
            state.defer_retirement(),
            Some(RetirementRegistration::Registered)
        ));

        let (entered, release_entered) = tokio::sync::oneshot::channel();
        let cleanup = tokio::spawn(async move {
            retirement
                .released()
                .await
                .expect("successful final capture");
            assert!(retirement.prepare(Ok(())));
            entered.send(()).expect("observe prepared cleanup");
            retirement.published().await.expect("inventory publication");
            retirement.complete(Ok(()));
        });
        assert!(transfer.release_final_capture());
        release_entered.await.expect("retirement prepared cleanup");
        transfer
            .wait_for_deferred_retirement_prepared()
            .await
            .expect("prepared deferred retirement");
        let mut joined = Box::pin(transfer.wait_for_deferred_retirement());
        assert!(matches!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(std::future::Future::poll(
                joined.as_mut(),
                cx
            )))
            .await,
            std::task::Poll::Pending
        ));
        assert!(transfer.final_capture_publishable());
        assert!(transfer.publish_final_capture());
        joined.await.expect("successful deferred retirement");
        cleanup.await.expect("join retirement owner");
    }

    #[tokio::test]
    async fn deferred_retirement_reports_cleanup_failure_and_owner_loss() {
        for drop_owner in [false, true] {
            let state = Arc::new(State::default());
            let mut transfer = state.prepare().expect("prepare");
            assert!(transfer.commit());
            let RetirementRegistration::Owner(retirement) =
                state.defer_retirement().expect("committed retirement")
            else {
                panic!("first retirement registration must own cleanup");
            };
            assert!(transfer.release_final_capture());
            retirement
                .released()
                .await
                .expect("successful final capture");
            if drop_owner {
                drop(retirement);
            } else {
                assert!(retirement.prepare(Err("cleanup failed".into())));
                assert_eq!(
                    transfer.wait_for_deferred_retirement_prepared().await,
                    Err("cleanup failed".into())
                );
                assert!(transfer.abandon_final_capture("inventory cleanup failed"));
                assert_eq!(
                    retirement.published().await,
                    Err("inventory cleanup failed".into())
                );
                retirement.complete(Err("cleanup failed".into()));
            }
            let error = transfer
                .wait_for_deferred_retirement()
                .await
                .expect_err("failed cleanup must fail final capture");
            assert!(error.contains(if drop_owner {
                "without a cleanup result"
            } else {
                "cleanup failed"
            }));
        }
    }

    #[tokio::test]
    async fn abandoned_capture_wakes_retirement_with_failure_and_cannot_be_released_as_success() {
        let state = Arc::new(State::default());
        let mut transfer = state.prepare().expect("prepare");
        assert!(transfer.commit());
        let RetirementRegistration::Owner(retirement) =
            state.defer_retirement().expect("committed retirement")
        else {
            panic!("first retirement registration must own cleanup");
        };

        assert!(transfer.abandon_final_capture("capture task was never polled"));
        assert!(!transfer.release_final_capture());
        assert_eq!(
            retirement.released().await,
            Err("capture task was never polled".into())
        );
        assert!(retirement.prepare(Err("source capture was abandoned".into())));
        assert_eq!(
            retirement.published().await,
            Err("capture task was never polled".into())
        );
        retirement.complete(Err("source capture was abandoned".into()));
        assert_eq!(
            transfer.wait_for_deferred_retirement().await,
            Err("source capture was abandoned".into())
        );
    }

    #[tokio::test]
    async fn provisional_success_is_overridden_by_abandonment_before_publication() {
        let state = Arc::new(State::default());
        let mut transfer = state.prepare().expect("prepare");
        assert!(transfer.commit());
        let RetirementRegistration::Owner(retirement) =
            state.defer_retirement().expect("committed retirement")
        else {
            panic!("first retirement registration must own cleanup");
        };

        assert!(transfer.release_final_capture());
        retirement.released().await.expect("provisional release");
        assert!(retirement.prepare(Ok(())));
        assert!(transfer.final_capture_publishable());
        assert!(transfer.abandon_final_capture("capture owner was cancelled"));
        assert!(!transfer.final_capture_publishable());
        assert!(!transfer.publish_final_capture());
        assert_eq!(
            retirement.published().await,
            Err("capture owner was cancelled".into())
        );
        retirement.complete(Err("capture owner was cancelled".into()));
        assert_eq!(
            transfer.wait_for_deferred_retirement().await,
            Err("capture owner was cancelled".into())
        );
    }

    #[tokio::test]
    async fn inventory_publication_owns_completion_after_the_cleanup_task_is_lost() {
        let state = Arc::new(State::default());
        let mut transfer = state.prepare().expect("prepare");
        assert!(transfer.commit());
        let RetirementRegistration::Owner(retirement) =
            state.defer_retirement().expect("committed retirement")
        else {
            panic!("first retirement registration must own cleanup");
        };

        assert!(transfer.release_final_capture());
        retirement.released().await.expect("provisional release");
        assert!(retirement.prepare(Ok(())));
        assert!(transfer.publish_final_capture_inventory());
        assert!(retirement.inventory_published());
        drop(retirement);
        transfer
            .wait_for_deferred_retirement()
            .await
            .expect("published inventory owns completed cleanup");
    }

    #[tokio::test]
    async fn failed_inventory_commit_overrides_an_early_member_completion() {
        let state = Arc::new(State::default());
        let mut transfer = state.prepare().expect("prepare");
        assert!(transfer.commit());
        let RetirementRegistration::Owner(retirement) =
            state.defer_retirement().expect("committed retirement")
        else {
            panic!("first retirement registration must own cleanup");
        };

        assert!(transfer.release_final_capture());
        retirement.released().await.expect("provisional release");
        assert!(retirement.prepare(Ok(())));
        assert!(transfer.publish_final_capture_inventory());
        retirement.complete(Ok(()));
        transfer
            .wait_for_deferred_retirement()
            .await
            .expect("first member completed before cohort failure");
        assert!(transfer.fail_final_capture_inventory("later member failed publication"));
        assert_eq!(
            transfer.wait_for_deferred_retirement().await,
            Err("later member failed publication".into())
        );
    }

    #[test]
    fn retirement_wins_atomically_against_an_uncommitted_preparation() {
        let state = Arc::new(State::default());
        let mut transfer = state.prepare().expect("prepare");

        assert!(state.defer_retirement().is_none());
        assert!(!transfer.valid());
        assert!(!transfer.commit_if(|| { panic!("retirement invalidated the host decision") }));
        drop(transfer);
        assert!(matches!(state.admit(), Err(Refusal::Transferred)));
        assert!(matches!(state.prepare(), Err(Refusal::Transferred)));
    }

    #[test]
    fn retirement_from_open_prevents_a_checkpoint_that_already_waits_outside_the_barrier() {
        let state = Arc::new(State::default());

        assert!(state.defer_retirement().is_none());
        assert!(matches!(state.prepare(), Err(Refusal::Transferred)));
        assert!(matches!(state.admit(), Err(Refusal::Transferred)));
    }

    #[test]
    fn concurrent_commit_and_retirement_have_exactly_one_owner() {
        for _ in 0..128 {
            let state = Arc::new(State::default());
            let mut transfer = state.prepare().expect("prepare");
            let barrier = std::sync::Barrier::new(2);
            std::thread::scope(|scope| {
                let retirement = scope.spawn(|| {
                    barrier.wait();
                    state.defer_retirement()
                });
                barrier.wait();
                let committed = transfer.commit();
                let retirement = retirement.join().expect("retirement thread");
                assert_eq!(committed, retirement.is_some());
                if committed {
                    assert!(matches!(retirement, Some(RetirementRegistration::Owner(_))));
                }
            });
        }
    }
}
