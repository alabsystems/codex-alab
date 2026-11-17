// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Keep goal RPC ownership after dispatch and persist unloaded-thread outcomes.

use super::*;
use codex_core::context::ContextualUserFragment;
use codex_core::context::UserGoalUpdate;
use codex_goal_extension::GoalClearOutcome;
use codex_rollout::RolloutRecorderParams;
use std::future::Future;
use std::pin::Pin;
use std::sync::Weak;

type GoalOutput = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

pub(crate) struct GoalProcessorThreadState {
    thread_state_manager: ThreadStateManager,
    pending_thread_unloads: PendingThreadUnloads,
}

impl GoalProcessorThreadState {
    pub(crate) fn new(
        thread_state_manager: ThreadStateManager,
        pending_thread_unloads: PendingThreadUnloads,
    ) -> Self {
        Self {
            thread_state_manager,
            pending_thread_unloads,
        }
    }

    pub(super) fn into_parts(self) -> (ThreadStateManager, PendingThreadUnloads) {
        (self.thread_state_manager, self.pending_thread_unloads)
    }
}

pub(super) struct GoalOutputQueue {
    tails: std::sync::Mutex<std::collections::HashMap<ThreadId, GoalOutputTail>>,
    thread_limits:
        std::sync::Mutex<std::collections::HashMap<ThreadId, Weak<tokio::sync::Semaphore>>>,
    global_limit: Arc<tokio::sync::Semaphore>,
}

const THREAD_OUTPUT_CAPACITY: usize = 8;
const GLOBAL_OUTPUT_CAPACITY: usize = 128;

fn note_person_goal_set(
    thread_id: ThreadId,
    resulting_status: codex_protocol::protocol::ThreadGoalStatus,
) {
    codex_alab_core::goal_stops::person_set(
        &thread_id.to_string(),
        resulting_status != codex_protocol::protocol::ThreadGoalStatus::Active,
    );
}

impl Default for GoalOutputQueue {
    fn default() -> Self {
        Self {
            tails: std::sync::Mutex::default(),
            thread_limits: std::sync::Mutex::default(),
            global_limit: Arc::new(tokio::sync::Semaphore::new(GLOBAL_OUTPUT_CAPACITY)),
        }
    }
}

pub(super) struct GoalOutputReservation {
    _global: tokio::sync::OwnedSemaphorePermit,
    _thread: tokio::sync::OwnedSemaphorePermit,
}

pub(super) struct GoalMutationCustody {
    rollout: Option<UnloadedGoalRollout>,
    output: GoalOutputReservation,
}

pub(super) struct GoalOutputRelease(Option<tokio::sync::oneshot::Sender<()>>);

impl GoalOutputRelease {
    pub(super) fn release(mut self) {
        if let Some(release) = self.0.take() {
            let _ = release.send(());
        }
    }
}

struct GoalOutputTail {
    identity: Arc<()>,
    completed: tokio::sync::oneshot::Receiver<()>,
}

struct GoalOutputCompletion {
    queue: Arc<GoalOutputQueue>,
    thread_id: ThreadId,
    identity: Arc<()>,
    completed: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Drop for GoalOutputCompletion {
    fn drop(&mut self) {
        if let Some(completed) = self.completed.take() {
            let _ = completed.send(());
        }
        let mut tails = self
            .queue
            .tails
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if tails
            .get(&self.thread_id)
            .is_some_and(|tail| Arc::ptr_eq(&tail.identity, &self.identity))
        {
            tails.remove(&self.thread_id);
        }
    }
}

impl GoalOutputQueue {
    fn reserve(&self, thread_id: ThreadId) -> Option<GoalOutputReservation> {
        let global = Arc::clone(&self.global_limit).try_acquire_owned().ok()?;
        let thread_limit = {
            let mut limits = self
                .thread_limits
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            limits.retain(|_, limit| limit.strong_count() > 0);
            if let Some(limit) = limits.get(&thread_id).and_then(Weak::upgrade) {
                limit
            } else {
                let limit = Arc::new(tokio::sync::Semaphore::new(THREAD_OUTPUT_CAPACITY));
                limits.insert(thread_id, Arc::downgrade(&limit));
                limit
            }
        };
        let thread = thread_limit.try_acquire_owned().ok()?;
        Some(GoalOutputReservation {
            _global: global,
            _thread: thread,
        })
    }

    fn enqueue(
        self: &Arc<Self>,
        thread_id: ThreadId,
        reservation: GoalOutputReservation,
        output: GoalOutput,
    ) {
        let identity = Arc::new(());
        let (completed, receiver) = tokio::sync::oneshot::channel();
        let previous = self
            .tails
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                thread_id,
                GoalOutputTail {
                    identity: Arc::clone(&identity),
                    completed: receiver,
                },
            )
            .map(|tail| tail.completed);
        let queue = Arc::clone(self);
        tokio::spawn(async move {
            let _reservation = reservation;
            let _completion = GoalOutputCompletion {
                queue,
                thread_id,
                identity,
                completed: Some(completed),
            };
            if let Some(previous) = previous {
                let _ = previous.await;
            }
            output.await;
        });
    }
}

