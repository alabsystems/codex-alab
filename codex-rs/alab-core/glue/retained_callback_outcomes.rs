// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Source callback-endpoint facts only. Closed means the original endpoint was
//! actually removed through its response/error/cancel lifecycle. It says nothing
//! about Core consuming the result, Native effects, Go, or UI application.
//! Only the real retained Incoming constructor enables this ledger.
use super::*;
use codex_alab_aterm::frontend_control::retained_service::AcceptedOrigin;
use codex_alab_aterm::frontend_control::retained_service::BoundBackend;
use serde::Serialize;
use std::io;
use std::result::Result;
use std::sync::Mutex as StdMutex;
use std::sync::PoisonError;
use std::sync::Weak;
use std::sync::atomic::AtomicU8;

const PENDING: u8 = 0;
const RESPONSE: u8 = 1;
const CLIENT_ERROR: u8 = 2;
const CANCELLED: u8 = 3;
const WITHDRAWN: u8 = 4;
const UNKNOWN: u8 = 5;

#[derive(Clone, Copy)]
pub(super) enum Closed {
    Response,
    ClientError,
    Cancelled,
    Withdrawn,
}
impl Closed {
    fn value(self) -> u8 {
        match self {
            Self::Response => RESPONSE,
            Self::ClientError => CLIENT_ERROR,
            Self::Cancelled => CANCELLED,
            Self::Withdrawn => WITHDRAWN,
        }
    }
}
struct Record {
    id: RequestId,
    thread: Option<ThreadId>,
    state: AtomicU8,
    changed: tokio::sync::watch::Sender<u8>,
}

/// Held by the actual PendingCallbackEntry, never deserialized or recreated by
/// a requester. An unmarked entry destruction remains Unknown, not terminal.
pub(super) struct Endpoint(Arc<Record>);
impl Endpoint {
    pub(super) fn close(&self, why: Closed) {
        if self
            .0
            .state
            .compare_exchange(PENDING, why.value(), Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.0.changed.send_replace(why.value());
        }
    }
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        if self
            .0
            .state
            .compare_exchange(PENDING, UNKNOWN, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.0.changed.send_replace(UNKNOWN);
        }
    }
}

#[derive(Default)]
struct State {
    records: HashMap<RequestId, Weak<Record>>,
    connections: HashMap<ConnectionId, Arc<crate::connection_rpc_gate::ConnectionRpcGate>>,
    ingress: HashMap<ConnectionId, Arc<ingress::ConnectionCut>>,
    observers: HashMap<ConnectionId, Vec<Weak<ObserverStop>>>,
    faulted: bool,
    reserved: usize, // Capacity only; never a callback/native permission.
    // Terminal cells live until actual UI-applied resolution acknowledgement,
    // source connection closure, or a captured owner taking their real custody.
    recipients: HashMap<ConnectionId, HashMap<RequestId, Arc<Record>>>,
}
pub(super) struct Ledger {
    state: StdMutex<State>,
    limits: Limits,
    // None exists only in lower private Source tests. The actual constructor
    // retains SAME Incoming; no label/boolean can select a backend authority.
    input: Option<codex_alab_core::native_incoming::Incoming>,
}

fn tracked(request: &ServerRequest) -> bool {
    matches!(
        request,
        ServerRequest::CommandExecutionRequestApproval { .. }
            | ServerRequest::FileChangeRequestApproval { .. }
            | ServerRequest::PermissionsRequestApproval { .. }
            | ServerRequest::ToolRequestUserInput { .. }
    ) || matches!(request, ServerRequest::McpServerElicitationRequest { params, .. }
            if !matches!(&params.request, codex_app_server_protocol::McpServerElicitationRequest::UserVerification { .. }))
}

