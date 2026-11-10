// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Stop a prepared child without ending its turn or shutting down its siblings.
use super::ExecRecoveryOptions;
use crate::CodexThread;
use crate::ExecTreeSeal;
use crate::session::input_queue::exec_input_state;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::state::TaskKind;
use codex_alab_core::quota::HeadlessExec;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

type StopResult = std::result::Result<Option<ExecRecoveryOptions>, String>;

/// Retains the exact source task selected under a tree barrier. This is not
/// authority to execute a replacement; native admission is still required.
#[derive(Clone)]
pub struct ExecPreparedChild {
    session: Arc<Session>,
    context: Arc<TurnContext>,
    turn_state: Arc<tokio::sync::Mutex<crate::state::TurnState>>,
    native_done: Arc<tokio::sync::Notify>,
    outcome: Arc<OnceLock<tokio::sync::watch::Receiver<Option<StopResult>>>>,
}

fn refused(message: &str) -> CodexErr {
    CodexErr::InvalidRequest(message.into())
}

impl CodexThread {
    /// Preflight every child before committing the barrier and stopping any
    /// producer. The barrier must remain held until the whole tree is captured.
    pub async fn prepare_exec_child(&self, seal: &ExecTreeSeal) -> Result<ExecPreparedChild> {
        if !self.session_source.is_non_root_agent() {
            return Err(refused("child suspension requires a descendant session"));
        }
        ExecPreparedChild::prepare(Arc::clone(&self.session), seal).await
    }
}

impl ExecPreparedChild {
    /// Capture this child's immutable configuration before committing to stop.
    /// The invocation supplies original CLI inputs which are absent from layers.
    pub fn capture_configuration(
        &self,
        invocation: &crate::config::ExecConfiguration,
    ) -> std::io::Result<crate::config::ExecConfiguration> {
        invocation.capture_child(&self.context.config)
    }

    async fn prepare(session: Arc<Session>, seal: &ExecTreeSeal) -> Result<Self> {
        if !seal.protects(&session.services.local_agent_runtime, false) {
            return Err(refused(
                "child suspension requires its own drained tree barrier",
            ));
        }
        let (context, turn_state, native_done) = {
            let active = session.active_turn.lock().await;
            let active = active
                .as_ref()
                .ok_or_else(|| refused("child suspension requires an active turn"))?;
            let task = active
                .task
                .as_ref()
                .ok_or_else(|| refused("child suspension requires an active turn"))?;
            if (task.kind != TaskKind::Regular
                && !crate::tasks::review_recovery::capture_ready(task.kind, &task.turn_context))
                || task.cancellation_token.is_cancelled()
            {
                return Err(refused("child suspension requires a live regular turn"));
            }
            (
                Arc::clone(&task.turn_context),
                Arc::clone(&active.turn_state),
                Arc::clone(&task.done),
            )
        };
        exec_input_state::validate_session_input(&session, &context.sub_id, false)
            .await
            .map_err(CodexErr::InvalidRequest)?;
        // A persisted child has a canonical writer. An ephemeral child must
        // already have recorded its original start; marking it now is too late.
        let _ = super::ephemeral::prepare_suspension(&session).await?;
        let history = if session.live_thread().is_some() {
            session
                .services
                .thread_store
                .load_latest_turn_history(codex_thread_store::LoadThreadHistoryParams {
                    thread_id: session.thread_id,
                    include_archived: false,
                })
                .await
        } else {
            super::ephemeral::history(&session)
        }
        .map_err(|error| CodexErr::Fatal(error.to_string()))?;
        if history.thread_id != session.thread_id
            || super::ephemeral::open_turn(&history.items) != Some(context.sub_id.as_str())
        {
            return Err(refused(
                "child history does not name its unfinished original turn",
            ));
        }
        Ok(Self {
            session,
            context,
            turn_state,
            native_done,
            outcome: Arc::new(OnceLock::new()),
        })
    }

