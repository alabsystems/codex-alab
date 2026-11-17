// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One Source scheduling owner on the SAME physical backend/control binding.
//! H1 accepts owned work before awaiting the actual admission drain. Neither
//! observer loss nor an old frontend's EOF can acknowledge/release this hold.
use super::*;
use codex_alab_core::retained_goal_gate::Gate;
use codex_alab_core::retained_goal_gate::Hold;
use codex_alab_core::retained_goal_gate::Owner;
use std::collections::BTreeMap;
#[path = "frontend_retained_runtime_frontier.rs"]
mod runtime;
#[path = "frontend_source_transition.rs"]
mod source_selection;
#[path = "frontend_retained_source_shutdown.rs"]
mod source_shutdown;
pub use runtime::ClosedCustody;
pub use runtime::ConfiguredRuntimeAck;
pub use runtime::ConfiguredRuntimeActivation;
pub use runtime::ConfiguredRuntimeCommit;
pub use runtime::ConfiguredRuntimeFrontier;
pub use runtime::PendingHold;
pub use runtime::RuntimeAck;
pub use runtime::RuntimeActivation;
pub use runtime::RuntimeCommit;
pub use runtime::RuntimeFrontier;
pub use runtime::RuntimeObservationError;
pub use source_selection::HeldSourceTransition;
pub use source_selection::RegisteredSourceTransition;
pub use source_selection::SelectionAck;
pub use source_selection::SelectionCommit;
pub use source_selection::SelectionFrontier;
pub use source_selection::SelectionObservation;
pub use source_selection::SelectionReleased;
pub use source_selection::SourceTransition;
pub use source_shutdown::SourceShutdown;

const MAX_REPLACEMENTS: usize = 4096;
const MAX_ACTIVE_HOLDS: usize = 128;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Accepted,
    Held,
    Acknowledged,
    Unknown,
}
struct AcceptedHold {
    order: u64,
    nonce: [u8; 32],
    deadline: tokio::time::Instant,
    hold: Mutex<Option<Arc<Hold>>>,
    transferred: std::sync::atomic::AtomicBool,
    phase: watch::Sender<Phase>,
}
struct State {
    next: u64,
    initial: Option<Arc<Hold>>,
    initial_transferred: bool,
    replacements: BTreeMap<[u8; 32], Arc<AcceptedHold>>,
    origins: BTreeMap<[u8; 32], crate::frontend_control::retained_origin::WeakOrigin>,
    runtimes: BTreeMap<String, Arc<runtime::Record>>,
    source_selections: BTreeMap<String, Arc<source_selection::Record>>,
}
pub(in crate::frontend_control) struct GoalOwner {
    owner: Owner,
    gate: Gate,
    controller: Arc<Controller>,
    withdrawal: watch::Receiver<bool>,
    state: Mutex<State>,
    changed: watch::Sender<u64>,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    // Shared by Runtime release, Source-selection release and the genuine
    // NoPrimary closed-hold transfer companion. Never a Native permission.
    mutation: tokio::sync::Semaphore,
    joins: tokio::sync::Semaphore,
    stopping: watch::Sender<bool>,
    // New Source API admission only. Existing exact retirement still uses the
    // unchanged actual controller/withdrawal live() checks through cleanup.
    source_closing: std::sync::atomic::AtomicBool,
    failed: std::sync::atomic::AtomicBool,
}

impl GoalOwner {
    /// Call exactly once after the real original control bind, before its
    /// receive-loop spawn. The factory uses THIS gate before first Core start.
    pub(super) fn new(controller: Arc<Controller>, withdrawal: watch::Receiver<bool>) -> Arc<Self> {
        let (owner, gate, initial) = Owner::new_held();
        let (changed, _) = watch::channel(0);
        let (stopping, _) = watch::channel(false);
        Arc::new(Self {
            owner,
            gate,
            controller,
            withdrawal,
            state: Mutex::new(State {
                next: 0,
                initial: Some(Arc::new(initial)),
                initial_transferred: false,
                replacements: BTreeMap::new(),
                origins: BTreeMap::new(),
                runtimes: BTreeMap::new(),
                source_selections: BTreeMap::new(),
            }),
            changed,
            tasks: Mutex::new(Vec::new()),
            mutation: tokio::sync::Semaphore::new(1),
            joins: tokio::sync::Semaphore::new(1),
            stopping,
            source_closing: std::sync::atomic::AtomicBool::new(false),
            failed: std::sync::atomic::AtomicBool::new(false),
        })
    }
    pub(super) fn gate(&self) -> Gate {
        self.gate.clone()
    }
    pub(super) fn withdraw(&self) {
        self.owner.withdraw();
        self.stopping.send_replace(true);
    }
    pub(super) async fn shutdown(&self) -> io::Result<()> {
        self.withdraw();
        let _joins = self.joins.acquire().await.map_err(|_| unavailable())?;
        let mut failed = false;
        loop {
            let task = self.tasks.lock().map_err(|_| unavailable())?.pop();
            let Some(task) = task else {
                break;
            };
            let mut owned = TaskLease {
                tasks: &self.tasks,
                task: Some(task),
            };
            if owned.task.as_mut().ok_or_else(unavailable)?.await.is_err() {
                failed = true;
                self.failed
                    .store(true, std::sync::atomic::Ordering::Release);
            }
            owned.task.take();
        }
        if failed || self.failed.load(std::sync::atomic::Ordering::Acquire) {
            Err(unavailable())
        } else {
            Ok(())
        }
    }

