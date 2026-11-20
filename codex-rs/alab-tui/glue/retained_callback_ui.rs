// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original App callback lifecycle. Source facts are endpoint observations,
//! never Native/App/Core/effect authority or proof our own answer was accepted.
use super::*;
use crate::app::alab_retained_views::identity::CapturedView;
use crate::app_server_session::alab_retained_ui::Pending;
use crate::app_server_session::alab_retained_ui::kind;
use codex_app_server_client::retained_callbacks::CallbackCapture;
use codex_app_server_client::retained_callbacks::CallbackCaptureOperation;
use codex_app_server_client::retained_callbacks::CallbackObservation;
use codex_app_server_client::retained_callbacks::CallbackRetirement;
use codex_app_server_client::retained_callbacks::ClosedCallbackObservation;
use codex_app_server_client::retained_callbacks::ConfiguredCallbackObservation;
use codex_app_server_client::retained_callbacks::Outcome;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigReadParams;
use codex_app_server_protocol::RequestId;
use std::collections::HashMap;
use std::collections::HashSet;
use tokio::sync::Semaphore;

/// Original caller provides its real decoder budgets, not a reconstructed UI.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Limits {
    pub(crate) bytes: usize,
    pub(crate) generations: usize,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TicketData {
    thread: String,
    operation: serde_json::Value,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TransferData {
    schema: u8,
    delayed_clock: Box<serde_json::value::RawValue>,
    original_inputs: Vec<super::original_input::transfer::Data>,
    graph: String,
    service: String,
    limits: Limits,
    views: Vec<CapturedView>,
    replies: Vec<HumanReplyData>,
    tickets: Vec<TicketData>,
}
pub(crate) struct CallbackUi {
    tenure: FrontendTenure,
    transfers: Mutex<Vec<Arc<Transfer>>>,
    live: Mutex<Vec<Arc<Live>>>,
    tickets: Mutex<Vec<Arc<Ticket>>>,
    blocked_logged: std::sync::atomic::AtomicBool,
    stop: tokio_util::sync::CancellationToken,
    observers: Mutex<Vec<Arc<observer::Observer>>>,
}
impl CallbackUi {
    pub(super) fn new(tenure: FrontendTenure) -> Arc<Self> {
        Arc::new(Self {
            tenure,
            transfers: Mutex::new(Vec::new()),
            live: Mutex::new(Vec::new()),
            tickets: Mutex::new(Vec::new()),
            blocked_logged: std::sync::atomic::AtomicBool::new(false),
            stop: tokio_util::sync::CancellationToken::new(),
            observers: Mutex::new(Vec::new()),
        })
    }
    pub(in crate::app) fn validate(&self, session: &AppServerSession) -> io::Result<()> {
        if session
            .retained_frontend()
            .is_none_or(|actual| !actual.same_delivery(&self.tenure))
        {
            return Err(invalid("callback UI changed its actual physical tenure"));
        }
        Ok(())
    }
    fn all(&self) -> Vec<Arc<Transfer>> {
        self.transfers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}
pub(crate) struct Transfer {
    delayed_clock: Box<serde_json::value::RawValue>,
    graph: String,
    original_graph: bool,
    source_data: Option<serde_json::Value>,
    tenure: FrontendTenure,
    views: Vec<CapturedView>,
    replies: Vec<HumanReplyData>,
    inputs: Vec<Arc<super::original_input::Item>>,
    tickets: Vec<Arc<Ticket>>,
    applied_views: Mutex<HashSet<usize>>,
    applications: Mutex<HashMap<String, Applied>>,
    configured: Mutex<HashMap<String, configured::Applied>>,
    unknown_application: std::sync::atomic::AtomicBool,
    limits: Limits,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Original,
    Transferred,
}
struct Ticket {
    key: String,
    origin: Origin,
    thread: String,
    ids: Vec<RequestId>,
    operation: Arc<CallbackCaptureOperation>,
    serial: Semaphore,
    progress: Mutex<TicketProgress>,
}
#[derive(Default)]
struct TicketProgress {
    capture: Option<CallbackCapture>,
    retirement: Option<CallbackRetirement>,
    observer: Option<Arc<observer::Observer>>,
    obsolete_observers: Vec<Arc<observer::Observer>>,
}
#[derive(Clone)]
enum Cut {
    Projected(Box<CallbackObservation>),
    Closed(Arc<ClosedCallbackObservation>),
}
impl Cut {
    fn facts(&self) -> Option<&[codex_app_server_client::retained_callbacks::Fact]> {
        match self {
            Self::Projected(actual) => actual.facts(),
            Self::Closed(actual) => actual.facts(),
        }
    }
    fn retired(&self) -> bool {
        match self {
            Self::Projected(actual) => actual.ticket_retired(),
            Self::Closed(actual) => actual.ticket_retired(),
        }
    }
}
struct Applied {
    observation: Cut,
    pending: Arc<Pending>,
    activation: Option<kind::Activation>,
    closed: Vec<RequestId>,
}
/// The live C1 owner is installed before capture/ACK's first await. A lost
/// result retains SAME stable operation for a Source-authenticated retry.
struct Live {
    id: RequestId,
    ticket: Arc<Ticket>,
    serial: Semaphore,
    progress: Mutex<LiveProgress>,
}
#[derive(Default)]
struct LiveProgress {
    retirement: Option<CallbackRetirement>,
}

fn requests(
    views: &[CapturedView],
    replies: &[HumanReplyData],
    bytes: usize,
) -> io::Result<Vec<(String, Vec<RequestId>)>> {
    let mut groups: Vec<(String, Vec<RequestId>)> = Vec::new();
    let mut seen = HashMap::<RequestId, String>::new();
    let mut push = |thread: &str, id: &RequestId| -> io::Result<()> {
        let thread = codex_protocol::ThreadId::from_string(thread)
            .map_err(invalid)?
            .to_string();
        if let Some(prior) = seen.insert(id.clone(), thread.clone()) {
            if prior != thread {
                return Err(invalid(
                    "one callback ID was captured for different actual Core threads",
                ));
            }
            return Ok(());
        }
        if let Some((_, ids)) = groups.iter_mut().find(|(actual, _)| actual == &thread) {
            ids.push(id.clone());
        } else {
            groups.push((thread, vec![id.clone()]));
        }
        Ok(())
    };
    let mut view_ids = HashSet::new();
    for view in views {
        view.validate_data(bytes)?;
        if view
            .request_ids_data()
            .iter()
            .any(|id| !view_ids.insert(id.clone()))
        {
            return Err(invalid(
                "one original callback is repeated by different editing views",
            ));
        }
        let original = view.original_thread_ids_data(bytes)?;
        for (thread, id) in original.iter().zip(view.request_ids_data()) {
            push(thread, id)?;
        }
    }
    for reply in replies {
        reply.validate_data()?;
        push(reply.thread_id_data(), reply.request_id_data())?;
    }
    Ok(groups)
}
impl Transfer {
    fn from_actual(
        owner: &CallbackUi,
        views: Vec<CapturedView>,
        replies: Vec<HumanReplyData>,
        inputs: Vec<Arc<super::original_input::Item>>,
        delayed_clock: Box<serde_json::value::RawValue>,
        limits: Limits,
    ) -> io::Result<Arc<Self>> {
        let _clock = crate::bottom_pane::alab_retained_delayed_approval::PreparedDelayed::decode(
            &delayed_clock,
            limits.bytes,
        )?;
        let mut expected = requests(&views, &replies, limits.bytes)?;
        for item in &inputs {
            let thread = item.thread_id_data();
            let id = item.id();
            if let Some((_, ids)) = expected.iter_mut().find(|(actual, _)| actual == thread) {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            } else {
                expected.push((thread.to_owned(), vec![id]));
            }
        }
        let mut canonical = owner.tickets.lock().unwrap_or_else(PoisonError::into_inner);
        // Carry every unfinished actual ticket, even if its original view has
        // already disappeared. Preserve ALL immutable descriptor IDs, including
        // extra Pending cells. Only uncovered original IDs create an operation.
        let mut tickets: Vec<_> = canonical
            .iter()
            .filter(|ticket| !ticket.retired())
            .cloned()
            .collect();
        for item in &inputs {
            if let Some(actual) = item.callback_capture() {
                let ticket = actual.0;
                if !tickets.iter().any(|prior| Arc::ptr_eq(prior, &ticket)) {
                    tickets.push(ticket);
                }
            }
        }
        for (thread, ids) in expected {
            let mut uncovered = Vec::new();
            for id in ids {
                if let Some(ticket) = tickets.iter().find(|ticket| ticket.ids.contains(&id)) {
                    if ticket.thread != thread {
                        return Err(invalid("captured callback changed original Core identity"));
                    }
                } else {
                    uncovered.push(id);
                }
            }
            if !uncovered.is_empty() {
                let operation = CallbackCaptureOperation::new(
                    owner.tenure.target.service_data(),
                    uncovered.clone(),
                )
                .map_err(io::Error::other)?;
                let ticket = Ticket::new(thread, uncovered, operation, Origin::Original)?;
                canonical.push(ticket.clone());
                tickets.push(ticket);
            }
        }
        Ok(Arc::new(Self {
            delayed_clock,
            graph: graph_id(),
            original_graph: true,
            source_data: None,
            tenure: owner.tenure.clone(),
            views,
            replies,
            inputs,
            tickets,
            applied_views: Mutex::new(HashSet::new()),
            applications: Mutex::new(HashMap::new()),
            configured: Mutex::new(HashMap::new()),
            unknown_application: std::sync::atomic::AtomicBool::new(false),
            limits,
        }))
    }
    /// Must finish while original C1 still owns its authenticated recipients.
    /// The App retains this SAME Transfer before the first capture is polled.
    pub(crate) async fn capture(&self, session: &AppServerSession) -> io::Result<()> {
        self.validate(session)?;
        for ticket in &self.tickets {
            let _actual = ticket.capture(session).await?;
        }
        for item in &self.inputs {
            item.prepare(session).await?;
        }
        Ok(())
    }
    fn validate(&self, session: &AppServerSession) -> io::Result<()> {
        if session
            .retained_frontend()
            .is_none_or(|actual| !actual.same_delivery(&self.tenure))
        {
            return Err(invalid(
                "callback transfer changed actual physical delivery",
            ));
        }
        Ok(())
    }
    /// DATA is encoded only after every actual Source capture was observed.
    /// The original App/Source owners remain retained; bytes are not applied.
    pub(crate) fn checkpoint(&self) -> io::Result<serde_json::Value> {
        for ticket in &self.tickets {
            if ticket
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .capture
                .is_none()
            {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Source callback capture still unobserved",
                ));
            }
        }
        let tickets = self
            .tickets
            .iter()
            .map(|ticket| {
                Ok(TicketData {
                    thread: ticket.thread.clone(),
                    operation: ticket.operation.checkpoint().map_err(io::Error::other)?,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        serde_json::to_value(serde_json::json!({"schema":3,"delayedClock":self.delayed_clock,"originalInputs":self.inputs.iter().map(|item| item.checkpoint()).collect::<io::Result<Vec<_>>>()?,"graph":self.graph,"service":self.tenure.target.service_data(),
            "limits":self.limits,"views":self.views,"replies":self.replies,"tickets":tickets})).map_err(invalid)
    }
    fn restore(
        owner: &CallbackUi,
        replies_owner: &Replies,
        value: serde_json::Value,
        limits: Limits,
    ) -> io::Result<Arc<Self>> {
        let data: TransferData = serde_json::from_value(value.clone()).map_err(invalid)?;
        if data.schema != 3
            || data.service != owner.tenure.target.service_data()
            || !token(&data.graph)
        {
            return Err(invalid("callback transfer changed service/schema/graph"));
        }
        let _clock = crate::bottom_pane::alab_retained_delayed_approval::PreparedDelayed::decode(
            &data.delayed_clock,
            limits.bytes,
        )?;
        if let Some(old) = owner.all().into_iter().find(|old| old.graph == data.graph) {
            if old.source_data.as_ref() != Some(&value) {
                return Err(invalid("repeated callback graph changed immutable DATA"));
            }
            return Ok(old);
        }
        let inputs = super::original_input::transfer::restore(replies_owner, data.original_inputs)?;
        let mut expected = requests(&data.views, &data.replies, limits.bytes)?;
        for item in &inputs {
            let thread = item.thread_id_data();
            let id = item.id();
            if let Some((_, ids)) = expected.iter_mut().find(|(actual, _)| actual == thread) {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            } else {
                expected.push((thread.to_owned(), vec![id]));
            }
        }
        let mut tickets = Vec::new();
        let mut all_ids = HashMap::<RequestId, String>::new();
        let mut canonical = owner.tickets.lock().unwrap_or_else(PoisonError::into_inner);
        for data in data.tickets {
            let thread = codex_protocol::ThreadId::from_string(&data.thread)
                .map_err(invalid)?
                .to_string();
            if thread != data.thread
                || data
                    .operation
                    .get("service")
                    .and_then(serde_json::Value::as_str)
                    != Some(owner.tenure.target.service_data())
                || data
                    .operation
                    .get("descriptor")
                    .is_none_or(serde_json::Value::is_null)
            {
                return Err(invalid(
                    "callback checkpoint changed original thread/service or lacks accepted Source capture",
                ));
            }
            let ids: Vec<RequestId> = serde_json::from_value(
                data.operation
                    .get("ids")
                    .cloned()
                    .ok_or_else(|| invalid("callback IDs missing"))?,
            )
            .map_err(invalid)?;
            for id in &ids {
                if all_ids.insert(id.clone(), thread.clone()).is_some() {
                    return Err(invalid("different tickets repeat an original callback ID"));
                }
            }
            let operation = CallbackCaptureOperation::restore_checkpoint(data.operation)
                .map_err(io::Error::other)?;
            let key = operation_key(&operation)?;
            let ticket = if let Some(actual) = canonical.iter().find(|actual| actual.key == key) {
                if actual.thread != thread || actual.ids != ids {
                    return Err(invalid(
                        "canonical callback ticket changed immutable original IDs",
                    ));
                }
                actual.clone()
            } else {
                let actual = Ticket::new(thread, ids, operation, Origin::Transferred)?;
                canonical.push(actual.clone());
                actual
            };
            tickets.push(ticket);
        }
        // Extra private descriptor IDs are intentionally preserved, never
        // omitted because a form resolved or disappeared from current UI DATA.
        for (thread, ids) in expected {
            for id in ids {
                if all_ids.get(&id) != Some(&thread) {
                    return Err(invalid(
                        "callback checkpoint omitted a captured original view/reply",
                    ));
                }
            }
        }
        Ok(Arc::new(Self {
            delayed_clock: data.delayed_clock,
            graph: data.graph,
            original_graph: false,
            source_data: Some(value),
            tenure: owner.tenure.clone(),
            views: data.views,
            replies: data.replies,
            inputs,
            tickets,
            applied_views: Mutex::new(HashSet::new()),
            applications: Mutex::new(HashMap::new()),
            configured: Mutex::new(HashMap::new()),
            unknown_application: std::sync::atomic::AtomicBool::new(false),
            limits,
        }))
    }
}
pub(crate) fn restore_owned(
    owner: &Arc<CallbackUi>,
    replies: &Replies,
    session: &AppServerSession,
    data: serde_json::Value,
    limits: Limits,
) -> io::Result<Arc<Transfer>> {
    owner.validate(session)?;
    let transfer = Transfer::restore(owner, replies, data, limits)?;
    let mut transfers = owner
        .transfers
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if !transfers.iter().any(|old| Arc::ptr_eq(old, &transfer)) {
        transfers.push(transfer.clone());
    }
    Ok(transfer)
}
impl Transfer {
    pub(crate) async fn resume_original_inputs(
        &self,
        session: &AppServerSession,
    ) -> io::Result<()> {
        self.validate(session)?;
        for item in &self.inputs {
            item.resume(session).await?;
        }
        Ok(())
    }
}
impl Ticket {
    fn new(
        thread: String,
        ids: Vec<RequestId>,
        operation: Arc<CallbackCaptureOperation>,
        origin: Origin,
    ) -> io::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            key: operation_key(&operation)?,
            origin,
            thread,
            ids,
            operation,
            serial: Semaphore::new(1),
            progress: Mutex::new(TicketProgress::default()),
        }))
    }
    fn retired(&self) -> bool {
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retirement
            .as_ref()
            .is_some_and(CallbackRetirement::ticket_retired)
    }
    async fn capture(&self, session: &AppServerSession) -> io::Result<CallbackCapture> {
        let _permit = self.serial.acquire().await.map_err(invalid)?;
        let prior = self
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .capture
            .clone();
        if let Some(actual) = prior {
            return Ok(actual);
        }
        let actual = self
            .operation
            .capture(&session.request_handle(), read_request())
            .await
            .map_err(io::Error::other)?;
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .capture = Some(actual.clone());
        Ok(actual)
    }
    async fn observe(
        &self,
        session: &AppServerSession,
        receipt: &kind::Receipt,
    ) -> io::Result<Cut> {
        let capture = self.capture(session).await?;
        let same_core = self.thread == receipt.thread_id_data();
        let actual = match (receipt, same_core) {
            (kind::Receipt::Runtime(receipt), true) => Cut::Projected(Box::new(
                capture
                    .observe_runtime(&session.request_handle(), read_request(), receipt, None)
                    .await
                    .map_err(io::Error::other)?,
            )),
            (kind::Receipt::Source(receipt), true) => Cut::Projected(Box::new(
                capture
                    .observe_source(&session.request_handle(), read_request(), receipt, None)
                    .await
                    .map_err(io::Error::other)?,
            )),
            (kind::Receipt::Runtime(receipt), false) => Cut::Closed(Arc::new(
                capture
                    .observe_closed_runtime(
                        &session.request_handle(),
                        read_request(),
                        receipt,
                        self.ids.clone(),
                    )
                    .await
                    .map_err(io::Error::other)?,
            )),
            (kind::Receipt::Source(receipt), false) => Cut::Closed(Arc::new(
                capture
                    .observe_closed_source(
                        &session.request_handle(),
                        read_request(),
                        receipt,
                        self.ids.clone(),
                    )
                    .await
                    .map_err(io::Error::other)?,
            )),
        };
        validate_cut(&self.thread, &self.ids, &actual)?;
        Ok(actual)
    }
}
/// This function classifies DATA only. Its production caller accepts a private
/// actual CallbackObservation from the protected Source exchange above.
fn validate_cut(thread: &str, ids: &[RequestId], observation: &Cut) -> io::Result<Vec<RequestId>> {
    let Some(facts) = observation.facts() else {
        if observation.retired() {
            return Ok(ids.to_vec());
        }
        return Err(invalid("callback cut omitted actual Source facts"));
    };
    if facts.len() != ids.len() {
        return Err(invalid("callback cut lost an exact captured ID"));
    }
    let mut closed = Vec::new();
    for (fact, id) in facts.iter().zip(ids) {
        if fact.request_id_data() != id || fact.thread_id_data() != Some(thread) {
            return Err(invalid(
                "callback cut changed its actual Core/endpoint identity",
            ));
        }
        match fact.outcome_data() {
            Outcome::Pending {} => {}
            Outcome::Response {}
            | Outcome::ClientError {}
            | Outcome::Cancelled {}
            | Outcome::Withdrawn {} => closed.push(id.clone()),
            Outcome::Unknown {} => {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "actual Source endpoint outcome remains Unknown",
                ));
            }
        }
    }
    Ok(closed)
}
pub(in crate::app) fn read_request() -> ClientRequest {
    ClientRequest::ConfigRead {
        request_id: RequestId::String(format!(
            "retained-callback-ui-{:016x}{:016x}",
            codex_alab_core::native_source::connection_request_prefix(),
            codex_alab_core::native_source::connection_request_prefix()
        )),
        params: ConfigReadParams {
            include_layers: false,
            cwd: None,
        },
    }
}
fn ui(app: &App, session: &AppServerSession) -> io::Result<Option<Arc<CallbackUi>>> {
    if app.app_server_backend_id != Some(session.backend_id()) {
        return Err(invalid("callback UI changed actual backend"));
    }
    let Some(owner) = &app.pending_app_server_requests.alab_retained_replies.owner else {
        if session.retained_frontend().is_none() {
            return Ok(None);
        }
        return Err(invalid(
            "original retained reply owner missing before callback event",
        ));
    };
    owner.callbacks.validate(session)?;
    Ok(Some(owner.callbacks.clone()))
}
impl App {
    /// Restrict-only: a retained original Value must acquire the genuine
    /// Source input-publication owner before endpoint-ticket retirement may
    /// release its only private Cell. Endpoint closure alone cannot settle it.
    fn alab_callback_value_needs_source_binding(
        &self,
        owner: &Arc<CallbackUi>,
        id: &RequestId,
    ) -> bool {
        self.pending_app_server_requests
            .alab_retained_replies
            .owner
            .as_ref()
            .is_some_and(|actual| {
                Arc::ptr_eq(&actual.callbacks, owner)
                    && (actual
                        .inputs
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .iter()
                        .any(|item| item.id() == *id && item.needs_binding())
                        || actual
                            .jobs
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .iter()
                            .any(|job| job.id == *id))
            })
            || owner.all().iter().any(|transfer| {
                transfer
                    .replies
                    .iter()
                    .any(|reply| reply.request_id_data() == id)
            })
    }
    /// Synchronous capture from original App registry/graph, stored before IO.
    pub(crate) fn alab_prepare_callback_transfer(
        &mut self,
        session: &AppServerSession,
        limits: Limits,
    ) -> io::Result<Arc<Transfer>> {
        let owner = ui(self, session)?
            .ok_or_else(|| invalid("callback transfer requires actual retained session"))?;
        let (views, native_inputs, delayed_clock) =
            self.alab_capture_retained_pending_view_owners(session, limits.bytes)?;
        let replies = self.alab_capture_retained_human_replies(session)?;
        let mut inputs = super::original_input::transfer::current_items(
            &self.pending_app_server_requests.alab_retained_replies,
            &owner,
        )?;
        for item in native_inputs {
            if !inputs.iter().any(|actual| Arc::ptr_eq(actual, &item)) {
                inputs.push(item);
            }
        }
        let transfer =
            Transfer::from_actual(&owner, views, replies, inputs, delayed_clock, limits)?;
        owner
            .transfers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(transfer.clone());
        Ok(transfer)
    }
    pub(crate) fn alab_restore_callback_section(
        &mut self,
        session: &AppServerSession,
        raw: &serde_json::value::RawValue,
    ) -> io::Result<Arc<Transfer>> {
        let value: serde_json::Value = serde_json::from_str(raw.get()).map_err(invalid)?;
        let data: TransferData = serde_json::from_value(value.clone()).map_err(invalid)?;
        let limits = data.limits;
        if limits.bytes == 0
            || limits.bytes > codex_alab_tui::upgrade_exec_custody::MAX_STATE_BYTES
            || limits.generations == 0
            || limits.generations > limits.bytes
            || raw.get().len() > limits.bytes
        {
            return Err(invalid(
                "callback section changed its captured native budget",
            ));
        }
        self.alab_restore_callback_transfer(session, value, limits)
    }
    /// The actual E1 consumer calls before replay/event drain; fresh Native A
    /// and Source Capture are still required before any editing is restored.
    pub(crate) fn alab_restore_callback_transfer(
        &mut self,
        session: &AppServerSession,
        data: serde_json::Value,
        limits: Limits,
    ) -> io::Result<Arc<Transfer>> {
        let owner = ui(self, session)?
            .ok_or_else(|| invalid("callback restore requires actual retained session"))?;
        let transfer = Transfer::restore(
            &owner,
            &self.pending_app_server_requests.alab_retained_replies,
            data,
            limits,
        )?;
        self.chat_widget
            .alab_prime_delayed_clock(&transfer.delayed_clock, limits.bytes)?;
        let mut transfers = owner
            .transfers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !transfers.iter().any(|old| Arc::ptr_eq(old, &transfer)) {
            transfers.push(transfer.clone());
        }
        Ok(transfer)
    }
    /// Called only after actual replay End, before a NEW successful draw. A
    /// returned true requires redraw before Native UIACK; no queue-empty proof.
    pub(in crate::app) async fn alab_reconcile_callback_transfer(
        &mut self,
        session: &AppServerSession,
        pending: &Arc<Pending>,
    ) -> io::Result<bool> {
        let Some(owner) = ui(self, session)? else {
            return Ok(false);
        };
        let Some(receipt) = pending
            .callback_receipt(session)
            .map_err(io::Error::other)?
        else {
            return Ok(false);
        };
        self.alab_flush_original_inputs(session).await?;
        let mut changed = false;
        for transfer in owner.all() {
            transfer.validate(session)?;
            if transfer.original_graph {
                continue;
            }
            if transfer.unknown_application.load(Ordering::Acquire) {
                return Err(invalid("callback view application remains unobserved"));
            }
            let mut cuts = Vec::new();
            let mut closed_set = HashSet::new();
            let mut covered = HashSet::new();
            let mut fresh = false;
            for ticket in &transfer.tickets {
                let prior = transfer
                    .applications
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(&ticket.key)
                    .filter(|applied| Arc::ptr_eq(&applied.pending, pending))
                    .map(|applied| applied.observation.clone());
                let observation = match prior {
                    Some(actual) => actual,
                    None => {
                        fresh = true;
                        ticket.observe(session, &receipt).await?
                    }
                };
                let closed = validate_cut(&ticket.thread, &ticket.ids, &observation)?;
                observer::ensure(
                    &owner,
                    ticket,
                    pending,
                    &receipt,
                    &observation,
                    self.app_event_tx.clone(),
                    session.request_handle(),
                )?;
                closed_set.extend(closed.iter().cloned());
                covered.extend(ticket.ids.iter().cloned());
                cuts.push((ticket.clone(), observation, closed));
            }
            // A form's original queue may span several immutable tickets.
            // Observe all its exact cells BEFORE the first UI mutation.
            if transfer.replies.iter().any(|reply| {
                covered.contains(reply.request_id_data())
                    && !closed_set.contains(reply.request_id_data())
            }) {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "original human response needs its genuine publication continuation",
                ));
            }
            if fresh {
                let mut guard = ApplicationGuard {
                    transfer: &transfer,
                    finished: false,
                };
                for (ticket, _, closed) in &cuts {
                    for id in closed {
                        self.alab_apply_closed_callback(&ticket.thread, id);
                    }
                }
                for (index, view) in transfer.views.iter().enumerate() {
                    if !view
                        .request_ids_data()
                        .iter()
                        .all(|id| covered.contains(id))
                    {
                        continue;
                    }
                    if !transfer
                        .applied_views
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .contains(&index)
                    {
                        self.alab_install_callback_editing(
                            session,
                            view,
                            &closed_set,
                            transfer.limits.bytes,
                            transfer.limits.generations,
                        )?;
                        transfer
                            .applied_views
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .insert(index);
                    }
                }
                let mut applications = transfer
                    .applications
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                for (ticket, observation, closed) in cuts {
                    applications.insert(
                        ticket.key.clone(),
                        Applied {
                            observation,
                            pending: pending.clone(),
                            activation: None,
                            closed,
                        },
                    );
                }
                guard.finished = true;
                changed = true;
            }
            if transfer.views.iter().enumerate().any(|(index, _)| {
                !transfer
                    .applied_views
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .contains(&index)
            }) || transfer.tickets.iter().any(|ticket| {
                !transfer
                    .applications
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .contains_key(&ticket.key)
            }) {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "remaining callback graph requires its genuine per-Core or closed-subset projection",
                ));
            }
        }
        Ok(changed)
    }
    /// Strict-ID original registry resolution +original ChatWidget dismissal.
    /// Never call a reply handle, reopen a request or infer outcome from absence.
    pub(in crate::app) fn alab_apply_closed_callback(&mut self, thread: &str, id: &RequestId) {
        let _resolved = super::original_input::resolving(id);
        self.pending_app_server_requests
            .alab_retained_replies
            .retire_unused_input(thread, id);
        let thread_id = codex_protocol::ThreadId::from_string(thread).ok();
        self.pending_primary_events.retain(|event| !matches!(event, crate::app::ThreadBufferedEvent::Request(request)
            if request.id() == id && crate::app::app_server_event_targets::server_request_thread_id(request) == thread_id));
        if let Some(thread_id) = thread_id
            && let Some(requests) = self.agents_overview.dispatched_requests.get_mut(&thread_id)
        {
            requests.retain(|request| request.request.id() != id);
        }
        if let Some(resolved) = self
            .pending_app_server_requests
            .resolve_notification(thread, id)
            && self
                .chat_widget
                .thread_id()
                .map(|actual| actual.to_string())
                == Some(resolved.thread_id)
        {
            self.chat_widget
                .dismiss_app_server_request(&resolved.request);
            if self.startup_pending_protected_request {
                self.startup_pending_protected_request = self.chat_widget.has_active_modal()
                    || self.chat_widget.has_pending_protected_request();
            }
        }
    }
    pub(in crate::app) async fn alab_ack_applied_callbacks(
        &mut self,
        session: &AppServerSession,
    ) -> io::Result<()> {
        let Some(owner) = ui(self, session)? else {
            return Ok(());
        };
        for transfer in owner.all() {
            for ticket in &transfer.tickets {
                let prepared = {
                    if ticket.retired() {
                        continue;
                    }
                    let applications = transfer
                        .applications
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner);
                    applications.get(&ticket.key).map(|applied| {
                        (
                            applied.pending.clone(),
                            applied.activation.clone(),
                            applied.observation.clone(),
                            applied.closed.clone(),
                        )
                    })
                };
                let Some((pending, prior_activation, observation, mut ids)) = prepared else {
                    continue;
                };
                ids.retain(|id| !self.alab_callback_value_needs_source_binding(&owner, id));
                if ids.is_empty() {
                    continue;
                }
                let activation = match prior_activation {
                    Some(actual) => actual,
                    None => {
                        let Some(actual) = pending
                            .callback_activation(session)
                            .map_err(io::Error::other)?
                        else {
                            continue;
                        };
                        if let Some(applied) = transfer
                            .applications
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .get_mut(&ticket.key)
                        {
                            applied.activation = Some(actual.clone()); // Exact result retained BEFORE ACK await.
                        }
                        actual
                    }
                };
                let capture = ticket.capture(session).await?;
                let retirement = match &observation {
                    Cut::Projected(actual) => {
                        activation
                            .acknowledge_callbacks(
                                &capture,
                                &session.request_handle(),
                                read_request(),
                                actual,
                                ids.clone(),
                            )
                            .await
                    }
                    Cut::Closed(actual) => {
                        activation
                            .acknowledge_closed_callbacks(
                                &capture,
                                &session.request_handle(),
                                read_request(),
                                actual,
                                ids.clone(),
                            )
                            .await
                    }
                }
                .map_err(io::Error::other)?;
                // Preserve the genuine acknowledgement BEFORE cleanup. Source
                // accepted endpoint retirement, not our human response/effects.
                ticket
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .retirement = Some(retirement);
                // Original human Submission/value owners intentionally remain.
                // Endpoint semantic retirement is not original-value acceptance
                // or an old-ingress closure receipt. The distinct publication
                // settlement companion must retire those original owners.
            }
        }
        Ok(())
    }
}
struct ApplicationGuard<'a> {
    transfer: &'a Transfer,
    finished: bool,
}
impl Drop for ApplicationGuard<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.transfer
                .unknown_application
                .store(true, Ordering::Release);
        }
    }
}

