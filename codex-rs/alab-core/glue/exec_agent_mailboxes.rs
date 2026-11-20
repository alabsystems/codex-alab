// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Raw queue custody. Encoding shares the root input's publication identity table.
use super::super::LocalAgentRuntime;
use super::*;
use crate::agent_communication::MessageDelivery;

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
    fn append(&self, mailboxes: HashMap<ThreadId, Mailbox>) {
        let mut messages = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        for (thread, mailbox) in mailboxes {
            mailbox.state.send_replace(false);
            messages.extend(mailbox.pending.into_iter().map(|mail| ExecMailboxMessage {
                thread,
                receive_id: mail.id,
                communication: mail.communication,
            }));
        }
    }

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

/// A controller held open through the forest's single native decision.
pub(crate) struct ExecResultController<'a> {
    runtime: &'a LocalAgentRuntime,
    // Detach the open map only while holding its original controller lock.
    controller: std::sync::MutexGuard<'a, Option<HashMap<ThreadId, Mailbox>>>,
    mailboxes: HashMap<ThreadId, Mailbox>,
    restore_on_drop: bool,
}

impl Drop for ExecResultController<'_> {
    fn drop(&mut self) {
        if self.restore_on_drop {
            *self.controller = Some(std::mem::take(&mut self.mailboxes));
        }
    }
}

impl ExecResultController<'_> {
    pub(crate) fn messages(&self) -> impl Iterator<Item = &PendingAgentMessage> {
        self.mailboxes
            .values()
            .flat_map(|mailbox| mailbox.pending.iter().map(|mail| &mail.communication))
    }

    pub(crate) fn result_routes_belong_to(&self, member: impl Fn(ThreadId) -> bool) -> bool {
        self.mailboxes.iter().all(|(id, mailbox)| {
            member(*id)
                || !mailbox
                    .pending
                    .iter()
                    .any(|mail| mail.communication.is_terminal_result())
        })
    }

    pub(crate) fn seal(mut self) {
        let frozen = self.runtime.frozen_exec_mailboxes();
        self.restore_on_drop = false;
        frozen.append(std::mem::take(&mut self.mailboxes));
    }
}

impl LocalAgentRuntime {
    pub(crate) fn exec_result_counter(&self) -> &std::sync::Arc<std::sync::atomic::AtomicUsize> {
        &self.unconfirmed_results
    }

    pub(crate) fn append_exec_terminal_mail(
        &self,
        messages: &mut Vec<ExecMailboxMessage>,
    ) -> Result<()> {
        if messages.is_empty() {
            return Ok(());
        }
        let frozen = self.mailboxes.1.0.get().ok_or_else(|| {
            CodexErr::InvalidRequest("terminal mailbox was not frozen by its source".into())
        })?;
        frozen
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .append(messages);
        Ok(())
    }

    // Consumption transfers the receipt out of its delivery. Only the original
    // runtime can confirm a claimed append, including one with no queue entry.
    pub(crate) fn require_confirmed_exec_results(&self) -> Result<()> {
        self.check_exec_result_writers(false)
    }

