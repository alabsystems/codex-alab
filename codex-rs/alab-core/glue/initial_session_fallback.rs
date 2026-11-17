// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exact original TUI history/dynamic-tool compatibility predicates.
//! Original UI and the one accepted startup worker share these bodies.
use super::super::alab_task_read::history;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ThreadForkParams;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadStartParams;

pub(crate) fn start_history(params: &ThreadStartParams, error: &JSONRPCErrorError) -> bool {
    params.history_mode.is_some() && history::is_history_pagination_unsupported(error)
}
pub(crate) fn start_dynamic(params: &ThreadStartParams, error: &JSONRPCErrorError) -> bool {
    params.dynamic_tools.is_some() && matches!(error.code, -32600 | -32602) && {
        let message = error.message.to_ascii_lowercase();
        ["dynamictools", "dynamic tool", "namespace", "inputschema"]
            .into_iter()
            .any(|field| message.contains(field))
    }
}
pub(crate) fn fork_history(params: &ThreadForkParams, error: &JSONRPCErrorError) -> bool {
    params.exclude_turns
        && !params.attach_to_source
        && history::is_history_pagination_unsupported(error)
}

pub(crate) fn resume_history(params: &ThreadResumeParams, error: &JSONRPCErrorError) -> bool {
    params.exclude_turns && history::is_history_pagination_unsupported(error)
}
