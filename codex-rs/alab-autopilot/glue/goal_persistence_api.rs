// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_alab_core::goal_persistence::MutationEpoch;
use std::future::Future;

impl GoalService {
    /// Read the registered native owner's epoch; this is not committed stop evidence.
    pub fn alab_model_goal_epoch(&self, thread_id: ThreadId) -> Option<MutationEpoch> {
        self.runtime_for_thread(thread_id)
            .map(|runtime| runtime.alab_model_goal_epoch())
    }

    pub fn alab_model_goal_epoch_current(
        &self,
        thread_id: ThreadId,
        epoch: &MutationEpoch,
    ) -> bool {
        self.runtime_for_thread(thread_id)
            .is_some_and(|runtime| runtime.alab_model_goal_epoch_current(epoch))
    }

    /// Keep the expected stop scoped through native permit and final intent checks.
    pub async fn alab_resume_model_goal(
        &self,
        state_db: &codex_state::StateRuntime,
        request: GoalSetRequest<'_>,
        expected: &MutationEpoch,
        instruction: impl Future<Output = Result<(), GoalServiceError>> + Send,
    ) -> Result<GoalSetOutcome, GoalServiceError> {
        if request.status != Some(ThreadGoalStatus::Active)
            || !matches!(request.objective, GoalObjectiveUpdate::Keep)
            || !matches!(request.token_budget, GoalTokenBudgetUpdate::Keep)
        {
            return Err(GoalServiceError::InvalidRequest(
                "model goal resume must preserve the objective and token budget".into(),
            ));
        }
        if !self.alab_model_goal_epoch_current(request.thread_id, expected) {
            return Err(GoalServiceError::InvalidRequest(
                "the model goal stop's native owner or mutation epoch changed".into(),
            ));
        }
        codex_alab_core::model_goal_stops::resume_scoped(
            request.thread_id.to_string(),
            expected.clone(),
            self.set_thread_goal(state_db, request, instruction),
        )
        .await
    }

    fn alab_model_resume_allowed(&self, runtime: Option<&Arc<GoalRuntimeHandle>>) -> bool {
        match runtime {
            Some(runtime) => runtime.alab_model_resume_allowed(),
            None => codex_alab_core::model_goal_stops::resume_unscoped(),
        }
    }

    pub(super) async fn alab_record_goal_instruction(
        &self,
        runtime: Option<&Arc<GoalRuntimeHandle>>,
        instruction: impl Future<Output = Result<(), GoalServiceError>> + Send,
    ) -> Result<(), GoalServiceError> {
        if !self.alab_model_resume_allowed(runtime) {
            return Err(GoalServiceError::InvalidRequest(
                "the model goal stop changed before native intent validation".into(),
            ));
        }
        let record = runtime.and_then(|runtime| {
            let accounting = runtime.accounting_state();
            if codex_alab_core::active() {
                Some(accounting.alab_persistence.begin())
            } else {
                if codex_alab_core::is_alab_binary() {
                    accounting.alab_persistence.revoke_mutation_epoch();
                }
                None
            }
        });
        let result = match instruction.await {
            Ok(()) if !self.alab_model_resume_allowed(runtime) => {
                Err(GoalServiceError::InvalidRequest(
                    "the model goal stop changed during native intent validation".into(),
                ))
            }
            result => result,
        };
        if let Some(record) = record {
            // Stale automatic decisions and healthy validation refusals have
            // reached no goal SQL. A failed/cancelled recording remains held.
            record.finish(
                result.is_ok() || matches!(&result, Err(GoalServiceError::InvalidRequest(_))),
            );
        }
        result
    }
}
