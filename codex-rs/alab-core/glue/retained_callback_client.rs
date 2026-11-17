// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Source endpoint bookkeeping over the actual Remote channel. Checkpoints,
//! descriptors and outcome facts cannot issue Native/App/Core/effect authority.
use super::*;
use codex_app_server_protocol::JSONRPCRequest;
use codex_app_server_protocol::RequestId;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
#[path = "retained_callback_metadata.rs"]
mod metadata;
pub use metadata::Fact;
pub use metadata::Outcome;
impl Fact {
    pub fn request_id_data(&self) -> &RequestId {
        &self.request_id
    }
    pub fn thread_id_data(&self) -> Option<&str> {
        self.thread_id.as_deref()
    }
    pub fn outcome_data(&self) -> &Outcome {
        &self.outcome
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Checkpoint {
    schema: u8,
    service: String,
    operation: String,
    ids: Vec<RequestId>,
    descriptor: Option<metadata::Descriptor>,
}
/// Store this SAME owner before polling capture. Checkpoint before the request
/// survives a lost reply; restoring is DATA, reauthorized by the actual Source.
pub struct CallbackCaptureOperation {
    state: Mutex<Checkpoint>,
}
impl CallbackCaptureOperation {
    pub fn new(service: &str, ids: Vec<RequestId>) -> Result<Arc<Self>, TypedRequestError> {
        let state = Checkpoint {
            schema: 1,
            service: service.into(),
            operation: format!(
                "{:016x}{:016x}",
                codex_alab_core::native_source::connection_request_prefix(),
                codex_alab_core::native_source::connection_request_prefix()
            ),
            ids,
            descriptor: None,
        };
        Self::from_checkpoint(state)
    }
    fn from_checkpoint(state: Checkpoint) -> Result<Arc<Self>, TypedRequestError> {
        if state.schema != 1
            || !metadata::token(&state.service)
            || !metadata::token(&state.operation)
            || !metadata::ids_valid(&state.ids)
            || state
                .descriptor
                .as_ref()
                .is_some_and(|d| !d.valid() || d.operation != state.operation || d.ids != state.ids)
        {
            return Err(invalid("invalid callback checkpoint"));
        }
        Ok(Arc::new(Self {
            state: Mutex::new(state),
        }))
    }
    pub fn restore_checkpoint(value: serde_json::Value) -> Result<Arc<Self>, TypedRequestError> {
        Self::from_checkpoint(serde_json::from_value(value).map_err(decode_error)?)
    }
    pub fn checkpoint(&self) -> Result<serde_json::Value, TypedRequestError> {
        serde_json::to_value(&*self.state.lock().unwrap_or_else(PoisonError::into_inner))
            .map_err(decode_error)
    }
    pub async fn capture(
        self: &Arc<Self>,
        handle: &AppServerRequestHandle,
        original: ClientRequest,
    ) -> Result<CallbackCapture, TypedRequestError> {
        let request = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            metadata::Request::Capture {
                service: state.service.clone(),
                operation: state.operation.clone(),
                ids: state.ids.clone(),
                previous: state.descriptor.clone(),
            }
        };
        let reply = send(handle, original, &request).await?;
        let metadata::Reply::Captured { descriptor } = reply else {
            return Err(invalid("callback capture reply kind changed"));
        };
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if !descriptor.valid()
            || descriptor.operation != state.operation
            || descriptor.ids != state.ids
            || state
                .descriptor
                .as_ref()
                .is_some_and(|prior| prior != &descriptor)
        {
            return Err(invalid("callback capture reply identity changed"));
        }
        state.descriptor = Some(descriptor.clone());
        Ok(CallbackCapture {
            operation: self.clone(),
            descriptor,
        })
    }
}
/// Contains a Source descriptor, never a substitute for Source's exact cells.
#[derive(Clone)]
pub struct CallbackCapture {
    operation: Arc<CallbackCaptureOperation>,
    descriptor: metadata::Descriptor,
}
#[derive(Clone)]
pub struct CallbackObservation {
    observation: metadata::Observation,
}
impl CallbackObservation {
    pub fn facts(&self) -> Option<&[Fact]> {
        match &self.observation.content {
            metadata::Content::Current { facts } => Some(facts),
            metadata::Content::Retired {} => None,
        }
    }
    /// True means only that this exact Source ticket was semantically retired.
    pub fn ticket_retired(&self) -> bool {
        matches!(self.observation.content, metadata::Content::Retired {})
    }
    pub fn sequence_data(&self) -> u64 {
        self.observation.sequence
    }
}
pub struct CallbackRetirement {
    ids: Vec<RequestId>,
    retired: bool,
}
impl CallbackRetirement {
    pub fn request_ids_data(&self) -> &[RequestId] {
        &self.ids
    }
    pub fn ticket_retired(&self) -> bool {
        self.retired
    }
}
impl CallbackCapture {
    pub fn checkpoint(&self) -> Result<serde_json::Value, TypedRequestError> {
        self.operation.checkpoint()
    }
    pub fn request_ids_data(&self) -> &[RequestId] {
        &self.descriptor.ids
    }
    pub async fn observe_runtime(
        &self,
        handle: &AppServerRequestHandle,
        original: ClientRequest,
        receipt: &crate::retained_frontier::Receipt,
        after: Option<&CallbackObservation>,
    ) -> Result<CallbackObservation, TypedRequestError> {
        self.observe(
            handle,
            original,
            projection(
                receipt.frontier_data(),
                receipt.snapshot_data(),
                receipt.thread_id_data(),
            ),
            after,
        )
        .await
    }
    pub async fn observe_source(
        &self,
        handle: &AppServerRequestHandle,
        original: ClientRequest,
        receipt: &crate::source_selection::Receipt,
        after: Option<&CallbackObservation>,
    ) -> Result<CallbackObservation, TypedRequestError> {
        self.observe(
            handle,
            original,
            projection(
                receipt.frontier_data(),
                receipt.snapshot_data(),
                receipt.thread_id_data(),
            ),
            after,
        )
        .await
    }
    async fn observe(
        &self,
        handle: &AppServerRequestHandle,
        original: ClientRequest,
        projection: metadata::Projection,
        after: Option<&CallbackObservation>,
    ) -> Result<CallbackObservation, TypedRequestError> {
        if after.is_some_and(|prior| prior.observation.descriptor != self.descriptor) {
            return Err(invalid("callback continuation changed capture"));
        }
        let request = metadata::Request::Observe {
            service: self.service(),
            descriptor: self.descriptor.clone(),
            projection: projection.clone(),
            after: after.map_or(0, |prior| prior.observation.sequence),
        };
        let metadata::Reply::Observed { observation } = send(handle, original, &request).await?
        else {
            return Err(invalid("callback observation reply kind changed"));
        };
        if !observation.valid()
            || observation.descriptor != self.descriptor
            || observation.projection != projection
            || after.is_some_and(|prior| observation.sequence <= prior.observation.sequence)
        {
            return Err(invalid("callback outcome cut correlation changed"));
        }
        Ok(CallbackObservation { observation })
    }
    /// Caller invokes only after the genuine App has reconciled these exact views.
    /// Activation is the actual typed Source commit, never caller labels or IDs.
    pub async fn acknowledge_runtime(
        &self,
        handle: &AppServerRequestHandle,
        original: ClientRequest,
        observation: &CallbackObservation,
        activation: &crate::retained_frontier::Activation,
        ids: Vec<RequestId>,
    ) -> Result<CallbackRetirement, TypedRequestError> {
        let r = activation.receipt();
        self.acknowledge(
            handle,
            original,
            observation,
            projection(r.frontier_data(), r.snapshot_data(), r.thread_id_data()),
            ids,
        )
        .await
    }
    pub async fn acknowledge_source(
        &self,
        handle: &AppServerRequestHandle,
        original: ClientRequest,
        observation: &CallbackObservation,
        activation: &crate::source_selection::Activation,
        ids: Vec<RequestId>,
    ) -> Result<CallbackRetirement, TypedRequestError> {
        let r = activation.receipt();
        self.acknowledge(
            handle,
            original,
            observation,
            projection(r.frontier_data(), r.snapshot_data(), r.thread_id_data()),
            ids,
        )
        .await
    }
    async fn acknowledge(
        &self,
        handle: &AppServerRequestHandle,
        original: ClientRequest,
        observation: &CallbackObservation,
        projection: metadata::Projection,
        ids: Vec<RequestId>,
    ) -> Result<CallbackRetirement, TypedRequestError> {
        if observation.observation.descriptor != self.descriptor
            || observation.observation.projection != projection
            || !metadata::ids_valid(&ids)
        {
            return Err(invalid(
                "callback semantic acknowledgement changed capture/projection",
            ));
        }
        let request = metadata::Request::Acknowledge {
            service: self.service(),
            descriptor: self.descriptor.clone(),
            observation: observation.observation.clone(),
            ids: ids.clone(),
        };
        let metadata::Reply::Acknowledged {
            descriptor,
            sequence,
            ids: actual,
            retired,
        } = send(handle, original, &request).await?
        else {
            return Err(invalid("callback acknowledgement reply kind changed"));
        };
        if descriptor != self.descriptor
            || sequence != observation.observation.sequence
            || actual != ids
        {
            return Err(invalid("callback acknowledgement correlation changed"));
        }
        Ok(CallbackRetirement { ids, retired })
    }
    fn service(&self) -> String {
        self.operation
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .service
            .clone()
    }
}
fn projection(frontier: &str, snapshot: &str, thread_id: &str) -> metadata::Projection {
    metadata::Projection {
        frontier: frontier.into(),
        snapshot: snapshot.into(),
        thread_id: thread_id.into(),
    }
}
fn invalid(message: &'static str) -> TypedRequestError {
    TypedRequestError::Transport {
        method: "config/read".into(),
        source: IoError::new(ErrorKind::InvalidData, message),
    }
}
fn decode_error(source: serde_json::Error) -> TypedRequestError {
    TypedRequestError::Deserialize {
        method: "config/read".into(),
        source,
    }
}
async fn send(
    handle: &AppServerRequestHandle,
    original: ClientRequest,
    selected: &metadata::Request,
) -> Result<metadata::Reply, TypedRequestError> {
    if original.method_name() != "config/read" || !selected.valid() {
        return Err(invalid("callback operation requires original config/read"));
    }
    let AppServerRequestHandle::Remote(remote) = handle else {
        return Err(invalid(
            "callback operation requires actual physical Remote IO",
        ));
    };
    let mut wire: JSONRPCRequest =
        serde_json::from_value(serde_json::to_value(original).map_err(decode_error)?)
            .map_err(decode_error)?;
    let params = wire
        .params
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| invalid("callback config/read params missing"))?;
    let meta = params
        .entry("_meta")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| invalid("callback metadata not object"))?;
    if [
        metadata::KEY,
        "alab.initialSession",
        "alab.clientConstruction",
        "alab.clientConstructionPrepare",
        "alab.sourceSelectionFrontier",
        "alab.retainedFrontier",
        "alab.retainedConfigured",
        "alab.retainedReader",
    ]
    .iter()
    .any(|key| meta.contains_key(*key))
    {
        return Err(invalid("callback namespace occupied"));
    }
    meta.insert(
        metadata::KEY.into(),
        serde_json::to_value(selected).map_err(decode_error)?,
    );
    let mut value = decode_typed_response::<serde_json::Value>(
        "config/read",
        remote.request_json_rpc(wire).await,
    )?;
    let meta = value
        .as_object_mut()
        .and_then(|body| body.get_mut("_meta"))
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| invalid("callback result metadata missing"))?;
    let reply = serde_json::from_value(
        meta.remove(metadata::RESULT_KEY)
            .ok_or_else(|| invalid("callback result missing"))?,
    )
    .map_err(decode_error)?;
    if meta.is_empty() {
        value
            .as_object_mut()
            .ok_or_else(|| invalid("callback result not object"))?
            .remove("_meta");
    }
    let _original: codex_app_server_protocol::ConfigReadResponse =
        serde_json::from_value(value).map_err(decode_error)?;
    Ok(reply)
}
#[cfg(test)]
#[path = "retained_callback_client_tests.rs"]
mod tests;

#[path = "retained_callback_ack_client.rs"]
mod ack_routes;
pub use ack_routes::ClosedCallbackObservation;
pub use ack_routes::ConfiguredCallbackObservation;

#[path = "retained_input_client.rs"]
mod original_input;
pub use original_input::OriginalInputPublication;
pub use original_input::OriginalInputReceipt;
pub use original_input::OriginalInputReservation;
pub use original_input::PublicationOutcome;
