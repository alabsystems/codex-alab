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
}

#[derive(Debug, Default)]
pub struct State {
    inner: Mutex<Inner>,
    changed: tokio::sync::Notify,
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
        if matches!(inner.phase, Phase::Committed(_)) {
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

    /// The final synchronous handoff decision. Committed runtimes refuse new
    /// admissions instead of leaving a continuation waiting through teardown.
    pub fn commit(&mut self) -> bool {
        self.commit_if(|| true)
    }

    /// Linearize the native TUI/host decision with native admission. A refused
    /// decision leaves rollback available; an invalidated transfer never calls it.
    pub fn commit_if(&mut self, decide: impl FnOnce() -> bool) -> bool {
        let mut inner = self.state.inner();
        if inner.phase != Phase::Prepared(self.generation) || inner.waiting != 0 {
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
}
