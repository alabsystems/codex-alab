// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Public API contracts independent of the application and its test harness.

#[cfg(test)]
mod buffered_http_error_body_tests;
#[cfg(test)]
mod child_capture_tests;
#[cfg(test)]
mod http_error_body_capture_tests;
#[cfg(test)]
mod http_operation_diagnostic_tests;
#[cfg(test)]
mod http_retry_diagnostic_tests;
#[cfg(test)]
mod http_wire_retry_tests;
#[cfg(test)]
mod marker_capture_tests;
#[cfg(test)]
mod memories_decode_contract_tests;
#[cfg(test)]
mod sse_audio_rejection_tests;
#[cfg(test)]
mod sse_diagnostic_context_tests;
#[cfg(test)]
mod sse_fixture;
#[cfg(test)]
mod sse_invalid_request_tests;
#[cfg(test)]
#[path = "../../alab-core/glue/test_child_process.rs"]
mod test_child_process;
#[cfg(test)]
mod websocket_deflate_tests;
#[cfg(test)]
mod websocket_error_status_tests;
#[cfg(test)]
mod websocket_fixture;
#[cfg(test)]
mod websocket_invalid_request_tests;
#[cfg(test)]
mod websocket_nested_error_ids_tests;
#[cfg(test)]
mod websocket_proxy_error_privacy_tests;
#[cfg(test)]
mod websocket_rejection_body_tests;
#[cfg(test)]
mod websocket_semantic_error_ids_tests;

#[cfg(test)]
fn streaming_request(model: &str, prompt: impl Into<String>) -> codex_api::ResponsesApiRequest {
    use codex_protocol::models::ResponseInputItem;
    use codex_protocol::models::input_text;

    let message = ResponseInputItem::Message {
        role: "user".into(),
        content: vec![input_text(prompt)],
        phase: None,
    };
    codex_api::ResponsesApiRequest {
        model: model.into(),
        stream: true,
        service_tier: None,
        input: vec![message.into()],
        tools: None,

        tool_choice: "auto".into(),
        parallel_tool_calls: true,
        reasoning: None,
        store: false,
        stream_options: None,
        include: vec![],
        prompt_cache_key: None,
        prompt_cache_options: None,
        text: None,
        client_metadata: None,
        access_programs: None,
    }
}
