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
        Ok(Arc::new(GoalForestCapture {
            goals: self,
            forest,
            outcome: OnceLock::new(),
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
type CaptureResult = Result<Arc<FinalGoalForest>, String>;

/// An owned source operation; losing a waiter cannot skip final native accounting.
pub struct GoalForestCapture {
    goals: PreparedGoals,
    forest: Arc<codex_core::CommittedUpgradeForest>,
    outcome: OnceLock<tokio::sync::watch::Receiver<Option<CaptureResult>>>,
}
impl GoalForestCapture {
    pub async fn capture(self: &Arc<Self>) -> CaptureResult {
        let owner = self.clone();
        let mut outcome = self
            .outcome
            .get_or_init(move || {
                let (send, receive) = tokio::sync::watch::channel(None);
                tokio::spawn(async move {
                    let result = owner.run().await.map(Arc::new);
                    drop(owner);
                    send.send_replace(Some(result));
                });
                receive
            })
            .clone();
        loop {
            if let Some(result) = outcome.borrow_and_update().clone() {
                return result;
            }
            outcome
                .changed()
                .await
                .map_err(|_| "final goal capture ended without a result".to_owned())?;
        }
    }
    async fn run(&self) -> Result<FinalGoalForest, String> {
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
