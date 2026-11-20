// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Local transfer cancellation is neither a user abort nor successful tool work.
use crate::function_tool::FunctionCallError;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::tools::context::AbortedToolOutput;
use crate::tools::registry::AnyToolResult;
use codex_alab_core::quota::ExecQuotaMember;
use codex_alab_core::quota::HeadlessExec;
use codex_history::ResponseItemEnvelope;
use codex_protocol::items::CollabAgentTool;
use codex_protocol::items::CollabAgentToolCallItem;
use codex_protocol::items::CollabAgentToolCallStatus;
use codex_protocol::items::TurnItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::ResponseItem;
use tokio_util::sync::CancellationToken;

struct Transferring;

#[cfg(test)]
use crate::alab_test_process as test_process;

pub(crate) fn mark(turn: &TurnContext) {
    if codex_alab_core::active()
        && (turn.extension_data.get::<HeadlessExec>().is_some()
            || turn.extension_data.get::<ExecQuotaMember>().is_some())
    {
        turn.extension_data.insert(Transferring);
    }
}

fn message(turn: &TurnContext) -> &'static str {
    if turn.extension_data.get::<Transferring>().is_some() {
        "Interrupted for account handoff."
    } else {
        "Wait interrupted by cancellation."
    }
}

pub(crate) fn attribute_abort(turn: &TurnContext, mut response: AnyToolResult) -> AnyToolResult {
    if turn.extension_data.get::<Transferring>().is_some() {
        response.result = Box::new(AbortedToolOutput {
            message: message(turn).into(),
        });
    }
    response
}

pub(crate) fn before_dispatch(
    turn: &TurnContext,
    mut response: ResponseItemEnvelope,
) -> ResponseItemEnvelope {
    if turn.extension_data.get::<Transferring>().is_some() {
        match &mut response.item {
            ResponseItem::FunctionCallOutput { output, .. }
            | ResponseItem::CustomToolCallOutput { output, .. } => {
                output.body = FunctionCallOutputBody::Text(
                    "Tool call did not run. Cancelled before dispatch for account handoff.".into(),
                );
            }
            // The Responses input schema carries a status and tool definitions, with no text field.
            ResponseItem::ToolSearchOutput { status, .. } => *status = "incomplete".into(),
            _ => {}
        }
    }
    response
}

