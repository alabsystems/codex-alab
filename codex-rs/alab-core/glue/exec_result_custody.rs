// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Source-side custody for original queued result receipts until all producers join.
use super::PendingMailboxCommunication;
use super::TurnInput;
use super::TurnInputQueue;
use crate::CodexThread;
use crate::agent::control::ExecMailboxMessage;
use crate::agent::control::LocalAgentRuntime;
use crate::agent_communication::AgentCommunicationContext;
use crate::agent_communication::AgentCommunicationKind;
use crate::agent_communication::ExecResultGrant;
use crate::agent_communication::MessageDelivery;
use crate::state::ActiveTurn;
use crate::state::TurnState;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;
use codex_protocol::protocol::InterAgentCommunication;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::Ordering;

#[path = "exec_forest_results.rs"]
mod forest;
pub(crate) use forest::ForestResults;

type Mailbox = VecDeque<PendingMailboxCommunication>;

struct Held {
    accepting: bool,
    original: HashMap<ThreadId, Mailbox>,
    late: Vec<ExecMailboxMessage>,
    turns: HashMap<ThreadId, (String, TurnInputQueue)>,
}

pub(crate) struct ExecResultCustody {
    grant: Arc<ExecResultGrant>,
    parents: HashMap<ThreadId, ThreadId>,
    held: Mutex<Held>,
}

pub(crate) struct PreparedResults {
    manager: crate::UpgradeManagerBarrier,
    root: ThreadId,
    source: Arc<ExecResultCustody>,
}

pub(crate) struct CommittedResults {
    root: ThreadId,
    inventory: crate::CommittedUpgradeInventory,
    source: Arc<ExecResultCustody>,
}

struct Queues<'a> {
    _active: Vec<tokio::sync::MutexGuard<'a, Option<ActiveTurn>>>,
    turn: Vec<Option<(String, tokio::sync::OwnedMutexGuard<TurnState>)>>,
    mail: Vec<tokio::sync::MutexGuard<'a, Mailbox>>,
}

impl<'a> Queues<'a> {
    fn lock(members: &'a [Arc<CodexThread>]) -> Result<Self, String> {
        let mut active = Vec::with_capacity(members.len());
        let mut turn = Vec::with_capacity(members.len());
        let mut mail = Vec::with_capacity(members.len());
        // Native order is active turn -> turn state -> local mail -> controller.
        // Refuse contention across members rather than waiting on another tree operation.
        for thread in members {
            let guard = thread
                .session
                .active_turn
                .try_lock()
                .map_err(|_| "queued result active turn is busy")?;
            turn.push(match guard.as_ref() {
                Some(active) => {
                    let task = active
                        .task
                        .as_ref()
                        .ok_or("queued result turn is being replaced")?;
                    let state = Arc::clone(&active.turn_state)
                        .try_lock_owned()
                        .map_err(|_| "queued result turn input is busy")?;
                    Some((task.turn_context.sub_id.clone(), state))
                }
                None => None,
            });
            active.push(guard);
            mail.push(
                thread
                    .session
                    .input_queue
                    .mailbox_pending_mails
                    .try_lock()
                    .map_err(|_| "queued result mailbox is busy")?,
            );
        }
        Ok(Self {
            _active: active,
            turn,
            mail,
        })
    }

    fn messages(&self) -> impl Iterator<Item = &crate::PendingAgentMessage> {
        let turn = self.turn.iter().flatten().flat_map(|(_, state)| {
            state
                .pending_input
                .items
                .iter()
                .filter_map(|input| match input {
                    TurnInput::InterAgentCommunication(mail) => Some(mail),
                    _ => None,
                })
        });
        turn.chain(
            self.mail
                .iter()
                .flat_map(|mailbox| mailbox.iter().map(|mail| &mail.communication)),
        )
    }
}

