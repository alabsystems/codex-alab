// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Assertion adapters compiled only in the native TUI's test module.

/// Preserve the browser's deadline while reporting which asynchronous stage is pending.
#[expect(
    clippy::print_stderr,
    reason = "test-only phase diagnostics must survive a deadline failure"
)]
pub(crate) fn worktree_phase(name: &'static str) -> WorktreePhase {
    eprintln!("ALab worktree phase started: {name}");
    WorktreePhase {
        name,
        started: std::time::Instant::now(),
        completed: false,
    }
}

pub(crate) struct WorktreePhase {
    name: &'static str,
    started: std::time::Instant,
    completed: bool,
}

impl WorktreePhase {
    pub(crate) fn complete(mut self) {
        self.completed = true;
    }
}

impl Drop for WorktreePhase {
    #[expect(
        clippy::print_stderr,
        reason = "test-only phase diagnostics include cancelled futures"
    )]
    fn drop(&mut self) {
        eprintln!(
            "ALab worktree phase {}: {} after {:?}",
            if self.completed {
                "completed"
            } else {
                "left without completing"
            },
            self.name,
            self.started.elapsed()
        );
    }
}

/// A shared helper may reach the same inline snapshot from separate test cases.
/// Each invocation still checks the original expected content; only Insta's
/// process-wide prohibition on reusing the source location is scoped out.
pub(super) fn repeated_inline(assertion: impl FnOnce()) {
    insta::allow_duplicates!(assertion());
}

pub(super) fn followup_registration(event: &crate::app_event::AppEvent) {
    assert!(
        matches!(
            event,
            crate::app_event::AppEvent::DynamicToolThreadStarted { .. }
        ),
        "follow-up event was {event:?}"
    );
}

/// Let delegated inputs reach sampling instead of failing against an unserved backend.
pub(super) async fn lifecycle_model(backend: &wiremock::MockServer) {
    use core_test_support::responses;

    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path_regex(".*/responses$"))
        .respond_with(responses::sse_response(responses::sse(vec![
            responses::ev_response_created("lifecycle-response"),
            responses::ev_completed("lifecycle-response"),
        ])))
        .mount(backend)
        .await;
}

fn accepted_tool_output(body: &serde_json::Value, expected: &serde_json::Value) -> bool {
    body["generate"] != false
        && body["input"].as_array().is_some_and(|input| {
            input.iter().any(|item| {
                item["type"] == "function_call_output"
                    && item["name"] == expected["name"]
                    && item["namespace"] == expected["namespace"]
                    && item["output"] == expected["output"]
            })
        })
}

/// TurnStart acknowledges enqueueing. Sampling this exact input follows core's
/// record_pending_input durability barrier, including LiveThread metadata apply.
/// The single subsequent history read verifies that projection before a follow-up.
pub(super) async fn first_tool_output_materialized(
    backend: &wiremock::MockServer,
    server: &mut crate::app_server_session::AppServerSession,
    thread_id: codex_protocol::ThreadId,
    expected: &serde_json::Value,
) -> color_eyre::eyre::Result<()> {
    let expected_body: codex_protocol::models::FunctionCallOutputBody =
        serde_json::from_value(expected["output"].clone())?;
    tokio::time::timeout(std::time::Duration::from_secs(60), async {
        loop {
            if core_test_support::responses::received_responses_requests(backend)
                .await
                .iter()
                .any(|request| accepted_tool_output(&request.body_json(), expected))
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        let thread = server.thread_read(thread_id, true).await?;
        assert_eq!(thread.id, thread_id.to_string());
        assert!(
            thread
                .turns
                .iter()
                .flat_map(|turn| &turn.items)
                .any(|item| {
                    matches!(&item.payload, codex_app_server_protocol::ThreadItemPayload::FunctionCallOutput {
                    name, namespace, output, ..
                } if expected["name"] == name.as_str()
                    && expected["namespace"].as_str() == namespace.as_deref()
                    && output == &expected_body)
                }),
            "accepted delegation input is absent from stored history"
        );
        Ok::<_, color_eyre::Report>(())
    })
    .await??;
    Ok(())
}

#[test]
fn lifecycle_readiness_requires_the_accepted_delegation_input() {
    let expected = serde_json::json!({
        "name": "create_thread",
        "namespace": "codex_tui",
        "output": "delegated input",
    });
    let input = serde_json::json!({
        "type": "function_call_output",
        "name": "create_thread",
        "namespace": "codex_tui",
        "output": "delegated input",
    });
    assert!(!accepted_tool_output(
        &serde_json::json!({"generate": false, "input": [input]}),
        &expected,
    ));
    assert!(!accepted_tool_output(
        &serde_json::json!({"input": []}),
        &expected,
    ));
    let mut unrelated = input.clone();
    unrelated["namespace"] = serde_json::json!("codex_app");
    assert!(!accepted_tool_output(
        &serde_json::json!({"input": [unrelated]}),
        &expected,
    ));
    assert!(accepted_tool_output(
        &serde_json::json!({"input": [input]}),
        &expected,
    ));
}