impl Live {
    async fn drive(&self, session: &AppServerSession) -> io::Result<bool> {
        let _permit = self.serial.acquire().await.map_err(invalid)?;
        if self.ticket.retired()
            || self
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retirement
                .is_some()
        {
            return Ok(true);
        }
        let capture = self.ticket.capture(session).await?;
        // DISTINCT live route: Source verifies the SAME originally captured A,
        // fresh current origin, OWN closed cells and authenticated Book retry.
        // A cross-origin C2 cannot use this instead of genuine projection+commit.
        let retirement = capture
            .acknowledge_live(
                &session.request_handle(),
                read_request(),
                vec![self.id.clone()],
            )
            .await
            .map_err(io::Error::other)?;
        let complete = retirement.request_ids_data() == [self.id.clone()];
        // Canonical ticket keeps the actual Source retirement even after this
        // temporary live observer disappears. A partial ACK preserves all IDs.
        if retirement.ticket_retired() {
            self.ticket
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retirement = Some(retirement);
        } else {
            self.progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retirement = Some(retirement);
        }
        if !complete {
            return Err(invalid(
                "live callback retirement changed exact request identity",
            ));
        }
        Ok(true)
    }
}
impl App {
    /// Hook is AFTER the complete original ServerRequestResolved handler. The
    /// actual registry/view/buffer dismissal has already happened, even when a
    /// response was sent by another frontend. This never submits a human answer.
    pub(in crate::app) async fn alab_after_callback_resolved(
        &mut self,
        session: &AppServerSession,
        thread: &str,
        id: &RequestId,
    ) -> io::Result<()> {
        let Some(owner) = ui(self, session)? else {
            return Ok(());
        };
        let ticket = {
            let mut tickets = owner.tickets.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(actual) = tickets
                .iter()
                .find(|ticket| ticket.ids.contains(id) && !ticket.retired())
            {
                if actual.thread != thread {
                    return Err(invalid("resolved callback changed actual Core identity"));
                }
                actual.clone()
            } else {
                let operation = CallbackCaptureOperation::new(
                    owner.tenure.target.service_data(),
                    vec![id.clone()],
                )
                .map_err(io::Error::other)?;
                let actual = Ticket::new(
                    thread.to_owned(),
                    vec![id.clone()],
                    operation,
                    Origin::Original,
                )?;
                tickets.push(actual.clone());
                actual
            }
        };
        if ticket.origin == Origin::Original {
            let job = {
                let mut jobs = owner.live.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(actual) = jobs
                    .iter()
                    .find(|job| job.id == *id && Arc::ptr_eq(&job.ticket, &ticket))
                {
                    actual.clone()
                } else {
                    let actual = Arc::new(Live {
                        id: id.clone(),
                        ticket: ticket.clone(),
                        serial: Semaphore::new(1),
                        progress: Mutex::new(LiveProgress::default()),
                    });
                    jobs.push(actual.clone());
                    actual
                }
            };
            if self.alab_callback_value_needs_source_binding(&owner, id) {
                let _pinned = job.ticket.capture(session).await?;
            } else {
                let _observed = job.drive(session).await?;
            }
        } else if let Some(pending) = session.alab_retained_ui()
            && let Some(receipt) = pending
                .callback_receipt(session)
                .map_err(io::Error::other)?
        {
            let observation = ticket.observe(session, &receipt).await?;
            let closed = validate_cut(thread, &ticket.ids, &observation)?;
            if !closed.contains(id) {
                return Err(invalid("resolved callback lacks exact Source closed cell"));
            }
            for closed in &closed {
                self.alab_apply_closed_callback(thread, closed);
            }
            for transfer in owner.all() {
                if transfer.original_graph {
                    continue;
                }
                let mut applications = transfer
                    .applications
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if let Some(applied) = applications.get_mut(&ticket.key) {
                    applied.observation = observation.clone();
                    applied.pending = pending.clone();
                    applied.activation = None;
                    applied.closed = closed.clone();
                }
            }
        }
        self.alab_flush_callback_retirements(session).await
    }

