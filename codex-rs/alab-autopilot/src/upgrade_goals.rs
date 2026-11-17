// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual native goal guards for the exact prepared Core member inventory.
//! Snapshots carry state only; they grant neither SDK nor continuation authority.
use codex_alab_core::upgrade_barrier::Transfer;
use codex_core::CodexThread;
use codex_goal_extension::GoalRuntimeHandle;
use codex_protocol::ThreadId;
use codex_state::ThreadGoal;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::OnceLock;

struct Entry {
    thread: ThreadId,
    owner: Arc<CodexThread>,
    runtime: Arc<GoalRuntimeHandle>,
    snapshot: Option<ThreadGoal>,
    transfer: Transfer,
}

pub struct PreparedGoals {
    entries: Vec<Entry>,
}

impl Drop for PreparedGoals {
    fn drop(&mut self) {
        // A committed inventory which is abandoned before capture still owns
        // every deferred runtime retirement. Release it at the ownership edge;
        // uncommitted transfers retain their ordinary rollback-on-drop behavior.
        for entry in &self.entries {
            if entry.transfer.committed() {
                let _ = entry
                    .transfer
                    .abandon_final_capture("committed goal capture was dropped before completion");
            }
        }
    }
}

impl PreparedGoals {
    /// Use the exact handles retained by PreparedUpgradeForest. No missing
    /// runtime or unreadable row is interpreted as an absent goal.
    pub async fn prepare(threads: &[Arc<CodexThread>]) -> Result<Self, String> {
        if threads.is_empty() {
            return Err("goal checkpoint inventory is empty".into());
        }
        let mut seen = HashSet::new();
        let mut entries = Vec::new();
        for thread in threads {
            let id = thread.exec_thread_id();
            if !seen.insert(id) {
                return Err("goal checkpoint inventory repeats a member".into());
            }
            let runtime = thread
                .thread_extension_data()
                .get::<GoalRuntimeHandle>()
                .ok_or("native goal runtime is unavailable for an inventory member")?;
            let (snapshot, transfer) = runtime.alab_upgrade_goal_state_checkpoint(id).await?;
            entries.push(Entry {
                thread: id,
                owner: thread.clone(),
                runtime,
                snapshot,
                transfer,
            });
        }
        let prepared = Self { entries };
        if !prepared.valid() {
            return Err("goal checkpoint inventory changed during preparation".into());
        }
        Ok(prepared)
    }

    pub fn valid(&self) -> bool {
        self.entries.iter().all(|entry| entry.transfer.valid())
    }
    pub(crate) async fn invalidated(&self) {
        let mut waits = self
            .entries
            .iter()
            .map(|entry| Box::pin(entry.transfer.invalidated()))
            .collect::<Vec<_>>();
        std::future::poll_fn(move |context| {
            for wait in &mut waits {
                if std::future::Future::poll(wait.as_mut(), context).is_ready() {
                    return std::task::Poll::Ready(());
                }
            }
            std::task::Poll::Pending
        })
        .await;
    }

    pub fn snapshots(&self) -> impl Iterator<Item = (ThreadId, Option<&ThreadGoal>)> {
        self.entries
            .iter()
            .map(|entry| (entry.thread, entry.snapshot.as_ref()))
    }

    /// Hold every actual native goal admission through one synchronous decision.
    /// A refused member leaves all earlier guards revocable for ordinary work.
    pub fn commit_if(&mut self, decide: impl FnOnce() -> bool) -> bool {
        let mut decision = Some(decide);
        commit_entries(&mut self.entries, &mut || {
            decision.take().is_some_and(|decide| decide())
        })
    }

