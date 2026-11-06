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
use codex_protocol::items::CollabAgentTool;
use codex_protocol::items::CollabAgentToolCallItem;
use codex_protocol::items::CollabAgentToolCallStatus;
use codex_protocol::items::TurnItem;
use tokio_util::sync::CancellationToken;

struct Transferring;

#[cfg(test)]
#[path = "exec_test_process.rs"]
mod test_process;

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
    use crate::tools::registry::CoreToolRuntime;
    use codex_protocol::protocol::EventMsg;

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