pub(super) struct UnloadedGoalRollout {
    recorder: RolloutRecorder,
    plain_rollout_path: std::path::PathBuf,
}

impl UnloadedGoalRollout {
    pub(super) async fn record_user_update(
        &self,
        update: UserGoalUpdate,
    ) -> Result<(), GoalServiceError> {
        let item = ContextualUserFragment::into(update);
        self.recorder
            .record_canonical_items(&[RolloutItem::ResponseItem(item.into())])
            .await
            .map_err(|error| {
                GoalServiceError::Internal(format!(
                    "failed to record unloaded goal instruction: {error}"
                ))
            })?;
        self.recorder.flush().await.map_err(|error| {
            GoalServiceError::Internal(format!(
                "failed to flush unloaded goal instruction: {error}"
            ))
        })
    }

    pub(super) async fn record_goal_update(
        &self,
        item: RolloutItem,
    ) -> Result<(), codex_thread_store::ThreadStoreError> {
        let record = self
            .recorder
            .record_canonical_items(&[item])
            .await
            .map_err(
                |error| codex_thread_store::ThreadStoreError::OutcomeUnknown {
                    message: error.to_string(),
                },
            );
        let shutdown = self.shutdown().await.map_err(|error| {
            codex_thread_store::ThreadStoreError::OutcomeUnknown {
                message: error.to_string(),
            }
        });
        record.and(shutdown)
    }

    pub(super) async fn shutdown(&self) -> std::io::Result<()> {
        self.recorder.shutdown().await
    }
}

impl ThreadGoalRequestProcessor {
    pub(super) async fn alab_flush_goal_chronology(
        &self,
        rollout: Option<&UnloadedGoalRollout>,
        thread_id: ThreadId,
    ) -> Result<(), JSONRPCErrorError> {
        if !codex_alab_core::is_alab_binary() {
            return Ok(());
        }
        if let Some(rollout) = rollout {
            rollout.recorder.flush().await.map_err(|error| {
                internal_error(format!(
                    "failed to flush unloaded goal chronology for {thread_id}: {error}"
                ))
            })?;
            if tokio::fs::try_exists(&rollout.plain_rollout_path)
                .await
                .map_err(|error| internal_error(error.to_string()))?
            {
                self.alab_sync_goal_chronology_file(&rollout.plain_rollout_path)
                    .await
                    .map_err(|error| {
                        internal_error(format!(
                            "failed to sync unloaded goal chronology for {thread_id}: {error}"
                        ))
                    })?;
            }
            return Ok(());
        }
        let thread = self
            .thread_manager
            .get_thread(thread_id)
            .await
            .map_err(|error| {
                internal_error(format!(
                    "lost live goal chronology owner for {thread_id}: {error}"
                ))
            })?;
        let plain_path = thread
            .rollout_path()
            .map(|path| codex_rollout::plain_rollout_path(path.as_path()));
        thread.flush_rollout().await.map_err(|error| {
            internal_error(format!(
                "failed to flush live goal chronology for {thread_id}: {error}"
            ))
        })?;
        if let Some(path) = plain_path
            && tokio::fs::try_exists(&path)
                .await
                .map_err(|error| internal_error(error.to_string()))?
        {
            self.alab_sync_goal_chronology_file(&path)
                .await
                .map_err(|error| {
                    internal_error(format!(
                        "failed to sync live goal chronology for {thread_id}: {error}"
                    ))
                })?;
        }
        Ok(())
    }

    pub(super) async fn alab_goal_chronology_fence_path(
        &self,
        rollout: Option<&UnloadedGoalRollout>,
        thread_id: ThreadId,
    ) -> Result<Option<std::path::PathBuf>, JSONRPCErrorError> {
        if !codex_alab_core::is_alab_binary() {
            return Ok(None);
        }
        let plain_path = if let Some(rollout) = rollout {
            rollout.plain_rollout_path.clone()
        } else {
            let thread = self
                .thread_manager
                .get_thread(thread_id)
                .await
                .map_err(|error| {
                    internal_error(format!(
                        "lost live goal chronology owner for {thread_id}: {error}"
                    ))
                })?;
            let path = thread.rollout_path().ok_or_else(|| {
                invalid_request(format!(
                    "ephemeral thread does not support goals: {thread_id}"
                ))
            })?;
            codex_rollout::plain_rollout_path(path.as_path())
        };
        Ok(Some(plain_path))
    }

