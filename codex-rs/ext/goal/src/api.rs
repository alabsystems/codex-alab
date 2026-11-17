use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::Weak;

use codex_protocol::ThreadId;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadGoal;
use codex_protocol::protocol::ThreadGoalStatus;
use codex_protocol::protocol::ThreadGoalUpdatedEvent;
use codex_protocol::protocol::validate_thread_goal_objective;
use codex_rollout::RolloutItem;

use crate::runtime::GoalRuntimeHandle;
use crate::runtime::PreviousGoalSnapshot;
use crate::tool::fill_empty_thread_preview_if_possible;
use crate::tool::protocol_goal_from_state;
use crate::tool::state_status_from_protocol;
use crate::tool::validate_goal_budget;

#[path = "../../../alab-autopilot/glue/goal_persistence_api.rs"]
mod alab_persistence; // ALAB(storage): validation refusals differ from failed or cancelled goal intent recording.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalServiceError {
    InvalidRequest(String),
    Internal(String),
}

impl fmt::Display for GoalServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(message) | Self::Internal(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for GoalServiceError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalObjectiveUpdate<'a> {
    Keep,
    Set(&'a str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalTokenBudgetUpdate {
    Keep,
    Set(Option<i64>),
}

#[derive(Clone, Copy, Debug)]
pub struct GoalSetRequest<'a> {
    pub thread_id: ThreadId,
    pub objective: GoalObjectiveUpdate<'a>,
    pub status: Option<ThreadGoalStatus>,
    pub token_budget: GoalTokenBudgetUpdate,
    pub max_goal_token_budget: Option<i64>,
}

#[must_use = "goal mutation effects remain held until the outcome is explicitly applied or intentionally dropped"]
#[derive(Clone, Debug)]
pub struct GoalSetOutcome {
    pub goal: ThreadGoal,
    state_goal: codex_state::ThreadGoal,
    previous_goal: Option<PreviousGoalSnapshot>,
    _alab_boundary: Option<crate::runtime::GoalMutationBoundary>, // ALAB(storage): cancellation before runtime effects thaws old-goal custody.
}

#[must_use = "goal mutation effects remain held until the outcome is explicitly applied or intentionally dropped"]
#[derive(Clone, Debug)]
pub struct GoalClearOutcome {
    cleared_goal: Option<codex_state::ThreadGoal>,
    _alab_boundary: Option<crate::runtime::GoalMutationBoundary>, // ALAB(storage): cancellation before the exact clear effect retains committed-effect custody.
}

impl GoalClearOutcome {
    pub fn cleared(&self) -> bool {
        self.cleared_goal.is_some()
    }

    pub async fn apply_runtime_effects(&self, goal_service: &GoalService) {
        if codex_alab_core::is_alab_binary() && self._alab_boundary.is_none() {
            return;
        } // ALAB(storage): an ownerless clear has no exact live runtime effect token; startup restores its durable absence.
        let runtime = self
            ._alab_boundary
            .as_ref()
            .map(crate::runtime::GoalMutationBoundary::runtime)
            .or_else(|| {
                self.cleared_goal
                    .as_ref()
                    .and_then(|goal| goal_service.runtime_for_thread(goal.thread_id))
            }); // ALAB(storage): the effect targets the exact runtime that acknowledged the commit, not a later registry occupant.
        let apply = async {
            if let Some(runtime) = runtime
                && (codex_alab_core::is_alab_binary() || self.cleared_goal.is_some())
                && let Err(err) = runtime
                    .apply_external_goal_clear(self.cleared_goal.clone())
                    .await
            {
                tracing::warn!("failed to apply external goal clear runtime effects: {err}");
            }
        };
        if let Some(boundary) = &self._alab_boundary {
            let _ = boundary.scope(apply).await;
        } else {
            apply.await;
        }
    }
}

