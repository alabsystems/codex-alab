// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A transient provider refusal must not finish a real collaboration child.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use codex_alab_core::capacity::CAPACITY_RETRY_NOTICE;
use codex_features::Feature;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::TurnAbortReason;
use core_test_support::responses;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use wiremock::Mock;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

const MODEL: &str = "gpt-5.5";
const SEED: &str = "This completed parent context must reach the full-history child.";
const CHILD_TASK: &str = "Finish the delegated task after a transient model-capacity refusal.";
const PLAN_CALL: &str = "child-completed-plan";
const ANSWER: &str = "child recovered without repeating completed work";

fn response(id: &str, answer: &str) -> ResponseTemplate {
    responses::sse_response(responses::sse(vec![
        responses::ev_response_created(id),
        responses::ev_assistant_message(&format!("message-{id}"), answer),
        responses::ev_completed(id),
    ]))
}

fn is_child(body: &Value) -> bool {
    body["client_metadata"]["parent_turn_id"].is_string()
}

#[test]
fn full_history_child_retries_capacity_without_reporting_terminal_failure() {
    run(child_recovery);
}

fn run<F, Fut>(test: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()>,
{
    // This owns every async task; a missing event is a hang, not a latency result.
    let outcome = std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap_or_else(|error| panic!("owned test runtime: {error:?}"))
                .block_on(async {
                    tokio::time::timeout(Duration::from_secs(60), test())
                        .await
                        .unwrap_or_else(|error| {
                            panic!("child capacity recovery hang detector: {error:?}")
                        });
                });
        })
        .unwrap_or_else(|error| panic!("owned test thread: {error:?}"))
        .join();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

struct FailedChild {
    test: TestCodex,
    child: Arc<codex_core::CodexThread>,
    child_requests: Arc<AtomicUsize>,
    parent_requests: Arc<Mutex<Vec<Value>>>,
    _server: wiremock::MockServer,
}

async fn spawned_failure(failure: ResponseTemplate) -> FailedChild {
    codex_alab_core::activate();
    let server = responses::start_mock_server().await;
    let parent_count = Arc::new(AtomicUsize::new(0));
    let child_count = Arc::new(AtomicUsize::new(0));
    let parent_requests = Arc::new(Mutex::new(Vec::new()));
    let counted_parent = Arc::clone(&parent_count);
    let counted_child = Arc::clone(&child_count);
    let captured_parent = Arc::clone(&parent_requests);
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(move |request: &wiremock::Request| {
            let body: Value = request.body_json().unwrap_or_else(|error| panic!("owned failure fixture JSON: {error:?}"));
            if is_child(&body) {
                counted_child.fetch_add(1, Ordering::SeqCst);
                return failure.clone();
            }
            let has_notification = body["input"].as_array().unwrap_or_else(|| panic!("root input")).iter()
                .any(|item| item["type"] == "agent_message");
            captured_parent.lock().unwrap_or_else(|error| panic!("captured parent request: {error:?}")).push(body);
            match counted_parent.fetch_add(1, Ordering::SeqCst) {
                0 => responses::sse_response(responses::sse(vec![
                    responses::ev_response_created("negative-parent-spawn"),
                    responses::ev_function_call_with_namespace(
                        "spawn-negative-child", "collaboration", "spawn_agent",
                        &json!({"task_name":"capacity_child", "message":CHILD_TASK, "fork_turns":"all"}).to_string(),
                    ),
                    responses::ev_completed("negative-parent-spawn"),
                ])),
                1 => response("negative-parent-idle", "Child started."),
                2 if !has_notification => responses::sse_response(responses::sse(vec![
                    responses::ev_response_created("negative-parent-wait"),
                    responses::ev_function_call_with_namespace("negative-wait", "collaboration", "wait_agent", "{}"),
                    responses::ev_completed("negative-parent-wait"),
                ])),
                2 | 3 if has_notification => response("negative-parent-observed", "Child result observed."),
                count => panic!("unexpected parent failure-fixture request {count}"),
            }
        })
        .mount(&server)
        .await;
    let test = test_codex()
        .with_model(MODEL)
        .with_config(|config| {
            config
                .features
                .enable(Feature::Collab)
                .unwrap_or_else(|error| panic!("enable collaboration: {error:?}"));
            config
                .features
                .enable(Feature::MultiAgentV2)
                .unwrap_or_else(|error| panic!("enable V2 children: {error:?}"));
            config.agent_default_subagent_model = Some(MODEL.to_owned());
            config.agent_default_subagent_reasoning_effort = Some(ReasoningEffort::High);
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(0);
            config.model_provider.supports_websockets = false;
        })
        .build(&server)
        .await
        .unwrap_or_else(|error| panic!("owned negative root fixture: {error:?}"));
    test.submit_turn("Spawn a child to exercise a provider failure.")
        .await
        .unwrap_or_else(|error| panic!("parent spawn turn: {error:?}"));
    let root_id = test.session_configured.thread_id;
    let child_id = test
        .thread_manager
        .list_thread_ids()
        .await
        .into_iter()
        .find(|id| *id != root_id)
        .unwrap_or_else(|| panic!("actual negative-test child"));
    let child = test
        .thread_manager
        .get_thread(child_id)
        .await
        .unwrap_or_else(|error| panic!("negative child thread: {error:?}"));
    FailedChild {
        test,
        child,
        child_requests: child_count,
        parent_requests,
        _server: server,
    }
}