    /// No caller-owned future is the worker. Duplicate physical delivery joins
    /// the same accepted operation/nonce and never submits a second hold.
    pub(super) fn accept(self: &Arc<Self>, frame: super::control_frame::Hold) -> io::Result<()> {
        self.live()?;
        let mut tasks = self.tasks.lock().map_err(|_| unavailable())?;
        let record = {
            let mut state = self.state.lock().map_err(|_| unavailable())?;
            if let Some(original) = state.replacements.get(&frame.operation) {
                return if original.nonce == frame.nonce {
                    Ok(())
                } else {
                    Err(unavailable())
                };
            }
            if state.replacements.len() >= MAX_REPLACEMENTS
                || state
                    .replacements
                    .values()
                    .filter(|hold| {
                        *hold.phase.borrow() != Phase::Acknowledged
                            || hold.hold.lock().map_or(true, |hold| hold.is_some())
                    })
                    .count()
                    >= MAX_ACTIVE_HOLDS
            {
                return Err(unavailable());
            }
            let order = state.next.checked_add(1).ok_or_else(unavailable)?;
            state.next = order;
            let (phase, _) = watch::channel(Phase::Accepted);
            let record = Arc::new(AcceptedHold {
                order,
                nonce: frame.nonce,
                deadline: frame.deadline,
                hold: Mutex::new(None),
                transferred: std::sync::atomic::AtomicBool::new(false),
                phase,
            });
            state.replacements.insert(frame.operation, record.clone());
            record
        };
        // Preguard exists before spawn: panic or unpolled task destruction
        // preserves Unknown and permanently closes fresh Source admission.
        let guard = Guard {
            owner: self.clone(),
            record: record.clone(),
            completed: false,
        };
        self.notify(); // Accepted physical H1 is visible to the real NoPrimary keeper.
        let worker = self.clone();
        tasks.push(tokio::spawn(async move {
            let mut guard = guard;
            let result = tokio::select! {
                biased;
                _ = worker.withdrawn() => Err(unavailable()),
                result = tokio::time::timeout_at(frame.deadline, worker.acquire_and_ack(&frame, &record)) =>
                    result.map_err(|_| io::Error::from(io::ErrorKind::TimedOut)).and_then(std::convert::identity),
            };
            if result.is_ok() { guard.completed = true; }
        }));
        Ok(())
    }

    async fn acquire_and_ack(
        &self,
        frame: &super::control_frame::Hold,
        accepted: &AcceptedHold,
    ) -> io::Result<()> {
        let mut changed = self.changed.subscribe();
        loop {
            self.live()?;
            // Native may request the next H1 just after it observed the prior
            // UI frontier, while Source is still finishing the exact release.
            // Only that real Source owner release may wake this waiter.
            let previous_held = {
                let state = self.state.lock().map_err(|_| unavailable())?;
                state.initial.is_some()
                    || state.replacements.iter().any(|(operation, record)| {
                        operation != &frame.operation
                            && (record.hold.lock().map_or(true, |hold| hold.is_some())
                                || (record.order < accepted.order
                                    && *record.phase.borrow() != Phase::Acknowledged))
                    })
            };
            let transferred = accepted.hold.lock().map_err(|_| unavailable())?.is_some();
            if transferred || !previous_held {
                break;
            }
            changed.changed().await.map_err(|_| unavailable())?;
        }
        let transferred = accepted.hold.lock().map_err(|_| unavailable())?.is_some();
        if !transferred {
            let hold = self.owner.hold().await.map_err(|_| unavailable())?;
            // No await between actual drain and storing the exact Hold.
            *accepted.hold.lock().map_err(|_| unavailable())? = Some(Arc::new(hold));
        }
        // A genuine configured NoPrimary path can move the SAME closed/drained
        // Hold here. This original worker alone still owns its physical ACK.
        accepted.phase.send_replace(Phase::Held);
        self.live()?;
        let operation = std::str::from_utf8(&frame.operation).map_err(|_| unavailable())?;
        let nonce = std::str::from_utf8(&frame.nonce).map_err(|_| unavailable())?;
        control(
            &self.controller,
            "controller-retained-frontier-ack",
            operation,
            Some(nonce),
        )
        .await?;
        self.live()?;
        accepted.phase.send_replace(Phase::Acknowledged);
        Ok(())
    }

