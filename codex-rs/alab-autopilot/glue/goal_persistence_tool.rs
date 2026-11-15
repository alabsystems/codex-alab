// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::future::Future;
use std::pin::Pin;

impl GoalToolExecutor {
    pub(super) fn alab_owned_active_progress(
        &self,
        mode: codex_state::GoalAccountingMode,
        event_id: &str,
        disposition: BudgetLimitedGoalDisposition,
    ) -> Pin<Box<dyn Future<Output = Result<Option<ThreadGoal>, FunctionCallError>> + Send + '_>>
    {
        let state = Arc::clone(&self.accounting_state.alab_persistence);
        let tool = self.clone();
        let event_id = event_id.to_string();
        Box::pin(async move {
            codex_alab_core::goal_persistence::owned(state, async move {
                tool.account_active_goal_progress(mode, &event_id, disposition)
                    .await
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
}

impl Mutation<'_> {
    pub(super) fn commit(&mut self) {
        if let Some(write) = self.write.take() {
            write.commit_mutation();
        }
    }

    pub(super) fn commit_stop(
        &mut self,
        goal: &codex_protocol::protocol::ThreadGoal,
        turn: &str,
        call: &str,
    ) {
        let Some(write) = self.write.take() else {
            return;
        };
        let Some(receipt) = write.commit_mutation_with_receipt() else {
            return;
        };
        let Some(previous) = self.previous_goal.take() else {
            return;
        };
        if goal.status != codex_protocol::protocol::ThreadGoalStatus::Blocked
            || previous.objective != goal.objective
            || previous.token_budget != goal.token_budget
            || goal
                .token_budget
                .is_some_and(|budget| goal.tokens_used >= budget)
        {
            return;
        }
        codex_alab_core::model_goal_stops::committed(
            &goal.thread_id.to_string(),
            turn,
            call,
            codex_alab_core::model_goal_stops::CommittedStop {
                receipt,
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
                    turn: turn.to_owned(),
                    person_stops: 0,
                },
            },
        );
    }

    fn reject_validation(&mut self) {
        if let Some(write) = self.write.take() {
            write.finish(true);
        }
    }
}

impl GoalToolExecutor {
    pub(super) async fn alab_prepare_stop(&self) -> Result<Mutation<'_>, FunctionCallError> {
        let state = &self.accounting_state.alab_persistence;
        let mut guard = Mutation {
            write: None,
            permit: None,
            previous_goal: None,
        };
        if !codex_alab_core::active() {
            if codex_alab_core::is_alab_binary() {
                state.revoke_mutation_epoch();
            }
            return Ok(guard);
        }
        let write = state.begin();
        let generation = write.generation();
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
        if !state.generation_is_current(generation) {
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
        if !state.permits_write() {
            return Err(FunctionCallError::RespondToModel(
                "goal status is held until ambiguous usage can be reconciled".to_owned(),
            ));
        }
        if let Some(runtime) = &self.alab_runtime {
            let current = self.accounting_state.current_turn_id();
            runtime
                .alab_drain_other_progress(current.as_deref())
                .await
                .map_err(FunctionCallError::RespondToModel)?;
        }
        guard.reject_validation();
        guard.write = Some(state.begin_mutation());
        Ok(guard)
    }

    pub(super) async fn alab_prepare_create(&self) -> Result<Mutation<'_>, FunctionCallError> {
        let state = &self.accounting_state.alab_persistence;
        let mut guard = Mutation {
            write: None,
            permit: None,
            previous_goal: None,
        };
        if !codex_alab_core::active() {
            if codex_alab_core::is_alab_binary() {
                state.revoke_mutation_epoch();
            }
            return Ok(guard);
        }
        guard.write = Some(state.begin());
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
            Some(runtime) => runtime
                .prepare_external_goal_mutation()
                .await
                .map_err(FunctionCallError::RespondToModel)?,
            None if !state.permits_write() => return Err(FunctionCallError::RespondToModel(
                "goal creation is held until retained usage can be accounted by its native runtime"
                    .to_owned(),
            )),
            None => {}
        }
        guard.reject_validation();
        guard.write = Some(state.begin_mutation());
        Ok(guard)
    }
}
