// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Recovery re-decides one native failure under current policy and its original clocks.

use crate::decide::Act;
use crate::decide::Mode;
use crate::decide::Policy;
use crate::decide::Questions;
use crate::decide::Texts;
use crate::decide::ThreadState;
use crate::decide::Timing;
use crate::decide::TurnEnd;
use crate::decide::Wall;
use crate::decide::decide;
use crate::recovery;
use codex_alab_core::session::Recovery;
use codex_alab_core::session::RecoveryWall;
use codex_history::RolloutItem;
use codex_protocol::error::CodexErr;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::error::ResponseIncompleteError;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::EventMsg;
use serde_json::json;
use std::time::Duration;
use std::time::Instant;

const EPOCH: u64 = 1_800_000_000_000;
const MINUTE: u64 = 60_000;
const FAILED: &str = "the-native-failed-turn";

#[test]
fn native_turn_retries_opaque_http_400_but_not_a_specific_refusal() {
    native_request_recovery(false);
}

#[test]
fn native_websocket_handshake_retries_opaque_400_then_falls_back_to_http() {
    native_request_recovery(true);
}

fn native_request_recovery(websocket: bool) {
    core_test_support::test_codex::run_test_with_large_stack(
        "opaque request recovery",
        move || async move {
            use core_test_support::responses;
            use core_test_support::test_codex::test_codex;
            use wiremock::Mock;
            use wiremock::ResponseTemplate;
            use wiremock::matchers::method;
            use wiremock::matchers::path_regex;

            codex_alab_core::activate();
            for (body, recover) in [
                (json!({"detail":"Bad Request"}), true),
                (json!(["Bad Request"]), false),
                (
                    json!({"error":{"code":"invalid_request","message":"Invalid tool schema"}}),
                    false,
                ),
            ] {
                let server = responses::start_mock_server().await;
                let failure = ResponseTemplate::new(400).set_body_json(body);
                if websocket {
                    Mock::given(method("GET"))
                        .and(path_regex(".*/responses$"))
                        .respond_with(failure.clone())
                        .mount(&server)
                        .await;
                }
                let completion = responses::sse(vec![
                    responses::ev_response_created("recovered"),
                    responses::ev_assistant_message("answer", "RECOVERED"),
                    responses::ev_completed("recovered"),
                ]);
                let requests = if websocket {
                    responses::mount_sse_once(&server, completion).await
                } else {
                    let mut replies = vec![failure];
                    if recover {
                        replies.push(
                            ResponseTemplate::new(200)
                                .insert_header("content-type", "text/event-stream")
                                .set_body_string(completion),
                        );
                    }
                    responses::mount_response_sequence(&server, replies).await
                };
                let mut builder = test_codex().with_config(move |config| {
                    config.model_provider.supports_websockets = websocket;
                    config.model_provider.request_max_retries = Some(0);
                    config.model_provider.stream_max_retries = Some(2);
                });
                let test = builder.build(&server).await?;
                test.codex
                    .start_or_steer_turn(codex_core::TurnInputRequest::user_input(vec![
                        codex_protocol::user_input::UserInput::Text {
                            text: "Return RECOVERED.".into(),
                            text_elements: Vec::new(),
                        },
                    ]))
                    .await?;
                let end = core_test_support::wait_for_event(&test.codex, |event| {
                    matches!(event, EventMsg::TurnComplete(_))
                })
                .await;
                let EventMsg::TurnComplete(completed) = end else {
                    unreachable!()
                };
                assert_eq!(completed.error.is_none(), recover, "{completed:?}");
                assert_eq!(
                    requests.requests().len(),
                    match (websocket, recover) {
                        (false, true) => 2,
                        (false, false) | (true, true) => 1,
                        (true, false) => 0,
                    }
                );
                if websocket {
                    let received = server.received_requests().await.unwrap();
                    let handshakes = received
                        .iter()
                        .filter(|request| {
                            request.method == "GET" && request.url.path().ends_with("/responses")
                        })
                        .count();
                    // Startup prewarm and the original turn's handshake precede
                    // one clean HTTP replay, without retrying the rejected socket.
                    assert_eq!(handshakes, 2);
                }
                if recover {
                    assert_eq!(completed.last_agent_message.as_deref(), Some("RECOVERED"));
                    let input = requests.last_request().unwrap().message_input_texts("user");
                    assert_eq!(
                        input
                            .iter()
                            .filter(|text| text.as_str() == "Return RECOVERED.")
                            .count(),
                        1
                    );
                }
                test.codex.shutdown_and_wait().await?;
            }
            Ok(())
        },
    )
    .expect("native request recovery");
}

#[test]
fn native_opaque_handshake_fallback_preserves_provider_deadline() {
    core_test_support::test_codex::run_test_with_large_stack(
        "opaque handshake deadline",
        || async move {
            use core_test_support::responses;
            use core_test_support::test_codex::test_codex;
            use std::sync::Arc;
            use std::sync::Mutex;
            use wiremock::Mock;
            use wiremock::ResponseTemplate;
            use wiremock::matchers::method;
            use wiremock::matchers::path_regex;

            codex_alab_core::activate();
            for (body, recover) in [
                (json!({"detail":"Bad Request"}), true),
                (
                    json!({"error":{"code":"invalid_request","message":"Invalid tool schema"}}),
                    false,
                ),
            ] {
                let server = responses::start_mock_server().await;
                let handshakes = Arc::new(Mutex::new(Vec::new()));
                let posts = Arc::new(Mutex::new(Vec::new()));
                let observed_handshakes = Arc::clone(&handshakes);
                Mock::given(method("GET"))
                    .and(path_regex(".*/responses$"))
                    .respond_with(move |_: &wiremock::Request| {
                        observed_handshakes.lock().unwrap().push(Instant::now());
                        ResponseTemplate::new(400)
                            .insert_header("retry-after", "1")
                            .set_body_json(body.clone())
                    })
                    .mount(&server)
                    .await;
                let observed_posts = Arc::clone(&posts);
                Mock::given(method("POST"))
                    .and(path_regex(".*/responses$"))
                    .respond_with(move |_: &wiremock::Request| {
                        observed_posts.lock().unwrap().push(Instant::now());
                        ResponseTemplate::new(200)
                            .insert_header("content-type", "text/event-stream")
                            .set_body_string(responses::sse(vec![
                                responses::ev_response_created("recovered"),
                                responses::ev_assistant_message("answer", "RECOVERED"),
                                responses::ev_completed("recovered"),
                            ]))
                    })
                    .mount(&server)
                    .await;
                let test = test_codex()
                    .with_config(|config| {
                        config.model_provider.supports_websockets = true;
                        config.model_provider.request_max_retries = Some(0);
                        config.model_provider.stream_max_retries = Some(2);
                    })
                    .build(&server)
                    .await?;
                test.codex
                    .start_or_steer_turn(codex_core::TurnInputRequest::user_input(vec![
                        codex_protocol::user_input::UserInput::Text {
                            text: "Preserve this prompt through the rejected handshake.".into(),
                            text_elements: Vec::new(),
                        },
                    ]))
                    .await?;
                let end = core_test_support::wait_for_event(&test.codex, |event| {
                    matches!(event, EventMsg::TurnComplete(_))
                })
                .await;
                let EventMsg::TurnComplete(completed) = end else {
                    unreachable!()
                };
                assert_eq!(completed.error.is_none(), recover, "{completed:?}");
                let handshake_times = handshakes.lock().unwrap().clone();
                let post_times = posts.lock().unwrap().clone();
                assert_eq!(
                    handshake_times.len(),
                    2,
                    "prewarm and original request only"
                );
                assert_eq!(post_times.len(), usize::from(recover));
                if recover {
                    assert!(
                        post_times[0].duration_since(*handshake_times.last().unwrap())
                            >= Duration::from_secs(1),
                        "HTTP fallback must retain the rejected handshake's provider deadline"
                    );
                    let requests = responses::received_responses_requests(&server).await;
                    assert_eq!(requests.len(), 1);
                    let input = requests[0].message_input_texts("user");
                    assert_eq!(
                        input
                            .iter()
                            .filter(|text| {
                                text.as_str()
                                    == "Preserve this prompt through the rejected handshake."
                            })
                            .count(),
                        1
                    );
                    assert_eq!(completed.last_agent_message.as_deref(), Some("RECOVERED"));
                }
                test.codex.shutdown_and_wait().await?;
            }
            Ok(())
        },
    )
    .expect("native handshake fallback deadline");
}