async fn permanent_child_failure(response: ResponseTemplate, expected: CodexErrorInfo) {
    let fixture = spawned_failure(response).await;
    let mut turn = None;
    let mut errors = 0;
    loop {
        match fixture
            .child
            .next_event()
            .await
            .unwrap_or_else(|error| panic!("permanent child event: {error:?}"))
            .msg
        {
            EventMsg::TurnStarted(event) => {
                assert!(turn.replace(event.turn_id).is_none());
            }
            EventMsg::Error(error) => {
                errors += 1;
                assert_eq!(error.codex_error_info, Some(expected.clone()));
            }
            EventMsg::StreamError(error) => panic!("permanent child refusal retried: {error:?}"),
            EventMsg::TurnAborted(event) => panic!("permanent child refusal aborted: {event:?}"),
            EventMsg::TurnComplete(event) => {
                assert_eq!(Some(event.turn_id), turn);
                assert_eq!(
                    event.error.and_then(|error| error.codex_error_info),
                    Some(expected)
                );
                break;
            }
            _ => {}
        }
    }
    assert_eq!(errors, 1);
    assert!(matches!(
        fixture.child.agent_status().await,
        AgentStatus::Errored(_)
    ));
    fixture
        .test
        .submit_turn("Read the child failure.")
        .await
        .unwrap_or_else(|error| panic!("parent consumes refusal: {error:?}"));
    {
        let parents = fixture
            .parent_requests
            .lock()
            .unwrap_or_else(|error| panic!("permanent parent requests: {error:?}"));
        let input = &parents
            .last()
            .unwrap_or_else(|| panic!("parent refusal observation"))["input"];
        assert_eq!(
            input
                .as_array()
                .unwrap_or_else(|| panic!("parent refusal input"))
                .iter()
                .filter(|item| {
                    item["type"] == "agent_message"
                        && serde_json::to_string(item)
                            .unwrap_or_else(|error| panic!("child refusal JSON: {error:?}"))
                            .contains("Agent errored:")
                })
                .count(),
            1
        );
    }
    let shutdown = fixture
        .test
        .thread_manager
        .shutdown_all_threads_bounded(Duration::from_secs(5))
        .await;
    assert!(
        shutdown.submit_failed.is_empty() && shutdown.timed_out.is_empty(),
        "{shutdown:?}"
    );
    assert_eq!(
        fixture.child_requests.load(Ordering::SeqCst),
        1,
        "permanent refusal was not resubmitted"
    );
}

#[test]
fn a_child_forbidden_response_stays_terminal() {
    run(|| {
        permanent_child_failure(
        ResponseTemplate::new(403).set_body_json(json!({"error":{
            "type":"invalid_request_error", "code":"access_denied", "message":"The owned fixture denies access."
        }})),
        CodexErrorInfo::HttpConnectionFailed { http_status_code: Some(403) },
    )
    });
}

#[test]
fn a_child_forbidden_capacity_shaped_response_stays_terminal() {
    run(|| {
        permanent_child_failure(
            ResponseTemplate::new(403)
                .insert_header("Retry-After", "3600")
                .set_body_json(json!({"error": {
                    "code": "server_is_overloaded",
                    "message": "Selected model is at capacity. Please try a different model."
                }})),
            CodexErrorInfo::HttpConnectionFailed {
                http_status_code: Some(403),
            },
        )
    });
}

#[test]
fn a_child_policy_refusal_stays_terminal() {
    run(|| {
        permanent_child_failure(
            responses::sse_response(responses::sse_failed(
                "child-policy",
                "cyber_policy",
                "The owned fixture refuses this request.",
            )),
            CodexErrorInfo::CyberPolicy,
        )
    });
}

