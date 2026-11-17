use std::sync::Arc;
use std::sync::Weak;

use codex_analytics::AnalyticsEventsClient;
use codex_core::ThreadManager;
use codex_core::TurnStartOptions;
use codex_extension_api::ConfigContributor;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionEventSink;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ThreadIdleInput;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadResumeInput;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ThreadStopInput;
use codex_extension_api::TokenUsageContributor;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolCallOutcome;
use codex_extension_api::ToolContributor;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolFinishInput;
use codex_extension_api::ToolLifecycleContributor;
use codex_extension_api::ToolLifecycleFuture;
use codex_extension_api::TurnAbortInput;
use codex_extension_api::TurnErrorInput;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStartInput;
use codex_extension_api::TurnStopInput;
use codex_otel::MetricsClient;
use codex_protocol::ThreadId;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::protocol::ThreadGoalStatus;
use codex_protocol::protocol::TokenUsageInfo;

use crate::accounting::BudgetLimitedGoalDisposition;
use crate::accounting::GoalAccountingState;
use crate::analytics::GoalAnalytics;
use crate::api::GoalService;
use crate::events::GoalEventEmitter;
use crate::metrics::GoalMetrics;
use crate::runtime::ActiveGoalStopReason;
use crate::runtime::GoalRuntimeConfig;
use crate::runtime::GoalRuntimeHandle;
use crate::spec::CREATE_GOAL_TOOL_NAME;
use crate::spec::UPDATE_GOAL_TOOL_NAME;
use crate::steering::budget_limit_steering_item;
use crate::tool::GoalToolExecutor;

#[derive(Clone, Debug)]
pub struct GoalExtensionConfig {
    pub enabled: bool,
    pub max_goal_token_budget: Option<i64>,
}

#[derive(Clone)]
pub struct GoalExtension<C> {
    state_dbs: Arc<codex_state::StateRuntime>,
    analytics: GoalAnalytics,
    event_emitter: GoalEventEmitter,
    metrics: GoalMetrics,
    thread_manager: Weak<ThreadManager>,
    goal_service: Arc<GoalService>,
    goal_config: Arc<dyn Fn(&C) -> GoalExtensionConfig + Send + Sync>,
}

impl<C> std::fmt::Debug for GoalExtension<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoalExtension").finish_non_exhaustive()
    }
}

impl<C> GoalExtension<C> {
    pub(crate) fn new_with_host_capabilities(
        state_dbs: Arc<codex_state::StateRuntime>,
        analytics_events_client: AnalyticsEventsClient,
        event_sink: Arc<dyn ExtensionEventSink>,
        metrics_client: Option<MetricsClient>,
        thread_manager: Weak<ThreadManager>,
        goal_service: Arc<GoalService>,
        goal_config: impl Fn(&C) -> GoalExtensionConfig + Send + Sync + 'static,
    ) -> Self {
        Self {
            state_dbs,
            analytics: GoalAnalytics::new(analytics_events_client),
            event_emitter: GoalEventEmitter::new(event_sink),
            metrics: GoalMetrics::new(metrics_client),
            thread_manager,
            goal_service,
            goal_config: Arc::new(goal_config),
        }
    }
}

