// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use codex_app_server_protocol::JSONRPCRequest;
use serde_json::Value;
use std::io;
#[path = "retained_source_selection_metadata.rs"]
mod metadata;
pub(crate) use metadata::Presentation;
pub(crate) use metadata::RESULT_KEY;
pub(crate) use metadata::Receipt;
pub(crate) use metadata::Selection;
pub(crate) use metadata::side_display;
#[derive(Clone)]
pub(crate) struct Claim(pub(crate) Selection);
impl std::fmt::Debug for Claim {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("SourceSelection(<DATA; requires actual Native/S2/runtime/UI custody>)")
    }
}
pub(crate) fn take(request: &mut JSONRPCRequest) -> io::Result<Option<Claim>> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid Source selection metadata",
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
    if request.method != "thread/resume"
        || [
            "alab.initialSession",
            "alab.clientConstruction",
            "alab.clientConstructionPrepare",
            "alab.retainedFrontier",
            "alab.retainedConfigured",
        ]
        .into_iter()
        .any(|key| meta.contains_key(key))
    {
        return Err(invalid());
    }
    let selection: Selection = serde_json::from_value(value).map_err(|_| invalid())?;
    if meta.is_empty() {
        params.remove("_meta");
    }
    if !selection.valid()
        || params.get("threadId").and_then(Value::as_str) != Some(selection.thread_id())
    {
        return Err(invalid());
    }
    Ok(Some(Claim(selection)))
}

#[cfg(test)]
#[path = "retained_source_selection_claim_tests.rs"]
mod tests;

pub(crate) use metadata::HistoryIntent;
