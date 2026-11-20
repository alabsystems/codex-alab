// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use codex_app_server_protocol::JSONRPCRequest;
use serde_json::Value;
#[path = "retained_side_view_metadata.rs"]
pub(crate) mod metadata;
#[derive(Clone)]
pub(crate) struct Claim {
    pub selection: metadata::Selection,
    pub parameters: Value,
}
impl std::fmt::Debug for Claim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SideView(<DATA; requires actual installed owner/current Native A>)")
    }
}
pub(crate) fn take(request: &mut JSONRPCRequest) -> std::io::Result<Option<Claim>> {
    let bad = || {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid Side view metadata",
        )
    };
    let Some(params) = request.params.as_mut().and_then(Value::as_object_mut) else {
        return Ok(None);
    };
    let Some(meta) = params.get_mut("_meta").and_then(Value::as_object_mut) else {
        return Ok(None);
    };
    let Some(value) = meta.remove(metadata::KEY) else {
        return Ok(None);
    };
    #[cfg(not(unix))]
    return Err(bad());
    if [
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
        return Err(bad());
    }
    let selection: metadata::Selection = serde_json::from_value(value).map_err(|_| bad())?;
    if !selection.valid() {
        return Err(bad());
    }
    if meta.is_empty() {
        params.remove("_meta");
    }
    match &selection {
        metadata::Selection::ReadResult { .. } | metadata::Selection::AcknowledgeResult { .. } => {
            if request.method != "config/read"
                || params.len() != 1
                || params.get("includeLayers").and_then(Value::as_bool) != Some(false)
            {
                return Err(bad());
            }
        }
        metadata::Selection::Close { view, .. } | metadata::Selection::Observe { view, .. } => {
            if request.method != "thread/unsubscribe"
                || params.get("threadId").and_then(Value::as_str) != Some(view.thread_id.as_str())
            {
                return Err(bad());
            }
        }
    }
    Ok(Some(Claim {
        selection,
        parameters: Value::Object(params.clone()),
    }))
}

#[cfg(unix)]
impl metadata::ReadResult {
    pub(crate) fn new(
        view: &metadata::SideView,
        operation: &str,
        result: &Result<
            codex_app_server_protocol::ThreadUnsubscribeResponse,
            codex_app_server_protocol::JSONRPCErrorError,
        >,
    ) -> Self {
        let reply = match result {
            Ok(response) => metadata::ReadReply::Success {
                status: response.status.clone(),
            },
            Err(error) => metadata::ReadReply::Rejected {
                code: error.code,
                message: error.message.clone(),
                had_data: error.data.is_some(),
                value: error.data.clone().unwrap_or(Value::Null),
            },
        };
        Self {
            schema: 1,
            view: view.clone(),
            operation: operation.into(),
            reply,
        }
    }
}

#[cfg(all(test, unix))]
#[path = "retained_side_view_query_claim_tests.rs"]
mod query_claim_tests;
