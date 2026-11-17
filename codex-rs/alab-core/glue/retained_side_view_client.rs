// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Original close wire and observed UI result DATA. Owner is retained before
//! polling; a lost C1 observation replays SAME backend-owned operation on C2.
use super::side_cleanup_reply::SavedReply;
use super::*;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
#[path = "retained_side_view_metadata.rs"]
mod metadata;
pub use metadata::SideView;
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
#[derive(Clone)]
pub struct SideViewCloseOperation(Arc<Mutex<SideViewCloseCheckpoint>>);
tokio::task_local! {static CLOSE:SideViewCloseOperation;}
impl SideViewCloseOperation {
    pub fn new(view: SideView) -> IoResult<Self> {
        if !view.valid() {
            return Err(invalid("invalid original Side view data"));
        }
        Ok(Self(Arc::new(Mutex::new(SideViewCloseCheckpoint {
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
        }))))
    }
    pub fn same_operation(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub fn original_succeeded(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .result
            .as_ref()
            .is_some_and(SavedReply::succeeded)
    }
    pub fn checkpoint(&self) -> SideViewCloseCheckpoint {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
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
                .is_some_and(|reply| !reply.valid())
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
        Ok(Self(Arc::new(Mutex::new(checkpoint))))
    }
    pub async fn run<T>(&self, original: impl std::future::Future<Output = T>) -> T {
        CLOSE.scope(self.clone(), original).await
    }
    pub fn view_data(&self) -> SideView {
        self.0
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
        let mut p = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if parameters
            .get("threadId")
            .and_then(serde_json::Value::as_str)
            != Some(p.view.thread_id.as_str())
            || p.parameters.as_ref().is_some_and(|old| old != &parameters)
        {
            return Err(invalid("Side close cannot change full original parameters"));
        }
        p.parameters = Some(parameters);
        Ok((
            metadata::Selection::Close {
                view: p.view.clone(),
                operation: p.operation.clone(),
            },
            p.result.as_ref().map(SavedReply::original),
        ))
    }
    fn record(&self, result: RequestResult) -> IoResult<()> {
        let mut p = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if p.result
            .as_ref()
            .is_some_and(|old| old.original() != result)
        {
            return Err(invalid("Side close replay changed original result"));
        }
        p.result = Some(SavedReply::capture(&result));
        Ok(())
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
        let AppServerRequestHandle::Remote(remote) = handle else {
            return Err(invalid("Side observation requires actual physical client"));
        };
        let mut wire:codex_app_server_protocol::JSONRPCRequest=serde_json::from_value(serde_json::json!({"id":format!("alab-side-close-observed-{}",checkpoint.operation),"method":"thread/unsubscribe","params":checkpoint.parameters})).map_err(std::io::Error::other)?;
        decorate(
            &mut wire,
            &metadata::Selection::Observe {
                view: checkpoint.view,
                operation: checkpoint.operation,
            },
        )?;
        let actual = remote
            .request_json_rpc(wire)
            .await?
            .map_err(|_| invalid("Side close observation not confirmed"))?;
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
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .observed = true;
        Ok(())
    }
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
    decorate(&mut wire, &selection)?;
    let result = remote.request_json_rpc(wire).await?;
    owner.record(result.clone())?;
    Ok(Some(result))
}

pub(super) fn active() -> bool {
    CLOSE.try_with(|_| ()).is_ok()
}

#[cfg(test)]
#[path = "retained_side_view_client_tests.rs"]
mod tests;
