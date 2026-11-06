// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exercise ALab's capacity retry through a real core turn and an owned Responses mock.
//! No autopilot continuation or live provider is installed in these tests.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use codex_features::Feature;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::turn_input::TurnInputSubmission;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::json;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

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
                .unwrap_or_else(|err| panic!("owned test runtime: {err}"))
                .block_on(async {
                    tokio::time::timeout(HANG_DETECTOR, test())
                        .await
                        .unwrap_or_else(|err| panic!("capacity test hang detector: {err}"));
                });
        })
        .unwrap_or_else(|err| panic!("owned test thread: {err}"))
        .join();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn build_test(server: &MockServer) -> TestCodex {
    build_test_with_stream_retries(server, 0).await
}

async fn build_test_with_stream_retries(server: &MockServer, stream_retries: u64) -> TestCodex {
    codex_alab_core::activate();
    test_codex()
        .with_model(MODEL)
        .with_config(move |config| {
            config
                .features
                .enable(Feature::InstantInterrupt)
                .unwrap_or_else(|err| {
                    panic!("enable InstantInterrupt for steering coverage: {err}")
                });
            config.update_plan_enabled = true;
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(stream_retries);
        })
        .build(server)
        .await
        .unwrap_or_else(|err| panic!("core fixture with owned home and working directory: {err}"))
}

async fn start_turn(test: &TestCodex) {
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: INPUT.to_owned(),
            text_elements: Vec::new(),
        }]))
        .await
        .unwrap_or_else(|err| panic!("start the user turn: {err}"));
}

async fn next_event(test: &TestCodex) -> EventMsg {
    test.codex
        .next_event()
        .await
        .unwrap_or_else(|err| panic!("core event: {err}"))
        .msg
}

