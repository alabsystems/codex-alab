// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual original socket reader + SAME connection gate + accepted publication
//! dispatch custody. EOF and callback endpoint facts are insufficient.
use super::*;
use crate::connection_rpc_gate::ConnectionRpcGate;

pub(crate) struct ConnectionCut {
    gate: Arc<ConnectionRpcGate>,
    accepting: StdMutex<bool>,
    closed: tokio_util::sync::CancellationToken,
    ingress: tokio_util::sync::CancellationToken,
    dispatches: tokio_util::task::TaskTracker,
    frames: tokio_util::task::TaskTracker,
}
impl ConnectionCut {
    pub(super) fn new(gate: Arc<ConnectionRpcGate>) -> Self {
        Self {
            gate,
            accepting: StdMutex::new(true),
            closed: tokio_util::sync::CancellationToken::new(),
            ingress: tokio_util::sync::CancellationToken::new(),
            dispatches: tokio_util::task::TaskTracker::new(),
            frames: tokio_util::task::TaskTracker::new(),
        }
    }
    pub(crate) fn is_for_gate(&self, actual: &Arc<ConnectionRpcGate>) -> bool {
        Arc::ptr_eq(&self.gate, actual)
    }
    pub(crate) fn frame(self: &Arc<Self>) -> io::Result<Arc<FrameLease>> {
        let accepting = self
            .accepting
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !*accepting || self.gate.is_closed() {
            return Err(invalid("original input ingress admission closed"));
        }
        Ok(Arc::new(FrameLease {
            _connection: self.clone(),
            token: StdMutex::new(Some(self.frames.token())),
        }))
    }
    /// Actual accepted publication owns this token BEFORE spawn. Dropping the
    /// frontend waiter does not release it; the original worker owns it.
    pub(crate) fn dispatch(&self) -> io::Result<tokio_util::task::task_tracker::TaskTrackerToken> {
        let accepting = self
            .accepting
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !*accepting || self.gate.is_closed() {
            return Err(invalid("original publication dispatch admission closed"));
        }
        Ok(self.dispatches.token())
    }
    fn close(&self) {
        let mut accepting = self
            .accepting
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *accepting = false;
        self.dispatches.close();
        self.frames.close();
        self.closed.cancel();
    }
    /// Only the actual joined transport reader event calls this. It is not
    /// inferred from connection ID absence, endpoint closure, or a timer.
    fn reader_joined(&self) {
        self.ingress.cancel();
    }
    /// No lock crosses await. Original input-frame dispatch and the accepted
    /// spawned publication jobs BOTH finish; unrelated Core/RPC work may live.
    pub(crate) async fn drain(&self) {
        self.closed.cancelled().await;
        self.ingress.cancelled().await;
        self.frames.wait().await;
        self.dispatches.wait().await;
    }
}
/// Travels with SAME original queued request/context. Completion is only the
/// actual registered/spawned owner's custody handoff or last original path drop.
pub(crate) struct FrameLease {
    _connection: Arc<ConnectionCut>,
    token: StdMutex<Option<tokio_util::task::task_tracker::TaskTrackerToken>>,
}
impl FrameLease {
    pub(crate) fn dispatch(&self) -> io::Result<tokio_util::task::task_tracker::TaskTrackerToken> {
        self._connection.dispatch()
    }
    #[cfg(test)]
    pub(crate) fn connection_for_control(&self) -> Arc<ConnectionCut> {
        self._connection.clone()
    }
    pub(crate) fn accepted(&self) {
        let token = self
            .token
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        drop(token);
    }
}
impl OutgoingMessageSender {
    pub(crate) fn alab_callback_connection_closing(
        &self,
        id: ConnectionId,
        actual: &Arc<ConnectionRpcGate>,
    ) {
        let cut = self.alab_callback_outcomes.as_ref().and_then(|ledger| {
            let state = ledger.state.lock().unwrap_or_else(PoisonError::into_inner);
            state
                .ingress
                .get(&id)
                .filter(|cut| Arc::ptr_eq(&cut.gate, actual))
                .cloned()
        });
        if let Some(cut) = cut {
            cut.close();
        }
    }
    pub(crate) fn alab_callback_ingress_drained(&self, id: ConnectionId) {
        let cut = self.alab_callback_outcomes.as_ref().and_then(|ledger| {
            ledger
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .ingress
                .remove(&id)
        });
        if let Some(cut) = cut {
            cut.reader_joined();
        }
    }
}
#[cfg(test)]
#[path = "retained_input_ingress_tests.rs"]
mod tests;
