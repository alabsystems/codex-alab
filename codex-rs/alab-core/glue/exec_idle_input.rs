// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Idle mail has no unfinished turn or turn-local accounting to restore.
use super::*;
use std::sync::Arc;

struct FrozenIdle {
    thread: ThreadId,
    upgrade: bool,
    controller: Option<crate::agent::control::FrozenExecMailboxes>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IdleMetadata {
    pub(super) thread: ThreadId,
    pub(super) mailbox_options: Vec<SavedStart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) controller_routes: Option<Vec<ControllerRoute>>,
}

/// Retained idle mail, not permission to start a turn or activate delivery.
/// The native tree consumer must supply fresh admission before installation.
pub struct PreparedExecIdleInput {
    thread: ThreadId,
    pub(super) mailbox: Vec<(crate::PendingAgentMessage, TurnStartOptions)>,
    controller: Option<Vec<crate::agent::control::ExecMailboxMessage>>,
}

impl PreparedExecIdleInput {
    pub(crate) fn owns_controller(&self) -> bool {
        self.controller.is_some()
    }

    pub fn thread(&self) -> ThreadId {
        self.thread
    }
    pub fn len(&self) -> usize {
        self.mailbox.len() + self.controller.as_ref().map_or(0, Vec::len)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl IdleMetadata {
    pub(super) fn decode(self, mut input: Vec<TurnInput>) -> Result<PreparedExecIdleInput, String> {
        if input.len()
            != self.mailbox_options.len() + self.controller_routes.as_ref().map_or(0, Vec::len)
        {
            return Err("idle mailbox boundaries are invalid".into());
        }
        let controller_input = input.split_off(self.mailbox_options.len());
        let controller = self
            .controller_routes
            .map(|routes| {
                controller_input
                    .into_iter()
                    .zip(routes)
                    .map(|(item, route)| match item {
                        TurnInput::InterAgentCommunication(communication) => {
                            Ok(crate::agent::control::ExecMailboxMessage {
                                thread: route.thread,
                                receive_id: route.receive_id,
                                communication,
                            })
                        }
                        _ => Err("idle controller contains a non-mail item".to_owned()),
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        let mailbox = input
            .into_iter()
            .zip(self.mailbox_options)
            .map(|(item, options)| match item {
                TurnInput::InterAgentCommunication(mail) => Ok((mail, options.into())),
                _ => Err("idle mailbox contains a non-mail item".to_owned()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(PreparedExecIdleInput {
            thread: self.thread,
            mailbox,
            controller,
        })
    }
}

impl crate::CodexThread {
    /// Check queue portability while the uncommitted barrier can still roll back.
    pub async fn validate_exec_idle_inputs(
        &self,
        seal: &crate::ExecTreeSeal,
    ) -> Result<(), String> {
        if !self.session_source.is_non_root_agent() {
            return Err("idle input validation requires a child session".into());
        }
        validate_idle(&self.session, seal, false).await
    }

    /// Record that a loaded child is idle under the committed source barrier.
    /// Capture its accepted mail only after the entire tree's producers stop.
    pub async fn seal_exec_idle_inputs(&self, seal: &crate::ExecTreeSeal) -> Result<(), String> {
        if !self.session_source.is_non_root_agent() {
            return Err("idle input sealing requires a child session".into());
        }
        seal_idle(&self.session, seal).await
    }

    /// Non-destructive preflight under this manager's still-revocable inventory.
    pub async fn validate_upgrade_idle_inputs(
        &self,
        inventory: &crate::UpgradeManagerBarrier,
    ) -> Result<(), String> {
        if !inventory.protects_member(self, false) {
            return Err("idle input preflight requires its own prepared manager".into());
        }
        validate_idle_state(&self.session, !self.session_source.is_non_root_agent()).await?;
        if !inventory.protects_member(self, false) {
            return Err("idle input preflight was superseded".into());
        }
        Ok(())
    }

    /// Complete fallible writer preparation while manager commitment is revocable.
    pub async fn prepare_upgrade_idle_inputs(
        &self,
        inventory: &crate::UpgradeManagerBarrier,
    ) -> Result<(), String> {
        self.validate_upgrade_idle_inputs(inventory).await?;
        persist_idle_history(&self.session).await?;
        self.validate_upgrade_idle_inputs(inventory).await
    }

    /// Seal source queues only after permanent native commitment. Callers must
    /// then join all source producers before capture; this grants no restore right.
    pub async fn seal_upgrade_idle_inputs(
        &self,
        inventory: &crate::CommittedUpgradeInventory,
    ) -> Result<(), String> {
        if !inventory.protects_member(self) {
            return Err("idle input sealing requires its own committed manager".into());
        }
        if let Some(frozen) = self
            .session
            .services
            .thread_extension_data
            .get::<FrozenIdle>()
        {
            return if frozen.upgrade && frozen.thread == self.session.thread_id {
                Ok(())
            } else {
                Err("idle input was sealed by another transfer".into())
            };
        }
        let root = !self.session_source.is_non_root_agent();
        validate_idle_state(&self.session, root).await?;
        persist_idle_history(&self.session).await?;
        let controller = if root {
            Some(
                self.session
                    .services
                    .local_agent_runtime
                    .freeze_exec_mailboxes()
                    .map_err(|error| error.to_string())?,
            )
        } else {
            None
        };
        self.session
            .services
            .thread_extension_data
            .insert(FrozenIdle {
                thread: self.session.thread_id,
                upgrade: true,
                controller,
            });
        Ok(())
    }
}

async fn validate_idle(
    session: &Session,
    seal: &crate::ExecTreeSeal,
    committed: bool,
) -> Result<(), String> {
    if !seal.protects(&session.services.local_agent_runtime, committed) {
        return Err("idle input requires its own drained tree barrier".into());
    }
    validate_idle_state(session, false).await
}

#[expect(
    clippy::await_holding_invalid_type,
    reason = "idle task and mailbox portability must be checked atomically"
)]
async fn validate_idle_state(session: &Session, owns_controller: bool) -> Result<(), String> {
    let active = session.active_turn.lock().await;
    if active.is_some()
        || session
            .services
            .thread_extension_data
            .get::<FrozenTurn>()
            .is_some()
    {
        return Err("idle input sealing found an active or suspended turn".into());
    }
    if !session
        .services
        .extensions
        .message_lifecycle_contributors()
        .is_empty()
    {
        return Err("idle input handoff requires a lifecycle-observer restoration protocol".into());
    }
    let mut mailbox = session.input_queue.mailbox_pending_mails.lock().await;
    // Retain accepted channel items in the same ordered queue before checking
    // portability. This does not publish delivery or consume a message.
    session.input_queue.read_mailbox(&mut mailbox);
    let input = mailbox
        .iter()
        .map(|mail| TurnInput::InterAgentCommunication(mail.communication.clone()))
        .collect::<Vec<_>>();
    let controller = if owns_controller {
        session
            .services
            .local_agent_runtime
            .exec_mailboxes()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|mail| TurnInput::InterAgentCommunication(mail.communication))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    ExecPendingInputs::capture_groups(&[&input, &controller]).map(|_| ())
}

// A durable but never-started member may still have only a lazy writer.
// Close must not discard its accepted startup/settings records before capture.
// Ephemeral sessions keep their existing journal; no missing store is retyped.
async fn persist_idle_history(session: &Session) -> Result<(), String> {
    let Some(writer) = session.live_thread() else {
        return Ok(());
    };
    let result = async {
        writer
            .persist(codex_thread_store::PersistContext::Standard)
            .await?;
        writer.flush().await
    }
    .await;
    match result {
        Ok(()) => Ok(()),
        Err(codex_thread_store::ThreadStoreError::ThreadNotFound { thread_id })
            if thread_id == session.thread_id =>
        {
            // A completed private delegate may already have closed its writer.
            // Require its actual durable idle evidence, never synthetic memory.
            let history = session
                .services
                .thread_store
                .load_latest_turn_history(codex_thread_store::LoadThreadHistoryParams {
                    thread_id,
                    include_archived: false,
                })
                .await
                .map_err(|e| format!("closed idle handoff history: {e}"))?;
            if history.thread_id != thread_id
                || crate::session::alab_exec_recovery_options::ephemeral::open_turn(&history.items)
                    .is_some()
            {
                return Err(
                    "closed idle handoff history changed its identity or terminal state".into(),
                );
            }
            Ok(())
        }
        Err(error) => Err(format!("idle handoff history persistence: {error}")),
    }
}

async fn seal_idle(session: &Session, seal: &crate::ExecTreeSeal) -> Result<(), String> {
    validate_idle(session, seal, true).await?;
    persist_idle_history(session).await?;
    let active = session.active_turn.lock().await;
    if active.is_some() {
        return Err("idle input became active before sealing".into());
    }
    session.services.thread_extension_data.insert(FrozenIdle {
        thread: session.thread_id,
        upgrade: false,
        controller: None,
    });
    Ok(())
}

pub(super) async fn capture(session: &Session) -> Result<(IdleMetadata, Vec<TurnInput>), String> {
    let frozen = session
        .services
        .thread_extension_data
        .get::<FrozenIdle>()
        .ok_or("idle input was not sealed before shutdown")?;
    if frozen.thread != session.thread_id || session.active_turn.lock().await.is_some() {
        return Err("idle input suspension changed origin or became active".into());
    }
    let mut mailbox = session.input_queue.mailbox_pending_mails.lock().await;
    // The root owns shared-controller custody. After tree shutdown this takes
    // no controller mail; it retains only this thread's already accepted queue.
    session.input_queue.read_mailbox(&mut mailbox);
    let controller = frozen
        .controller
        .as_ref()
        .map(crate::agent::control::FrozenExecMailboxes::snapshot);
    let metadata = IdleMetadata {
        thread: session.thread_id,
        mailbox_options: mailbox
            .iter()
            .map(|mail| mail.start_options.clone().into())
            .collect(),
        controller_routes: controller.as_ref().map(|messages| {
            messages
                .iter()
                .map(|mail| ControllerRoute {
                    thread: mail.thread,
                    receive_id: mail.receive_id.clone(),
                })
                .collect()
        }),
    };
    let mut input = mailbox
        .iter()
        .map(|mail| TurnInput::InterAgentCommunication(mail.communication.clone()))
        .collect::<Vec<_>>();
    if let Some(controller) = controller {
        input.extend(
            controller
                .into_iter()
                .map(|mail| TurnInput::InterAgentCommunication(mail.communication)),
        );
    }
    Ok((metadata, input))
}

struct IdleInstalled {
    controller: bool,
}

pub(crate) async fn activate(session: &Arc<Session>) -> Result<(), String> {
    let installed = session
        .services
        .thread_extension_data
        .get::<IdleInstalled>()
        .ok_or("idle mailbox has not been installed")?;
    if installed.controller {
        session
            .services
            .local_agent_runtime
            .activate_exec_mailboxes()
            .map_err(|error| error.to_string())?;
    }
    session.input_queue.notify_mailbox();
    // A local queue notification alone does not drive the submission loop's
    // pending-work scheduler. Match native mail submission after global release.
    if session.input_queue.has_trigger_turn_mailbox_items().await
        || session
            .features
            .enabled(codex_features::Feature::WakeOnAgentMessages)
        || session.has_outstanding_durable_sleep()
    {
        session.maybe_start_turn_for_pending_work().await;
    }
    Ok(())
}

// Called only by the owned child-restoration task after its live native gate.
// This queues retained mail without fabricating a recovered turn or accounting.
// It deliberately does not notify: bootstrap must install every idle queue
// before any child can wake another child with newer mail.
#[expect(
    clippy::await_holding_invalid_type,
    reason = "idle validation and accepted mailbox installation must be atomic"
)]
pub(crate) async fn install(session: &Session, input: PreparedExecIdleInput) -> Result<(), String> {
    if input.thread != session.thread_id {
        return Err("idle input belongs to another child".into());
    }
    if input.controller.is_some()
        && session
            .state
            .lock()
            .await
            .session_configuration
            .session_source
            .is_non_root_agent()
    {
        return Err("idle controller data requires its owning root".into());
    }
    let _start = session
        .services
        .local_agent_runtime
        .admit_exec_tree_start()
        .map_err(|e| e.to_string())?;
    let active = session.active_turn.lock().await;
    if active.is_some()
        || session
            .services
            .thread_extension_data
            .get::<FrozenTurn>()
            .is_some()
        || session
            .services
            .thread_extension_data
            .get::<IdleInstalled>()
            .is_some()
    {
        return Err("idle input requires a fresh idle child".into());
    }
    let owns_controller = input.controller.is_some();
    if let Some(controller) = input.controller {
        session
            .services
            .local_agent_runtime
            .restore_exec_mailboxes_quiet(controller)
            .map_err(|error| error.to_string())?;
    }
    let mut mailbox = session.input_queue.mailbox_pending_mails.lock().await;
    session.input_queue.read_mailbox(&mut mailbox);
    let mut restored = input
        .mailbox
        .into_iter()
        .map(
            |(communication, start_options)| PendingMailboxCommunication {
                communication,
                start_options,
                _diagnostics_guard: super::super::PENDING_MAILBOX_MESSAGES.track(),
            },
        )
        .collect::<std::collections::VecDeque<_>>();
    restored.append(&mut mailbox);
    *mailbox = restored;
    session
        .services
        .thread_extension_data
        .insert(IdleInstalled {
            controller: owns_controller,
        });
    drop(mailbox);
    drop(active);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::AgentPath;
    use codex_protocol::protocol::InterAgentCommunication;

    #[tokio::test]
    async fn idle_capture_requires_same_committed_barrier_and_keeps_source_mail() {
        let (session, _) = crate::session::tests::make_session_and_context().await;
        let (other, _) = crate::session::tests::make_session_and_context().await;
        let foreign = other
            .services
            .local_agent_runtime
            .seal_exec_tree_starts()
            .await
            .unwrap();
        assert!(seal_idle(&session, &foreign).await.is_err());
        let mut seal = session
            .services
            .local_agent_runtime
            .seal_exec_tree_starts()
            .await
            .unwrap();
        assert!(seal_idle(&session, &seal).await.is_err());
        let mail = crate::PendingAgentMessage::from(InterAgentCommunication::new(
            AgentPath::root(),
            AgentPath::root(),
            Vec::new(),
            "accepted while idle".into(),
            false,
        ));
        session
            .input_queue
            .mailbox_pending_mails
            .lock()
            .await
            .push_back(PendingMailboxCommunication {
                communication: mail,
                start_options: TurnStartOptions {
                    parent_turn_id: Some("parent".into()),
                    ..Default::default()
                },
                _diagnostics_guard: super::super::super::PENDING_MAILBOX_MESSAGES.track(),
            });
        validate_idle(&session, &seal, false).await.unwrap();
        assert!(capture(&session).await.is_err());
        assert_eq!(
            session.input_queue.mailbox_pending_mails.lock().await.len(),
            1
        );
        seal.commit();
        seal_idle(&session, &seal).await.unwrap();
        let (metadata, input) = capture(&session).await.unwrap();
        let prepared = metadata.decode(input).unwrap();
        assert_eq!(prepared.thread(), session.thread_id);
        assert_eq!(prepared.len(), 1);
        assert_eq!(
            prepared.mailbox[0].1.parent_turn_id.as_deref(),
            Some("parent")
        );
        assert!(session.active_turn.lock().await.is_none());
        assert_eq!(
            session.input_queue.mailbox_pending_mails.lock().await.len(),
            1
        );
        assert!(
            session
                .services
                .local_agent_runtime
                .freeze_exec_mailboxes()
                .is_ok()
        );
    }

    #[tokio::test]
    async fn idle_capture_refuses_reserved_and_previously_suspended_turns() {
        let (session, _) = crate::session::tests::make_session_and_context().await;
        let mut seal = session
            .services
            .local_agent_runtime
            .seal_exec_tree_starts()
            .await
            .unwrap();
        seal.commit();
        *session.active_turn.lock().await = Some(ActiveTurn::default());
        assert!(seal_idle(&session, &seal).await.is_err());
        *session.active_turn.lock().await = None;
        session.services.thread_extension_data.insert(FrozenTurn {
            thread: session.thread_id,
            turn: "original".into(),
            queue: Default::default(),
            accepts_mail: true,
            accounting: Default::default(),
            controller: Ok(None),
        });
        assert!(seal_idle(&session, &seal).await.is_err());
    }
    #[tokio::test]
    async fn restored_idle_mail_preserves_order_and_never_creates_a_turn() {
        let (session, _) = crate::session::tests::make_session_and_context().await;
        let mail = |body: &str| {
            crate::PendingAgentMessage::from(InterAgentCommunication::new(
                AgentPath::root(),
                AgentPath::root(),
                Vec::new(),
                body.into(),
                false,
            ))
        };
        let original = mail("captured first");
        let fresh = mail("arrived after bootstrap");
        session
            .input_queue
            .enqueue_mailbox_communication(fresh.clone(), Default::default())
            .await;
        let prepared = PreparedExecIdleInput {
            thread: session.thread_id,
            controller: None,
            mailbox: vec![(
                original.clone(),
                TurnStartOptions {
                    parent_turn_id: Some("parent".into()),
                    ..Default::default()
                },
            )],
        };
        install(&session, prepared).await.unwrap();
        assert!(session.active_turn.lock().await.is_none());
        {
            let queue = session.input_queue.mailbox_pending_mails.lock().await;
            assert_eq!(queue.len(), 2);
            assert_eq!(queue[0].communication, original);
            assert_eq!(
                queue[0].start_options.parent_turn_id.as_deref(),
                Some("parent")
            );
            assert_eq!(queue[1].communication, fresh);
        }
        assert!(
            install(
                &session,
                PreparedExecIdleInput {
                    thread: session.thread_id,
                    controller: None,
                    mailbox: Vec::new()
                }
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn root_controller_install_is_idle_once_only_and_rejects_a_child_owner() {
        let (session, _) = crate::session::tests::make_session_and_context().await;
        let session = Arc::new(session);
        let input = |thread| PreparedExecIdleInput {
            thread,
            mailbox: Vec::new(),
            controller: Some(Vec::new()),
        };
        install(&session, input(session.thread_id)).await.unwrap();
        assert!(session.active_turn.lock().await.is_none());
        assert!(install(&session, input(session.thread_id)).await.is_err());
        activate(&session).await.unwrap();
        let (child, _) = crate::session::tests::make_session_and_context().await;
        child
            .state
            .lock()
            .await
            .session_configuration
            .session_source = codex_protocol::protocol::SessionSource::Internal(
            codex_protocol::protocol::InternalSessionSource::MemoryConsolidation,
        );
        assert!(install(&child, input(child.thread_id)).await.is_err());
        assert!(
            child
                .services
                .thread_extension_data
                .get::<IdleInstalled>()
                .is_none()
        );
    }
}
