// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Idle mail has no unfinished turn or turn-local accounting to restore.
use super::*;

struct FrozenIdle {
    thread: ThreadId,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IdleMetadata {
    pub(super) thread: ThreadId,
    pub(super) mailbox_options: Vec<SavedStart>,
}

/// Retained idle mail, not permission to start a turn or activate delivery.
/// The native tree consumer must supply fresh admission before installation.
pub struct PreparedExecIdleInput {
    thread: ThreadId,
    pub(super) mailbox: Vec<(crate::PendingAgentMessage, TurnStartOptions)>,
}

impl PreparedExecIdleInput {
    pub fn thread(&self) -> ThreadId {
        self.thread
    }
    pub fn len(&self) -> usize {
        self.mailbox.len()
    }
    pub fn is_empty(&self) -> bool {
        self.mailbox.is_empty()
    }
}

impl IdleMetadata {
    pub(super) fn decode(self, input: Vec<TurnInput>) -> Result<PreparedExecIdleInput, String> {
        if input.len() != self.mailbox_options.len() {
            return Err("idle mailbox boundaries are invalid".into());
        }
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
}

#[expect(
    clippy::await_holding_invalid_type,
    reason = "idle task and mailbox portability must be checked atomically"
)]
async fn validate_idle(
    session: &Session,
    seal: &crate::ExecTreeSeal,
    committed: bool,
) -> Result<(), String> {
    if !seal.protects(&session.services.local_agent_runtime, committed) {
        return Err("idle input requires its own drained tree barrier".into());
    }
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
    let input = session
        .input_queue
        .mailbox_pending_mails
        .lock()
        .await
        .iter()
        .map(|mail| TurnInput::InterAgentCommunication(mail.communication.clone()))
        .collect::<Vec<_>>();
    ExecPendingInputs::capture(&input).map(|_| ())
}

async fn seal_idle(session: &Session, seal: &crate::ExecTreeSeal) -> Result<(), String> {
    validate_idle(session, seal, true).await?;
    let active = session.active_turn.lock().await;
    if active.is_some() {
        return Err("idle input became active before sealing".into());
    }
    session.services.thread_extension_data.insert(FrozenIdle {
        thread: session.thread_id,
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
    Ok((
        IdleMetadata {
            thread: session.thread_id,
            mailbox_options: mailbox
                .iter()
                .map(|mail| mail.start_options.clone().into())
                .collect(),
        },
        mailbox
            .iter()
            .map(|mail| TurnInput::InterAgentCommunication(mail.communication.clone()))
            .collect(),
    ))
}

struct IdleInstalled;

pub(crate) fn activate(session: &Session) -> Result<(), String> {
    if session
        .services
        .thread_extension_data
        .get::<IdleInstalled>()
        .is_none()
    {
        return Err("idle mailbox has not been installed".into());
    }
    session.input_queue.notify_mailbox();
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
    session.services.thread_extension_data.insert(IdleInstalled);
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
                    mailbox: Vec::new()
                }
            )
            .await
            .is_err()
        );
    }
}
