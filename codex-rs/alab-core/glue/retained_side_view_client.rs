// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Original close wire and observed UI result DATA. Owner is retained before
//! polling; a successor can read the original result without closing its own view.
use super::side_cleanup_reply::SavedReply;
use super::*;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
#[path = "retained_side_view_metadata.rs"]
mod metadata;
pub use metadata::SideView;
impl metadata::ReadResult {
    pub(super) fn into_original(self, view: &SideView, operation: &str) -> IoResult<RequestResult> {
        if self.schema != 1 || &self.view != view || self.operation != operation {
            return Err(invalid(
                "Side close readback changed the selected operation",
            ));
        }
        match self.reply {
            metadata::ReadReply::Success { status } => Ok(Ok(serde_json::to_value(
                codex_app_server_protocol::ThreadUnsubscribeResponse { status },
            )
            .map_err(std::io::Error::other)?)),
            metadata::ReadReply::Rejected {
                code,
                message,
                had_data,
                value,
            } => {
                if !had_data && !value.is_null() {
                    return Err(invalid(
                        "Side close readback changed the original error data",
                    ));
                }
                Ok(Err(codex_app_server_protocol::JSONRPCErrorError {
                    code,
                    message,
                    data: had_data.then_some(value),
                }))
            }
        }
    }
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SideViewCloseCheckpoint {
    schema: u8,
    operation: String,
    view: SideView,
    parameters: Option<serde_json::Value>,
    result: Option<SavedReply>,
    observed: bool,
}

fn valid_close_reply(reply: &SavedReply) -> bool {
    match reply {
        SavedReply::Success { value } => serde_json::from_value::<
            codex_app_server_protocol::ThreadUnsubscribeResponse,
        >(value.clone())
        .is_ok(),
        SavedReply::Error { .. } => reply.valid(),
    }
}

#[derive(Clone)]
pub struct SideViewCloseOperation {
    checkpoint: Arc<Mutex<SideViewCloseCheckpoint>>,
    // Checkpoint DATA can recover a result, but cannot originate the first close.
    original_owner: bool,
}
tokio::task_local! {static CLOSE:SideViewCloseOperation;}
impl SideViewCloseOperation {
    pub fn new(view: SideView) -> IoResult<Self> {
        if !view.valid() {
            return Err(invalid("invalid original Side view data"));
        }
        let checkpoint = SideViewCloseCheckpoint {
            schema: 1,
            operation: format!(
                "{:016x}{:016x}",
                codex_alab_core::native_source::connection_request_prefix(),
                codex_alab_core::native_source::connection_request_prefix(),
            ),
            view,
            parameters: None,
            result: None,
            observed: false,
        };
        Ok(Self {
            checkpoint: Arc::new(Mutex::new(checkpoint)),
            original_owner: true,
        })
    }
    pub fn same_operation(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.checkpoint, &other.checkpoint)
    }
    pub fn original_succeeded(&self) -> bool {
        self.checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .result
            .as_ref()
            .is_some_and(SavedReply::succeeded)
    }
    pub fn checkpoint(&self) -> SideViewCloseCheckpoint {
        self.checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    /// The first caller recorded its exact parameters before attempting the wire send.
    pub fn has_frozen_request(&self) -> bool {
        self.checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .parameters
            .is_some()
    }
    pub fn restore(checkpoint: SideViewCloseCheckpoint) -> IoResult<Self> {
        let selection = metadata::Selection::Close {
            view: checkpoint.view.clone(),
            operation: checkpoint.operation.clone(),
        };
        if checkpoint.schema != 1
            || !selection.valid()
            || (checkpoint.observed && checkpoint.result.is_none())
            || checkpoint
                .result
                .as_ref()
                .is_some_and(|reply| !valid_close_reply(reply))
            || (checkpoint.result.is_some() && checkpoint.parameters.is_none())
            || checkpoint.parameters.as_ref().is_some_and(|parameters| {
                parameters
                    .get("threadId")
                    .and_then(serde_json::Value::as_str)
                    != Some(checkpoint.view.thread_id.as_str())
            })
        {
            return Err(invalid("invalid original Side close checkpoint"));
        }
        Ok(Self {
            checkpoint: Arc::new(Mutex::new(checkpoint)),
            original_owner: false,
        })
    }
    pub async fn run<T>(&self, original: impl std::future::Future<Output = T>) -> T {
        CLOSE.scope(self.clone(), original).await
    }
    pub fn view_data(&self) -> SideView {
        self.checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .view
            .clone()
    }
    fn freeze(
        &self,
        wire: &codex_app_server_protocol::JSONRPCRequest,
    ) -> IoResult<(metadata::Selection, Option<RequestResult>)> {
        if wire.method != "thread/unsubscribe" {
            return Err(invalid("original Side close method changed"));
        }
        let parameters = wire
            .params
            .clone()
            .ok_or_else(|| invalid("original Side close parameters missing"))?;
        let mut p = self
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if parameters
            .get("threadId")
            .and_then(serde_json::Value::as_str)
            != Some(p.view.thread_id.as_str())
            || p.parameters.as_ref().is_some_and(|old| old != &parameters)
        {
            return Err(invalid("Side close cannot change full original parameters"));
        }
        let selection = if p.parameters.is_some() {
            metadata::Selection::ReadResult {
                view: p.view.clone(),
                operation: p.operation.clone(),
                parameters,
            }
        } else if self.original_owner {
            p.parameters = Some(parameters);
            metadata::Selection::Close {
                view: p.view.clone(),
                operation: p.operation.clone(),
            }
        } else {
            return Err(invalid(
                "restored Side close has no frozen original request",
            ));
        };
        Ok((selection, p.result.as_ref().map(SavedReply::original)))
    }
    fn record(&self, result: RequestResult) -> IoResult<()> {
        let mut p = self
            .checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if p.result
            .as_ref()
            .is_some_and(|old| old.original() != result)
        {
            return Err(invalid("Side close replay changed original result"));
        }
        // The caller's normal typed decoder still reports an ill-formed success.
        // Only a real unsubscribe status or an original rejection belongs in the checkpoint.
        let reply = SavedReply::capture(&result);
        if valid_close_reply(&reply) {
            p.result = Some(reply);
        }
        Ok(())
    }
    /// Read the original unsubscribe result using only its frozen identity and wire.
    pub async fn read_result(
        &self,
        handle: &AppServerRequestHandle,
    ) -> Result<codex_app_server_protocol::ThreadUnsubscribeResponse, crate::TypedRequestError>
    {
        let result = self.read_frozen_result(handle).await;
        crate::decode_typed_response("thread/unsubscribe", result)
    }
    async fn read_frozen_result(&self, handle: &AppServerRequestHandle) -> IoResult<RequestResult> {
        let checkpoint = self.checkpoint();
        if let Some(cached) = checkpoint.result {
            return Ok(cached.original());
        }
        let selection = metadata::Selection::ReadResult {
            view: checkpoint.view,
            operation: checkpoint.operation,
            parameters: checkpoint
                .parameters
                .ok_or_else(|| invalid("Side close result requires its frozen original request"))?,
        };
        self.lookup(handle, &selection).await
    }
    async fn lookup(
        &self,
        handle: &AppServerRequestHandle,
        selection: &metadata::Selection,
    ) -> IoResult<RequestResult> {
        let response = read_control(handle, selection).await?;
        let envelope =
            super::side_cleanup_reply::observation_value(response, metadata::READ_RESULT_KEY)?;
        let receipt: metadata::ReadResult =
            serde_json::from_value(envelope).map_err(std::io::Error::other)?;
        let original = receipt.into_original(selection.view(), selection.operation())?;
        self.record(original.clone())?;
        Ok(original)
    }
    /// Pure observed-result cleanup, not Native permission or effects proof.
    /// Call after UI application; cached result remains in this owner on error.
    pub async fn observe_applied(&self, handle: &AppServerRequestHandle) -> IoResult<()> {
        let checkpoint = self.checkpoint();
        if checkpoint.result.is_none() {
            return Err(invalid("original Side close has no observed result"));
        }
        if checkpoint.observed {
            return Ok(());
        }
        let selection = metadata::Selection::AcknowledgeResult {
            view: checkpoint.view,
            operation: checkpoint.operation,
            parameters: checkpoint.parameters.ok_or_else(|| {
                invalid("Side close observation requires its frozen original request")
            })?,
        };
        let actual = read_control(handle, &selection).await?;
        let observed: metadata::observation::Observation = serde_json::from_value(
            super::side_cleanup_reply::observation_value(actual, metadata::OBSERVATION_KEY)?,
        )
        .map_err(std::io::Error::other)?;
        let current = self.checkpoint();
        if !observed.matches(
            &current.operation,
            &current.view.claim,
            &current.view.thread_id,
        ) {
            return Err(invalid("Side close observation changed exact operation"));
        }
        self.checkpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .observed = true;
        Ok(())
    }
}

// An older peer that ignores ALab metadata sees an ordinary configuration read.
async fn read_control(
    handle: &AppServerRequestHandle,
    selection: &metadata::Selection,
) -> IoResult<serde_json::Value> {
    let AppServerRequestHandle::Remote(remote) = handle else {
        return Err(invalid(
            "Side close result requires an actual physical client",
        ));
    };
    let mut wire = codex_app_server_protocol::JSONRPCRequest {
        id: RequestId::String(format!(
            "alab-side-close-read-{:016x}",
            codex_alab_core::native_source::connection_request_prefix()
        )),
        method: "config/read".into(),
        params: Some(serde_json::json!({"includeLayers": false})),
        trace: None,
    };
    decorate(&mut wire, selection)?;
    remote
        .request_json_rpc(wire)
        .await?
        .map_err(|_| invalid("Side close result control was refused"))
}

fn decorate(
    wire: &mut codex_app_server_protocol::JSONRPCRequest,
    selection: &metadata::Selection,
) -> IoResult<()> {
    if !selection.valid() {
        return Err(invalid("invalid Side close selection"));
    }
    let params = wire
        .params
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| invalid("original Side close parameters missing"))?;
    let meta = params
        .entry("_meta")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| invalid("original Side metadata not an object"))?;
    if [
        metadata::KEY,
        "alab.sideRollback",
        "alab.sideInjection",
        "alab.retainedCallbacks",
        "alab.retainedReader",
        "alab.retainedConfigured",
        "alab.retainedFrontier",
        "alab.sourceSelectionFrontier",
        "alab.initialSession",
        "alab.clientConstruction",
        "alab.clientConstructionPrepare",
    ]
    .into_iter()
    .any(|key| meta.contains_key(key))
    {
        return Err(invalid("Side close metadata namespace occupied"));
    }
    meta.insert(
        metadata::KEY.into(),
        serde_json::to_value(selection).map_err(std::io::Error::other)?,
    );
    Ok(())
}
pub(super) async fn request(
    handle: &AppServerRequestHandle,
    request: ClientRequest,
) -> IoResult<Option<RequestResult>> {
    if request.method_name() != "thread/unsubscribe" {
        return Ok(None);
    }
    let Ok(owner) = CLOSE.try_with(Clone::clone) else {
        return Ok(None);
    };
    let AppServerRequestHandle::Remote(remote) = handle else {
        return Err(invalid("Side close requires actual physical client"));
    };
    let mut wire =
        serde_json::from_value(serde_json::to_value(request).map_err(std::io::Error::other)?)
            .map_err(std::io::Error::other)?;
    let (selection, cached) = owner.freeze(&wire)?;
    if let Some(cached) = cached {
        return Ok(Some(cached));
    }
    let result = match &selection {
        metadata::Selection::Close { .. } => {
            decorate(&mut wire, &selection)?;
            let result = remote.request_json_rpc(wire).await?;
            owner.record(result.clone())?;
            result
        }
        metadata::Selection::ReadResult { .. } => owner.lookup(handle, &selection).await?,
        _ => return Err(invalid("unexpected Side close operation phase")),
    };
    Ok(Some(result))
}

pub(super) fn active() -> bool {
    CLOSE.try_with(|_| ()).is_ok()
}

#[cfg(test)]
#[path = "retained_side_view_client_tests.rs"]
mod tests;
