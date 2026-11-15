// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compiled inside the goal runtime; native policy and SQL remain in their owners.
use super::*;
use std::future::Future;
use std::pin::Pin;

tokio::task_local! { static STOP_OWNER: (Arc<codex_alab_core::goal_persistence::State>, u64); }

impl GoalRuntimeHandle {
    pub fn alab_model_goal_epoch(&self) -> codex_alab_core::goal_persistence::MutationEpoch {
        self.inner
            .accounting_state
            .alab_persistence
            .mutation_epoch()
    }

    pub fn alab_model_goal_epoch_current(
        &self,
        epoch: &codex_alab_core::goal_persistence::MutationEpoch,
    ) -> bool {
        self.inner
            .accounting_state
            .alab_persistence
            .epoch_is_current(epoch)
    }

    pub(crate) fn alab_model_resume_allowed(&self) -> bool {
        let state = &self.inner.accounting_state.alab_persistence;
        codex_alab_core::model_goal_stops::resume_allowed(&self.thread_id().to_string(), state)
            // The intent recorder owns a healthy validation write here. A
            // failed persistence hold still cannot be cleared by automatic resume.
            && (codex_alab_core::model_goal_stops::resume_unscoped()
                || (codex_alab_core::active()
                    && state.admission() != codex_alab_core::goal_persistence::Admission::Held))
    }

    pub(crate) fn alab_begin_goal_mutation(
        &self,
    ) -> Option<codex_alab_core::goal_persistence::Write> {
        let state = &self.inner.accounting_state.alab_persistence;
        if !codex_alab_core::active() {
            if codex_alab_core::is_alab_binary() {
                state.revoke_mutation_epoch();
            }
            return None;
        }
        Some(state.begin_mutation())
    }

    /// Observable backpressure; this grants neither write nor continuation authority.
    pub fn alab_goal_persistence_held(&self) -> bool {
        codex_alab_core::active()
            && !self
                .inner
                .accounting_state
                .alab_persistence
                .permits_admission()
    }

    /// Read-only owner phase, separate from an unfinished validation operation.
    pub fn alab_accounting_write_pending(&self) -> bool {
        codex_alab_core::active()
            && self
                .inner
                .accounting_state
                .alab_persistence
                .accounting_write_pending()
    }

    pub(super) fn alab_stop_still_current() -> bool {
        STOP_OWNER
            .try_with(|(state, generation)| state.generation_is_current(*generation))
            .unwrap_or(true)
    }

    pub(super) fn alab_owns_stop() -> bool {
        STOP_OWNER.try_with(|_| ()).is_ok()
    }

    pub(super) fn alab_owned_active_progress(
        &self,
        turn_id: &str,
        event_id: &str,
        mode: codex_state::GoalAccountingMode,
        disposition: BudgetLimitedGoalDisposition,
    ) -> Pin<Box<dyn Future<Output = Result<Option<AccountedGoalProgress>, String>> + Send + '_>>
    {
        let state = Arc::clone(&self.inner.accounting_state.alab_persistence);
        let runtime = self.clone();
        let turn_id = turn_id.to_string();
        let event_id = event_id.to_string();
        Box::pin(async move {
            codex_alab_core::goal_persistence::owned(state, async move {
                runtime
                    .account_active_goal_progress(&turn_id, &event_id, mode, disposition)
                    .await
            })
            .await
            .map_err(|error| {
                format!("goal accounting owner ended before acknowledgement: {error}")
            })?
        })
    }

    pub(super) fn alab_owned_idle_progress(
        &self,
        event_id: &str,
        mode: codex_state::GoalAccountingMode,
        disposition: BudgetLimitedGoalDisposition,
    ) -> Pin<Box<dyn Future<Output = Result<Option<AccountedGoalProgress>, String>> + Send + '_>>
    {
        let state = Arc::clone(&self.inner.accounting_state.alab_persistence);
        let runtime = self.clone();
        let event_id = event_id.to_string();
        Box::pin(async move {
            codex_alab_core::goal_persistence::owned(state, async move {
                runtime
                    .account_idle_goal_progress(&event_id, mode, disposition)
                    .await
            })
            .await
            .map_err(|error| {
                format!("idle goal accounting owner ended before acknowledgement: {error}")
            })?
        })
    }

    pub(super) fn alab_owned_stop(
        &self,
        turn_id: &str,
        reason: ActiveGoalStopReason,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        let state = Arc::clone(&self.inner.accounting_state.alab_persistence);
        let write = state.begin();
        let generation = write.generation();
        let runtime = self.clone();
        let turn_id = turn_id.to_string();
        Box::pin(async move {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            tokio::spawn(STOP_OWNER.scope((state, generation), async move {
                let result = runtime.stop_active_goal_for_turn(&turn_id, reason).await;
                write.finish(result.is_ok());
                let _ = sender.send(result);
            }));
            receiver
                .await
                .map_err(|error| format!("goal stop owner ended before acknowledgement: {error}"))?
        })
    }