    pub(super) async fn alab_settle_goal_chronology_fence(
        &self,
        thread_id: ThreadId,
        plain_path: Option<&std::path::PathBuf>,
    ) -> Result<(), codex_thread_store::ThreadStoreError> {
        let Some(plain_path) = plain_path else {
            return Ok(());
        };
        self.alab_sync_goal_chronology_file(plain_path)
            .await
            .map_err(
                |error| codex_thread_store::ThreadStoreError::OutcomeUnknown {
                    message: format!("failed to sync goal chronology for {thread_id}: {error}"),
                },
            )?;
        codex_alab_core::goal_chronology_fence::settle_dispatch_fence().map_err(|error| {
            codex_thread_store::ThreadStoreError::OutcomeUnknown {
                message: format!("failed to clear goal chronology fence for {thread_id}: {error}"),
            }
        })
    }

    pub(super) async fn alab_sync_goal_chronology_file(
        &self,
        plain_path: &std::path::Path,
    ) -> std::io::Result<()> {
        tokio::fs::File::open(plain_path).await?.sync_all().await?;
        codex_alab_core::goal_chronology_fence::sync_parent_chain(
            &self.config.codex_home,
            plain_path,
        )
    }

    pub(super) fn alab_settle_cleared_goal_chronology_fence(
        &self,
        thread_id: ThreadId,
        plain_path: Option<&std::path::PathBuf>,
    ) -> Result<(), JSONRPCErrorError> {
        if plain_path.is_none() {
            return Ok(());
        }
        codex_alab_core::goal_chronology_fence::settle_dispatch_fence().map_err(|error| {
            internal_error(format!(
                "failed to clear settled goal chronology fence for {thread_id}: {error}"
            ))
        })
    }

    pub(super) async fn alab_record_goal_instruction(
        &self,
        rollout: Option<&UnloadedGoalRollout>,
        thread_id: ThreadId,
        update: UserGoalUpdate,
    ) -> Result<(), GoalServiceError> {
        if let Some(rollout) = rollout {
            rollout.record_user_update(update).await
        } else {
            self.record_user_goal_update(thread_id, update).await
        }
    }