#[test]
fn a_child_quota_exhaustion_stays_terminal() {
    run(|| {
        permanent_child_failure(
            responses::sse_response(responses::sse_failed(
                "child-quota",
                "insufficient_quota",
                "The owned fixture has no quota.",
            )),
            CodexErrorInfo::UsageLimitExceeded,
        )
    });
}

#[test]
fn an_interrupt_stops_a_childs_capacity_retry() {
    run(|| async {
        let fixture = spawned_failure(responses::sse_response(responses::sse(vec![json!({
            "type":"response.failed", "response":{"id":"interrupted-capacity", "error":{
                "code":"server_is_overloaded", "message":"Selected model is at capacity. Please try a different model.",
                "headers":{"Retry-After":"3600"}
            }}
        })]))).await;
        loop {
            match fixture
                .child
                .next_event()
                .await
                .expect("child waiting event")
                .msg
            {
                EventMsg::StreamError(error) => {
                    assert_eq!(error.message, CAPACITY_RETRY_NOTICE);
                    break;
                }
                EventMsg::Error(error) => panic!("capacity became terminal: {error:?}"),
                EventMsg::TurnComplete(event) => panic!("capacity completed early: {event:?}"),
                EventMsg::TurnAborted(event) => panic!("capacity aborted early: {event:?}"),
                _ => {}
            }
        }
        assert_eq!(fixture.child.agent_status().await, AgentStatus::Running);
        fixture
            .child
            .submit(Op::Interrupt)
            .await
            .expect("interrupt the live child wait");
        loop {
            match fixture
                .child
                .next_event()
                .await
                .expect("child interrupt event")
                .msg
            {
                EventMsg::TurnAborted(event) => {
                    assert_eq!(event.reason, TurnAbortReason::Interrupted);
                    break;
                }
                EventMsg::Error(error) => panic!("interrupt became a terminal error: {error:?}"),
                EventMsg::TurnComplete(event) => panic!("interrupt completed child: {event:?}"),
                _ => {}
            }
        }
        assert_eq!(fixture.child.agent_status().await, AgentStatus::Interrupted);
        let shutdown = fixture
            .test
            .thread_manager
            .shutdown_all_threads_bounded(Duration::from_secs(5))
            .await;
        assert!(
            shutdown.submit_failed.is_empty() && shutdown.timed_out.is_empty(),
            "{shutdown:?}"
        );
        assert_eq!(
            fixture.child_requests.load(Ordering::SeqCst),
            1,
            "cancelled wait did not retry"
        );
        let parents = fixture
            .parent_requests
            .lock()
            .expect("interrupted parent requests");
        assert!(
            !serde_json::to_string(&*parents)
                .expect("parent JSON")
                .contains("Agent errored:")
        );
    });
}