/// The caller's runtime promises to let this handler finish its cancellation.
/// Only the tool attempt ends here; no new turn or execution authority is created.
pub(crate) async fn wait<F: std::future::Future>(
    session: &Session,
    turn: &TurnContext,
    call_id: &str,
    cancellation: &CancellationToken,
    pending: F,
) -> Result<F::Output, FunctionCallError> {
    if !codex_alab_core::active() {
        return Ok(pending.await);
    }
    tokio::select! {
        biased;
        result = pending => Ok(result),
        _ = cancellation.cancelled() => {
            session.emit_turn_item_completed(turn, TurnItem::CollabAgentToolCall(CollabAgentToolCallItem {
                id: call_id.into(), tool: CollabAgentTool::Wait,
                status: CollabAgentToolCallStatus::Interrupted,
                sender_thread_id: session.thread_id, receiver_thread_ids: Vec::new(),
                receiver_agents: Vec::new(), prompt: None, model: None,
                reasoning_effort: None, agents_states: Default::default(),
            })).await;
            Err(FunctionCallError::RespondToModel(message(turn).into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::step_context::StepContext;
    use crate::stream_events_utils::HandleOutputCtx;
    use crate::stream_events_utils::handle_output_item_done;
    use crate::tools::parallel::ToolCallRuntime;
    use crate::tools::registry::CoreToolRuntime;
    use codex_protocol::protocol::EventMsg;
    use serde_json::json;
    use std::sync::Arc;

    #[tokio::test]
    async fn queued_tool_results_keep_dispatch_boundary_and_handoff_reason() {
        if !test_process::child(concat!(
            module_path!(),
            "::queued_tool_results_keep_dispatch_boundary_and_handoff_reason"
        )) {
            return;
        }
        codex_alab_core::activate();
        for handoff in [false, true] {
            let (session, turn, _events) =
                crate::session::tests::make_session_and_context_with_rx().await;
            turn.extension_data.insert(HeadlessExec);
            let step = StepContext::for_test(Arc::clone(&turn));
            let mut ctx = HandleOutputCtx {
                tool_runtime: ToolCallRuntime::new(
                    Arc::clone(&session),
                    Arc::clone(&step),
                    Default::default(),
                ),
                sess: session,
                step_context: step,
                turn_store: Arc::new(codex_extension_api::ExtensionData::new(turn.sub_id.clone())),
                defer_tool_dispatch: true,
                cancellation_token: CancellationToken::new(),
            };
            let mut queued = Vec::new();
            for call in [
                json!({"type":"function_call", "call_id":"f", "name":"exec_command", "arguments":"{}"}),
                json!({"type":"custom_tool_call", "call_id":"c", "name":"apply_patch", "input":"patch"}),
                json!({"type":"tool_search_call", "call_id":"s", "execution":"client", "arguments":{"query":"files"}}),
            ] {
                let item = serde_json::from_value(call).unwrap();
                let result = handle_output_item_done(&mut ctx, item, None, None)
                    .await
                    .unwrap();
                queued.push(result.tool_future.expect("tool should be queued"));
            }
            if handoff {
                mark(&turn);
            }
            ctx.cancellation_token.cancel();
            let mut wire = Vec::new();
            for future in queued {
                wire.push(serde_json::to_value(future.await.unwrap().item).unwrap());
            }
            let reason = if handoff {
                "Tool call did not run. Cancelled before dispatch for account handoff."
            } else {
                "aborted before dispatch"
            };
            let status = if handoff { "incomplete" } else { "completed" };
            assert_eq!(
                wire,
                [
                    json!({"type":"function_call_output", "call_id":"f", "output":reason}),
                    json!({"type":"custom_tool_call_output", "call_id":"c", "output":reason}),
                    json!({"type":"tool_search_output", "call_id":"s", "execution":"client", "status":status, "tools":[]}),
                ]
            );
        }
    }

    #[tokio::test]
    async fn cancelled_wait_closes_once_with_the_transfer_reason() {
        if !test_process::child(concat!(
            module_path!(),
            "::cancelled_wait_closes_once_with_the_transfer_reason"
        )) {
            return;
        }
        codex_alab_core::activate();
        let (session, turn, events) =
            crate::session::tests::make_session_and_context_with_rx().await;
        turn.extension_data.insert(HeadlessExec);
        mark(&turn);
        let token = CancellationToken::new();
        token.cancel();
        assert!(
            crate::tools::handlers::multi_agents_v2::WaitAgentHandler::default()
                .finishes_on_cancellation()
        );
        let result = wait(
            &session,
            &turn,
            "original-wait",
            &token,
            std::future::pending::<()>(),
        )
        .await;
        let error = result.unwrap_err().to_string();
        assert!(error.contains("account handoff"));
        assert!(!error.contains("user"));
        let mut completed = 0;
        while let Ok(event) = events.try_recv() {
            if let EventMsg::ItemCompleted(event) = event.msg
                && let TurnItem::CollabAgentToolCall(item) = event.item
            {
                assert_eq!(item.id, "original-wait");
                assert_eq!(item.status, CollabAgentToolCallStatus::Interrupted);
                completed += 1;
            }
        }
        assert_eq!(completed, 1);
    }

    #[tokio::test]
    async fn ready_wait_wins_over_simultaneous_cancellation() {
        if !test_process::child(concat!(
            module_path!(),
            "::ready_wait_wins_over_simultaneous_cancellation"
        )) {
            return;
        }
        codex_alab_core::activate();
        let (session, turn, events) =
            crate::session::tests::make_session_and_context_with_rx().await;
        let token = CancellationToken::new();
        token.cancel();
        assert_eq!(
            wait(&session, &turn, "ready", &token, std::future::ready(7))
                .await
                .unwrap(),
            7
        );
        assert!(events.try_recv().is_err());
    }

    #[tokio::test]
    async fn ordinary_cancellation_is_not_called_an_account_handoff() {
        if !test_process::child(concat!(
            module_path!(),
            "::ordinary_cancellation_is_not_called_an_account_handoff"
        )) {
            return;
        }
        codex_alab_core::activate();
        let (session, turn, _) = crate::session::tests::make_session_and_context_with_rx().await;
        let token = CancellationToken::new();
        token.cancel();
        let error = wait(
            &session,
            &turn,
            "wait",
            &token,
            std::future::pending::<()>(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("cancellation"));
        assert!(!error.contains("account handoff"));
    }
}
