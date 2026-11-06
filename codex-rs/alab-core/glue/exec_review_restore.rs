// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Reattach a captured review consumer to fresh runtime bindings. Construction
//! and task selection do not replace Core's required native turn admission.
use super::ExecReviewSnapshot;
use super::Handle;
use super::State;
use crate::CodexThread;
use crate::session::TurnInput;
use crate::session::input_queue::exec_input_state;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::tasks::SessionTask;
use crate::tasks::SessionTaskResult;
use codex_alab_core::quota::HeadlessExec;
use codex_extension_api::ExtensionDataInit;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_protocol::protocol::InternalSessionSource;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::user_input::UserInput;
use std::sync::Arc;
use std::sync::Weak;
use tokio_util::sync::CancellationToken;

struct Prepared {
    snapshot: ExecReviewSnapshot,
    parent: Weak<Session>,
    child: Arc<CodexThread>,
}

pub(crate) fn has_prepared(init: &ExtensionDataInit) -> bool {
    init.get::<Prepared>().is_some()
}

impl ExecReviewSnapshot {
    /// Attach data only to the exact fresh, idle runtimes that hold the
    /// original input frames. The caller must still obtain native admission.
    pub async fn prepare_recovery(
        &self,
        parent: &Arc<CodexThread>,
        child: &Arc<CodexThread>,
    ) -> std::result::Result<ExtensionDataInit, String> {
        self.validate(parent.session.thread_id, &self.turn)?;
        validate_child(&parent.session, child, self).await?;
        if parent
            .thread_extension_data()
            .get::<HeadlessExec>()
            .is_none()
            || parent.session.active_turn.lock().await.is_some()
            || child
                .thread_extension_data()
                .get::<crate::ExecRestoredMember>()
                .is_none()
            || exec_input_state::pending_origin(&parent.session)
                != Some((self.parent, self.turn.clone()))
            || child.session.active_turn.lock().await.is_some()
        {
            return Err("review recovery runtimes do not own their original input frames".into());
        }
        match exec_input_state::pending_origin(&child.session) {
            Some(origin) if origin == (self.delegate, self.delegate_turn.clone()) => {}
            None => {
                child
                    .flush_rollout()
                    .await
                    .map_err(|error| error.to_string())?;
                if !self.retains_completed_delegate(
                    &child
                        .exec_recovery_history()
                        .await
                        .map_err(|error| error.to_string())?,
                ) {
                    return Err(
                        "review recovery has no original delegate frame or retained completion"
                            .into(),
                    );
                }
            }
            _ => return Err("review recovery delegate owns another input frame".into()),
        }
        let mut init = ExtensionDataInit::new();
        init.insert(Prepared {
            snapshot: self.clone(),
            parent: Arc::downgrade(&parent.session),
            child: Arc::clone(child),
        });
        Ok(init)
    }
}

async fn validate_child(
    parent: &Session,
    child: &CodexThread,
    snapshot: &ExecReviewSnapshot,
) -> std::result::Result<(), String> {
    let settings = child.session.thread_config_snapshot().await;
    if parent.thread_id != snapshot.parent
        || child.session.thread_id != snapshot.delegate
        || !child.is_running()
        || child.session_source != SessionSource::Internal(InternalSessionSource::Review)
        || !child
            .session
            .services
            .local_agent_runtime
            .shares_exec_runtime(&parent.services.local_agent_runtime)
        || !Arc::ptr_eq(
            &child.session.services.auth_manager,
            &parent.services.auth_manager,
        )
        || !Arc::ptr_eq(
            &child.session.services.exec_policy,
            &parent.services.exec_policy,
        )
        || settings.parent_thread_id != Some(parent.thread_id)
        || settings.session_source != SessionSource::SubAgent(SubAgentSource::Review)
    {
        return Err("review recovery delegate has a different owner or identity".into());
    }
    Ok(())
}

impl State {
    fn restore_data(&self, snapshot: &ExecReviewSnapshot) -> std::result::Result<(), String> {
        snapshot.validate(self.parent, &self.turn)?;
        if self.receiver.get().is_some() {
            return Err("review consumer already has a receiver".into());
        }
        let mut data = self
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if data.delegate.is_some() || data.buffered.is_some() || !data.pending.is_empty() {
            return Err("review consumer already owns live state".into());
        }
        data.delegate = Some(snapshot.delegate);
        data.delegate_turn = Some(snapshot.delegate_turn.clone());
        data.buffered = snapshot.buffered.clone();
        data.pending = snapshot.pending.iter().cloned().collect();
        Ok(())
    }
}