async fn shutdown(test: &TestCodex) {
    test.codex
        .shutdown_and_wait()
        .await
        .unwrap_or_else(|err| panic!("core shutdown acknowledgement: {err}"));
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
fn capacity_after_completed_work_honors_http_advice_without_repeating_the_tool() {
    big_stack(|| async {
        let server = responses::start_mock_server().await;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        Mock::given(method("POST"))
            .and(path_regex(".*/responses$"))
            .respond_with(move |request: &wiremock::Request| {
                let mut requests = captured
                    .lock()
                    .unwrap_or_else(|err| panic!("captured request lock: {err}"));
                requests.push((
                    Instant::now(),
                    request
                        .body_json::<serde_json::Value>()
                        .unwrap_or_else(|err| panic!("Responses request JSON: {err}")),
                ));
                match requests.len() {
                    1 => responses::sse_response(responses::sse(vec![
                        responses::ev_response_created("completed-work"),
                        responses::ev_function_call(
                            "completed-plan-call",
                            "update_plan",
                            &json!({"plan": [{
                                "step": "Keep completed work while capacity returns",
                                "status": "completed"
                            }]})
                            .to_string(),
                        ),
                        responses::ev_completed("completed-work"),
                    ])),
                    2 => ResponseTemplate::new(503)
                        .insert_header("Retry-After", "2")
                        .set_body_json(json!({"error": {
                            "code": "server_is_overloaded",
                            "message": "The selected model is at capacity."
                        }})),
                    3 => responses::sse_response(responses::sse(vec![json!({
                        "type": "response.failed",
                        "response": {"id": "streamed-capacity", "error": {
                            "code": "server_is_overloaded",
                            "message": "The selected model is still at capacity.",
                            "headers": {"Retry-After": "0"}
                        }}
                    })])),
                    4 => responses::sse_response(responses::sse(vec![
                        responses::ev_response_created("recovered"),
                        responses::ev_assistant_message("recovered-msg", "capacity recovered"),
                        responses::ev_completed("recovered"),
                    ])),
                    count => panic!("unexpected capacity request {count}"),
                }
            })
            .expect(4)
            .mount(&server)
            .await;
        let test = build_test(&server).await;
        start_turn(&test).await;

        let mut turns = Vec::new();
        let mut capacity_waits = 0;
        let mut completed_tools = 0;
        loop {
            match next_event(&test).await {
                EventMsg::TurnStarted(event) => turns.push(event.turn_id),
                EventMsg::PlanUpdate(_) => completed_tools += 1,
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
        assert_eq!(
            completed_tools, 1,
            "capacity does not execute completed work again"
        );
        let requests = requests
            .lock()
            .unwrap_or_else(|err| panic!("captured request lock: {err}"));
        assert_eq!(requests.len(), 4);
        assert!(
            requests[2].0 >= requests[1].0 + Duration::from_secs(2),
            "HTTP capacity advice is a lower bound for the in-turn retry"
        );
        assert!(
            requests[3].0 >= requests[2].0 + Duration::from_secs(1),
            "zero streamed advice keeps the second positive capacity backoff"
        );
        for (index, (_, request)) in requests.iter().enumerate() {
            assert_eq!(request["model"], json!(MODEL));
            let input = request["input"]
                .as_array()
                .unwrap_or_else(|| panic!("Responses input array: {request:?}"));
            let original_messages = input
                .iter()
                .filter(|item| {
                    item["role"] == "user"
                        && item["content"]
                            .as_array()
                            .is_some_and(|content| content.iter().any(|part| part["text"] == INPUT))
                })
                .count();
            assert_eq!(
                original_messages, 1,
                "the original task stays in context once"
            );
            if index > 0 {
                let outputs = input
                    .iter()
                    .filter(|item| {
                        item["type"] == "function_call_output"
                            && item["call_id"] == "completed-plan-call"
                    })
                    .count();
                assert_eq!(
                    outputs, 1,
                    "completed work stays in the recovered context once"
                );
            }
        }
    });
}

#[test]
fn steering_still_skips_ordinary_rate_limit_backoff_with_alab_active() {
    big_stack(|| async {
        const STEER: &str = "Use this clarification in the same task.";
        let server = responses::start_mock_server().await;
        let mock = responses::mount_sse_sequence(
            &server,
            vec![
                responses::sse_failed(
                    "ordinary-rate-limit",
                    "rate_limit_exceeded",
                    "Rate limit exceeded. Please try again in 60s.",
                ),
                responses::sse_completed("ordinary-steered-recovery"),
            ],
        )
        .await;
        let test = build_test_with_stream_retries(&server, 1).await;
        assert!(codex_alab_core::active());
        start_turn(&test).await;
        let mut turn_id = None;
        loop {
            match next_event(&test).await {
                EventMsg::TurnStarted(event) => {
                    assert!(turn_id.replace(event.turn_id).is_none(), "one native turn");
                }
                EventMsg::StreamError(event) => {
                    assert!(event.message.starts_with("Reconnecting..."), "{event:?}");
                    break;
                }
                EventMsg::TurnComplete(event) => panic!("ordinary retry ended early: {event:?}"),
                EventMsg::TurnAborted(event) => panic!("ordinary retry aborted early: {event:?}"),
                EventMsg::Error(error) => panic!("ordinary retry became terminal: {error:?}"),
                _ => {}
            }
        }
        let turn_id = turn_id.expect("the ordinary retry belongs to a started turn");
        let submitted = test
            .codex
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: STEER.into(),
                text_elements: Vec::new(),
            }]))
            .await
            .expect("steer during ordinary retry backoff");
        assert!(
            matches!(submitted, TurnInputSubmission::Steered { turn_id: ref steered, .. } if steered == &turn_id)
        );
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match next_event(&test).await {
                    EventMsg::TurnComplete(event) => {
                        assert_eq!(event.turn_id, turn_id);
                        assert!(event.error.is_none(), "{event:?}");
                        break;
                    }
                    EventMsg::TurnStarted(event) => {
                        panic!("ordinary steer started a new turn: {event:?}")
                    }
                    EventMsg::TurnAborted(event) => panic!("ordinary steer aborted: {event:?}"),
                    EventMsg::Error(error) => panic!("ordinary steer became terminal: {error:?}"),
                    _ => {}
                }
            }
        })
        .await
        .expect("active ALab must retain upstream's steering past a 60-second rate-limit wait");
        shutdown(&test).await;
        let requests = mock.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].body_json()["model"], json!(MODEL));
        let user_messages = requests[1].message_input_texts("user");
        assert_eq!(
            user_messages
                .iter()
                .filter(|text| text.as_str() == INPUT)
                .count(),
            1
        );
        assert_eq!(
            user_messages
                .iter()
                .filter(|text| text.as_str() == STEER)
                .count(),
            1
        );
    });
}

