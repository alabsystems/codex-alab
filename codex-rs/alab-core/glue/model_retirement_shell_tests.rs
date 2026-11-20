// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::tools::context::ToolCallSource;
use crate::tools::context::ToolEnvironments;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::ExecCommandHandler;
use crate::tools::handlers::WriteStdinHandler;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use crate::turn_diff_tracker::TurnDiffTracker;
use serde_json::json;

fn invocation(
    session: &Arc<Session>,
    step: &Arc<StepContext>,
    tool: &str,
    arguments: serde_json::Value,
) -> ToolInvocation {
    ToolInvocation {
        session: Arc::clone(session),
        turn: Arc::clone(&step.turn),
        step_context: Arc::clone(step),
        cancellation_token: CancellationToken::new(),
        tracker: Arc::new(tokio::sync::Mutex::new(TurnDiffTracker::new())),
        call_id: format!("retired-{tool}"),
        tool_name: codex_tools::ToolName::plain(tool),
        source: ToolCallSource::Direct,
        payload: ToolPayload::Function {
            arguments: arguments.to_string(),
        },
        native_call: None,
        environments: Default::default(),
    }
}

pub(super) async fn launch(session: &Arc<Session>, step: &Arc<StepContext>) -> Vec<ResponseItem> {
    let arguments = json!({
        "cmd": "printf 'retirement-ready\\n'; read -r reply",
        "workdir": step.turn.config.codex_home.as_path(),
        "tty": true,
        "yield_time_ms": 250
    });
    let handler = ExecCommandHandler::default();
    let mut request = invocation(session, step, "exec_command", arguments.clone());
    request.environments = handler.environments_for_input(&request).unwrap();
    let call_id = request.call_id.clone();
    let payload = request.payload.clone();
    let output = handler.handle(request).await.unwrap();
    let result: ResponseItem = output.to_response_item(&call_id, &payload).into();
    let terminals = session.services.unified_exec_manager.list_processes().await;
    let text = serde_json::to_string(&result).unwrap();
    assert_eq!(terminals.len(), 1, "original exec_command result {text}");
    assert_eq!(terminals[0].item_id, call_id);
    assert!(text.contains("retirement-ready") && text.contains(&terminals[0].process_id));
    vec![
        serde_json::from_value(json!({
            "type":"function_call","name":"exec_command","arguments":arguments.to_string(),"call_id":call_id
        })).unwrap(),
        result,
    ]
}

pub(super) async fn verify_pending_operations(session: &Arc<Session>, step: &Arc<StepContext>) {
    let manager = &session.services.unified_exec_manager;
    assert!(step.alab_has_complete_retirement_input(session).await);
    let (number, lease) = manager.reserve_process_id().await;
    assert!(!step.alab_has_complete_retirement_input(session).await);
    manager.release_process_id(number).await;
    drop(lease);
    let id: i32 = manager.list_processes().await[0]
        .process_id
        .parse()
        .unwrap();
    let mut poll = invocation(
        session,
        step,
        "write_stdin",
        json!({
            "session_id": id, "chars": "", "yield_time_ms": 10000
        }),
    );
    poll.environments = ToolEnvironments::Terminal(manager.resolve_terminal(id).await.unwrap());
    let task = tokio::spawn(async move { WriteStdinHandler.handle(poll).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !manager.alab_terminal_operation_pending().await {
            assert!(
                !task.is_finished(),
                "the native poll returned before acquiring terminal I/O"
            );
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the original stdin poll never acquired terminal I/O");
    assert!(!step.alab_has_complete_retirement_input(session).await);
    task.abort();
    assert!(matches!(task.await, Err(error) if error.is_cancelled()));
    assert!(step.alab_has_complete_retirement_input(session).await);
}

pub(super) async fn finish(session: &Session, original: &[ResponseItem]) {
    let manager = &session.services.unified_exec_manager;
    let terminals = manager.list_processes().await;
    assert_eq!(terminals.len(), 1);
    assert_eq!(terminals[0].item_id, "retired-exec_command");
    assert!(
        serde_json::to_string(&original[1])
            .unwrap()
            .contains(&terminals[0].process_id)
    );
    assert!(
        manager
            .terminate_process(terminals[0].process_id.parse().unwrap())
            .await
    );
}