#[test]
fn native_repeated_opaque_http_400_exhausts_retries_without_repeating_completed_work() {
    core_test_support::test_codex::run_test_with_large_stack(
        "bounded opaque recovery retains completed work",
        || async move {
            use core_test_support::responses;
            use core_test_support::test_codex::test_codex;
            use wiremock::ResponseTemplate;

            codex_alab_core::activate();
            for (body, failure_count) in [
                (json!({"detail":"Bad Request"}), 3),
                (
                    json!({"error":{"code":"invalid_request","message":"Invalid tool schema"}}),
                    1,
                ),
            ] {
                let server = responses::start_mock_server().await;
                let first = responses::sse(vec![
                    responses::ev_response_created("completed-work"),
                    responses::ev_function_call(
                        "completed-plan",
                        "update_plan",
                        &json!({"plan":[{"step":"Keep completed work","status":"completed"}]})
                            .to_string(),
                    ),
                    responses::ev_completed("completed-work"),
                ]);
                let mut replies = vec![
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "text/event-stream")
                        .set_body_string(first),
                ];
                replies.extend(std::iter::repeat_n(
                    ResponseTemplate::new(400).set_body_json(body),
                    failure_count,
                ));
                let requests = responses::mount_response_sequence(&server, replies).await;
                let test = test_codex()
                    .with_config(|config| {
                        config.update_plan_enabled = true;
                        config.model_provider.supports_websockets = false;
                        config.model_provider.request_max_retries = Some(0);
                        config.model_provider.stream_max_retries = Some(2);
                    })
                    .build(&server)
                    .await?;
                test.codex
                    .start_or_steer_turn(codex_core::TurnInputRequest::user_input(vec![
                        codex_protocol::user_input::UserInput::Text {
                            text: "Complete the plan once, then continue.".into(),
                            text_elements: Vec::new(),
                        },
                    ]))
                    .await?;
                let mut plan_updates = 0;
                let end = core_test_support::wait_for_event(&test.codex, |event| {
                    if matches!(event, EventMsg::PlanUpdate(_)) {
                        plan_updates += 1;
                    }
                    matches!(event, EventMsg::TurnComplete(_))
                })
                .await;
                let EventMsg::TurnComplete(completed) = end else {
                    unreachable!()
                };
                assert!(completed.error.is_some(), "{completed:?}");
                assert_eq!(
                    plan_updates, 1,
                    "completed tool executes once across retries"
                );
                let captured = requests.requests();
                assert_eq!(captured.len(), 1 + failure_count);
                let original = captured[0].body_json()["input"].as_array().unwrap().clone();
                let failed = captured[1].body_json()["input"].as_array().unwrap().clone();
                assert!(failed.starts_with(&original), "original input survives");
                let calls = failed
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| {
                        item["type"] == "function_call" && item["call_id"] == "completed-plan"
                    })
                    .collect::<Vec<_>>();
                let outputs = failed
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| {
                        item["type"] == "function_call_output"
                            && item["call_id"] == "completed-plan"
                    })
                    .collect::<Vec<_>>();
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].1["name"], "update_plan");
                assert_eq!(outputs.len(), 1);
                assert!(calls[0].0 < outputs[0].0, "call precedes its one result");
                for retry in captured.iter().skip(2) {
                    assert_eq!(retry.body_json()["input"], json!(failed));
                }
                test.codex.shutdown_and_wait().await?;
            }
            Ok(())
        },
    )
    .expect("bounded native request recovery");
}

#[test]
fn opaque_bad_requests_keep_retrying_instead_of_hitting_the_cutoff_hold() {
    let error = CodexErr::Stream(format!(
        "{}{}",
        codex_alab_core::request_error::OPAQUE_BAD_REQUEST_PREFIX,
        r#"{"detail":"Bad Request"}"#,
    ));
    let wall = crate::extension::classify(&error.to_codex_protocol_error(), error.details(), false)
        .unwrap();
    assert!(matches!(wall, Wall::Server { cut_off: false, .. }));
    let timing = Timing::default();
    let mut now = Instant::now();
    let mut state = state();
    for _ in 0..8 {
        state.observe_turn_end(
            TurnEnd {
                at: now,
                by_autopilot: true,
                by_person: false,
                text: None,
                earlier: None,
                questions: false,
                worked: Duration::ZERO,
                wall: Some(wall.clone()),
                aborted: false,
                ran_tools: false,
                restored: false,
            },
            &timing,
        );
        let retry = decide(&state, &policy(), &timing, now, Duration::ZERO);
        assert!(
            matches!(
                retry.act,
                Act::Type {
                    rule: crate::words::RULE_API_RETRY,
                    ..
                }
            ),
            "{retry:?}"
        );
        let due = retry.not_before.unwrap();
        assert!(due > now);
        state.acted(&retry.act, due, &timing);
        now = due + Duration::from_secs(1);
    }
}

#[test]
fn array_shaped_bad_requests_remain_terminal() {
    let error = CodexErr::InvalidRequest(r#"["Bad Request"]"#.into());
    let wall = crate::extension::classify(&error.to_codex_protocol_error(), error.details(), false);
    assert!(matches!(wall, Some(Wall::Fatal { .. })), "{wall:?}");
}

#[test]
fn a_saturated_restored_retry_count_keeps_the_slowest_rung() {
    let timing = Timing::default();
    let mut now = Instant::now();
    let wall = Wall::Server {
        what: "opaque backend HTTP 400".into(),
        cut_off: false,
        retry_at: None,
    };
    let mut current = at_wall(wall, now, &timing);
    current.wall =
        crate::decide::WallTrack::carried("server", now, u32::MAX, None, Some(now), false);
    let mut failed = current.point.clone().unwrap();
    for _ in 0..3 {
        let retry = decide(&current, &policy(), &timing, now, Duration::ZERO);
        assert!(matches!(
            retry.act,
            Act::Type {
                rule: crate::words::RULE_API_RETRY,
                ..
            }
        ));
        let due = now + *timing.retry_backoff.last().unwrap();
        assert_eq!(retry.not_before, Some(due));
        current.acted(&retry.act, due, &timing);
        assert_eq!(current.wall.as_ref().unwrap().attempts, u32::MAX);
        now = due + Duration::from_secs(1);
        failed.at = now;
        current.observe_turn_end(failed.clone(), &timing);
    }
}

#[test]
fn a_saturated_restored_short_streak_still_resets_after_real_work() {
    let now = Instant::now();
    let timing = Timing::default();
    for by_autopilot in [false, true] {
        let mut current = state();
        current.seed_pace(u32::MAX, None);
        if by_autopilot {
            current.acted(
                &Act::Type {
                    rule: crate::words::RULE_CONTINUE,
                    text: "continue".into(),
                },
                now,
                &timing,
            );
        }
        let mut end = TurnEnd {
            at: now,
            by_autopilot,
            by_person: false,
            text: None,
            earlier: None,
            questions: false,
            worked: Duration::ZERO,
            wall: None,
            aborted: false,
            ran_tools: false,
            restored: false,
        };
        current.observe_turn_end(end.clone(), &timing);
        assert_eq!(current.short_streak, u32::MAX);
        end.at += timing.min_work;
        end.worked = timing.min_work;
        current.observe_turn_end(end, &timing);
        assert_eq!(current.short_streak, 0);
    }
}

#[test]
fn recorded_raw_bad_request_requires_its_pacing_record() {
    let now = Instant::now();
    let items = [
        event(json!({
            "type":"task_started", "turn_id":FAILED,
            "turn_attribution":{"turn_id":FAILED,"turn_trigger":"user"},
            "model_context_window":258400,"collaboration_mode_kind":"default"
        })),
        event(json!({"type":"user_message","message":"Finish the task.",
            "images":null,"local_images":[],"text_elements":[]})),
        event(json!({"type":"task_complete","turn_id":FAILED,
            "last_agent_message":null,
            "error":{"message":r#"{"detail":"Bad Request"}"#,"codex_error_info":"other"}})),
    ];
    let timing = Timing::default();
    for mode in [Mode::Cruise, Mode::Land] {
        for relaunched in [false, true] {
            let mut restored = ThreadState::default();
            assert!(crate::extension::resume_into(
                &mut restored,
                &items,
                relaunched,
                None,
                0,
                now,
            ));
            let mut policy = policy();
            policy.autopilot = mode;
            let without_record = decide(&restored, &policy, &timing, now, Duration::ZERO);
            assert!(
                matches!(without_record.act, Act::Attention { .. }),
                "the raw body cannot replace a missing provider deadline: {without_record:?}",
            );
            let saved = Recovery {
                wall: RecoveryWall::Server {
                    what: "an API error (opaque backend HTTP 400)".into(),
                    cut_off: false,
                    retry_at_ms: Some(EPOCH + 40 * MINUTE),
                },
                ..server()
            };
            assert!(
                recovery::restore(
                    &mut restored,
                    &items,
                    &saved,
                    Some(&crate::recovery::test_sample(EPOCH)),
                    now,
                    &timing
                )
                .restored()
            );
            let retry = decide(&restored, &policy, &timing, now, Duration::ZERO);
            assert!(
                matches!(
                    retry.act,
                    Act::Type {
                        rule: crate::words::RULE_API_RETRY,
                        ..
                    }
                ),
                "{retry:?}"
            );
            assert_eq!(retry.not_before, Some(now + Duration::from_secs(40 * 60)));
            policy.retry_api_errors = false;
            assert!(matches!(
                decide(&restored, &policy, &timing, now, Duration::ZERO).act,
                Act::Attention { .. }
            ));
        }
    }
}

fn policy() -> Policy {
    Policy {
        continue_on: true,
        continue_text: Some("continue the current task".into()),
        continue_per_hour: None,
        approve_all: true,
        answer_questions: true,
        answer_text: None,
        questions: Questions::Recommended,
        retry_api_errors: true,
        probe_api: true,
        resume_limits: true,
        limit_wait: true,
        compact_on_context_wall: true,
        upgrade: true,
        upgrade_ask: false,
        facts: false,
        human_grace: Duration::ZERO,
        autopilot: Mode::Cruise,
        texts: Texts::default(),
    }
}

fn state() -> ThreadState {
    let mut state = ThreadState::default();
    state.observe_resume(true, None, false);
    state.observe_mode(Mode::Cruise);
    state
}

fn at_wall(wall: Wall, now: Instant, timing: &Timing) -> ThreadState {
    let mut state = state();
    state.observe_turn_end(
        TurnEnd {
            at: now,
            by_autopilot: true,
            by_person: false,
            text: None,
            earlier: None,
            questions: false,
            worked: Duration::ZERO,
            wall: Some(wall),
            aborted: false,
            ran_tools: false,
            restored: false,
        },
        timing,
    );
    state
}

fn event(value: serde_json::Value) -> RolloutItem {
    RolloutItem::EventMsg(serde_json::from_value::<EventMsg>(value).expect("native rollout event"))
}

fn complete(turn: &str, error: Option<CodexErrorInfo>) -> RolloutItem {
    event(json!({
        "type": "task_complete",
        "turn_id": turn,
        "last_agent_message": null,
        "error": error.map(|info| json!({
            "message": "the provider ended this turn",
            "codex_error_info": info,
        })),
    }))
}

#[test]
fn a_crash_record_still_marked_busy_cannot_generically_repeat_a_policy_refusal() {
    let now = Instant::now();
    let items = [
        event(json!({
            "type": "task_started", "turn_id": FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
            "model_context_window": 258400, "collaboration_mode_kind": "default",
        })),
        event(
            json!({"type":"user_message", "message":"Review the authorized task.",
            "images":null, "local_images":[], "text_elements":[]}),
        ),
        complete(FAILED, Some(CodexErrorInfo::CyberPolicy)),
    ];
    let mut resumed = ThreadState::default();
    assert!(crate::extension::resume_into(
        &mut resumed,
        &items,
        true,
        None,
        0,
        now,
    ));
    let decision = decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO);
    assert!(
        matches!(decision.act, Act::Type { rule: crate::words::RULE_POLICY_CONTINUE, ref text } if text == crate::words::POLICY_CONTINUE),
        "the native policy refusal must survive a lagging busy record: {decision:?}"
    );
}

#[test]
fn a_resumed_abort_is_not_a_busy_crash_carry_on() {
    let now = Instant::now();
    let items = [
        event(
            json!({"type":"task_started", "turn_id":FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
            "model_context_window":258400, "collaboration_mode_kind":"default"}),
        ),
        event(json!({"type":"turn_aborted", "turn_id":FAILED, "reason":"interrupted"})),
    ];
    let mut resumed = ThreadState::default();
    assert!(crate::extension::resume_into(
        &mut resumed,
        &items,
        true,
        None,
        0,
        now
    ));
    let decision = decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO);
    assert_eq!(decision.act, Act::Nothing, "{decision:?}");
}

#[test]
fn a_resumed_native_abort_requires_the_exact_signal_teardown_exception() {
    let now = Instant::now();
    let start = event(
        json!({"type":"task_started", "turn_id":FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
        "model_context_window":258400, "collaboration_mode_kind":"default"}),
    );
    for (turn, reason, allowed) in [
        (Some(FAILED), "interrupted", true),
        (None, "interrupted", false),
        (Some("another-turn"), "interrupted", false),
        (Some(FAILED), "replaced", false),
        (Some(FAILED), "review_ended", false),
        (Some(FAILED), "budget_limited", false),
    ] {
        let items = [
            start.clone(),
            event(json!({"type":"turn_aborted", "turn_id":turn, "reason":reason})),
        ];
        let mut resumed = ThreadState::default();
        assert!(crate::extension::resume_into(
            &mut resumed,
            &items,
            crate::resume_history::ResumeSource::Signal,
            None,
            0,
            now
        ));
        let decision = decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO);
        assert_eq!(
            matches!(
                decision.act,
                Act::Type {
                    rule: crate::words::RULE_HOST_TURN,
                    ..
                }
            ),
            allowed,
            "{turn:?} {reason}: {decision:?}"
        );
        if !allowed {
            assert_eq!(decision.act, Act::Nothing);
        }
    }
    let abort = event(json!({"type":"turn_aborted", "turn_id":FAILED, "reason":"interrupted"}));
    let correction = event(
        json!({"type":"user_message", "message":"a corrected request", "images":null,
        "local_images":[], "text_elements":[]}),
    );
    let items = [start, abort.clone(), correction, abort];
    let mut resumed = ThreadState::default();
    assert!(crate::extension::resume_into(
        &mut resumed,
        &items,
        crate::resume_history::ResumeSource::Signal,
        None,
        0,
        now
    ));
    assert_eq!(
        decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO).act,
        Act::Nothing
    );
}

#[test]
fn a_saved_retry_cannot_revive_after_new_native_input_or_abort() {
    let now = Instant::now();
    let start = event(
        json!({"type":"task_started", "turn_id":FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
        "model_context_window":258400, "collaboration_mode_kind":"default"}),
    );
    for barrier in [
        event(
            json!({"type":"user_message", "message":"a corrected request", "images":null,
            "local_images":[], "text_elements":[]}),
        ),
        event(json!({"type":"turn_aborted", "turn_id":FAILED, "reason":"interrupted"})),
        event(
            json!({"type":"task_started", "turn_id":"newer", "turn_attribution": {"turn_id": "newer", "turn_trigger": "user"},
            "model_context_window":258400, "collaboration_mode_kind":"default"}),
        ),
    ] {
        let items = [
            start.clone(),
            complete(FAILED, Some(CodexErrorInfo::ServerOverloaded)),
            barrier,
            complete(FAILED, Some(CodexErrorInfo::ServerOverloaded)),
        ];
        let mut resumed = state();
        assert!(
            !recovery::restore(
                &mut resumed,
                &items,
                &server(),
                Some(&crate::recovery::test_sample(EPOCH)),
                now,
                &Timing::default()
            )
            .restored()
        );
    }
}

#[test]
fn a_legacy_failure_without_classification_cannot_become_a_crash_continuation() {
    let now = Instant::now();
    let items = [
        event(
            json!({"type":"task_started", "turn_id":FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
            "model_context_window":258400, "collaboration_mode_kind":"default"}),
        ),
        event(json!({"type":"task_complete", "turn_id":FAILED,
            "error":{"message":"the previous provider failed without an error tag"}})),
    ];
    let mut resumed = ThreadState::default();
    assert!(crate::extension::resume_into(
        &mut resumed,
        &items,
        true,
        None,
        0,
        now
    ));
    let decision = decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO);
    assert!(
        matches!(decision.act, Act::Attention { .. }),
        "{decision:?}"
    );
    assert!(!resumed.carry_on);
    assert!(resumed.owed.is_none());
}

