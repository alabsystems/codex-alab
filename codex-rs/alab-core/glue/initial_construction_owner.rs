// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual client owner retained BEFORE the original initial mutation await.
//! Checkpoint is immutable request/correlation DATA, not running handle/role.
use super::*;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InitialConstructionCheckpoint {
    schema: u32,
    selection: InitialSessionSelection,
    action: Option<InitialAction>,
    request: Option<FrozenInitialRequest>,
    result: Option<InitialSessionResult>,
    next_selection: Option<NextSelection>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FrozenInitialRequest {
    method: String,
    had_params: bool,
    params: serde_json::Value,
}
impl FrozenInitialRequest {
    fn into_tuple(self) -> (String, Option<serde_json::Value>) {
        (self.method, self.had_params.then_some(self.params))
    }
    fn valid(&self) -> bool {
        matches!(
            self.method.as_str(),
            "thread/start" | "thread/fork" | "thread/resume"
        ) && (self.had_params || self.params.is_null())
            && serde_json::to_vec(self)
                .is_ok_and(|bytes| bytes.len() <= metadata::preparation::MAX_ARGUMENT_BYTES)
    }
}
#[derive(Clone)]
pub struct InitialConstructionOperation(Arc<InitialScope>);
impl InitialConstructionOperation {
    pub fn new(selection: InitialSessionSelection, action: InitialAction) -> IoResult<Self> {
        if !selection.valid() {
            return Err(invalid("invalid initial construction selection"));
        }
        Ok(Self::fresh(selection, Some(action)))
    }
    pub(super) fn fresh(selection: InitialSessionSelection, action: Option<InitialAction>) -> Self {
        Self(Arc::new(InitialScope {
            selection,
            action,
            result: Mutex::new(None),
            next_selection: Mutex::new(None),
            request: Mutex::new(None),
        }))
    }
    pub async fn run<T>(&self, original: impl std::future::Future<Output = T>) -> T {
        INITIAL_SCOPE.scope(self.0.clone(), original).await
    }
    pub fn observed_result(&self) -> Option<InitialSessionResult> {
        self.0
            .result
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    pub fn prepared_offer(&self) -> Option<ClientConstructionOffer> {
        self.observed_result().map(|result| result.source_selection)
    }
    /// Immutable original choice DATA, never a Native or Source admission.
    pub fn selection_data(&self) -> &InitialSessionSelection {
        &self.0.selection
    }
    pub fn action_data(&self) -> Option<&InitialAction> {
        self.0.action.as_ref()
    }
    /// Local request-construction fact only; not a Native/effect receipt.
    pub fn original_request_started_data(&self) -> bool {
        self.0
            .request
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }
    /// Identity of this local shared owner, not Native/currentness authority.
    pub fn same_operation(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub fn next_selection(&self) -> Option<NextSelection> {
        self.0
            .next_selection
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    pub fn checkpoint(&self) -> InitialConstructionCheckpoint {
        let request = self
            .0
            .request
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .map(|(method, params)| FrozenInitialRequest {
                method,
                had_params: params.is_some(),
                params: params.unwrap_or(serde_json::Value::Null),
            });
        InitialConstructionCheckpoint {
            schema: 1,
            selection: self.0.selection.clone(),
            action: self.0.action.clone(),
            request,
            result: self.observed_result(),
            next_selection: self.next_selection(),
        }
    }
    pub fn restore(checkpoint: InitialConstructionCheckpoint) -> IoResult<Self> {
        Self::validate_checkpoint(&checkpoint)?;
        Ok(Self(Arc::new(InitialScope {
            selection: checkpoint.selection,
            action: checkpoint.action,
            request: Mutex::new(checkpoint.request.map(FrozenInitialRequest::into_tuple)),
            result: Mutex::new(checkpoint.result),
            next_selection: Mutex::new(checkpoint.next_selection),
        })))
    }
    /// Check retained request and result data before constructing an operation.
    pub fn validate_checkpoint(checkpoint: &InitialConstructionCheckpoint) -> IoResult<()> {
        if checkpoint.schema != 1
            || !checkpoint.selection.valid()
            || checkpoint.request.as_ref().is_some_and(|request| {
                !request.valid()
                    || checkpoint.action.as_ref().is_some_and(|action| {
                        !action.matches(
                            &request.method,
                            request.had_params.then_some(&request.params),
                        )
                    })
            })
            || (checkpoint.request.is_none()
                && (checkpoint.result.is_some() || checkpoint.next_selection.is_some()))
            || (checkpoint.result.is_some() && checkpoint.next_selection.is_some())
            || checkpoint.result.as_ref().is_some_and(|result| {
                result.claim != checkpoint.selection.claim
                    || result.thread_id.is_empty()
                    || !result.source_selection.valid()
                    || result.source_selection.preparation != checkpoint.selection.claim
            })
            || checkpoint.next_selection.as_ref().is_some_and(|next| {
                !next.valid() || next.previous_claim != checkpoint.selection.claim
            })
        {
            return Err(invalid("invalid initial construction checkpoint"));
        }
        Ok(())
    }
}
impl InitialAction {
    pub(super) fn matches(&self, method: &str, params: Option<&serde_json::Value>) -> bool {
        match self {
            Self::Start => method == "thread/start",
            Self::Fork(id) => {
                method == "thread/fork"
                    && params
                        .and_then(|value| value.get("threadId"))
                        .and_then(serde_json::Value::as_str)
                        == Some(id.as_str())
            }
            Self::Resume(id) => {
                method == "thread/resume"
                    && params
                        .and_then(|value| value.get("threadId"))
                        .and_then(serde_json::Value::as_str)
                        == Some(id.as_str())
            }
        }
    }
}

#[cfg(test)]
#[path = "initial_construction_owner_tests.rs"]
mod tests;
