// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real START→initial-turn component controls. The model endpoint is a local
//! protocol fixture; this is not Native SDK, recipe inheritance or product proof.

use super::*;
use crate::request_processors::thread_processor::alab_task_start::tests::Harness;
use crate::request_processors::thread_processor::alab_task_start::tests::completed;
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use codex_app_server_protocol::TurnToolOutput;
use codex_app_server_protocol::UserInput;
use core_test_support::responses;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use serde_json::json;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

fn initial_turn(h: &Harness) -> BackendInitialTurn {
    // Reuse the actual START fixture's manager/config/state/watch/skills owners.
    // The client-facing processor is never constructed as a facade for this call.
    TurnRequestProcessor::new(
        h.parent.thread_manager.clone(),
        h.processor.outgoing.clone(),
        AnalyticsEventsClient::disabled(),
        h.processor.config.clone(),
        h.processor.config_manager.clone(),
        h.processor.pending_thread_unloads.clone(),
        h.processor.thread_state_manager.clone(),
        h.processor.thread_watch_manager.clone(),
        h.processor.skills_watcher.clone(),
        h.processor.turn_cost_worker.clone(),
    )
    .alab_backend_initial_turn(h.input.clone())
}

fn operation_context() -> OperationContext {
    OperationContext {
        span: tracing::info_span!("actual_backend_initial_turn_test_operation"),
        parent_trace: None,
        cancellation: CancellationToken::new(),
    }
}

fn delegated_input(id: &str, text: &str) -> TurnStartParams {
    TurnStartParams {
        thread_id: id.to_string(),
        input: Vec::new(),
        model: None,
        tool_output: Some(Box::new(TurnToolOutput {
            name: "create_thread".to_string(),
            namespace: Some("codex_tui".to_string()),
            output: FunctionCallOutputBody::Text(text.to_string()),
            retain: false,
        })),
        ..Default::default()
    }
}

