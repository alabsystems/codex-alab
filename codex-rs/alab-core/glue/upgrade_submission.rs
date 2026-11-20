// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Keep native queue acceptance in the same manager-wide handoff decision as
//! startup. A caller's cancellation cannot retract an already queued operation.

use super::Submission;
use codex_alab_core::upgrade_barrier;
use codex_alab_core::upgrade_startup_hold::Fence;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_protocol::protocol::Op;
use std::sync::Arc;

// Both commands start a model request from the root's current settings instead
// of accepting a TurnInput. An explicit review model is applied by its handler.
fn uses_current_model(op: &Op) -> bool {
    matches!(op, Op::Compact | Op::Review { .. })
}

impl super::Session {
    pub(super) async fn alab_prepare_handoff_command(&self, op: &Op) {
        if uses_current_model(op) {
            self.alab_finish_handoff_model_refresh().await;
        }
    }
}

#[derive(Clone)]
pub(crate) struct Sender {
    channel: async_channel::Sender<Submission>,
    owner: Arc<upgrade_barrier::State>,
    startup: Option<Arc<Fence>>,
}

impl Sender {
    pub(crate) fn new(
        channel: async_channel::Sender<Submission>,
        owner: Arc<upgrade_barrier::State>,
        startup: Option<Arc<Fence>>,
    ) -> Self {
        Self {
            channel,
            owner,
            startup,
        }
    }

    pub(crate) fn redirect(&self, channel: async_channel::Sender<Submission>) -> Self {
        Self::new(channel, Arc::clone(&self.owner), self.startup.clone())
    }

    pub(crate) async fn send(&self, mut submission: Submission) -> Result<()> {
        // Queue watchers and goal/agent callers can find a restored Core before
        // the frontend is ready. Wait outside its submission actor, before
        // accepting the input or taking a model/settings snapshot.
        let needs_selection = uses_current_model(&submission.op)
            || matches!(
                submission.op,
                Op::TurnInput { .. }
                    | Op::InterAgentCommunication { .. }
                    | Op::ThreadSettings { .. }
                    | Op::TurnSettings { .. }
            );
        if needs_selection
            && let Some(fence) = &self.startup
            && fence.pending()
        {
            tokio::select! {
                released = fence.wait_for_release() => {
                    if !released {
                        return Err(CodexErr::InternalAgentDied);
                    }
                }
                () = self.channel.closed() => return Err(CodexErr::InternalAgentDied),
            }
        }
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

    pub(crate) async fn send_suspension(
        &self,
        thread: &crate::CodexThread,
        owner: Option<&crate::session::input_queue::exec_input_state::results::CommittedResults>,
        submission: Submission,
    ) -> Result<()> {
        let Some(owner) = owner else {
            return self.send(submission).await;
        };
        if !owner.protects_root(thread)
            || !std::ptr::eq(self, &thread.io.tx_sub)
            || !Arc::ptr_eq(
                &self.owner,
                &thread.session.services.local_agent_runtime.alab_upgrade,
            )
            || !matches!(submission.op, Op::SuspendTurnAndShutdown { .. })
        {
            return Err(CodexErr::InvalidRequest(
                "suspension must belong to the committed original result owner".into(),
            ));
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
        Self::new(channel, Arc::default(), None)
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
    async fn startup_ingress_retains_cancellation_and_rejects_abandoned_native_owners() {
        fn input() -> Submission {
            submission(Op::TurnInput {
                request: Box::new(codex_protocol::turn_input::TurnInputRequest::user_input(
                    vec![],
                )),
                mode: codex_protocol::turn_input::TurnInputMode::StartIfIdle,
                reply: tokio::sync::oneshot::channel().0,
            })
        }
        let owner = codex_alab_core::upgrade_startup_hold::Owner::default();
        let (tx, rx) = async_channel::bounded(2);
        let sender = Sender::new(tx.clone(), Arc::default(), Some(Arc::new(owner.fence())));
        {
            let mut cancelled = Box::pin(sender.send(input()));
            tokio::select! {
                biased;
                result = &mut cancelled => panic!("accepted held input: {result:?}"),
                () = std::future::ready(()) => {}
            }
        }
        let forwarded = sender.redirect(tx);
        let mut waiting = Box::pin(forwarded.send(input()));
        tokio::select! {
            biased;
            result = &mut waiting => panic!("forwarding escaped startup: {result:?}"),
            () = std::future::ready(()) => {}
        }
        sender.send(submission(Op::Shutdown)).await.unwrap();
        assert!(matches!(rx.try_recv().unwrap().op, Op::Shutdown));
        assert!(rx.is_empty());
        drop(owner);
        assert!(waiting.await.is_err());
        assert!(rx.is_empty());
    }

    #[tokio::test]
    async fn queue_and_forwarding_hold_admission_until_dispatch_or_discard() {
        let owner = Arc::new(upgrade_barrier::State::default());
        let (tx, rx) = async_channel::bounded(1);
        let sender = Sender::new(tx, Arc::clone(&owner), None);
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
        let sender = Sender::new(tx, Arc::clone(&owner), None);
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
        let sender = Sender::new(tx, Arc::clone(&owner), None);
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
