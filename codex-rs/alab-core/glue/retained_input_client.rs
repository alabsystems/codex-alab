// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! One immutable original answer. Restored DATA resumes its SAME Source owner;
//! it cannot construct a reply handle, callback cell, Native grant or effects.
use super::*;
use metadata::input::InputDescriptor;
use metadata::input::InputReceipt;
pub use metadata::input::PublicationOutcome;
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct State {
    schema: u8,
    service: String,
    operation: String,
    request_id: RequestId,
    #[serde(deserialize_with = "required_option")]
    descriptor: Option<InputDescriptor>,
    #[serde(deserialize_with = "required_option")]
    value: Option<serde_json::Value>,
    #[serde(deserialize_with = "required_option")]
    receipt: Option<InputReceipt>,
}
fn required_option<'de, D, T>(decoder: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    serde::Deserialize::deserialize(decoder)
}
pub struct OriginalInputReservation {
    state: Mutex<State>,
    actual: Option<crate::server_request::alab_original_input::ReplyOwner>,
}
pub struct OriginalInputPublication {
    reservation: Arc<OriginalInputReservation>,
}
#[derive(Clone)]
pub struct OriginalInputReceipt {
    receipt: InputReceipt,
}
impl OriginalInputReceipt {
    pub fn outcome_data(&self) -> &PublicationOutcome {
        &self.receipt.outcome
    }
    pub fn request_id_data(&self) -> &RequestId {
        &self.receipt.descriptor.request_id
    }
}
impl OriginalInputReservation {
    pub(crate) fn from_reply(
        actual: crate::server_request::alab_original_input::ReplyOwner,
        service: &str,
    ) -> std::io::Result<Arc<Self>> {
        if !metadata::token(service) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid retained service",
            ));
        }
        let state = State {
            schema: 1,
            service: service.into(),
            operation: format!(
                "{:016x}{:016x}",
                codex_alab_core::native_source::connection_request_prefix(),
                codex_alab_core::native_source::connection_request_prefix()
            ),
            request_id: actual.request_id().clone(),
            descriptor: None,
            value: None,
            receipt: None,
        };
        Ok(Arc::new(Self {
            state: Mutex::new(state),
            actual: Some(actual),
        }))
    }
    pub(crate) fn attach_reply(
        &self,
        actual: crate::server_request::alab_original_input::ReplyOwner,
    ) -> std::io::Result<Arc<Self>> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if self.actual.is_some() || actual.request_id() != &state.request_id {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "only restored SAME input operation can attach its actual replayed reply",
            ));
        }
        Ok(Arc::new(Self {
            state: Mutex::new(state),
            actual: Some(actual),
        }))
    }
    pub fn is_for_client(&self, actual: &AppServerRequestHandle) -> bool {
        self.actual
            .as_ref()
            .is_some_and(|owner| owner.is_for_client(actual))
    }
    pub fn is_for_reply(&self, actual: &crate::AppServerReplyHandle) -> bool {
        self.actual
            .as_ref()
            .is_some_and(|owner| owner.is_for_reply(actual))
    }
    /// DATA-only routing hint. A stored descriptor does not establish that a
    /// current Source owner exists; the same protected Publish must verify it.
    pub fn binding_data_present(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .descriptor
            .is_some()
    }
    pub fn service_data(&self) -> String {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .service
            .clone()
    }
    pub fn request_id_data(&self) -> RequestId {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .request_id
            .clone()
    }
    pub fn checkpoint(&self) -> Result<serde_json::Value, TypedRequestError> {
        serde_json::to_value(&*self.state.lock().unwrap_or_else(PoisonError::into_inner))
            .map_err(decode_error)
    }
    /// Restores SAME operation DATA, not a new reply capability. The private
    /// Source owner/old ingress cut must still authorize every resume.
    pub fn restore_checkpoint(value: serde_json::Value) -> Result<Arc<Self>, TypedRequestError> {
        let state: State = serde_json::from_value(value).map_err(decode_error)?;
        if state.value.as_ref().is_some_and(|value| {
            serde_json::to_vec(value).map_or(true, |wire| wire.len() > 1_048_576)
        }) || state.schema != 1
            || !metadata::token(&state.service)
            || !metadata::token(&state.operation)
            || !metadata::ids_valid(std::slice::from_ref(&state.request_id))
            || state.descriptor.as_ref().is_some_and(|d| {
                !d.valid() || d.operation != state.operation || d.request_id != state.request_id
            })
            || state.receipt.as_ref().is_some_and(|receipt| {
                !receipt.valid()
                    || state.descriptor.as_ref() != Some(&receipt.descriptor)
                    || state.value.is_none()
            })
        {
            return Err(invalid("invalid original input checkpoint"));
        }
        Ok(Arc::new(Self {
            state: Mutex::new(state),
            actual: None,
        }))
    }
    /// Retain this owner BEFORE polling. Source pins original cell+connection
    /// independently from callback-view ticket retirement before returning.
    pub async fn bind_capture(
        &self,
        handle: &AppServerRequestHandle,
        original: ClientRequest,
        capture: &CallbackCapture,
    ) -> Result<(), TypedRequestError> {
        let request = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.value.is_none() {
                return Err(invalid(
                    "freeze original answer BEFORE Source binding await",
                ));
            }
            if capture.service() != state.service
                || !capture.descriptor.ids.contains(&state.request_id)
            {
                return Err(invalid("original input changed actual capture"));
            }
            metadata::Request::PrepareInput {
                service: state.service.clone(),
                descriptor: capture.descriptor.clone(),
                operation: state.operation.clone(),
                request_id: state.request_id.clone(),
                value: state
                    .value
                    .clone()
                    .ok_or_else(|| invalid("original answer absent before binding"))?,
                previous: state.descriptor.clone(),
            }
        };
        let metadata::Reply::InputPrepared { descriptor } =
            send(handle, original, &request).await?
        else {
            return Err(invalid("original input preparation reply changed"));
        };
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if !descriptor.valid()
            || descriptor.operation != state.operation
            || descriptor.request_id != state.request_id
            || state
                .descriptor
                .as_ref()
                .is_some_and(|prior| prior != &descriptor)
        {
            return Err(invalid("original input reservation changed"));
        }
        state.descriptor = Some(descriptor);
        Ok(())
    }
    /// Must run before actual AppEvent enqueue. Repeated freeze accepts only
    /// the SAME immutable Value; C2 cannot choose another answer.
    pub fn freeze(
        self: &Arc<Self>,
        value: serde_json::Value,
    ) -> std::io::Result<Arc<OriginalInputPublication>> {
        if serde_json::to_vec(&value)
            .map_err(std::io::Error::other)?
            .len()
            > 1_048_576
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "original input exceeds bound",
            ));
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.value.as_ref().is_some_and(|prior| prior != &value) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "original input value already fixed",
            ));
        }
        state.value = Some(value);
        Ok(Arc::new(OriginalInputPublication {
            reservation: self.clone(),
        }))
    }
    pub fn pending_publication(self: &Arc<Self>) -> Option<Arc<OriginalInputPublication>> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .value
            .as_ref()
            .map(|_| {
                Arc::new(OriginalInputPublication {
                    reservation: self.clone(),
                })
            })
    }
}
impl OriginalInputPublication {
    pub fn reservation(&self) -> &Arc<OriginalInputReservation> {
        &self.reservation
    }
    pub fn checkpoint(&self) -> Result<serde_json::Value, TypedRequestError> {
        self.reservation.checkpoint()
    }
    /// Same operation on C1 or C2. If Source never received its immutable value,
    /// the new-origin branch waits the REAL original ingress/dispatch drain.
    pub async fn publish(
        &self,
        handle: &AppServerRequestHandle,
        original: ClientRequest,
    ) -> Result<OriginalInputReceipt, TypedRequestError> {
        let request = {
            let state = self
                .reservation
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            metadata::Request::PublishInput {
                service: state.service.clone(),
                descriptor: state
                    .descriptor
                    .clone()
                    .ok_or_else(|| invalid("original input Source reservation not yet bound"))?,
                value: state
                    .value
                    .clone()
                    .ok_or_else(|| invalid("original input value absent"))?,
                previous: state.receipt.clone(),
            }
        };
        let metadata::Reply::InputPublished { receipt } = send(handle, original, &request).await?
        else {
            return Err(invalid("original publication reply changed"));
        };
        let mut state = self
            .reservation
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !receipt.valid()
            || state.descriptor.as_ref() != Some(&receipt.descriptor)
            || state
                .receipt
                .as_ref()
                .is_some_and(|prior| prior != &receipt)
        {
            return Err(invalid("original publication identity/outcome changed"));
        }
        state.receipt = Some(receipt.clone());
        Ok(OriginalInputReceipt { receipt })
    }
    /// Call only after the original local owner retains the actual terminal
    /// receipt. This retires private publication bookkeeping, not effects.
    pub async fn retire(
        &self,
        handle: &AppServerRequestHandle,
        original: ClientRequest,
        receipt: &OriginalInputReceipt,
    ) -> Result<(), TypedRequestError> {
        let request = {
            let state = self
                .reservation
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if state.receipt.as_ref() != Some(&receipt.receipt) {
                return Err(invalid("original input receipt not observed by SAME owner"));
            }
            metadata::Request::RetireInput {
                service: state.service.clone(),
                receipt: receipt.receipt.clone(),
                value: state
                    .value
                    .clone()
                    .ok_or_else(|| invalid("original input value absent"))?,
            }
        };
        let metadata::Reply::InputRetired { receipt: actual } =
            send(handle, original, &request).await?
        else {
            return Err(invalid("original input retirement reply changed"));
        };
        if actual != receipt.receipt {
            return Err(invalid("original input retirement correlation changed"));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "retained_input_client_tests.rs"]
mod tests;