#[test]
fn a_resumed_other_failure_needs_its_saved_recovery_before_any_retry() {
    let now = Instant::now();
    let items = [
        event(
            json!({"type":"task_started", "turn_id":FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
            "model_context_window":258400, "collaboration_mode_kind":"default"}),
        ),
        complete(FAILED, Some(CodexErrorInfo::Other)),
    ];
    let mut resumed = ThreadState::default();
    assert!(crate::extension::resume_into(
        &mut resumed,
        &items,
        true,
        None,
        0,
        now
    ));
    let decision = decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO);
    assert!(
        matches!(decision.act, Act::Attention { .. }),
        "{decision:?}"
    );
    assert!(
        recovery::restore(
            &mut resumed,
            &items,
            &server(),
            Some(&crate::recovery::test_sample(EPOCH)),
            now,
            &Timing::default()
        )
        .restored()
    );
    let recovered = decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO);
    assert!(matches!(recovered.act, Act::Type { .. }), "{recovered:?}");
    assert!(recovered.not_before.is_some_and(|at| at > now));
}

#[test]
fn normalized_bad_request_keeps_provider_deadline_across_restart() {
    let now = Instant::now();
    let error = CodexErr::Stream(format!(
        "{}{}",
        codex_alab_core::request_error::OPAQUE_BAD_REQUEST_PREFIX,
        r#"{"detail":"Bad Request"}"#,
    ));
    let items = [
        event(json!({"type":"task_started", "turn_id":FAILED,
            "turn_attribution":{"turn_id":FAILED,"turn_trigger":"user"},
            "model_context_window":258400,"collaboration_mode_kind":"default"})),
        event(json!({"type":"task_complete", "turn_id":FAILED,
            "error":{"message":error.to_string(),
                "codex_error_info":error.to_codex_protocol_error()}})),
    ];
    let timing = Timing::default();
    let mut restored = ThreadState::default();
    assert!(crate::extension::resume_into(
        &mut restored,
        &items,
        true,
        None,
        0,
        now,
    ));
    assert!(
        matches!(
            decide(&restored, &policy(), &timing, now, Duration::ZERO).act,
            Act::Attention { .. }
        ),
        "the formatted error alone cannot recover the provider's deadline"
    );

    let saved = Recovery {
        wall: RecoveryWall::Server {
            what: "an API error (opaque backend HTTP 400)".into(),
            cut_off: false,
            retry_at_ms: Some(EPOCH + 40 * MINUTE),
        },
        ..server()
    };
    assert!(
        recovery::restore(
            &mut restored,
            &items,
            &saved,
            Some(&crate::recovery::test_sample(EPOCH)),
            now,
            &timing
        )
        .restored()
    );
    let retry = decide(&restored, &policy(), &timing, now, Duration::ZERO);
    assert!(matches!(
        retry.act,
        Act::Type {
            rule: crate::words::RULE_API_RETRY,
            ..
        }
    ));
    assert_eq!(retry.not_before, Some(now + Duration::from_secs(40 * 60)));
}