impl<C> ThreadLifecycleContributor<C> for GoalExtension<C>
where
    C: Send + Sync + 'static,
{
    fn on_thread_start<'a>(&'a self, input: ThreadStartInput<'a, C>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let config = (self.goal_config)(input.config);
            let enabled = config.enabled;
            let tools_visible_for_thread = !matches!(
                input.session_source,
                SessionSource::SubAgent(SubAgentSource::Review)
            );
            let tools_available_for_thread =
                input.persistent_thread_state_available && tools_visible_for_thread;
            input.thread_store.insert(config);
            let accounting_state = input
                .thread_store
                .get_or_init::<GoalAccountingState>(GoalAccountingState::default);
            let Ok(thread_id) = ThreadId::from_string(input.thread_store.level_id()) else {
                return;
            };
            let root_accounting_state = input
                .session_source
                .parent_thread_id()
                .or_else(|| {
                    ThreadId::from_string(input.session_store.level_id())
                        .ok()
                        .filter(|_| input.session_source.is_non_root_agent())
                })
                .and_then(|parent_thread_id| {
                    self.goal_service
                        .runtime_for_thread(parent_thread_id)
                        .or_else(|| {
                            ThreadId::from_string(input.session_store.level_id())
                                .ok()
                                .and_then(|root_thread_id| {
                                    self.goal_service.runtime_for_thread(root_thread_id)
                                })
                        })
                })
                .map(|parent| {
                    parent
                        .root_accounting_state()
                        .unwrap_or_else(|| parent.accounting_state())
                });
            let runtime = input.thread_store.get_or_init::<GoalRuntimeHandle>(|| {
                GoalRuntimeHandle::new(
                    thread_id,
                    Arc::clone(&self.state_dbs),
                    self.event_emitter.clone(),
                    self.metrics.clone(),
                    self.thread_manager.clone(),
                    accounting_state,
                    GoalRuntimeConfig {
                        analytics: self.analytics.clone(),
                        enabled,
                        tools_available_for_thread,
                        tools_visible_for_thread,
                        root_accounting_state,
                    },
                )
            });
            runtime.set_enabled(enabled);
            runtime.alab_bind_retained_goal_gate(input.thread_store.get::<codex_alab_core::retained_goal_gate::Gate>()); // ALAB(retained-frontier): bind before registering the actual goal runtime.
            runtime.alab_bind_startup_hold(
                input
                    .thread_store
                    .get::<codex_alab_core::upgrade_startup_hold::Fence>(),
            ); // ALAB(upgrade): bind the actual native startup hold before publishing the runtime.
            self.goal_service.register_runtime(&runtime);
        })
    }

    fn on_thread_resume<'a>(&'a self, input: ThreadResumeInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(runtime) = goal_runtime_handle(input.thread_store) else {
                return;
            };

            if let Err(err) = runtime.restore_after_resume().await {
                tracing::warn!(
                    "failed to restore goal runtime after thread resume for {}: {err}",
                    runtime.thread_id()
                );
            }
        })
    }

    fn on_thread_idle<'a>(&'a self, input: ThreadIdleInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(runtime) = goal_runtime_handle(input.thread_store) else {
                return;
            };

            let accounting = runtime.accounting_state();
            if let Some(turn_id) = accounting.interrupted_turn_id() {
                if let Err(err) = runtime
                    .stop_active_goal_for_turn(&turn_id, ActiveGoalStopReason::EmptyResponse)
                    .await
                {
                    tracing::warn!("failed to stop goal after interrupted empty turns: {err}");
                    if codex_alab_core::is_alab_binary() {
                        return;
                    } // ALAB(storage): keep the interrupted turn and its unflushed usage after failed persistence in either experience.
                }
                accounting.finish_turn(&turn_id);
            }
            if let Err(err) = runtime.continue_if_idle().await {
                tracing::warn!(
                    "failed to continue active goal for idle thread {}: {err}",
                    runtime.thread_id()
                );
            }
        })
    }

    fn on_thread_stop<'a>(&'a self, input: ThreadStopInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if let Some(runtime) = goal_runtime_handle(input.thread_store) {
                runtime.alab_cancel_goal_admission_recovery(); // ALAB(goal-recovery): teardown retires a pending automatic admission
                self.goal_service.unregister_runtime(&runtime).await; // ALAB(storage-status): native retirement waits for already-admitted mutations.
            }
        })
    }
}

impl<C> ConfigContributor<C> for GoalExtension<C>
where
    C: Send + Sync + 'static,
{
    fn on_config_changed(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
        _previous_config: &C,
        new_config: &C,
    ) {
        let config = (self.goal_config)(new_config);
        let enabled = config.enabled;
        thread_store.insert(config);
        if let Some(runtime) = goal_runtime_handle(thread_store) {
            runtime.set_enabled(enabled);
        }
    }
}

