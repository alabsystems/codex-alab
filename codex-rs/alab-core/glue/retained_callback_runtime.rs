// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Private Source endpoint owners. No scheduling or Native permission is issued.
use super::*;
use crate::outgoing_message::alab_callback_claim::metadata as data;
use crate::outgoing_message::alab_callback_outcomes::PreparedCapture;
use crate::request_processors::config_processor::ConfigRequestProcessor;
use codex_app_server_protocol::ConfigReadParams;
use codex_app_server_protocol::RequestId;
#[path = "retained_callback_auth.rs"]
mod auth;
#[path = "retained_callback_book.rs"]
mod book;
const MAX_TICKETS: usize = 128;
const MAX_OBSERVERS: usize = 128;
pub(super) struct CallbackState {
    auth: Arc<auth::Auth>,
    book: book::Book<Arc<Ticket>>,
    admission: Admission,
    inputs: original_input::Inputs,
}
impl Default for CallbackState {
    fn default() -> Self {
        Self {
            auth: Arc::new(auth::Auth::new()),
            book: book::Book::default(),
            admission: Admission::new(MAX_OBSERVERS, 64),
            inputs: original_input::Inputs::default(),
        }
    }
}
struct Ticket {
    descriptor: data::Descriptor,
    original: Arc<AcceptedOrigin>,
    prepared: PreparedCapture,
    serial: tokio::sync::Semaphore,
    cut: StdMutex<Cut>,
}
#[derive(Default)]
struct Cut {
    sequence: u64,
    latest: Option<data::Observation>,
    acknowledged: Vec<RequestId>,
    uncertain: bool,
}
impl CallbackState {
    pub(super) fn frontier_changed(&self) {
        self.book.notify_change();
    }
    pub(super) fn unobserved(&self) -> bool {
        self.inputs.unobserved()
            || self.book.active.values().any(|ticket| {
                ticket
                    .cut
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .uncertain
            })
    }
    fn verify_descriptor(&self, d: &data::Descriptor) -> Result<(), JSONRPCErrorError> {
        if !d.valid()
            || d.epoch != self.auth.epoch
            || !self
                .auth
                .verify(
                    "descriptor",
                    &(d.epoch.as_str(), d.ordinal, d.operation.as_str(), &d.ids),
                    &d.proof,
                )
                .map_err(native_error)?
        {
            return Err(invalid_request(
                "callback descriptor is not from this Source owner",
            ));
        }
        Ok(())
    }
    fn verify_observation(&self, o: &data::Observation) -> Result<(), JSONRPCErrorError> {
        if !o.valid()
            || !self
                .auth
                .verify(
                    "observation",
                    &(&o.descriptor, &o.projection, o.sequence, &o.content),
                    &o.proof,
                )
                .map_err(native_error)?
        {
            return Err(invalid_request(
                "callback outcome cut is not from this Source owner",
            ));
        }
        Ok(())
    }
    fn observation(
        &self,
        d: &data::Descriptor,
        p: &data::Projection,
        sequence: u64,
        content: data::Content,
    ) -> Result<data::Observation, JSONRPCErrorError> {
        make_observation(&self.auth, d, p, sequence, content)
    }
}
impl Binding {
    async fn accept_callbacks(
        self: &Arc<Self>,
        context: RequestContext,
        params: ConfigReadParams,
        request: data::Request,
        config: ConfigRequestProcessor,
    ) -> Result<(), JSONRPCErrorError> {
        if matches!(
            &request,
            data::Request::PrepareInput { .. }
                | data::Request::PublishInput { .. }
                | data::Request::RetireInput { .. }
        ) {
            return self
                .accept_original_input(context, params, request, config)
                .await;
        }
        self.open()?;
        if !request.valid() {
            return Err(invalid_request("invalid callback selectors"));
        }
        let origin =
            context
                .alab_connection_resource
                .as_ref()
                .and_then(
                    codex_alab_core::connection_resource::ConnectionResource::downcast::<
                        AcceptedOrigin,
                    >,
                )
                .ok_or_else(|| invalid_request("callback observation needs actual A resource"))?;
        // Reserve bounded observer+exact cells synchronously, BEFORE any await. No
        // cancellation or C1 EOF can replace those cells after this accepted point.
        let (ticket, permit, token) = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed
                || self.input.is_stopped()
                || self.input.withdrawn().now_or_never().is_some()
            {
                return Err(invalid_request("callback admission closed"));
            }
            let callbacks = &mut state.callbacks;
            let permit = callbacks
                .admission
                .acquire(matches!(&request, data::Request::Observe { .. }))?;
            let ticket = match &request {
                data::Request::PrepareInput { .. }
                | data::Request::PublishInput { .. }
                | data::Request::RetireInput { .. } => {
                    return Err(invalid_request(
                        "original input must use its dedicated accepted owner",
                    ));
                }
                data::Request::Capture {
                    operation,
                    ids,
                    previous,
                    ..
                } => {
                    if let Some(previous) = previous {
                        callbacks.verify_descriptor(previous)?;
                        match callbacks.book.active.get(&previous.ordinal) {
                            Some(ticket) if &ticket.descriptor == previous => Some(ticket.clone()),
                            None if callbacks.book.retired.contains(previous.ordinal) => None,
                            _ => {
                                return Err(invalid_request(
                                    "restored callback ticket unavailable",
                                ));
                            }
                        }
                    } else if let Some(existing) = callbacks
                        .book
                        .operations
                        .get(operation)
                        .and_then(|id| callbacks.book.active.get(id))
                    {
                        if &existing.descriptor.ids != ids {
                            return Err(invalid_request("callback operation arguments changed"));
                        }
                        Some(existing.clone())
                    } else {
                        let ordinal = callbacks.book.admit(MAX_TICKETS).map_err(invalid_request)?;
                        let prepared = self
                            .processor
                            .outgoing
                            .alab_prepare_callback_capture(&self.backend, &context, ids)
                            .map_err(native_error)?;
                        let mut descriptor = data::Descriptor {
                            epoch: callbacks.auth.epoch.clone(),
                            ordinal,
                            operation: operation.clone(),
                            ids: ids.clone(),
                            proof: String::new(),
                        };
                        descriptor.proof = callbacks
                            .auth
                            .sign(
                                "descriptor",
                                &(descriptor.epoch.as_str(), ordinal, operation.as_str(), ids),
                            )
                            .map_err(native_error)?;
                        let ticket = Arc::new(Ticket {
                            descriptor,
                            original: origin.clone(),
                            prepared,
                            serial: tokio::sync::Semaphore::new(1),
                            cut: StdMutex::new(Cut::default()),
                        });
                        callbacks
                            .book
                            .insert(ordinal, operation.clone(), ticket.clone());
                        Some(ticket)
                    }
                }
                data::Request::Observe { descriptor, .. }
                | data::Request::Acknowledge { descriptor, .. }
                | data::Request::AcknowledgeLive { descriptor, .. }
                | data::Request::ObserveConfigured { descriptor, .. }
                | data::Request::AcknowledgeConfigured { descriptor, .. }
                | data::Request::ObserveClosed { descriptor, .. }
                | data::Request::AcknowledgeClosed { descriptor, .. } => {
                    callbacks.verify_descriptor(descriptor)?;
                    match callbacks.book.active.get(&descriptor.ordinal) {
                        Some(ticket) if &ticket.descriptor == descriptor => Some(ticket.clone()),
                        None if callbacks.book.retired.contains(descriptor.ordinal) => None,
                        _ => return Err(invalid_request("exact callback ticket unavailable")),
                    }
                }
            };
            (ticket, permit, self.tracker.token())
        };
        let guard = CallbackGuard {
            input: self.input.clone(),
            ticket: ticket.clone(),
            _token: token,
            _permit: permit,
            complete: false,
        };
        let binding = self.clone();
        tokio::spawn(async move {
            let mut guard = guard;
            let outcome = async {
                let observer = if matches!(&request, data::Request::Observe { .. }) {
                    Some(
                        binding
                            .processor
                            .outgoing
                            .alab_callback_observer(&context)
                            .await
                            .map_err(native_error)?,
                    )
                } else {
                    None
                };
                let body = async {
                    let current = binding
                        .backend
                        .observe_reader_origin(&origin)
                        .await
                        .map_err(native_error)?;
                    if !current.same_receipt(&origin) || current.service_data() != request.service()
                    {
                        return Err(invalid_request(
                            "callback observation changed actual backend",
                        ));
                    }
                    binding.open()?;
                    // Preserve original typed config policy/error behavior, not a Raw response.
                    let response = config.read(params).await?;
                    let reply = binding
                        .callback_reply(ticket.as_ref(), &context, &origin, &request)
                        .await?;
                    binding.open()?;
                    codex_app_server_protocol::alab_response_metadata::decorate(
                        response.into(),
                        data::RESULT_KEY,
                        serde_json::to_value(reply)
                            .map_err(|_| internal_error("callback encode failed"))?,
                    )
                    .map_err(|_| internal_error("callback metadata occupied"))
                };
                if let Some(observer) = observer {
                    tokio::select! {biased;()=observer.cancelled()=>Err(invalid_request("actual callback connection closed")),result=body=>result}
                } else {
                    body.await
                }
            };
            let result =
                observe_until_stop(binding.stopped(), context.cancellation.cancelled(), outcome)
                    .await;
            match result {
                Ok(payload) => {
                    binding
                        .processor
                        .outgoing
                        .send_response(context.alab_request_id().clone(), payload)
                        .await
                }
                Err(error) => {
                    binding
                        .processor
                        .outgoing
                        .send_error(context.alab_request_id().clone(), error)
                        .await
                }
            }
            guard.complete = true;
        });
        Ok(())
    }
    async fn callback_projection(
        &self,
        p: &data::Projection,
        origin: &AcceptedOrigin,
    ) -> Result<Arc<Projection>, JSONRPCErrorError> {
        let work = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .projections
            .get(&p.snapshot)
            .cloned()
            .ok_or_else(|| invalid_request("callback projection unavailable"))?;
        if work.receipt.frontier != p.frontier
            || work.receipt.thread_id != p.thread_id
            || !work.frontier.same_receipt(origin)
        {
            return Err(invalid_request(
                "callback projection changed physical delivery",
            ));
        }
        work.wait_ready(&self.input).await?; // actual original replay AND typed tail
        let current = self
            .backend
            .observe_reader_origin(origin)
            .await
            .map_err(native_error)?;
        if !current.same_receipt(origin) {
            return Err(invalid_request("callback projection lost actual current A"));
        }
        work.primary.recheck().await?;
        self.current(&work)?;
        Ok(work)
    }
    fn callback_committed(&self, work: &Arc<Projection>) -> Result<(), JSONRPCErrorError> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if !state
            .activations
            .get(&work.receipt.snapshot)
            .is_some_and(|actual| {
                Arc::ptr_eq(&actual.projection, work)
                    && !actual.uncertain.load(Ordering::Acquire)
                    && matches!(actual.result.borrow().as_ref(), Some(Ok(())))
            })
        {
            return Err(invalid_request(
                "callback semantic ACK needs actual committed Source activation",
            ));
        }
        Ok(())
    }
    async fn callback_reply(
        &self,
        ticket: Option<&Arc<Ticket>>,
        context: &RequestContext,
        origin: &AcceptedOrigin,
        request: &data::Request,
    ) -> Result<data::Reply, JSONRPCErrorError> {
        let outgoing = &self.processor.outgoing;
        match request {
            data::Request::PrepareInput { .. }
            | data::Request::PublishInput { .. }
            | data::Request::RetireInput { .. } => Err(invalid_request(
                "original input cannot use endpoint-only callback reply",
            )),
            data::Request::ObserveClosed {
                descriptor,
                projection,
                ids,
                ..
            } => {
                self.observe_closed_callbacks(ticket, context, origin, descriptor, projection, ids)
                    .await
            }
            data::Request::AcknowledgeClosed {
                descriptor,
                observation,
                ids,
                ..
            } => {
                self.acknowledge_closed_callbacks(
                    ticket,
                    context,
                    origin,
                    descriptor,
                    observation,
                    ids,
                )
                .await
            }
            data::Request::AcknowledgeLive {
                descriptor, ids, ..
            } => {
                self.acknowledge_live_callbacks(ticket, context, origin, descriptor, ids)
                    .await
            }
            data::Request::ObserveConfigured {
                descriptor,
                projection,
                ..
            } => {
                self.observe_configured_callbacks(ticket, context, origin, descriptor, projection)
                    .await
            }
            data::Request::AcknowledgeConfigured {
                descriptor,
                observation,
                ids,
                ..
            } => {
                self.acknowledge_configured_callbacks(
                    ticket,
                    context,
                    origin,
                    descriptor,
                    observation,
                    ids,
                )
                .await
            }
            data::Request::Capture { previous, .. } => {
                let Some(ticket) = ticket else {
                    let descriptor = previous.as_ref().ok_or_else(|| {
                        invalid_request("capture cannot reconstruct a retired owner")
                    })?;
                    // A genuine authenticated retired descriptor returns bookkeeping only.
                    return Ok(data::Reply::Captured {
                        descriptor: descriptor.clone(),
                    });
                };
                let _capture = ticket
                    .prepared
                    .observe_current_for(outgoing, &self.backend, context)
                    .await
                    .map_err(native_error)?;
                Ok(data::Reply::Captured {
                    descriptor: ticket.descriptor.clone(),
                })
            }
            data::Request::Observe {
                descriptor,
                projection,
                after,
                ..
            } => {
                let work = self.callback_projection(projection, origin).await?;
                let Some(ticket) = ticket else {
                    let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                    return Ok(data::Reply::Observed {
                        observation: state.callbacks.observation(
                            descriptor,
                            projection,
                            after
                                .checked_add(1)
                                .ok_or_else(|| invalid_request("callback sequence exhausted"))?,
                            data::Content::Retired {},
                        )?,
                    });
                };
                // This is an observer lease, not the SDK scheduling mutation semaphore.
                let capture = ticket
                    .prepared
                    .observe_current_for(outgoing, &self.backend, context)
                    .await
                    .map_err(native_error)?;
                loop {
                    // Arm actual Book retirement BEFORE checking it. A notice is only a wake;
                    // the private interval, descriptor and fresh projection remain required.
                    let (mut retired_watch, retired) = {
                        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                        (
                            state.callbacks.book.subscribe(),
                            state.callbacks.book.retired.contains(descriptor.ordinal),
                        )
                    };
                    if retired {
                        self.callback_projection(projection, origin).await?;
                        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                        return Ok(data::Reply::Observed {
                            observation: state.callbacks.observation(
                                descriptor,
                                projection,
                                after.checked_add(1).ok_or_else(|| {
                                    invalid_request("callback sequence exhausted")
                                })?,
                                data::Content::Retired {},
                            )?,
                        });
                    }
                    let serial = ticket
                        .serial
                        .acquire()
                        .await
                        .map_err(|_| internal_error("callback observer lease unavailable"))?;
                    let mut watches = capture.watch_for(outgoing).map_err(native_error)?; // before cut
                    let facts = capture.facts_for(outgoing).await.map_err(native_error)?;
                    let facts = endpoint_facts(facts);
                    if facts.iter().any(|fact| {
                        fact.thread_id.as_deref() != Some(work.receipt.thread_id.as_str())
                    }) {
                        return Err(invalid_request(
                            "callback facts do not belong to this projected thread",
                        ));
                    }
                    self.current(&work)?;
                    let auth = {
                        self.state
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .callbacks
                            .auth
                            .clone()
                    };
                    let ready = {
                        let mut cut = ticket.cut.lock().unwrap_or_else(PoisonError::into_inner);
                        if cut.uncertain {
                            return Err(invalid_request("callback worker outcome remains unknown"));
                        }
                        if *after > cut.sequence {
                            return Err(invalid_request("callback sequence was never issued"));
                        }
                        let content = data::Content::Current { facts };
                        if cut.latest.as_ref().is_none_or(|last| {
                            last.projection != *projection || last.content != content
                        }) {
                            cut.sequence = cut
                                .sequence
                                .checked_add(1)
                                .ok_or_else(|| internal_error("callback sequence exhausted"))?;
                            cut.latest = Some(make_observation(
                                &auth,
                                descriptor,
                                projection,
                                cut.sequence,
                                content,
                            )?);
                        }
                        cut.latest
                            .as_ref()
                            .filter(|latest| latest.sequence > *after)
                            .cloned()
                    };
                    drop(serial); // a long-poll never blocks semantic retirement
                    if let Some(observation) = ready {
                        // Retirement may win after the cut's serial lease was released. Recheck
                        // its real interval before publishing stale Pending view DATA.
                        if self
                            .state
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .callbacks
                            .book
                            .retired
                            .contains(descriptor.ordinal)
                        {
                            continue;
                        }
                        let actual = self.callback_projection(projection, origin).await?;
                        if !Arc::ptr_eq(&actual, &work) {
                            return Err(invalid_request("callback projection resource changed"));
                        }
                        return Ok(data::Reply::Observed { observation });
                    }
                    if watches.is_empty() {
                        return Err(invalid_request(
                            "empty callback capture has no later outcome",
                        ));
                    }
                    // Every event future borrows its actual original cell; outer stop/cancel
                    // select ends only this observer, leaving the ticket/ledger owned.
                    tokio::select! {biased;
                     changed=retired_watch.changed()=>changed.map_err(|_|internal_error("callback retirement observer unavailable"))?,
                     changed=futures::future::select_all(watches.iter_mut().map(|watch|Box::pin(watch.changed())))=>{changed.0.map_err(native_error)?;},
                    }
                }
            }
            data::Request::Acknowledge {
                descriptor,
                observation,
                ids,
                ..
            } => {
                {
                    let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                    state.callbacks.verify_observation(observation)?;
                }
                let Some(ticket) = ticket else {
                    return Ok(data::Reply::Acknowledged {
                        descriptor: descriptor.clone(),
                        sequence: observation.sequence,
                        ids: ids.clone(),
                        retired: true,
                    });
                };
                let _serial = ticket
                    .serial
                    .acquire()
                    .await
                    .map_err(|_| internal_error("callback observer lease unavailable"))?;
                let work = self
                    .callback_projection(&observation.projection, origin)
                    .await?;
                self.callback_committed(&work)?;
                let data::Content::Current { facts } = &observation.content else {
                    return Err(invalid_request(
                        "active callback cannot use retired observation",
                    ));
                };
                if !ids.iter().all(|id| {
                    facts
                        .iter()
                        .any(|fact| &fact.request_id == id && closed_outcome(&fact.outcome))
                }) {
                    return Err(invalid_request(
                        "semantic ACK lacks exact closed endpoint facts",
                    ));
                }
                let capture = ticket
                    .prepared
                    .observe_current_for(outgoing, &self.backend, context)
                    .await
                    .map_err(native_error)?;
                capture
                    .acknowledge_for(outgoing, &self.backend, context, ids)
                    .await
                    .map_err(native_error)?;
                work.primary.recheck().await?;
                self.current(&work)?;
                self.callback_committed(&work)?;
                let retired = {
                    let mut cut = ticket.cut.lock().unwrap_or_else(PoisonError::into_inner);
                    if cut.uncertain {
                        return Err(invalid_request("callback worker outcome remains unknown"));
                    }
                    for id in ids {
                        if !cut.acknowledged.contains(id) {
                            cut.acknowledged.push(id.clone());
                        }
                    }
                    cut.acknowledged.len() == descriptor.ids.len()
                };
                let removed = if retired {
                    let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                    state
                        .callbacks
                        .book
                        .retire(descriptor.ordinal, &descriptor.operation)
                } else {
                    None
                };
                drop(removed); // exact cells/resources are destroyed outside Source locks
                Ok(data::Reply::Acknowledged {
                    descriptor: descriptor.clone(),
                    sequence: observation.sequence,
                    ids: ids.clone(),
                    retired,
                })
            }
        }
    }
}
fn endpoint_facts(
    facts: Vec<crate::outgoing_message::alab_callback_outcomes::Fact>,
) -> Vec<data::Fact> {
    facts
        .into_iter()
        .map(|fact| {
            use crate::outgoing_message::alab_callback_outcomes::Outcome as O;
            data::Fact {
                request_id: fact.request_id_data().clone(),
                thread_id: fact.thread_id_data().map(|id| id.to_string()),
                outcome: match fact.outcome_data() {
                    O::Pending {} => data::Outcome::Pending {},
                    O::Response {} => data::Outcome::Response {},
                    O::ClientError {} => data::Outcome::ClientError {},
                    O::Cancelled {} => data::Outcome::Cancelled {},
                    O::Withdrawn {} => data::Outcome::Withdrawn {},
                    O::Unknown {} => data::Outcome::Unknown {},
                },
            }
        })
        .collect()
}
struct CallbackGuard {
    input: Incoming,
    ticket: Option<Arc<Ticket>>,
    _token: tokio_util::task::task_tracker::TaskTrackerToken,
    _permit: tokio::sync::OwnedSemaphorePermit,
    complete: bool,
}
impl Drop for CallbackGuard {
    fn drop(&mut self) {
        if !self.complete {
            if let Some(ticket) = &self.ticket {
                ticket
                    .cut
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .uncertain = true;
            }
            self.input.fail(std::io::Error::other(
                "accepted callback worker outcome remains unobserved",
            ));
        }
    }
}
impl ThreadRequestProcessor {
    pub(crate) async fn alab_retained_callbacks(
        &self,
        context: RequestContext,
        params: ConfigReadParams,
        request: data::Request,
        config: ConfigRequestProcessor,
    ) -> Result<(), JSONRPCErrorError> {
        self.alab_retained_frontier
            .actual()?
            .accept_callbacks(context, params, request, config)
            .await
    }
}