#[test]
fn a_valid_saved_recovery_keeps_its_carried_ladder_before_native_fallback() {
    let now = Instant::now();
    let timing = Timing::default();
    let cases = [
        (
            "server",
            CodexErrorInfo::Other,
            server().wall,
            Duration::from_secs(30 * 60),
        ),
        (
            "network",
            CodexErrorInfo::HttpConnectionFailed {
                http_status_code: None,
            },
            RecoveryWall::Network {
                what: "API unreachable".into(),
                measured: false,
            },
            Duration::from_secs(5 * 60),
        ),
        (
            "server",
            CodexErrorInfo::Other,
            RecoveryWall::Overlong {
                what: "output token limit".into(),
            },
            Duration::from_secs(15 * 60),
        ),
        (
            "entitlement",
            CodexErrorInfo::UsageLimitExceeded,
            RecoveryWall::Entitlement { quota: None },
            Duration::from_secs(360 * 60),
        ),
        (
            "usage",
            CodexErrorInfo::UsageLimitExceeded,
            RecoveryWall::Usage {
                quota: None,
                reset_at_ms: None,
                reset_label: None,
            },
            Duration::from_secs(30 * 60),
        ),
    ];
    for (class, info, wall, next_wait) in cases {
        let saved = Recovery {
            not_before_ms: EPOCH + 7 * 60 * MINUTE,
            wall,
            ..server()
        };
        let carried = crate::decide::WallTrack::carried(
            class,
            now - Duration::from_secs(10 * 60),
            2,
            Some(now - Duration::from_secs(60)),
            None,
            false,
        )
        .expect("carried wall class");
        let mut restored = state();
        restored.seed_pace(4, Some(carried.clone()));
        let items = [
            event(
                json!({"type":"task_started", "turn_id":FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
            "model_context_window":258400, "collaboration_mode_kind":"default"}),
            ),
            complete(FAILED, Some(info)),
        ];
        let mut recovered = false;
        assert!(crate::extension::resume_bound_with(
            &mut restored,
            &items,
            false,
            Some(crate::owed_origin::BoundHandoff::empty()),
            0,
            now,
            |state| {
                recovered = recovery::restore(
                    state,
                    &items,
                    &saved,
                    Some(&crate::recovery::test_sample(EPOCH)),
                    now,
                    &timing,
                )
                .restored();
                recovered
            }
        ));
        assert!(recovered);
        assert_eq!(
            restored.wall.as_ref(),
            Some(&carried),
            "{class}: a provisional fatal must not erase the carried rung"
        );
        assert_eq!(restored.short_streak, 4);
        let first = decide(&restored, &policy(), &timing, now, Duration::ZERO);
        let due = now + Duration::from_secs(7 * 60 * 60);
        assert_eq!(
            first.not_before,
            Some(due),
            "{class}: the exact saved lower bound is retained"
        );
        let wall = restored
            .point
            .as_ref()
            .and_then(|point| point.wall.clone())
            .expect("restored wall");
        restored.acted(&first.act, due, &timing);
        assert_eq!(restored.wall.as_ref().map(|track| track.attempts), Some(3));
        restored.observe_turn_end(
            TurnEnd {
                at: due + Duration::from_secs(1),
                by_autopilot: true,
                by_person: false,
                text: None,
                earlier: None,
                questions: false,
                worked: Duration::ZERO,
                wall: Some(wall),
                aborted: false,
                ran_tools: false,
                restored: false,
            },
            &timing,
        );
        assert_eq!(
            decide(
                &restored,
                &policy(),
                &timing,
                due + Duration::from_secs(1),
                Duration::ZERO
            )
            .not_before,
            // Counted from the failure's answer, a second after the try (aterm's ladder,
            // parity-3: every wall's ladder counts from its answer).
            Some(due + Duration::from_secs(1) + next_wait),
            "{class}: the next failure continues its carried retry rung"
        );
    }
}

#[test]
fn a_rejected_saved_recovery_keeps_native_fatal_attention_and_drops_the_old_ladder() {
    let now = Instant::now();
    let timing = Timing::default();
    let items = [
        event(
            json!({"type":"task_started", "turn_id":FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
        "model_context_window":258400, "collaboration_mode_kind":"default"}),
        ),
        complete(FAILED, Some(CodexErrorInfo::Other)),
    ];
    for saved in [
        Recovery {
            turn_id: "old-attempt".into(),
            ..server()
        },
        Recovery {
            wall: RecoveryWall::Network {
                what: "wrong old class".into(),
                measured: false,
            },
            ..server()
        },
    ] {
        let mut restored = state();
        restored.seed_pace(
            4,
            crate::decide::WallTrack::carried("server", now, 2, Some(now), None, false),
        );
        let mut recovered = true;
        assert!(crate::extension::resume_bound_with(
            &mut restored,
            &items,
            true,
            Some(crate::owed_origin::BoundHandoff::empty()),
            0,
            now,
            |state| {
                recovered = recovery::restore(
                    state,
                    &items,
                    &saved,
                    Some(&crate::recovery::test_sample(EPOCH)),
                    now,
                    &timing,
                )
                .restored();
                recovered
            }
        ));
        assert!(!recovered);
        assert!(matches!(
            decide(&restored, &policy(), &timing, now, Duration::ZERO).act,
            Act::Attention { .. }
        ));
        assert_eq!(
            restored
                .wall
                .as_ref()
                .and_then(crate::decide::WallTrack::carried_class),
            None
        );
        assert!(!restored.carry_on);
        assert!(restored.owed.is_none());
    }
}

#[test]
fn a_resumed_correction_or_abort_cannot_be_overwritten_by_an_old_completion() {
    let now = Instant::now();
    let start = event(
        json!({"type":"task_started", "turn_id":FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
        "model_context_window":258400, "collaboration_mode_kind":"default"}),
    );
    for barrier in [
        event(
            json!({"type":"user_message", "message":"a corrected request", "images":null,
            "local_images":[], "text_elements":[]}),
        ),
        event(json!({"type":"turn_aborted", "turn_id":FAILED, "reason":"interrupted"})),
    ] {
        let items = [
            start.clone(),
            complete(FAILED, Some(CodexErrorInfo::CyberPolicy)),
            barrier,
            complete(FAILED, Some(CodexErrorInfo::CyberPolicy)),
        ];
        let mut resumed = ThreadState::default();
        assert!(crate::extension::resume_into(
            &mut resumed,
            &items,
            false,
            None,
            0,
            now
        ));
        let decision = decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO);
        assert_eq!(decision.act, Act::Nothing, "{items:?}: {decision:?}");
    }
}

#[test]
fn definitive_native_refusals_restore_attention_without_replaying_old_acts() {
    let now = Instant::now();
    let refused = [
        CodexErrorInfo::BioPolicy,
        CodexErrorInfo::MisalignmentPolicyViolation,
        CodexErrorInfo::Unauthorized,
        CodexErrorInfo::BadRequest,
        CodexErrorInfo::InvalidPrompt,
        CodexErrorInfo::SessionBudgetExceeded,
        CodexErrorInfo::TooManyDenials,
        CodexErrorInfo::SandboxError,
        CodexErrorInfo::HttpConnectionFailed {
            http_status_code: Some(401),
        },
        CodexErrorInfo::ResponseStreamDisconnected {
            http_status_code: Some(403),
        },
    ];
    for info in refused {
        let items = [
            event(json!({
                "type": "task_started", "turn_id": FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
                "model_context_window": 258400, "collaboration_mode_kind": "default",
            })),
            complete(FAILED, Some(info.clone())),
        ];
        for relaunched in [false, true] {
            for mode in [Mode::Cruise, Mode::Land] {
                let mut resumed = ThreadState::default();
                let mut policy = policy();
                policy.autopilot = mode;
                assert!(crate::extension::resume_into(
                    &mut resumed,
                    &items,
                    relaunched,
                    None,
                    0,
                    now,
                ));
                let decision = decide(&resumed, &policy, &Timing::default(), now, Duration::ZERO);
                if matches!(
                    info,
                    CodexErrorInfo::BioPolicy | CodexErrorInfo::MisalignmentPolicyViolation
                ) {
                    assert!(
                        matches!(
                            decision.act,
                            Act::Type { rule, ref text }
                                if rule == crate::words::RULE_POLICY_CONTINUE
                                    && text == crate::words::POLICY_CONTINUE
                        ),
                        "{info:?}: {decision:?}"
                    );
                } else {
                    assert!(
                        matches!(decision.act, Act::Attention { .. }),
                        "{info:?}: {decision:?}"
                    );
                    assert_eq!(decision.not_before, None);
                }
                assert!(!resumed.carry_on);
                assert!(resumed.owed.is_none());
            }
        }
    }
}

#[test]
fn only_the_latest_native_attempt_can_leave_a_resumed_refusal_standing() {
    let now = Instant::now();
    let start = |turn| {
        event(json!({
            "type": "task_started", "turn_id": turn, "turn_attribution": {"turn_id": turn, "turn_trigger": "user"},
            "model_context_window": 258400, "collaboration_mode_kind": "default",
        }))
    };
    let base = vec![
        start(FAILED),
        complete(FAILED, Some(CodexErrorInfo::CyberPolicy)),
    ];
    let tails = [
        vec![start("corrected")],
        vec![start("corrected"), complete("corrected", None)],
        vec![event(
            json!({"type":"turn_aborted", "turn_id":FAILED, "reason":"interrupted"}),
        )],
        vec![event(
            json!({"type":"user_message", "message":"a corrected request", "images":null,
            "local_images":[], "text_elements":[]}),
        )],
        vec![
            start("corrected"),
            complete(FAILED, Some(CodexErrorInfo::CyberPolicy)),
        ],
    ];
    for tail in tails {
        let mut items = base.clone();
        items.extend(tail);
        let mut resumed = ThreadState::default();
        assert!(crate::extension::resume_into(
            &mut resumed,
            &items,
            false,
            None,
            0,
            now,
        ));
        let decision = decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO);
        assert_eq!(decision.act, Act::Nothing, "{items:?}: {decision:?}");
    }
    // A retryable failure and a terminal success gain no attention authority here.
    for info in [None, Some(CodexErrorInfo::ServerOverloaded)] {
        let mut resumed = ThreadState::default();
        assert!(crate::extension::resume_into(
            &mut resumed,
            &[start(FAILED), complete(FAILED, info)],
            false,
            None,
            0,
            now,
        ));
        assert_eq!(
            decide(&resumed, &policy(), &Timing::default(), now, Duration::ZERO).act,
            Act::Nothing
        );
    }
}

fn server() -> Recovery {
    Recovery {
        clock: crate::recovery::test_clock(EPOCH),
        thread: "the-primary-thread".into(),
        turn_id: FAILED.into(),
        point_at_ms: EPOCH,
        not_before_ms: EPOCH + 30 * MINUTE,
        by_autopilot: true,
        by_person: false,
        wall: RecoveryWall::Server {
            what: "model at capacity".into(),
            cut_off: false,
            retry_at_ms: Some(EPOCH + 20 * MINUTE),
        },
    }
}

