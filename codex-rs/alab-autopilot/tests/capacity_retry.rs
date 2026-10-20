// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exercise ALab's capacity retry through a real core turn and an owned Responses mock.
//! No autopilot continuation or live provider is installed in these tests.

use std::time::Duration;

use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::json;
use wiremock::MockServer;
use wiremock::ResponseTemplate;

const MODEL: &str = "gpt-5.5";
const INPUT: &str = "Keep this same turn and input while the model is at capacity.";
// A missing event is a hang, not a latency failure. Capacity retries use their real clock.
const HANG_DETECTOR: Duration = Duration::from_secs(60);

/// Core turns need a larger stack than libtest's default; all tasks use this owned runtime.
fn big_stack<F, Fut>(test: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()>,
{
    let outcome = std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("owned test runtime")
                .block_on(async {
                    tokio::time::timeout(HANG_DETECTOR, test())
                        .await
                        .expect("capacity test hang detector");
                });
        })
        .expect("owned test thread")
        .join();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn build_test(server: &MockServer) -> TestCodex {
    codex_alab_core::activate();
    test_codex()
        .with_model(MODEL)
        .with_config(|config| {
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(0);
        })
        .build(server)
        .await
        .expect("core fixture with owned home and working directory")
}

async fn start_turn(test: &TestCodex) {
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: INPUT.to_owned(),
            text_elements: Vec::new(),
        }]))
        .await
        .expect("start the user turn");
}

async fn next_event(test: &TestCodex) -> EventMsg {
    test.codex.next_event().await.expect("core event").msg
}

async fn shutdown(test: &TestCodex) {
    test.codex
        .shutdown_and_wait()
        .await
        .expect("core shutdown acknowledgement");
}

fn overload(id: &str) -> ResponseTemplate {
    responses::sse_response(responses::sse_failed(
        id,
        "server_is_overloaded",
        "The selected model is at capacity.",
    ))
}

#[test]
fn capacity_twice_recovers_in_the_same_turn_with_zero_ordinary_retries() {
    big_stack(|| async {
        let server = responses::start_mock_server().await;
        let mock = responses::mount_response_sequence(
            &server,
            vec![
                overload("capacity-1"),
                overload("capacity-2"),
                responses::sse_response(responses::sse(vec![
                    responses::ev_response_created("recovered"),
                    responses::ev_assistant_message("recovered-msg", "capacity recovered"),
                    responses::ev_completed("recovered"),
                ])),
            ],
        )
        .await;
        let test = build_test(&server).await;
        start_turn(&test).await;

        let mut turns = Vec::new();
        let mut capacity_waits = 0;
        loop {
            match next_event(&test).await {
                EventMsg::TurnStarted(event) => turns.push(event.turn_id),
                EventMsg::StreamError(_) => capacity_waits += 1,
                EventMsg::Error(error) => panic!("capacity became terminal: {error:?}"),
                EventMsg::TurnAborted(event) => panic!("capacity aborted the turn: {event:?}"),
                EventMsg::TurnComplete(event) => {
                    assert!(event.error.is_none(), "{event:?}");
                    assert_eq!(
                        event.last_agent_message.as_deref(),
                        Some("capacity recovered")
                    );
                    assert_eq!(turns, vec![event.turn_id]);
                    break;
                }
                _ => {}
            }
        }
        shutdown(&test).await;

        assert_eq!(capacity_waits, 2);
        let requests = mock.requests();
        assert_eq!(requests.len(), 3);
        let original_input = requests[0].input();
        let user_messages = requests[0].message_input_texts("user");
        assert_eq!(user_messages.last().map(String::as_str), Some(INPUT));
        assert_eq!(
            user_messages.iter().filter(|text| *text == INPUT).count(),
            1
        );
        for request in &requests {
            assert_eq!(request.body_json()["model"], json!(MODEL));
            assert_eq!(request.input(), original_input);
        }
        assert_eq!(
            responses::received_responses_requests(&server).await.len(),
            3
        );
    });
}

