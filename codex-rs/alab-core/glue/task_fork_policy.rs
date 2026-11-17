// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The original task fork's pure schema, arguments and result in both hosts.
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ForkArguments {
    pub(crate) thread_id: Option<String>,
}

pub(crate) fn tool_definition() -> (&'static str, &'static str, Value, Vec<&'static str>) {
    (
        "fork_thread",
        "Fork a Codex task without starting a new turn. Omit threadId to fork the calling task.",
        json!({"threadId":{"type":"string","minLength":1}}),
        Vec::new(),
    )
}

pub(crate) fn result(source: &str, forked: &str) -> Value {
    json!({
        "environment": {"type": "same-directory"},
        "sourceThreadId": source,
        "threadId": forked,
        "continuation": "The fork contains completed history only. If the source thread was running, the active turn and unfinished response are not in the child. Send a follow-up message to threadId only if the task requires work to continue there."
    })
}
