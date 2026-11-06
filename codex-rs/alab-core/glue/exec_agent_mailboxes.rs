// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Raw queue custody. Encoding shares the root input's publication identity table.
use super::super::LocalAgentRuntime;
use super::*;

#[derive(Clone)]
pub(crate) struct ExecMailboxMessage {
    pub(crate) thread: ThreadId,
    pub(crate) receive_id: Option<String>,
    pub(crate) communication: PendingAgentMessage,
}

#[derive(Default)]
pub(super) struct ExecMailboxCustody(std::sync::OnceLock<FrozenExecMailboxes>);

#[derive(Clone)]
pub(crate) struct FrozenExecMailboxes(std::sync::Arc<Mutex<Vec<ExecMailboxMessage>>>);

impl FrozenExecMailboxes {
    /// Read after the entire tree's shutdown barrier, including in-flight evictions.
    pub(crate) fn snapshot(&self) -> Vec<ExecMailboxMessage> {
        let mut messages = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        messages.sort_by_key(|message| message.thread.to_string());
        messages
    }
}

impl LocalAgentRuntime {
    pub(crate) fn exec_mailboxes(&self) -> Result<Vec<ExecMailboxMessage>> {
        let mailboxes = self
            .mailboxes
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mailboxes = mailboxes.as_ref().ok_or_else(|| {
            CodexErr::InvalidRequest("controller closed before mail capture".into())
        })?;
        let mut messages = Vec::new();
        for (thread, mailbox) in mailboxes {
            messages.extend(mailbox.pending.iter().map(|mail| ExecMailboxMessage {
                thread: *thread,
                receive_id: mail.id.clone(),
                communication: mail.communication.clone(),
            }));
        }
        messages.sort_by_key(|message| message.thread.to_string());
        Ok(messages)
    }

    /// Seal queue-only delivery under the same mutex that accepts it. Every
    /// accepted message is either in this custody transfer or was already taken.
    pub(crate) fn freeze_exec_mailboxes(&self) -> Result<FrozenExecMailboxes> {
        let mut mailboxes = self
            .mailboxes
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if mailboxes.is_none() && self.mailboxes.1.0.get().is_none() {
            return Err(CodexErr::InvalidRequest(
                "controller closed before mail capture".into(),
            ));
        }
        let frozen = self
            .mailboxes
            .1
            .0
            .get_or_init(|| FrozenExecMailboxes(std::sync::Arc::new(Mutex::new(Vec::new()))))
            .clone();
        if let Some(mailboxes) = mailboxes.take() {
            let mut messages = frozen.0.lock().unwrap_or_else(PoisonError::into_inner);
            for (thread, mailbox) in mailboxes {
                mailbox.state.send_replace(false);
                messages.extend(mailbox.pending.into_iter().map(|mail| ExecMailboxMessage {
                    thread,
                    receive_id: mail.id,
                    communication: mail.communication,
                }));
            }
        }
        Ok(frozen)
    }

    /// Eviction transfers mail already accepted by a resident session. Such a
    /// transfer may finish after sealing; retain it until the tree barrier ends.
    /// Ordinary new sends still use enqueue and are refused after sealing.
    pub(crate) fn retain_exec_evicted_mail(
        &self,
        thread: ThreadId,
        messages: Vec<PendingAgentMessage>,
    ) -> Result<()> {
        let mut mailboxes = self
            .mailboxes
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(mailboxes) = mailboxes.as_mut() {
            let mailbox = mailboxes.entry(thread).or_default();
            mailbox
                .pending
                .extend(messages.into_iter().map(|communication| StoredMail {
                    id: None,
                    communication,
                    _diagnostics_guard: PENDING_MAILBOX_MESSAGES.track(),
                }));
            mailbox.state.send_replace(!mailbox.pending.is_empty());
            return Ok(());
        }
        let Some(frozen) = self.mailboxes.1.0.get() else {
            return Err(CodexErr::InvalidRequest(
                "agent runtime is shutting down".into(),
            ));
        };
        frozen
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(
                messages
                    .into_iter()
                    .map(|communication| ExecMailboxMessage {
                        thread,
                        receive_id: None,
                        communication,
                    }),
            );
        Ok(())
    }

