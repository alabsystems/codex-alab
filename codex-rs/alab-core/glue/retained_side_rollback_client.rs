// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! One original attached rollback, retained before first mutation await.
use super::side_cleanup_reply::SavedReply;
use super::*;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
#[path = "retained_side_rollback_metadata.rs"]
mod metadata;
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SideRollbackCheckpoint {
    schema: u8,
    claim: String,
    thread_id: String,
    operation: String,
    parameters: Option<serde_json::Value>,
    result: Option<SavedReply>,
    observed: bool,
}
#[derive(Clone)]
pub struct SideRollbackOperation(Arc<Mutex<SideRollbackCheckpoint>>);
tokio::task_local! {static ROLLBACK:SideRollbackOperation;}
impl SideRollbackOperation {
    pub fn new(actual: &ClientConstructionOperation) -> IoResult<Self> {
        let (claim, thread_id) = actual.side_rollback_data()?;
        Ok(Self(Arc::new(Mutex::new(SideRollbackCheckpoint {
            schema: 1,
            claim,
            thread_id,
            operation: format!(
                "{:016x}{:016x}",
                codex_alab_core::native_source::connection_request_prefix(),
                codex_alab_core::native_source::connection_request_prefix(),
            ),
            parameters: None,
            result: None,
            observed: false,
        }))))
    }
    pub fn checkpoint(&self) -> SideRollbackCheckpoint {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    pub fn restore(
        actual: &ClientConstructionOperation,
        data: SideRollbackCheckpoint,
    ) -> IoResult<Self> {
        let (claim, thread) = actual.side_rollback_data()?;
        let selection = metadata::Selection::Rollback {
            schema: data.schema,
            claim: data.claim.clone(),
            operation: data.operation.clone(),
        };
        if !selection.valid()
            || claim != data.claim
            || thread != data.thread_id
            || (data.observed && data.result.is_none())
            || data.result.as_ref().is_some_and(|reply| !reply.valid())
            || (data.result.is_some() && data.parameters.is_none())
            || data.parameters.as_ref().is_some_and(|params| {
                params.get("threadId").and_then(serde_json::Value::as_str) != Some(thread.as_str())
            })
        {
            return Err(invalid(
                "original Side rollback checkpoint changed its actual construction",
            ));
        }
        Ok(Self(Arc::new(Mutex::new(data))))
    }
    pub async fn run<T>(&self, original: impl std::future::Future<Output = T>) -> T {
        ROLLBACK.scope(self.clone(), original).await
    }
    fn freeze(
        &self,
        wire: &codex_app_server_protocol::JSONRPCRequest,
    ) -> IoResult<(metadata::Selection, Option<RequestResult>)> {
        if wire.method != "thread/delete" {
            return Err(invalid("original Side rollback method changed"));
        }
        let params = wire
            .params
            .clone()
            .ok_or_else(|| invalid("original Side rollback parameters missing"))?;
        let mut p = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if params.get("threadId").and_then(serde_json::Value::as_str) != Some(p.thread_id.as_str())
            || p.parameters.as_ref().is_some_and(|old| old != &params)
        {
            return Err(invalid("original Side rollback changed full parameters"));
        }
        p.parameters = Some(params);
        Ok((
            metadata::Selection::Rollback {
                schema: 1,
                claim: p.claim.clone(),
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
            return Err(invalid("original Side rollback result changed"));
        }
        p.result = Some(SavedReply::capture(&result));
        Ok(())
    }
    /// Called only after SAME SideCreation records its original result. A lost
    /// ACK leaves this cached result and same server operation recoverable.
    pub async fn observe_captured(&self, handle: &AppServerRequestHandle) -> IoResult<()> {
        let data = self.checkpoint();
        if data.result.is_none() {
            return Err(invalid("original rollback result not observed"));
        }
        if data.observed {
            return Ok(());
        }
        let AppServerRequestHandle::Remote(remote) = handle else {
            return Err(invalid("rollback observation needs actual physical client"));
        };
        let mut wire:codex_app_server_protocol::JSONRPCRequest=serde_json::from_value(serde_json::json!({"id":format!("alab-side-rollback-observed-{}",data.operation),"method":"thread/delete","params":data.parameters})).map_err(std::io::Error::other)?;
        decorate(
            &mut wire,
            &metadata::Selection::Observe {
                schema: 1,
                claim: data.claim,
                operation: data.operation,
            },
        )?;
        let actual = remote
            .request_json_rpc(wire)
            .await?
            .map_err(|_| invalid("rollback observation not confirmed"))?;
        let observed: metadata::observation::Observation = serde_json::from_value(
            super::side_cleanup_reply::observation_value(actual, metadata::OBSERVATION_KEY)?,
        )
        .map_err(std::io::Error::other)?;
        let current = self.checkpoint();
        if !observed.matches(&current.operation, &current.claim, &current.thread_id) {
            return Err(invalid("rollback observation changed exact operation"));
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
        return Err(invalid("invalid original rollback selection"));
    }
    let params = wire
        .params
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| invalid("rollback params missing"))?;
    let meta = params
        .entry("_meta")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| invalid("original rollback metadata not object"))?;
    if [
        metadata::KEY,
        "alab.sideInjection",
        "alab.sideView",
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
        return Err(invalid("rollback metadata namespace occupied"));
    }
    meta.insert(
        metadata::KEY.into(),
        serde_json::to_value(selection).map_err(std::io::Error::other)?,
    );
    Ok(())
}
pub(super) fn active() -> bool {
    ROLLBACK.try_with(|_| ()).is_ok()
}
pub(super) async fn request(
    handle: &AppServerRequestHandle,
    original: ClientRequest,
) -> IoResult<RequestResult> {
    let owner = ROLLBACK
        .try_with(Clone::clone)
        .map_err(|_| invalid("original rollback owner missing"))?;
    let AppServerRequestHandle::Remote(remote) = handle else {
        return Err(invalid("rollback needs actual physical client"));
    };
    let mut wire =
        serde_json::from_value(serde_json::to_value(original).map_err(std::io::Error::other)?)
            .map_err(std::io::Error::other)?;
    let (selection, cached) = owner.freeze(&wire)?;
    if let Some(cached) = cached {
        return Ok(cached);
    }
    decorate(&mut wire, &selection)?;
    let result = remote.request_json_rpc(wire).await?;
    owner.record(result.clone())?;
    Ok(result)
}

#[cfg(test)]
#[path = "retained_side_rollback_client_tests.rs"]
mod tests;
