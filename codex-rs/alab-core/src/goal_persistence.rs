// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Failed goal persistence is backpressure, never permission for another model turn.
//! Dispatched writes finish their bookkeeping even when their observer disappears.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio::sync::oneshot;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Admission {
    Ready,
    Writing,
    Held,
}

#[derive(Debug)]
pub struct State(Mutex<Inner>, tokio::sync::watch::Sender<u64>);

/// Process-local evidence of one native owner's exact mutation boundary.
/// Accounting leaves it unchanged. Retaining it does not retain the owner.
#[derive(Clone, Debug)]
pub struct MutationEpoch {
    owner: Weak<State>,
    epoch: Arc<()>,
    initial: bool,
}

impl PartialEq for MutationEpoch {
    fn eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.owner, &other.owner) && Arc::ptr_eq(&self.epoch, &other.epoch)
    }
}

impl Eq for MutationEpoch {}

impl MutationEpoch {
    pub fn is_current(&self) -> bool {
        self.owner
            .upgrade()
            .is_some_and(|owner| owner.epoch_is_current(self))
    }

    pub fn belongs_to(&self, owner: &Arc<State>) -> bool {
        Weak::ptr_eq(&self.owner, &Arc::downgrade(owner))
    }

    /// Only a native owner which has seen no mutation can originate import custody.
    /// A duplicate may transfer that exact custody; it cannot mint another one.
    pub fn is_initial(&self) -> bool {
        self.initial
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutationReceipt {
    pub before: MutationEpoch,
    pub after: MutationEpoch,
}

#[cfg(test)]
#[path = "goal_mutation_epoch_tests.rs"]
mod mutation_epoch_tests;

impl Default for State {
    fn default() -> Self {
        Self(Mutex::default(), tokio::sync::watch::channel(0).0)
    }
}

/// One atomic passive view; failed admission can coexist with a live writer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub revision: u64,
    pub admission: Admission,
    pub writes_pending: bool,
}

#[derive(Debug, Default)]
struct Inner {
    generation: u64,
    mutation_epoch: Arc<()>,
    mutated: bool,
    revision: u64,
    next: u64,
    active: BTreeMap<u64, (u64, bool)>,
    failed: bool,
    ambiguous: bool,
    quarantined: BTreeMap<String, QuarantinedUsage>,
}

/// An unsupported raw goal replacement crossed an unacknowledged accounting
/// boundary. Keep the original observations; this is never replay authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuarantinedUsage {
    pub original_goal_id: String,
    pub elapsed_seconds: i64,
    pub descendant_tokens: i64,
}

/// Opaque equality identity for passive observer lifetimes, never write authority.
pub fn observation_incarnation() -> String {
    format!("{:032x}", rand::random::<u128>())
}

tokio::task_local! {
    static ROLLED_BACK: Arc<AtomicBool>;
}

/// Called only for a typed error proving that the additive write did not commit.
pub fn definitely_rolled_back() {
    let _ = ROLLED_BACK.try_with(|safe| safe.store(true, Ordering::SeqCst));
}

/// True only inside the owner that will finalize the dispatched accounting write.
pub fn owns_current_write() -> bool {
    ROLLED_BACK.try_with(|_| ()).is_ok()
}

impl State {
    fn epoch_locked(self: &Arc<Self>, inner: &Inner) -> MutationEpoch {
        MutationEpoch {
            owner: Arc::downgrade(self),
            epoch: Arc::clone(&inner.mutation_epoch),
            initial: !inner.mutated,
        }
    }

    pub fn mutation_epoch(self: &Arc<Self>) -> MutationEpoch {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.epoch_locked(&inner)
    }

    pub fn initial_mutation_epoch(self: &Arc<Self>) -> Option<MutationEpoch> {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (!inner.mutated).then(|| self.epoch_locked(&inner))
    }

    /// Owner and epoch equality are independent of admission: a healthy native
    /// validation future itself holds a non-mutating persistence operation.
    pub fn epoch_is_current(self: &Arc<Self>, epoch: &MutationEpoch) -> bool {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        epoch.belongs_to(self) && Arc::ptr_eq(&inner.mutation_epoch, &epoch.epoch)
    }

