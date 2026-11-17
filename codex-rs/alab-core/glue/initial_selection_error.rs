// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Server-only lossless carrier. No original data shape or namespace is overwritten.
use super::metadata::NEXT_KEY;
use super::metadata::NextSelection;
use serde_json::Map;
use serde_json::Value;

/// The accepted worker supplies a validated backend-issued selection. Build the
/// carrier before storing that selection; this has no fallible serialization.
pub(crate) fn decorate_error(
    error: &codex_app_server_protocol::JSONRPCErrorError,
    selection: NextSelection,
) -> codex_app_server_protocol::JSONRPCErrorError {
    let selection = Value::Object(Map::from_iter([
        (
            "previousClaim".into(),
            Value::String(selection.previous_claim),
        ),
        ("claim".into(), Value::String(selection.claim)),
    ]));
    let offer = Value::Object(Map::from_iter([
        ("schema".into(), Value::from(1_u32)),
        ("kind".into(), Value::String("nextInitialSelection".into())),
        ("selection".into(), selection),
        ("hadData".into(), Value::Bool(error.data.is_some())),
        (
            "originalValue".into(),
            error.data.clone().unwrap_or(Value::Null),
        ),
    ]));
    let mut wire = error.clone();
    wire.data = Some(Value::Object(Map::from_iter([(NEXT_KEY.into(), offer)])));
    wire
}
