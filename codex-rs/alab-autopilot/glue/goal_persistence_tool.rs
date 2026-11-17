// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::future::Future;
use std::pin::Pin;

impl GoalToolExecutor {
    pub(super) async fn alab_install_created_goal_turn_options(
        &self,
        thread_manager: &Weak<ThreadManager>,
        invocation: &ToolCall<'_>,
    ) {
        let Some(manager) = thread_manager.upgrade() else {
            return;
        };
        let Ok(thread) = manager.get_thread(self.thread_id).await else {
            return;
        };
        let Some(root_turn_id) = thread.active_turn_root(&invocation.turn_id).await else {
            return;
        };
        thread
            .thread_extension_data()
            .insert(codex_core::TurnStartOptions {
                root_turn_id: Some(root_turn_id),
                parent_turn_id: Some(invocation.turn_id.to_string()),
                ..Default::default()
            });
    }

    pub(super) fn alab_owned_active_progress(
        &self,
        mode: codex_state::GoalAccountingMode,
        event_id: &str,
        disposition: BudgetLimitedGoalDisposition,
    ) -> Pin<Box<dyn Future<Output = Result<Option<ThreadGoal>, FunctionCallError>> + Send + '_>>
    {
        let state = Arc::clone(&self.accounting_state.alab_persistence);
        let accounting = Arc::clone(&self.accounting_state);
        let tool = self.clone();
        let event_id = event_id.to_string();
        Box::pin(async move {
            codex_alab_core::goal_persistence::owned(state, async move {
                let progress = tool
                    .account_active_goal_progress(mode, &event_id, disposition)
                    .await?;
                if progress.is_none()
                    && let Some(turn_id) = accounting.current_turn_id()
                {
                    match disposition {
                        BudgetLimitedGoalDisposition::ClearActive => {
                            accounting.alab_close_empty_turn_boundary(&turn_id);
                        }
                        BudgetLimitedGoalDisposition::KeepActive => {
                            accounting.alab_reopen_empty_turn_boundary(&turn_id);
                        }
                    }
                }
                Ok(progress)
            })
            .await
            .map_err(|error| {
                FunctionCallError::Fatal(format!(
                    "goal accounting owner ended before acknowledgement: {error}"
                ))
            })?
        })
    }
}

/// Fields drop in declaration order: publish the failed/cancelled mutation fence
/// before releasing the native permit to any queued stop or automatic admission.
pub(super) struct Mutation<'a> {
    write: Option<codex_alab_core::goal_persistence::Write>,
    permit: Option<tokio::sync::SemaphorePermit<'a>>,
    previous_goal: Option<codex_state::ThreadGoal>,
    runtime: Option<&'a GoalRuntimeHandle>,
    mode_epoch: Option<u64>,
    supersedes_settled_unknown: bool,
    chronology_fence_generation:
        Result<Option<codex_alab_core::goal_chronology_fence::FenceGeneration>, ()>,
}