impl<C> TurnLifecycleContributor for GoalExtension<C>
where
    C: Send + Sync + 'static,
{
    fn on_turn_start<'a>(&'a self, input: TurnStartInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(runtime) = goal_runtime_handle(input.thread_store) else {
                return;
            };
            runtime.alab_clear_policy_continuation(); // ALAB(policy-resume): a newly admitted turn owns the next recovery sequence.
            if !runtime.is_enabled() {
                return;
            }

            let Some(token_usage_at_turn_start) = input.token_usage_at_turn_start else {
                tracing::warn!("skipping goal turn accounting: token baseline unavailable");
                return;
            };
            runtime.alab_auth_turn_started(); // ALAB(renewal): native admission now owns the original work.
            if matches!(
                input.collaboration_mode.mode,
                codex_protocol::config_types::ModeKind::Plan
            ) {
                runtime.alab_prepare_plan_accounting().await;
            } // ALAB(storage): preserve pre-Plan usage before native Plan mode stops its accounting clock.

            let accounting = runtime.accounting_state();
            let Ok(_turn_start_permit) = accounting.turn_start_permit().await else {
                return;
            };
            if let Err(err) = self
                .state_dbs
                .thread_goals()
                .clear_thread_goal_continuation_deferral(runtime.thread_id())
                .await
            {
                tracing::warn!("failed to clear deferred goal continuation: {err}");
            }

            accounting.start_turn(
                input.turn_id,
                input.collaboration_mode.mode,
                token_usage_at_turn_start,
            );
            if matches!(
                input.collaboration_mode.mode,
                codex_protocol::config_types::ModeKind::Plan
            ) {
                accounting.clear_current_turn_goal();
                return;
            }
            let Ok(goal) = self
                .state_dbs
                .thread_goals()
                .get_thread_goal(runtime.thread_id())
                .await
            else {
                return;
            };
            if !accounting
                .alab_goal_association_allowed(goal.as_ref().map(|goal| goal.goal_id.as_str()))
            {
                return;
            } // ALAB(storage): raw replacement cannot reset a failed original goal's unacknowledged shared usage.
            let active_goal = goal.as_ref().filter(|goal| {
                matches!(
                    goal.status,
                    codex_state::ThreadGoalStatus::Active
                        | codex_state::ThreadGoalStatus::BudgetLimited
                )
            });
            let associated_goal = active_goal.and_then(|goal| {
                accounting
                    .mark_turn_goal_active(input.turn_id, goal.goal_id.clone())
                    .then(|| goal.goal_id.clone())
            }); // ALAB(storage): association rechecks the clean generation under the same State lock that changes native baselines.
            if codex_alab_core::active() {
                input
                    .turn_store
                    .insert(codex_alab_core::renewal::GoalAtTurnStart {
                        goal_id: associated_goal,
                    });
            } // ALAB(renewal): retain only the exact native association even if this turn later completes its goal.
        })
    }

    fn on_item_completed<'a>(
        &'a self,
        thread_store: &'a ExtensionData,
        turn_store: &'a ExtensionData,
        item: &'a TurnItem,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if let Some(runtime) = goal_runtime_handle(thread_store)
                && runtime.is_enabled()
            {
                runtime
                    .accounting_state()
                    .record_item(turn_store.level_id(), item);
            }
        })
    }

    fn on_turn_stop<'a>(&'a self, input: TurnStopInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(runtime) = goal_runtime_handle(input.thread_store) else {
                return;
            };
            if !runtime.is_enabled() {
                return;
            }

            let turn_id = input.turn_store.level_id();
            let _alab_finished = runtime.accounting_state().alab_finished_turn(turn_id); // ALAB(storage): failed final accounting retains usage, not live-turn ownership.
            if let Some(expected_goal_id) =
                runtime.accounting_state().execution_failure_goal(turn_id)
                && let Err(err) = runtime
                    .stop_active_goal_for_turn(
                        turn_id,
                        ActiveGoalStopReason::ExecutionUnavailable { expected_goal_id },
                    )
                    .await
            {
                input.thread_store.remove::<TurnStartOptions>();
                tracing::warn!(
                    "failed to stop active goal after repeated execution failures for {turn_id}: {err}"
                );
                return;
            }
            if let Err(err) = runtime
                .stop_active_goal_for_turn(turn_id, ActiveGoalStopReason::EmptyResponse)
                .await
            {
                input.thread_store.remove::<TurnStartOptions>();
                tracing::warn!("failed to stop goal after empty responses for {turn_id}: {err}");
                return;
            }
            if let Err(err) = runtime
                .alab_account_ending_turn_progress(turn_id, &format!("{turn_id}:turn-stop"))
                .await
            // ALAB(storage): late same-goal tokens settle against the durable terminal status selected under the native goal permit.
            {
                input.thread_store.remove::<TurnStartOptions>();
                tracing::warn!(
                    "failed to account active goal progress at turn stop for {turn_id}: {err}"
                );
                return;
            }
            let accounting = runtime.accounting_state();
            if accounting
                .current_active_goal_id_for_turn(turn_id)
                .is_some()
                && let Some(options) = input.thread_store.get::<TurnStartOptions>()
            {
                input.thread_store.insert_if(
                    TurnStartOptions {
                        parent_turn_id: Some(turn_id.to_string()),
                        ..options.as_ref().clone()
                    },
                    |current| current.is_some(),
                );
            }
            accounting.finish_turn(turn_id);
        })
    }

    fn on_turn_abort<'a>(&'a self, input: TurnAbortInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(runtime) = goal_runtime_handle(input.thread_store) else {
                return;
            };
            if !runtime.is_enabled() {
                runtime.accounting_state().reset_empty_responses();
                return;
            }

            let turn_id = input.turn_store.level_id();
            let _alab_finished = runtime.accounting_state().alab_finished_turn(turn_id); // ALAB(storage): aborted Core work cannot leave an orphan current-turn marker.
            input.thread_store.remove::<TurnStartOptions>();
            if input.reason != codex_protocol::protocol::TurnAbortReason::Interrupted {
                runtime.accounting_state().reset_empty_responses();
            }
            if let Err(err) = runtime
                .alab_account_ending_turn_progress(turn_id, &format!("{turn_id}:turn-abort"))
                .await
            // ALAB(storage): aborts must settle the same terminal late-token boundary as normal turn stops.
            {
                tracing::warn!(
                    "failed to account active goal progress after turn abort for {turn_id}: {err}"
                );
                return;
            }
            let accounting = runtime.accounting_state();
            match input.reason {
                codex_protocol::protocol::TurnAbortReason::Interrupted => {
                    accounting.interrupt_turn(turn_id);
                }
                codex_protocol::protocol::TurnAbortReason::Replaced
                | codex_protocol::protocol::TurnAbortReason::ReviewEnded
                | codex_protocol::protocol::TurnAbortReason::BudgetLimited => {
                    accounting.finish_turn(turn_id);
                }
            }
        })
    }

    fn on_turn_error<'a>(&'a self, input: TurnErrorInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(runtime) = goal_runtime_handle(input.thread_store) else {
                return;
            };

            if matches!(
                input.error,
                CodexErrorInfo::CyberPolicy
                    | CodexErrorInfo::BioPolicy
                    | CodexErrorInfo::MisalignmentPolicyViolation
            ) && codex_alab_core::active()
            {
                // ALAB(policy-recovery): a refused turn never blocks the goal's ordinary continuation.
                if let Err(err) = runtime
                    .account_active_goal_progress(
                        input.turn_id,
                        &format!("{}:policy-refusal-progress", input.turn_id),
                        codex_state::GoalAccountingMode::ActiveOnly,
                        BudgetLimitedGoalDisposition::ClearActive,
                    )
                    .await
                {
                    tracing::warn!(
                        "failed to account active goal progress after Daybreak refusal: {err}"
                    );
                    return; // ALAB(storage): preserve refused-turn usage until its failed write is accounted.
                }
                runtime.accounting_state().finish_turn(input.turn_id);
                return;
            }

            let reason = match input.error {
                ref error
                    if codex_alab_core::active() && GoalRuntimeHandle::alab_is_auth_wall(error) =>
                {
                    GoalRuntimeHandle::alab_auth_stop_reason(input.turn_store)
                } // ALAB(renewal): typed auth failure plus original turn association.
                CodexErrorInfo::UsageLimitExceeded if codex_alab_core::active() => {
                    GoalRuntimeHandle::alab_quota_stop_reason(input.turn_store)
                } // ALAB(accounts): only an owned subscription request retains quota recovery custody.
                CodexErrorInfo::UsageLimitExceeded => ActiveGoalStopReason::UsageLimit,
                // The turn has ended because the error was non-retryable or its
                // retries were exhausted. Block the goal to prevent automatic
                // continuation from looping and consuming tokens, as can happen
                // with compaction errors.
                _ => ActiveGoalStopReason::TurnError,
            };
            if let Err(err) = runtime
                .stop_active_goal_for_turn(input.turn_id, reason)
                .await
            {
                tracing::warn!(
                    error = ?input.error,
                    "failed to stop active goal after turn error: {err}"
                );
            }
        })
    }
}