fn usage() -> Recovery {
    Recovery {
        not_before_ms: EPOCH + 20 * MINUTE + 70_000,
        wall: RecoveryWall::Usage {
            quota: None,
            reset_at_ms: Some(EPOCH + 20 * MINUTE),
            reset_label: Some("the provider's original reset".into()),
        },
        ..server()
    }
}

fn assert_serialized_deadline(before: Option<Instant>, after: Option<Instant>) {
    let before = before.expect("original future wait");
    let after = after.expect("restored future wait");
    assert!(
        after >= before,
        "serialization cannot shorten a lower bound"
    );
    assert!(
        after <= before + Duration::from_millis(2),
        "fixed samples add at most two millisecond rounding steps"
    );
}

#[test]
fn a_floored_epoch_sample_never_shortens_a_future_provider_deadline() {
    let now = Instant::now();
    let timing = Timing::default();
    for fraction in [Duration::ZERO, Duration::from_nanos(1)] {
        let original = at_wall(
            Wall::Server {
                what: "model at capacity".into(),
                cut_off: false,
                retry_at: Some(now + Duration::from_secs(30 * 60) + fraction),
            },
            now,
            &timing,
        );
        let before = decide(&original, &policy(), &timing, now, Duration::ZERO);
        let captured = recovery::capture(
            "the-primary-thread",
            FAILED,
            &original,
            &before,
            false,
            now,
            EPOCH,
            Some(&crate::recovery::test_sample(EPOCH)),
            None,
        )
        .expect("future recovery intent");
        // Capture's hidden epoch fraction was 999 microseconds. One microsecond
        // later its floor advances a full millisecond; no wall-clock wait is used.
        let restored_now = now + Duration::from_micros(1);
        let mut restored = state();
        assert!(
            recovery::restore(
                &mut restored,
                &[complete(FAILED, Some(CodexErrorInfo::ServerOverloaded))],
                &captured,
                Some(&crate::recovery::test_sample(EPOCH + 1)),
                restored_now,
                &timing
            )
            .restored()
        );
        let after = decide(&restored, &policy(), &timing, restored_now, Duration::ZERO);
        assert_serialized_deadline(before.not_before, after.not_before);
        assert!(matches!(
            restored.point.as_ref().and_then(|point| point.wall.as_ref()),
            Some(Wall::Server { retry_at: Some(at), .. })
                if *at >= now + Duration::from_secs(30 * 60) + fraction
        ));
    }
}

#[test]
fn a_forward_wall_clock_step_cannot_shorten_a_restored_provider_wait() {
    use codex_alab_core::session::HandPid;
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::Launch;
    use codex_alab_core::session::LaunchWitness;
    use codex_alab_core::session::Predecessor;
    use codex_alab_core::session::Shared;
    use codex_alab_tui::record;

    const THREAD: &str = "the-primary-thread";
    let now = Instant::now();
    let timing = Timing::default();
    let provider_deadline = now + Duration::from_secs(600);
    let original = at_wall(
        Wall::Server {
            what: "an API error (opaque backend HTTP 400)".into(),
            cut_off: false,
            retry_at: Some(provider_deadline),
        },
        now,
        &timing,
    );
    let before = decide(&original, &policy(), &timing, now, Duration::ZERO);
    assert_eq!(before.not_before, Some(provider_deadline));
    let captured = recovery::capture(
        THREAD,
        FAILED,
        &original,
        &before,
        false,
        now,
        EPOCH,
        Some(&crate::recovery::test_sample(EPOCH)),
        None,
    )
    .expect("the relative provider wait is carried across a crash");

    // Only sixty seconds elapse on the monotonic clock. The wall clock steps
    // forward twenty minutes before the process restamps its record and dies.
    let resumed_now = now + Duration::from_secs(60);
    let resumed_epoch = EPOCH + 1_260_000;
    let args = vec!["/abs/bin/codex".to_owned(), "resume".into(), THREAD.into()];
    let argv: Vec<_> = args.iter().map(std::ffi::OsString::from).collect();
    let bytes = serde_json::to_vec(&json!({
        "v":1,
        "pid":41,
        "sessionId":THREAD,
        "version":"clock-step-fixture",
        "execGeneration":0,
        "resumeArgv":args,
        "env":{},
        "status":"idle",
        "state":"error",
        "draftEmpty":true,
        "autopilot":"active",
        "statusUpdatedAt":resumed_epoch,
        "updatedAt":resumed_epoch,
        "kind":"codex",
        "entrypoint":"codex",
        "recovery":record::Recovery::from(captured.clone()),
    }))
    .expect("serialize the predecessor's session record");
    let recorded: record::Record =
        serde_json::from_slice(&bytes).expect("the typed record remains readable");
    assert_eq!(
        recorded.recovery.clone().map(Recovery::from),
        Some(captured),
        "the normal record reader must retain the captured recovery"
    );
    assert!(record::relaunch_owed(&recorded));
    assert_eq!(
        record::judge(&recorded, &argv, resumed_epoch, 86_400_000),
        record::Sweep::Take
    );

    let shared = Shared::new();
    assert!(shared.set_launch(Launch {
        args: argv,
        started_at_ms: resumed_epoch,
        ..Default::default()
    }));
    shared.note_predecessor(Predecessor {
        pid: recorded.pid,
        session_id: recorded.session_id.expect("the primary thread"),
        exit: recorded.exit,
        resume_argv: recorded.resume_argv,
        status: Some("idle".into()),
        updated_at_ms: recorded.updated_at,
        recovery: recorded.recovery.map(Into::into),
        ..Default::default()
    });
    shared.set_swept();
    assert!(shared.set_launch_witness(Some(LaunchWitness {
        hand: Some("lease:aterm-harness@812".into()),
        human: HumanSeen::Never,
        read_at_ms: resumed_epoch,
        server_pid: Some(812),
        hand_pid: HandPid::Pid(812),
    })));
    let crate::extension::Relaunch::Recovery(selected) =
        crate::extension::relaunch(&shared, THREAD, resumed_epoch)
    else {
        panic!("the exact witnessed crash must select its provider recovery");
    };
    let items = [
        event(json!({
            "type":"task_started", "turn_id":FAILED,
            "turn_attribution":{"turn_id":FAILED,"turn_trigger":"user"},
            "model_context_window":258400,"collaboration_mode_kind":"default"
        })),
        complete(FAILED, Some(CodexErrorInfo::Other)),
    ];
    let mut restored = state();
    let mut recovered = false;
    assert!(crate::extension::resume_bound_with(
        &mut restored,
        &items,
        true,
        Some(crate::owed_origin::BoundHandoff::empty()),
        0,
        resumed_now,
        |state| {
            recovered = recovery::restore(
                state,
                &items,
                &selected,
                Some(&crate::recovery::test_sample(EPOCH + 60_000)),
                resumed_now,
                &timing,
            )
            .restored();
            recovered
        },
    ));
    assert!(recovered, "the saved intent matches the latest failed turn");
    assert_eq!(restored.point_turn_id.as_deref(), Some(FAILED));
    let disabled = Policy {
        retry_api_errors: false,
        ..policy()
    };
    assert!(matches!(
        decide(&restored, &disabled, &timing, resumed_now, Duration::ZERO).act,
        Act::Attention { .. }
    ));
    let after = decide(&restored, &policy(), &timing, resumed_now, Duration::ZERO);
    assert!(matches!(
        after.act,
        Act::Type {
            rule: crate::words::RULE_API_RETRY,
            ..
        }
    ));
    assert!(
        after.not_before.is_some_and(|at| at >= provider_deadline),
        "a wall-clock step must not spend the remaining nine minutes of the provider wait: {after:?}"
    );
}

#[test]
fn an_existing_boot_anchor_keeps_a_republished_wait_stable_across_a_wall_step() {
    let now = Instant::now();
    let timing = Timing::default();
    let original = at_wall(
        Wall::Server {
            what: "model at capacity".into(),
            cut_off: false,
            retry_at: Some(now + Duration::from_secs(600)),
        },
        now,
        &timing,
    );
    let before = decide(&original, &policy(), &timing, now, Duration::ZERO);
    let first = recovery::capture(
        "the-primary-thread",
        FAILED,
        &original,
        &before,
        false,
        now,
        EPOCH,
        Some(&recovery::test_sample(EPOCH)),
        None,
    )
    .unwrap();
    let later = now + Duration::from_secs(60);
    let after = decide(&original, &policy(), &timing, later, Duration::ZERO);
    let second = recovery::capture(
        "the-primary-thread",
        FAILED,
        &original,
        &after,
        false,
        later,
        EPOCH + 1_260_000,
        Some(&recovery::test_sample(EPOCH + 60_000)),
        Some(&first),
    )
    .unwrap();
    assert_eq!(
        second, first,
        "an unchanged wait must not churn its anchor or deadline"
    );
    let overflow = recovery::capture(
        "the-primary-thread",
        FAILED,
        &original,
        &before,
        false,
        now,
        u64::MAX,
        Some(&recovery::test_sample(EPOCH)),
        None,
    )
    .expect("an unrepresentable bound remains a known timing fact");
    assert!(overflow.clock.is_none());
    let mut restored = state();
    assert_eq!(
        recovery::restore(
            &mut restored,
            &[complete(FAILED, Some(CodexErrorInfo::ServerOverloaded))],
            &overflow,
            Some(&recovery::test_sample(EPOCH)),
            now,
            &timing,
        ),
        recovery::Outcome::RefusedClock,
        "a saturated timestamp must not become a verified already-due wait"
    );
}