impl Mutation<'_> {
    pub(super) async fn make_write_durable(
        &mut self,
        state_db: &codex_state::StateRuntime,
    ) -> Result<(), FunctionCallError> {
        if !codex_alab_core::is_alab_binary() {
            return Ok(());
        }
        if let Err(error) = state_db.thread_goals().alab_durable_goal_barrier().await {
            if let Some(write) = &mut self.write {
                // The SQL command already returned success; a barrier failure is
                // outcome-unknown regardless of the checkpoint's typed error.
                write.settled_unknown();
            }
            return Err(FunctionCallError::RespondToModel(format!(
                "goal update committed but its durability could not be proven: {error}"
            )));
        }
        Ok(())
    }

    pub(super) fn begin_dispatched_mutation(
        &mut self,
        accounting: &GoalAccountingState,
    ) -> Result<(), FunctionCallError> {
        if codex_alab_core::is_alab_binary() {
            self.write = Some(
                accounting
                    .alab_begin_dispatched_mutation()
                    .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?,
            );
        }
        Ok(())
    }

    pub(super) fn commit(
        &mut self,
        thread: codex_protocol::ThreadId,
        turn: &str,
        call: &str,
    ) -> Option<codex_alab_core::goal_persistence::MutationEffect> {
        self.commit_marker(thread, turn, call, None)
    }

    pub(super) fn settle_storage_error(
        &mut self,
        accounting: &GoalAccountingState,
        error: &anyhow::Error,
    ) {
        let Some(write) = &mut self.write else {
            return;
        };
        if codex_state::alab_goal_read_recovery::mutation_definitely_uncommitted(error) {
            if write.predecessor_is_authoritative() {
                accounting.alab_reopen_rolled_back_mutation();
            }
            write.definitely_rolled_back();
        } else {
            write.settled_unknown();
        }
    }

    pub(super) fn settle_noop(&mut self, accounting: &GoalAccountingState) {
        let Some(write) = &mut self.write else {
            return;
        };
        if write.predecessor_is_authoritative() {
            accounting.alab_reopen_rolled_back_mutation();
        }
        write.definitely_rolled_back();
    }

    pub(super) fn commit_stop(
        &mut self,
        goal: &codex_protocol::protocol::ThreadGoal,
        turn: &str,
        call: &str,
    ) -> Option<codex_alab_core::goal_persistence::MutationEffect> {
        let chronology_fence_generation = self.chronology_fence_generation.ok();
        let stop = self.previous_goal.take().and_then(|previous| {
            let chronology_fence_generation = chronology_fence_generation?;
            (goal.status == codex_protocol::protocol::ThreadGoalStatus::Blocked
                && previous.objective == goal.objective
                && previous.token_budget == goal.token_budget
                && !goal
                    .token_budget
                    .is_some_and(|budget| goal.tokens_used >= budget))
            .then(|| codex_alab_core::model_goal_stops::CommittedStop {
                previous_status: match previous.status {
                    codex_state::ThreadGoalStatus::Active => {
                        codex_alab_core::model_goal_stops::PreviousStatus::Active
                    }
                    codex_state::ThreadGoalStatus::Blocked => {
                        codex_alab_core::model_goal_stops::PreviousStatus::Blocked
                    }
                    _ => codex_alab_core::model_goal_stops::PreviousStatus::Other,
                },
                goal: codex_alab_core::session::WallGoal {
                    goal_id: previous.goal_id,
                    objective: goal.objective.clone(),
                    token_budget: goal.token_budget,
                    chronology_fence_generation,
                    turn: turn.to_owned(),
                    person_stops: 0,
                },
            })
        });
        self.commit_marker(goal.thread_id, turn, call, stop)
    }

    fn commit_marker(
        &mut self,
        thread: codex_protocol::ThreadId,
        turn: &str,
        call: &str,
        stop: Option<codex_alab_core::model_goal_stops::CommittedStop>,
    ) -> Option<codex_alab_core::goal_persistence::MutationEffect> {
        let transition = self
            .write
            .take()
            .and_then(codex_alab_core::goal_persistence::Write::commit_mutation)?;
        let effect = transition.effect.clone();
        if let Some(runtime) = self.runtime {
            runtime.alab_reconcile_unowned_goal_mutation();
        }
        let Some(mode_epoch) = self
            .mode_epoch
            .filter(|epoch| codex_alab_core::mode_epoch() == Some(*epoch))
        else {
            return Some(effect);
        };
        codex_alab_core::model_goal_stops::committed(
            &thread.to_string(),
            turn,
            call,
            codex_alab_core::model_goal_stops::CommittedMutation {
                transition,
                stop,
                mode_epoch,
            },
        );
        Some(effect)
    }

    fn reject_validation(&mut self) {
        if let Some(write) = self.write.take() {
            write.finish(true);
        }
    }
}

impl GoalToolExecutor {
    pub(super) async fn alab_apply_created_goal_effect(
        &self,
        effect: codex_alab_core::goal_persistence::MutationEffect,
        goal_id: String,
    ) -> Option<String> {
        let custody = effect.clone();
        let (turn_id, rejected, applied) =
            codex_alab_core::goal_persistence::mutation_effect(effect, async {
                let turn_id = self.accounting_state.mark_current_turn_goal_active(goal_id);
                if !codex_alab_core::goal_persistence::mutation_effect_rejected() {
                    codex_alab_core::goal_persistence::mutation_effect_applied();
                }
                turn_id
            })
            .await;
        if rejected || !applied {
            self.accounting_state
                .alab_persistence
                .hold_if_mutation_effect_epoch_current(&custody);
        }
        turn_id
    }

    pub(super) async fn alab_apply_stopped_goal_effect(
        &self,
        effect: codex_alab_core::goal_persistence::MutationEffect,
        goal_id: &str,
    ) -> Option<String> {
        let custody = effect.clone();
        let (turn_id, rejected, applied) =
            codex_alab_core::goal_persistence::mutation_effect(effect, async {
                if self.accounting_state.alab_preserves_pending_progress() {
                    let turn_id = self.accounting_state.current_turn_id();
                    self.accounting_state
                        .alab_acknowledge_pending_same_goal_effect(goal_id);
                    return turn_id;
                }
                let turn_id = self.accounting_state.clear_current_turn_goal();
                if !codex_alab_core::goal_persistence::mutation_effect_rejected() {
                    codex_alab_core::goal_persistence::mutation_effect_applied();
                }
                turn_id
            })
            .await;
        if rejected || !applied {
            self.accounting_state
                .alab_persistence
                .hold_if_mutation_effect_epoch_current(&custody);
        }
        turn_id
    }