#[test]
fn steering_preserves_the_same_turns_http_capacity_deadline() {
    big_stack(|| steering_preserves_capacity_deadline(false));
}

#[test]
fn steering_preserves_the_same_turns_streamed_capacity_deadline() {
    big_stack(|| steering_preserves_capacity_deadline(true));
}

async fn steering_preserves_capacity_deadline(streamed: bool) {
    const STEER: &str = "Keep working and include this clarification.";
    let advice = Duration::from_secs(3);
    let server = responses::start_mock_server().await;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&requests);
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(move |request: &wiremock::Request| {
            let mut requests = captured
                .lock()
                .unwrap_or_else(|err| panic!("capture steered capacity requests: {err}"));
            requests.push((
                Instant::now(),
                request
                    .body_json::<serde_json::Value>()
                    .unwrap_or_else(|err| panic!("decode steered capacity request JSON: {err}")),
            ));
            match requests.len() {
                1 if streamed => responses::sse_response(responses::sse(vec![json!({
                    "type": "response.failed",
                    "response": {"id": "steered-capacity", "error": {
                        "code": "server_is_overloaded",
                        "message": "The selected model is at capacity.",
                        "headers": {"Retry-After": "3"}
                    }}
                })])),
                1 => ResponseTemplate::new(503)
                    .insert_header("Retry-After", "3")
                    .set_body_json(json!({"error": {
                        "code": "server_is_overloaded",
                        "message": "The selected model is at capacity."
                    }})),
                2 => responses::sse_response(responses::sse(vec![
                    responses::ev_response_created("steered-recovery"),
                    responses::ev_assistant_message("steered-message", "clarification received"),
                    responses::ev_completed("steered-recovery"),
                ])),
                count => panic!("unexpected steered capacity request {count}"),
            }
        })
        .expect(2)
        .mount(&server)
        .await;
    let test = build_test(&server).await;
    start_turn(&test).await;
    let turn_id = wait_for_capacity_notice(&test).await;
    let submitted = test
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: STEER.into(),
            text_elements: Vec::new(),
        }]))
        .await
        .unwrap_or_else(|err| panic!("accept steering during the capacity wait: {err}"));
    assert!(
        matches!(submitted, TurnInputSubmission::Steered { turn_id: ref steered, .. } if steered == &turn_id)
    );
    assert!(
        Instant::now()
            < requests
                .lock()
                .unwrap_or_else(|err| panic!("read pending steered capacity deadline: {err}"))[0]
                .0
                + advice,
        "the fixture must accept steering while the capacity deadline is still pending"
    );
    loop {
        match next_event(&test).await {
            EventMsg::TurnComplete(event) => {
                assert_eq!(event.turn_id, turn_id);
                assert!(event.error.is_none(), "{event:?}");
                assert_eq!(
                    event.last_agent_message.as_deref(),
                    Some("clarification received")
                );
                break;
            }
            EventMsg::TurnStarted(event) => panic!("steering started another turn: {event:?}"),
            EventMsg::TurnAborted(event) => panic!("steering aborted the turn: {event:?}"),
            EventMsg::Error(error) => panic!("steering made capacity terminal: {error:?}"),
            _ => {}
        }
    }
    shutdown(&test).await;
    let requests = requests
        .lock()
        .unwrap_or_else(|err| panic!("inspect completed steered capacity requests: {err}"));
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1].0 >= requests[0].0 + advice,
        "same-turn steering must keep the provider's capacity lower bound"
    );
    assert_eq!(requests[1].1["model"], json!(MODEL));
    let input = serde_json::to_string(&requests[1].1["input"])
        .unwrap_or_else(|err| panic!("encode recovered steered capacity input: {err}"));
    assert_eq!(input.matches(INPUT).count(), 1);
    assert_eq!(input.matches(STEER).count(), 1);
}