#[test]
fn matching_unverifiable_recovery_blocks_carried_actions_until_a_new_native_point() {
    let now = Instant::now();
    let timing = Timing::default();
    let items = [complete(FAILED, Some(CodexErrorInfo::ServerOverloaded))];
    let valid = recovery::test_sample(EPOCH);
    let other_boot = codex_alab_core::recovery_clock::Sample {
        boot_id: "another-boot".into(),
        ..valid
    };
    let rewound = codex_alab_core::recovery_clock::Sample {
        continuous_ns: valid.continuous_ns - 1,
        ..valid.clone()
    };
    for (clock, sample) in [
        (None, Some(&valid)),
        (recovery::test_clock(EPOCH), None),
        (recovery::test_clock(EPOCH), Some(&other_boot)),
        (recovery::test_clock(EPOCH), Some(&rewound)),
    ] {
        let saved = Recovery { clock, ..server() };
        let mut restored = state();
        restored.carry_on = true;
        restored.owed = Some(crate::decide::Owed {
            native_origin: None,
            rule: crate::words::RULE_API_RETRY,
            text: "the old carried retry".into(),
            mode: Mode::Cruise,
            not_before: now,
            repo_owed: None,
            resume_goal: false,
            landing: false,
        });
        let origin = codex_alab_core::session::NativeOrigin {
            turn_id: FAILED.into(),
            phase: codex_alab_core::session::NativePhase::Complete,
        };
        restored.native_origin = Some(origin.clone());
        assert_eq!(
            recovery::restore(&mut restored, &items, &saved, sample, now, &timing),
            recovery::Outcome::RefusedClock,
        );
        assert!(!restored.carry_on);
        assert!(restored.owed.is_none());
        assert_eq!(restored.native_origin, Some(origin));
        assert!(matches!(
            restored
                .point
                .as_ref()
                .and_then(|point| point.wall.as_ref()),
            Some(Wall::Fatal { .. })
        ));
        crate::extension::rewatch_bound(&mut restored, &items, 0, now);
        assert!(
            restored.retry_clock_refused.is_some(),
            "re-reading the same failed completion retains its clock refusal"
        );
        restored.park_off();
        for mode in [Mode::Cruise, Mode::Land] {
            restored.observe_mode(mode);
            let live = Policy {
                autopilot: mode,
                ..policy()
            };
            assert!(matches!(
                decide(&restored, &live, &timing, now, Duration::ZERO).act,
                Act::Nothing | Act::Attention { .. }
            ));
        }
        let newer = [event(json!({
            "type":"task_started", "turn_id":"newer-native-turn",
            "turn_attribution":{"turn_id":"newer-native-turn","turn_trigger":"user"},
            "model_context_window":258400,"collaboration_mode_kind":"default"
        }))];
        assert!(crate::extension::resume_bound_with(
            &mut restored,
            &newer,
            crate::resume_history::ResumeSource::Crash,
            None,
            0,
            now,
            |_| false,
        ));
        assert!(restored.retry_clock_refused.is_none());
        assert!(
            restored.point.is_none(),
            "a superseded refusal must not leave its Fatal point behind"
        );
        assert!(
            restored.carry_on,
            "the newer native turn owns its independent witnessed continuation"
        );
        assert!(matches!(
            decide(&restored, &policy(), &timing, now, Duration::ZERO).act,
            Act::Type { .. }
        ));
        restored.observe_turn_start();
        assert!(
            restored.retry_clock_refused.is_none(),
            "a real new native start replaces the refused point"
        );
    }
}

#[test]
fn only_the_latest_matching_native_failure_can_restore_a_wait() {
    let now = Instant::now();
    let timing = Timing::default();
    let recovery = server();
    let failures = [
        complete("another-turn", Some(CodexErrorInfo::ServerOverloaded)),
        complete(FAILED, None),
        complete(FAILED, Some(CodexErrorInfo::UsageLimitExceeded)),
        complete(FAILED, Some(CodexErrorInfo::Unauthorized)),
        complete(FAILED, Some(CodexErrorInfo::BadRequest)),
        complete(FAILED, Some(CodexErrorInfo::ContextWindowExceeded)),
        complete(
            FAILED,
            Some(CodexErrorInfo::HttpConnectionFailed {
                http_status_code: Some(403),
            }),
        ),
        complete(
            FAILED,
            Some(CodexErrorInfo::HttpConnectionFailed {
                http_status_code: None,
            }),
        ),
        event(
            json!({"type": "task_started", "turn_id": "a-newer-turn", "turn_attribution": {"turn_id": "a-newer-turn", "turn_trigger": "user"}}),
        ),
        event(json!({"type": "user_message", "message": "a new instruction"})),
        event(json!({"type": "turn_aborted", "turn_id": FAILED, "reason": "interrupted"})),
    ];
    for newer in failures {
        let mut restored = state();
        let items = [
            complete(FAILED, Some(CodexErrorInfo::ServerOverloaded)),
            newer,
        ];
        assert!(
            !recovery::restore(
                &mut restored,
                &items,
                &recovery,
                Some(&crate::recovery::test_sample(EPOCH)),
                now,
                &timing
            )
            .restored()
        );
        assert!(restored.point.is_none());
        assert!(restored.recovery_not_before.is_none());
    }

    let items = [
        complete(FAILED, Some(CodexErrorInfo::ServerOverloaded)),
        event(json!({"type": "warning", "message": "a later informational warning"})),
    ];
    assert!(
        recovery::restore(
            &mut state(),
            &items,
            &recovery,
            Some(&crate::recovery::test_sample(EPOCH)),
            now,
            &timing
        )
        .restored()
    );
    let mut taskless = ThreadState::default();
    assert!(
        !recovery::restore(
            &mut taskless,
            &items,
            &recovery,
            Some(&crate::recovery::test_sample(EPOCH)),
            now,
            &timing
        )
        .restored()
    );
    assert!(
        !recovery::restore(
            &mut state(),
            &[complete(FAILED, Some(CodexErrorInfo::ServerOverloaded))],
            &usage(),
            Some(&crate::recovery::test_sample(EPOCH)),
            now,
            &timing
        )
        .restored()
    );
}

