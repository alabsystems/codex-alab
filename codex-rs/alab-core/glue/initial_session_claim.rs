// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALab-owned JSON-RPC parameter metadata; selection DATA, never permission.
//! Both original client conversion and server extraction use this same codec.
use codex_app_server_protocol::JSONRPCRequest;
use serde_json::Value;
use std::io;
use std::sync::Arc;
#[path = "initial_session_metadata.rs"]
mod metadata;
pub(crate) use metadata::InitialSessionResult;
pub(crate) use metadata::InitialSessionSelection;
use metadata::KEY;
pub(crate) use metadata::META;
pub(crate) use metadata::NextSelection;
pub(crate) use metadata::RESULT_KEY;

#[derive(Clone)]
pub(crate) struct Claim {
    pub(crate) selection: Arc<InitialSessionSelection>,
    /// Full original policy arguments, including other metadata. Request IDs
    /// and fresh observer tracing are not a request to repeat the Core effect.
    pub(crate) arguments: Arc<Value>,
    pub(crate) method: Arc<str>,
}
impl std::fmt::Debug for Claim {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("InitialSessionClaim(<selection data; requires actual Native origin>)")
    }
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid initial session metadata",
    )
}
/// Remove only this owned parameter metadata before the unchanged original
/// typed request parser/policy. Every actual request/context remains original.
pub(crate) fn take(request: &mut JSONRPCRequest) -> io::Result<Option<Claim>> {
    let Some(params) = request.params.as_mut().and_then(Value::as_object_mut) else {
        return Ok(None);
    };
    let Some(metadata) = params.get_mut(META).and_then(Value::as_object_mut) else {
        return Ok(None);
    };
    let Some(selection) = metadata.remove(KEY) else {
        return Ok(None);
    };
    let selection: InitialSessionSelection =
        serde_json::from_value(selection).map_err(|_| invalid())?;
    if !selection.valid() {
        return Err(invalid());
    }
    if !matches!(
        request.method.as_str(),
        "thread/start" | "thread/fork" | "thread/resume"
    ) {
        return Err(invalid());
    }
    if metadata.is_empty() {
        params.remove(META);
    }
    let arguments = Arc::new(Value::Object(params.clone()));
    Ok(Some(Claim {
        selection: Arc::new(selection),
        arguments,
        method: request.method.clone().into(),
    }))
}

#[path = "initial_selection_error.rs"]
mod selection_error;
pub(crate) use selection_error::decorate_error;

pub(crate) use metadata::CLIENT_KEY;
pub(crate) use metadata::CLIENT_RESULT_KEY;
pub(crate) use metadata::ClientConstructionResult;
pub(crate) use metadata::ClientConstructionSelection;
pub(crate) use metadata::ClientPresentation;
#[path = "client_construction_claim.rs"]
mod client_construction;
pub(crate) use client_construction::ClientClaim;
pub(crate) use client_construction::take_client;

#[cfg(all(test, unix))]
#[path = "initial_selection_publication_codec.rs"]
pub(crate) mod alab_publication_codec;

#[path = "client_construction_preparation_claim.rs"]
mod client_preparation;
pub(crate) use client_preparation::PreparationClaim;
pub(crate) use client_preparation::PreparationCompletion;
pub(crate) use client_preparation::PreparationOffer;
pub(crate) use client_preparation::PreparationRequest;
pub(crate) use client_preparation::PreparedArguments;
pub(crate) use client_preparation::take_preparation;
pub(crate) use metadata::preparation::RESULT_KEY as PREPARATION_RESULT_KEY;
pub(crate) use metadata::preparation::SavedResumeOutcome;
