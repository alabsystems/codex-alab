// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A native refusal excludes its action while the harness may advance independent work.

use super::*;
use codex_alab_aterm::WAIT_OWNER;
use codex_alab_autopilot::words;

pub(super) async fn assert_cyber_continuation(
    auth: Option<CodexAuth>,
    program: Option<CyberAccessProgram>,
    refused_again: bool,
) {
    let expected_account = auth.as_ref().and_then(CodexAuth::get_account_id);
    let refusal = || {
        responses::sse_response(responses::sse_failed(
            "refused",
            "cyber_policy",
            "This action was refused by the cybersecurity access policy.",
        ))
    };
    let h = harness_timed(
        Mode::Current,
        vec![
            refusal(),
            if refused_again {
                refusal()
            } else {
                responses::sse_response(reply(
                    "independent",
                    "Reviewed independently authorized tests.",
                ))
            },
        ],
        &[("human_grace_s", "0")],
        &[],
        None,
        auth,
        Timing {
            min_work: Duration::from_secs(60),
            ..Default::default()
        },
    )
    .await;
    assert!(
        h.fake
            .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
            .await
    );
    let started = h
        .test
        .codex
        .start_or_steer_turn(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "the initial request".into(),
                text_elements: Vec::new(),
            }])
            .on_start(TurnStartOptions {
                cyber_access_program: program,
                ..Default::default()
            }),
        )
        .await
        .unwrap_or_else(|err| panic!("first native turn: {err:?}"));
    let codex_protocol::turn_input::TurnInputSubmission::Started {
        turn_id: first_turn,
        ..
    } = started
    else {
        panic!("the fixture must start its first native turn");
    };
    wait_until("one independent-work continuation is sent", || {
        h.mock.requests().len() == 2
    })
    .await;
    wait_for_event_with_timeout(&h.test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(done) if done.turn_id != first_turn &&
            if refused_again { done.error.is_some() } else { done.last_agent_message.as_deref() == Some("Reviewed independently authorized tests.") })
    }, Duration::from_secs(60)).await;
    if refused_again {
        // An ask, never the back-off's wait (`WAIT_OWNER`), which the continuation's own short
        // yield puts up beside it and which would end this wait before the ask is up.
        assert!(
            h.fake
                .wait_for(|f| f.attention().keys().any(|owner| owner != WAIT_OWNER))
                .await
        );
        let mut attention = h.fake.attention();
        attention.remove(WAIT_OWNER);
        assert!(
            attention.values().all(|text| {
                text.contains("cybersecurity access policy")
                    && text.contains("cyberPolicy")
                    && text.contains("independent-work continuation")
            }),
            "the stopped continuation must retain its actual policy cause: {attention:?}"
        );
        assert!(
            !h.autopilot
                .thread_rt(&h.thread)
                .unwrap_or_else(|| panic!("thread"))
                .timer_pending()
        );
    } else {
        wait_until("the next continuation remains paced", || {
            h.autopilot
                .thread_rt(&h.thread)
                .unwrap_or_else(|| panic!("thread"))
                .timer_pending()
        })
        .await;
    }
    assert_eq!(h.fake.acts(), [words::RULE_POLICY_CONTINUE]);
    h.test
        .codex
        .flush_rollout()
        .await
        .unwrap_or_else(|err| panic!("native rollout flushed: {err:?}"));
    let (items, _, _) = codex_core::RolloutRecorder::load_rollout_items(
        &h.test
            .codex
            .rollout_path()
            .unwrap_or_else(|| panic!("native rollout")),
    )
    .await
    .unwrap_or_else(|err| panic!("native rollout reads: {err:?}"));
    let safe_messages = items.iter().filter(|item| matches!(item,
        codex_history::RolloutItem::EventMsg(EventMsg::UserMessage(message)) if message.message == words::POLICY_CONTINUE
    )).count();
    assert_eq!(
        safe_messages, 1,
        "core persists the exact bounded instruction for restart custody"
    );
    h.test
        .codex
        .shutdown_and_wait()
        .await
        .unwrap_or_else(|err| panic!("native shutdown: {err:?}"));
    let requests = h.mock.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1]
            .message_input_texts("user")
            .last()
            .map(String::as_str),
        Some(words::POLICY_CONTINUE)
    );
    for request in &requests {
        assert_eq!(
            request.body_json()["model"],
            requests[0].body_json()["model"]
        );
        assert_eq!(
            request.body_json().get("access_programs").cloned(),
            program.map(|cyber| serde_json::json!({"cyber":cyber}))
        );
        if let Some(account) = expected_account.as_deref() {
            assert_eq!(
                request.header("chatgpt-account-id").as_deref(),
                Some(account)
            );
        }
    }
    for header in ["authorization", "chatgpt-account-id"] {
        assert_eq!(requests[0].header(header), requests[1].header(header));
    }
}