async fn wait_for_capacity_notice(test: &TestCodex) -> String {
    let mut turn_id = None;
    loop {
        match next_event(test).await {
            EventMsg::TurnStarted(event) => {
                assert!(turn_id.replace(event.turn_id).is_none(), "one native turn");
            }
            EventMsg::StreamError(event) => {
                assert_eq!(
                    event.message,
                    codex_alab_core::capacity::CAPACITY_RETRY_NOTICE
                );
                return turn_id.unwrap_or_else(|| panic!("capacity belongs to a started turn"));
            }
            EventMsg::TurnComplete(event) => panic!("capacity completed early: {event:?}"),
            EventMsg::TurnAborted(event) => panic!("capacity aborted early: {event:?}"),
            EventMsg::Error(error) => panic!("capacity became terminal: {error:?}"),
            _ => {}
        }
    }
}

#[test]
fn an_interrupt_after_steering_retires_capacity_pacing_before_a_new_turn() {
    big_stack(|| async {
        const STEER: &str = "This queued clarification must be cancelled.";
        const NEXT: &str = "This is the next explicit task.";
        let server = responses::start_mock_server().await;
        let mock = responses::mount_response_sequence(
            &server,
            vec![
                ResponseTemplate::new(503)
                    .insert_header("Retry-After", "3600")
                    .set_body_json(json!({"error": {
                        "code": "server_is_overloaded",
                        "message": "The selected model is at capacity."
                    }})),
                responses::sse_response(responses::sse(vec![
                    responses::ev_response_created("new-explicit-task"),
                    responses::ev_completed("new-explicit-task"),
                ])),
            ],
        )
        .await;
        let test = build_test(&server).await;
        start_turn(&test).await;
        let old_turn = wait_for_capacity_notice(&test).await;
        let submitted = test
            .codex
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: STEER.into(),
                text_elements: Vec::new(),
            }]))
            .await
            .expect("queue steering before the explicit interrupt");
        assert!(
            matches!(submitted, TurnInputSubmission::Steered { turn_id, .. } if turn_id == old_turn)
        );
        test.codex
            .submit(Op::Interrupt)
            .await
            .expect("interrupt the same waiting turn");
        loop {
            match next_event(&test).await {
                EventMsg::TurnAborted(event) => {
                    assert_eq!(event.turn_id.as_deref(), Some(old_turn.as_str()));
                    assert_eq!(event.reason, TurnAbortReason::Interrupted);
                    break;
                }
                EventMsg::TurnComplete(event) => {
                    panic!("the interrupted capacity turn completed: {event:?}")
                }
                EventMsg::Error(error) => panic!("interrupt became terminal error: {error:?}"),
                _ => {}
            }
        }
        assert_eq!(mock.requests().len(), 1, "no request escaped the old wait");
        let submitted = test
            .codex
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: NEXT.into(),
                text_elements: Vec::new(),
            }]))
            .await
            .expect("start the next explicit task");
        let TurnInputSubmission::Started {
            turn_id: next_turn, ..
        } = submitted
        else {
            panic!("the explicit task must start a new turn: {submitted:?}");
        };
        assert_ne!(next_turn, old_turn);
        loop {
            match next_event(&test).await {
                EventMsg::TurnComplete(event) => {
                    assert_eq!(event.turn_id, next_turn);
                    assert!(event.error.is_none(), "{event:?}");
                    break;
                }
                EventMsg::TurnAborted(event) => panic!("new task aborted: {event:?}"),
                EventMsg::Error(error) => panic!("new task failed: {error:?}"),
                _ => {}
            }
        }
        shutdown(&test).await;
        let requests = mock.requests();
        assert_eq!(requests.len(), 2);
        let user_messages = requests[1].message_input_texts("user");
        assert_eq!(
            user_messages
                .iter()
                .filter(|text| text.as_str() == NEXT)
                .count(),
            1
        );
        assert!(
            !user_messages.iter().any(|text| text == STEER),
            "cancelled queued input cannot leak into the new task"
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