#[test]
fn native_capacity_and_processing_errors_keep_their_recorded_server_classification() {
    let now = Instant::now();
    let timing = Timing::default();
    let policy = policy();
    for (details, expected_info, provider_deadline) in [
        (CodexErrorDetails::Timeout, CodexErrorInfo::Other, None),
        (
            CodexErrorDetails::InvalidRequest(r#"{"detail":"Bad Request"}"#.into()),
            CodexErrorInfo::Other,
            None,
        ),
        (
            CodexErrorDetails::Stream(format!(
                "{}{}",
                codex_alab_core::request_error::OPAQUE_BAD_REQUEST_PREFIX,
                r#"{"detail":"Bad Request"}"#,
            )),
            CodexErrorInfo::Other,
            Some(now + Duration::from_secs(300)),
        ),
        (
            CodexErrorDetails::RequestTimeout,
            CodexErrorInfo::Other,
            None,
        ),
        (
            CodexErrorDetails::Stream("the response stream ended early".into()),
            CodexErrorInfo::Other,
            None,
        ),
        (
            CodexErrorDetails::PolicyCapacityUnavailable,
            CodexErrorInfo::PolicyCapacityUnavailable,
            Some(now + Duration::from_secs(300)),
        ),
    ] {
        let error = CodexErr::new(details);
        let info = error.to_codex_protocol_error();
        assert_eq!(info, expected_info);
        let mut wall = crate::extension::classify(&info, error.details(), false)
            .expect("the live native failure is recoverable");
        assert!(matches!(wall, Wall::Server { .. }));
        if let Wall::Server { retry_at, .. } = &mut wall {
            // The native error lifecycle retains the provider's advice on this wall.
            *retry_at = provider_deadline;
        }
        let original = at_wall(wall, now, &timing);
        let before = decide(&original, &policy, &timing, now, Duration::ZERO);
        if let Some(deadline) = provider_deadline {
            assert_eq!(before.not_before, Some(deadline));
        }
        let captured = recovery::capture(
            "the-primary-thread",
            FAILED,
            &original,
            &before,
            false,
            now,
            EPOCH,
            Some(&crate::recovery::test_sample(EPOCH)),
            None,
        )
        .expect("the original provider recovery intent");
        let mut restored = state();
        assert!(
            recovery::restore(
                &mut restored,
                &[complete(FAILED, Some(info))],
                &captured,
                Some(&crate::recovery::test_sample(EPOCH)),
                now,
                &timing
            )
            .restored()
        );
        let after = decide(&restored, &policy, &timing, now, Duration::ZERO);
        assert_eq!(after.act, before.act);
        assert_serialized_deadline(before.not_before, after.not_before);
    }
}

#[test]
fn network_overlong_and_entitlement_waits_round_trip_without_becoming_unguarded_prompts() {
    let now = Instant::now();
    let timing = Timing::default();
    let network = CodexErrorInfo::HttpConnectionFailed {
        http_status_code: None,
    };
    let network_wall = crate::extension::classify(
        &network,
        &CodexErrorDetails::Stream("no connection".into()),
        false,
    )
    .expect("native network classification");
    let incomplete = CodexErr::new(CodexErrorDetails::ResponseIncomplete(Box::new(
        ResponseIncompleteError {
            partial_output: Vec::new(),
            reason: "max_output_tokens".into(),
            response_id: None,
            token_usage: None,
        },
    )));
    let entitlement = CodexErr::new(CodexErrorDetails::QuotaExceeded);
    let cases = [
        (network_wall, network),
        (
            crate::extension::classify(
                &incomplete.to_codex_protocol_error(),
                incomplete.details(),
                false,
            )
            .expect("native output limit classification"),
            incomplete.to_codex_protocol_error(),
        ),
        (
            crate::extension::classify(
                &entitlement.to_codex_protocol_error(),
                entitlement.details(),
                false,
            )
            .expect("native entitlement classification"),
            entitlement.to_codex_protocol_error(),
        ),
    ];
    for (wall, info) in cases {
        let mut original = at_wall(wall.clone(), now, &timing);
        // An output-limit continuation can also be waiting on its retry ladder.
        if matches!(wall, Wall::Overlong { .. }) {
            original
                .wall
                .as_mut()
                .expect("output-limit ladder")
                .attempts = 1;
        }
        let before = decide(&original, &policy(), &timing, now, Duration::ZERO);
        assert!(matches!(before.act, Act::Type { .. }));
        assert!(before.not_before.is_some_and(|at| at > now));
        let captured = recovery::capture(
            "the-primary-thread",
            FAILED,
            &original,
            &before,
            false,
            now,
            EPOCH,
            Some(&crate::recovery::test_sample(EPOCH)),
            None,
        )
        .expect("this retryable wait crosses a crash");
        assert!(matches!(
            (&wall, &captured.wall),
            (Wall::Network { .. }, RecoveryWall::Network { .. })
                | (Wall::Overlong { .. }, RecoveryWall::Overlong { .. })
                | (Wall::Entitlement, RecoveryWall::Entitlement { .. })
        ));
        let stored: codex_alab_tui::record::Recovery = captured.into();
        let json = serde_json::to_string(&stored).expect("typed session recovery JSON");
        let recovered: codex_alab_tui::record::Recovery =
            serde_json::from_str(&json).expect("recovery JSON remains readable");
        let mut restored = state();
        assert!(
            recovery::restore(
                &mut restored,
                &[complete(FAILED, Some(info.clone()))],
                &recovered.into(),
                Some(&crate::recovery::test_sample(EPOCH)),
                now,
                &timing
            )
            .restored(),
            "{wall:?} with native protocol error {info:?}"
        );
        let after = decide(&restored, &policy(), &timing, now, Duration::ZERO);
        assert_eq!(after.act, before.act);
        assert_serialized_deadline(before.not_before, after.not_before);
        let disabled = Policy {
            retry_api_errors: false,
            resume_limits: false,
            ..policy()
        };
        assert!(matches!(
            decide(&restored, &disabled, &timing, now, Duration::ZERO).act,
            Act::Attention { .. }
        ));
    }
}

#[test]
fn auth_fatal_and_policy_refusals_never_create_recovery_metadata() {
    let now = Instant::now();
    let timing = Timing::default();
    for info in [
        CodexErrorInfo::Unauthorized,
        CodexErrorInfo::BadRequest,
        CodexErrorInfo::CyberPolicy,
        CodexErrorInfo::ContextWindowExceeded,
    ] {
        let wall = crate::extension::classify(
            &info,
            &CodexErrorDetails::Stream("terminal refusal".into()),
            false,
        )
        .expect("the native refusal is classified");
        let original = at_wall(wall, now, &timing);
        let decision = decide(&original, &policy(), &timing, now, Duration::ZERO);
        if info == CodexErrorInfo::CyberPolicy {
            assert!(matches!(
                decision.act,
                Act::Type {
                    rule: crate::words::RULE_POLICY_CONTINUE,
                    ..
                }
            ));
        } else {
            assert!(matches!(decision.act, Act::Attention { .. }));
        }
        assert!(
            recovery::capture(
                "the-primary-thread",
                FAILED,
                &original,
                &decision,
                false,
                now,
                EPOCH,
                Some(&crate::recovery::test_sample(EPOCH)),
                None
            )
            .is_none()
        );
    }
}

#[test]
fn server_recovery_honors_both_original_deadlines_after_elapsed_downtime() {
    let now = Instant::now();
    let epoch = EPOCH + 10 * MINUTE;
    let timing = Timing::default();
    for (provider_minutes, original_minutes) in [(40, 30), (20, 45)] {
        let recovery = Recovery {
            not_before_ms: EPOCH + original_minutes * MINUTE,
            wall: RecoveryWall::Server {
                what: "model at capacity".into(),
                cut_off: false,
                retry_at_ms: Some(EPOCH + provider_minutes * MINUTE),
            },
            ..server()
        };
        let mut restored = state();
        assert!(
            recovery::restore(
                &mut restored,
                &[complete(FAILED, Some(CodexErrorInfo::ServerOverloaded))],
                &recovery,
                Some(&crate::recovery::test_sample(epoch)),
                now,
                &timing
            )
            .restored()
        );
        let decision = decide(&restored, &policy(), &timing, now, Duration::ZERO);
        assert!(matches!(
            decision.act,
            Act::Type {
                rule: crate::words::RULE_API_RETRY,
                ..
            }
        ));
        assert_eq!(
            decision.not_before,
            Some(
                now + Duration::from_millis((provider_minutes.max(original_minutes) - 10) * MINUTE)
            ),
            "downtime spends the original wait instead of restarting it"
        );
        let longer = Timing {
            retry_backoff: vec![Duration::from_secs(60 * 60)],
            ..timing.clone()
        };
        let extended = decide(&restored, &policy(), &longer, now, Duration::ZERO);
        assert!(extended.not_before > decision.not_before);
    }
}

#[test]
fn usage_reset_grace_is_applied_to_the_provider_reset_and_not_to_the_carried_floor() {
    let now = Instant::now();
    let epoch = EPOCH + 5 * MINUTE;
    let timing = Timing::default();
    let recovery = usage();
    let mut restored = state();
    assert!(
        recovery::restore(
            &mut restored,
            &[complete(FAILED, Some(CodexErrorInfo::UsageLimitExceeded))],
            &recovery,
            Some(&crate::recovery::test_sample(epoch)),
            now,
            &timing
        )
        .restored()
    );
    let reset = now + Duration::from_millis(15 * MINUTE);
    assert!(matches!(
        restored.point.as_ref().and_then(|point| point.wall.as_ref()),
        Some(Wall::Usage { reset_at: Some(at), .. }) if *at == reset
    ));
    let first = decide(&restored, &policy(), &timing, now, Duration::ZERO);
    assert!(matches!(first.act, Act::ResumeLimit { .. }));
    assert_eq!(first.not_before, Some(reset + Duration::from_secs(70)));
    assert_eq!(
        decide(&restored, &policy(), &timing, now, Duration::ZERO),
        first,
        "re-evaluation does not add another reset grace"
    );
    let longer = Timing {
        reset_grace: Duration::from_secs(180),
        ..timing
    };
    assert_eq!(
        decide(&restored, &policy(), &longer, now, Duration::ZERO).not_before,
        Some(reset + Duration::from_secs(180)),
        "a live longer grace still extends the original floor"
    );
}

#[test]
fn restored_intents_do_not_override_live_retry_or_usage_switches() {
    let now = Instant::now();
    let timing = Timing::default();
    let mut server_state = state();
    assert!(
        recovery::restore(
            &mut server_state,
            &[complete(FAILED, Some(CodexErrorInfo::ServerOverloaded))],
            &server(),
            Some(&crate::recovery::test_sample(EPOCH)),
            now,
            &timing
        )
        .restored()
    );
    let disabled = Policy {
        retry_api_errors: false,
        ..policy()
    };
    let decision = decide(&server_state, &disabled, &timing, now, Duration::ZERO);
    assert!(matches!(decision.act, Act::Attention { key: "api", .. }));
    assert_eq!(decision.not_before, None);
    assert!(matches!(
        decide(&server_state, &policy(), &timing, now, Duration::ZERO).act,
        Act::Type {
            rule: crate::words::RULE_API_RETRY,
            ..
        }
    ));

    let mut usage_state = state();
    assert!(
        recovery::restore(
            &mut usage_state,
            &[complete(FAILED, Some(CodexErrorInfo::UsageLimitExceeded))],
            &usage(),
            Some(&crate::recovery::test_sample(EPOCH)),
            now,
            &timing
        )
        .restored()
    );
    for disabled in [
        Policy {
            resume_limits: false,
            ..policy()
        },
        Policy {
            limit_wait: false,
            ..policy()
        },
    ] {
        let decision = decide(&usage_state, &disabled, &timing, now, Duration::ZERO);
        assert!(matches!(decision.act, Act::Attention { key: "limit", .. }));
        assert_eq!(decision.not_before, None);
    }
    usage_state.park_off();
    assert!(usage_state.recovery_not_before.is_none());
    assert_eq!(
        Mode::of_status(codex_alab_aterm::status::Autopilot::Off),
        None,
        "Off supplies no active mode for the supervisor to dispatch"
    );
}

#[test]
fn a_past_usage_reset_keeps_a_valid_immediate_recovery_boundary() {
    let now = Instant::now();
    let timing = Timing::default();
    let original = at_wall(
        Wall::Usage {
            reset_at: Some(now - Duration::from_secs(10 * 60)),
            reset_label: Some("a reset already passed".into()),
        },
        now,
        &timing,
    );
    let decision = decide(&original, &policy(), &timing, now, Duration::ZERO);
    assert!(decision.not_before.is_some_and(|at| at < now));
    let captured = recovery::capture(
        "the-primary-thread",
        FAILED,
        &original,
        &decision,
        false,
        now,
        EPOCH,
        Some(&crate::recovery::test_sample(EPOCH)),
        None,
    )
    .expect("the immediate native usage recovery intent");
    assert_eq!(captured.point_at_ms, EPOCH);
    assert_eq!(captured.not_before_ms, EPOCH);
    assert!(matches!(
        captured.wall,
        RecoveryWall::Usage { reset_at_ms: Some(at), .. } if at < captured.point_at_ms
    ));
    let mut restored = state();
    assert!(
        recovery::restore(
            &mut restored,
            &[complete(FAILED, Some(CodexErrorInfo::UsageLimitExceeded))],
            &captured,
            Some(&crate::recovery::test_sample(EPOCH)),
            now,
            &timing
        )
        .restored()
    );
    let after = decide(&restored, &policy(), &timing, now, Duration::ZERO);
    assert!(matches!(after.act, Act::ResumeLimit { .. }));
    assert_eq!(after.not_before, Some(now));
}

/// A MEASURED RETURN'S ACT LEAVES A RECOVERY THAT MEASURES AGAIN (B2): the API measured
/// reachable again, the wall's act is `api-back@v1`, which a check may hold back (the record's
/// `owed` then carries the blind retry); its point leaves a network recovery all the same,
/// naming the route as measured, so the process after a crash restores the wall and measures
/// its route ([`recovery::rearms`]) rather than finding no recovery and no try. Before, only
/// the blind retry left one. NEGATIVE CONTROL: a route not measured leaves a recovery that
/// measures nothing.
#[test]
fn a_measured_returns_act_leaves_a_recovery_that_measures_again() {
    let now = Instant::now();
    let timing = Timing::default();
    let what = "an API error (the API could not be reached)";
    let mut original = at_wall(Wall::Network { what: what.into() }, now, &timing);
    original.reach = crate::netwatch::Reach::Up { since: now };
    let decision = decide(&original, &policy(), &timing, now, Duration::ZERO);
    assert!(matches!(
        decision.act,
        Act::Type {
            rule: crate::words::RULE_API_BACK,
            ..
        }
    ));
    for measured in [true, false] {
        let captured = recovery::capture(
            "the-primary-thread",
            FAILED,
            &original,
            &decision,
            measured,
            now,
            EPOCH,
            Some(&crate::recovery::test_sample(EPOCH)),
            None,
        )
        .unwrap_or_else(|| panic!("measured={measured}: the measured return's point recovers"));
        assert_eq!(
            captured.wall,
            RecoveryWall::Network {
                what: what.into(),
                measured,
            }
        );
        assert_eq!(recovery::rearms(&captured), measured);
    }
}

/// A RESTORED POINT LEAVES RULING F'S REPLIES A RE-EXEC SEEDED (parity review of the stage-1
/// port, test lens): the resume seeds the record's replies before a recovery or a terminal
/// wall restores its point, and both restore it as a restored end, which neither counts nor
/// lifts; the hold stands across either. NEGATIVE CONTROL: the same usage point seen live (no
/// act awaited, no one's prompt but codex-alab's) is another turn, and lifts the replies.
#[test]
fn a_restored_point_leaves_the_replies_a_re_exec_seeded() {
    use crate::replies::HeldBy;
    use crate::replies::Kept;
    use crate::replies::Replies;
    let now = Instant::now();
    let timing = Timing::default();
    let kept = Replies::of(&[Kept {
        mark: 0x5e,
        answers: 2,
        walled: false,
        tries: 0,
    }]);
    // A valid recovery.
    let mut recovered = state();
    recovered.seed_replies(kept, Some(now));
    assert!(
        recovery::restore(
            &mut recovered,
            &[complete(FAILED, Some(CodexErrorInfo::UsageLimitExceeded))],
            &usage(),
            Some(&crate::recovery::test_sample(EPOCH)),
            now,
            &timing
        )
        .restored()
    );
    assert_eq!(recovered.replies, kept);
    assert_eq!(recovered.held_by(), Some(HeldBy::SameReply));
    // A terminal wall the resumed history ends on.
    let items = [
        event(
            json!({"type":"task_started", "turn_id":FAILED, "turn_attribution": {"turn_id": FAILED, "turn_trigger": "user"},
            "model_context_window":258400, "collaboration_mode_kind":"default"}),
        ),
        complete(FAILED, Some(CodexErrorInfo::Other)),
    ];
    let mut resumed = ThreadState::default();
    resumed.seed_replies(kept, Some(now));
    assert!(crate::extension::resume_into(
        &mut resumed,
        &items,
        true,
        None,
        0,
        now
    ));
    assert!(resumed.point.as_ref().is_some_and(|p| p.restored));
    assert_eq!(resumed.replies, kept);
    assert_eq!(resumed.held_by(), Some(HeldBy::SameReply));
    // NEGATIVE CONTROL: the usage point seen live.
    let mut live = state();
    live.seed_replies(kept, Some(now));
    live.observe_turn_end(
        TurnEnd {
            at: now,
            by_autopilot: true,
            by_person: false,
            text: None,
            earlier: None,
            questions: false,
            worked: Duration::ZERO,
            wall: Some(Wall::Usage {
                reset_at: None,
                reset_label: Some("the provider's original reset".into()),
            }),
            aborted: false,
            ran_tools: false,
            restored: false,
        },
        &timing,
    );
    assert_eq!(live.replies, Replies::default());
}

/// A USAGE WALL WHOSE ACCOUNT HANDOFF IS BEING ASKED FOR IS CARRIED WITH ITS RESET (review of
/// the five-hour yield, 2026-11-12). The ask owns the point, so no decision is made at the
/// wall and [`recovery::capture`] is never reached; the process after a restart then held the
/// handoff's intent with no wall, and nothing to stop asking the host at.
/// [`recovery::capture_asked_wall`] leaves the wall's recovery with no decision. Restored two
/// hours later, the wall stands with the hour its reset is still away, and its decision is
/// the wall's own: nothing carried holds it back or brings it forward.
#[test]
fn a_usage_wall_whose_handoff_is_asked_for_is_carried_with_its_reset() {
    let now = Instant::now();
    let timing = Timing::default();
    let hour = Duration::from_secs(3600);
    let met = now - Duration::from_secs(120);
    let usage_wall = |reset_at| Wall::Usage {
        reset_at,
        reset_label: Some("07:30".into()),
    };
    let mut original = at_wall(usage_wall(Some(now + 3 * hour)), met, &timing);
    original.point_turn_id = Some(FAILED.into());
    let captured = recovery::capture_asked_wall(
        "the-primary-thread",
        FAILED,
        &original,
        now,
        EPOCH,
        Some(&crate::recovery::test_sample(EPOCH)),
        None,
    )
    .expect("the asked wall's recovery intent");
    assert_eq!(captured.thread, "the-primary-thread");
    assert_eq!(captured.turn_id, FAILED);
    assert_eq!(captured.point_at_ms, EPOCH - 120_000);
    assert_eq!(
        captured.not_before_ms, captured.point_at_ms,
        "no decision was made, so no floor of one is carried"
    );
    assert!(captured.by_autopilot && !captured.by_person);
    let RecoveryWall::Usage {
        reset_at_ms: Some(reset_at_ms),
        reset_label,
        quota: None,
    } = &captured.wall
    else {
        panic!("not a usage wall with its reset: {:?}", captured.wall);
    };
    let three_hours = EPOCH + 3 * 60 * MINUTE;
    assert!(
        (three_hours..=three_hours + 2).contains(reset_at_ms),
        "{reset_at_ms}"
    );
    assert_eq!(reset_label.as_deref(), Some("07:30"));

    // The next process, two hours later on the wall clock.
    let later = now + Duration::from_secs(7);
    let mut restored = state();
    assert!(
        recovery::restore(
            &mut restored,
            &[complete(FAILED, Some(CodexErrorInfo::UsageLimitExceeded))],
            &captured,
            Some(&crate::recovery::test_sample(EPOCH + 2 * 60 * MINUTE)),
            later,
            &timing
        )
        .restored()
    );
    assert_eq!(restored.point_turn_id.as_deref(), Some(FAILED));
    let Some(Wall::Usage {
        reset_at: Some(reset),
        ..
    }) = restored
        .point
        .as_ref()
        .and_then(|point| point.wall.as_ref())
    else {
        panic!("the restored point is not the usage wall with its reset");
    };
    assert_serialized_deadline(Some(later + hour), Some(*reset));
    let decision = decide(&restored, &policy(), &timing, later, Duration::ZERO);
    assert!(matches!(decision.act, Act::ResumeLimit { .. }));
    assert_eq!(
        decision.not_before,
        Some(*reset + timing.reset_grace),
        "the wall's own reset and grace, and nothing carried besides"
    );

    // A wall that names no reset is carried as one: it has no moment, and gains none.
    let mut unnamed = at_wall(usage_wall(None), met, &timing);
    unnamed.point_turn_id = Some(FAILED.into());
    assert!(matches!(
        recovery::capture_asked_wall(
            "the-primary-thread",
            FAILED,
            &unnamed,
            now,
            EPOCH,
            Some(&crate::recovery::test_sample(EPOCH)),
            None
        ),
        Some(Recovery {
            wall: RecoveryWall::Usage {
                reset_at_ms: None,
                ..
            },
            ..
        })
    ));

    // NEGATIVE CONTROLS: another turn's point, an aborted point, no point, another wall.
    assert!(
        recovery::capture_asked_wall(
            "the-primary-thread",
            "another-turn",
            &original,
            now,
            EPOCH,
            Some(&crate::recovery::test_sample(EPOCH)),
            None
        )
        .is_none()
    );
    let mut unidentified = original.clone();
    unidentified.point_turn_id = None;
    assert!(
        recovery::capture_asked_wall(
            "the-primary-thread",
            FAILED,
            &unidentified,
            now,
            EPOCH,
            Some(&crate::recovery::test_sample(EPOCH)),
            None
        )
        .is_none()
    );
    let mut aborted = original;
    aborted.point.as_mut().expect("the wall's point").aborted = true;
    assert!(
        recovery::capture_asked_wall(
            "the-primary-thread",
            FAILED,
            &aborted,
            now,
            EPOCH,
            Some(&crate::recovery::test_sample(EPOCH)),
            None
        )
        .is_none()
    );
    assert!(
        recovery::capture_asked_wall(
            "the-primary-thread",
            FAILED,
            &state(),
            now,
            EPOCH,
            Some(&crate::recovery::test_sample(EPOCH)),
            None
        )
        .is_none()
    );
    for other in [
        Wall::Login,
        Wall::Entitlement,
        Wall::Server {
            what: "model at capacity".into(),
            cut_off: false,
            retry_at: Some(now + hour),
        },
    ] {
        let mut state = at_wall(other.clone(), met, &timing);
        state.point_turn_id = Some(FAILED.into());
        assert!(
            recovery::capture_asked_wall(
                "the-primary-thread",
                FAILED,
                &state,
                now,
                EPOCH,
                Some(&crate::recovery::test_sample(EPOCH)),
                None
            )
            .is_none(),
            "{other:?}"
        );
    }
}