#[test]
fn a_second_policy_refusal_has_no_retry_or_followup_timer() {
    big_stack(|| async { assert_cyber_continuation(None, None, true).await });
}

#[test]
fn a_new_root_turn_clears_only_its_preceding_wall_alert_at_start() {
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![reply("initial", "Initial work was reviewed.")],
            &[("human_grace_s", "0"), ("continue", "false")],
        )
        .await;
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        h.test
            .submit_turn("initial independent work")
            .await
            .unwrap_or_else(|err| panic!("initial turn: {err:?}"));
        wait_until("the initial turn and claim are settled", || {
            journal_has(&h, "nothing (continue is off)")
        })
        .await;
        let sup = h.autopilot.supervisor();
        sup.attention_for(&h.thread, "wall", Some("old refusal".into()))
            .await;
        let session = ExtensionData::new("session");
        let turn = ExtensionData::new("corrected-root-turn");
        let mode = h
            .test
            .codex
            .thread_settings_snapshot()
            .await
            .collaboration_mode;
        let side = ThreadId::new();
        let side_store = ExtensionData::new(side.to_string());
        side_store.insert(codex_alab_autopilot::extension::ThreadRt::new(
            side,
            None,
            false,
            Duration::ZERO,
        ));
        let side_turn = ExtensionData::new("side-turn");
        h.autopilot
            .on_turn_start(codex_extension_api::TurnStartInput {
                turn_id: "side-turn",
                collaboration_mode: &mode,
                token_usage_at_turn_start: None,
                session_store: &session,
                thread_store: &side_store,
                turn_store: &side_turn,
            })
            .await;
        assert_eq!(
            sup.standing_attention().as_deref(),
            Some("old refusal"),
            "a side thread cannot retire a root wall"
        );
        sup.attention("login", Some("separate login".into())).await;
        h.autopilot
            .on_turn_start(codex_extension_api::TurnStartInput {
                turn_id: "corrected-root-turn",
                collaboration_mode: &mode,
                token_usage_at_turn_start: None,
                session_store: &session,
                thread_store: h.test.codex.thread_extension_data(),
                turn_store: &turn,
            })
            .await;
        assert_eq!(sup.standing_attention().as_deref(), Some("separate login"));
        sup.attention("login", None).await;
        assert!(
            sup.standing_attention().is_none(),
            "only wall was cleared at acceptance"
        );
        h.test
            .codex
            .shutdown_and_wait()
            .await
            .unwrap_or_else(|err| panic!("shutdown: {err:?}"));
    });
}

#[test]
fn a_goal_triggered_native_turn_preserves_the_policy_exclusion() {
    big_stack(|| async {
        let refusal = || {
            responses::sse_response(responses::sse_failed(
                "refused",
                "cyber_policy",
                "The policy refused this action.",
            ))
        };
        let h = harness_timed(
            Mode::Current,
            vec![
                refusal(),
                responses::sse_response(reply("safe", "Reviewed independent tests.")),
                refusal(),
            ],
            &[("human_grace_s", "0")],
            &[],
            None,
            None,
            Timing {
                min_work: Duration::from_secs(60),
                ..Default::default()
            },
        )
        .await;
        assert!(h.fake.wait_for(|f| f.claim().is_some()).await);
        h.test
            .submit_turn("original task")
            .await
            .unwrap_or_else(|err| panic!("original turn: {err:?}"));
        wait_for_event_with_timeout(&h.test.codex, |event| matches!(event, EventMsg::TurnComplete(done) if done.last_agent_message.as_deref() == Some("Reviewed independent tests.")), Duration::from_secs(60)).await;
        h.test
            .codex
            .start_or_steer_turn(
                TurnInputRequest::user_input(vec![UserInput::Text {
                    text: "native goal continuation".into(),
                    text_elements: Vec::new(),
                }])
                .on_start(TurnStartOptions {
                    turn_trigger: Some("goal".into()),
                    ..Default::default()
                }),
            )
            .await
            .unwrap_or_else(|err| panic!("goal's native turn: {err:?}"));
        // The goal turn's ask, never the back-off's wait (`WAIT_OWNER`) the safe reply's short
        // turn put up before the goal turn's request was sent.
        assert!(
            h.fake
                .wait_for(|f| f.attention().keys().any(|owner| owner != WAIT_OWNER))
                .await
        );
        assert_eq!(
            h.mock.requests().len(),
            3,
            "the goal turn cannot rearm a second policy continuation"
        );
        assert_eq!(h.fake.acts(), [words::RULE_POLICY_CONTINUE]);
        assert!(
            !h.autopilot
                .thread_rt(&h.thread)
                .unwrap_or_else(|| panic!("thread"))
                .timer_pending()
        );
        h.test
            .codex
            .shutdown_and_wait()
            .await
            .unwrap_or_else(|err| panic!("shutdown: {err:?}"));
    });
}