fn make_observation(
    auth: &auth::Auth,
    d: &data::Descriptor,
    p: &data::Projection,
    sequence: u64,
    content: data::Content,
) -> Result<data::Observation, JSONRPCErrorError> {
    let proof = auth
        .sign("observation", &(d, p, sequence, &content))
        .map_err(native_error)?;
    Ok(data::Observation {
        descriptor: d.clone(),
        projection: p.clone(),
        sequence,
        content,
        proof,
    })
}
fn closed_outcome(outcome: &data::Outcome) -> bool {
    matches!(
        outcome,
        data::Outcome::Response {}
            | data::Outcome::ClientError {}
            | data::Outcome::Cancelled {}
            | data::Outcome::Withdrawn {}
    )
}
/// Ends only an observation; accepted ticket/endpoint custody stays owned.
async fn observe_until_stop<T>(
    stopped: impl std::future::Future<Output = ()>,
    cancelled: impl std::future::Future<Output = ()>,
    body: impl std::future::Future<Output = Result<T, JSONRPCErrorError>>,
) -> Result<T, JSONRPCErrorError> {
    tokio::select! {biased;()=stopped=>Err(invalid_request("callback observer stopped")),()=cancelled=>Err(invalid_request("callback observer cancelled")),result=body=>result}
}
#[cfg(test)]
#[path = "retained_callback_runtime_tests.rs"]
mod tests;

#[path = "retained_callback_ack_routes.rs"]
mod ack_routes;

struct Admission {
    observers: Arc<tokio::sync::Semaphore>,
    controls: Arc<tokio::sync::Semaphore>,
}
impl Admission {
    fn new(observers: usize, controls: usize) -> Self {
        Self {
            observers: Arc::new(tokio::sync::Semaphore::new(observers)),
            controls: Arc::new(tokio::sync::Semaphore::new(controls)),
        }
    }
    fn acquire(
        &self,
        observer: bool,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, JSONRPCErrorError> {
        (if observer {
            self.observers.clone()
        } else {
            self.controls.clone()
        })
        .try_acquire_owned()
        .map_err(|_| invalid_request("callback admission capacity unavailable"))
    }
}

#[path = "retained_input_runtime.rs"]
mod original_input;