    /// Join all goal guards with the exact Core forest and actual source-reader
    /// catalog. The caller still supplies its final native TUI/host decision.
    pub fn commit_forest_if(
        &mut self,
        forest: &mut codex_core::PreparedUpgradeForest,
        decide: impl FnOnce() -> bool,
    ) -> Result<bool, String> {
        let expected = forest.threads();
        if expected.len() != self.entries.len()
            || expected.iter().any(|thread| {
                !self
                    .entries
                    .iter()
                    .any(|entry| Arc::ptr_eq(thread, &entry.owner))
            })
        {
            return Err("goal checkpoints differ from the native forest inventory".into());
        }
        let mut refusal = None;
        let committed = self.commit_if(|| match forest.commit_with_readers_if(decide) {
            Ok(committed) => committed,
            Err(error) => {
                refusal = Some(error);
                false
            }
        });
        refusal.map_or(Ok(committed), Err)
    }

    /// Retain the exact committed Core and goal owners through final accounting.
    pub fn into_capture(
        self,
        forest: Arc<codex_core::CommittedUpgradeForest>,
    ) -> Result<Arc<GoalForestCapture>, String> {
        if !forest.source_readers_committed()
            || self.entries.iter().any(|entry| !entry.transfer.committed())
            || forest.threads().len() != self.entries.len()
            || forest.threads().iter().any(|thread| {
                !self
                    .entries
                    .iter()
                    .any(|entry| Arc::ptr_eq(thread, &entry.owner))
            })
        {
            return Err(
                "final goal capture requires the exact committed forest and readers".into(),
            );
        }
        let retirement = self
            .entries
            .first()
            .and_then(|entry| entry.runtime.alab_upgrade_retirement_coordinator())
            .ok_or("final goal capture has no registered retirement coordinator")?;
        if self.entries.iter().any(|entry| {
            entry
                .runtime
                .alab_upgrade_retirement_coordinator()
                .is_none_or(|current| !retirement.same_owner(&current))
        }) {
            return Err("final goal capture spans different runtime registries".into());
        }
        Ok(Arc::new(GoalForestCapture {
            goals: self,
            forest,
            retirement,
            owner_claimed: std::sync::atomic::AtomicBool::new(false),
            outcome: OnceLock::new(),
            #[cfg(test)]
            capture_task: std::sync::Mutex::new(None),
            #[cfg(test)]
            capture_task_gate: std::sync::Mutex::new(None),
            #[cfg(test)]
            capture_release_gate: std::sync::Mutex::new(None),
        }))
    }
}

pub struct FinalGoalForest {
    pub forest: Arc<codex_core::UpgradeForestSnapshot>,
    pub goals: Vec<FinalGoalState>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalGoalState {
    pub thread: ThreadId,
    pub goal: Option<ThreadGoal>,
    pub clock: Option<codex_alab_core::goal_clock::Stamp>,
}
pub(crate) type CaptureResult = Result<Arc<FinalGoalForest>, String>;
pub(crate) type CaptureFuture = Pin<Box<dyn Future<Output = CaptureResult> + Send + 'static>>;

/// An owned source operation; losing a waiter cannot skip final native accounting.
pub struct GoalForestCapture {
    goals: PreparedGoals,
    forest: Arc<codex_core::CommittedUpgradeForest>,
    retirement: Arc<codex_goal_extension::UpgradeRetirementCoordinator>,
    owner_claimed: std::sync::atomic::AtomicBool,
    outcome: OnceLock<tokio::sync::watch::Receiver<Option<CaptureResult>>>,
    #[cfg(test)]
    capture_task: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    #[cfg(test)]
    capture_task_gate: std::sync::Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
    #[cfg(test)]
    capture_release_gate: std::sync::Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
}

/// Armed synchronously before spawning the one source task. Tokio may discard
/// an unpolled task during runtime teardown, so the guard owns every provisional
/// release until the inventory-wide non-awaiting publication point.
pub(crate) struct GoalCaptureOwner {
    capture: Arc<GoalForestCapture>,
    published: bool,
}

impl Drop for GoalCaptureOwner {
    fn drop(&mut self) {
        if !self.published {
            self.capture.abandon_final_capture(
                "final goal capture task ended without a transferable snapshot",
            );
        }
    }
}

impl GoalCaptureOwner {
    pub(crate) async fn prepare(&mut self) -> CaptureResult {
        self.capture.prepare_final_capture().await.map(Arc::new)
    }

