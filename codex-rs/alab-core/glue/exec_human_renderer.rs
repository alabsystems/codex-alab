// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    kind: String,
    version: u8,
    final_message: Option<String>,
    final_message_rendered: bool,
    emit_final_message_on_shutdown: bool,
    last_total_token_usage: Option<ThreadTokenUsage>,
    last_turn_diff: String,
}

pub(super) fn snapshot(
    renderer: &EventProcessorWithHumanOutput,
) -> Result<serde_json::Value, String> {
    serde_json::to_value(Snapshot {
        kind: "human".into(),
        version: 1,
        final_message: renderer.final_message.clone(),
        final_message_rendered: renderer.final_message_rendered,
        emit_final_message_on_shutdown: renderer.emit_final_message_on_shutdown,
        last_total_token_usage: renderer.last_total_token_usage.clone(),
        last_turn_diff: renderer.last_turn_diff.clone(),
    })
    .map_err(|_| "human renderer checkpoint could not be encoded".into())
}

pub(super) fn restore(
    renderer: &mut EventProcessorWithHumanOutput,
    value: serde_json::Value,
) -> Result<(), String> {
    let state: Snapshot = serde_json::from_value(value)
        .map_err(|_| "invalid human renderer checkpoint".to_owned())?;
    if state.kind != "human" || state.version != 1 {
        return Err("inconsistent human renderer checkpoint".into());
    }
    renderer.final_message = state.final_message;
    renderer.final_message_rendered = state.final_message_rendered;
    renderer.emit_final_message_on_shutdown = state.emit_final_message_on_shutdown;
    renderer.last_total_token_usage = state.last_total_token_usage;
    renderer.last_turn_diff = state.last_turn_diff;
    Ok(())
}