#[test]
fn interrupt_cancels_a_capacity_wait_with_long_server_retry_advice() {
    big_stack(|| async {
        let server = responses::start_mock_server().await;
        let mock = responses::mount_response_once(
            &server,
            ResponseTemplate::new(503)
                .insert_header("Retry-After", "3600")
                .set_body_json(json!({
                    "error": {
                        "code": "server_is_overloaded",
                        "message": "The selected model is at capacity."
                    }
                })),
        )
        .await;
        let test = build_test(&server).await;
        start_turn(&test).await;

        let mut turns = Vec::new();
        loop {
            match next_event(&test).await {
                EventMsg::TurnStarted(event) => turns.push(event.turn_id),
                EventMsg::StreamError(_) => break,
                EventMsg::Error(error) => panic!("capacity became terminal: {error:?}"),
                EventMsg::TurnComplete(event) => panic!("capacity completed early: {event:?}"),
                EventMsg::TurnAborted(event) => panic!("unexpected interruption: {event:?}"),
                _ => {}
            }
        }
        test.codex.submit(Op::Interrupt).await.expect("interrupt");
        loop {
            match next_event(&test).await {
                EventMsg::TurnStarted(event) => turns.push(event.turn_id),
                EventMsg::TurnAborted(event) => {
                    assert_eq!(event.reason, TurnAbortReason::Interrupted);
                    assert_eq!(turns.len(), 1);
                    assert_eq!(event.turn_id.as_deref(), Some(turns[0].as_str()));
                    break;
                }
                EventMsg::Error(error) => panic!("interruption became an error: {error:?}"),
                EventMsg::TurnComplete(event) => panic!("interruption completed: {event:?}"),
                _ => {}
            }
        }
        // The shutdown acknowledgement closes the task before checking for stray retries.
        shutdown(&test).await;
        assert_eq!(mock.requests().len(), 1);
        assert_eq!(
            responses::received_responses_requests(&server).await.len(),
            1
        );
    });
}

async fn assert_terminal(response: ResponseTemplate, expected: CodexErrorInfo) {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_response_once(&server, response).await;
    let test = build_test(&server).await;
    start_turn(&test).await;

    let mut turns = Vec::new();
    let mut errors = 0;
    loop {
        match next_event(&test).await {
            EventMsg::TurnStarted(event) => turns.push(event.turn_id),
            EventMsg::StreamError(event) => panic!("noncapacity failure retried: {event:?}"),
            EventMsg::Error(error) => {
                errors += 1;
                assert_eq!(error.codex_error_info.as_ref(), Some(&expected));
            }
            EventMsg::TurnAborted(event) => panic!("terminal failure aborted: {event:?}"),
            EventMsg::TurnComplete(event) => {
                assert_eq!(turns, vec![event.turn_id]);
                assert_eq!(
                    event.error.and_then(|error| error.codex_error_info),
                    Some(expected)
                );
                break;
            }
            _ => {}
        }
    }
    shutdown(&test).await;
    assert_eq!(errors, 1);
    assert_eq!(mock.requests().len(), 1);
    assert_eq!(
        responses::received_responses_requests(&server).await.len(),
        1
    );
}

#[test]
fn quota_exhaustion_remains_terminal() {
    big_stack(|| async {
        assert_terminal(
            responses::sse_response(responses::sse_failed(
                "quota",
                "insufficient_quota",
                "The project has no remaining quota.",
            )),
            CodexErrorInfo::UsageLimitExceeded,
        )
        .await;
    });
}

#[test]
fn authentication_failure_remains_terminal() {
    big_stack(|| async {
        assert_terminal(
            ResponseTemplate::new(401).set_body_json(json!({
                "error": {
                    "type": "invalid_request_error",
                    "code": "invalid_api_key",
                    "message": "The captured test credential was rejected."
                }
            })),
            CodexErrorInfo::HttpConnectionFailed {
                http_status_code: Some(401),
            },
        )
        .await;
    });
}