impl GoalSetOutcome {
    pub fn thread_goal_updated_item(&self) -> RolloutItem {
        RolloutItem::EventMsg(EventMsg::ThreadGoalUpdated(ThreadGoalUpdatedEvent {
            thread_id: self.goal.thread_id,
            turn_id: codex_alab_core::is_alab_binary()
                .then(|| codex_alab_core::goal_persistence::HOST_MUTATION_TURN_ID.to_owned()), // ALAB(goal-recovery): durable host/API provenance cannot borrow a model turn, including in vanilla mode.
            goal: self.goal.clone(),
        }))
    }

    pub async fn apply_runtime_effects(&self, goal_service: &GoalService) {
        self.apply_runtime_effects_with_policy(goal_service, None)
            .await;
    }

    // ALAB(policy-resume): reactivation and its policy instruction share one native continuation.
    pub async fn apply_runtime_effects_with_policy(
        &self,
        goal_service: &GoalService,
        policy: Option<String>,
    ) {
        if codex_alab_core::is_alab_binary() && self._alab_boundary.is_none() {
            return;
        } // ALAB(storage): an ownerless commit has no exact live runtime effect token; startup restores its durable row.
        let runtime = self
            ._alab_boundary
            .as_ref()
            .map(crate::runtime::GoalMutationBoundary::runtime)
            .or_else(|| goal_service.runtime_for_thread(self.goal.thread_id)); // ALAB(storage): the effect targets the exact runtime that acknowledged the commit, not a later registry occupant.
        let apply = async {
            if let Some(runtime) = runtime
                && let Err(err) = runtime
                    .apply_external_goal_set_with_policy(
                        self.state_goal.clone(),
                        self.previous_goal.clone(),
                        policy,
                    )
                    .await
            {
                tracing::warn!("failed to apply external goal status runtime effects: {err}");
            }
        };
        if let Some(boundary) = &self._alab_boundary {
            let _ = boundary.scope(apply).await;
        } else {
            apply.await;
        }
    }
}

#[derive(Debug, Default)]
pub struct GoalService {
    runtimes: Arc<Mutex<HashMap<String, Weak<GoalRuntimeHandle>>>>, // ALAB(storage-status): watchers retain only a Weak registry, never the server.
    alab_unowned_mutations: Arc<Mutex<alab_persistence::UnownedMutations>>, // ALAB(goal-recovery): runtime-gap intents fence every successor through SQL settlement.
    alab_upgrade_retirement: std::sync::OnceLock<Arc<crate::runtime::UpgradeRetirementCoordinator>>, // ALAB(goal-recovery): one registry owner publishes a committed upgrade inventory atomically.
}

impl GoalService {
    pub fn alab_runtime_status_snapshot(
        &self,
        thread_id: ThreadId,
    ) -> Option<codex_protocol::protocol::AlabGoalRuntimeSnapshot> {
        self.runtime_for_thread(thread_id)
            .and_then(|runtime| runtime.alab_runtime_status_snapshot())
    } // ALAB(storage-status): exact registered owner capture and native transport stamp.

    pub fn alab_emit_runtime_status(&self, thread_id: ThreadId) {
        if let Some(runtime) = self.runtime_for_thread(thread_id) {
            runtime.alab_emit_runtime_status();
        }
    } // ALAB(storage-status): reattach observes native owner without reading SQLite.

    pub fn new() -> Self {
        Self::default()
    }

    /// Restores persisted goal state into the registered runtime for `thread_id`.
    pub async fn restore_thread_runtime_after_resume(
        &self,
        thread_id: ThreadId,
    ) -> Result<(), GoalServiceError> {
        let runtime = self.runtime_for_thread(thread_id).ok_or_else(|| {
            GoalServiceError::Internal(format!(
                "goal runtime is unavailable for thread {thread_id}"
            ))
        })?;
        runtime
            .restore_after_resume()
            .await
            .map_err(GoalServiceError::Internal)
    }

    /// Flushes any in-flight goal accounting before a fork copies the source goal snapshot.
    pub async fn flush_thread_goal_progress_for_fork(
        &self,
        thread_id: ThreadId,
    ) -> Result<(), GoalServiceError> {
        let Some(runtime) = self.runtime_for_thread(thread_id) else {
            return Ok(());
        };
        let _goal_state_permit = runtime
            .goal_state_permit()
            .await
            .map_err(GoalServiceError::Internal)?;
        runtime
            .prepare_external_goal_mutation()
            .await
            .map_err(GoalServiceError::Internal)
    }