    /// Stop only the selected task. Normal tree shutdown subsequently closes
    /// its producers and writer; capture the batch after that shared barrier.
    /// None means this exact task completed naturally before removal.
    /// Repeated calls on this preparation reconcile the same result. Once
    /// accepted, caller cancellation cannot abandon the removed task's input.
    pub async fn suspend(&self, seal: &ExecTreeSeal) -> Result<Option<ExecRecoveryOptions>> {
        if !seal.protects(&self.session.services.local_agent_runtime, true) {
            return Err(refused(
                "child suspension requires its committed tree barrier",
            ));
        }
        let mut outcome = self
            .outcome
            .get_or_init(|| {
                let (sender, receiver) = tokio::sync::watch::channel(None);
                let prepared = self.clone();
                // This owned task outlives a dropped request. OnceLock publishes
                // exactly one receiver even when several callers arrive together.
                tokio::spawn(async move {
                    let result = prepared.stop().await.map_err(|error| error.to_string());
                    let _ = sender.send_replace(Some(result));
                });
                receiver
            })
            .clone();
        loop {
            let result = { outcome.borrow_and_update().clone() };
            if let Some(result) = result {
                return result.map_err(CodexErr::Fatal);
            }
            outcome.changed().await.map_err(|_| {
                CodexErr::Fatal("child suspension owner exited before retaining its outcome".into())
            })?;
        }
    }

    async fn stop(self) -> Result<Option<ExecRecoveryOptions>> {
        let session = &self.session;
        let mut turn = loop {
            // Native completion removes the task before it clears ActiveTurn.
            // Register first so completion between inspection and waiting is not lost.
            let finished = self.native_done.notified();
            tokio::pin!(finished);
            finished.as_mut().enable();
            {
                let mut active = session.active_turn.lock().await;
                let Some(current) = active.as_ref() else {
                    return Ok(None);
                };
                if !Arc::ptr_eq(&current.turn_state, &self.turn_state) {
                    return Err(refused("prepared child task changed before suspension"));
                }
                if let Some(task) = current.task.as_ref() {
                    if !Arc::ptr_eq(&task.turn_context, &self.context)
                        || (task.kind != TaskKind::Regular
                            && !crate::tasks::review_recovery::suspension_ready(
                                task.kind,
                                &task.turn_context,
                            ))
                        || task.cancellation_token.is_cancelled()
                    {
                        return Err(refused("prepared child task changed before suspension"));
                    }
                    // Mark before removal so accepted input belongs to the stopped
                    // original turn. This does not create missing ephemeral history.
                    session.services.thread_extension_data.insert(HeadlessExec);
                    break active
                        .take()
                        .ok_or_else(|| refused("active child disappeared during suspension"))?;
                }
                if current.finishing.is_none() {
                    return Err(refused("prepared child task changed before suspension"));
                }
            }
            // Keep custody through this exact task's native finalization. No
            // active-turn guard is held while its completion signal is awaited.
            finished.await;
        };
        let task = turn
            .task
            .take()
            .ok_or_else(|| refused("child task disappeared during suspension"))?;
        super::transfer::mark(&task.turn_context);
        task.cancellation_token.cancel();
        task.turn_context
            .turn_metadata_state
            .cancel_git_enrichment_task();
        let mut handle = task.handle.detach();
        if tokio::time::timeout(
            Duration::from_millis(crate::tasks::GRACEFULL_INTERRUPTION_TIMEOUT_MS),
            &mut handle,
        )
        .await
        .is_err()
        {
            handle.abort();
            let _ = handle.await;
        }
        // The root owns the shared controller mailbox exactly once.
        exec_input_state::freeze_local(session, &turn, &self.context.sub_id, false).await;
        Ok(Some(ExecRecoveryOptions::from_context(
            session.thread_id,
            &self.context,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::input_queue::TurnInput;
    use crate::tasks::SessionTask;
    use crate::tasks::SessionTaskResult;
    use tokio_util::sync::CancellationToken;

    struct WaitingTask;
    impl SessionTask for WaitingTask {
        fn kind(&self) -> TaskKind {
            TaskKind::Regular
        }
        fn span_name(&self) -> &'static str {
            "exec_child_suspension_test"
        }
        async fn run(
            self: Arc<Self>,
            _session: Arc<Session>,
            _ctx: Arc<TurnContext>,
            _input: Vec<TurnInput>,
            token: CancellationToken,
        ) -> SessionTaskResult {
            token.cancelled().await;
            Ok(None)
        }
    }

    async fn running() -> (Arc<Session>, String) {
        let (session, context) = crate::session::tests::make_session_and_context().await;
        session.services.thread_extension_data.insert(HeadlessExec);
        let turn = context.sub_id.clone();
        let session = Arc::new(session);
        let context = Arc::new(context);
        session
            .spawn_task(Arc::clone(&context), Vec::new(), WaitingTask)
            .await;
        session.emit_turn_started(&context, TaskKind::Regular).await;
        (session, turn)
    }

    struct CompletingTask(Arc<tokio::sync::Notify>);

    impl SessionTask for CompletingTask {
        fn kind(&self) -> TaskKind {
            TaskKind::Regular
        }
        fn span_name(&self) -> &'static str {
            "exec_child_natural_completion_test"
        }
        async fn run(
            self: Arc<Self>,
            _session: Arc<Session>,
            _ctx: Arc<TurnContext>,
            _input: Vec<TurnInput>,
            _token: CancellationToken,
        ) -> SessionTaskResult {
            self.0.notified().await;
            Ok(None)
        }
    }

    struct NativeCompletionGate {
        entered: tokio::sync::Notify,
        release: tokio::sync::Semaphore,
    }

    impl codex_extension_api::TurnLifecycleContributor for NativeCompletionGate {
        fn on_turn_stop<'a>(
            &'a self,
            _input: codex_extension_api::TurnStopInput<'a>,
        ) -> codex_extension_api::ExtensionFuture<'a, ()> {
            Box::pin(async move {
                self.entered.notify_one();
                self.release
                    .acquire()
                    .await
                    .expect("completion released")
                    .forget();
            })
        }
    }