    pub(super) async fn alab_finish_goal_outcome(
        &self,
        thread_id: ThreadId,
        rollout: Option<&UnloadedGoalRollout>,
        outcome: Result<codex_goal_extension::GoalSetOutcome, GoalServiceError>,
    ) -> Result<codex_goal_extension::GoalSetOutcome, JSONRPCErrorError> {
        match outcome {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                if let Some(rollout) = rollout
                    && let Err(shutdown) = rollout.shutdown().await
                {
                    warn!(
                        "failed to close unloaded goal chronology after rejected mutation for {thread_id}: {shutdown}"
                    );
                }
                Err(goal_service_error(error))
            }
        }
    }

    pub(super) async fn alab_finish_goal_clear(
        &self,
        thread_id: ThreadId,
        rollout: Option<&UnloadedGoalRollout>,
        result: Result<GoalClearOutcome, GoalServiceError>,
    ) -> Result<GoalClearOutcome, JSONRPCErrorError> {
        let shutdown = if let Some(rollout) = rollout {
            rollout.shutdown().await
        } else {
            Ok(())
        };
        match (result, shutdown) {
            (Ok(outcome), Ok(())) => Ok(outcome),
            (Ok(outcome), Err(error)) => {
                warn!(
                    "failed to close unloaded goal chronology after committed clear for {thread_id}: {error}"
                );
                Ok(outcome)
            }
            (Err(error), Ok(())) => Err(goal_service_error(error)),
            (Err(error), Err(shutdown)) => {
                warn!(
                    "failed to close unloaded goal chronology after rejected clear for {thread_id}: {shutdown}"
                );
                Err(goal_service_error(error))
            }
        }
    }

    pub(super) fn alab_reserve_goal_output(
        &self,
        thread_id: ThreadId,
    ) -> Result<GoalOutputReservation, JSONRPCErrorError> {
        if !codex_alab_core::is_alab_binary() {
            return Err(internal_error(
                "ALab goal output custody was requested outside an ALab binary",
            ));
        }
        self.alab_goal_outputs.reserve(thread_id).ok_or_else(|| {
            internal_error(format!(
                "goal output backlog is full for {thread_id}; retry after the client drains"
            ))
        })
    }

    pub(super) fn alab_enqueue_goal_set_output(
        &self,
        thread_id: ThreadId,
        request_id: ConnectionRequestId,
        goal: ThreadGoal,
        listener_command_tx: Option<tokio::sync::mpsc::UnboundedSender<ThreadListenerCommand>>,
        reservation: GoalOutputReservation,
    ) -> GoalOutputRelease {
        let outgoing = Arc::clone(&self.outgoing);
        let (release, wait_for_release) = tokio::sync::oneshot::channel();
        let listener_ack = listener_command_tx.and_then(|listener_command_tx| {
            let (response_enqueued, wait_for_response) = tokio::sync::oneshot::channel();
            let (notification_enqueued, wait_for_notification) = tokio::sync::oneshot::channel();
            let listener_outgoing = Arc::clone(&self.outgoing);
            let listener_goal = goal.clone();
            listener_command_tx
                .send(ThreadListenerCommand::RunExtension(Box::pin(async move {
                    if wait_for_response.await.is_err() {
                        return;
                    }
                    listener_outgoing
                        .send_server_notification(ServerNotification::ThreadGoalUpdated(
                            ThreadGoalUpdatedNotification {
                                thread_id: thread_id.to_string(),
                                turn_id: None,
                                goal: listener_goal,
                            },
                        ))
                        .await;
                    let _ = notification_enqueued.send(());
                })))
                .ok()
                .map(|()| (response_enqueued, wait_for_notification))
        });
        self.alab_goal_outputs.enqueue(
            thread_id,
            reservation,
            Box::pin(async move {
                if wait_for_release.await.is_err() {
                    return;
                }
                outgoing
                    .send_response(
                        request_id,
                        ThreadGoalSetResponse { goal: goal.clone() },
                    )
                    .await;
                if let Some((response_enqueued, notification_enqueued)) = listener_ack {
                    let _ = response_enqueued.send(());
                    if notification_enqueued.await.is_ok() {
                        return;
                    }
                    warn!(
                        "ordered thread goal update was dropped for {thread_id}; sending direct fallback"
                    );
                }
                outgoing
                    .send_server_notification(ServerNotification::ThreadGoalUpdated(
                        ThreadGoalUpdatedNotification {
                            thread_id: thread_id.to_string(),
                            turn_id: None,
                            goal,
                        },
                    ))
                    .await;
            }),
        );
        GoalOutputRelease(Some(release))
    }

    pub(super) fn alab_enqueue_goal_clear_output(
        &self,
        thread_id: ThreadId,
        request_id: ConnectionRequestId,
        cleared: bool,
        listener_command_tx: Option<tokio::sync::mpsc::UnboundedSender<ThreadListenerCommand>>,
        reservation: GoalOutputReservation,
    ) -> GoalOutputRelease {
        let outgoing = Arc::clone(&self.outgoing);
        let (release, wait_for_release) = tokio::sync::oneshot::channel();
        let listener_ack =
            cleared
                .then_some(listener_command_tx)
                .flatten()
                .and_then(|listener_command_tx| {
                    let (response_enqueued, wait_for_response) = tokio::sync::oneshot::channel();
                    let (notification_enqueued, wait_for_notification) =
                        tokio::sync::oneshot::channel();
                    let listener_outgoing = Arc::clone(&self.outgoing);
                    listener_command_tx
                        .send(ThreadListenerCommand::RunExtension(Box::pin(async move {
                            if wait_for_response.await.is_err() {
                                return;
                            }
                            listener_outgoing
                                .send_server_notification(ServerNotification::ThreadGoalCleared(
                                    ThreadGoalClearedNotification {
                                        thread_id: thread_id.to_string(),
                                    },
                                ))
                                .await;
                            let _ = notification_enqueued.send(());
                        })))
                        .ok()
                        .map(|()| (response_enqueued, wait_for_notification))
                });
        self.alab_goal_outputs.enqueue(
            thread_id,
            reservation,
            Box::pin(async move {
                if wait_for_release.await.is_err() {
                    return;
                }
                outgoing
                    .send_response(request_id, ThreadGoalClearResponse { cleared })
                    .await;
                if !cleared {
                    return;
                }
                if let Some((response_enqueued, notification_enqueued)) = listener_ack {
                    let _ = response_enqueued.send(());
                    if notification_enqueued.await.is_ok() {
                        return;
                    }
                    warn!(
                        "ordered thread goal clear was dropped for {thread_id}; sending direct fallback"
                    );
                }
                outgoing
                    .send_server_notification(ServerNotification::ThreadGoalCleared(
                        ThreadGoalClearedNotification {
                            thread_id: thread_id.to_string(),
                        },
                    ))
                    .await;
            }),
        );
        GoalOutputRelease(Some(release))
    }

    pub(super) async fn alab_complete_goal_set_request(
        &self,
        thread_id: ThreadId,
        request_id: ConnectionRequestId,
        goal: ThreadGoal,
        listener_command_tx: Option<tokio::sync::mpsc::UnboundedSender<ThreadListenerCommand>>,
        outcome: codex_goal_extension::GoalSetOutcome,
        custody: GoalMutationCustody,
    ) {
        let GoalMutationCustody { rollout, output } = custody;
        drop(rollout);
        let output = self.alab_enqueue_goal_set_output(
            thread_id,
            request_id,
            goal,
            listener_command_tx,
            output,
        );
        outcome.apply_runtime_effects(&self.goal_service).await;
        output.release();
    }

    pub(super) fn alab_complete_goal_error(
        &self,
        thread_id: ThreadId,
        request_id: ConnectionRequestId,
        error: JSONRPCErrorError,
        custody: GoalMutationCustody,
    ) {
        let GoalMutationCustody { rollout, output } = custody;
        drop(rollout);
        let outgoing = Arc::clone(&self.outgoing);
        self.alab_goal_outputs.enqueue(
            thread_id,
            output,
            Box::pin(async move {
                outgoing.send_error(request_id, error).await;
            }),
        );
    }

    pub(super) async fn alab_complete_goal_clear_request(
        &self,
        thread_id: ThreadId,
        request_id: ConnectionRequestId,
        listener_command_tx: Option<tokio::sync::mpsc::UnboundedSender<ThreadListenerCommand>>,
        outcome: GoalClearOutcome,
        custody: GoalMutationCustody,
    ) {
        let GoalMutationCustody { rollout, output } = custody;
        drop(rollout);
        let cleared = outcome.cleared();
        let output = self.alab_enqueue_goal_clear_output(
            thread_id,
            request_id,
            cleared,
            listener_command_tx,
            output,
        );
        outcome.apply_runtime_effects(&self.goal_service).await;
        output.release();
    }

    async fn alab_thread_goal_set_inner(
        &self,
        request_id: ConnectionRequestId,
        params: ThreadGoalSetParams,
    ) -> Result<(), JSONRPCErrorError> {
        if !self.config.features.enabled(Feature::Goals) {
            return Err(invalid_request("goals feature is disabled"));
        }

        let thread_id = parse_thread_id_for_request(params.thread_id.as_str())?;
        if params.objective.is_none() && params.status.is_none() && params.token_budget.is_none() {
            return Err(invalid_request(
                "goal update must change objective, status, or token budget",
            ));
        }
        let output_reservation = self.alab_reserve_goal_output(thread_id)?;
        let _goal_resume_guard = self.thread_state_manager.lock_goal_resume(thread_id).await;
        let state_db = self
            .state_db_for_materialized_thread(thread_id, GoalAccess::Mutate)
            .await?;
        let unloaded_rollout = self.alab_reserve_unloaded_goal_rollout(thread_id).await?;
        self.reconcile_thread_goal_rollout(thread_id, &state_db)
            .await?;

        let listener_command_tx = {
            let thread_state = self.thread_state_manager.thread_state(thread_id).await;
            let thread_state = thread_state.lock().await;
            thread_state.listener_command_tx()
        };
        let status = params.status.map(ThreadGoalStatus::to_core);
        let objective = params.objective.as_deref().map(str::trim);
        let chronology_fence = self
            .alab_goal_chronology_fence_path(unloaded_rollout.as_ref(), thread_id)
            .await?;

        let host_operation = async {
            let (thread_config, max_goal_token_budget) =
                match self.thread_manager.get_thread(thread_id).await {
                    Ok(thread) => {
                        let config = thread.config().await;
                        let max_goal_token_budget = config.max_goal_token_budget;
                        (Some(config), max_goal_token_budget)
                    }
                    Err(_) => (None, self.config.max_goal_token_budget),
                };
            self.alab_flush_goal_chronology(unloaded_rollout.as_ref(), thread_id)
                .await?;
            let provider_error = Arc::new(std::sync::Mutex::new(None));
            let callback_provider_error = Arc::clone(&provider_error);
            let outcome = self
                .goal_service
                .set_thread_goal(
                    &state_db,
                    GoalSetRequest {
                        thread_id,
                        objective: objective
                            .map(GoalObjectiveUpdate::Set)
                            .unwrap_or(GoalObjectiveUpdate::Keep),
                        status,
                        token_budget: match params.token_budget {
                            Some(token_budget) => GoalTokenBudgetUpdate::Set(token_budget),
                            None => GoalTokenBudgetUpdate::Keep,
                        },
                        max_goal_token_budget,
                    },
                    Box::pin(async {
                        // GoalService has now drained progress and completed its
                        // authoritative read under the native permit. Re-read
                        // that row before classifying an objective-only update.
                        let current = self
                            .goal_service
                            .get_thread_goal(&state_db, thread_id)
                            .await?;
                        let resulting_status = status
                            .or_else(|| current.as_ref().map(|goal| goal.status))
                            .unwrap_or(codex_protocol::protocol::ThreadGoalStatus::Active);
                        if resulting_status
                            == codex_protocol::protocol::ThreadGoalStatus::Active
                            && let Some(config) = thread_config.as_ref()
                            && let Err(error) = self
                                .config_manager
                                .check_thread_model_provider(config.as_ref())
                                .await
                        {
                            let error = config_load_error(&error);
                            let message = error.message.clone();
                            *callback_provider_error
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error);
                            return Err(GoalServiceError::InvalidRequest(message));
                        }
                        if params.origin == Some(ThreadGoalMutationOrigin::User)
                            && (objective.is_some() || status.is_some())
                        {
                            self.alab_record_goal_instruction(
                                unloaded_rollout.as_ref(),
                                thread_id,
                                UserGoalUpdate::Set {
                                    objective: objective.map(str::to_owned),
                                    status,
                                },
                            )
                            .await?;
                            if let Some(path) = chronology_fence.as_deref() {
                                self.alab_sync_goal_chronology_file(path)
                                    .await
                                    .map_err(|error| {
                                        GoalServiceError::Internal(format!(
                                            "failed to sync goal instruction for {thread_id}: {error}"
                                        ))
                                    })?;
                            }
                            note_person_goal_set(thread_id, resulting_status);
                        }
                        Ok(())
                    }),
                )
                .await;
            let outcome = self
                .alab_finish_goal_outcome(thread_id, unloaded_rollout.as_ref(), outcome)
                .await;
            let exact_provider_error = provider_error
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            let outcome = match (outcome, exact_provider_error) {
                (Ok(outcome), None) => outcome,
                (Ok(outcome), Some(error)) => {
                    drop(outcome);
                    self.alab_complete_goal_error(
                        thread_id,
                        request_id,
                        error,
                        GoalMutationCustody {
                            rollout: unloaded_rollout,
                            output: output_reservation,
                        },
                    );
                    return Ok(());
                }
                (Err(error), exact_provider_error) => {
                    let error = exact_provider_error.unwrap_or(error);
                    self.alab_complete_goal_error(
                        thread_id,
                        request_id,
                        error,
                        GoalMutationCustody {
                            rollout: unloaded_rollout,
                            output: output_reservation,
                        },
                    );
                    return Ok(());
                }
            };
            let goal = ThreadGoal::from(outcome.goal.clone());
            let unloaded_persist_result = if let Some(rollout) = &unloaded_rollout {
                Some(
                    rollout
                        .record_goal_update(outcome.thread_goal_updated_item())
                        .await,
                )
            } else {
                None
            };
            let mut persist_result = match unloaded_persist_result {
                Some(result) => result,
                None => match self.thread_manager.get_thread(thread_id).await {
                    Ok(thread) => {
                        let needs_settings_checkpoint = match thread.rollout_path() {
                            Some(path) => {
                                codex_rollout::existing_rollout_path(&path).await.is_none()
                            }
                            None => false,
                        };
                        match thread
                            .append_rollout_items(&[outcome.thread_goal_updated_item()])
                            .await
                        {
                            Ok(()) if needs_settings_checkpoint => {
                                thread.checkpoint_thread_settings().await
                            }
                            result => result,
                        }
                    }
                    Err(error) => Err(codex_thread_store::ThreadStoreError::OutcomeUnknown {
                        message: format!(
                            "lost live goal chronology owner for {thread_id}: {error}"
                        ),
                    }),
                },
            };
            if persist_result.is_ok()
                && let Err(error) = state_db.thread_goals().alab_durable_goal_barrier().await
            {
                persist_result = Err(codex_thread_store::ThreadStoreError::OutcomeUnknown {
                    message: format!("failed to make goal update durable for {thread_id}: {error}"),
                });
            }
            if persist_result.is_ok()
                && let Err(error) = self
                    .alab_settle_goal_chronology_fence(thread_id, chronology_fence.as_ref())
                    .await
            {
                persist_result = Err(error);
            }
            if let Err(error) = persist_result {
                warn!("failed to persist goal update for live thread {thread_id}: {error}");
                drop(outcome);
                let error = internal_error(format!(
                    "goal update committed but its durable chronology is unknown for {thread_id}: {error}"
                ));
                self.alab_complete_goal_error(
                    thread_id,
                    request_id,
                    error,
                    GoalMutationCustody {
                        rollout: unloaded_rollout,
                        output: output_reservation,
                    },
                );
                return Ok(());
            }

            self.alab_complete_goal_set_request(
                thread_id,
                request_id,
                goal,
                listener_command_tx,
                outcome,
                GoalMutationCustody {
                    rollout: unloaded_rollout,
                    output: output_reservation,
                },
            )
            .await;
            Ok(())
        };
        let scoped = async {
            codex_alab_core::goal_chronology_fence::scope(
                self.config.codex_home.to_path_buf(),
                thread_id.to_string(),
                host_operation,
            )
            .await
            .map_err(|error| {
                internal_error(format!(
                    "failed to acquire cross-process goal chronology custody for {thread_id}: {error}"
                ))
            })?
        };
        match self
            .goal_service
            .alab_scope_host_goal_mutation(thread_id, scoped)
            .await
        {
            Ok(result) => result,
            Err(error) => Err(internal_error(format!(
                "failed to acquire host goal chronology custody for {thread_id}: {error}"
            ))),
        }
    }

    async fn alab_thread_goal_clear_inner(
        &self,
        request_id: ConnectionRequestId,
        params: ThreadGoalClearParams,
    ) -> Result<(), JSONRPCErrorError> {
        if !self.config.features.enabled(Feature::Goals) {
            return Err(invalid_request("goals feature is disabled"));
        }

        let thread_id = parse_thread_id_for_request(params.thread_id.as_str())?;
        let output_reservation = self.alab_reserve_goal_output(thread_id)?;
        let _goal_resume_guard = self.thread_state_manager.lock_goal_resume(thread_id).await;
        let state_db = self
            .state_db_for_materialized_thread(thread_id, GoalAccess::Mutate)
            .await?;
        let unloaded_rollout = self.alab_reserve_unloaded_goal_rollout(thread_id).await?;
        self.reconcile_thread_goal_rollout(thread_id, &state_db)
            .await?;
        let listener_command_tx = {
            let thread_state = self.thread_state_manager.thread_state(thread_id).await;
            let thread_state = thread_state.lock().await;
            thread_state.listener_command_tx()
        };
        let chronology_fence = self
            .alab_goal_chronology_fence_path(unloaded_rollout.as_ref(), thread_id)
            .await?;
        let host_operation = async {
            self.alab_flush_goal_chronology(unloaded_rollout.as_ref(), thread_id)
                .await?;
            let outcome = self
                .goal_service
                .clear_thread_goal(
                    &state_db,
                    thread_id,
                    Box::pin(async {
                        if params.origin == Some(ThreadGoalMutationOrigin::User) {
                            self.alab_record_goal_instruction(
                                unloaded_rollout.as_ref(),
                                thread_id,
                                UserGoalUpdate::Clear,
                            )
                            .await?;
                            if let Some(path) = chronology_fence.as_deref() {
                                self.alab_sync_goal_chronology_file(path)
                                    .await
                                    .map_err(|error| {
                                        GoalServiceError::Internal(format!(
                                            "failed to sync goal clear instruction for {thread_id}: {error}"
                                        ))
                                    })?;
                            }
                            codex_alab_core::goal_stops::person_set(
                                &thread_id.to_string(),
                                true,
                            );
                        }
                        Ok(())
                    }),
                )
                .await;
            let outcome = match self
                .alab_finish_goal_clear(thread_id, unloaded_rollout.as_ref(), outcome)
                .await
            {
                Ok(outcome) => outcome,
                Err(error) => {
                    self.alab_complete_goal_error(
                        thread_id,
                        request_id,
                        error,
                        GoalMutationCustody {
                            rollout: unloaded_rollout,
                            output: output_reservation,
                        },
                    );
                    return Ok(());
                }
            };
            if let Err(error) = state_db.thread_goals().alab_durable_goal_barrier().await {
                drop(outcome);
                self.alab_complete_goal_error(
                    thread_id,
                    request_id,
                    internal_error(format!(
                        "failed to make goal clear durable for {thread_id}: {error}"
                    )),
                    GoalMutationCustody {
                        rollout: unloaded_rollout,
                        output: output_reservation,
                    },
                );
                return Ok(());
            }
            if let Err(error) =
                self.alab_settle_cleared_goal_chronology_fence(thread_id, chronology_fence.as_ref())
            {
                drop(outcome);
                self.alab_complete_goal_error(
                    thread_id,
                    request_id,
                    error,
                    GoalMutationCustody {
                        rollout: unloaded_rollout,
                        output: output_reservation,
                    },
                );
                return Ok(());
            }
            self.alab_complete_goal_clear_request(
                thread_id,
                request_id,
                listener_command_tx,
                outcome,
                GoalMutationCustody {
                    rollout: unloaded_rollout,
                    output: output_reservation,
                },
            )
            .await;
            Ok(())
        };
        let scoped = async {
            codex_alab_core::goal_chronology_fence::scope(
                self.config.codex_home.to_path_buf(),
                thread_id.to_string(),
                host_operation,
            )
            .await
            .map_err(|error| {
                internal_error(format!(
                    "failed to acquire cross-process goal chronology custody for {thread_id}: {error}"
                ))
            })?
        };
        match self
            .goal_service
            .alab_scope_host_goal_mutation(thread_id, scoped)
            .await
        {
            Ok(result) => result,
            Err(error) => Err(internal_error(format!(
                "failed to acquire host goal chronology custody for {thread_id}: {error}"
            ))),
        }
    }

    /// Once accepted by the app server, an ALab goal request owns its SQL,
    /// chronology and runtime effects even if the observing RPC is cancelled.
    pub(super) async fn alab_owned_thread_goal_set(
        &self,
        request_id: ConnectionRequestId,
        params: ThreadGoalSetParams,
    ) -> Result<(), JSONRPCErrorError> {
        let owner = self.clone();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let result = owner.alab_thread_goal_set_inner(request_id, params).await;
            let _ = sender.send(result);
        });
        receiver.await.map_err(|error| {
            internal_error(format!(
                "goal set owner ended before acknowledgement: {error}"
            ))
        })?
    }

    pub(super) async fn alab_owned_thread_goal_clear(
        &self,
        request_id: ConnectionRequestId,
        params: ThreadGoalClearParams,
    ) -> Result<(), JSONRPCErrorError> {
        let owner = self.clone();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let result = owner.alab_thread_goal_clear_inner(request_id, params).await;
            let _ = sender.send(result);
        });
        receiver.await.map_err(|error| {
            internal_error(format!(
                "goal clear owner ended before acknowledgement: {error}"
            ))
        })?
    }

    /// Acquire cross-process rollout ownership before SQLite can change. The
    /// caller already holds the process-local goal/resume lock, so this owner
    /// spans instruction, SQL, goal event and shutdown in one order.
    pub(super) async fn alab_reserve_unloaded_goal_rollout(
        &self,
        thread_id: ThreadId,
    ) -> Result<Option<UnloadedGoalRollout>, JSONRPCErrorError> {
        if !codex_alab_core::is_alab_binary() {
            return Ok(None);
        }
        let terminal_unload_owns_thread = self
            .pending_thread_unloads
            .lock()
            .await
            .contains_key(&thread_id);
        if !terminal_unload_owns_thread && self.thread_manager.get_thread(thread_id).await.is_ok() {
            return Ok(None);
        } // A shutting-down runtime can remain registered after its ThreadState/recorder owner is gone; pending unload therefore takes the unloaded writer before SQL.
        let writer_lock = Arc::new(self.writer_locks.acquire(thread_id).map_err(|error| {
            internal_error(format!(
                "cannot own unloaded goal chronology for {thread_id}: {error}"
            ))
        })?);
        let path = codex_rollout::find_thread_path_by_id_str(
            &self.config.codex_home,
            &thread_id.to_string(),
            self.state_db.as_deref(),
        )
        .await
        .map_err(|error| {
            internal_error(format!(
                "failed to locate unloaded goal thread {thread_id}: {error}"
            ))
        })?
        .ok_or_else(|| invalid_request(format!("thread not found: {thread_id}")))?;
        let plain_rollout_path = codex_rollout::plain_rollout_path(path.as_path());
        let recorder = RolloutRecorder::new_with_writer_lock(
            self.config.as_ref(),
            RolloutRecorderParams::resume(path),
            writer_lock,
        )
        .await
        .map_err(|error| {
            internal_error(format!(
                "failed to open unloaded goal chronology for {thread_id}: {error}"
            ))
        })?;
        Ok(Some(UnloadedGoalRollout {
            recorder,
            plain_rollout_path,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_active_result_is_not_a_person_stop() {
        let thread_id = ThreadId::new();
        note_person_goal_set(
            thread_id,
            codex_protocol::protocol::ThreadGoalStatus::Active,
        );
        assert_eq!(
            codex_alab_core::goal_stops::person_stops(&thread_id.to_string()),
            0
        );

        note_person_goal_set(
            thread_id,
            codex_protocol::protocol::ThreadGoalStatus::Paused,
        );
        assert_eq!(
            codex_alab_core::goal_stops::person_stops(&thread_id.to_string()),
            1
        );
    }

    #[tokio::test]
    async fn goal_output_queue_is_fifo_per_thread_without_global_backpressure() {
        let queue = Arc::new(GoalOutputQueue::default());
        let blocked_thread = ThreadId::new();
        let independent_thread = ThreadId::new();
        let (release, wait) = tokio::sync::oneshot::channel();
        let (observed, mut observations) = tokio::sync::mpsc::unbounded_channel();

        let first = observed.clone();
        queue.enqueue(
            blocked_thread,
            queue.reserve(blocked_thread).unwrap(),
            Box::pin(async move {
                let _ = wait.await;
                let _ = first.send(1);
            }),
        );
        let second = observed.clone();
        queue.enqueue(
            blocked_thread,
            queue.reserve(blocked_thread).unwrap(),
            Box::pin(async move {
                let _ = second.send(2);
            }),
        );
        queue.enqueue(
            independent_thread,
            queue.reserve(independent_thread).unwrap(),
            Box::pin(async move {
                let _ = observed.send(3);
            }),
        );

        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), observations.recv())
                .await
                .unwrap(),
            Some(3)
        );
        let _ = release.send(());
        assert_eq!(observations.recv().await, Some(1));
        assert_eq!(observations.recv().await, Some(2));
        tokio::task::yield_now().await;
        assert!(
            queue
                .tails
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );
    }

    #[test]
    fn goal_output_queue_rejects_a_thread_before_its_backlog_can_grow_unbounded() {
        let queue = GoalOutputQueue::default();
        let thread_id = ThreadId::new();
        let reservations = (0..THREAD_OUTPUT_CAPACITY)
            .map(|_| queue.reserve(thread_id).expect("capacity should remain"))
            .collect::<Vec<_>>();
        assert!(queue.reserve(thread_id).is_none());
        drop(reservations);
        assert!(queue.reserve(thread_id).is_some());
    }
}