    pub async fn get_thread_goal(
        &self,
        state_db: &codex_state::StateRuntime,
        thread_id: ThreadId,
    ) -> Result<Option<ThreadGoal>, GoalServiceError> {
        state_db
            .thread_goals()
            .get_thread_goal(thread_id)
            .await
            .map(|goal| goal.map(protocol_goal_from_state))
            .map_err(|err| GoalServiceError::Internal(format!("failed to read thread goal: {err}")))
    }

    /// Records the user's instruction after validation and before changing goal state.
    /// The recording future runs under the runtime's goal mutation permit; failure leaves
    /// the goal unchanged. Recorded user intent remains valid if the database write fails.
    pub async fn set_thread_goal(
        &self,
        state_db: &codex_state::StateRuntime,
        request: GoalSetRequest<'_>,
        record_user_instruction: impl Future<Output = Result<(), GoalServiceError>> + Send,
    ) -> Result<GoalSetOutcome, GoalServiceError> {
        let GoalSetRequest {
            thread_id,
            objective,
            status,
            token_budget,
            max_goal_token_budget,
        } = request;
        let status = status.map(state_status_from_protocol);
        let objective = match objective {
            GoalObjectiveUpdate::Keep => None,
            GoalObjectiveUpdate::Set(objective) => Some(objective.trim()),
        };
        let token_budget = match token_budget {
            GoalTokenBudgetUpdate::Keep => None,
            GoalTokenBudgetUpdate::Set(token_budget) => {
                Some(token_budget.or(max_goal_token_budget))
            }
        };

        if let Some(objective) = objective {
            validate_thread_goal_objective(objective).map_err(GoalServiceError::InvalidRequest)?;
        }
        if objective.is_some() || token_budget.is_some() {
            validate_goal_budget(token_budget.flatten(), max_goal_token_budget)
                .map_err(GoalServiceError::InvalidRequest)?;
        }

        if codex_alab_core::is_alab_binary()
            && objective.is_none()
            && status.is_none()
            && token_budget.is_none()
        {
            return Err(GoalServiceError::InvalidRequest(
                "goal update must change objective, status, or token budget".to_owned(),
            ));
        } // ALAB(storage): reject a read-only request before provenance, runtime/gap intent, accounting drain, or SQLite custody.

        let (runtime, mut alab_unowned_mutation) = self.alab_runtime_for_goal_mutation(thread_id); // ALAB(goal-recovery): an absent-owner intent fences successor import before any await or SQL.
        if alab_unowned_mutation
            .as_ref()
            .is_some_and(|mutation| !mutation.admitted())
        {
            return Err(GoalServiceError::Internal(
                "another ownerless goal mutation is already pending".to_owned(),
            ));
        } // ALAB(storage): reject overlap before recording an instruction; only one runtime-gap effect may own an exact epoch.
        // Hold this through the prepare/write window so idle continuation cannot
        // launch from goal state that this external mutation is about to change.
        let _goal_state_permit = match runtime.as_ref() {
            Some(_) if crate::runtime::alab_host_goal_mutation_permit_held(thread_id) => None,
            Some(runtime) => Some(
                runtime
                    .goal_state_permit()
                    .await
                    .map_err(GoalServiceError::Internal)?,
            ),
            None => None,
        };
        if let Some(runtime) = runtime.as_ref()
            && let Err(err) = runtime.prepare_external_goal_mutation().await
        {
            tracing::warn!("failed to prepare external goal mutation: {err}");
            if codex_alab_core::is_alab_binary() {
                return Err(GoalServiceError::Internal(err));
            } // ALAB(storage): a replacement cannot erase unflushed or ambiguously committed usage, including in vanilla mode.
        }

        let existing_goal = state_db
            .thread_goals()
            .get_thread_goal(thread_id)
            .await
            .map_err(|err| {
                GoalServiceError::Internal(format!("failed to read thread goal: {err}"))
            })?;
        if objective.is_none() && existing_goal.is_none() {
            return Err(GoalServiceError::InvalidRequest(format!(
                "cannot update goal for thread {thread_id}: no goal exists"
            )));
        }
        self.alab_record_goal_instruction(runtime.as_ref(), record_user_instruction)
            .await?; // ALAB(storage): healthy pre-write refusal does not manufacture a failed-persistence hold.
        if codex_alab_core::is_alab_binary() && !codex_alab_core::goal_dispatch::authorized() {
            return Err(GoalServiceError::InvalidRequest(
                "automatic goal provenance changed before SQLite dispatch".to_owned(),
            ));
        } // ALAB(model-stop): recheck mode/source authority after the instruction await and immediately before mutation creation.
        let (alab_boundary, mut alab_mutation) =
            self.alab_begin_dispatched_goal_mutation(runtime.as_ref())?; // ALAB(storage): freeze, begin, and authorize the exact native owner under one callback-excluding State guard.
        if let Some(mutation) = &mut alab_unowned_mutation
            && !mutation.mark_dispatched()
        {
            return Err(GoalServiceError::Internal(
                "goal persistence changed before SQLite dispatch".to_owned(),
            ));
        } // ALAB(storage): authorize against the live gap carry immediately before SQL.
        self.alab_arm_goal_chronology_dispatch(
            runtime.as_ref(),
            &mut alab_mutation,
            &mut alab_unowned_mutation,
        )?; // ALAB(goal-recovery): durable negative proof begins only after the exact SQL owner is authorized.

        let (goal, previous_goal) = if let Some(objective) = objective {
            if let Some(existing_goal) = existing_goal.as_ref() {
                let previous_goal = PreviousGoalSnapshot::from(existing_goal);
                state_db
                    .thread_goals()
                    .update_thread_goal(
                        thread_id,
                        codex_state::GoalUpdate {
                            objective: Some(objective.to_string()),
                            status,
                            token_budget,
                            expected_goal_id: Some(existing_goal.goal_id.clone()),
                        },
                    )
                    .await
                    .map_err(|err| {
                        self.alab_settle_goal_mutation_error(
                            runtime.as_ref(),
                            &mut alab_mutation,
                            &mut alab_unowned_mutation,
                            &err,
                        ); // ALAB(storage): distinguish a typed rollback from a settled post-commit read failure.
                        GoalServiceError::Internal(format!("failed to update thread goal: {err}"))
                    })?
                    .ok_or_else(|| {
                        self.alab_settle_goal_mutation_noop(
                            runtime.as_ref(),
                            &mut alab_mutation,
                            &mut alab_unowned_mutation,
                        ); // ALAB(goal-recovery): a conditional update that changed no row definitely did not consume its chronology generation.
                        GoalServiceError::InvalidRequest(format!(
                            "cannot update goal for thread {thread_id}: no goal exists"
                        ))
                    })
                    .map(|goal| (goal, Some(previous_goal)))?
            } else {
                state_db
                    .thread_goals()
                    .replace_thread_goal(
                        thread_id,
                        objective,
                        status.unwrap_or(codex_state::ThreadGoalStatus::Active),
                        token_budget.flatten().or(max_goal_token_budget),
                    )
                    .await
                    .map_err(|err| {
                        self.alab_settle_goal_mutation_error(
                            runtime.as_ref(),
                            &mut alab_mutation,
                            &mut alab_unowned_mutation,
                            &err,
                        ); // ALAB(storage): returned SQL errors are no longer running when this request reports them.
                        GoalServiceError::Internal(format!("failed to replace thread goal: {err}"))
                    })
                    .map(|goal| (goal, None))?
            }
        } else {
            let existing_goal = existing_goal.ok_or_else(|| {
                GoalServiceError::InvalidRequest(format!(
                    "cannot update goal for thread {thread_id}: no goal exists"
                ))
            })?;
            let previous_goal = PreviousGoalSnapshot::from(&existing_goal);
            let expected_goal_id = existing_goal.goal_id.clone();
            state_db
                .thread_goals()
                .update_thread_goal(
                    thread_id,
                    codex_state::GoalUpdate {
                        objective: None,
                        status,
                        token_budget,
                        expected_goal_id: Some(expected_goal_id),
                    },
                )
                .await
                .map_err(|err| {
                    self.alab_settle_goal_mutation_error(
                        runtime.as_ref(),
                        &mut alab_mutation,
                        &mut alab_unowned_mutation,
                        &err,
                    ); // ALAB(storage): cancellation remains permanent; a returned failure receives its typed settlement.
                    GoalServiceError::Internal(format!("failed to update thread goal: {err}"))
                })?
                .ok_or_else(|| {
                    self.alab_settle_goal_mutation_noop(
                        runtime.as_ref(),
                        &mut alab_mutation,
                        &mut alab_unowned_mutation,
                    ); // ALAB(goal-recovery): preserve only the durable instruction when the conditional SQL write was a no-op.
                    GoalServiceError::InvalidRequest(format!(
                        "cannot update goal for thread {thread_id}: no goal exists"
                    ))
                })
                .map(|goal| (goal, Some(previous_goal)))?
        };

        if codex_alab_core::is_alab_binary()
            && let Err(err) = state_db.thread_goals().alab_durable_goal_barrier().await
        {
            self.alab_settle_goal_mutation_unknown(&mut alab_mutation, &mut alab_unowned_mutation);
            return Err(GoalServiceError::Internal(format!(
                "goal update committed but its durability is unknown: {err}"
            )));
        } // ALAB(goal-recovery): no direct caller may apply or acknowledge a set before its SQLite row is power-loss durable.
        let alab_boundary = self.alab_commit_goal_mutation(
            thread_id,
            runtime.as_ref(),
            alab_boundary,
            alab_mutation,
            alab_unowned_mutation,
        ); // ALAB(storage): transfer the exact committed effect to either the original or an attached successor runtime.
        if !codex_alab_core::is_alab_binary()
            && let Some(runtime) = runtime.as_ref()
        {
            runtime.clear_pending_turn_start_options().await;
        }

        if objective.is_some() {
            fill_empty_thread_preview_if_possible(state_db, thread_id, &goal).await;
        }
        Ok(GoalSetOutcome {
            goal: protocol_goal_from_state(goal.clone()),
            state_goal: goal,
            previous_goal,
            _alab_boundary: alab_boundary,
        })
    }