    pub(in crate::app) fn alab_callback_restore_blocked(
        &self,
        session: &AppServerSession,
        error: &io::Error,
    ) {
        if let Ok(Some(actual)) = ui(self, session)
            && !actual.blocked_logged.swap(true, Ordering::AcqRel)
        {
            tracing::warn!(%error,"retained callback graph remains owned before UI commit");
        }
    }
    /// Ordinary subsequent handler/draw events retry SAME endpoint-bookkeeping
    /// owners. Lost transport results never recreate or resubmit a human reply.
    pub(in crate::app) async fn alab_flush_callback_retirements(
        &mut self,
        session: &AppServerSession,
    ) -> io::Result<()> {
        let Some(owner) = ui(self, session)? else {
            return Ok(());
        };
        self.alab_flush_original_inputs(session).await?;
        let jobs = owner
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        for job in jobs {
            if self.alab_callback_value_needs_source_binding(&owner, &job.id) {
                let _pinned = job.ticket.capture(session).await?;
            } else {
                let _observed = job.drive(session).await?;
            }
        }
        self.alab_ack_applied_callbacks(session).await?;
        let retired = {
            let mut jobs = owner.live.lock().unwrap_or_else(PoisonError::into_inner);
            let mut retired = Vec::new();
            let mut index = 0;
            while index < jobs.len() {
                if jobs[index].ticket.retired()
                    || jobs[index]
                        .progress
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .retirement
                        .is_some()
                {
                    retired.push(jobs.remove(index));
                } else {
                    index += 1;
                }
            }
            retired
        };
        drop(retired); // Actual Source closed-cell retirement was observed.
        let retired_transfers = {
            let mut transfers = owner
                .transfers
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let mut removed = Vec::new();
            let mut index = 0;
            while index < transfers.len() {
                let actual = &transfers[index];
                if actual.replies.is_empty()
                    && actual.inputs.iter().all(|item| item.retired())
                    && !actual.unknown_application.load(Ordering::Acquire)
                    && actual.tickets.iter().all(|ticket| ticket.retired())
                {
                    removed.push(transfers.remove(index));
                } else {
                    index += 1;
                }
            }
            removed
        };
        drop(retired_transfers); // Private results/Native contexts drop outside owner/progress locks.
        let retired_tickets = {
            let mut tickets = owner.tickets.lock().unwrap_or_else(PoisonError::into_inner);
            let mut removed = Vec::new();
            let mut index = 0;
            while index < tickets.len() {
                if tickets[index].retired() {
                    removed.push(tickets.remove(index));
                } else {
                    index += 1;
                }
            }
            removed
        };
        drop(retired_tickets);
        observer::reap(&owner).await?;
        Ok(())
    }
}

