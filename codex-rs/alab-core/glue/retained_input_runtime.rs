// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Private Source-owned immutable original-answer publication. Descriptor/MAC
//! is bookkeeping; exact cells and accepted jobs supply real custody.
use super::*;
use crate::outgoing_message::alab_callback_outcomes::original_input::DispatchOutcome;
use crate::outgoing_message::alab_callback_outcomes::original_input::InputCell;
use data::input::InputDescriptor;
use data::input::InputReceipt;
use data::input::PublicationOutcome;
const MAX_INPUTS: usize = 256;
#[derive(Default)]
pub(super) struct Inputs {
    book: book::Book<Arc<Input>>,
}
struct Input {
    descriptor: InputDescriptor,
    capture_descriptor: data::Descriptor,
    cell: InputCell,
    serial: tokio::sync::Semaphore,
    progress: StdMutex<Progress>,
}
#[derive(Default)]
struct Progress {
    value: Option<serde_json::Value>,
    delivery: Option<PublicationOutcome>,
    receipt: Option<InputReceipt>,
    unknown: bool,
}
impl Progress {
    fn bind_value(&mut self, value: &serde_json::Value) -> Result<(), JSONRPCErrorError> {
        if self.unknown || self.value.as_ref().is_some_and(|prior| prior != value) {
            return Err(invalid_request(
                "original immutable answer changed or remains unknown",
            ));
        }
        self.value = Some(value.clone());
        Ok(())
    }
}
/// Actual new-origin admission must await the genuine original input cut
/// BEFORE selecting a previously unregistered value. No guard crosses await.
async fn bind_after_original_ingress(
    progress: &StdMutex<Progress>,
    original: impl std::future::Future<Output = ()>,
    value: &serde_json::Value,
) -> Result<(), JSONRPCErrorError> {
    original.await;
    progress
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .bind_value(value)
}
impl Inputs {
    pub(super) fn unobserved(&self) -> bool {
        self.book.active.values().any(|input| {
            input
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .unknown
        })
    }
}
fn descriptor_body(d: &InputDescriptor) -> (&str, u64, &str, &RequestId) {
    (&d.epoch, d.ordinal, &d.operation, &d.request_id)
}
fn verify_descriptor(auth: &auth::Auth, d: &InputDescriptor) -> Result<(), JSONRPCErrorError> {
    if !d.valid()
        || d.epoch != auth.epoch
        || !auth
            .verify("original-input-descriptor", &descriptor_body(d), &d.proof)
            .map_err(native_error)?
    {
        return Err(invalid_request(
            "original input descriptor is not from SAME Source",
        ));
    }
    Ok(())
}
fn verify_receipt(
    auth: &auth::Auth,
    receipt: &InputReceipt,
    value: &serde_json::Value,
) -> Result<(), JSONRPCErrorError> {
    verify_descriptor(auth, &receipt.descriptor)?;
    if !receipt.valid()
        || !auth
            .verify(
                "original-input-receipt",
                &(&receipt.descriptor, value, &receipt.outcome),
                &receipt.proof,
            )
            .map_err(native_error)?
    {
        return Err(invalid_request(
            "original input receipt/value is not from SAME Source",
        ));
    }
    Ok(())
}
impl Binding {
    pub(super) async fn accept_original_input(
        self: &Arc<Self>,
        context: RequestContext,
        params: ConfigReadParams,
        request: data::Request,
        config: ConfigRequestProcessor,
    ) -> Result<(), JSONRPCErrorError> {
        self.open()?;
        if !request.valid() {
            return Err(invalid_request("invalid original input selectors"));
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
                .ok_or_else(|| invalid_request("original input requires actual physical A"))?;
        let ingress = context
            .alab_input_ingress
            .as_ref()
            .ok_or_else(|| {
                invalid_request("original input lacks actual read/queued-dispatch lease")
            })?
            .clone();
        let (input, permit, token, dispatch, auth) = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed
                || self.input.is_stopped()
                || self.input.withdrawn().now_or_never().is_some()
            {
                return Err(invalid_request("original input admission closed"));
            }
            let callbacks = &mut state.callbacks;
            let permit = callbacks.admission.acquire(false)?;
            let auth = callbacks.auth.clone();
            let input = match &request {
                data::Request::PrepareInput {
                    descriptor,
                    operation,
                    request_id,
                    value,
                    previous,
                    ..
                } => {
                    callbacks.verify_descriptor(descriptor)?;
                    if let Some(previous) = previous {
                        verify_descriptor(&auth, previous)?;
                    }
                    if let Some(input) = callbacks
                        .inputs
                        .book
                        .operations
                        .get(operation)
                        .and_then(|id| callbacks.inputs.book.active.get(id))
                    {
                        if input.descriptor.request_id != *request_id
                            || input.capture_descriptor != *descriptor
                            || previous
                                .as_ref()
                                .is_some_and(|prior| prior != &input.descriptor)
                        {
                            return Err(invalid_request(
                                "original input operation arguments changed",
                            ));
                        }
                        if let Some(actual) = callbacks
                            .book
                            .active
                            .get(&descriptor.ordinal)
                            .filter(|ticket| ticket.descriptor == *descriptor)
                        {
                            let actual = actual
                                .prepared
                                .original_input(request_id)
                                .map_err(native_error)?;
                            if !input.cell.same_cell(&actual) {
                                return Err(invalid_request(
                                    "original input changed exact private callback cell",
                                ));
                            }
                        }
                        {
                            let mut progress = input
                                .progress
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner);
                            if Arc::ptr_eq(input.cell.original_origin(), &origin) {
                                progress.bind_value(value)?;
                            } else if progress.unknown
                                || progress.value.as_ref().is_some_and(|prior| prior != value)
                            {
                                return Err(invalid_request(
                                    "prepared original input value changed or unknown",
                                ));
                            }
                        }
                        Some(input.clone())
                    } else {
                        if previous.is_some() {
                            return Err(invalid_request(
                                "original input owner retired/unavailable; cannot mint cells",
                            ));
                        }
                        let ticket = callbacks
                            .book
                            .active
                            .get(&descriptor.ordinal)
                            .filter(|ticket| ticket.descriptor == *descriptor)
                            .ok_or_else(|| {
                                invalid_request(
                                    "original input requires active SAME captured cells",
                                )
                            })?;
                        let cell = ticket
                            .prepared
                            .original_input(request_id)
                            .map_err(native_error)?;
                        let ordinal = callbacks
                            .inputs
                            .book
                            .admit(MAX_INPUTS)
                            .map_err(invalid_request)?;
                        let capture_descriptor = descriptor.clone();
                        let mut descriptor = InputDescriptor {
                            epoch: auth.epoch.clone(),
                            ordinal,
                            operation: operation.clone(),
                            request_id: request_id.clone(),
                            proof: String::new(),
                        };
                        descriptor.proof = auth
                            .sign("original-input-descriptor", &descriptor_body(&descriptor))
                            .map_err(native_error)?;
                        let mut progress = Progress::default();
                        // Original accepted frame wins synchronously, before
                        // any Native query/worker scheduling. A new origin
                        // keeps only a provisional cell/op until real old drain.
                        if Arc::ptr_eq(cell.original_origin(), &origin) {
                            progress.bind_value(value)?;
                        }
                        let input = Arc::new(Input {
                            descriptor,
                            capture_descriptor,
                            cell,
                            serial: tokio::sync::Semaphore::new(1),
                            progress: StdMutex::new(progress),
                        });
                        callbacks
                            .inputs
                            .book
                            .insert(ordinal, operation.clone(), input.clone());
                        Some(input)
                    }
                }
                data::Request::PublishInput {
                    descriptor,
                    value,
                    previous,
                    ..
                } => {
                    verify_descriptor(&auth, descriptor)?;
                    if let Some(receipt) = previous {
                        verify_receipt(&auth, receipt, value)?;
                    }
                    match callbacks.inputs.book.active.get(&descriptor.ordinal) {
                        Some(input) if input.descriptor == *descriptor => {
                            let progress = input
                                .progress
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner);
                            if progress.unknown || progress.value.as_ref() != Some(value) {
                                return Err(invalid_request(
                                    "original input value not previously bound or changed",
                                ));
                            }
                            Some(input.clone())
                        }
                        None if callbacks.inputs.book.retired.contains(descriptor.ordinal)
                            && previous.is_some() =>
                        {
                            None
                        }
                        _ => {
                            return Err(invalid_request(
                                "original input private owner unavailable; absence is not NotAccepted",
                            ));
                        }
                    }
                }
                data::Request::RetireInput { receipt, value, .. } => {
                    verify_receipt(&auth, receipt, value)?;
                    match callbacks
                        .inputs
                        .book
                        .active
                        .get(&receipt.descriptor.ordinal)
                    {
                        Some(input) if input.descriptor == receipt.descriptor => {
                            Some(input.clone())
                        }
                        None if callbacks
                            .inputs
                            .book
                            .retired
                            .contains(receipt.descriptor.ordinal) =>
                        {
                            None
                        }
                        _ => {
                            return Err(invalid_request(
                                "original input retirement owner unavailable",
                            ));
                        }
                    }
                }
                _ => return Err(invalid_request("not an original input operation")),
            };
            let dispatch = ingress.dispatch().map_err(native_error)?;
            (input, permit, self.tracker.token(), dispatch, auth)
        };
        let guard = InputGuard {
            input: self.input.clone(),
            operation: input.clone(),
            _token: token,
            _dispatch: dispatch,
            _permit: permit,
            complete: false,
        };
        ingress.accepted(); // Original frame finishes only after SAME registered owner+pre-spawn guard exist.
        let binding = self.clone();
        tokio::spawn(async move {
            let mut guard = guard;
            // This SAME accepted owner is independent of the frontend waiter.
            // No context-cancellation select may discard an accepted publication.
            let outcome = async {
                let current = binding
                    .backend
                    .observe_reader_origin(&origin)
                    .await
                    .map_err(native_error)?;
                if !current.same_receipt(&origin) || current.service_data() != request.service() {
                    return Err(invalid_request("original input changed actual backend/A"));
                }
                binding.open()?;
                let response = config.read(params).await?;
                let reply = binding
                    .original_input_reply(input.as_ref(), &context, &origin, &auth, &request)
                    .await?;
                codex_app_server_protocol::alab_response_metadata::decorate(
                    response.into(),
                    data::RESULT_KEY,
                    serde_json::to_value(reply)
                        .map_err(|_| internal_error("original input reply encode failed"))?,
                )
                .map_err(|_| internal_error("original input result metadata occupied"))
            }
            .await;
            match outcome {
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
    async fn original_input_reply(
        &self,
        input: Option<&Arc<Input>>,
        context: &RequestContext,
        origin: &Arc<AcceptedOrigin>,
        auth: &auth::Auth,
        request: &data::Request,
    ) -> Result<data::Reply, JSONRPCErrorError> {
        let outgoing = &self.processor.outgoing;
        match request {
            data::Request::PrepareInput { value, .. } => {
                let input = input
                    .ok_or_else(|| invalid_request("original reservation owner unavailable"))?;
                if !Arc::ptr_eq(input.cell.original_origin(), origin) {
                    // Do not hold this operation's serial: an older original
                    // frame/job may still need it before the cut can finish.
                    observe_until_stop(
                        self.stopped(),
                        std::future::pending(),
                        bind_after_original_ingress(
                            &input.progress,
                            input.cell.drain_original(),
                            value,
                        ),
                    )
                    .await?;
                } else {
                    input
                        .progress
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .bind_value(value)?;
                }
                input
                    .cell
                    .observe(outgoing, &self.backend, context)
                    .await
                    .map_err(native_error)?;
                Ok(data::Reply::InputPrepared {
                    descriptor: input.descriptor.clone(),
                })
            }
            data::Request::PublishInput {
                value, previous, ..
            } => {
                let Some(input) = input else {
                    return Ok(data::Reply::InputPublished {
                        receipt: previous.clone().ok_or_else(|| {
                            invalid_request("retired original input needs actual receipt")
                        })?,
                    });
                };
                let prior = {
                    let progress = input
                        .progress
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner);
                    if progress.unknown {
                        return Err(invalid_request("original publication outcome unknown"));
                    }
                    progress.receipt.clone()
                };
                if let Some(receipt) = prior {
                    return Ok(data::Reply::InputPublished { receipt });
                }
                if !Arc::ptr_eq(input.cell.original_origin(), origin) {
                    // A never-reached/missing original publication is not a
                    // negative. Join actual old reader+RPC+accepted-dispatch
                    // owners BEFORE resuming SAME immutable operation.
                    observe_until_stop(self.stopped(), std::future::pending(), async {
                        input.cell.drain_original().await;
                        Ok(())
                    })
                    .await?;
                }
                let _serial = input
                    .serial
                    .acquire()
                    .await
                    .map_err(|_| internal_error("original publication lease unavailable"))?;
                let prior = {
                    let progress = input
                        .progress
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner);
                    if progress.unknown {
                        return Err(invalid_request("original publication outcome unknown"));
                    }
                    progress.receipt.clone()
                };
                if let Some(receipt) = prior {
                    return Ok(data::Reply::InputPublished { receipt });
                }
                input
                    .cell
                    .observe(outgoing, &self.backend, context)
                    .await
                    .map_err(native_error)?;
                self.open()?;
                let known = {
                    input
                        .progress
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .delivery
                        .clone()
                };
                let outcome = if let Some(known) = known {
                    known
                } else {
                    let actual = match input
                        .cell
                        .publish(
                            outgoing,
                            context.alab_request_id().connection_id,
                            value.clone(),
                        )
                        .await
                        .map_err(native_error)?
                    {
                        DispatchOutcome::Published => PublicationOutcome::Published {},
                        DispatchOutcome::ReceiverClosed => PublicationOutcome::ReceiverClosed {},
                        DispatchOutcome::SealedClosed => PublicationOutcome::SealedClosed {},
                        DispatchOutcome::Unknown => {
                            input
                                .progress
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .unknown = true;
                            self.input.fail(std::io::Error::other(
                                "original private callback send outcome remains unknown",
                            ));
                            return Err(invalid_request("original publication remains unknown"));
                        }
                    };
                    // Retain the ACTUAL send outcome before any fallible
                    // signing/encoding step; retry never substitutes Sealed.
                    input
                        .progress
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .delivery = Some(actual.clone());
                    actual
                };
                let mut receipt = InputReceipt {
                    descriptor: input.descriptor.clone(),
                    outcome,
                    proof: String::new(),
                };
                receipt.proof = auth
                    .sign(
                        "original-input-receipt",
                        &(&receipt.descriptor, value, &receipt.outcome),
                    )
                    .map_err(native_error)?;
                {
                    let mut progress = input
                        .progress
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner);
                    if progress.unknown {
                        return Err(invalid_request("original publication guard lost outcome"));
                    }
                    progress.receipt = Some(receipt.clone());
                }
                Ok(data::Reply::InputPublished { receipt })
            }
            data::Request::RetireInput { receipt, value, .. } => {
                if let Some(input) = input {
                    let _serial = input
                        .serial
                        .acquire()
                        .await
                        .map_err(|_| internal_error("original retirement lease unavailable"))?;
                    {
                        let progress = input
                            .progress
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner);
                        if progress.unknown
                            || progress.value.as_ref() != Some(value)
                            || progress.receipt.as_ref() != Some(receipt)
                        {
                            return Err(invalid_request(
                                "original input retirement lacks actual terminal same-value result",
                            ));
                        }
                    }
                    let retired = {
                        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                        state
                            .callbacks
                            .inputs
                            .book
                            .retire(receipt.descriptor.ordinal, &receipt.descriptor.operation)
                    };
                    drop(retired); // Original values/cells drop outside Source locks.
                }
                Ok(data::Reply::InputRetired {
                    receipt: receipt.clone(),
                })
            }
            _ => Err(invalid_request("not an original input reply")),
        }
    }
}
struct InputGuard {
    input: Incoming,
    operation: Option<Arc<Input>>,
    _token: tokio_util::task::task_tracker::TaskTrackerToken,
    _dispatch: tokio_util::task::task_tracker::TaskTrackerToken,
    _permit: tokio::sync::OwnedSemaphorePermit,
    complete: bool,
}
impl Drop for InputGuard {
    fn drop(&mut self) {
        if !self.complete {
            if let Some(operation) = &self.operation {
                operation
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .unknown = true;
            }
            self.input.fail(std::io::Error::other(
                "accepted original input worker lost its outcome",
            ));
        }
    }
}

#[cfg(test)]
#[path = "retained_input_runtime_tests.rs"]
mod tests;