impl PreparedResults {
    pub(crate) async fn prepare(
        root: &CodexThread,
        members: &[Arc<CodexThread>],
    ) -> Result<Option<Self>, String> {
        let runtime = &root.session.services.local_agent_runtime;
        let parents = ExecResultCustody::parents(members);
        if parents.is_empty() && runtime.exec_result_counter().load(Ordering::Acquire) == 0 {
            return Ok(None);
        }
        let manager = root
            .prepare_upgrade_manager()
            .await
            .map_err(|error| error.to_string())?;
        let inventory = manager.threads();
        if inventory.len() != members.len()
            || !members
                .iter()
                .all(|thread| manager.protects_member(thread, false))
        {
            return Err(
                "queued result handoff requires the complete dedicated source manager".into(),
            );
        }
        let source = ExecResultCustody::prepare(runtime, &inventory, parents)?;
        Ok(Some(Self {
            manager,
            root: root.exec_thread_id(),
            source,
        }))
    }

    pub(crate) fn commit(mut self) -> Result<CommittedResults, String> {
        let members = self.manager.threads();
        let root = members
            .iter()
            .find(|thread| thread.exec_thread_id() == self.root)
            .ok_or("queued result source root is missing")?;
        let mut queues = Queues::lock(&members)?;
        let runtime = &members
            .first()
            .ok_or("queued result source manager is empty")?
            .session
            .services
            .local_agent_runtime;
        runtime.with_exec_result_controller(true, |controller| {
            self.source
                .grant
                .recheck(queues.messages().chain(controller))?;
            if !self.manager.commit_if(|| {
                if !runtime.reserve_exec_result_shutdown() {
                    return false;
                }
                self.source.hold(root, &members, &mut queues);
                true
            }) {
                return Err(
                    "queued result handoff was superseded before stopping its source".into(),
                );
            }
            Ok(())
        })?;
        Ok(CommittedResults {
            root: self.root,
            inventory: self
                .manager
                .into_committed()
                .map_err(|error| error.to_string())?,
            source: self.source,
        })
    }
}

impl ExecResultCustody {
    fn parents(members: &[Arc<CodexThread>]) -> HashMap<ThreadId, ThreadId> {
        members
            .iter()
            .filter_map(|member| match member.session_source() {
                SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
                    parent_thread_id,
                    agent_path: Some(_),
                    ..
                }) => Some((member.exec_thread_id(), *parent_thread_id)),
                _ => None,
            })
            .collect()
    }

    fn prepare(
        runtime: &LocalAgentRuntime,
        members: &[Arc<CodexThread>],
        parents: HashMap<ThreadId, ThreadId>,
    ) -> Result<Arc<Self>, String> {
        let queues = Queues::lock(members)?;
        let grant = runtime.with_exec_result_controller(false, |controller| {
            ExecResultGrant::prepare(
                runtime.exec_result_counter(),
                queues.messages().chain(controller),
            )
        })?;
        let source = Arc::new(ExecResultCustody {
            grant,
            parents,
            held: Mutex::new(Held {
                accepting: false,
                original: HashMap::new(),
                late: Vec::new(),
                turns: HashMap::new(),
            }),
        });
        runtime.set_exec_result_custody(&source);
        Ok(source)
    }

    fn hold(
        self: &Arc<Self>,
        root: &CodexThread,
        members: &[Arc<CodexThread>],
        queues: &mut Queues<'_>,
    ) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        for ((member, mailbox), active) in
            members.iter().zip(&mut queues.mail).zip(&mut queues.turn)
        {
            let id = member.exec_thread_id();
            held.original.insert(id, std::mem::take(&mut **mailbox));
            if let Some((turn, state)) = active
                && !state.pending_input.items.is_empty()
            {
                held.turns
                    .insert(id, (turn.clone(), std::mem::take(&mut state.pending_input)));
            }
        }
        held.accepting = true;
        // A failure after stopping the source must keep its original queues reachable.
        root.thread_extension_data().insert_arc(Arc::clone(self));
    }

    pub(super) fn restore_turn(&self, id: ThreadId, turn: &str, queue: &mut TurnInputQueue) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        if held
            .turns
            .get(&id)
            .is_some_and(|(original_turn, _)| original_turn == turn)
            && let Some((_, mut original)) = held.turns.remove(&id)
        {
            original.items.append(&mut queue.items);
            original.notified_activity = original.notified_activity.max(queue.notified_activity);
            *queue = original;
        }
    }

    pub(crate) fn accounts_for(&self, runtime: &LocalAgentRuntime) -> bool {
        let _held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        self.grant.accounts_for(runtime.exec_result_counter())
    }

    pub(crate) fn accept_terminal(
        &self,
        runtime: &LocalAgentRuntime,
        recipient: ThreadId,
        context: &AgentCommunicationContext,
        communication: InterAgentCommunication,
        has_observers: bool,
        publish: impl FnOnce() -> Option<MessageDelivery>,
    ) -> Result<String, CodexErr> {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        if !held.accepting
            || has_observers
            || communication.trigger_turn
            || context.kind != AgentCommunicationKind::Result
            || self.parents.get(&context.sender_thread_id) != Some(&recipient)
            || !self.grant.accounts_for(runtime.exec_result_counter())
        {
            return Err(CodexErr::InvalidRequest(
                "terminal publication is outside the committed source queue".into(),
            ));
        }
        let id = uuid::Uuid::now_v7().to_string();
        let delivery = self
            .grant
            .publish_late(publish)
            .map_err(CodexErr::InvalidRequest)?;
        held.late.push(ExecMailboxMessage {
            thread: recipient,
            receive_id: Some(id.clone()),
            communication: crate::PendingAgentMessage::new(communication, Some(delivery)),
        });
        Ok(id)
    }
}

