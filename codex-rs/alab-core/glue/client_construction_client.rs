// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Scope only the original later UI Start/Fork/Resume RPC. This carries exact typed
//! projection DATA and observes a real backend result, never Native permission.
use super::*;
use metadata::CLIENT_KEY;
use metadata::CLIENT_RESULT_KEY;
use metadata::META;

#[path = "client_construction_preparation_client.rs"]
mod preparation;
pub use preparation::ClientConstructionCheckpoint;
pub use preparation::ClientConstructionOperation;
pub use preparation::ClientPreparationRetirement;
#[path = "retained_side_boundary_client.rs"]
mod side_boundary;
pub use side_boundary::scope_original_side_injection;
tokio::task_local! { static CLIENT_SCOPE: ClientConstructionOperation; }

pub async fn observe_client_construction<T>(
    selection: Option<ClientConstructionSelection>,
    work: impl std::future::Future<Output = T>,
) -> (T, Option<ClientConstructionResult>) {
    let Some(selection) = selection else {
        return (work.await, None);
    };
    // The existing convenience entry remains scoped to one original choice;
    // the genuine pending UI producer uses the public owner before this await.
    // Invalid external recovery data is never installed here.
    let operation = ClientConstructionOperation::fresh(selection);
    let value = operation.run(work).await;
    (value, operation.observed_result())
}
pub(super) async fn request(
    handle: &AppServerRequestHandle,
    request: ClientRequest,
) -> IoResult<RequestResult> {
    let operation = CLIENT_SCOPE.try_with(Clone::clone).ok();
    if request.method_name() == "thread/inject_items"
        && side_boundary::active()
        && let Some(operation) = operation.as_ref()
    {
        return side_boundary::request(handle, request, operation).await;
    }
    if operation.is_none()
        || !matches!(
            request.method_name(),
            "thread/start" | "thread/fork" | "thread/resume"
        )
    {
        return handle.request(request).await;
    }
    let AppServerRequestHandle::Remote(remote) = handle else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "owned client construction requires actual retained frontend IO",
        ));
    };
    let operation = operation.ok_or_else(|| invalid("client construction scope lost"))?;
    let mut wire: codex_app_server_protocol::JSONRPCRequest =
        serde_json::from_value(serde_json::to_value(request).map_err(std::io::Error::other)?)
            .map_err(std::io::Error::other)?;
    let offer = match operation.prepare(remote, &wire).await? {
        Ok(offer) => offer,
        Err(error) => return Ok(Err(error)),
    };
    let selection = operation.begin_mutation(&offer)?;
    let params = wire
        .params
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| invalid("client construction parameters must be an object"))?;
    let metadata = params
        .entry(META)
        .or_insert_with(|| serde_json::Value::Object(Default::default()))
        .as_object_mut()
        .ok_or_else(|| invalid("client construction metadata must be an object"))?;
    if metadata.contains_key(CLIENT_KEY) || metadata.contains_key(metadata::KEY) {
        return Err(invalid("construction metadata namespace occupied"));
    }
    metadata.insert(
        CLIENT_KEY.into(),
        serde_json::to_value(selection).map_err(std::io::Error::other)?,
    );
    let result = remote.request_json_rpc(wire).await?;
    let mut result = match result {
        Ok(value) => value,
        Err(error) => return Ok(Err(error)),
    };
    let body = result
        .as_object_mut()
        .ok_or_else(|| invalid("client construction result must be an object"))?;
    let metadata = body
        .get_mut(META)
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| invalid("client construction observation missing"))?;
    let observed: ClientConstructionResult = serde_json::from_value(
        metadata
            .remove(CLIENT_RESULT_KEY)
            .ok_or_else(|| invalid("client construction observation missing"))?,
    )
    .map_err(|_| invalid("invalid client construction observation"))?;
    if observed.thread_id.is_empty()
        || body
            .get("thread")
            .and_then(|thread| thread.get("id"))
            .and_then(serde_json::Value::as_str)
            != Some(observed.thread_id.as_str())
    {
        return Err(invalid("client construction observation does not match"));
    }
    if body
        .get(META)
        .and_then(serde_json::Value::as_object)
        .is_some_and(serde_json::Map::is_empty)
    {
        body.remove(META);
    }
    operation.record(observed)?;
    Ok(Ok(result))
}

pub(super) fn active() -> bool {
    CLIENT_SCOPE.try_with(|_| ()).is_ok()
}