#[tokio::test]
async fn actual_child_initial_turn_sends_delegated_tool_output_and_returns_its_turn_id()
-> Result<()> {
    let mut h = Harness::new(32).await?;
    let (release, gate) = oneshot::channel();
    let (model, _finished) = start_streaming_sse_server(vec![vec![StreamingSseChunk {
        gate: Some(gate),
        body: responses::sse(vec![
            responses::ev_response_created("backend-initial"),
            responses::ev_assistant_message("backend-initial", "Done"),
            responses::ev_completed("backend-initial"),
        ]),
    }]])
    .await;
    let result: Result<()> = async {
        let mut request = h.request();
        let provider = request
            .params
            .model_provider
            .as_ref()
            .context("fixture provider absent")?;
        request
            .params
            .config
            .get_or_insert_with(Default::default)
            .insert(
                format!("model_providers.{provider}.base_url"),
                json!(format!("{}/v1", model.uri())),
            );
        let start = h
            .executor()
            .begin(request)
            .map_err(|error| anyhow::anyhow!("START begin code {}", error.code))?;
        let child = completed(&start).await?;
        let context = operation_context();
        let delegated = "Inspect the selected workspace and report the build result.";
        let turn = initial_turn(&h)
            .start(
                h.parent.session_configured.thread_id,
                h.parent.codex.clone(),
                child.thread.clone(),
                &context,
                delegated_input(&child.response.thread.id, delegated),
            )
            .await
            .map_err(|error| anyhow::anyhow!("initial turn code {}", error.code))?;
        ensure!(!turn.turn.id.is_empty(), "accepted initial turn had no ID");
        ensure!(
            turn.turn.status == TurnStatus::InProgress,
            "initial turn was not accepted in progress"
        );
        tokio::time::timeout(Duration::from_secs(30), model.wait_for_request_count(1))
            .await
            .context("actual child did not issue its model request")?;
        let requests = model.requests().await;
        ensure!(
            requests.len() == 1,
            "initial turn issued another model request before release"
        );
        let body: serde_json::Value = serde_json::from_slice(&requests[0])?;
        let input = body["input"]
            .as_array()
            .context("model request has no input array")?;
        let outputs = input
            .iter()
            .filter(|item| {
                item["type"] == "function_call_output"
                    && item["name"] == "create_thread"
                    && item["namespace"] == "codex_tui"
            })
            .collect::<Vec<_>>();
        ensure!(
            outputs.len() == 1,
            "delegated tool output was absent or duplicated"
        );
        ensure!(
            outputs[0]["output"] == delegated,
            "delegated prompt changed in actual model request"
        );
        ensure!(
            child
                .thread
                .current_turn_model(&turn.turn.id)
                .await
                .is_some(),
            "returned ID did not identify the child's real active model turn",
        );
        ensure!(
            child.thread.active_turn_root(&turn.turn.id).await == turn.turn.root_turn_id,
            "returned turn/root identity differs from the actual child",
        );
        h.no_client_reply()?;
        Ok(())
    }
    .await;
    // Release the real response stream on assertion error too, before joining
    // the actual START fixture's threads and shutting down this HTTP fixture.
    let _ = release.send(());
    let cleanup = h.shutdown().await;
    model.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn backend_initial_turn_keeps_tool_output_and_direct_input_validation() -> Result<()> {
    let mut h = Harness::new(32).await?;
    let result: Result<()> = async {
        let start = h
            .executor()
            .begin(h.request())
            .map_err(|error| anyhow::anyhow!("START begin code {}", error.code))?;
        let child = completed(&start).await?;
        let context = operation_context();
        let turn = initial_turn(&h);
        let mut mixed = delegated_input(&child.response.thread.id, "delegated");
        mixed.input = vec![UserInput::Text {
            text: "direct input".into(),
            text_elements: Vec::new(),
        }];
        let mut unnamed = delegated_input(&child.response.thread.id, "delegated");
        unnamed
            .tool_output
            .as_mut()
            .context("tool output absent")?
            .name
            .clear();
        let remote_image = TurnStartParams {
            thread_id: child.response.thread.id.clone(),
            input: vec![UserInput::Image {
                image: V2ImageReference::Inline {
                    url: "https://example.invalid/private-image".to_string(),
                },
                detail: None,
            }],
            ..Default::default()
        };
        for (params, message) in [
            (
                mixed,
                "`toolOutput` cannot be combined with nonempty `input`",
            ),
            (unnamed, "`toolOutput.name` must not be empty"),
            (remote_image, REMOTE_IMAGE_URL_ERROR),
        ] {
            let error = turn
                .start(
                    h.parent.session_configured.thread_id,
                    h.parent.codex.clone(),
                    child.thread.clone(),
                    &context,
                    params,
                )
                .await
                .err()
                .context("invalid initial turn was accepted")?;
            ensure!(error.code == -32600, "wrong invalid-input error category");
            ensure!(
                error.message == message,
                "initial-turn validation policy changed"
            );
        }
        ensure!(
            !child.thread.has_active_turn().await,
            "invalid input started a real child turn"
        );
        let requests = h
            ._server
            .received_requests()
            .await
            .context("HTTP capture unavailable")?;
        ensure!(
            !requests
                .iter()
                .any(|request| request.method.as_str() == "POST"
                    && request.url.path().ends_with("/responses")),
            "invalid input reached the actual model endpoint",
        );
        h.no_client_reply()?;
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn backend_initial_turn_cannot_substitute_the_parent_for_the_captured_child() -> Result<()> {
    let mut h = Harness::new(32).await?;
    let result: Result<()> = async {
        let start = h
            .executor()
            .begin(h.request())
            .map_err(|error| anyhow::anyhow!("START begin code {}", error.code))?;
        let child = completed(&start).await?;
        let context = operation_context();
        let error = initial_turn(&h)
            .start(
                h.parent.session_configured.thread_id,
                h.parent.codex.clone(),
                h.parent.codex.clone(),
                &context,
                delegated_input(&child.response.thread.id, "must not reach either runtime"),
            )
            .await
            .err()
            .context("parent substitution was accepted")?;
        ensure!(
            error.code == -32600,
            "wrong actual-child rejection category"
        );
        ensure!(
            !child.thread.has_active_turn().await,
            "rejected target started the child"
        );
        ensure!(
            !h.parent.codex.has_active_turn().await,
            "rejected target steered the parent"
        );
        h.no_client_reply()?;
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}