impl<C> TokenUsageContributor for GoalExtension<C>
where
    C: Send + Sync + 'static,
{
    fn on_token_usage<'a>(
        &'a self,
        _session_store: &'a ExtensionData,
        thread_store: &'a ExtensionData,
        turn_store: &'a ExtensionData,
        token_usage: &'a TokenUsageInfo,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(runtime) = goal_runtime_handle(thread_store) else {
                return;
            };
            if !runtime.is_enabled() {
                return;
            }

            if let Some(root_accounting_state) = runtime.root_accounting_state() {
                let Ok(_callback) = root_accounting_state.callback_permit().await else {
                    return;
                }; // ALAB(storage): retirement's final drain excludes callbacks on this exact accounting owner.
                if root_accounting_state
                    .alab_persistence
                    .accepts_accounting_callbacks()
                {
                    root_accounting_state
                        .record_descendant_token_usage(&token_usage.last_token_usage);
                } // ALAB(storage): a callback queued behind retirement cannot mutate the carried-away owner.
            }
            let accounting = runtime.accounting_state();
            let Ok(_callback) = accounting.callback_permit().await else {
                return;
            }; // ALAB(storage): carry capture cannot be overtaken by a late turn-token callback.
            if runtime.alab_runtime_status_retired()
                || !accounting.alab_persistence.accepts_accounting_callbacks()
            {
                return;
            } // ALAB(storage): waiters released after retirement are discarded before touching native accounting.
            let _ = accounting
                .record_token_usage(turn_store.level_id(), &token_usage.total_token_usage);
        })
    }
}