    pub(super) async fn alab_drain_retained_progress(&self) -> Result<(), String> {
        self.alab_drain_other_progress(None).await?;
        if !codex_alab_core::active() {
            return Ok(());
        }
        let accounting = self.accounting_state();
        let Some(snapshot) = accounting.idle_progress_snapshot() else {
            return Ok(());
        };
        let validation = accounting.alab_persistence.begin();
        let goal = self
            .inner
            .state_dbs
            .thread_goals()
            .get_thread_goal(self.thread_id())
            .await
            .map_err(|error| error.to_string())?;
        let Some(goal) = goal.filter(|goal| goal.goal_id == snapshot.expected_goal_id) else {
            accounting.alab_quarantine_pending_boundaries();
            return Err("retained idle usage belongs to a different or missing goal; the original boundary remains quarantined".to_owned());
        };
        // The caller retains its native permit. From dispatch onward the
        // additive task owns cancellation and baseline finalization; an
        // observer's preparation guard must not manufacture a failed write.
        validation.finish(true);
        self.account_idle_goal_progress(
            &format!("{}:retained-idle-goal-mutation", self.thread_id()),
            Self::alab_retained_accounting_mode(goal.status),
            BudgetLimitedGoalDisposition::ClearActive,
        )
        .await?;
        Ok(())
    }

    /// Plan's native clear must exclude Plan time. Drain first under the native
    /// permit, before turn-start takes its differently ordered accounting lock.
    /// If storage still refuses, quarantine the original boundary before reset.
    pub(crate) async fn alab_prepare_plan_accounting(&self) {
        let accounting = self.accounting_state();
        if !accounting.alab_preserves_pending_progress() {
            return;
        }
        let prepared = async {
            let _permit = self.goal_state_permit().await?;
            self.alab_drain_retained_progress().await
        }
        .await;
        if let Err(error) = prepared {
            accounting.alab_quarantine_pending_boundaries();
            tracing::warn!(%error, "Plan input retained an unacknowledged original goal boundary without replay");
        }
    }

    fn alab_retained_accounting_mode(
        status: codex_state::ThreadGoalStatus,
    ) -> codex_state::GoalAccountingMode {
        match status {
            codex_state::ThreadGoalStatus::Active
            | codex_state::ThreadGoalStatus::BudgetLimited => {
                codex_state::GoalAccountingMode::ActiveOnly
            }
            codex_state::ThreadGoalStatus::Complete => {
                codex_state::GoalAccountingMode::ActiveOrComplete
            }
            codex_state::ThreadGoalStatus::Paused
            | codex_state::ThreadGoalStatus::Blocked
            | codex_state::ThreadGoalStatus::UsageLimited => {
                codex_state::GoalAccountingMode::ActiveOrStopped
            }
        }
    }

    /// Caller owns the native goal permit. Keep the ending turn for its own
    /// native status/policy path; account all older retained records first.
    pub(crate) async fn alab_drain_other_progress(
        &self,
        ending_turn: Option<&str>,
    ) -> Result<(), String> {
        if !codex_alab_core::active() {
            return Ok(());
        }
        let accounting = self.accounting_state();
        if !accounting.alab_persistence.quarantined_usage().is_empty() {
            return Err("original goal usage is quarantined after an uncoordinated replacement; it remains held without replay".to_owned());
        }
        let turns: Vec<_> = accounting
            .alab_retained_turn_ids()
            .into_iter()
            .filter(|turn| Some(turn.as_str()) != ending_turn)
            .collect();
        if turns.is_empty() {
            return Ok(());
        }
        for turn_id in turns {
            let Some(expected_goal_id) = accounting.alab_retained_goal_id(&turn_id) else {
                if accounting.current_turn_id().as_deref() != Some(&turn_id) {
                    accounting.finish_turn(&turn_id);
                }
                continue;
            };
            let validation = accounting.alab_persistence.begin();
            let goal = self
                .inner
                .state_dbs
                .thread_goals()
                .get_thread_goal(self.thread_id())
                .await
                .map_err(|error| error.to_string())?;
            let Some(goal) = goal.filter(|goal| goal.goal_id == expected_goal_id) else {
                accounting.alab_quarantine_pending_boundaries();
                return Err("retained usage belongs to a different or missing goal; it was preserved without attribution".to_owned());
            };
            validation.finish(true);
            if accounting.progress_snapshot(&turn_id).is_none() {
                if accounting.current_turn_id().as_deref() != Some(&turn_id) {
                    accounting.finish_turn(&turn_id);
                }
                continue;
            }
            let mode = Self::alab_retained_accounting_mode(goal.status);
            let progress = self
                .account_active_goal_progress(
                    &turn_id,
                    &format!("{turn_id}:retained-goal-mutation"),
                    mode,
                    BudgetLimitedGoalDisposition::ClearActive,
                )
                .await?;
            if progress.is_none() {
                // A concurrently completed owner may have consumed this snapshot.
                // An Unchanged result itself never authorizes dropping a record.
                if accounting.progress_snapshot(&turn_id).is_some() {
                    accounting.alab_persistence.begin().finish(false);
                    return Err("the original goal did not accept its retained usage; the snapshot remains held".to_owned());
                }
                continue;
            }
            if accounting.current_turn_id().as_deref() != Some(&turn_id) {
                accounting.finish_turn(&turn_id);
            }
        }
        Ok(())
    }
}