#[derive(Clone, Copy)]
struct Limits {
    records: usize,
    associations: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            records: 4096,
            associations: 8192,
        }
    }
}
impl Default for Ledger {
    fn default() -> Self {
        Self {
            state: StdMutex::new(State::default()),
            limits: Limits::default(),
            input: None,
        }
    }
}
impl Ledger {
    pub(super) fn for_input(input: codex_alab_core::native_incoming::Incoming) -> Self {
        Self {
            input: Some(input),
            ..Self::default()
        }
    }
    fn belongs_to(&self, backend: &BoundBackend) -> bool {
        self.input
            .as_ref()
            .is_some_and(|input| *input == backend.incoming())
    }
    /// Called under the original callback-map lock, after original registration
    /// admission. Recipients are actual original sends, not client-supplied IDs.
    pub(super) fn register(
        &self,
        request: &ServerRequest,
        thread: Option<ThreadId>,
        recipients: Option<&[ConnectionId]>,
    ) -> io::Result<Option<Endpoint>> {
        if !tracked(request) {
            return Ok(None);
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.records.retain(|_, record| record.strong_count() != 0);
        let recipients: HashSet<_> = recipients
            .unwrap_or_default()
            .iter()
            .copied()
            .filter(|recipient| {
                state
                    .connections
                    .get(recipient)
                    .is_some_and(|actual| !actual.is_closed())
            })
            .collect();
        // A closed destination acquires no new association. The actual endpoint
        // remains Pending/owned so accepted no-recipient work can replay later.
        if state.faulted
            || state.records.contains_key(request.id())
            || state.records.len() >= self.limits.records
            || association_count(&state)
                .saturating_add(state.reserved)
                .saturating_add(recipients.len())
                > self.limits.associations
        {
            return Err(capacity());
        }
        let record = Arc::new(Record {
            id: request.id().clone(),
            thread,
            state: AtomicU8::new(PENDING),
            changed: tokio::sync::watch::channel(PENDING).0,
        });
        state
            .records
            .insert(record.id.clone(), Arc::downgrade(&record));
        for recipient in recipients {
            state
                .recipients
                .entry(recipient)
                .or_default()
                .insert(record.id.clone(), record.clone());
        }
        Ok(Some(Endpoint(record)))
    }
    /// Called under the ORIGINAL callback-map guard. These are the actual
    /// PendingCallbackEntry endpoints, not a later ID lookup after EOF.
    pub(super) fn snapshot(
        self: &Arc<Self>,
        callbacks: &HashMap<RequestId, PendingCallbackEntry>,
        requests: &[ServerRequest],
    ) -> Snapshot {
        let records = requests
            .iter()
            .filter(|request| tracked(request))
            .map(|request| {
                callbacks
                    .get(request.id())
                    .and_then(|entry| entry.alab_outcome.as_ref())
                    .map(|endpoint| endpoint.0.clone())
                    .ok_or("actual tracked replay endpoint missing at original map cut")
            })
            .collect();
        Snapshot {
            ledger: self.clone(),
            records,
        }
    }
    pub(super) fn reserve(
        self: &Arc<Self>,
        snapshot: &Snapshot,
        requests: &[ServerRequest],
        recipient: ConnectionId,
        human: bool,
    ) -> io::Result<ReplayReservation> {
        if !Arc::ptr_eq(self, &snapshot.ledger) {
            return Err(invalid("replay snapshot changed actual Source ledger"));
        }
        let records = snapshot
            .records
            .as_ref()
            .map_err(|message| invalid(message))?;
        let records: Vec<_> = records
            .iter()
            .filter(|record| {
                requests.iter().any(|request| {
                    request.id() == &record.id && (!requires_human_approval(request) || human)
                })
            })
            .cloned()
            .collect();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let connection = state
            .connections
            .get(&recipient)
            .filter(|actual| !actual.is_closed())
            .cloned()
            .ok_or_else(|| invalid("actual replay connection admission closed"))?;
        let mut needed = 0usize;
        for record in &records {
            match state
                .recipients
                .get(&recipient)
                .and_then(|entries| entries.get(&record.id))
            {
                Some(actual) if !Arc::ptr_eq(actual, record) => {
                    return Err(invalid("replay callback identity changed"));
                }
                Some(_) => {}
                None => needed += 1,
            }
        }
        if state.faulted
            || association_count(&state)
                .saturating_add(state.reserved)
                .saturating_add(needed)
                > self.limits.associations
        {
            return Err(capacity());
        }
        state.reserved += needed;
        Ok(ReplayReservation {
            ledger: self.clone(),
            recipient,
            connection,
            records,
            remaining: std::sync::atomic::AtomicUsize::new(needed),
        })
    }
    pub(super) fn connection_closed(&self, recipient: ConnectionId) {
        let (connection, retired, observers) = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            (
                state.connections.remove(&recipient),
                state.recipients.remove(&recipient),
                state.observers.remove(&recipient).unwrap_or_default(),
            )
        };
        for observer in observers
            .into_iter()
            .filter_map(|observer| observer.upgrade())
        {
            observer.closed.cancel();
        }
        drop(retired);
        drop(connection); // Only readonly observers stop; callback/Native/Source work stays owned.
    }
    #[cfg(test)]
    fn capture(&self, recipient: ConnectionId, ids: &[RequestId]) -> io::Result<Vec<Arc<Record>>> {
        self.capture_with_connection(recipient, ids)
            .map(|(records, _)| records)
    }
    fn capture_with_connection(
        &self,
        recipient: ConnectionId,
        ids: &[RequestId],
    ) -> io::Result<(Vec<Arc<Record>>, Arc<ingress::ConnectionCut>)> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if !state
            .connections
            .get(&recipient)
            .is_some_and(|actual| !actual.is_closed())
        {
            return Err(invalid("actual callback recipient admission closed"));
        }
        let mut unique = HashSet::new();
        let mut records = Vec::with_capacity(ids.len());
        for id in ids {
            if !unique.insert(id) {
                return Err(invalid("duplicate captured callback"));
            }
            let record = state
                .recipients
                .get(&recipient)
                .and_then(|records| records.get(id))
                .cloned()
                .ok_or_else(|| {
                    invalid("captured callback has no actual original recipient record")
                })?;
            records.push(record);
        }
        let connection = state
            .ingress
            .get(&recipient)
            .cloned()
            .ok_or_else(|| invalid("actual original ingress owner absent"))?;
        Ok((records, connection))
    }
    fn acknowledge(&self, recipient: ConnectionId, ids: &[RequestId]) -> io::Result<()> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let records = state
            .recipients
            .get_mut(&recipient)
            .ok_or_else(|| invalid("callback recipient is gone"))?;
        // Validate the entire semantic acknowledgement before removing any cell.
        let mut unique = HashSet::new();
        for id in ids {
            if !unique.insert(id)
                || !records.get(id).is_some_and(|record| {
                    matches!(
                        record.state.load(Ordering::Acquire),
                        RESPONSE | CLIENT_ERROR | CANCELLED | WITHDRAWN
                    )
                })
            {
                return Err(invalid(
                    "callback UI acknowledgement lacks actual endpoint closure",
                ));
            }
        }
        let retired: Vec<_> = ids.iter().filter_map(|id| records.remove(id)).collect();
        state.recipients.retain(|_, entries| !entries.is_empty());
        drop(state);
        drop(retired);
        Ok(())
    }
    fn acknowledge_captured(
        &self,
        recipient: ConnectionId,
        captured: &[Arc<Record>],
        ids: &[RequestId],
    ) -> io::Result<()> {
        let mut unique = HashSet::new();
        let mut selected = Vec::with_capacity(ids.len());
        for id in ids {
            if !unique.insert(id) {
                return Err(invalid("duplicate captured callback acknowledgement"));
            }
            let record = captured
                .iter()
                .find(|record| &record.id == id)
                .ok_or_else(|| {
                    invalid("callback acknowledgement is outside exact accepted Capture")
                })?;
            if !matches!(
                record.state.load(Ordering::Acquire),
                RESPONSE | CLIENT_ERROR | CANCELLED | WITHDRAWN
            ) {
                return Err(invalid("captured callback has no actual closed endpoint"));
            }
            selected.push(record);
        }
        let retired = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(records) = state.recipients.get_mut(&recipient) else {
                return Ok(());
            };
            // C2 may never have received a callback closed during the C1 gap.
            // An absent bucket is not closure evidence: selected exact cells
            // above already supplied closure. Never synthesize closed replay.
            let mut retired = Vec::new();
            for record in selected {
                if records
                    .get(&record.id)
                    .is_some_and(|actual| Arc::ptr_eq(actual, record))
                    && let Some(actual) = records.remove(&record.id)
                {
                    retired.push(actual);
                }
            }
            state.recipients.retain(|_, entries| !entries.is_empty());
            retired
        };
        drop(retired);
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub(crate) enum Outcome {
    Pending {},
    Response {},
    ClientError {},
    Cancelled {},
    Withdrawn {},
    Unknown {},
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Fact {
    request_id: RequestId,
    thread_id: Option<ThreadId>,
    outcome: Outcome,
}
impl Fact {
    pub(crate) fn request_id_data(&self) -> &RequestId {
        &self.request_id
    }
    pub(crate) fn thread_id_data(&self) -> Option<ThreadId> {
        self.thread_id
    }
    pub(crate) fn outcome_data(&self) -> &Outcome {
        &self.outcome
    }
}
impl Record {
    fn fact(&self) -> Fact {
        Fact {
            request_id: self.id.clone(),
            thread_id: self.thread,
            outcome: match self.state.load(Ordering::Acquire) {
                PENDING => Outcome::Pending {},
                RESPONSE => Outcome::Response {},
                CLIENT_ERROR => Outcome::ClientError {},
                CANCELLED => Outcome::Cancelled {},
                WITHDRAWN => Outcome::Withdrawn {},
                _ => Outcome::Unknown {},
            },
        }
    }
}

/// Actual Source endpoint cells + original physical A custody. IDs/tokens on
/// the wire may select this owner, but cannot construct it or any Native grant.
pub(crate) struct Capture {
    ledger: Arc<Ledger>,
    records: Vec<Arc<Record>>,
    _original_origin: Arc<AcceptedOrigin>,
    original_connection: Arc<ingress::ConnectionCut>,
}

/// Synchronous exact-cell custody, prepared by the actual accepted Source
/// ticket before its first awaited Native observation. This is not admission,
/// publication, endpoint completion, or a Native grant. If C1 EOF wins before
/// preparation, preparation refuses; after preparation its cells cannot vanish.
pub(crate) struct PreparedCapture {
    capture: Arc<Capture>,
    cancellation: tokio_util::sync::CancellationToken,
}
impl PreparedCapture {
    /// Every observation borrows the SAME prepared Capture. No post-EOF lookup
    /// or resubmission can substitute a different recipient/cell set.
    pub(crate) async fn observe(
        &self,
        actual: &OutgoingMessageSender,
        backend: &BoundBackend,
    ) -> io::Result<Arc<Capture>> {
        self.capture.check_sender(actual)?;
        if !self.capture.ledger.belongs_to(backend) {
            return Err(invalid(
                "prepared callback capture changed actual bound Incoming",
            ));
        }
        if self.cancellation.is_cancelled() {
            return Err(invalid("callback capture observation cancelled"));
        }
        let origin = &self.capture._original_origin;
        let current = backend.observe_reader_origin(origin).await?;
        if !current.same_receipt(origin) {
            return Err(invalid("callback capture changed actual bound A receipt"));
        }
        if self.cancellation.is_cancelled() {
            return Err(invalid("callback capture observation cancelled"));
        }
        Ok(self.capture.clone())
    }
    /// A genuine later ticket observer supplies its NEW actual request context.
    /// Old canceled C1 observation cannot choose the new origin or erase cells.
    pub(crate) async fn observe_current_for(
        &self,
        actual: &OutgoingMessageSender,
        backend: &BoundBackend,
        current: &RequestContext,
    ) -> io::Result<Arc<Capture>> {
        self.capture.check_sender(actual)?;
        let _origin = observe_current_origin(&self.capture.ledger, backend, current).await?;
        Ok(self.capture.clone())
    }
}
impl Capture {
    fn check_sender(&self, actual: &OutgoingMessageSender) -> io::Result<()> {
        if !actual
            .alab_callback_outcomes
            .as_ref()
            .is_some_and(|ledger| Arc::ptr_eq(ledger, &self.ledger))
        {
            return Err(invalid(
                "callback capture belongs to another actual Source sender",
            ));
        }
        Ok(())
    }
    pub(crate) async fn facts_for(&self, actual: &OutgoingMessageSender) -> io::Result<Vec<Fact>> {
        self.check_sender(actual)?;
        facts_cut(actual, &self.records).await
    }
    /// Arm receivers BEFORE taking the ordered facts cut; changes after a
    /// successful pending replay must remain observable through the same owner.
    /// The protected projection consumer owns actual delivery/currentness.
    pub(crate) fn watch_for(&self, actual: &OutgoingMessageSender) -> io::Result<Vec<Watch>> {
        self.check_sender(actual)?;
        Ok(self
            .records
            .iter()
            .map(|record| Watch {
                record: record.clone(),
                receiver: record.changed.subscribe(),
            })
            .collect())
    }
    /// Paired ticket owner invokes only after SAME UI applied/reconciled these
    /// exact callback views. This validates actual current Source request/A and
    /// OWN closed cells, not a fabricated replay recipient or IDs alone. Ticket
    /// owner retains/drops its final Capture custody separately after real ACK.
    pub(crate) async fn acknowledge_for(
        &self,
        actual: &OutgoingMessageSender,
        backend: &BoundBackend,
        current: &RequestContext,
        ids: &[RequestId],
    ) -> io::Result<()> {
        self.check_sender(actual)?;
        let _origin = observe_current_origin(&self.ledger, backend, current).await?;
        self.ledger.acknowledge_captured(
            current.alab_request_id().connection_id,
            &self.records,
            ids,
        )
    }
}

async fn observe_current_origin(
    ledger: &Ledger,
    backend: &BoundBackend,
    actual: &RequestContext,
) -> io::Result<Arc<AcceptedOrigin>> {
    if !ledger.belongs_to(backend) {
        return Err(invalid(
            "callback observation changed actual bound Incoming",
        ));
    }
    let origin = actual
        .alab_connection_resource
        .as_ref()
        .and_then(
            codex_alab_core::connection_resource::ConnectionResource::downcast::<AcceptedOrigin>,
        )
        .ok_or_else(|| invalid("callback observation requires actual physical A resource"))?;
    if actual.cancellation.is_cancelled() {
        return Err(invalid("callback observation cancelled"));
    }
    let observed = backend.observe_reader_origin(&origin).await?;
    if !observed.same_receipt(&origin) {
        return Err(invalid(
            "callback observation changed actual bound A receipt",
        ));
    }
    if actual.cancellation.is_cancelled() {
        return Err(invalid("callback observation cancelled"));
    }
    Ok(origin)
}
async fn facts_cut(
    actual: &OutgoingMessageSender,
    records: &[Arc<Record>],
) -> io::Result<Vec<Fact>> {
    // Arm before the first map cut. Removal and the original caller's
    // terminal hook are distinct instructions; wait that real endpoint's
    // close/drop rather than mislabel the short removed-but-owned interval.
    let mut transitions: Vec<_> = records
        .iter()
        .map(|record| record.changed.subscribe())
        .collect();
    loop {
        let (facts, transitioning) = {
            let callbacks = actual.request_id_to_callback.lock().await;
            let mut facts = Vec::with_capacity(records.len());
            let mut transitioning = None;
            for (index, record) in records.iter().enumerate() {
                let fact = record.fact();
                if matches!(fact.outcome, Outcome::Pending {})
                    && !callbacks
                        .get(&record.id)
                        .and_then(|entry| entry.alab_outcome.as_ref())
                        .is_some_and(|endpoint| Arc::ptr_eq(&endpoint.0, record))
                {
                    transitioning = Some(index);
                    break;
                }
                facts.push(fact);
            }
            (facts, transitioning)
        };
        let Some(index) = transitioning else {
            return Ok(facts);
        };
        // No callback-map or std mutex guard is held over this await.
        transitions[index]
            .changed()
            .await
            .map_err(io::Error::other)?;
    }
}

pub(crate) struct Watch {
    record: Arc<Record>,
    receiver: tokio::sync::watch::Receiver<u8>,
}
impl Watch {
    pub(crate) fn request_id_data(&self) -> &RequestId {
        &self.record.id
    }
    /// Initial facts come only from facts_for's callback-map cut. This method
    /// returns subsequent real transition observations, not an unvalidated
    /// initial Pending value from the watch receiver.
    pub(crate) async fn changed(&mut self) -> io::Result<Fact> {
        self.receiver.changed().await.map_err(io::Error::other)?;
        let _version = *self.receiver.borrow_and_update();
        Ok(self.record.fact())
    }
}
impl OutgoingMessageSender {
    /// Prepare synchronously during accepted ticket construction, before any
    /// awaited Native/currentness work. Caller stores THIS owner before polling
    /// observe; drop/cancellation never turns lost observation into NoCapture.
    /// The same-lock original recipient cut either owns all requested cells or
    /// refuses before acceptance. IDs and preparation grant no Native authority.
    pub(crate) fn alab_prepare_callback_capture(
        &self,
        backend: &BoundBackend,
        actual: &RequestContext,
        ids: &[RequestId],
    ) -> io::Result<PreparedCapture> {
        let ledger = self
            .alab_callback_outcomes
            .as_ref()
            .ok_or_else(|| invalid("ordinary sender has no retained callback ledger"))?;
        if !ledger.belongs_to(backend) {
            return Err(invalid("callback capture changed actual bound Incoming"));
        }
        let origin =
            actual
                .alab_connection_resource
                .as_ref()
                .and_then(
                    codex_alab_core::connection_resource::ConnectionResource::downcast::<
                        AcceptedOrigin,
                    >,
                )
                .ok_or_else(|| invalid("callback capture requires actual physical A resource"))?;
        if actual.cancellation.is_cancelled() {
            return Err(invalid("callback capture observation cancelled"));
        }
        let (records, original_connection) =
            ledger.capture_with_connection(actual.alab_request_id().connection_id, ids)?;
        Ok(PreparedCapture {
            capture: Arc::new(Capture {
                ledger: ledger.clone(),
                records,
                _original_origin: origin,
                original_connection,
            }),
            cancellation: actual.cancellation.clone(),
        })
    }
    /// Invoke only AFTER the same-session original UI applied typed resolution
    /// and removed/reconciled its view. Delivery/absence/local send is not ACK.
    pub(crate) async fn alab_ack_callback_views(
        &self,
        backend: &BoundBackend,
        actual: &RequestContext,
        ids: &[RequestId],
    ) -> io::Result<()> {
        let ledger = self
            .alab_callback_outcomes
            .as_ref()
            .ok_or_else(|| invalid("ordinary sender has no retained callback ledger"))?;
        if !ledger.belongs_to(backend) {
            return Err(invalid(
                "callback acknowledgement changed actual bound Incoming",
            ));
        }
        let origin =
            actual
                .alab_connection_resource
                .as_ref()
                .and_then(
                    codex_alab_core::connection_resource::ConnectionResource::downcast::<
                        AcceptedOrigin,
                    >,
                )
                .ok_or_else(|| {
                    invalid("callback acknowledgement requires actual physical A resource")
                })?;
        let current = backend.observe_reader_origin(&origin).await?;
        if !current.same_receipt(&origin) {
            return Err(invalid(
                "callback acknowledgement changed actual bound A receipt",
            ));
        }
        if actual.cancellation.is_cancelled() {
            return Err(invalid("callback acknowledgement observation cancelled"));
        }
        ledger.acknowledge(actual.alab_request_id().connection_id, ids)
    }
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(all(test, unix))]
#[path = "retained_callback_outcome_tests.rs"]
mod tests;

fn association_count(state: &State) -> usize {
    state.recipients.values().map(HashMap::len).sum()
}
fn capacity() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        "Source callback ledger capacity unavailable",
    )
}
/// Source endpoint pins only. Constructed at the actual original map cut.
pub(super) struct Snapshot {
    ledger: Arc<Ledger>,
    records: Result<Vec<Arc<Record>>, &'static str>,
}
/// Reserves budget for the complete eligible replay BEFORE the first send.
/// Association is installed only at the original per-request send point.
pub(super) struct ReplayReservation {
    ledger: Arc<Ledger>,
    recipient: ConnectionId,
    connection: Arc<crate::connection_rpc_gate::ConnectionRpcGate>,
    records: Vec<Arc<Record>>,
    remaining: std::sync::atomic::AtomicUsize,
}
impl ReplayReservation {
    pub(super) fn install(&self, request: &ServerRequest) -> io::Result<()> {
        if !tracked(request) {
            return Ok(());
        }
        let record = self
            .records
            .iter()
            .find(|record| &record.id == request.id())
            .ok_or_else(|| invalid("request was not in actual reserved replay snapshot"))?;
        let mut state = self
            .ledger
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if self.connection.is_closed()
            || !state
                .connections
                .get(&self.recipient)
                .is_some_and(|actual| Arc::ptr_eq(actual, &self.connection))
        {
            return Err(invalid(
                "actual reserved connection closed or changed incarnation",
            ));
        }
        if let Some(actual) = state
            .recipients
            .get(&self.recipient)
            .and_then(|entries| entries.get(&record.id))
        {
            return if Arc::ptr_eq(actual, record) {
                Ok(())
            } else {
                Err(invalid("actual replay cell identity changed"))
            };
        }
        let remaining = self.remaining.load(Ordering::Acquire);
        if state.faulted || remaining == 0 || state.reserved == 0 {
            return Err(invalid("actual replay capacity reservation exhausted"));
        }
        self.remaining.store(remaining - 1, Ordering::Release);
        state.reserved -= 1;
        state
            .recipients
            .entry(self.recipient)
            .or_default()
            .insert(record.id.clone(), record.clone());
        Ok(())
    }
}
impl Drop for ReplayReservation {
    fn drop(&mut self) {
        let remaining = self.remaining.swap(0, Ordering::AcqRel);
        let mut state = self
            .ledger
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(left) = state.reserved.checked_sub(remaining) {
            state.reserved = left
        } else {
            state.faulted = true;
        }
        // No endpoint is retired, cancelled or acknowledged by releasing budget.
    }
}
#[cfg(test)]
#[path = "retained_callback_capacity_tests.rs"]
mod capacity_tests;