fn graph_id() -> String {
    format!(
        "{:016x}{:016x}",
        codex_alab_core::native_source::connection_request_prefix(),
        codex_alab_core::native_source::connection_request_prefix()
    )
}
fn token(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn operation_key(operation: &CallbackCaptureOperation) -> io::Result<String> {
    let checkpoint = operation.checkpoint().map_err(io::Error::other)?;
    let key = checkpoint
        .get("operation")
        .and_then(serde_json::Value::as_str)
        .filter(|key| token(key))
        .ok_or_else(|| invalid("callback operation key missing"))?;
    Ok(key.to_owned())
}

#[path = "retained_callback_ui_configured.rs"]
mod configured;

impl Drop for CallbackUi {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
#[path = "retained_callback_ui_observer.rs"]
pub(in crate::app) mod observer;

#[path = "retained_callback_input_capture.rs"]
pub(in crate::app) mod input_capture;

#[path = "retained_callback_ui_startup.rs"]
mod startup;
pub(crate) use startup::StartupCallbacks;

/// Mandatory actual-pane clock DATA, even with no delayed callback rows. This
/// decoder neither constructs a pane nor supplies callback/Native authority.
pub(crate) fn prepare_delayed_clock(
    section: &serde_json::value::RawValue,
    service: &str,
    max_bytes: usize,
) -> io::Result<crate::bottom_pane::alab_retained_delayed_approval::PreparedDelayed> {
    if section.get().len() > max_bytes {
        return Err(invalid("callback clock section exceeds original bound"));
    }
    let data: TransferData = serde_json::from_str(section.get()).map_err(invalid)?;
    if data.schema != 3
        || data.service != service
        || !token(&data.graph)
        || data.limits.bytes == 0
        || data.limits.bytes > max_bytes
        || data.limits.generations == 0
        || data.limits.generations > data.limits.bytes
    {
        return Err(invalid("callback clock changed schema/service/budget"));
    }
    crate::bottom_pane::alab_retained_delayed_approval::PreparedDelayed::decode(
        &data.delayed_clock,
        data.limits.bytes,
    )
}

#[cfg(test)]
mod mandatory_clock_controls {
    use super::*;
    #[test]
    fn callback_clock_leaf_is_required_and_same_service_without_empty_view_ids() -> io::Result<()> {
        let service = "00000000000000000000000000000001";
        let value = serde_json::json!({"schema":3,"graph":"00000000000000000000000000000002","service":service,"limits":{"bytes":1_048_576,"generations":1024},"delayedClock":{"schema":1,"requests":[],"last_activity":null,"generation":0},"originalInputs":[],"views":[],"replies":[],"tickets":[]});
        let raw = serde_json::value::to_raw_value(&value).map_err(invalid)?;
        let _clock = prepare_delayed_clock(&raw, service, 1_048_576)?;
        assert!(
            prepare_delayed_clock(&raw, "00000000000000000000000000000003", 1_048_576).is_err()
        );
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove("delayedClock");
        let raw = serde_json::value::to_raw_value(&missing).map_err(invalid)?;
        assert!(prepare_delayed_clock(&raw, service, 1_048_576).is_err());
        let mut null = value;
        null["delayedClock"] = serde_json::Value::Null;
        let raw = serde_json::value::to_raw_value(&null).map_err(invalid)?;
        assert!(prepare_delayed_clock(&raw, service, 1_048_576).is_err());
        Ok(())
    }
}