    /// Publish all prepared barriers and deliver the cached result while the
    /// exact runtime registry and persistence-gap registry remain excluded.
    /// There is deliberately no cancellation point in this method.
    pub(crate) fn publish(
        &mut self,
        forest: Arc<FinalGoalForest>,
        mut deliver: impl FnMut(Arc<FinalGoalForest>),
    ) -> Result<(), String> {
        if self.published {
            return Err("final goal capture was already published".into());
        }
        if self
            .capture
            .goals
            .entries
            .iter()
            .any(|entry| !entry.transfer.final_capture_publishable())
        {
            return Err("final goal cleanup is not prepared for publication".into());
        }
        let runtimes = self
            .capture
            .goals
            .entries
            .iter()
            .map(|entry| Arc::clone(&entry.runtime))
            .collect::<Vec<_>>();
        let entries = &self.capture.goals.entries;
        let mut delivered = false;
        let publication = self.capture.retirement.publish(&runtimes, &mut || {
            if entries
                .iter()
                .any(|entry| !entry.transfer.final_capture_publishable())
            {
                return Err("final goal cleanup changed before batch publication".into());
            }
            for entry in entries {
                if !entry.transfer.publish_final_capture_inventory() {
                    return Err(format!(
                        "{} lost its prepared final-capture publication",
                        entry.thread
                    ));
                }
            }
            deliver(Arc::clone(&forest));
            delivered = true;
            Ok(())
        });
        if let Err(error) = publication {
            let reason = format!("final goal inventory publication failed: {error}");
            for entry in entries {
                let _ = entry.transfer.fail_final_capture_inventory(reason.clone());
            }
            return Err(error);
        }
        if !delivered {
            return Err("final goal publication did not deliver its result".into());
        }
        self.published = true;
        Ok(())
    }
}

impl GoalForestCapture {
    pub async fn capture(self: &Arc<Self>) -> CaptureResult {
        self.begin_capture().await
    }

    /// Claim the sole capture owner before its task can be discarded without a
    /// first poll. The outer source capture uses this directly so terminal and
    /// goal publication have one cancellation boundary.
    pub(crate) fn claim_owner(self: &Arc<Self>) -> Result<GoalCaptureOwner, String> {
        self.owner_claimed
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .map_err(|_| "final goal capture already has an owner".to_owned())?;
        Ok(GoalCaptureOwner {
            capture: Arc::clone(self),
            published: false,
        })
    }

    /// Start ownership synchronously and return a waiter detached from its
    /// caller. Direct goal-capture callers retain one cached outcome; the outer
    /// source capture instead claims this owner inside its own task boundary.
    pub(crate) fn begin_capture(self: &Arc<Self>) -> CaptureFuture {
        let owner = self.clone();
        let mut outcome = self
            .outcome
            .get_or_init(move || {
                let (send, receive) = tokio::sync::watch::channel(None);
                #[cfg(test)]
                let task_owner = Arc::clone(&owner);
                let capture_owner = owner.claim_owner();
                let task = tokio::spawn(async move {
                    let Ok(mut capture_owner) = capture_owner else {
                        send.send_replace(Some(Err(
                            "final goal capture already has an owner".into()
                        )));
                        return;
                    };
                    #[cfg(test)]
                    let gate = capture_owner
                        .capture
                        .capture_task_gate
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .take();
                    #[cfg(test)]
                    if let Some((entered, release)) = gate {
                        let _ = entered.send(());
                        let _ = release.await;
                    }
                    match capture_owner.prepare().await {
                        Ok(forest) => {
                            if let Err(error) = capture_owner.publish(forest, |forest| {
                                send.send_replace(Some(Ok(forest)));
                            }) {
                                send.send_replace(Some(Err(error)));
                            }
                        }
                        Err(error) => {
                            send.send_replace(Some(Err(error)));
                        }
                    }
                });
                #[cfg(test)]
                {
                    *task_owner
                        .capture_task
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(task);
                }
                #[cfg(not(test))]
                drop(task);
                receive
            })
            .clone();
        Box::pin(async move {
            loop {
                if let Some(result) = outcome.borrow_and_update().clone() {
                    return result;
                }
                outcome
                    .changed()
                    .await
                    .map_err(|_| "final goal capture ended without a result".to_owned())?;
            }
        })
    }

