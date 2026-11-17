// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Test-only tap of an ACTUAL registered MessageProcessor context. The original
//! request body remains unpolled; the control must abort/join that outer task.
use super::ConnectionRequestId;
use super::OutgoingMessageSender;
use super::RequestContext;
use std::io;
use std::sync::Arc;
use std::sync::PoisonError;
use std::sync::Weak;
use tokio::sync::watch;

pub(crate) struct Gate {
    context: watch::Sender<Option<RequestContext>>,
}
pub(crate) struct Capture {
    outgoing: Weak<OutgoingMessageSender>,
    gate: Arc<Gate>,
    seen: watch::Receiver<Option<RequestContext>>,
}
impl Capture {
    pub(crate) fn arm(outgoing: &Arc<OutgoingMessageSender>) -> io::Result<Self> {
        let (context, seen) = watch::channel(None);
        let gate = Arc::new(Gate { context });
        let mut slot = outgoing
            .alab_selection_context_capture
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if slot.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "actual context tap already armed",
            ));
        }
        *slot = Some(gate.clone());
        Ok(Self {
            outgoing: Arc::downgrade(outgoing),
            gate,
            seen,
        })
    }
    pub(crate) async fn next(&mut self) -> io::Result<RequestContext> {
        loop {
            let context = self.seen.borrow_and_update().clone();
            if let Some(context) = context {
                return Ok(context);
            }
            self.seen
                .changed()
                .await
                .map_err(|_| io::Error::other("actual context tap ended"))?;
        }
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        if let Some(outgoing) = self.outgoing.upgrade() {
            let mut slot = outgoing
                .alab_selection_context_capture
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if slot
                .as_ref()
                .is_some_and(|actual| Arc::ptr_eq(actual, &self.gate))
            {
                *slot = None;
            }
        }
    }
}
pub(crate) async fn registered(outgoing: &OutgoingMessageSender, actual: &RequestContext) {
    let gate = outgoing
        .alab_selection_context_capture
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    if let Some(gate) = gate {
        let _previous = gate.context.send_replace(Some(actual.clone()));
        std::future::pending::<()>().await;
    }
}
pub(crate) async fn remains_registered(
    outgoing: &OutgoingMessageSender,
    request: &ConnectionRequestId,
) -> bool {
    outgoing.request_contexts.lock().await.contains_key(request)
}