    /// Vanilla-mode goal intents still retire old enhanced recovery evidence.
    /// This passive fence neither owns a write nor changes native admission.
    pub fn revoke_mutation_epoch(&self) {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.mutation_epoch = Arc::new(());
        inner.mutated = true;
        self.changed(&mut inner);
    }

    pub fn changes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.1.subscribe()
    }

    pub fn snapshot(&self) -> Snapshot {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Snapshot {
            revision: inner.revision,
            admission: if inner.failed {
                Admission::Held
            } else if inner.active.is_empty() {
                Admission::Ready
            } else {
                Admission::Writing
            },
            writes_pending: !inner.active.is_empty(),
        }
    }

    fn changed(&self, inner: &mut Inner) {
        inner.revision = inner.revision.saturating_add(1);
        self.1.send_replace(inner.revision);
    }

    /// An additive owner has been dispatched; preparation/validation alone is
    /// not proof that a write will outlive its observer.
    pub fn accounting_write_pending(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active
            .values()
            .any(|(_, additive)| *additive)
    }

    pub fn quarantine_original_goal(&self, usage: QuarantinedUsage) {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .quarantined
            .entry(usage.original_goal_id.clone())
            .or_insert(usage);
        inner.failed = true;
        inner.ambiguous = true;
        self.changed(&mut inner);
    }

    /// Read-only evidence of the original boundary, never a reconciliation API.
    pub fn quarantined_usage(&self) -> Vec<QuarantinedUsage> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .quarantined
            .values()
            .cloned()
            .collect()
    }

    /// Publish before releasing the accounting semaphore, so a queued observer
    /// cannot replay a snapshot whose first commit has an unknown outcome.
    pub fn block_ambiguous(&self) {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.failed = true;
        inner.ambiguous = true;
        self.changed(&mut inner);
    }

    pub fn generation_is_current(&self, generation: u64) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .generation
            == generation
    }

    pub fn admission(&self) -> Admission {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inner.failed {
            Admission::Held
        } else if inner.active.is_empty() {
            Admission::Ready
        } else {
            Admission::Writing
        }
    }

    pub fn permits_admission(&self) -> bool {
        self.admission() == Admission::Ready
    }

    /// An ambiguous commit cannot be replayed as another additive usage update.
    pub fn permits_write(&self) -> bool {
        !self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .ambiguous
    }

    pub fn begin(self: &Arc<Self>) -> Write {
        self.begin_kind(false)
    }

    /// Revoke old stop custody before the external write can reach SQLite.
    pub fn begin_mutation(self: &Arc<Self>) -> Write {
        self.begin_kind(true)
    }

    fn begin_kind(self: &Arc<Self>, mutation: bool) -> Write {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let before = mutation.then(|| self.epoch_locked(&inner));
        if mutation {
            inner.generation = inner.generation.wrapping_add(1);
            inner.mutation_epoch = Arc::new(());
            inner.mutated = true;
        }
        inner.next = inner.next.wrapping_add(1);
        let id = inner.next;
        let generation = inner.generation;
        inner.active.insert(id, (generation, false));
        self.changed(&mut inner);
        Write {
            state: Arc::clone(self),
            id,
            generation,
            finished: false,
            additive: false,
            mutation,
            before,
            epoch: Arc::clone(&inner.mutation_epoch),
        }
    }
}

pub struct Write {
    state: Arc<State>,
    id: u64,
    generation: u64,
    finished: bool,
    additive: bool,
    mutation: bool,
    before: Option<MutationEpoch>,
    epoch: Arc<()>,
}