    #[tokio::test]
    async fn naturally_finishing_child_waits_for_native_completion_before_becoming_idle() {
        let (mut session, context) = crate::session::tests::make_session_and_context().await;
        let completion = Arc::new(NativeCompletionGate {
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Semaphore::new(0),
        });
        let mut extensions =
            codex_extension_api::ExtensionRegistryBuilder::<crate::config::Config>::new();
        extensions.turn_lifecycle_contributor(completion.clone());
        session.services.extensions = Arc::new(extensions.build());
        session.services.thread_extension_data.insert(HeadlessExec);
        let session = Arc::new(session);
        let context = Arc::new(context);
        let finish = Arc::new(tokio::sync::Notify::new());
        session
            .spawn_task(
                Arc::clone(&context),
                Vec::new(),
                CompletingTask(Arc::clone(&finish)),
            )
            .await;
        session.emit_turn_started(&context, TaskKind::Regular).await;
        let mut seal = session
            .services
            .local_agent_runtime
            .seal_exec_tree_starts()
            .await
            .unwrap();
        let prepared = ExecPreparedChild::prepare(Arc::clone(&session), &seal)
            .await
            .unwrap();
        seal.commit();

        // Native completion removes the task before it records the terminal
        // event and clears ActiveTurn. Hold that real transition open.
        finish.notify_one();
        tokio::time::timeout(Duration::from_secs(5), completion.entered.notified())
            .await
            .expect("native completion reached its stop callback");
        {
            let active = session.active_turn.lock().await;
            assert!(
                active
                    .as_ref()
                    .is_some_and(|turn| turn.task.is_none() && turn.finishing.is_some())
            );
        }
        let waiting = prepared.suspend(&seal);
        tokio::pin!(waiting);
        let premature = tokio::time::timeout(Duration::from_millis(100), waiting.as_mut()).await;
        completion.release.add_permits(1);
        assert!(
            premature.is_err(),
            "completion was reported while the native turn was still active"
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(5), waiting)
                .await
                .expect("native completion settled")
                .unwrap()
                .is_none()
        );
        assert!(session.active_turn.lock().await.is_none());
        let history = super::super::ephemeral::history(&session).unwrap();
        assert!(history.items.iter().any(|item| matches!(item,
            codex_history::RolloutItem::EventMsg(codex_protocol::protocol::EventMsg::TurnComplete(event)) if event.turn_id == context.sub_id)));
        assert!(!history.items.iter().any(|item| matches!(
            item,
            codex_history::RolloutItem::EventMsg(codex_protocol::protocol::EventMsg::TurnAborted(
                _
            ))
        )));
    }

    #[tokio::test]
    async fn child_stop_preserves_open_turn_and_leaves_controller_available() {
        let (session, turn) = running().await;
        let (mut sibling, sibling_context) =
            crate::session::tests::make_session_and_context().await;
        sibling.services.local_agent_runtime = session.services.local_agent_runtime.clone();
        let sibling = Arc::new(sibling);
        sibling
            .spawn_task(Arc::new(sibling_context), Vec::new(), WaitingTask)
            .await;
        let mut seal = session
            .services
            .local_agent_runtime
            .seal_exec_tree_starts()
            .await
            .unwrap();
        let prepared = ExecPreparedChild::prepare(Arc::clone(&session), &seal)
            .await
            .unwrap();
        seal.commit();
        let options = prepared.suspend(&seal).await.unwrap().unwrap();
        assert_eq!(options.turn(), turn);
        assert!(session.active_turn.lock().await.is_none());
        assert!(
            sibling
                .active_turn
                .lock()
                .await
                .as_ref()
                .unwrap()
                .task
                .as_ref()
                .is_some_and(|task| !task.cancellation_token.is_cancelled())
        );
        // Local freezing must not seal the tree's shared mailbox. The root
        // can still take its single controller snapshot during shutdown.
        assert!(
            session
                .services
                .local_agent_runtime
                .freeze_exec_mailboxes()
                .is_ok()
        );
        let history = super::super::ephemeral::history(&session).unwrap();
        assert!(history.items.iter().any(|item| matches!(item,
            codex_history::RolloutItem::EventMsg(codex_protocol::protocol::EventMsg::TurnStarted(event)) if event.turn_id == turn)));
        assert!(!history.items.iter().any(|item| matches!(
            item,
            codex_history::RolloutItem::EventMsg(
                codex_protocol::protocol::EventMsg::TurnAborted(_)
                    | codex_protocol::protocol::EventMsg::TurnComplete(_)
            )
        )));
    }

    #[tokio::test]
    async fn uncommitted_or_foreign_barrier_cannot_stop_the_task() {
        let (session, _) = running().await;
        let (other, _) = running().await;
        let foreign = other
            .services
            .local_agent_runtime
            .seal_exec_tree_starts()
            .await
            .unwrap();
        assert!(
            ExecPreparedChild::prepare(Arc::clone(&session), &foreign)
                .await
                .is_err()
        );
        let seal = session
            .services
            .local_agent_runtime
            .seal_exec_tree_starts()
            .await
            .unwrap();
        let prepared = ExecPreparedChild::prepare(Arc::clone(&session), &seal)
            .await
            .unwrap();
        assert!(prepared.suspend(&seal).await.is_err());
        assert!(
            session
                .active_turn
                .lock()
                .await
                .as_ref()
                .unwrap()
                .task
                .as_ref()
                .is_some_and(|task| !task.cancellation_token.is_cancelled())
        );
    }
    #[tokio::test]
    async fn missing_ephemeral_start_is_refused_before_stopping() {
        let (session, context) = crate::session::tests::make_session_and_context().await;
        let session = Arc::new(session);
        session
            .spawn_task(Arc::new(context), Vec::new(), WaitingTask)
            .await;
        let seal = session
            .services
            .local_agent_runtime
            .seal_exec_tree_starts()
            .await
            .unwrap();
        assert!(
            ExecPreparedChild::prepare(Arc::clone(&session), &seal)
                .await
                .is_err()
        );
        assert!(
            session
                .active_turn
                .lock()
                .await
                .as_ref()
                .unwrap()
                .task
                .as_ref()
                .is_some_and(|task| !task.cancellation_token.is_cancelled())
        );
        assert!(
            session
                .services
                .thread_extension_data
                .get::<HeadlessExec>()
                .is_none()
        );
    }
    #[tokio::test]
    async fn mismatched_canonical_turn_is_refused_before_stopping() {
        let (session, original) = running().await;
        let (_, mut other) = crate::session::tests::make_session_and_context().await;
        other.sub_id = format!("{original}-other");
        session.emit_turn_started(&other, TaskKind::Regular).await;
        let seal = session
            .services
            .local_agent_runtime
            .seal_exec_tree_starts()
            .await
            .unwrap();
        assert!(
            ExecPreparedChild::prepare(Arc::clone(&session), &seal)
                .await
                .is_err()
        );
        assert!(
            session
                .active_turn
                .lock()
                .await
                .as_ref()
                .unwrap()
                .task
                .as_ref()
                .is_some_and(|task| task.turn_context.sub_id == original
                    && !task.cancellation_token.is_cancelled())
        );
    }
    struct HeldCancellation {
        seen: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }

    impl SessionTask for HeldCancellation {
        fn kind(&self) -> TaskKind {
            TaskKind::Regular
        }
        fn span_name(&self) -> &'static str {
            "held_child_cancellation"
        }
        async fn run(
            self: Arc<Self>,
            _session: Arc<Session>,
            _ctx: Arc<TurnContext>,
            _input: Vec<TurnInput>,
            token: CancellationToken,
        ) -> SessionTaskResult {
            token.cancelled().await;
            self.seen.notify_one();
            self.release.notified().await;
            Ok(None)
        }
    }

    #[tokio::test]
    async fn dropped_caller_does_not_abandon_input_and_retry_reconciles_same_stop() {
        let (session, context) = crate::session::tests::make_session_and_context().await;
        session.services.thread_extension_data.insert(HeadlessExec);
        let session = Arc::new(session);
        let context = Arc::new(context);
        let seen = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        session
            .spawn_task(
                Arc::clone(&context),
                Vec::new(),
                HeldCancellation {
                    seen: Arc::clone(&seen),
                    release: Arc::clone(&release),
                },
            )
            .await;
        session.emit_turn_started(&context, TaskKind::Regular).await;
        let mut seal = session
            .services
            .local_agent_runtime
            .seal_exec_tree_starts()
            .await
            .unwrap();
        let prepared = ExecPreparedChild::prepare(Arc::clone(&session), &seal)
            .await
            .unwrap();
        // Hold the checkpoint lock so completion cannot precede the lost reply.
        let turn_state = session
            .active_turn
            .lock()
            .await
            .as_ref()
            .unwrap()
            .turn_state
            .clone();
        let (checkpoint_locked, checkpoint_ready) = tokio::sync::oneshot::channel();
        let (release_checkpoint, checkpoint_release) = std::sync::mpsc::channel();
        // A blocking owner holds the deliberate test obstruction. The async
        // observer never carries a checkpoint guard across a suspension await.
        let lock_owner = tokio::task::spawn_blocking(move || {
            let _checkpoint_lock = turn_state.blocking_lock();
            checkpoint_locked.send(()).unwrap();
            let _ = checkpoint_release.recv();
        });
        checkpoint_ready.await.unwrap();
        seal.commit();
        let mut caller = Box::pin(prepared.suspend(&seal));
        assert!(futures::poll!(caller.as_mut()).is_pending());
        seen.notified().await;
        assert!(session.active_turn.lock().await.is_none());
        drop(caller);
        release.notify_one();
        let mut outcome = prepared.outcome.get().unwrap().clone();
        assert!(outcome.borrow().is_none());
        release_checkpoint.send(()).unwrap();
        lock_owner.await.unwrap();
        // Observe the owned operation without driving another suspend future.
        tokio::time::timeout(Duration::from_secs(5), outcome.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(
            outcome
                .borrow()
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .is_some()
        );
        let first = prepared.suspend(&seal).await.unwrap().unwrap();
        let repeated = prepared.clone().suspend(&seal).await.unwrap().unwrap();
        assert_eq!(first.thread(), repeated.thread());
        assert_eq!(first.turn(), context.sub_id);
        assert_eq!(first.turn(), repeated.turn());
        let history = super::super::ephemeral::history(&session).unwrap();
        assert_eq!(
            super::super::ephemeral::open_turn(&history.items),
            Some(context.sub_id.as_str())
        );
    }
}