impl<C> ToolLifecycleContributor for GoalExtension<C>
where
    C: Send + Sync + 'static,
{
    fn on_tool_finish<'a>(&'a self, input: ToolFinishInput<'a>) -> ToolLifecycleFuture<'a> {
        Box::pin(async move {
            let Some(runtime) = goal_runtime_handle(input.thread_store) else {
                return;
            };
            if !runtime.is_enabled() {
                return;
            }
            runtime.accounting_state().record_tool_outcome(
                input.turn_id,
                input.tool_name,
                input.outcome,
            );
            // ALAB(goal-recovery): ALab publishes successor turn options through its durable mutation path.
            if input.tool_name.is_default_namespace()
                && input.tool_name.name == CREATE_GOAL_TOOL_NAME
                && matches!(input.outcome, ToolCallOutcome::Completed { success: true })
                && !codex_alab_core::is_alab_binary()
            {
                input.thread_store.remove::<TurnStartOptions>();
                if let Ok(_goal_state_permit) = runtime.goal_state_permit().await
                    && let Some(thread_manager) = self.thread_manager.upgrade()
                    && let Ok(thread) = thread_manager.get_thread(runtime.thread_id()).await
                    && let Some(root_turn_id) = thread.active_turn_root(input.turn_id).await
                {
                    input.thread_store.insert(TurnStartOptions {
                        root_turn_id: Some(root_turn_id),
                        parent_turn_id: Some(input.turn_id.to_string()),
                        ..Default::default()
                    });
                }
            }
            let should_count_for_goal_progress =
                tool_attempt_counts_for_goal_progress(input.outcome)
                    && !(input.tool_name.is_default_namespace()
                        && input.tool_name.name == UPDATE_GOAL_TOOL_NAME);
            if !should_count_for_goal_progress {
                return;
            }
            let turn_id = input.turn_id;
            let progress = match runtime
                .account_active_goal_progress(
                    turn_id,
                    input.call_id,
                    codex_state::GoalAccountingMode::ActiveOnly,
                    BudgetLimitedGoalDisposition::KeepActive,
                )
                .await
            {
                Ok(Some(progress)) => progress,
                Ok(None) => return,
                Err(err) => {
                    tracing::warn!(
                        "failed to account active goal progress after tool finish for {turn_id}: {err}"
                    );
                    return;
                }
            };
            let goal = progress.goal;
            if goal.status != ThreadGoalStatus::BudgetLimited {
                return;
            }
            if !runtime
                .accounting_state()
                .mark_budget_limit_reported_if_new(progress.goal_id.as_str())
            {
                return;
            }
            let item = budget_limit_steering_item(&goal);
            runtime.inject_active_turn_steering(item).await;
        })
    }
}