impl Write {
    fn mark_accounting(&mut self) {
        self.additive = true;
        if let Some((_, additive)) = self
            .state
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active
            .get_mut(&self.id)
        {
            *additive = true;
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn previous_mutation_epoch(&self) -> Option<MutationEpoch> {
        self.before.clone()
    }

    fn complete(&mut self, success: bool, safe: bool) -> Option<MutationReceipt> {
        let mut inner = self
            .state
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut receipt = None;
        if let Some((generation, _)) = inner.active.remove(&self.id)
            && generation == inner.generation
            && (!self.mutation || Arc::ptr_eq(&self.epoch, &inner.mutation_epoch))
        {
            if self.mutation {
                // Also revoke stops queued while this intent held the native
                // permit. Cancellation after COMMIT must fence them as well.
                inner.generation = inner.generation.wrapping_add(1);
                inner.mutation_epoch = Arc::new(());
                if success {
                    receipt = self.before.clone().map(|before| MutationReceipt {
                        before,
                        after: self.state.epoch_locked(&inner),
                    });
                }
            }
            if !success {
                inner.failed = true;
                inner.ambiguous |= !safe;
            } else if self.mutation && inner.quarantined.is_empty() {
                inner.failed = false;
                inner.ambiguous = false;
            }
        }
        self.state.changed(&mut inner);
        self.finished = true;
        receipt
    }

    pub fn commit_mutation(mut self) {
        assert!(
            self.mutation,
            "only a native mutation intent can acknowledge its commit"
        );
        let _ = self.complete(true, true);
    }

    /// Return the successful writer's exact settlement boundary under the same
    /// mutex. Sampling current state after settlement could bless a later write.
    pub fn commit_mutation_with_receipt(mut self) -> Option<MutationReceipt> {
        assert!(
            self.mutation,
            "only a native mutation intent can acknowledge its commit"
        );
        self.complete(true, true)
    }

    /// A failed stop remains a hold even if a later accounting write succeeds.
    pub fn finish(mut self, success: bool) {
        let _ = self.complete(success, true);
    }
}

impl Drop for Write {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.complete(false, !self.additive);
        }
    }
}