    /// Called only after root original-turn admission. Prepend the old queue to
    /// any newer mail without emitting receive/consume events during installation.
    pub(crate) fn restore_exec_mailboxes(&self, messages: Vec<ExecMailboxMessage>) -> Result<()> {
        if messages.is_empty() {
            return Ok(());
        }
        let mut queues = self
            .mailboxes
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let queues = queues.as_mut().ok_or_else(|| {
            CodexErr::InvalidRequest("cannot restore controller mail into a closed runtime".into())
        })?;
        let mut restored: HashMap<ThreadId, Vec<StoredMail>> = HashMap::new();
        for message in messages {
            restored
                .entry(message.thread)
                .or_default()
                .push(StoredMail {
                    id: message.receive_id,
                    communication: message.communication,
                    _diagnostics_guard: PENDING_MAILBOX_MESSAGES.track(),
                });
        }
        for (thread, mut old) in restored {
            let mailbox = queues.entry(thread).or_default();
            old.append(&mut mailbox.pending);
            mailbox.pending = old;
            mailbox.state.send_replace(true);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::control::LocalAgentControl;
    use codex_protocol::AgentPath;
    use codex_protocol::protocol::InterAgentCommunication;

    fn message(text: &str) -> PendingAgentMessage {
        InterAgentCommunication::new(
            AgentPath::root(),
            AgentPath::root(),
            Vec::new(),
            text.into(),
            false,
        )
        .into()
    }

    #[test]
    fn frozen_controller_mail_is_sealed_and_prepended_with_receive_ids() {
        let source = LocalAgentControl::default().runtime;
        let first = ThreadId::new();
        let second = ThreadId::new();
        source
            .mailboxes
            .enqueue(
                first,
                Some("receive-a".into()),
                vec![message("old-a"), message("old-b")],
            )
            .unwrap();
        source
            .mailboxes
            .enqueue(second, None, vec![message("other")])
            .unwrap();
        let pending = source.exec_mailboxes().unwrap();
        assert_eq!(pending.len(), 3);
        let frozen = source.freeze_exec_mailboxes().unwrap();
        source.request_shutdown();
        assert!(
            source
                .mailboxes
                .enqueue(first, None, vec![message("late")])
                .is_err()
        );
        assert!(source.exec_mailboxes().is_err());
        assert_eq!(frozen.snapshot().len(), 3);
        let replacement = LocalAgentControl::default().runtime;
        replacement
            .mailboxes
            .enqueue(first, None, vec![message("new")])
            .unwrap();
        replacement
            .restore_exec_mailboxes(frozen.snapshot())
            .unwrap();
        let restored = replacement.exec_mailboxes().unwrap();
        let same_recipient: Vec<_> = restored
            .iter()
            .filter(|mail| mail.thread == first)
            .collect();
        assert_eq!(same_recipient.len(), 3);
        assert_eq!(same_recipient[0].receive_id.as_deref(), Some("receive-a"));
        assert_eq!(same_recipient[1].receive_id.as_deref(), Some("receive-a"));
        let messages = replacement.mailboxes.take(first);
        assert_eq!(
            messages,
            vec![message("old-a"), message("old-b"), message("new")]
        );
        assert_eq!(replacement.mailboxes.take(second), vec![message("other")]);
        assert!(replacement.mailboxes.take(first).is_empty());
    }

    #[test]
    fn accepted_eviction_transfer_finishes_after_controller_sealing() {
        let runtime = LocalAgentControl::default().runtime;
        let recipient = ThreadId::new();
        let frozen = runtime.freeze_exec_mailboxes().unwrap();
        runtime.request_shutdown();
        assert!(
            runtime
                .mailboxes
                .enqueue(recipient, None, vec![message("new send")])
                .is_err()
        );
        runtime
            .retain_exec_evicted_mail(recipient, vec![message("accepted before eviction")])
            .unwrap();
        let captured = frozen.snapshot();
        assert_eq!(captured.len(), 1);
        assert_eq!(
            captured[0].communication,
            message("accepted before eviction")
        );
        let replacement = LocalAgentControl::default().runtime;
        replacement.restore_exec_mailboxes(captured).unwrap();
        assert_eq!(
            replacement.mailboxes.take(recipient),
            vec![message("accepted before eviction")]
        );
        let ordinary = LocalAgentControl::default().runtime;
        ordinary.request_shutdown();
        assert!(ordinary.freeze_exec_mailboxes().is_err());
        assert!(ordinary.exec_mailboxes().is_err());
        assert!(
            ordinary
                .retain_exec_evicted_mail(recipient, vec![message("not a handoff")])
                .is_err()
        );
    }

    #[test]
    fn enqueue_racing_with_seal_never_loses_an_accepted_message() {
        let source = LocalAgentControl::default().runtime;
        let recipient = ThreadId::new();
        source
            .mailboxes
            .enqueue(recipient, None, vec![message("seed")])
            .unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(9));
        let mut writers = Vec::new();
        for number in 0..8 {
            let runtime = source.clone();
            let barrier = barrier.clone();
            writers.push(std::thread::spawn(move || {
                let id = number.to_string();
                barrier.wait();
                runtime
                    .mailboxes
                    .enqueue(recipient, Some(id.clone()), vec![message(&id)])
                    .ok()
                    .map(|()| id)
            }));
        }
        barrier.wait();
        let frozen = source.freeze_exec_mailboxes().unwrap();
        let accepted: std::collections::HashSet<_> = writers
            .into_iter()
            .filter_map(|writer| writer.join().unwrap())
            .collect();
        let frozen = frozen.snapshot();
        let captured: std::collections::HashSet<_> = frozen
            .iter()
            .filter_map(|mail| mail.receive_id.clone())
            .collect();
        assert_eq!(accepted, captured);
        assert_eq!(frozen.len(), accepted.len() + 1);
    }
}