impl<C> ToolContributor for GoalExtension<C>
where
    C: Send + Sync + 'static,
{
    fn tools(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
    ) -> Vec<
        Arc<dyn for<'call> codex_extension_api::ToolExecutor<codex_extension_api::ToolCall<'call>>>,
    > {
        let Some(runtime) = goal_runtime_handle(thread_store) else {
            return Vec::new();
        };
        if !runtime.tools_visible() {
            return Vec::new();
        }
        let max_goal_token_budget = thread_store
            .get::<GoalExtensionConfig>()
            .and_then(|config| config.max_goal_token_budget);

        let tools = [
            GoalToolExecutor::get(
                runtime.thread_id(),
                Arc::clone(&self.state_dbs),
                runtime.accounting_state(),
                self.analytics.clone(),
                self.event_emitter.clone(),
                self.metrics.clone(),
            ),
            GoalToolExecutor::create(
                &runtime,
                self.thread_manager.clone(),
                Arc::clone(&self.state_dbs),
                self.analytics.clone(),
                self.event_emitter.clone(),
                self.metrics.clone(),
                max_goal_token_budget,
            ),
            GoalToolExecutor::update(
                runtime.thread_id(),
                Arc::clone(&self.state_dbs),
                runtime.accounting_state(),
                self.analytics.clone(),
                self.event_emitter.clone(),
                self.metrics.clone(),
            ),
        ];
        tools
            .into_iter()
            .map(|mut tool| {
                tool.alab_runtime = Some(runtime.as_ref().clone()); // ALAB(storage): actual goal tools and native mutations serialize on the same permit.
                tool.execution_allowed = runtime.tools_available();
                Arc::new(tool) as Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>>
            })
            .collect()
    }
}

pub fn install_with_backend<C>(
    registry: &mut ExtensionRegistryBuilder<C>,
    state_dbs: Arc<codex_state::StateRuntime>,
    analytics_events_client: AnalyticsEventsClient,
    metrics_client: Option<MetricsClient>,
    thread_manager: Weak<ThreadManager>,
    goal_service: Arc<GoalService>,
    goal_config: impl Fn(&C) -> GoalExtensionConfig + Send + Sync + 'static,
) where
    C: Send + Sync + 'static,
{
    let extension = Arc::new(GoalExtension::new_with_host_capabilities(
        state_dbs,
        analytics_events_client,
        registry.event_sink(),
        metrics_client,
        thread_manager,
        Arc::clone(&goal_service),
        goal_config,
    ));
    registry.thread_lifecycle_contributor(extension.clone());
    registry.config_contributor(extension.clone());
    registry.turn_lifecycle_contributor(extension.clone());
    registry.token_usage_contributor(extension.clone());
    registry.tool_lifecycle_contributor(extension.clone());
    registry.tool_contributor(extension);
}

fn goal_runtime_handle(thread_store: &ExtensionData) -> Option<Arc<GoalRuntimeHandle>> {
    thread_store.get::<GoalRuntimeHandle>()
}

fn tool_attempt_counts_for_goal_progress(outcome: ToolCallOutcome) -> bool {
    match outcome {
        ToolCallOutcome::Completed { .. } => true,
        ToolCallOutcome::Failed {
            handler_executed: true,
        } => true,
        ToolCallOutcome::Blocked
        | ToolCallOutcome::Failed {
            handler_executed: false,
        }
        | ToolCallOutcome::Aborted => false,
    }
}