/// Start ownership before dispatch; dropping the receiver never cancels the write.
pub async fn owned<T, E, F>(
    state: Arc<State>,
    work: F,
) -> Result<Result<T, E>, oneshot::error::RecvError>
where
    T: Send + 'static,
    E: Send + 'static,
    F: Future<Output = Result<T, E>> + Send + 'static,
{
    let mut write = state.begin();
    write.mark_accounting();
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(async move {
        let safe = Arc::new(AtomicBool::new(false));
        let result = ROLLED_BACK.scope(Arc::clone(&safe), work).await;
        write.complete(result.is_ok(), safe.load(Ordering::SeqCst));
        let _ = sender.send(result);
    });
    receiver.await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn goal_runtime_status_wakes_for_settlement_and_distinguishes_sticky_failure_from_live_work()
     {
        let state = Arc::new(State::default());
        let mut changes = state.changes();
        let failed = state.begin();
        let in_flight = state.begin();
        failed.finish(false);
        changes.changed().await.unwrap();
        let active = state.snapshot();
        assert_eq!(active.admission, Admission::Held);
        assert!(active.writes_pending);
        changes.borrow_and_update();
        in_flight.finish(true);
        changes.changed().await.unwrap();
        let held = state.snapshot();
        assert_eq!(held.admission, Admission::Held);
        assert!(!held.writes_pending);
        assert!(held.revision > active.revision);
        changes.borrow_and_update();
        state.begin_mutation().commit_mutation();
        changes.changed().await.unwrap();
        let ready = state.snapshot();
        assert_eq!(ready.admission, Admission::Ready);
        assert!(!ready.writes_pending);
        assert!(ready.revision > held.revision);
    }

    #[tokio::test]
    async fn observer_cancellation_cannot_cancel_commit_bookkeeping() {
        let state = Arc::new(State::default());
        let (release, wait) = oneshot::channel();
        let (committed, commit) = oneshot::channel();
        let observer = tokio::spawn(owned(Arc::clone(&state), async move {
            wait.await.unwrap();
            committed.send(()).unwrap();
            Ok::<_, ()>(())
        }));
        tokio::task::yield_now().await;
        assert!(!state.permits_admission());
        assert!(state.accounting_write_pending());
        observer.abort();
        let _ = observer.await;
        release.send(()).unwrap();
        commit.await.unwrap();
        tokio::task::yield_now().await;
        assert!(state.permits_admission());
        assert!(!state.accounting_write_pending());
    }

    #[tokio::test]
    async fn failed_write_with_successful_reads_never_admits_again() {
        let state = Arc::new(State::default());
        assert!(
            owned(Arc::clone(&state), async {
                definitely_rolled_back();
                Err::<(), _>("injected SQLITE_FULL")
            })
            .await
            .unwrap()
            .is_err()
        );
        for _ in 0..100 {
            let durable_status = "active"; // Reads succeeding cannot acknowledge a write.
            assert_eq!(durable_status, "active");
            assert!(!state.permits_admission());
        }
        assert!(state.permits_write());
        assert!(
            owned(Arc::clone(&state), async { Ok::<_, ()>(()) })
                .await
                .unwrap()
                .is_ok()
        );
        assert!(!state.permits_admission());
        state.begin_mutation().commit_mutation();
        assert!(state.permits_admission());
    }

    #[tokio::test]
    async fn ambiguous_commit_is_held_without_replay() {
        let state = Arc::new(State::default());
        let _ = owned(Arc::clone(&state), async {
            Err::<(), _>("injected commit I/O")
        })
        .await;
        assert!(!state.permits_write());
        assert!(!state.permits_admission());
    }

    #[test]
    fn successful_accounting_does_not_erase_failed_stop() {
        let state = Arc::new(State::default());
        state.begin().finish(false);
        state.begin().finish(true);
        assert!(!state.permits_admission());
    }

    #[test]
    fn old_completion_cannot_overwrite_new_mutation_or_stop() {
        let state = Arc::new(State::default());
        let old = state.begin();
        state.begin_mutation().commit_mutation(); // A fresh explicit mutation also supersedes same-ID resumes.
        let new = state.begin();
        old.finish(false);
        assert!(!state.permits_admission()); // New operation still owns admission.
        new.finish(false);
        assert!(!state.permits_admission());
    }
    #[test]
    fn cancelled_mutation_fences_stops_queued_after_its_intent() {
        let state = Arc::new(State::default());
        let mutation = state.begin_mutation();
        let queued_stop = state.begin();
        assert!(state.generation_is_current(queued_stop.generation()));
        drop(mutation); // Includes lost acknowledgement after a native COMMIT.
        assert!(!state.generation_is_current(queued_stop.generation()));
        queued_stop.finish(true);
        assert!(!state.permits_admission());
        state.begin_mutation().commit_mutation();
        assert!(state.permits_admission());
    }

    #[tokio::test]
    async fn queued_writer_rechecks_ambiguity_before_replaying_a_delta() {
        let state = Arc::new(State::default());
        let accounting = Arc::new(tokio::sync::Semaphore::new(1));
        let held = accounting.acquire().await.unwrap();
        let second = tokio::spawn({
            let accounting = Arc::clone(&accounting);
            let state = Arc::clone(&state);
            async move {
                let _permit = accounting.acquire().await.unwrap();
                state.permits_write()
            }
        });
        tokio::task::yield_now().await;
        // The first writer may already have committed but lost its response.
        // Publish while its accounting permit still excludes the queued writer.
        state.block_ambiguous();
        drop(held);
        assert!(!second.await.unwrap());
        assert!(!state.permits_admission());
    }
    #[test]
    fn healthy_pending_write_is_distinct_from_a_failed_write_hold() {
        let state = Arc::new(State::default());
        assert_eq!(state.admission(), Admission::Ready);
        let pending = state.begin();
        assert_eq!(state.admission(), Admission::Writing);
        pending.finish(true);
        assert_eq!(state.admission(), Admission::Ready);
        state.begin().finish(false);
        let later = state.begin();
        assert_eq!(state.admission(), Admission::Held);
        later.finish(true);
        assert_eq!(state.admission(), Admission::Held);
    }
}
