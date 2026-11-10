// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Keep an exec review's original Session and event consumer, while registering
//! its lifetime privately with the parent's manager. This authorizes no recovery.
use super::ThreadManagerState;
use crate::CodexThread;
use crate::session::SessionIo;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_alab_core::quota::ExecQuotaMember;
use codex_alab_core::quota::HeadlessExec;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_protocol::protocol::InternalSessionSource;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use std::sync::Arc;
use std::sync::Weak;
use tokio::sync::OwnedRwLockReadGuard;

pub(crate) struct Registration {
    manager: Arc<ThreadManagerState>,
    parent: Arc<Session>,
    context: Arc<TurnContext>,
    _start: OwnedRwLockReadGuard<()>,
}

struct Parent(Weak<Session>);

pub(crate) fn bind_restore_parent(
    init: &mut codex_extension_api::ExtensionDataInit,
    parent: &Arc<Session>,
) {
    init.insert(Parent(Arc::downgrade(parent)));
}

pub(crate) fn watch_private_lifetime(manager: &Arc<ThreadManagerState>, thread: &Arc<CodexThread>) {
    if thread.session_source != SessionSource::Internal(InternalSessionSource::Review) {
        return;
    }
    let weak_manager = Arc::downgrade(manager);
    let weak_thread = Arc::downgrade(thread);
    let ended = thread.io.session_loop_termination.clone();
    tokio::spawn(async move {
        ended.await;
        let (Some(manager), Some(thread)) = (weak_manager.upgrade(), weak_thread.upgrade()) else {
            return;
        };
        let mut threads = manager.threads.write().await;
        if threads
            .get(&thread.session.thread_id)
            .is_some_and(|current| Arc::ptr_eq(current, &thread))
        {
            threads.remove(&thread.session.thread_id);
            thread.session.services.mcp_runtime.revoke_ema();
        }
    });
}

pub(crate) fn validate_registration_parent(
    threads: &std::collections::HashMap<codex_protocol::ThreadId, Arc<CodexThread>>,
    session: &Session,
    source: &SessionSource,
) -> Result<()> {
    if !matches!(
        source,
        SessionSource::Internal(InternalSessionSource::Review)
    ) {
        return Ok(());
    }
    let parent = session
        .services
        .thread_extension_data
        .get::<Parent>()
        .and_then(|parent| parent.0.upgrade());
    if parent.as_ref().is_some_and(|parent| {
        threads
            .get(&parent.thread_id)
            .is_some_and(|current| Arc::ptr_eq(&current.session, parent) && current.is_running())
    }) {
        Ok(())
    } else {
        Err(CodexErr::InvalidRequest(
            "review parent changed before registration".into(),
        ))
    }
}

pub(crate) async fn prepare(
    parent: &Arc<Session>,
    context: &Arc<TurnContext>,
    source: &SubAgentSource,
) -> Result<Option<Registration>> {
    let data = &parent.services.thread_extension_data;
    if !matches!(source, SubAgentSource::Review)
        || (data.get::<HeadlessExec>().is_none() && data.get::<ExecQuotaMember>().is_none())
    {
        return Ok(None);
    }
    parent
        .services
        .local_agent_runtime
        .prepare_exec_review(Arc::clone(parent), Arc::clone(context))
        .await
        .map(Some)
}

impl Registration {
    pub(crate) async fn prepare(
        manager: Arc<ThreadManagerState>,
        parent: Arc<Session>,
        context: Arc<TurnContext>,
        start: OwnedRwLockReadGuard<()>,
    ) -> Result<Self> {
        let owner = Self {
            manager,
            parent,
            context,
            _start: start,
        };
        owner.validate_parent().await?;
        Ok(owner)
    }

    async fn validate_parent(&self) -> Result<()> {
        let threads = self.manager.threads.read().await;
        if threads
            .get(&self.parent.thread_id)
            .is_some_and(|thread| Arc::ptr_eq(&thread.session, &self.parent) && thread.is_running())
        {
            Ok(())
        } else {
            Err(CodexErr::InvalidRequest(
                "review parent is no longer registered".into(),
            ))
        }
    }

    async fn register(self, session: Arc<Session>, io: SessionIo) -> Result<SessionIo> {
        self.validate_parent().await?;
        let snapshot = session.thread_config_snapshot().await;
        if snapshot.parent_thread_id != Some(self.parent.thread_id)
            || snapshot.session_source != SessionSource::SubAgent(SubAgentSource::Review)
            || !session
                .services
                .local_agent_runtime
                .shares_exec_runtime(&self.parent.services.local_agent_runtime)
            || !Arc::ptr_eq(
                &session.services.auth_manager,
                &self.parent.services.auth_manager,
            )
            || !Arc::ptr_eq(
                &session.services.exec_policy,
                &self.parent.services.exec_policy,
            )
        {
            return Err(CodexErr::InvalidRequest(
                "review delegate does not belong to its captured parent".into(),
            ));
        }
        // finalize_thread_spawn consumes only SessionConfigured, which the
        // delegate forwarder already suppresses. It adds no competing event reader.
        session
            .services
            .thread_extension_data
            .insert(Parent(Arc::downgrade(&self.parent)));
        let created = self
            .manager
            .finalize_thread_spawn(
                session,
                io,
                SessionSource::Internal(InternalSessionSource::Review),
            )
            .await?;
        let (created, acceptance) = created;
        if let Some(accepted) = acceptance {
            accepted.await;
        }
        let thread = created.thread;
        let result = thread.io.clone();
        watch_private_lifetime(&self.manager, &thread);
        crate::tasks::review_recovery::bind_child(&self.context, &self.parent, &thread)?;
        Ok(result)
    }
}

pub(crate) async fn finish(
    registration: Option<Registration>,
    session: Arc<Session>,
    io: SessionIo,
) -> Result<SessionIo> {
    let Some(registration) = registration else {
        return Ok(io);
    };
    let cleanup = io.clone();
    match registration.register(session, io).await {
        Ok(io) => Ok(io),
        Err(error) => {
            let _ = cleanup.shutdown_and_wait().await;
            Err(error)
        }
    }
}

pub(crate) fn live_resume_refusal(thread: &CodexThread) -> Option<CodexErr> {
    matches!(
        thread.session_source,
        SessionSource::Internal(InternalSessionSource::Review)
    )
    .then(|| CodexErr::InvalidRequest("cannot resume a live owner-private review delegate".into()))
}

#[cfg(test)]
#[path = "exec_review_registration_tests.rs"]
mod tests;
