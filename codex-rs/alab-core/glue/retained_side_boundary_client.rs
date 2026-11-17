// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Only the genuine original sidebar_fork injection is scoped. An immutable
//! checkpoint carries DATA across EOF; Source still requires actual Native A
//! and SAME accepted Side construction before any original Core injection.
use super::*;
#[path = "retained_side_boundary_metadata.rs"]
pub(super) mod side_metadata;
tokio::task_local! { static ORIGINAL_SIDE_INJECTION: (); }
pub async fn scope_original_side_injection<T>(original: impl std::future::Future<Output = T>) -> T {
    ORIGINAL_SIDE_INJECTION.scope((), original).await
}
pub(super) fn active() -> bool {
    ORIGINAL_SIDE_INJECTION.try_with(|_| ()).is_ok()
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Checkpoint {
    pub(super) parameters: serde_json::Value,
    pub(super) result: Option<side_metadata::Completion>,
}
pub(super) async fn request(
    handle: &AppServerRequestHandle,
    request: ClientRequest,
    operation: &ClientConstructionOperation,
) -> IoResult<RequestResult> {
    let AppServerRequestHandle::Remote(remote) = handle else {
        return Err(invalid(
            "owned Side injection requires original physical request client",
        ));
    };
    let mut wire: codex_app_server_protocol::JSONRPCRequest =
        serde_json::from_value(serde_json::to_value(request).map_err(std::io::Error::other)?)
            .map_err(std::io::Error::other)?;
    let selection = operation.freeze_side(&wire)?;
    if !selection.valid() {
        return Err(invalid("Side injection selection data invalid"));
    }
    let params = wire
        .params
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| invalid("original Side injection params missing"))?;
    let meta = params
        .entry("_meta")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| invalid("original Side metadata must be an object"))?;
    if [
        side_metadata::KEY,
        "alab.sideRollback",
        "alab.sideView",
        "alab.initialSession",
        "alab.clientConstruction",
        "alab.clientConstructionPrepare",
        "alab.retainedFrontier",
        "alab.sourceSelectionFrontier",
        "alab.retainedConfigured",
        "alab.retainedReader",
        "alab.retainedCallbacks",
    ]
    .into_iter()
    .any(|key| meta.contains_key(key))
    {
        return Err(invalid("original Side metadata namespace occupied"));
    }
    meta.insert(
        side_metadata::KEY.into(),
        serde_json::to_value(&selection).map_err(std::io::Error::other)?,
    );
    let result = remote.request_json_rpc(wire).await?;
    let mut value = match result {
        Ok(value) => value,
        Err(error) => return Ok(Err(error)),
    };
    let body = value
        .as_object_mut()
        .ok_or_else(|| invalid("Side injection response must be an object"))?;
    let meta = body
        .get_mut("_meta")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| invalid("original Side recording observation missing"))?;
    let complete: side_metadata::Completion = serde_json::from_value(
        meta.remove(side_metadata::RESULT_KEY)
            .ok_or_else(|| invalid("original Side recording observation missing"))?,
    )
    .map_err(|_| invalid("malformed Side recording observation"))?;
    operation.record_side(complete)?;
    if meta.is_empty() {
        body.remove("_meta");
    }
    Ok(Ok(value))
}