    /// Records a clear instruction even when no goal exists, then deletes goal state under the mutation permit.
    pub async fn clear_thread_goal(
        &self,
        state_db: &codex_state::StateRuntime,
        thread_id: ThreadId,
        record_user_instruction: impl Future<Output = Result<(), GoalServiceError>> + Send,
    ) -> Result<GoalClearOutcome, GoalServiceError> {
        let (runtime, mut alab_unowned_mutation) = self.alab_runtime_for_goal_mutation(thread_id); // ALAB(goal-recovery): an absent-owner clear intent fences successor import before any await or SQL.
        if alab_unowned_mutation
            .as_ref()
            .is_some_and(|mutation| !mutation.admitted())
        {
            return Err(GoalServiceError::Internal(
                "another ownerless goal mutation is already pending".to_owned(),
            ));
        } // ALAB(storage): a concurrent gap request reaches neither provenance nor SQLite.
        // Hold this through the prepare/write window so idle continuation cannot
        // launch from goal state that this external mutation is about to change.
        let goal_state_permit = match runtime.as_ref() {
            Some(_) if crate::runtime::alab_host_goal_mutation_permit_held(thread_id) => None,
            Some(runtime) => Some(
                runtime
                    .goal_state_permit()
                    .await
                    .map_err(GoalServiceError::Internal)?,
            ),
            None => None,
        };
        if let Some(runtime) = runtime.as_ref()
            && let Err(err) = runtime.prepare_external_goal_mutation().await
        {
            tracing::warn!("failed to prepare external goal mutation: {err}");
            if codex_alab_core::is_alab_binary() {
                return Err(GoalServiceError::Internal(err));
            } // ALAB(storage): a replacement cannot erase unflushed or ambiguously committed usage, including in vanilla mode.
        }

        // A failed set can leave authorization in history without a goal row.
        self.alab_record_goal_instruction(runtime.as_ref(), record_user_instruction)
            .await?; // ALAB(storage): healthy pre-write refusal does not manufacture a failed-persistence hold.
        let (alab_boundary, mut alab_mutation) =
            self.alab_begin_dispatched_goal_mutation(runtime.as_ref())?; // ALAB(storage): exact final freeze and dispatch admission share one native State critical section.
        if let Some(mutation) = &mut alab_unowned_mutation
            && !mutation.mark_dispatched()
        {
            return Err(GoalServiceError::Internal(
                "goal persistence changed before SQLite dispatch".to_owned(),
            ));
        } // ALAB(storage): preserve the old row if another unknown command won the gap.
        self.alab_arm_goal_chronology_dispatch(
            runtime.as_ref(),
            &mut alab_mutation,
            &mut alab_unowned_mutation,
        )?; // ALAB(goal-recovery): a clear gets the same exact pre-SQL crash boundary as a set.

        let cleared_goal = state_db
            .thread_goals()
            .delete_thread_goal(thread_id)
            .await
            .map_err(|err| {
                self.alab_settle_goal_mutation_error(
                    runtime.as_ref(),
                    &mut alab_mutation,
                    &mut alab_unowned_mutation,
                    &err,
                ); // ALAB(storage): a returned delete error settles without manufacturing abandoned-command custody.
                GoalServiceError::Internal(format!("failed to clear thread goal: {err}"))
            })?;
        if codex_alab_core::is_alab_binary()
            && let Err(err) = state_db.thread_goals().alab_durable_goal_barrier().await
        {
            self.alab_settle_goal_mutation_unknown(&mut alab_mutation, &mut alab_unowned_mutation);
            return Err(GoalServiceError::Internal(format!(
                "goal clear committed but its durability is unknown: {err}"
            )));
        } // ALAB(goal-recovery): never publish or apply a clear before its successful DELETE is power-loss durable.
        let alab_boundary = self.alab_commit_goal_mutation(
            thread_id,
            runtime.as_ref(),
            alab_boundary,
            alab_mutation,
            alab_unowned_mutation,
        ); // ALAB(storage): successful clear transfers its exact effect to the runtime that owns settlement.
        if cleared_goal.is_some()
            && !codex_alab_core::is_alab_binary()
            && let Some(runtime) = runtime.as_ref()
        {
            runtime.clear_pending_turn_start_options().await;
        }
        drop(goal_state_permit);
        drop(runtime);

        Ok(GoalClearOutcome {
            cleared_goal,
            _alab_boundary: alab_boundary,
        }) // ALAB(goal-recovery): the caller owns the exact clear effect until its host chronology is durable.
    }

    pub(crate) fn register_runtime(&self, runtime: &Arc<GoalRuntimeHandle>) {
        self.alab_register_goal_runtime(runtime); // ALAB(storage-status): one live native owner sequences mutation and typed status publication.
    }

    pub(crate) async fn unregister_runtime(&self, runtime: &Arc<GoalRuntimeHandle>) {
        self.alab_unregister_goal_runtime(runtime).await; // ALAB(storage-status): retirement drains the owner's mutation permit before replacement.
    }

    pub(crate) fn runtime_for_thread(&self, thread_id: ThreadId) -> Option<Arc<GoalRuntimeHandle>> {
        let key = thread_id.to_string();
        let mut runtimes = self.runtimes();
        let published = runtimes.contains_key(&key);
        let runtime = runtimes.get(&key).and_then(Weak::upgrade);
        if runtime.is_none() {
            if published {
                self.alab_note_dead_goal_runtime(&key);
            } // ALAB(storage-status): a vanished published owner is an unclean permanent boundary, never pristine absence.
            runtimes.remove(&key);
        }
        runtime
    }

    fn runtimes(&self) -> std::sync::MutexGuard<'_, HashMap<String, Weak<GoalRuntimeHandle>>> {
        self.runtimes.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
