use codex_core::ThreadManager;
use std::sync::Arc;
use std::sync::Weak;

use codex_extension_api::FunctionCallError;
use codex_extension_api::JsonToolOutput;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolOutput;
use codex_extension_api::ToolSpec;
use codex_protocol::ThreadId;
use codex_protocol::protocol::ThreadGoal;
use codex_protocol::protocol::ThreadGoalStatus;
use codex_protocol::protocol::validate_thread_goal_objective;
use serde::Deserialize;
use serde::Serialize;

use crate::accounting::BudgetLimitedGoalDisposition;
use crate::accounting::GoalAccountingState;
use crate::analytics::GoalAnalytics;
use crate::analytics::GoalEventAttribution;
use crate::events::GoalEventEmitter;
use crate::metrics::GoalMetrics;
use crate::runtime::GoalRuntimeHandle;
use crate::spec::CREATE_GOAL_TOOL_NAME;
use crate::spec::GET_GOAL_TOOL_NAME;
use crate::spec::UPDATE_GOAL_TOOL_NAME;
use crate::spec::create_create_goal_tool;
use crate::spec::create_get_goal_tool;
use crate::spec::create_update_goal_tool;
use crate::steering::GOAL_OBJECTIVE_CONTEXT;

#[path = "../../../alab-autopilot/glue/goal_persistence_tool.rs"]
mod alab_persistence; // ALAB(storage): dispatched usage owns its commit bookkeeping independently of tool cancellation.

#[derive(Clone)]
pub(crate) struct GoalToolExecutor {
    pub(crate) alab_runtime: Option<GoalRuntimeHandle>, // ALAB(storage): tool mutations share the native goal permit with explicit changes.
    pub(crate) execution_allowed: bool,
    kind: GoalToolKind,
    thread_id: ThreadId,
    state_db: Arc<codex_state::StateRuntime>,
    accounting_state: Arc<GoalAccountingState>,
    analytics: GoalAnalytics,
    event_emitter: GoalEventEmitter,
    metrics: GoalMetrics,
    max_goal_token_budget: Option<i64>,
}

