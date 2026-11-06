// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Quiesce the review consumer before its delegate stops. The snapshot is
//! source data; restoring a task still requires independent native admission.
use crate::CodexThread;
use crate::ExecTreeSeal;
use crate::session::SessionIo;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_alab_core::quota::ExecQuotaMember;
use codex_alab_core::quota::HeadlessExec;
use codex_protocol::ThreadId;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::ReviewOutputEvent;
use serde::Deserialize;
use serde::Serialize;
use std::collections::VecDeque;
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::Weak;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[path = "exec_review_completion.rs"]
mod completion;
#[path = "exec_review_restore.rs"]
mod restore;
pub(crate) use restore::has_prepared;
pub(crate) use restore::recovery_task;
pub(crate) use restore::start_or_resume;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecReviewSnapshot {
    version: u32,
    parent: ThreadId,
    turn: String,
    delegate: ThreadId,
    delegate_turn: String,
    buffered: Option<Event>,
    pending: Vec<Event>,
}

impl ExecReviewSnapshot {
    pub fn delegate(&self) -> ThreadId {
        self.delegate
    }

    pub fn delegate_turn(&self) -> &str {
        &self.delegate_turn
    }

    pub fn validate(&self, parent: ThreadId, turn: &str) -> Result<(), String> {
        if self.version != 1
            || self.parent != parent
            || self.turn != turn
            || self.turn.is_empty()
            || self.delegate == self.parent
            || self.delegate_turn.is_empty()
        {
            return Err("review snapshot changed its original parent or turn".into());
        }
        if self
            .buffered
            .as_ref()
            .is_some_and(|event| !matches!(event.msg, EventMsg::AgentMessage(_)))
        {
            return Err("review snapshot has an invalid buffered message".into());
        }
        for event in &self.pending {
            match &event.msg {
                EventMsg::TurnStarted(start) if start.turn_id != self.delegate_turn => {
                    return Err("review snapshot contains another delegate turn".into());
                }
                EventMsg::TurnComplete(end) if end.turn_id != self.delegate_turn => {
                    return Err("review snapshot contains another completion".into());
                }
                EventMsg::TurnAborted(_) => {
                    return Err("review snapshot contains an aborted delegate".into());
                }
                EventMsg::ShutdownComplete => {
                    return Err("review snapshot retained source shutdown".into());
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Control {
    Running,
    Frozen(u64),
    Finished,
}

#[derive(Default)]
struct Data {
    epoch: u64,
    committed_epoch: Option<u64>,
    delegate: Option<ThreadId>,
    session: Option<Weak<Session>>,
    delegate_turn: Option<String>,
    buffered: Option<Event>,
    pending: VecDeque<Event>,
    restored_io: Option<SessionIo>,
}

struct State {
    parent: ThreadId,
    turn: String,
    cancel: CancellationToken,
    data: Mutex<Data>,
    receiver: OnceLock<async_channel::Receiver<Event>>,
    control: watch::Sender<Control>,
    acknowledged: watch::Sender<u64>,
    drain: tokio::sync::Mutex<()>,
}

struct Handle(Arc<State>);

pub(crate) fn has_consumer(context: &TurnContext) -> bool {
    context.extension_data.get::<Handle>().is_some()
}

pub(crate) fn capture_ready(kind: crate::state::TaskKind, context: &TurnContext) -> bool {
    if kind == crate::state::TaskKind::Regular {
        return true;
    }
    kind == crate::state::TaskKind::Review
        && context.extension_data.get::<Handle>().is_some_and(|handle| {
            matches!(*handle.0.control.borrow(), Control::Frozen(epoch) if *handle.0.acknowledged.borrow() == epoch)
        })
}

pub(crate) fn suspension_ready(kind: crate::state::TaskKind, context: &TurnContext) -> bool {
    kind == crate::state::TaskKind::Regular
        || (kind == crate::state::TaskKind::Review
            && context
                .extension_data
                .get::<Handle>()
                .is_some_and(|handle| handle.0.committed_freeze()))
}

/// Exec handoff owns the delegate's shutdown after commit. Ordinary reviews
/// keep the caller's original cancellation behavior.
fn delegate_cancel(context: &TurnContext, cancel: &CancellationToken) -> CancellationToken {
    context
        .extension_data
        .get::<Handle>()
        .map_or_else(|| cancel.clone(), |handle| handle.0.bridge_cancel().0)
}

pub(crate) struct RunGuard(Arc<State>);
impl Drop for RunGuard {
    fn drop(&mut self) {
        if self.0.committed_freeze() {
            return;
        }
        self.0.complete();
        let io = self
            .0
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .restored_io
            .take();
        if let Some(io) = io {
            tokio::spawn(async move {
                let _ = io.shutdown_and_wait().await;
            });
        }
    }
}

pub(crate) fn begin(
    session: &Arc<Session>,
    context: &Arc<TurnContext>,
    cancel: &CancellationToken,
) -> Option<RunGuard> {
    let data = &session.services.thread_extension_data;
    if data.get::<HeadlessExec>().is_none() && data.get::<ExecQuotaMember>().is_none() {
        return None;
    }
    // Core builds a fresh extension store for a review context. Restore only
    // the exec provenance supplied by its owning thread, not arbitrary data
    // from the preceding turn.
    if data.get::<HeadlessExec>().is_some() {
        context.extension_data.insert(HeadlessExec);
    }
    let state = State::new(session.thread_id, context.sub_id.clone(), cancel.clone());
    context.extension_data.insert(Handle(Arc::clone(&state)));
    Some(RunGuard(state))
}

pub(crate) fn bind_child(
    context: &TurnContext,
    parent: &Session,
    child: &Arc<CodexThread>,
) -> codex_protocol::error::Result<()> {
    let Some(handle) = context.extension_data.get::<Handle>() else {
        return Ok(());
    };
    let state = &handle.0;
    if state.parent != parent.thread_id
        || state.turn != context.sub_id
        || child.session.thread_id == parent.thread_id
        || child.session_source
            != codex_protocol::protocol::SessionSource::Internal(
                codex_protocol::protocol::InternalSessionSource::Review,
            )
        || !child
            .session
            .services
            .local_agent_runtime
            .shares_exec_runtime(&parent.services.local_agent_runtime)
        || !Arc::ptr_eq(
            &child.session.services.auth_manager,
            &parent.services.auth_manager,
        )
    {
        return Err(codex_protocol::error::CodexErr::InvalidRequest(
            "review consumer belongs to another turn".into(),
        ));
    }
    let mut data = state
        .data
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if data.delegate.is_some() {
        return Err(codex_protocol::error::CodexErr::InvalidRequest(
            "review already owns a delegate".into(),
        ));
    }
    data.delegate = Some(child.session.thread_id);
    data.session = Some(Arc::downgrade(&child.session));
    Ok(())
}

pub(crate) async fn process_events(
    session: Arc<Session>,
    context: Arc<TurnContext>,
    receiver: async_channel::Receiver<Event>,
) -> Option<ReviewOutputEvent> {
    let Some(handle) = context.extension_data.get::<Handle>() else {
        return super::process_review_events(session, context, receiver).await;
    };
    Arc::clone(&handle.0)
        .run(receiver, move |message| {
            let session = Arc::clone(&session);
            let context = Arc::clone(&context);
            async move {
                session.send_event(&context, message).await;
            }
        })
        .await
}

impl State {
    fn new(parent: ThreadId, turn: String, cancel: CancellationToken) -> Arc<Self> {
        let (control, _) = watch::channel(Control::Running);
        let (acknowledged, _) = watch::channel(0);
        Arc::new(Self {
            parent,
            turn,
            cancel,
            data: Mutex::new(Data::default()),
            receiver: OnceLock::new(),
            control,
            acknowledged,
            drain: tokio::sync::Mutex::new(()),
        })
    }

    fn committed_freeze(&self) -> bool {
        let control = *self.control.borrow();
        matches!(control, Control::Frozen(epoch)
            if *self.acknowledged.borrow() == epoch
                && self.data.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
                    .committed_epoch == Some(epoch))
    }

    fn bridge_cancel(self: &Arc<Self>) -> (CancellationToken, tokio::task::JoinHandle<()>) {
        let bridge = CancellationToken::new();
        if self.cancel.is_cancelled() {
            bridge.cancel();
        }
        let output = bridge.clone();
        let parent = self.cancel.clone();
        let state = Arc::downgrade(self);
        let mut control = self.control.subscribe();
        // The watcher holds no consumer or session alive. A committed transfer
        // leaves the bridge running until the tree owner shuts the delegate down.
        let watcher = tokio::spawn(async move {
            loop {
                if *control.borrow_and_update() == Control::Finished {
                    break;
                }
                tokio::select! {
                    biased;
                    _ = parent.cancelled() => {
                        if state.upgrade().is_some_and(|state| state.committed_freeze()) {
                            return;
                        }
                        break;
                    }
                    changed = control.changed() => {
                        if changed.is_err() {
                            break;
                        }
                    }
                }
            }
            output.cancel();
        });
        (bridge, watcher)
    }

    fn complete(&self) {
        self.control.send_replace(Control::Finished);
    }

    #[expect(
        clippy::await_holding_invalid_type,
        reason = "serialize frozen receiver reads with snapshot capture; cancellation releases the lock after retaining every received event"
    )]
    async fn retain_frozen_event(&self, receiver: &async_channel::Receiver<Event>) -> bool {
        let _drain = self.drain.lock().await;
        let Ok(event) = receiver.recv().await else {
            return false;
        };
        self.data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pending
            .push_back(event);
        true
    }

    async fn run<F, Fut>(
        self: Arc<Self>,
        receiver: async_channel::Receiver<Event>,
        mut deliver: F,
    ) -> Option<ReviewOutputEvent>
    where
        F: FnMut(EventMsg) -> Fut,
        Fut: Future<Output = ()>,
    {
        if self.receiver.set(receiver.clone()).is_err() {
            self.complete();
            return None;
        }
        let mut control = self.control.subscribe();
        let mut receiver_closed = false;
        loop {
            let mode = *control.borrow_and_update();
            match mode {
                Control::Frozen(epoch) => {
                    // Every prior delivery completed before this acknowledgement.
                    self.acknowledged.send_replace(epoch);
                    tokio::select! {
                        biased;
                        _ = self.cancel.cancelled() => return None,
                        result = control.changed() => { if result.is_err() { return None; } }
                        open = self.retain_frozen_event(&receiver), if !receiver_closed => {
                            receiver_closed = !open;
                        }
                    }
                    continue;
                }
                Control::Finished => return None,
                Control::Running => {}
            }
            let pending = self
                .data
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pending
                .pop_front();
            let event = if let Some(event) = pending {
                event
            } else {
                tokio::select! {
                    biased;
                    result = control.changed() => { if result.is_err() { return None; } continue; }
                    _ = self.cancel.cancelled() => { self.complete(); return None; }
                    event = receiver.recv() => match event {
                        Ok(event) => event,
                        Err(_) => { self.complete(); return None; }
                    }
                }
            };
            match event.clone().msg {
                EventMsg::AgentMessage(_) => {
                    let previous = self
                        .data
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .buffered
                        .replace(event);
                    if let Some(previous) = previous {
                        deliver(previous.msg).await;
                    }
                }
                EventMsg::ItemCompleted(ItemCompletedEvent {
                    item: TurnItem::AgentMessage(_),
                    ..
                })
                | EventMsg::AgentMessageContentDelta(_) => {}
                EventMsg::TurnStarted(ref start) => {
                    {
                        let mut data = self
                            .data
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        if data
                            .delegate_turn
                            .as_ref()
                            .is_some_and(|turn| turn != &start.turn_id)
                        {
                            self.complete();
                            return None;
                        }
                        data.delegate_turn = Some(start.turn_id.clone());
                    }
                    deliver(event.msg).await;
                }
                EventMsg::TurnComplete(done) => {
                    self.complete();
                    if self
                        .data
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .delegate_turn
                        .as_deref()
                        != Some(done.turn_id.as_str())
                    {
                        return None;
                    }
                    return done
                        .last_agent_message
                        .as_deref()
                        .map(super::parse_review_output_event);
                }
                EventMsg::TurnAborted(_) | EventMsg::ShutdownComplete => {
                    self.complete();
                    return None;
                }
                other => deliver(other).await,
            }
        }
    }

    async fn freeze(self: &Arc<Self>) -> Result<ExecFrozenReview, String> {
        if self.cancel.is_cancelled() {
            return Err("review was cancelled".into());
        }
        let epoch = {
            let mut data = self
                .data
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            data.epoch = data
                .epoch
                .checked_add(1)
                .ok_or("review freeze sequence exhausted")?;
            data.epoch
        };
        if !self.control.send_if_modified(|control| {
            if *control != Control::Running {
                return false;
            }
            *control = Control::Frozen(epoch);
            true
        }) {
            return Err("review is already frozen or finished".into());
        }
        let frozen = ExecFrozenReview {
            state: Arc::clone(self),
            epoch,
            committed: false,
        };
        let mut acknowledged = self.acknowledged.subscribe();
        let mut control = self.control.subscribe();
        loop {
            if *control.borrow() != Control::Frozen(epoch) {
                return Err("review finished before freezing".into());
            }
            if *acknowledged.borrow_and_update() == epoch {
                return Ok(frozen);
            }
            tokio::select! {
                result = acknowledged.changed() => result.map_err(|_| "review consumer disappeared")?,
                result = control.changed() => result.map_err(|_| "review consumer disappeared")?,
            }
        }
    }
}

pub struct ExecFrozenReview {
    state: Arc<State>,
    epoch: u64,
    committed: bool,
}

impl Drop for ExecFrozenReview {
    fn drop(&mut self) {
        if !self.committed {
            self.state.control.send_if_modified(|control| {
                if *control != Control::Frozen(self.epoch) {
                    return false;
                }
                *control = Control::Running;
                true
            });
        }
    }
}

impl ExecFrozenReview {
    pub(crate) fn delegate(&self) -> Result<ThreadId, String> {
        self.state
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .delegate
            .ok_or_else(|| "review has no registered delegate".into())
    }
    pub fn commit(&mut self) -> Result<(), String> {
        if self.state.cancel.is_cancelled()
            || *self.state.control.borrow() != Control::Frozen(self.epoch)
            || *self.state.acknowledged.borrow() != self.epoch
        {
            return Err("review is no longer live and frozen".into());
        }
        self.state
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .committed_epoch = Some(self.epoch);
        self.committed = true;
        Ok(())
    }

    pub async fn snapshot(&self, child: &CodexThread) -> Result<ExecReviewSnapshot, String> {
        let source_matches = {
            let data = self
                .state
                .data
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            data.delegate == Some(child.session.thread_id)
                && data
                    .session
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .is_some_and(|session| Arc::ptr_eq(&session, &child.session))
        };
        if !source_matches || child.is_running() {
            return Err("review delegate is not the stopped source runtime".into());
        }
        self.drain_snapshot().await
    }

    #[expect(
        clippy::await_holding_invalid_type,
        reason = "serialize destructive receiver reads until the stopped bridge closes; cancellation releases the drain lock while preserving already-read events"
    )]
    async fn drain_snapshot(&self) -> Result<ExecReviewSnapshot, String> {
        let _drain = self.state.drain.lock().await;
        if *self.state.control.borrow() != Control::Frozen(self.epoch)
            || *self.state.acknowledged.borrow() != self.epoch
        {
            return Err("review consumer is not frozen".into());
        }
        let receiver = self
            .state
            .receiver
            .get()
            .ok_or("review has no event receiver")?;
        // The stopped delegate's bridge closes after forwarding ShutdownComplete.
        // Retain each receive before awaiting again, including if this waiter is cancelled.
        while let Ok(event) = receiver.recv().await {
            self.state
                .data
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pending
                .push_back(event);
        }
        let data = self
            .state
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let pending = data
            .pending
            .iter()
            .filter(|event| !matches!(event.msg, EventMsg::ShutdownComplete))
            .cloned()
            .collect::<Vec<_>>();
        let delegate_turn = data
            .delegate_turn
            .clone()
            .or_else(|| {
                pending.iter().find_map(|event| {
                    if let EventMsg::TurnStarted(start) = &event.msg {
                        Some(start.turn_id.clone())
                    } else {
                        None
                    }
                })
            })
            .ok_or("review has no original delegate turn")?;
        let snapshot = ExecReviewSnapshot {
            version: 1,
            parent: self.state.parent,
            turn: self.state.turn.clone(),
            delegate: data.delegate.ok_or("review has no bound delegate")?,
            delegate_turn,
            buffered: data.buffered.clone(),
            pending,
        };
        snapshot.validate(self.state.parent, &self.state.turn)?;
        Ok(snapshot)
    }
}

impl CodexThread {
    pub async fn freeze_exec_review(
        &self,
        seal: &ExecTreeSeal,
        turn: &str,
    ) -> Result<Option<ExecFrozenReview>, String> {
        if !seal.protects(&self.session.services.local_agent_runtime, false) {
            return Err("review freeze requires its own sealed tree".into());
        }
        let (state, context) = {
            let active = self.session.active_turn.lock().await;
            let task = active
                .as_ref()
                .and_then(|active| active.task.as_ref())
                .ok_or("review parent has no active task")?;
            if task.kind != crate::state::TaskKind::Review {
                return Ok(None);
            }
            if task.turn_context.sub_id != turn {
                return Err("review parent turn changed".into());
            }
            let state = task
                .turn_context
                .extension_data
                .get::<Handle>()
                .map(|handle| Arc::clone(&handle.0))
                .ok_or("review parent has no recoverable consumer")?;
            (state, Arc::clone(&task.turn_context))
        };
        let frozen = state.freeze().await?;
        let active = self.session.active_turn.lock().await;
        if !self.is_running()
            || !active
                .as_ref()
                .and_then(|active| active.task.as_ref())
                .is_some_and(|task| {
                    task.kind == crate::state::TaskKind::Review
                        && Arc::ptr_eq(&task.turn_context, &context)
                })
        {
            return Err("review parent changed while freezing".into());
        }
        Ok(Some(frozen))
    }
}

#[cfg(test)]
#[path = "exec_review_stream_tests.rs"]
mod tests;