async fn child_recovery() {
    codex_alab_core::activate();
    let server = responses::start_mock_server().await;
    let parent_calls = Arc::new(AtomicUsize::new(0));
    let child_calls = Arc::new(Mutex::new(Vec::<(Instant, Value)>::new()));
    let parent_requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured_child = Arc::clone(&child_calls);
    let captured_parent = Arc::clone(&parent_requests);
    let counted_parent = Arc::clone(&parent_calls);
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(move |request: &wiremock::Request| {
            let body: Value = request.body_json().unwrap_or_else(|error| panic!("owned Responses fixture JSON: {error:?}"));
            if is_child(&body) {
                let mut captured = captured_child.lock().unwrap_or_else(|error| panic!("child request lock: {error:?}"));
                captured.push((Instant::now(), body));
                match captured.len() {
                    1 => responses::sse_response(responses::sse(vec![
                        responses::ev_response_created("child-work"),
                        responses::ev_function_call(
                            PLAN_CALL,
                            "update_plan",
                            &json!({"plan": [{"step": "Keep completed child work", "status": "completed"}]}).to_string(),
                        ),
                        responses::ev_completed("child-work"),
                    ])),
                    2 => responses::sse_response(responses::sse(vec![json!({
                        "type": "response.failed",
                        "response": {"id": "child-capacity", "error": {
                            "code": "server_is_overloaded",
                            "message": "Selected model is at capacity. Please try a different model.",
                            "headers": {"Retry-After": "2"}
                        }}
                    })])),
                    3 => response("child-recovered", ANSWER),
                    count => panic!("unexpected child request {count}"),
                }
            } else {
                let consumed_child_result = serde_json::to_string(&body["input"])
                    .unwrap_or_else(|error| panic!("parent input JSON: {error:?}"))
                    .contains(ANSWER);
                captured_parent.lock().unwrap_or_else(|error| panic!("parent request lock: {error:?}")).push(body);
                match counted_parent.fetch_add(1, Ordering::SeqCst) {
                    0 => response("parent-seed", "Parent context completed."),
                    1 => responses::sse_response(responses::sse(vec![
                        responses::ev_response_created("parent-spawn"),
                        responses::ev_function_call_with_namespace(
                            "spawn-capacity-child",
                            "collaboration",
                            "spawn_agent",
                            &json!({"task_name": "capacity_child", "message": CHILD_TASK, "fork_turns": "all"}).to_string(),
                        ),
                        responses::ev_completed("parent-spawn"),
                    ])),
                    2 => response("parent-after-spawn", "Waiting for the delegated child."),
                    3 if !consumed_child_result => responses::sse_response(responses::sse(vec![
                        responses::ev_response_created("parent-waits-for-delivery"),
                        responses::ev_function_call_with_namespace(
                            "wait-for-capacity-child",
                            "collaboration",
                            "wait_agent",
                            "{}",
                        ),
                        responses::ev_completed("parent-waits-for-delivery"),
                    ])),
                    3 | 4 if consumed_child_result => {
                        response("parent-followup", "Received the child's successful result.")
                    }
                    count => panic!("unexpected parent request {count}"),
                }
            }
        })
        .mount(&server)
        .await;
    let test = test_codex()
        .with_model(MODEL)
        .with_config(|config| {
            config
                .features
                .enable(Feature::Collab)
                .unwrap_or_else(|error| panic!("enable collaboration: {error:?}"));
            config
                .features
                .enable(Feature::MultiAgentV2)
                .unwrap_or_else(|error| panic!("enable V2 children: {error:?}"));
            config.agent_default_subagent_model = Some(MODEL.to_owned());
            config.agent_default_subagent_reasoning_effort = Some(ReasoningEffort::High);
            config.update_plan_enabled = true;
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(0);
            config.model_provider.supports_websockets = false;
        })
        .build(&server)
        .await
        .unwrap_or_else(|error| panic!("owned root fixture: {error:?}"));
    test.submit_turn(SEED)
        .await
        .unwrap_or_else(|error| panic!("seed full-history fork: {error:?}"));
    test.submit_turn("Delegate the next task to a full-history child.")
        .await
        .unwrap_or_else(|error| panic!("complete parent spawn turn: {error:?}"));

    let root_id = test.session_configured.thread_id;
    let child_id = test
        .thread_manager
        .list_thread_ids()
        .await
        .into_iter()
        .find(|id| *id != root_id)
        .unwrap_or_else(|| panic!("actual spawned child"));
    let child = test
        .thread_manager
        .get_thread(child_id)
        .await
        .unwrap_or_else(|error| panic!("live child thread: {error:?}"));
    let mut turn_id = None;
    let mut plans = 0;
    loop {
        match child
            .next_event()
            .await
            .unwrap_or_else(|error| panic!("child event: {error:?}"))
            .msg
        {
            EventMsg::TurnStarted(event) => {
                assert!(turn_id.replace(event.turn_id).is_none(), "one child turn");
            }
            EventMsg::PlanUpdate(_) => plans += 1,
            EventMsg::StreamError(event) => {
                assert_eq!(event.message, CAPACITY_RETRY_NOTICE);
                break;
            }
            EventMsg::Error(event) => panic!("capacity became a terminal child error: {event:?}"),
            EventMsg::TurnComplete(event) => panic!("child completed before recovery: {event:?}"),
            EventMsg::TurnAborted(event) => panic!("capacity aborted the child: {event:?}"),
            _ => {}
        }
    }
    let turn_id = turn_id.unwrap_or_else(|| panic!("native child TurnStarted before capacity"));
    assert!(!turn_id.is_empty());
    assert_eq!(child.agent_status().await, AgentStatus::Running);
    let rollout = test
        .codex
        .rollout_path()
        .unwrap_or_else(|| panic!("root rollout"));
    let while_waiting = tokio::fs::read_to_string(&rollout)
        .await
        .unwrap_or_else(|error| panic!("parent history during wait: {error:?}"));
    assert!(!while_waiting.contains("Agent errored:"));
    assert!(!while_waiting.contains("Message Type: FINAL_ANSWER"));

    loop {
        match child
            .next_event()
            .await
            .unwrap_or_else(|error| panic!("child recovery event: {error:?}"))
            .msg
        {
            EventMsg::PlanUpdate(_) => plans += 1,
            EventMsg::TurnComplete(event) => {
                assert!(event.error.is_none(), "{event:?}");
                assert_eq!(event.turn_id, turn_id);
                assert_eq!(event.last_agent_message.as_deref(), Some(ANSWER));
                break;
            }
            EventMsg::TurnStarted(event) => {
                panic!("retry started a different child turn: {event:?}")
            }
            EventMsg::StreamError(event) => panic!("unexpected further retry: {event:?}"),
            EventMsg::Error(event) => panic!("child recovery failed: {event:?}"),
            EventMsg::TurnAborted(event) => panic!("child recovery aborted: {event:?}"),
            _ => {}
        }
    }
    assert_eq!(plans, 1, "completed tool work must not execute again");
    assert!(matches!(
        child.agent_status().await,
        AgentStatus::Completed(_)
    ));
    // Notifications are queue-only. The next parent request consumes delivery;
    // if it races that request, the real wait_agent tool synchronizes it.
    test.submit_turn("Read the completed delegated task.")
        .await
        .unwrap_or_else(|error| panic!("parent consumes completion: {error:?}"));
    let requests = child_calls
        .lock()
        .unwrap_or_else(|error| panic!("final child requests: {error:?}"))
        .clone();
    assert_eq!(requests.len(), 3);
    assert!(
        serde_json::to_string(&requests[0].1["input"])
            .unwrap_or_else(|error| panic!("full-history child seed JSON: {error:?}"))
            .contains(SEED),
        "full fork retained parent context"
    );
    assert!(
        requests[0].1["input"]
            .as_array()
            .unwrap_or_else(|| panic!("first child input"))
            .iter()
            .any(|item| {
                item["type"] == "agent_message"
                    && serde_json::to_string(item)
                        .unwrap_or_else(|error| panic!("delegated task JSON: {error:?}"))
                        .contains(CHILD_TASK)
            }),
        "the delegated task reaches the actual child's new-task message"
    );
    assert_eq!(requests[1].1["input"], requests[2].1["input"]);
    for (_, request) in requests.iter() {
        assert_eq!(request["model"], json!(MODEL));
        assert_eq!(request["reasoning"]["effort"], json!("high"));
        assert_eq!(
            request["client_metadata"]["turn_id"].as_str(),
            Some(turn_id.as_str()),
            "every request belongs to the observed native child turn"
        );
    }
    assert!(
        requests[2].0.duration_since(requests[1].0) >= Duration::from_secs(2),
        "provider retry deadline is a lower bound"
    );
    for (_, request) in &requests[1..] {
        assert_eq!(
            request["input"]
                .as_array()
                .unwrap_or_else(|| panic!("replayed child input"))
                .iter()
                .filter(
                    |item| item["type"] == "function_call_output" && item["call_id"] == PLAN_CALL
                )
                .count(),
            1
        );
    }
    drop(requests);
    let parents = parent_requests
        .lock()
        .unwrap_or_else(|error| panic!("final parent requests: {error:?}"))
        .clone();
    assert!(matches!(parents.len(), 4 | 5));
    for request in &parents {
        let input = serde_json::to_string(&request["input"])
            .unwrap_or_else(|error| panic!("parent request input JSON: {error:?}"));
        assert!(!input.contains("Agent errored:"));
        assert!(!input.contains("This agent's turn failed."));
    }
    let final_parent = parents
        .last()
        .unwrap_or_else(|| panic!("parent consuming child completion"));
    let consumed = serde_json::to_string(&final_parent["input"])
        .unwrap_or_else(|error| panic!("parent completion input JSON: {error:?}"));
    assert!(consumed.contains(ANSWER));
    assert!(!consumed.contains("Agent errored:"));
    assert_eq!(
        final_parent["input"]
            .as_array()
            .unwrap_or_else(|| panic!("parent completion input"))
            .iter()
            .filter(|item| {
                item["type"] == "agent_message"
                    && serde_json::to_string(item)
                        .unwrap_or_else(|error| {
                            panic!("child completion notification JSON: {error:?}")
                        })
                        .contains(ANSWER)
            })
            .count(),
        1,
        "one successful child notification in the parent's input"
    );
    drop(parents);
    let shutdown = test
        .thread_manager
        .shutdown_all_threads_bounded(Duration::from_secs(5))
        .await;
    assert!(
        shutdown.submit_failed.is_empty() && shutdown.timed_out.is_empty(),
        "owned threads must shut down: {shutdown:?}"
    );
}
