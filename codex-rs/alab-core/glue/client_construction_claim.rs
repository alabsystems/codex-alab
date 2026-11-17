// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual later UI construction DATA. The real request/context and Native
//! accepted origin supply custody/admission separately; metadata grants none.
use super::*;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct ClientClaim {
    pub(crate) selection: Arc<ClientConstructionSelection>,
    pub(crate) arguments: Arc<Value>,
    pub(crate) method: Arc<str>,
}
pub(crate) fn take_client(request: &mut JSONRPCRequest) -> io::Result<Option<ClientClaim>> {
    let Some(params) = request.params.as_mut().and_then(Value::as_object_mut) else {
        return Ok(None);
    };
    let Some(metadata) = params.get_mut(META).and_then(Value::as_object_mut) else {
        return Ok(None);
    };
    if metadata.contains_key(CLIENT_KEY) && metadata.contains_key(KEY) {
        return Err(invalid());
    }
    let Some(selection) = metadata.remove(CLIENT_KEY) else {
        return Ok(None);
    };
    if !matches!(
        request.method.as_str(),
        "thread/start" | "thread/fork" | "thread/resume"
    ) {
        return Err(invalid());
    }
    let selection: ClientConstructionSelection =
        serde_json::from_value(selection).map_err(|_| invalid())?;
    if request.method == "thread/start" && selection.presentation != ClientPresentation::Primary {
        return Err(invalid());
    }
    if metadata.is_empty() {
        params.remove(META);
    }
    Ok(Some(ClientClaim {
        selection: Arc::new(selection),
        arguments: Arc::new(Value::Object(params.clone())),
        method: request.method.clone().into(),
    }))
}

impl std::fmt::Debug for ClientClaim {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("ClientConstruction(<original Source DATA; requires actual origin>)")
    }
}