#[derive(Clone)]
enum GoalToolKind {
    Get,
    Create(Weak<ThreadManager>),
    Update,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CreateGoalRequest {
    pub objective: String,
    pub token_budget: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct UpdateGoalArgs {
    status: ThreadGoalStatus,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct GoalToolResponse {
    goal: Option<ThreadGoal>,
    remaining_tokens: Option<i64>,
    completion_budget_report: Option<String>,
}

#[derive(Clone, Copy)]
enum CompletionBudgetReport {
    Include,
    Omit,
}

impl GoalToolExecutor {
    pub(crate) fn get(
        thread_id: ThreadId,
        state_db: Arc<codex_state::StateRuntime>,
        accounting_state: Arc<GoalAccountingState>,
        analytics: GoalAnalytics,
        event_emitter: GoalEventEmitter,
        metrics: GoalMetrics,
    ) -> Self {
        Self {
            alab_runtime: None, // ALAB(storage): extension binds the actual runtime before exposing tools.
            kind: GoalToolKind::Get,
            execution_allowed: true,
            thread_id,
            state_db,
            accounting_state,
            analytics,
            event_emitter,
            metrics,
            max_goal_token_budget: None,
        }
    }

    pub(crate) fn create(
        runtime: &GoalRuntimeHandle,
        thread_manager: Weak<ThreadManager>,
        state_db: Arc<codex_state::StateRuntime>,
        analytics: GoalAnalytics,
        event_emitter: GoalEventEmitter,
        metrics: GoalMetrics,
        max_goal_token_budget: Option<i64>,
    ) -> Self {
        Self {
            alab_runtime: Some(runtime.clone()), // ALAB(storage): creation drains the original goal before replacing it.
            kind: GoalToolKind::Create(thread_manager),
            execution_allowed: true,
            thread_id: runtime.thread_id(),
            state_db,
            accounting_state: runtime.accounting_state(),
            analytics,
            event_emitter,
            metrics,
            max_goal_token_budget,
        }
    }

    pub(crate) fn update(
        thread_id: ThreadId,
        state_db: Arc<codex_state::StateRuntime>,
        accounting_state: Arc<GoalAccountingState>,
        analytics: GoalAnalytics,
        event_emitter: GoalEventEmitter,
        metrics: GoalMetrics,
    ) -> Self {
        Self {
            alab_runtime: None, // ALAB(storage): extension binds the actual runtime before exposing tools.
            kind: GoalToolKind::Update,
            execution_allowed: true,
            thread_id,
            state_db,
            accounting_state,
            analytics,
            event_emitter,
            metrics,
            max_goal_token_budget: None,
        }
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for GoalToolExecutor {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(match self.kind {
            GoalToolKind::Get => GET_GOAL_TOOL_NAME,
            GoalToolKind::Create(_) => CREATE_GOAL_TOOL_NAME,
            GoalToolKind::Update => UPDATE_GOAL_TOOL_NAME,
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        matches!(self.kind, GoalToolKind::Get)
    }

    fn spec(&self) -> ToolSpec {
        match self.kind {
            GoalToolKind::Get => create_get_goal_tool(),
            GoalToolKind::Create(_) => create_create_goal_tool(),
            GoalToolKind::Update => create_update_goal_tool(),
        }
    }

    fn handle<'a>(
        &'a self,
        invocation: ToolCall<'call>,
    ) -> codex_extension_api::ToolExecutorFuture<'a>
    where
        'call: 'a,
    {
        Box::pin(async move {
            if !self.execution_allowed {
                return Err(FunctionCallError::RespondToModel(
                    "Goal tools require a persistent thread.".to_string(),
                ));
            }
            match &self.kind {
                GoalToolKind::Get => self.handle_get(invocation).await,
                GoalToolKind::Create(thread_manager) => {
                    self.handle_create(invocation, thread_manager).await
                }
                GoalToolKind::Update => self.handle_update(invocation).await,
            }
        })
    }
}

impl GoalToolExecutor {
    async fn handle_get(
        &self,
        invocation: ToolCall<'_>,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        let _ = invocation.function_arguments()?;
        let goal = self
            .state_db
            .thread_goals()
            .get_thread_goal(self.thread_id)
            .await
            .map(|goal| goal.map(protocol_goal_from_state))
            .map_err(|err| {
                FunctionCallError::RespondToModel(format!("failed to read goal: {err}"))
            })?;
        goal_response(goal, CompletionBudgetReport::Omit)
    }

    async fn handle_create(
        &self,
        invocation: ToolCall<'_>,
        thread_manager: &Weak<ThreadManager>,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        let mut request: CreateGoalRequest = parse_arguments(invocation.function_arguments()?)?;
        request.objective = request.objective.trim().to_string();
        validate_thread_goal_objective(&request.objective)
            .map_err(FunctionCallError::RespondToModel)?;
        request.token_budget = request.token_budget.or(self.max_goal_token_budget);
        validate_goal_budget(request.token_budget, self.max_goal_token_budget)
            .map_err(FunctionCallError::RespondToModel)?;

        let mut alab_mutation = self.alab_prepare_create().await?; // ALAB(storage): preserve retained usage before creating a new goal.
        alab_mutation.begin_dispatched_mutation(&self.accounting_state)?; // ALAB(storage): final freeze, mutation creation, and dispatch admission exclude callbacks as one operation.
        let goal = self
            .state_db
            .thread_goals()
            .insert_thread_goal(
                self.thread_id,
                request.objective.as_str(),
                codex_state::ThreadGoalStatus::Active,
                request.token_budget,
            )
            .await
            .map_err(|err| {
                alab_mutation.settle_storage_error(&self.accounting_state, &err); // ALAB(storage): returned SQL errors settle recoverably; only cancellation leaves an abandoned command.
                FunctionCallError::RespondToModel(format!("failed to create goal: {err}"))
            })?
            .ok_or_else(|| {
                alab_mutation.settle_noop(&self.accounting_state); // ALAB(storage): a uniqueness refusal changed no row and cannot leave an abandoned mutation owner.
                FunctionCallError::RespondToModel(
                    "cannot create a new goal because this thread has an unfinished goal; complete the existing goal first"
                        .to_string(),
                )
            })?;
        alab_mutation
            .make_write_durable(self.state_db.as_ref())
            .await?; // ALAB(goal-recovery): a model-visible commit is durable before its exact runtime/source witness can advance.
        let alab_effect =
            alab_mutation.commit(self.thread_id, &invocation.turn_id, &invocation.call_id); // ALAB(storage): acknowledge and identify only this persisted fresh-goal intent.
        let turn_id = if !codex_alab_core::is_alab_binary() {
            self.accounting_state
                .mark_current_turn_goal_active(goal.goal_id.clone())
        } else if let Some(effect) = alab_effect {
            self.alab_apply_created_goal_effect(effect, goal.goal_id.clone())
                .await
        } else {
            self.accounting_state.current_turn_id()
        }; // ALAB(storage): only the exact committed effect can replace retained predecessor attribution.
        fill_empty_thread_preview_if_possible(self.state_db.as_ref(), self.thread_id, &goal).await;
        if let Some(manager) = thread_manager.upgrade()
            && let Ok(thread) = manager.get_thread(self.thread_id).await
        {
            thread
                .inject_untrusted_context_for_turn(
                    &invocation.turn_id,
                    GOAL_OBJECTIVE_CONTEXT,
                    goal.objective.clone(),
                )
                .await;
        }
        self.metrics.record_created();
        self.analytics.created(
            &goal,
            GoalEventAttribution::Turn(invocation.turn_id.as_str()),
        );
        let goal = protocol_goal_from_state(goal);
        if codex_alab_core::is_alab_binary()
            && turn_id.as_deref() == Some(invocation.turn_id.as_str())
        {
            self.alab_install_created_goal_turn_options(thread_manager, &invocation)
                .await;
        } // ALAB(storage): publish create-goal continuation ancestry while its exact mutation permit still excludes later external effects.
        self.emit_goal_updated_from_tool_call(&invocation, turn_id, goal.clone());
        goal_response(Some(goal), CompletionBudgetReport::Omit)
    }

    async fn handle_update(
        &self,
        invocation: ToolCall<'_>,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        let args: UpdateGoalArgs = parse_arguments(invocation.function_arguments()?)?;
        if !matches!(
            args.status,
            ThreadGoalStatus::Complete | ThreadGoalStatus::Blocked | ThreadGoalStatus::Paused
        ) {
            return Err(FunctionCallError::RespondToModel(
                "update_goal can only mark the existing goal complete, blocked, or paused at the user's explicit request; resume, budget-limited, and usage-limited status changes are controlled by the user or system"
                    .to_string(),
            ));
        }

        let mut alab_stop = self.alab_prepare_stop().await?; // ALAB(storage): failed stop intent holds continuation; explicit mutations cannot interleave accounting and status.
        if !self
            .accounting_state
            .alab_persistence
            .settled_mutation_unknown()
        {
            // ALAB(storage): supersede a returned command without replaying its deliberately frozen boundary.
            self.account_active_goal_progress(
                match args.status {
                    ThreadGoalStatus::Complete => codex_state::GoalAccountingMode::ActiveOrComplete,
                    ThreadGoalStatus::Blocked | ThreadGoalStatus::Paused => {
                        codex_state::GoalAccountingMode::ActiveOrStopped
                    }
                    ThreadGoalStatus::Active
                    | ThreadGoalStatus::UsageLimited
                    | ThreadGoalStatus::BudgetLimited => unreachable!("status validated above"),
                },
                invocation.call_id.as_str(),
                BudgetLimitedGoalDisposition::KeepActive,
            )
            .await?;
        }
        let previous_status = self
            .current_goal_status_for_metrics(/*expected_goal_id*/ None)
            .await?;
        alab_stop.begin_dispatched_mutation(&self.accounting_state)?; // ALAB(storage): current-turn accounting and the exact frozen dispatch boundary cannot be split by a callback.
        let goal = self
            .state_db
            .thread_goals()
            .update_thread_goal(
                self.thread_id,
                codex_state::GoalUpdate {
                    objective: None,
                    status: Some(state_status_from_protocol(args.status)),
                    token_budget: None,
                    expected_goal_id: None,
                },
            )
            .await
            .map_err(|err| {
                alab_stop.settle_storage_error(&self.accounting_state, &err); // ALAB(storage): preserve post-commit read ambiguity while reopening only proven rollbacks.
                FunctionCallError::RespondToModel(format!("failed to update goal: {err}"))
            })?
            .ok_or_else(|| {
                alab_stop.settle_noop(&self.accounting_state); // ALAB(storage): a conditional no-row update is a proven rollback, not ambiguous custody.
                FunctionCallError::RespondToModel(
                    "cannot update goal because this thread has no goal".to_string(),
                )
            })?;
        alab_stop.make_write_durable(self.state_db.as_ref()).await?; // ALAB(goal-recovery): a durable model stop cannot be lost after its source record settles the chronology fence.
        let alab_goal_id = goal.goal_id.clone(); // ALAB(storage): a late same-goal callback may retain the exact committed stop's accounting boundary.
        let protocol_goal = protocol_goal_from_state(goal.clone()); // ALAB(storage): settle the exact mutation before any accounting clear can erase its predecessor.
        let alab_effect =
            alab_stop.commit_stop(&protocol_goal, &invocation.turn_id, &invocation.call_id); // ALAB(storage): acknowledge the persisted stop while its native witness is still exact.
        self.metrics
            .record_terminal_if_status_changed(previous_status, &goal);
        self.analytics.status_changed(
            &goal,
            previous_status,
            GoalEventAttribution::Turn(invocation.turn_id.as_str()),
        );
        let goal = protocol_goal;
        let turn_id = if !codex_alab_core::is_alab_binary() {
            self.accounting_state.clear_current_turn_goal()
        } else if let Some(effect) = alab_effect {
            self.alab_apply_stopped_goal_effect(effect, &alab_goal_id)
                .await
        } else {
            self.accounting_state.current_turn_id()
        }; // ALAB(storage): callbacks after commit cannot be erased by an unscoped clear.
        self.emit_goal_updated_from_tool_call(&invocation, turn_id, goal.clone());
        goal_response(
            Some(goal),
            if args.status == ThreadGoalStatus::Complete {
                CompletionBudgetReport::Include
            } else {
                CompletionBudgetReport::Omit
            },
        )
    }

    fn emit_goal_updated_from_tool_call(
        &self,
        invocation: &ToolCall<'_>,
        turn_id: Option<String>,
        goal: ThreadGoal,
    ) {
        self.event_emitter
            .thread_goal_updated(invocation.call_id.clone(), turn_id, goal);
    }

    async fn account_active_goal_progress(
        &self,
        mode: codex_state::GoalAccountingMode,
        event_id: &str,
        budget_limited_goal_disposition: BudgetLimitedGoalDisposition,
    ) -> Result<Option<ThreadGoal>, FunctionCallError> {
        if codex_alab_core::is_alab_binary()
            && !codex_alab_core::goal_persistence::owns_current_write()
        {
            return self
                .alab_owned_active_progress(mode, event_id, budget_limited_goal_disposition)
                .await;
        } // ALAB(storage): the dispatched write owns native baseline finalization in either experience.
        let Some(turn_id) = self.accounting_state.current_turn_id() else {
            return Ok(None);
        };
        let _accounting_permit = self
            .accounting_state
            .progress_accounting_permit()
            .await
            .map_err(|err| {
                FunctionCallError::Fatal(format!(
                    "goal progress accounting semaphore closed: {err}"
                ))
            })?;
        if codex_alab_core::is_alab_binary()
            && !self.accounting_state.alab_persistence.permits_write()
        {
            return Err(FunctionCallError::RespondToModel(
                "goal usage remains held after an ambiguous write".to_owned(),
            ));
        } // ALAB(storage): a queued writer rechecks after the previous outcome is published.
        let Some(snapshot) = self.accounting_state.progress_snapshot(turn_id.as_str()) else {
            return Ok(None);
        };
        let previous_status = self
            .current_goal_status_for_metrics(Some(snapshot.expected_goal_id.as_str()))
            .await
            .inspect_err(|_| {
                codex_alab_core::goal_persistence::definitely_rolled_back();
            })?; // ALAB(storage): metrics-read failure precedes the additive write.
        let outcome = self
            .state_db
            .thread_goals()
            .account_thread_goal_usage(
                self.thread_id,
                snapshot.time_delta_seconds,
                snapshot.token_delta,
                mode,
                Some(snapshot.expected_goal_id.as_str()),
            )
            .await
            .map_err(|err| {
                if codex_state::alab_goal_read_recovery::write_definitely_uncommitted(&err) {
                    codex_alab_core::goal_persistence::definitely_rolled_back();
                } else if codex_alab_core::is_alab_binary() {
                    self.accounting_state.alab_persistence.block_ambiguous();
                } // ALAB(storage): preserve typed rollback knowledge before the model-visible error.
                FunctionCallError::RespondToModel(format!("failed to account goal progress: {err}"))
            })?;
        if codex_alab_core::is_alab_binary()
            && matches!(&outcome, codex_state::GoalAccountingOutcome::Updated(_))
            && let Err(error) = self
                .state_db
                .thread_goals()
                .alab_durable_goal_barrier()
                .await
        {
            self.accounting_state.alab_persistence.block_ambiguous();
            return Err(FunctionCallError::RespondToModel(format!(
                "goal usage committed but its durability could not be proven: {error}"
            )));
        } // ALAB(goal-recovery): acknowledged usage cannot disappear across power loss and reopen spent budget.
        Ok(match outcome {
            codex_state::GoalAccountingOutcome::Updated(goal) => {
                self.metrics
                    .record_terminal_if_status_changed(previous_status, &goal);
                self.analytics
                    .usage_accounted(&goal, GoalEventAttribution::Turn(turn_id.as_str()));
                self.analytics.status_changed(
                    &goal,
                    previous_status,
                    GoalEventAttribution::Turn(turn_id.as_str()),
                );
                self.accounting_state.mark_progress_accounted_for_status(
                    turn_id.as_str(),
                    &snapshot,
                    goal.status,
                    budget_limited_goal_disposition,
                );
                let goal = protocol_goal_from_state(goal);
                self.event_emitter.thread_goal_updated(
                    event_id.to_string(),
                    Some(turn_id),
                    goal.clone(),
                );
                Some(goal)
            }
            codex_state::GoalAccountingOutcome::Unchanged(goal) => {
                self.accounting_state
                    .alab_retain_rejected_progress(&snapshot.expected_goal_id, goal.as_ref())
                    .map_err(FunctionCallError::RespondToModel)?;
                None
            } // ALAB(storage): retain positive usage if the original goal refuses this accounting mode or identity.
        })
    }

    async fn current_goal_status_for_metrics(
        &self,
        expected_goal_id: Option<&str>,
    ) -> Result<Option<codex_state::ThreadGoalStatus>, FunctionCallError> {
        let goal = self
            .state_db
            .thread_goals()
            .get_thread_goal(self.thread_id)
            .await
            .map_err(|err| {
                FunctionCallError::RespondToModel(format!(
                    "failed to read goal metrics status: {err}"
                ))
            })?;
        Ok(goal.and_then(|goal| {
            expected_goal_id
                .is_none_or(|expected_goal_id| goal.goal_id == expected_goal_id)
                .then_some(goal.status)
        }))
    }
}

fn parse_arguments<T>(arguments: &str) -> Result<T, FunctionCallError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_str(arguments)
        .map_err(|err| FunctionCallError::RespondToModel(err.to_string()))
}

pub(crate) fn validate_goal_budget(
    value: Option<i64>,
    max_goal_token_budget: Option<i64>,
) -> Result<(), String> {
    if let Some(value) = value
        && value <= 0
    {
        return Err("goal budgets must be positive when provided".to_string());
    }
    if let Some(value) = value
        && let Some(max_goal_token_budget) = max_goal_token_budget
        && value > max_goal_token_budget
    {
        return Err(format!(
            "goal token budget {value} exceeds the maximum allowed goal token budget of {max_goal_token_budget}"
        ));
    }
    Ok(())
}

fn goal_response(
    goal: Option<ThreadGoal>,
    completion_budget_report: CompletionBudgetReport,
) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
    let value = serde_json::to_value(GoalToolResponse::new(goal, completion_budget_report))
        .map_err(|err| FunctionCallError::Fatal(err.to_string()))?;
    Ok(Box::new(JsonToolOutput::new(value)))
}

impl GoalToolResponse {
    fn new(goal: Option<ThreadGoal>, report_mode: CompletionBudgetReport) -> Self {
        let remaining_tokens = goal.as_ref().and_then(|goal| {
            goal.token_budget
                .map(|budget| (budget - goal.tokens_used).max(0))
        });
        let completion_budget_report = match report_mode {
            CompletionBudgetReport::Include => goal
                .as_ref()
                .filter(|goal| goal.status == ThreadGoalStatus::Complete)
                .and_then(completion_budget_report),
            CompletionBudgetReport::Omit => None,
        };
        Self {
            goal,
            remaining_tokens,
            completion_budget_report,
        }
    }
}

pub(crate) async fn fill_empty_thread_preview_if_possible(
    state_db: &codex_state::StateRuntime,
    thread_id: ThreadId,
    goal: &codex_state::ThreadGoal,
) {
    if let Err(err) = state_db
        .set_thread_preview_if_empty(thread_id, goal.objective.as_str())
        .await
    {
        tracing::warn!(
            "failed to set empty thread preview from goal objective for {thread_id}: {err}"
        );
    }
}

pub(crate) fn protocol_goal_from_state(goal: codex_state::ThreadGoal) -> ThreadGoal {
    ThreadGoal {
        thread_id: goal.thread_id,
        objective: goal.objective,
        status: protocol_status_from_state(goal.status),
        token_budget: goal.token_budget,
        tokens_used: goal.tokens_used,
        time_used_seconds: goal.time_used_seconds,
        created_at: goal.created_at.timestamp(),
        updated_at: goal.updated_at.timestamp(),
    }
}

fn protocol_status_from_state(status: codex_state::ThreadGoalStatus) -> ThreadGoalStatus {
    match status {
        codex_state::ThreadGoalStatus::Active => ThreadGoalStatus::Active,
        codex_state::ThreadGoalStatus::Paused => ThreadGoalStatus::Paused,
        codex_state::ThreadGoalStatus::Blocked => ThreadGoalStatus::Blocked,
        codex_state::ThreadGoalStatus::UsageLimited => ThreadGoalStatus::UsageLimited,
        codex_state::ThreadGoalStatus::BudgetLimited => ThreadGoalStatus::BudgetLimited,
        codex_state::ThreadGoalStatus::Complete => ThreadGoalStatus::Complete,
    }
}

pub(crate) fn state_status_from_protocol(
    status: ThreadGoalStatus,
) -> codex_state::ThreadGoalStatus {
    match status {
        ThreadGoalStatus::Active => codex_state::ThreadGoalStatus::Active,
        ThreadGoalStatus::Paused => codex_state::ThreadGoalStatus::Paused,
        ThreadGoalStatus::Blocked => codex_state::ThreadGoalStatus::Blocked,
        ThreadGoalStatus::UsageLimited => codex_state::ThreadGoalStatus::UsageLimited,
        ThreadGoalStatus::BudgetLimited => codex_state::ThreadGoalStatus::BudgetLimited,
        ThreadGoalStatus::Complete => codex_state::ThreadGoalStatus::Complete,
    }
}

fn completion_budget_report(goal: &ThreadGoal) -> Option<String> {
    if goal.token_budget.is_none() && goal.time_used_seconds <= 0 {
        None
    } else {
        Some(
            "Goal achieved. Report final usage from this tool result's structured goal fields. If `goal.tokenBudget` is present, include token usage from `goal.tokensUsed` and `goal.tokenBudget`. If `goal.timeUsedSeconds` is greater than 0, summarize elapsed time in a concise, human-friendly form appropriate to the response language."
                .to_string(),
        )
    }
}