pub(crate) async fn start_or_resume(
    session: Arc<Session>,
    context: Arc<TurnContext>,
    input: Vec<UserInput>,
    cancel: CancellationToken,
) -> Result<Option<async_channel::Receiver<codex_protocol::protocol::Event>>> {
    let Some(prepared) = context.extension_data.get::<Prepared>() else {
        let bridge_cancel = super::delegate_cancel(&context, &cancel);
        return Ok(
            super::super::start_review_conversation(session, context, input, bridge_cancel).await,
        );
    };
    let fail = |message: String| CodexErr::InvalidRequest(message);
    prepared
        .snapshot
        .validate(session.thread_id, &context.sub_id)
        .map_err(fail)?;
    if !prepared
        .parent
        .upgrade()
        .is_some_and(|parent| Arc::ptr_eq(&parent, &session))
    {
        return Err(fail("review recovery parent runtime was replaced".into()));
    }
    validate_child(&session, &prepared.child, &prepared.snapshot)
        .await
        .map_err(fail)?;
    if !owns_original_turn(&prepared.child, &prepared.snapshot, &cancel).await? {
        return Err(fail(
            "review delegate no longer owns the original turn".into(),
        ));
    }
    let handle = context
        .extension_data
        .get::<Handle>()
        .ok_or_else(|| fail("review recovery has no consumer".into()))?;
    handle.0.restore_data(&prepared.snapshot).map_err(fail)?;
    {
        let mut data = handle
            .0
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        data.session = Some(Arc::downgrade(&prepared.child.session));
        data.restored_io = Some(prepared.child.io.clone());
    }
    // Reuse the existing event filter and cancellation bridge. No new delegate,
    // initial prompt or model request is created by reattachment.
    let bridge_cancel = super::delegate_cancel(&context, &cancel);
    let io = crate::codex_delegate::forward_session_io(
        Arc::new(prepared.child.io.clone()),
        bridge_cancel,
    );
    Ok(Some(io.rx_event))
}

async fn owns_original_turn(
    child: &CodexThread,
    snapshot: &ExecReviewSnapshot,
    cancel: &CancellationToken,
) -> Result<bool> {
    let mut status = child.subscribe_status();
    loop {
        let current_status = status.borrow_and_update().clone();
        let task = {
            let active = child.session.active_turn.lock().await;
            active.as_ref().map(|turn| {
                turn.task
                    .as_ref()
                    .map(|task| task.turn_context.sub_id == snapshot.delegate_turn)
            })
        };
        if let Some(Some(same)) = task {
            return Ok(same);
        }
        if let Some(origin) = exec_input_state::pending_origin(&child.session) {
            return Ok(origin == (snapshot.delegate, snapshot.delegate_turn.clone()));
        }
        if task.is_none()
            || !matches!(
                current_status,
                codex_protocol::protocol::AgentStatus::PendingInit
                    | codex_protocol::protocol::AgentStatus::Running
            )
        {
            child.flush_rollout().await?;
            let history = child
                .exec_recovery_history()
                .await
                .map_err(|error| CodexErr::InvalidRequest(error.to_string()))?;
            return Ok(snapshot.delegate_completed(&history));
        }
        // Core temporarily removes RunningTask while persisting its terminal
        // event. Wait for that status publication instead of rejecting the gap.
        tokio::select! {
            _ = cancel.cancelled() => return Err(CodexErr::TurnAborted),
            changed = status.changed() => if changed.is_err() { return Ok(false); },
        }
    }
}

pub(crate) enum RecoveryTask {
    Regular,
    Review,
}

pub(crate) fn recovery_task(context: &TurnContext) -> RecoveryTask {
    if context.extension_data.get::<Prepared>().is_some() {
        RecoveryTask::Review
    } else {
        RecoveryTask::Regular
    }
}

impl SessionTask for RecoveryTask {
    fn kind(&self) -> crate::state::TaskKind {
        match self {
            Self::Regular => crate::state::TaskKind::Regular,
            Self::Review => crate::state::TaskKind::Review,
        }
    }
    fn span_name(&self) -> &'static str {
        match self {
            Self::Regular => "session_task.turn",
            Self::Review => "session_task.review",
        }
    }
    async fn run(
        self: Arc<Self>,
        session: Arc<Session>,
        context: Arc<TurnContext>,
        input: Vec<TurnInput>,
        cancel: CancellationToken,
    ) -> SessionTaskResult {
        match self.as_ref() {
            Self::Regular => {
                Arc::new(crate::tasks::RegularTask::new())
                    .run(session, context, input, cancel)
                    .await
            }
            Self::Review => {
                Arc::new(super::super::ReviewTask::new())
                    .run(session, context, input, cancel)
                    .await
            }
        }
    }
    async fn abort(&self, session: Arc<Session>, context: Arc<TurnContext>) {
        match self {
            Self::Regular => {
                crate::tasks::RegularTask::new()
                    .abort(session, context)
                    .await
            }
            Self::Review => {
                super::super::ReviewTask::new()
                    .abort(session, context)
                    .await
            }
        }
    }
}

#[cfg(test)]
#[path = "exec_review_restore_tests.rs"]
mod tests;
