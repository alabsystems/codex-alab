// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::ThreadRequestProcessor;
use crate::outgoing_message::ConnectionRequestId;
use codex_alab_core::diagnostic_id::SAVED_CHAT_RELEASE_PREFIX;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadUnsubscribeParams;
use codex_app_server_protocol::ThreadUnsubscribeResponse;
use codex_app_server_protocol::ThreadUnsubscribeStatus;
use codex_protocol::ThreadId;
use uuid::Uuid;

fn saved_release_id(request: &RequestId) -> Option<&str> {
    let RequestId::String(wire) = request else {
        return None;
    };
    let suffix = wire.strip_prefix(SAVED_CHAT_RELEASE_PREFIX)?;
    if suffix.len() != 36 || suffix.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return None;
    }
    Uuid::parse_str(suffix).ok()?;
    Some(wire)
}

/// Observe the typed handler result, before the caller stops MCP streams and publishes.
/// The request ID selects a diagnostic. It confers no subscription authority.
pub(super) async fn observe(
    processor: &ThreadRequestProcessor,
    request: &ConnectionRequestId,
    params: ThreadUnsubscribeParams,
) -> Result<ThreadUnsubscribeResponse, JSONRPCErrorError> {
    let selected = saved_release_id(&request.request_id)
        .map(|wire| (wire, ThreadId::from_string(&params.thread_id).ok()));
    let result = processor
        .thread_unsubscribe_response_inner(params, request.connection_id)
        .await;
    if let Some((wire_request, child)) = selected {
        let (outcome, server_code) = match &result {
            Ok(response) => {
                let status = match response.status {
                    ThreadUnsubscribeStatus::Unsubscribed => "unsubscribed",
                    ThreadUnsubscribeStatus::NotSubscribed => "notSubscribed",
                    ThreadUnsubscribeStatus::NotLoaded => "notLoaded",
                };
                (status, None)
            }
            Err(error) => ("rejected", Some(error.code)),
        };
        tracing::info!(
            target: "codex_alab::saved_chat_release",
            parent: None,
            connection_id = request.connection_id.0,
            wire_request = %wire_request,
            thread_id = child.as_ref().map(tracing::field::display),
            outcome = %outcome,
            server_code,
            "saved chat unsubscribe handler completed"
        );
    }
    result
}
