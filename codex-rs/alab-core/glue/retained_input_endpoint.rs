// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual original private callback cell and original Source send. Task-local
//! expectation only narrows the existing original recipient policy.
use super::*;
use std::result::Result;
const MATCHED: u8 = 1;
const PUBLISHED: u8 = 2;
const RECEIVER_CLOSED: u8 = 3;
struct Dispatch {
    ledger: Arc<Ledger>,
    record: Arc<Record>,
    outcome: AtomicU8,
}
tokio::task_local! { static ACTUAL_DISPATCH: Arc<Dispatch>; }
pub(crate) struct InputCell {
    capture: Arc<Capture>,
    record: Arc<Record>,
}
pub(crate) enum DispatchOutcome {
    Published,
    ReceiverClosed,
    SealedClosed,
    Unknown,
}
impl PreparedCapture {
    pub(crate) fn original_input(&self, id: &RequestId) -> io::Result<InputCell> {
        let record = self
            .capture
            .records
            .iter()
            .find(|record| &record.id == id)
            .cloned()
            .ok_or_else(|| invalid("original input outside actual Capture"))?;
        Ok(InputCell {
            capture: self.capture.clone(),
            record,
        })
    }
}
impl InputCell {
    pub(crate) fn same_cell(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.record, &other.record)
            && Arc::ptr_eq(&self.capture.ledger, &other.capture.ledger)
    }
    pub(crate) fn original_origin(&self) -> &Arc<AcceptedOrigin> {
        &self.capture._original_origin
    }
    pub(crate) async fn drain_original(&self) {
        self.capture.original_connection.drain().await;
    }
    pub(crate) async fn observe(
        &self,
        outgoing: &OutgoingMessageSender,
        backend: &BoundBackend,
        context: &RequestContext,
    ) -> io::Result<()> {
        self.capture.check_sender(outgoing)?;
        let _origin = observe_current_origin(&self.capture.ledger, backend, context).await?;
        Ok(())
    }
    pub(crate) async fn publish(
        &self,
        outgoing: &OutgoingMessageSender,
        recipient: ConnectionId,
        value: serde_json::Value,
    ) -> io::Result<DispatchOutcome> {
        self.capture.check_sender(outgoing)?;
        publish_exact(
            outgoing,
            &self.capture.ledger,
            &self.record,
            recipient,
            value,
        )
        .await
    }
}
/// Shared with lower Source controls: actual callback/send custody only.
async fn publish_exact(
    outgoing: &OutgoingMessageSender,
    ledger: &Arc<Ledger>,
    record: &Arc<Record>,
    recipient: ConnectionId,
    value: serde_json::Value,
) -> io::Result<DispatchOutcome> {
    if !outgoing
        .alab_callback_outcomes
        .as_ref()
        .is_some_and(|actual| Arc::ptr_eq(actual, ledger))
    {
        return Err(invalid("original publication changed actual Source sender"));
    }
    let dispatch = Arc::new(Dispatch {
        ledger: ledger.clone(),
        record: record.clone(),
        outcome: AtomicU8::new(0),
    });
    // SAME original policy, analytics, endpoint lifecycle and callback.send.
    ACTUAL_DISPATCH
        .scope(
            dispatch.clone(),
            outgoing.notify_client_response(recipient, record.id.clone(), value),
        )
        .await;
    match dispatch.outcome.load(Ordering::Acquire) {
        PUBLISHED => Ok(DispatchOutcome::Published),
        RECEIVER_CLOSED => Ok(DispatchOutcome::ReceiverClosed),
        MATCHED => Ok(DispatchOutcome::Unknown),
        _ => {
            let facts = facts_cut(outgoing, std::slice::from_ref(record)).await?;
            if facts.iter().all(|fact| {
                matches!(
                    fact.outcome_data(),
                    Outcome::Response {}
                        | Outcome::ClientError {}
                        | Outcome::Cancelled {}
                        | Outcome::Withdrawn {}
                )
            }) {
                Ok(DispatchOutcome::SealedClosed)
            } else if facts
                .iter()
                .any(|fact| matches!(fact.outcome_data(), Outcome::Unknown {}))
            {
                Ok(DispatchOutcome::Unknown)
            } else {
                Err(invalid(
                    "original publication remains undispatched; actual endpoint/policy is not terminal",
                ))
            }
        }
    }
}
impl OutgoingMessageSender {
    pub(crate) fn alab_original_input_frame(
        &self,
        actual: &Arc<crate::connection_rpc_gate::ConnectionRpcGate>,
    ) -> Option<Arc<ingress::FrameLease>> {
        let ledger = self.alab_callback_outcomes.as_ref()?;
        let cut = {
            let state = ledger.state.lock().unwrap_or_else(PoisonError::into_inner);
            state
                .ingress
                .values()
                .find(|cut| cut.is_for_gate(actual))
                .cloned()
        }?;
        cut.frame().ok()
    }

    pub(in crate::outgoing_message) fn alab_original_input_matches(
        &self,
        entry: &PendingCallbackEntry,
    ) -> bool {
        ACTUAL_DISPATCH
            .try_with(|actual| {
                let same = self
                    .alab_callback_outcomes
                    .as_ref()
                    .is_some_and(|ledger| Arc::ptr_eq(ledger, &actual.ledger))
                    && entry
                        .alab_outcome
                        .as_ref()
                        .is_some_and(|endpoint| Arc::ptr_eq(&endpoint.0, &actual.record));
                if same {
                    actual.outcome.store(MATCHED, Ordering::Release);
                }
                same
            })
            .unwrap_or(true)
    }
    pub(crate) fn alab_original_input_sent<T>(&self, result: &Result<(), T>) {
        let _scope = ACTUAL_DISPATCH.try_with(|actual| {
            if self
                .alab_callback_outcomes
                .as_ref()
                .is_some_and(|ledger| Arc::ptr_eq(ledger, &actual.ledger))
                && actual.outcome.load(Ordering::Acquire) == MATCHED
            {
                actual.outcome.store(
                    if result.is_ok() {
                        PUBLISHED
                    } else {
                        RECEIVER_CLOSED
                    },
                    Ordering::Release,
                );
            }
        });
    }
}

#[cfg(test)]
#[path = "retained_input_endpoint_tests.rs"]
mod tests;
