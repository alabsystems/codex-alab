// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use codex_app_server_protocol::JSONRPCRequest;
use serde_json::Value;
use std::io;
#[path = "retained_reader_metadata.rs"]
pub(crate) mod metadata;
pub(crate) use metadata::Selection;
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid retained reader metadata",
    )
}
pub(crate) fn take(request: &mut JSONRPCRequest) -> io::Result<Option<Selection>> {
    let Some(params) = request.params.as_mut().and_then(Value::as_object_mut) else {
        return Ok(None);
    };
    let Some(meta) = params.get_mut("_meta").and_then(Value::as_object_mut) else {
        return Ok(None);
    };
    let Some(data) = meta.get(metadata::KEY) else {
        return Ok(None);
    };
    if request.method != "config/read"
        || [
            "alab.initialSession",
            "alab.clientConstruction",
            "alab.retainedFrontier",
            "alab.clientConstructionPrepare",
            "alab.sourceSelectionFrontier",
            "alab.retainedConfigured",
        ]
        .into_iter()
        .any(|key| meta.contains_key(key))
    {
        return Err(invalid());
    }
    let selected: Selection = serde_json::from_value(data.clone()).map_err(|_| invalid())?;
    if !selected.valid() {
        return Err(invalid());
    }
    meta.remove(metadata::KEY);
    if meta.is_empty() {
        params.remove("_meta");
    }
    Ok(Some(selected))
}

#[cfg(test)]
#[path = "retained_reader_claim_tests.rs"]
mod tests;
