// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The original config/read request remains typed. Only the owned metadata is
//! removed; real Native A/currentness and EmptyPrimary resources are required.
use codex_app_server_protocol::JSONRPCRequest;
use serde_json::Value;
use std::io;
#[path = "retained_configured_metadata.rs"]
mod metadata;
pub(crate) use metadata::RESULT_KEY;
pub(crate) use metadata::Receipt;
pub(crate) use metadata::Selection;
#[derive(Clone)]
pub(crate) struct Claim {
    pub(crate) selection: Selection,
    pub(crate) arguments: std::sync::Arc<Value>,
}
impl std::fmt::Debug for Claim {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("ConfiguredFrontierClaim(<Source DATA only>)")
    }
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid configured frontier metadata",
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
    let Some(selection) = meta.get(metadata::KEY) else {
        return Ok(None);
    };
    if request.method != "config/read"
        || [
            "alab.initialSession",
            "alab.clientConstruction",
            "alab.retainedFrontier",
            "alab.clientConstructionPrepare",
            "alab.sourceSelectionFrontier",
        ]
        .iter()
        .any(|key| meta.contains_key(*key))
    {
        return Err(invalid());
    }
    let selection: Selection = serde_json::from_value(selection.clone()).map_err(|_| invalid())?;
    if !selection.valid() {
        return Err(invalid());
    }
    meta.remove(metadata::KEY);
    if meta.is_empty() {
        params.remove(metadata::META);
    }
    Ok(Some(Claim {
        selection,
        arguments: std::sync::Arc::new(Value::Object(params.clone())),
    }))
}

#[cfg(test)]
#[path = "retained_configured_claim_tests.rs"]
mod tests;
