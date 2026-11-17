// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Selection DATA extracted before the unchanged original typed parser. The
//! actual accepted A resource and Native checker remain separately required.
use codex_app_server_protocol::JSONRPCRequest;
use serde_json::Value;
use std::io;
#[path = "retained_frontier_metadata.rs"]
mod metadata;
pub(crate) use metadata::RESULT_KEY;
pub(crate) use metadata::Receipt;
pub(crate) use metadata::Selection;
#[derive(Clone)]
pub(crate) struct Claim(pub(crate) Selection);
impl std::fmt::Debug for Claim {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("RetainedFrontierClaim(<selection data; requires physical origin>)")
    }
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid retained frontier metadata",
    )
}
pub(crate) fn take(request: &mut JSONRPCRequest) -> io::Result<Option<Claim>> {
    let Some(params) = request.params.as_mut().and_then(Value::as_object_mut) else {
        return Ok(None);
    };
    let Some(meta) = params
        .get_mut(metadata::META)
        .and_then(Value::as_object_mut)
    else {
        return Ok(None);
    };
    let Some(selection) = meta.remove(metadata::KEY) else {
        return Ok(None);
    };
    let selection: Selection = serde_json::from_value(selection).map_err(|_| invalid())?;
    if request.method != "thread/resume" || !selection.valid() {
        return Err(invalid());
    }
    if meta.contains_key("alab.initialSession") || meta.contains_key("alab.clientConstruction") {
        return Err(invalid());
    }
    if meta.is_empty() {
        params.remove(metadata::META);
    }
    if params.get("threadId").and_then(Value::as_str) != Some(selection.thread_id()) {
        return Err(invalid());
    }
    Ok(Some(Claim(selection)))
}

#[cfg(test)]
#[path = "retained_frontier_claim_tests.rs"]
mod tests;

pub(crate) use metadata::HistoryIntent;