/// Source physical-close observation only. Never callback cancellation or a
/// Native/effect/ingress-drained proof; accepted original work is untouched.
pub(crate) struct ObserverStop {
    closed: tokio_util::sync::CancellationToken,
}
impl ObserverStop {
    pub(crate) async fn cancelled(&self) {
        self.closed.cancelled().await
    }
}
impl OutgoingMessageSender {
    pub(crate) async fn alab_callback_observer(
        &self,
        context: &RequestContext,
    ) -> io::Result<Arc<ObserverStop>> {
        let ledger = self
            .alab_callback_outcomes
            .as_ref()
            .ok_or_else(|| invalid("ordinary sender has no callback observer"))?;
        // SAME lock/order as connection_closed: if EOF won, the real registered
        // context is absent; otherwise registration precedes its close notification.
        let contexts = self.request_contexts.lock().await;
        if !contexts.contains_key(context.alab_request_id()) {
            return Err(invalid(
                "actual callback observer request is no longer registered",
            ));
        }
        let observer = Arc::new(ObserverStop {
            closed: tokio_util::sync::CancellationToken::new(),
        });
        {
            let mut state = ledger.state.lock().unwrap_or_else(PoisonError::into_inner);
            if !state
                .connections
                .get(&context.alab_request_id().connection_id)
                .is_some_and(|actual| !actual.is_closed())
            {
                return Err(invalid("actual observer connection admission closed"));
            }
            state.observers.retain(|_, observers| {
                observers.retain(|observer| observer.strong_count() != 0);
                !observers.is_empty()
            });
            state
                .observers
                .entry(context.alab_request_id().connection_id)
                .or_default()
                .push(Arc::downgrade(&observer));
        }
        drop(contexts);
        Ok(observer)
    }
}

impl OutgoingMessageSender {
    pub(crate) fn alab_callback_connection_open(
        &self,
        id: ConnectionId,
        actual: Arc<crate::connection_rpc_gate::ConnectionRpcGate>,
    ) {
        if let Some(ledger) = &self.alab_callback_outcomes {
            let mut state = ledger.state.lock().unwrap_or_else(PoisonError::into_inner);
            // Same resource as actual physical ConnectionState/session. IDs select
            // bookkeeping only; they cannot recreate a closed incarnation.
            let cut = Arc::new(ingress::ConnectionCut::new(actual.clone()));
            let prior_cut = state.ingress.insert(id, cut);
            let prior = state.connections.insert(id, actual);
            drop(state);
            drop(prior);
            drop(prior_cut);
        }
    }
}

#[path = "retained_input_ingress.rs"]
pub(crate) mod ingress;
#[path = "retained_input_endpoint.rs"]
pub(crate) mod original_input;
