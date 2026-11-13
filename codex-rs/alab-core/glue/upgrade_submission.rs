// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Keep native queue acceptance in the same manager-wide handoff decision as
//! startup. A caller's cancellation cannot retract an already queued operation.

use super::Submission;
use codex_alab_core::upgrade_barrier;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_protocol::protocol::Op;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct Sender {
    channel: async_channel::Sender<Submission>,
    owner: Arc<upgrade_barrier::State>,
}

impl Sender {
    pub(crate) fn new(
        channel: async_channel::Sender<Submission>,
        owner: Arc<upgrade_barrier::State>,
    ) -> Self {
        Self { channel, owner }
    }

    pub(crate) fn redirect(&self, channel: async_channel::Sender<Submission>) -> Self {
        Self::new(channel, Arc::clone(&self.owner))
    }

    pub(crate) async fn send(&self, mut submission: Submission) -> Result<()> {
        // Teardown remains available after a successful handoff. Every other
        // operation, including interrupts and settings, revokes preparation.
        let teardown = matches!(submission.op, Op::Shutdown);
        match self.owner.admit() {
            Ok(admission) => submission.alab_upgrade.push(admission),
            Err(upgrade_barrier::Refusal::Transferred) if teardown => {}
            Err(error) => return Err(CodexErr::InvalidRequest(error.to_string())),
        }
        self.channel
            .send(submission)
            .await
            .map_err(|_| CodexErr::InternalAgentDied)
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.channel.is_empty()
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.channel.is_closed()
    }
}

#[cfg(test)]
impl From<async_channel::Sender<Submission>> for Sender {
    fn from(channel: async_channel::Sender<Submission>) -> Self {
        Self::new(channel, Arc::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn submission(op: Op) -> Submission {
        Submission {
            alab_upgrade: Vec::new(),
            id: "upgrade-queue-test".into(),
            op,
            turn_extension_init: None,
            trace: None,
            parent_turn_id: None,
            root_turn_id: None,
            residency_guard: None,
            message_delivery: None,
        }
    }

    #[tokio::test]
    async fn queue_and_forwarding_hold_admission_until_dispatch_or_discard() {
        let owner = Arc::new(upgrade_barrier::State::default());
        let (tx, rx) = async_channel::bounded(1);
        let sender = Sender::new(tx, Arc::clone(&owner));
        let mut transfer = owner.prepare().unwrap();
        sender.send(submission(Op::Interrupt)).await.unwrap();
        assert!(!transfer.commit_if(|| panic!("new input revoked the transfer")));
        drop(transfer);
        assert!(owner.prepare().is_err());
        let accepted = rx.recv().await.unwrap();
        assert!(
            owner.prepare().is_err(),
            "dequeue is not completed dispatch"
        );
        let (tx, next) = async_channel::bounded(1);
        sender.redirect(tx).send(accepted).await.unwrap();
        assert!(owner.prepare().is_err());
        let dispatched = next.recv().await.unwrap();
        assert_eq!(dispatched.alab_upgrade.len(), 2);
        drop(dispatched);
        assert!(owner.prepare().unwrap().valid());
    }

    #[tokio::test]
    async fn cancellation_and_closed_queues_release_only_their_own_admission() {
        let owner = Arc::new(upgrade_barrier::State::default());
        let (tx, rx) = async_channel::bounded(1);
        let sender = Sender::new(tx, Arc::clone(&owner));
        sender.send(submission(Op::Interrupt)).await.unwrap();
        let mut pending = Box::pin(sender.send(submission(Op::Interrupt)));
        assert!(futures::poll!(pending.as_mut()).is_pending());
        drop(pending);
        assert!(
            owner.prepare().is_err(),
            "first accepted request still owns admission"
        );
        drop(rx.recv().await.unwrap());
        assert!(owner.prepare().unwrap().valid());
        drop(rx);
        assert!(sender.send(submission(Op::Interrupt)).await.is_err());
        assert!(owner.prepare().unwrap().valid());
    }

    #[tokio::test]
    async fn committed_source_refuses_work_but_allows_native_shutdown() {
        let owner = Arc::new(upgrade_barrier::State::default());
        let (tx, rx) = async_channel::bounded(1);
        let sender = Sender::new(tx, Arc::clone(&owner));
        let mut initial = owner.prepare().unwrap();
        sender.send(submission(Op::Shutdown)).await.unwrap();
        assert!(!initial.commit_if(|| panic!("pre-commit shutdown revokes preparation")));
        drop(initial);
        assert!(owner.prepare().is_err());
        drop(rx.recv().await.unwrap());
        let mut transfer = owner.prepare().unwrap();
        assert!(transfer.commit());
        assert!(sender.send(submission(Op::Interrupt)).await.is_err());
        sender.send(submission(Op::Shutdown)).await.unwrap();
        let shutdown = rx.recv().await.unwrap();
        assert!(matches!(shutdown.op, Op::Shutdown));
        assert!(shutdown.alab_upgrade.is_empty());
    }
}
