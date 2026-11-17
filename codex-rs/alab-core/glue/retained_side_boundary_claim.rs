// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use codex_app_server_protocol::JSONRPCRequest;
use serde_json::Value;
use std::io;
#[path = "retained_side_boundary_metadata.rs"]
pub(crate) mod metadata;
#[derive(Clone)]
pub(crate) struct Claim {
    pub selection: metadata::Selection,
    // Freeze whole ORIGINAL parameters, including foreign metadata. Only this
    // namespace was removed; the original typed parser still owns its errors.
    pub parameters: Value,
}
impl std::fmt::Debug for Claim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SideInjection(<DATA; requires actual accepted Scope and Native A>)")
    }
}
pub(crate) fn take(request: &mut JSONRPCRequest) -> io::Result<Option<Claim>> {
    let bad = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid Side injection metadata",
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
    // Runs FIRST: no competing parser may erase evidence before this check.
    if request.method != "thread/inject_items"
        || [
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
        return Err(bad());
    }
    let selection: metadata::Selection = serde_json::from_value(value).map_err(|_| bad())?;
    if !selection.valid() {
        return Err(bad());
    }
    if meta.is_empty() {
        params.remove("_meta");
    }
    Ok(Some(Claim {
        selection,
        parameters: Value::Object(params.clone()),
    }))
}

#[cfg(all(test, unix))]
#[path = "retained_side_boundary_claim_tests.rs"]
mod tests;