    fn check_exec_result_writers(&self, preflight: bool) -> Result<()> {
        if self
            .unconfirmed_results
            .load(std::sync::atomic::Ordering::Acquire)
            != 0
            && !((preflight || self.alab_upgrade.committed())
                && self
                    .exec_result_custody()
                    .is_some_and(|source| source.accounts_for(self)))
        {
            return Err(CodexErr::InvalidRequest(
                "terminal result is awaiting its source writer's confirmation".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn exec_mailboxes(&self) -> Result<Vec<ExecMailboxMessage>> {
        self.check_exec_result_writers(true)?;
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
        self.require_confirmed_exec_results()?;
        let mut mailboxes = self
            .mailboxes
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.freeze_result_controller(&mut mailboxes)
    }

    /// Inspect the original controller entries while every loaded queue is
    /// locked. Seal this controller only after the manager commits the census.
    pub(crate) fn with_exec_result_controller<T>(
        &self,
        seal: bool,
        decide: impl FnOnce(Vec<&PendingAgentMessage>) -> std::result::Result<T, String>,
    ) -> std::result::Result<T, String> {
        let controller = self.lock_exec_result_controller()?;
        let value = decide(controller.messages().collect())?;
        if seal {
            controller.seal();
        }
        Ok(value)
    }

    pub(crate) fn lock_exec_result_controller(
        &self,
    ) -> std::result::Result<ExecResultController<'_>, String> {
        let mut controller = self
            .mailboxes
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mailboxes = controller
            .take()
            .ok_or("queued result controller is already closed")?;
        Ok(ExecResultController {
            runtime: self,
            controller,
            mailboxes,
            restore_on_drop: true,
        })
    }

    fn frozen_exec_mailboxes(&self) -> FrozenExecMailboxes {
        self.mailboxes
            .1
            .0
            .get_or_init(|| FrozenExecMailboxes(std::sync::Arc::new(Mutex::new(Vec::new()))))
            .clone()
    }

    fn freeze_result_controller(
        &self,
        mailboxes: &mut Option<HashMap<ThreadId, Mailbox>>,
    ) -> Result<FrozenExecMailboxes> {
        if mailboxes.is_none() && self.mailboxes.1.0.get().is_none() {
            return Err(CodexErr::InvalidRequest(
                "controller closed before mail capture".into(),
            ));
        }
        let frozen = self.frozen_exec_mailboxes();
        if let Some(mailboxes) = mailboxes.take() {
            frozen.append(mailboxes);
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
    #[cfg(test)]
    pub(crate) fn restore_exec_mailboxes(&self, messages: Vec<ExecMailboxMessage>) -> Result<()> {
        self.restore_exec_accepted_mail(messages, std::iter::empty(), true)
    }

    #[cfg(test)]
    pub(crate) fn restore_exec_mailboxes_quiet(
        &self,
        messages: Vec<ExecMailboxMessage>,
    ) -> Result<()> {
        self.restore_exec_accepted_mail(messages, std::iter::empty(), false)
    }

    pub(crate) fn activate_exec_mailboxes(&self) -> Result<()> {
        let queues = self
            .mailboxes
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let queues = queues
            .as_ref()
            .ok_or_else(|| CodexErr::InvalidRequest("agent runtime is closed".into()))?;
        for mailbox in queues.values() {
            mailbox.state.send_if_modified(|state| {
                let pending = !mailbox.pending.is_empty();
                if *state == pending {
                    false
                } else {
                    *state = pending;
                    true
                }
            });
        }
        Ok(())
    }

    pub(crate) fn restore_exec_accepted_mail<'a>(
        &self,
        messages: Vec<ExecMailboxMessage>,
        accepted: impl Iterator<Item = &'a PendingAgentMessage>,
        notify: bool,
    ) -> Result<()> {
        let queued = accepted
            .filter_map(|mail| mail.delivery.clone())
            .chain(
                messages
                    .iter()
                    .filter_map(|message| message.communication.delivery.clone()),
            )
            .filter(MessageDelivery::is_restored_queued_result)
            .collect::<Vec<_>>();
        if messages.is_empty() && queued.is_empty() {
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
        // A closed controller or a mismatched earlier binding fails before any
        // new writer is counted or any mail can wake its destination.
        MessageDelivery::bind_exec_results(queued, &self.unconfirmed_results)
            .map_err(CodexErr::InvalidRequest)?;
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
            if notify {
                mailbox.state.send_replace(true);
            }
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

    fn decoded_result(sender: ThreadId) -> PendingAgentMessage {
        let publication = crate::agent_communication::ExecPublication::QueuedResult {
            sender,
            scope_id: uuid::Uuid::now_v7(),
        };
        PendingAgentMessage::new(
            (*message("result")).clone(),
            Some(MessageDelivery::restore_exec(publication)),
        )
    }

    #[test]
    fn unsealed_result_controller_restores_mail_after_refusal_and_unwind() {
        let runtime = LocalAgentControl::default().runtime;
        let recipient = ThreadId::new();
        runtime
            .mailboxes
            .enqueue(
                recipient,
                Some("original".into()),
                vec![message("retained")],
            )
            .unwrap();
        let refused = runtime.with_exec_result_controller(true, |messages| {
            assert_eq!(messages, vec![&message("retained")]);
            assert!(runtime.mailboxes.0.try_lock().is_err());
            Err::<(), _>("decision refused".to_owned())
        });
        assert_eq!(refused, Err("decision refused".to_owned()));
        let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let controller = runtime.lock_exec_result_controller().unwrap();
            assert_eq!(controller.messages().count(), 1);
            panic!("interrupted decision");
        }));
        assert!(interrupted.is_err());
        assert!(runtime.mailboxes.1.0.get().is_none());
        let restored = runtime.exec_mailboxes().unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].receive_id.as_deref(), Some("original"));
        assert_eq!(restored[0].communication, message("retained"));
        assert_eq!(
            runtime.mailboxes.take(recipient, |_| true),
            vec![message("retained")]
        );
    }

    #[test]
    fn sealed_result_controller_transfers_original_mail_and_closes_delivery() {
        let runtime = LocalAgentControl::default().runtime;
        let recipient = ThreadId::new();
        runtime
            .mailboxes
            .enqueue(
                recipient,
                Some("original".into()),
                vec![message("retained")],
            )
            .unwrap();
        let watched = runtime.mailboxes.watch(recipient);
        let controller = runtime.lock_exec_result_controller().unwrap();
        assert_eq!(controller.messages().count(), 1);
        assert!(*watched.borrow());
        controller.seal();
        assert!(!*watched.borrow());
        assert!(runtime.lock_exec_result_controller().is_err());
        assert!(
            runtime
                .mailboxes
                .enqueue(recipient, None, vec![message("new")])
                .is_err()
        );
        let frozen = runtime.freeze_exec_mailboxes().unwrap().snapshot();
        assert_eq!(frozen.len(), 1);
        assert_eq!(frozen[0].receive_id.as_deref(), Some("original"));
        assert_eq!(frozen[0].communication, message("retained"));
    }

    #[test]
    fn queued_result_binds_once_across_controller_and_accepted_aliases() {
        use std::sync::atomic::Ordering;
        let runtime = LocalAgentControl::default().runtime;
        let recipient = ThreadId::new();
        let mail = decoded_result(recipient);
        assert_eq!(runtime.unconfirmed_results.load(Ordering::Acquire), 0);
        let controller = ExecMailboxMessage {
            thread: recipient,
            receive_id: None,
            communication: mail.clone(),
        };
        runtime
            .restore_exec_accepted_mail(vec![controller], std::iter::once(&mail), false)
            .unwrap();
        runtime
            .restore_exec_accepted_mail(Vec::new(), std::iter::once(&mail), false)
            .unwrap();
        assert_eq!(runtime.unconfirmed_results.load(Ordering::Acquire), 1);
        let receipt = mail.consumed(recipient).unwrap();
        assert!(mail.consumed(recipient).is_none());
        assert_eq!(runtime.unconfirmed_results.load(Ordering::Acquire), 1);
        receipt.confirm();
        assert_eq!(runtime.unconfirmed_results.load(Ordering::Acquire), 0);
    }

    #[test]
    fn failed_destination_install_does_not_charge_or_split_a_result_writer() {
        use std::sync::atomic::Ordering;
        let first = LocalAgentControl::default().runtime;
        let second = LocalAgentControl::default().runtime;
        let recipient = ThreadId::new();
        let held = decoded_result(recipient);
        let fresh = decoded_result(recipient);
        first
            .restore_exec_accepted_mail(Vec::new(), std::iter::once(&held), false)
            .unwrap();
        assert!(
            second
                .restore_exec_accepted_mail(Vec::new(), [&fresh, &held].into_iter(), false)
                .is_err()
        );
        assert_eq!(second.unconfirmed_results.load(Ordering::Acquire), 0);
        assert!(!fresh.is_terminal_result());
        second.request_shutdown();
        assert!(
            second
                .restore_exec_accepted_mail(Vec::new(), std::iter::once(&fresh), false)
                .is_err()
        );
        assert!(!fresh.is_terminal_result());
        assert_eq!(first.unconfirmed_results.load(Ordering::Acquire), 1);
        held.consumed(recipient).unwrap().confirm();
        assert_eq!(first.unconfirmed_results.load(Ordering::Acquire), 0);
    }

    #[test]
    fn an_already_claimed_terminal_receipt_blocks_mailbox_transfer_until_confirmed() {
        let runtime = LocalAgentControl::default().runtime;
        let sender = ThreadId::new();
        let delivery = crate::agent_communication::MessageDelivery::publish(
            sender,
            &[],
            Some(&runtime.unconfirmed_results),
        )
        .unwrap();
        let write = delivery.consumed(sender).unwrap();
        assert!(runtime.exec_mailboxes().is_err());
        assert!(runtime.freeze_exec_mailboxes().is_err());
        write.confirm();
        assert!(runtime.exec_mailboxes().unwrap().is_empty());
        assert!(
            runtime
                .freeze_exec_mailboxes()
                .unwrap()
                .snapshot()
                .is_empty()
        );
        let ambiguous = crate::agent_communication::MessageDelivery::publish(
            sender,
            &[],
            Some(&runtime.unconfirmed_results),
        )
        .unwrap();
        drop(ambiguous.consumed(sender).unwrap());
        assert!(runtime.require_confirmed_exec_results().is_err());
    }

    #[test]
    fn a_queued_alias_cannot_vouch_for_an_outstanding_writer() {
        use crate::agent_communication::MessageDelivery;
        use std::sync::atomic::Ordering;

        let runtime = LocalAgentControl::default().runtime;
        let writer_thread = ThreadId::new();
        let queued_thread = ThreadId::new();
        let delivery =
            MessageDelivery::publish(writer_thread, &[], Some(&runtime.unconfirmed_results))
                .unwrap();
        let message = PendingAgentMessage::new((*message("terminal")).clone(), Some(delivery));
        runtime
            .mailboxes
            .enqueue(writer_thread, None, vec![message.clone()])
            .unwrap();
        runtime
            .mailboxes
            .enqueue(queued_thread, None, vec![message])
            .unwrap();

        // The writer has already left its queue but has not taken the append receipt.
        let writer = runtime
            .mailboxes
            .take(writer_thread, |_| true)
            .pop()
            .unwrap();
        assert!(writer.is_terminal_result());
        let queues = runtime.mailboxes.0.lock().unwrap();
        let pending = &queues.as_ref().unwrap()[&queued_thread].pending;
        assert_eq!(pending.len(), 1);
        assert!(pending[0].communication.is_terminal_result());
        assert_eq!(runtime.unconfirmed_results.load(Ordering::Acquire), 1);
        drop(queues);
        assert!(runtime.freeze_exec_mailboxes().is_err());

        let append = writer.consumed(writer_thread).unwrap();
        assert!(runtime.freeze_exec_mailboxes().is_err());
        append.confirm();
        let frozen = runtime.freeze_exec_mailboxes().unwrap().snapshot();
        assert_eq!(frozen.len(), 1);
        assert_eq!(frozen[0].thread, queued_thread);
        assert!(!frozen[0].communication.is_terminal_result());
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
        let messages = replacement.mailboxes.take(first, |_| true);
        assert_eq!(
            messages,
            vec![message("old-a"), message("old-b"), message("new")]
        );
        assert_eq!(
            replacement.mailboxes.take(second, |_| true),
            vec![message("other")]
        );
        assert!(replacement.mailboxes.take(first, |_| true).is_empty());
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
            replacement.mailboxes.take(recipient, |_| true),
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

    #[test]
    fn quiet_controller_restore_retains_order_without_publishing_until_activation() {
        let runtime = super::super::super::LocalAgentControl::default().runtime;
        let thread = ThreadId::new();
        let mut watched = runtime.mailboxes.watch(thread);
        let message = |text: &str| ExecMailboxMessage {
            thread,
            receive_id: Some(text.into()),
            communication: PendingAgentMessage::from(
                codex_protocol::protocol::InterAgentCommunication::new(
                    codex_protocol::AgentPath::root(),
                    codex_protocol::AgentPath::root(),
                    Vec::new(),
                    text.into(),
                    false,
                ),
            ),
        };
        runtime
            .restore_exec_mailboxes_quiet(vec![message("retained")])
            .unwrap();
        assert!(!*watched.borrow());
        assert!(!watched.has_changed().unwrap());
        assert_eq!(runtime.exec_mailboxes().unwrap().len(), 1);
        runtime.activate_exec_mailboxes().unwrap();
        assert!(*watched.borrow_and_update());
        runtime.activate_exec_mailboxes().unwrap();
        assert!(
            !watched.has_changed().unwrap(),
            "activation is not a second delivery"
        );
        assert_eq!(
            runtime.exec_mailboxes().unwrap()[0].receive_id.as_deref(),
            Some("retained")
        );
    }
}