    #[cfg(test)]
    pub(crate) fn alab_debug_gate_after_final_release(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, entered_rx) = tokio::sync::oneshot::channel();
        let (release, release_rx) = tokio::sync::oneshot::channel();
        *self
            .capture_release_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((entered, release_rx));
        (entered_rx, release)
    }

    #[cfg(test)]
    pub(crate) fn alab_debug_gate_capture_task(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, entered_rx) = tokio::sync::oneshot::channel();
        let (release, release_rx) = tokio::sync::oneshot::channel();
        *self
            .capture_task_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((entered, release_rx));
        (entered_rx, release)
    }

    #[cfg(test)]
    pub(crate) async fn alab_debug_abort_capture_task(&self) {
        let task = self
            .capture_task
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .expect("final goal capture task was started");
        task.abort();
        assert!(
            matches!(task.await, Err(error) if error.is_cancelled()),
            "the gated final goal capture task is cancelled"
        );
    }

    fn abandon_final_capture(&self, reason: &str) {
        for entry in &self.goals.entries {
            let _ = entry.transfer.abandon_final_capture(reason);
        }
    }

    async fn prepare_final_capture(&self) -> Result<FinalGoalForest, String> {
        let result = async {
            // This joins all source session producers AND native source reader ACKs.
            let forest = self.forest.capture().await?;
            let mut goals = Vec::new();
            for entry in &self.goals.entries {
                let (goal, clock) = entry
                    .runtime
                    .alab_final_upgrade_goal_state(entry.thread, &entry.transfer)
                    .await?;
                goals.push(FinalGoalState {
                    thread: entry.thread,
                    goal,
                    clock,
                });
            }
            Ok(FinalGoalForest { forest, goals })
        }
        .await;

        let forest = match result {
            Ok(forest) => forest,
            Err(error) => {
                self.abandon_final_capture(&format!("final goal capture failed: {error}"));
                return Err(error);
            }
        };

        // Phase one lets every deferred owner finish failure-prone drains while
        // retaining every registry key. Abandonment may still override it.
        let mut release_errors = Vec::new();
        for entry in &self.goals.entries {
            if !entry.transfer.release_final_capture() {
                release_errors.push(format!(
                    "{} lost its committed runtime transfer during final release",
                    entry.thread
                ));
            }
        }
        if !release_errors.is_empty() {
            let error = release_errors.join("; ");
            self.abandon_final_capture(&error);
            return Err(error);
        }

        #[cfg(test)]
        let gate = self
            .capture_release_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        #[cfg(test)]
        if let Some((entered, release)) = gate {
            let _ = entered.send(());
            let _ = release.await;
        }

        let mut cleanup_errors = Vec::new();
        for entry in &self.goals.entries {
            if let Err(error) = entry.transfer.wait_for_deferred_retirement_prepared().await {
                cleanup_errors.push(format!(
                    "{} deferred runtime retirement preparation failed: {error}",
                    entry.thread
                ));
            }
        }
        if cleanup_errors.is_empty() {
            Ok(forest)
        } else {
            let error = cleanup_errors.join("; ");
            self.abandon_final_capture(&error);
            Err(error)
        }
    }
}

fn commit_entries(entries: &mut [Entry], decide: &mut dyn FnMut() -> bool) -> bool {
    let Some((entry, remaining)) = entries.split_first_mut() else {
        return decide();
    };
    entry
        .runtime
        .alab_commit_goal_state_upgrade(&mut entry.transfer, || commit_entries(remaining, decide))
}

#[path = "upgrade_goal_state.rs"]
mod state;
pub use state::GoalForestState;