    pub(super) async fn alab_prepare_stop(&self) -> Result<Mutation<'_>, FunctionCallError> {
        let state = &self.accounting_state.alab_persistence;
        let mut guard = Mutation {
            write: None,
            permit: None,
            previous_goal: None,
            runtime: self.alab_runtime.as_ref(),
            mode_epoch: codex_alab_core::mode_epoch(),
            supersedes_settled_unknown: false,
            chronology_fence_generation: Ok(None),
        };
        if !codex_alab_core::is_alab_binary() {
            return Ok(guard);
        }
        let write = state.begin();
        guard.supersedes_settled_unknown = write.predecessor_was_settled_mutation_unknown();
        let epoch = write.mutation_epoch().ok_or_else(|| {
            FunctionCallError::Fatal("the native goal mutation epoch is exhausted".to_owned())
        })?;
        guard.write = Some(write);
        guard.permit = match &self.alab_runtime {
            Some(runtime) => Some(
                runtime
                    .goal_state_permit()
                    .await
                    .map_err(FunctionCallError::Fatal)?,
            ),
            None => None,
        };
        guard.chronology_fence_generation = match &self.alab_runtime {
            Some(runtime) => runtime
                .alab_goal_chronology_generation()
                .await
                .map_err(|_| ()),
            None => Err(()),
        };
        if !state.mutation_epoch_is_current(&epoch) {
            guard.reject_validation();
            return Err(FunctionCallError::RespondToModel(
                "this goal stop was superseded by a newer explicit goal action".to_owned(),
            ));
        }
        let goal = self
            .state_db
            .thread_goals()
            .get_thread_goal(self.thread_id)
            .await
            .map_err(|error| {
                FunctionCallError::RespondToModel(format!(
                    "failed to read goal before stopping it: {error}"
                ))
            })?;
        if goal.is_none() {
            guard.reject_validation();
            return Err(FunctionCallError::RespondToModel(
                "cannot update goal because this thread has no goal".to_owned(),
            ));
        }
        guard.previous_goal = goal;
        let supersedes_settled_unknown = guard.supersedes_settled_unknown;
        if !state.permits_write() && !supersedes_settled_unknown {
            return Err(FunctionCallError::RespondToModel(
                "goal status is held until ambiguous usage can be reconciled".to_owned(),
            ));
        }
        if let Some(runtime) = &self.alab_runtime
            && !supersedes_settled_unknown
        {
            let current = self.accounting_state.current_turn_id();
            runtime
                .alab_drain_other_progress(current.as_deref())
                .await
                .map_err(FunctionCallError::RespondToModel)?;
        }
        guard.reject_validation();
        Ok(guard)
    }

    pub(super) async fn alab_prepare_create(&self) -> Result<Mutation<'_>, FunctionCallError> {
        let state = &self.accounting_state.alab_persistence;
        let mut guard = Mutation {
            write: None,
            permit: None,
            previous_goal: None,
            runtime: self.alab_runtime.as_ref(),
            mode_epoch: codex_alab_core::mode_epoch(),
            supersedes_settled_unknown: false,
            chronology_fence_generation: Ok(None),
        };
        if !codex_alab_core::is_alab_binary() {
            return Ok(guard);
        }
        let write = state.begin();
        guard.supersedes_settled_unknown = write.predecessor_was_settled_mutation_unknown();
        guard.write = Some(write);
        if let Some(runtime) = &self.alab_runtime {
            guard.permit = Some(
                runtime
                    .goal_state_permit()
                    .await
                    .map_err(FunctionCallError::Fatal)?,
            );
        }
        let existing = self
            .state_db
            .thread_goals()
            .get_thread_goal(self.thread_id)
            .await
            .map_err(|error| {
                FunctionCallError::RespondToModel(format!(
                    "failed to read goal before creation: {error}"
                ))
            })?;
        if existing.is_some_and(|goal| goal.status != codex_state::ThreadGoalStatus::Complete) {
            guard.reject_validation();
            return Err(FunctionCallError::RespondToModel("cannot create a new goal because this thread has an unfinished goal; complete the existing goal first".to_owned()));
        }
        match &self.alab_runtime {
            Some(_) if guard.supersedes_settled_unknown => {}
            Some(runtime) => runtime
                .prepare_external_goal_mutation()
                .await
                .map_err(FunctionCallError::RespondToModel)?,
            None if !state.permits_write() && !guard.supersedes_settled_unknown => return Err(FunctionCallError::RespondToModel(
                "goal creation is held until retained usage can be accounted by its native runtime"
                    .to_owned(),
            )),
            None => {}
        }
        guard.reject_validation();
        Ok(guard)
    }
}
