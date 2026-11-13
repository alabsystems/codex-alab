// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One source commitment for actual terminal/input, Core forest and goal owners.
//! Target admission and the host's final policy decision remain independent.
use crate::upgrade_goals::FinalGoalForest;
use crate::upgrade_goals::GoalForestCapture;
use crate::upgrade_goals::PreparedGoals;
use codex_alab_tui::upgrade_terminal;
use codex_core::CodexThread;
use codex_core::PreparedUpgradeForest;
use codex_core::config::ExecConfiguration;
use codex_protocol::ThreadId;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

pub struct Prepared {
    primary: ThreadId,
    forest: PreparedUpgradeForest,
    goals: PreparedGoals,
}
impl Prepared {
    pub async fn prepare(
        primary: &Arc<CodexThread>,
        invocation: &ExecConfiguration,
    ) -> Result<Self, String> {
        let forest = primary.prepare_upgrade_forest(invocation).await?;
        Self::from_forest(primary.exec_thread_id(), forest).await
    }

    pub(crate) async fn from_forest(
        primary: ThreadId,
        forest: PreparedUpgradeForest,
    ) -> Result<Self, String> {
        if forest.roots().first() != Some(&primary) {
            return Err("source preparation requires its forest's actual primary".into());
        }
        let threads = forest.threads();
        let goals = tokio::select! {
            biased;
            () = forest.invalidated() => return Err("source preparation was superseded".into()),
            goals = PreparedGoals::prepare(&threads) => goals?,
        };
        let prepared = Self {
            primary,
            forest,
            goals,
        };
        if !prepared.valid() {
            return Err("source inventory changed during preparation".into());
        }
        Ok(prepared)
    }
    pub fn goal_snapshots(
        &self,
    ) -> impl Iterator<Item = (ThreadId, Option<&codex_state::ThreadGoal>)> {
        self.goals.snapshots()
    }
    pub fn threads(&self) -> Vec<Arc<CodexThread>> {
        self.forest.threads()
    }
    pub fn valid(&self) -> bool {
        self.forest.valid() && self.goals.valid()
    }
    /// All native boundaries remain held through the host's one final decision.
    /// A refusal drops preparation and leaves ordinary work admitted.
    pub fn commit(
        mut self,
        mut terminal: upgrade_terminal::Prepared,
        decide: impl FnOnce() -> bool,
    ) -> Result<Option<Arc<Capture>>, String> {
        let mut refusal = None;
        let accepted = terminal
            .commit_if(
                || match self.goals.commit_forest_if(&mut self.forest, decide) {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        refusal = Some(error);
                        false
                    }
                },
            )
            .map_err(|error| error.to_string())?;
        if let Some(error) = refusal {
            return Err(error);
        }
        if !accepted {
            return Ok(None);
        }
        let goals = self.goals.into_capture(self.forest.into_committed()?)?;
        Ok(Some(Arc::new(Capture {
            primary: self.primary,
            goals,
            terminal: Mutex::new(Some(terminal)),
            outcome: OnceLock::new(),
        })))
    }
}

type Outcome = Result<Arc<Captured>, String>;
pub struct Capture {
    primary: ThreadId,
    goals: Arc<GoalForestCapture>,
    terminal: Mutex<Option<upgrade_terminal::Prepared>>,
    outcome: OnceLock<tokio::sync::watch::Receiver<Option<Outcome>>>,
}
impl Capture {
    /// Source cleanup and final accounting are owned independently of waiters.
    pub async fn capture(self: &Arc<Self>) -> Outcome {
        let owner = self.clone();
        let mut receive = self
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
            if let Some(result) = receive.borrow_and_update().clone() {
                return result;
            }
            receive
                .changed()
                .await
                .map_err(|_| "source capture ended without a result".to_owned())?;
        }
    }
    async fn run(&self) -> Result<Captured, String> {
        // This joins Core producers and actual SDK reader ACKs before flushing
        // the final native goal rows/clocks. Kernel input remains untouched.
        let goals = self.goals.capture().await?;
        let terminal = self
            .terminal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .ok_or("source terminal ownership was already consumed")?;
        if !terminal.committed() {
            return Err("source terminal ownership was not committed".into());
        }
        Ok(Captured {
            primary: self.primary,
            goals,
            terminal,
        })
    }
}

/// Retain the actual committed terminal owner alongside final source data until
/// execution succeeds or its owner explicitly restores after a fatal failure.
pub struct Captured {
    pub primary: ThreadId,
    pub goals: Arc<FinalGoalForest>,
    terminal: upgrade_terminal::Prepared,
}
impl Captured {
    pub fn input_snapshot(&self) -> &[u8] {
        self.terminal.input_snapshot()
    }
    pub fn mode_snapshot(&self) -> &codex_alab_tui::terminal_modes::Snapshot {
        self.terminal.mode_snapshot()
    }
    pub fn startup_snapshot(&self) -> Option<&codex_alab_tui::upgrade_startup::Snapshot> {
        self.terminal.startup_snapshot()
    }
    pub fn restore_after_failure(&self) -> Result<(), String> {
        self.terminal
            .restore_after_failure()
            .map_err(|error| error.to_string())
    }
}