    fn live(&self) -> io::Result<()> {
        if *self.stopping.borrow()
            || *self.withdrawal.borrow()
            || self.withdrawal.has_changed().is_err()
        {
            Err(unavailable())
        } else {
            Ok(())
        }
    }
    async fn withdrawn(&self) {
        let mut withdrawal = self.withdrawal.clone();
        let mut stopping = self.stopping.subscribe();
        loop {
            if self.live().is_err() {
                return;
            }
            tokio::select! {
                _ = stopping.changed() => {}
                _ = withdrawal.changed() => {}
            }
        }
    }
}

struct Guard {
    owner: Arc<GoalOwner>,
    record: Arc<AcceptedHold>,
    completed: bool,
}
struct TaskLease<'a> {
    tasks: &'a Mutex<Vec<tokio::task::JoinHandle<()>>>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl Drop for TaskLease<'_> {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            self.tasks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(task);
        }
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        if !self.completed {
            self.record.phase.send_replace(Phase::Unknown);
            self.owner
                .failed
                .store(true, std::sync::atomic::Ordering::Release);
            self.owner.withdraw();
        }
    }
}

/// The owned worker may already be in shutdown's join set. It must never
/// wait for the lease held by that shutdown observer while shutdown joins it.
/// A busy lease only defers housekeeping; actual shutdown still owns every
/// handle. Each popped completed handle is restored on cancelled observation.
async fn reap_ready_source_tasks(
    tasks: &Mutex<Vec<tokio::task::JoinHandle<()>>>,
    joins: &tokio::sync::Semaphore,
) -> io::Result<()> {
    let _joins = match joins.try_acquire() {
        Ok(permit) => permit,
        Err(tokio::sync::TryAcquireError::NoPermits) => return Ok(()),
        Err(tokio::sync::TryAcquireError::Closed) => return Err(unavailable()),
    };
    loop {
        let task = {
            let mut tasks = tasks.lock().map_err(|_| unavailable())?;
            tasks
                .iter()
                .position(tokio::task::JoinHandle::is_finished)
                .map(|index| tasks.swap_remove(index))
        };
        let Some(task) = task else {
            return Ok(());
        };
        let mut owned = TaskLease {
            tasks,
            task: Some(task),
        };
        let result = owned.task.as_mut().ok_or_else(unavailable)?.await;
        owned.task.take();
        result.map_err(|_| unavailable())?;
    }
}

#[cfg(test)]
#[path = "frontend_retained_task_reap_tests.rs"]
mod source_task_reap_tests;

impl GoalOwner {
    pub(in crate::frontend_control) async fn observe_reader_origin(
        &self,
        origin: &crate::frontend_control::retained_origin::AcceptedOrigin,
    ) -> io::Result<crate::frontend_control::retained_origin::CurrentOrigin> {
        self.check_reader_origin(origin)?;
        let observed = origin.observe_current().await?;
        self.check_reader_origin(origin)?;
        if !observed.same_receipt(origin) {
            return Err(unavailable());
        }
        Ok(observed)
    }
    fn check_reader_origin(
        &self,
        origin: &crate::frontend_control::retained_origin::AcceptedOrigin,
    ) -> io::Result<()> {
        self.admitting_source()?;
        if !origin.controlled_by(&self.controller) {
            return Err(unavailable());
        }
        let state = self.state.lock().map_err(|_| unavailable())?;
        if !state
            .origins
            .get(&origin.attachment())
            .and_then(crate::frontend_control::retained_origin::WeakOrigin::upgrade)
            .is_some_and(|actual| actual.same(origin))
        {
            return Err(unavailable());
        }
        Ok(())
    }
}