impl LocalAgentRuntime {
    pub(crate) fn retain_exec_terminal(
        &self,
        refusal: CodexErr,
        state: &crate::thread_manager::ThreadManagerState,
        recipient: ThreadId,
        communication: InterAgentCommunication,
        context: &AgentCommunicationContext,
    ) -> Result<String, CodexErr> {
        let source = self.exec_result_custody().ok_or(refusal)?;
        let log_content = crate::agent_communication::send_log_content(&communication);
        let observers = state.extensions.message_lifecycle_contributors();
        let id = source.accept_terminal(
            self,
            recipient,
            context,
            communication,
            !observers.is_empty(),
            || {
                MessageDelivery::publish(
                    context.sender_thread_id,
                    observers,
                    Some(self.exec_result_counter()),
                )
            },
        )?;
        crate::agent_communication::emit_agent_communication_send(
            &id,
            context,
            log_content.as_deref(),
            recipient,
        );
        Ok(id)
    }
}

impl CommittedResults {
    pub(crate) fn protects_root(&self, root: &CodexThread) -> bool {
        self.root == root.exec_thread_id() && self.inventory.protects_member(root)
    }

    pub(crate) fn captured(&self, root: &CodexThread) {
        root.thread_extension_data().remove::<ExecResultCustody>();
    }

    pub(crate) async fn finish(&self, root: &CodexThread) -> Result<(), String> {
        self.source.finish(root, self.inventory.threads()).await
    }
}

impl ExecResultCustody {
    async fn finish(&self, root: &CodexThread, members: &[Arc<CodexThread>]) -> Result<(), String> {
        let runtime = &root.session.services.local_agent_runtime;
        let mut mailboxes = Vec::with_capacity(members.len());
        for member in members {
            mailboxes.push(
                member
                    .session
                    .input_queue
                    .mailbox_pending_mails
                    .lock()
                    .await,
            );
        }
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.accepting = false;
        if !held.turns.is_empty() || !self.grant.accounts_for(runtime.exec_result_counter()) {
            return Err("terminal source writer ownership changed after producer shutdown".into());
        }
        runtime
            .append_exec_terminal_mail(&mut held.late)
            .map_err(|error| error.to_string())?;
        for (member, mut mailbox) in members.iter().zip(mailboxes.drain(..)) {
            if let Some(mut original) = held.original.remove(&member.exec_thread_id()) {
                original.append(&mut *mailbox);
                *mailbox = original;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) async fn original_queued_result(
    thread: &Arc<CodexThread>,
) -> Option<(serde_json::Value, ThreadId, String)> {
    let threads = [Arc::clone(thread)];
    let queues = Queues::lock(&threads).ok()?;
    let snapshot = |mail: &crate::PendingAgentMessage| {
        let (sender, scope) = mail.delivery.as_ref()?.original_result_identity()?;
        Some((serde_json::to_value(mail).unwrap(), sender, scope))
    };
    let local = queues.messages().find_map(snapshot);
    local.or_else(|| {
        thread
            .session
            .services
            .local_agent_runtime
            .with_exec_result_controller(false, |controller| {
                Ok(controller.into_iter().find_map(snapshot))
            })
            .ok()
            .flatten()
    })
}
