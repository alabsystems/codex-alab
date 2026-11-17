// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One source commitment for actual terminal/input, Core forest and goal owners.
//! Target admission and the host's final policy decision remain independent.
use crate::upgrade_goals::FinalGoalForest;
use crate::upgrade_goals::GoalCaptureOwner;
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
    pub async fn invalidated(&self) {
        tokio::select! {
            () = self.forest.invalidated() => {},
            () = self.goals.invalidated() => {},
        }
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
        let goals = match self
            .forest
            .into_committed()
            .and_then(|forest| self.goals.into_capture(forest))
        {
            Ok(goals) => goals,
            Err(why) => {
                // The final host decision already committed. This is fatal
                // source cleanup, never a revocable refusal.
                if let Err(error) = terminal.restore_after_failure() {
                    return Err(format!("{why}; terminal recovery failed: {error}"));
                }
                return Err(why);
            }
        };
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

/// Armed before spawning the sole source task. Any loss before final delivery
/// restores the committed terminal modes; successful delivery consumes the
/// terminal from `Capture`, making this drop a no-op.
struct TerminalCaptureRelease {
    capture: Arc<Capture>,
    send: tokio::sync::watch::Sender<Option<Outcome>>,
    restored: bool,
}

impl TerminalCaptureRelease {
    fn new(capture: Arc<Capture>, send: tokio::sync::watch::Sender<Option<Outcome>>) -> Self {
        Self {
            capture,
            send,
            restored: false,
        }
    }

    fn sender(&self) -> &tokio::sync::watch::Sender<Option<Outcome>> {
        &self.send
    }

    fn restore(&mut self) -> Result<(), String> {
        if self.restored {
            return Ok(());
        }
        let result = self
            .capture
            .terminal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .filter(|terminal| terminal.committed())
            .map(upgrade_terminal::Prepared::restore_after_failure)
            .transpose()
            .map(|_| ())
            .map_err(|error| error.to_string());
        if result.is_ok() {
            self.restored = true;
        }
        result
    }

    fn publish_error(&mut self, error: String) {
        let error = match self.restore() {
            Ok(()) => error,
            Err(restore) => format!("{error}; terminal restore failed: {restore}"),
        };
        self.send.send_replace(Some(Err(error)));
    }
}

impl Drop for TerminalCaptureRelease {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            tracing::warn!(%error, "failed to restore terminal after source capture ended");
        }
    }
}

impl Capture {
    /// Source cleanup and final accounting are owned independently of waiters.
    pub async fn capture(self: &Arc<Self>) -> Outcome {
        let owner = self.clone();
        let mut receive = self
            .outcome
            .get_or_init(move || {
                let (send, receive) = tokio::sync::watch::channel(None);
                // Arm the one goal/terminal owner before this task can be
                // discarded without a first poll. Publication and the outer
                // watch delivery then share one synchronous commit point.
                let goal_capture = owner.goals.claim_owner();
                let mut terminal_release = TerminalCaptureRelease::new(Arc::clone(&owner), send);
                tokio::spawn(async move {
                    let result = match goal_capture {
                        Ok(mut goal_capture) => {
                            owner
                                .run(&mut goal_capture, terminal_release.sender())
                                .await
                        }
                        Err(error) => Err(error),
                    };
                    if let Err(error) = result {
                        terminal_release.publish_error(error);
                    }
                    drop(owner);
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
    /// Fatal cleanup when final Core/goal capture failed before handing the
    /// terminal to Captured. This restores terminal modes, never old producers.
    pub fn restore_after_failure(&self) -> Result<(), String> {
        self.terminal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .ok_or("terminal recovery belongs to the final captured owner")?
            .restore_after_failure()
            .map_err(|error| error.to_string())
    }

    async fn run(
        &self,
        goal_capture: &mut GoalCaptureOwner,
        send: &tokio::sync::watch::Sender<Option<Outcome>>,
    ) -> Result<(), String> {
        // This joins Core producers and actual SDK reader ACKs before flushing
        // the final native goal rows/clocks. Kernel input remains untouched.
        let goals = goal_capture.prepare().await?;
        let mut terminal = self
            .terminal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !terminal
            .as_ref()
            .is_some_and(upgrade_terminal::Prepared::committed)
        {
            return Err("source terminal ownership was not committed".into());
        }
        let mut delivered = false;
        goal_capture.publish(goals, |goals| {
            if let Some(terminal) = terminal.take() {
                send.send_replace(Some(Ok(Arc::new(Captured {
                    primary: self.primary,
                    goals,
                    terminal,
                }))));
                delivered = true;
            }
        })?;
        if !delivered {
            return Err("validated source publication lost its terminal".into());
        }
        Ok(())
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
    pub fn editor_snapshot(&self) -> Option<&[u8]> {
        self.terminal.editor_snapshot()
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
