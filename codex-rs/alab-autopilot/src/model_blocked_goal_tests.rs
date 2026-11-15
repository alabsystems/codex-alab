// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! An agent's own blocked report gets another paced attempt at the original goal.
//! These fixtures execute the native goal tool, rather than treating an arbitrary
//! blocked database row as permission to resume it.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::upgrade_handoff_tests::wait_for_supervisor;
use super::*;
use codex_protocol::protocol::EventMsg;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::user_input::UserInput;
use codex_state::ThreadGoalStatus;
use core_test_support::responses;

#[path = "model_blocked_admission_tests.rs"]
mod admission;
#[path = "model_blocked_custody_tests.rs"]
mod custody;
#[path = "model_blocked_epoch_tests.rs"]
mod epoch;
#[path = "model_blocked_hook_tests.rs"]
mod hooks;
#[path = "model_blocked_late_import_tests.rs"]
mod late_import;
#[path = "model_blocked_partial_finish_tests.rs"]
mod partial_finish;
#[path = "model_blocked_restart_tests.rs"]
mod restart;
#[path = "model_blocked_source_tests.rs"]
mod source;
#[path = "model_blocked_teardown_tests.rs"]
mod teardown;

const HANG: Duration = super::upgrade_handoff_tests::NATIVE_HANG;
const OBJECTIVE: &str = "Finish and verify the original compiler integration and land it on main";

async fn harness() -> (Harness, Arc<GoalService>) {
    let goals = Arc::new(GoalService::new());
    let h = Harness::new_with_accounted_goals(Arc::clone(&goals)).await;
    configured_harness(h, goals).await
}

async fn configured_harness(h: Harness, goals: Arc<GoalService>) -> (Harness, Arc<GoalService>) {
    h.fake.set_policy_key("upgrade", "false");
    h.fake.set_policy_key("autopilot", "cruise");
    wait_for_supervisor(|| {
        h.autopilot
            .sup
            .policy()
            .is_some_and(|policy| !policy.upgrade && policy.autopilot == decide::Mode::Cruise)
    })
    .await;
    // Scripted SSE responses use Wiremock's default priority (5). Keep the
    // delayed final continuation below them, including later recovery scripts.
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_delay(Duration::from_secs(60)))
        .with_priority(10)
        .mount(&h._server)
        .await;
    (h, goals)
}

async fn goal(h: &Harness) -> codex_state::ThreadGoal {
    h.test
        .codex
        .state_db()
        .expect("native goal database")
        .thread_goals()
        .get_thread_goal(h.rt.id)
        .await
        .expect("read persisted goal")
        .expect("original goal remains")
}

async fn seed(h: &Harness, goals: &GoalService) -> codex_state::ThreadGoal {
    goals
        .set_thread_goal(
            &h.test.codex.state_db().expect("native goal database"),
            GoalSetRequest {
                thread_id: h.rt.id,
                objective: GoalObjectiveUpdate::Set(OBJECTIVE),
                status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                token_budget: GoalTokenBudgetUpdate::Set(Some(1_000)),
                max_goal_token_budget: None,
            },
            async { Ok(()) },
        )
        .await
        .expect("persist original active goal");
    h.test
        .codex
        .thread_extension_data()
        .get::<codex_goal_extension::GoalRuntimeHandle>()
        .expect("native goal runtime")
        .restore_after_resume()
        .await
        .expect("restore native accounting for original goal");
    goal(h).await
}

/// A later continuation falls through to the fixture's delayed provider response.
async fn mount_model_stop(h: &Harness, status: &str) {
    mount_goal_calls(
        h,
        vec![(
            codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
            serde_json::json!({ "status": status }),
        )],
    )
    .await;
}

async fn mount_goal_calls(h: &Harness, calls: Vec<(&str, serde_json::Value)>) {
    let sequence = responses::received_responses_requests(&h._server)
        .await
        .len();
    let mut bodies = Vec::new();
    for (index, (name, arguments)) in calls.into_iter().enumerate() {
        let response = format!("model-goal-stop-{sequence}-{index}");
        let call = format!("model-goal-stop-call-{sequence}-{index}");
        bodies.push(responses::sse(vec![
            responses::ev_response_created(&response),
            responses::ev_function_call(&call, name, &arguments.to_string()),
            responses::ev_completed_with_tokens(&response, 10),
        ]));
    }
    let final_response = format!("model-goal-stop-final-{sequence}");
    let final_message = format!("model-goal-stop-message-{sequence}");
    bodies.push(responses::sse(vec![
        responses::ev_response_created(&final_response),
        responses::ev_assistant_message(
            &final_message,
            "The temporary build-slot file is missing, so validation is blocked.",
        ),
        responses::ev_completed(&final_response),
    ]));
    responses::mount_sse_sequence(&h._server, bodies).await;
}

async fn completed(h: &Harness) {
    core_test_support::wait_for_event_with_timeout(
        &h.test.codex,
        |event| matches!(event, EventMsg::TurnComplete(_)),
        HANG,
    )
    .await;
    while h.test.codex.has_active_turn().await {
        tokio::task::yield_now().await;
    }
    wait_for_supervisor(|| !h.shared.turn_running_on(h.rt.key())).await;
}

/// Execute a genuine model-issued goal stop and its final response.
async fn model_stop(h: &Harness, status: &str) {
    mount_model_stop(h, status).await;
    start_and_complete(h).await;
    assert_eq!(
        responses::received_responses_requests(&h._server)
            .await
            .len(),
        2,
        "the real model tool and final response both ran"
    );
}

async fn start_and_complete(h: &Harness) {
    h.test
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Continue the original compiler goal; report the current obstacle.".into(),
            text_elements: Vec::new(),
        }]))
        .await
        .expect("native user turn accepted");
    completed(h).await;
}

/// Advance only the completed point's age, preserving production short-turn
/// pacing and the exact successful-tool recovery binding.
async fn due(h: &Harness) {
    h.rt.with_state(|state| {
        state.point.as_mut().expect("native completed point").at =
            clock() - Duration::from_secs(3601);
    });
    h.immediate_point().await;
}

async fn evaluate(h: &Harness) {
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
}

async fn wait_requests(h: &Harness, count: usize) {
    while responses::received_responses_requests(&h._server)
        .await
        .len()
        < count
    {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn finish(h: &Harness) {
    Autopilot::cancel_timer(&h.rt).await;
    h.test
        .codex
        .shutdown_and_wait()
        .await
        .expect("owned native shutdown");
}

#[test]
fn a_real_model_block_gets_a_paced_native_attempt_at_the_original_goal() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let evaluation = h.rt.hold_eval_when_free().await;
            let original = seed(&h, &goals).await;
            model_stop(&h, "blocked").await;
            let blocked = goal(&h).await;
            assert_eq!(blocked.goal_id, original.goal_id);
            assert_eq!(blocked.status, ThreadGoalStatus::Blocked);
            let binding = lock(&h.rt.model_blocked_goal)
                .clone()
                .expect("successful native blocked tool binds recovery");
            assert_eq!(binding.goal.goal_id, original.goal_id);
            assert_eq!(binding.goal.objective, original.objective);
            assert_eq!(binding.goal.token_budget, original.token_budget);
            assert_eq!(
                h.shared
                    .pace(h.rt.key())
                    .expect("recovery pace persisted")
                    .model_blocked_goal,
                Some(binding.goal)
            );
            let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
            drop(evaluation);
            h.immediate_point().await;
            evaluate(&h).await;
            assert_eq!(goal(&h).await.status, ThreadGoalStatus::Blocked);
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
            assert!(
                h.rt.owed_timer().is_some(),
                "the ordinary pacing wait remains owed"
            );

            due(&h).await;
            evaluate(&h).await;
            let resumed = goal(&h).await;
            assert_eq!(resumed.goal_id, original.goal_id);
            assert_eq!(resumed.objective, original.objective);
            assert_eq!(resumed.token_budget, original.token_budget);
            assert_eq!(resumed.tokens_used, blocked.tokens_used);
            assert_eq!(resumed.status, ThreadGoalStatus::Active);
            let started = core_test_support::wait_for_event_with_timeout(
                &h.test.codex,
                |event| matches!(event, EventMsg::TurnStarted(_)),
                HANG,
            )
            .await;
            assert_eq!(
                serde_json::to_value(started).expect("native start attribution")["turn_attribution"]
                    ["turn_trigger"],
                "goal",
                "recovery must use the original goal runtime"
            );
            wait_requests(&h, 3).await;
            let requests = responses::received_responses_requests(&h._server).await;
            assert_eq!(requests.len(), 3);
            assert!(requests[2].body_contains_text(OBJECTIVE));
            assert!(requests[2].body_contains_text(words::MODEL_BLOCKED_GOAL_CONTINUE));
            evaluate(&h).await;
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
            assert_eq!(
                responses::received_responses_requests(&h._server)
                    .await
                    .len(),
                3,
                "a second evaluation cannot duplicate the native goal continuation"
            );
            finish(&h).await;
        })
        .await
        .expect("real model-block recovery hang detector");
    });
}

#[test]
fn another_model_block_during_recovery_keeps_the_original_goal_going() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let evaluation = h.rt.hold_eval_when_free().await;
            let original = seed(&h, &goals).await;
            model_stop(&h, "blocked").await;
            let first_turn = lock(&h.rt.model_blocked_goal)
                .as_ref()
                .expect("first native stop bound")
                .goal
                .turn
                .clone();
            let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
            // The resumed native goal turn itself issues the second blocked
            // call. No additional human request authorizes this recovery.
            mount_model_stop(&h, "blocked").await;
            drop(evaluation);
            due(&h).await;
            evaluate(&h).await;
            let evaluation = h.rt.hold_eval_when_free().await;
            completed(&h).await;
            let blocked = goal(&h).await;
            assert_eq!(blocked.goal_id, original.goal_id);
            assert_eq!(blocked.objective, original.objective);
            assert_eq!(blocked.status, ThreadGoalStatus::Blocked);
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
            let next_turn = lock(&h.rt.model_blocked_goal)
                .as_ref()
                .expect("the native recovery turn's new stop is bound")
                .goal
                .turn
                .clone();
            assert_ne!(next_turn, first_turn);
            assert_eq!(
                responses::received_responses_requests(&h._server)
                    .await
                    .len(),
                4
            );
            drop(evaluation);
            due(&h).await;
            evaluate(&h).await;
            wait_requests(&h, 5).await;
            let resumed = goal(&h).await;
            assert_eq!(resumed.goal_id, original.goal_id);
            assert_eq!(resumed.objective, original.objective);
            assert_eq!(resumed.status, ThreadGoalStatus::Active);
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 2);
            let requests = responses::received_responses_requests(&h._server).await;
            assert_eq!(requests.len(), 5);
            assert!(requests[4].body_contains_text(OBJECTIVE));
            assert!(requests[4].body_contains_text(words::MODEL_BLOCKED_GOAL_CONTINUE));
            finish(&h).await;
        })
        .await
        .expect("repeated native model-block recovery hang detector");
    });
}

#[test]
fn an_unrelated_user_reply_preserves_the_original_model_block_for_recovery() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let evaluation = h.rt.hold_eval_when_free().await;
            let original = seed(&h, &goals).await;
            model_stop(&h, "blocked").await;
            let binding = lock(&h.rt.model_blocked_goal)
                .clone()
                .expect("the original successful block has recovery custody");
            responses::mount_sse_once(
                &h._server,
                responses::sse(vec![
                    responses::ev_response_created("unrelated-followup"),
                    responses::ev_assistant_message(
                        "unrelated-followup-message",
                        "Validation stopped when the temporary build-slot file disappeared.",
                    ),
                    responses::ev_completed("unrelated-followup"),
                ]),
            )
            .await;
            h.test
                .codex
                .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                    text: "Which validation step stopped?".into(),
                    text_elements: Vec::new(),
                }]))
                .await
                .expect("an ordinary follow-up is accepted without changing the goal");
            completed(&h).await;
            let stopped = goal(&h).await;
            assert_eq!(stopped.goal_id, original.goal_id);
            assert_eq!(stopped.status, ThreadGoalStatus::Blocked);
            assert_eq!(
                lock(&h.rt.model_blocked_goal).clone(),
                Some(binding.clone()),
                "a reply that calls no goal tool cannot revoke the earlier native block"
            );
            assert_eq!(
                h.shared
                    .pace(h.rt.key())
                    .expect("the reply retains the original recovery pace")
                    .model_blocked_goal,
                Some(binding.goal)
            );
            let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
            drop(evaluation);
            due(&h).await;
            evaluate(&h).await;
            wait_requests(&h, 4).await;
            let resumed = goal(&h).await;
            assert_eq!(resumed.goal_id, original.goal_id);
            assert_eq!(resumed.objective, original.objective);
            assert_eq!(resumed.status, ThreadGoalStatus::Active);
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
            let requests = responses::received_responses_requests(&h._server).await;
            assert_eq!(requests.len(), 4);
            assert!(requests[3].body_contains_text(words::MODEL_BLOCKED_GOAL_CONTINUE));
            finish(&h).await;
        })
        .await
        .expect("follow-up reply model-block custody hang detector");
    });
}

#[test]
fn interrupting_a_committed_model_block_preserves_recovery_and_a_persons_pause_still_wins() {
    native_test(|| async {
        for (relaunch, paused) in [(false, false), (true, false), (false, true)] {
            tokio::time::timeout(HANG, async {
                let (h, goals) = harness().await;
                let evaluation = h.rt.hold_eval_when_free().await;
                let original = seed(&h, &goals).await;
                responses::mount_sse_once(
                    &h._server,
                    responses::sse(vec![
                        responses::ev_response_created("block-before-interrupt"),
                        responses::ev_function_call(
                            "block-before-interrupt-call",
                            codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
                            &serde_json::json!({ "status": "blocked" }).to_string(),
                        ),
                        responses::ev_completed_with_tokens("block-before-interrupt", 10),
                    ]),
                )
                .await;
                h.test
                    .codex
                    .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                        text: "Continue the original compiler goal.".into(),
                        text_elements: Vec::new(),
                    }]))
                    .await
                    .expect("start the native turn that blocks its own goal");
                // Request two is the delayed answer after the real tool has
                // completed, so its binding was already published and flushed.
                wait_requests(&h, 2).await;
                let binding = lock(&h.rt.model_blocked_goal)
                    .clone()
                    .expect("the successful tool already bound the original goal");
                assert_eq!(goal(&h).await.status, ThreadGoalStatus::Blocked);
                assert!(h.test.codex.has_active_turn().await);
                if paused {
                    h.shared.person_goal_set(h.rt.key(), true);
                    let outcome = goals
                        .set_thread_goal(
                            &h.test.codex.state_db().expect("native goal database"),
                            GoalSetRequest {
                                thread_id: h.rt.id,
                                objective: GoalObjectiveUpdate::Keep,
                                status: Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
                                token_budget: GoalTokenBudgetUpdate::Keep,
                                max_goal_token_budget: None,
                            },
                            async { Ok(()) },
                        )
                        .await
                        .expect("the person's explicit pause is persisted");
                    outcome.apply_runtime_effects(&goals).await;
                }
                if relaunch {
                    assert!(h.shared.set_signal(15));
                    assert!(h.shared.leaving_for_relaunch());
                }
                h.test
                    .codex
                    .submit(Op::Interrupt)
                    .await
                    .expect("interrupt the native turn after its goal tool committed");
                core_test_support::wait_for_event_with_timeout(
                    &h.test.codex,
                    |event| matches!(event, EventMsg::TurnAborted(_)),
                    HANG,
                )
                .await;
                while h.test.codex.has_active_turn().await {
                    tokio::task::yield_now().await;
                }
                wait_for_supervisor(|| !h.shared.turn_running_on(h.rt.key())).await;
                let current = goal(&h).await;
                assert_eq!(current.goal_id, original.goal_id);
                assert_eq!(current.objective, original.objective);
                if paused {
                    assert_eq!(current.status, ThreadGoalStatus::Paused);
                    assert!(
                        h.shared
                            .pace(h.rt.key())
                            .is_none_or(|pace| pace.model_blocked_goal.is_none()),
                        "the explicit stop must retire durable model-block authority"
                    );
                    let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
                    drop(evaluation);
                    due(&h).await;
                    evaluate(&h).await;
                    assert_eq!(goal(&h).await.status, ThreadGoalStatus::Paused);
                    assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
                    assert_eq!(
                        responses::received_responses_requests(&h._server)
                            .await
                            .len(),
                        2
                    );
                } else {
                    assert_eq!(current.status, ThreadGoalStatus::Blocked);
                    assert_eq!(
                        lock(&h.rt.model_blocked_goal).clone(),
                        Some(binding.clone()),
                        "an aborted answer cannot erase the goal tool's committed stop"
                    );
                    assert_eq!(
                        h.shared
                            .pace(h.rt.key())
                            .expect("the committed stop must survive process handoff")
                            .model_blocked_goal,
                        Some(binding.goal)
                    );
                    drop(evaluation);
                }
                finish(&h).await;
            })
            .await
            .unwrap_or_else(|_| {
                panic!("interrupted model-block custody hung: relaunch={relaunch}, paused={paused}")
            });
        }
    });
}

#[test]
fn multiple_goal_tools_in_one_native_turn_preserve_model_block_recovery() {
    native_test(|| async {
        for create_first in [false, true] {
            tokio::time::timeout(HANG, async {
                let (h, goals) = harness().await;
                let evaluation = h.rt.hold_eval_when_free().await;
                let original = if create_first {
                    None
                } else {
                    Some(seed(&h, &goals).await)
                };
                let blocked_call = (
                    codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
                    serde_json::json!({ "status": "blocked" }),
                );
                mount_goal_calls(
                    &h,
                    vec![
                        if create_first {
                            (
                                codex_goal_extension::CREATE_GOAL_TOOL_NAME,
                                serde_json::json!({
                                    "objective": OBJECTIVE,
                                    "token_budget": 1_000,
                                }),
                            )
                        } else {
                            blocked_call.clone()
                        },
                        blocked_call,
                    ],
                )
                .await;
                start_and_complete(&h).await;
                assert_eq!(
                    responses::received_responses_requests(&h._server)
                        .await
                        .len(),
                    3,
                    "both native goal tools executed in the same turn"
                );
                let blocked = goal(&h).await;
                assert_eq!(blocked.status, ThreadGoalStatus::Blocked);
                assert_eq!(blocked.objective, OBJECTIVE);
                if let Some(original) = original {
                    assert_eq!(blocked.goal_id, original.goal_id);
                }
                let binding = lock(&h.rt.model_blocked_goal)
                    .clone()
                    .expect("the last successful goal tool retains model-block provenance");
                assert_eq!(binding.goal.goal_id, blocked.goal_id);
                assert_eq!(binding.goal.objective, blocked.objective);
                assert_eq!(binding.goal.token_budget, blocked.token_budget);
                assert_eq!(
                    h.shared
                        .pace(h.rt.key())
                        .expect("model-block recovery pace persisted")
                        .model_blocked_goal,
                    Some(binding.goal)
                );
                let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
                drop(evaluation);
                due(&h).await;
                evaluate(&h).await;
                wait_requests(&h, 4).await;
                let resumed = goal(&h).await;
                assert_eq!(resumed.goal_id, blocked.goal_id);
                assert_eq!(resumed.objective, blocked.objective);
                assert_eq!(resumed.token_budget, blocked.token_budget);
                assert_eq!(resumed.status, ThreadGoalStatus::Active);
                assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
                let requests = responses::received_responses_requests(&h._server).await;
                assert_eq!(requests.len(), 4);
                assert!(requests[3].body_contains_text(OBJECTIVE));
                assert!(requests[3].body_contains_text(words::MODEL_BLOCKED_GOAL_CONTINUE));
                finish(&h).await;
            })
            .await
            .unwrap_or_else(|_| {
                panic!("same-turn goal-tool recovery hung: create_first={create_first}")
            });
        }
    });
}

#[test]
fn an_unreadable_model_blocked_goal_keeps_its_binding_and_rechecks() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let evaluation = h.rt.hold_eval_when_free().await;
            let original = seed(&h, &goals).await;
            model_stop(&h, "blocked").await;
            let binding = lock(&h.rt.model_blocked_goal)
                .clone()
                .expect("native model stop binds recovery");
            let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
            let db = h.test.codex.state_db().expect("native goal database");
            super::description_goal_read_tests::saved_status(
                &db,
                h.rt.id,
                "unreadable-model-blocked-status",
            )
            .await;
            assert!(db.thread_goals().get_thread_goal(h.rt.id).await.is_err());
            drop(evaluation);
            due(&h).await;
            evaluate(&h).await;
            let retained = lock(&h.rt.model_blocked_goal).clone();
            let rechecking = h.rt.timer_pending();
            let accepted_after_fault = h.rt.accepted_turns.load(Ordering::SeqCst);
            let requests_after_fault = responses::received_responses_requests(&h._server)
                .await
                .len();
            // Restore valid decoding before verdicts, subsequent control work,
            // and teardown; the preceding evaluation saw a genuine SQL read fault.
            super::description_goal_read_tests::saved_status(&db, h.rt.id, "blocked").await;
            assert_eq!(retained, Some(binding));
            assert!(
                rechecking,
                "unknown standing must recheck instead of parking"
            );
            assert_eq!(accepted_after_fault, accepted);
            assert_eq!(requests_after_fault, 2);
            due(&h).await;
            evaluate(&h).await;
            wait_requests(&h, 3).await;
            let resumed = goal(&h).await;
            assert_eq!(resumed.goal_id, original.goal_id);
            assert_eq!(resumed.objective, original.objective);
            assert_eq!(resumed.status, ThreadGoalStatus::Active);
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
            let requests = responses::received_responses_requests(&h._server).await;
            assert_eq!(requests.len(), 3);
            assert!(requests[2].body_contains_text(words::MODEL_BLOCKED_GOAL_CONTINUE));
            finish(&h).await;
        })
        .await
        .expect("model-blocked goal read recovery hang detector");
    });
}

#[test]
fn an_upgrade_notice_stays_pending_while_the_original_model_blocked_goal_recovers() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let evaluation = h.rt.hold_eval_when_free().await;
            let original = seed(&h, &goals).await;
            model_stop(&h, "blocked").await;
            h.fake.set_policy_key("upgrade", "true");
            h.fake.set_policy_key("upgrade_ask", "true");
            wait_for_supervisor(|| {
                h.autopilot
                    .sup
                    .policy()
                    .is_some_and(|policy| policy.upgrade && policy.upgrade_ask)
            })
            .await;
            let policy = h.autopilot.sup.policy().expect("fresh upgrade policy");
            {
                let (round, step, _) = h
                    .autopilot
                    .ask_round(&policy)
                    .expect("the fixture has a due upgrade target");
                assert_eq!(step, AskStep::Notice);
                assert_eq!(round.asks(), 0);
            }
            let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
            drop(evaluation);
            due(&h).await;
            evaluate(&h).await;
            wait_requests(&h, 3).await;
            let resumed = goal(&h).await;
            assert_eq!(resumed.goal_id, original.goal_id);
            assert_eq!(resumed.objective, original.objective);
            assert_eq!(resumed.status, ThreadGoalStatus::Active);
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
            {
                let (round, step, _) = h
                    .autopilot
                    .ask_round(&policy)
                    .expect("upgrade round remains available");
                assert_eq!(round.asks(), 0, "goal recovery delivered no upgrade notice");
                assert_eq!(step, AskStep::Notice, "the notice remains pending");
            }
            assert!(
                h.fake
                    .acts()
                    .iter()
                    .all(|rule| rule != words::RULE_UPGRADE_NOTICE)
            );
            let requests = responses::received_responses_requests(&h._server).await;
            assert_eq!(requests.len(), 3);
            assert!(requests[2].body_contains_text(words::MODEL_BLOCKED_GOAL_CONTINUE));
            assert!(!h.shared.exec_pending());
            finish(&h).await;
        })
        .await
        .expect("upgrade notice beside model-block recovery hang detector");
    });
}

#[test]
fn a_model_pause_and_a_blocked_row_without_model_provenance_remain_stopped() {
    native_test(|| async {
        for native_pause in [false, true] {
            tokio::time::timeout(HANG, async {
                let (h, goals) = harness().await;
                let evaluation = h.rt.hold_eval_when_free().await;
                let original = seed(&h, &goals).await;
                let expected_requests = if native_pause {
                    model_stop(&h, "paused").await;
                    2
                } else {
                    h.test
                        .codex
                        .state_db()
                        .expect("native goal database")
                        .thread_goals()
                        .update_thread_goal(
                            h.rt.id,
                            codex_state::GoalUpdate {
                                objective: None,
                                status: Some(ThreadGoalStatus::Blocked),
                                token_budget: None,
                                expected_goal_id: Some(original.goal_id),
                            },
                        )
                        .await
                        .expect("plain blocked row");
                    0
                };
                let stopped = goal(&h).await;
                assert!(lock(&h.rt.model_blocked_goal).is_none());
                let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
                drop(evaluation);
                due(&h).await;
                evaluate(&h).await;
                assert_eq!(goal(&h).await, stopped);
                assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
                assert_eq!(
                    responses::received_responses_requests(&h._server)
                        .await
                        .len(),
                    expected_requests
                );
                finish(&h).await;
            })
            .await
            .expect("explicit stop without model-block provenance hang detector");
        }
    });
}

#[test]
fn a_new_stop_replacement_or_budget_exhaustion_defeats_prepared_model_block_recovery() {
    native_test(|| async {
        for change in [
            "person-block",
            "paused",
            "replacement",
            "objective",
            "budget",
        ] {
            tokio::time::timeout(HANG, async {
                let (h, goals) = harness().await;
                let evaluation = h.rt.hold_eval_when_free().await;
                seed(&h, &goals).await;
                model_stop(&h, "blocked").await;
                assert!(lock(&h.rt.model_blocked_goal).is_some());
                drop(evaluation);
                due(&h).await;
                let (entered, entry) = tokio::sync::oneshot::channel();
                let (release, blocked) = tokio::sync::oneshot::channel();
                *lock(&h.rt.before_submit) = Some((entered, blocked));
                let pilot = Arc::clone(&h.autopilot);
                let rt = Arc::clone(&h.rt);
                let recovering = tokio::spawn(async move { pilot.evaluate(rt).await });
                entry
                    .await
                    .expect("real resume passed its preparatory read");
                let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
                if change == "person-block" {
                    // This is the provenance hook used by the native app-server's
                    // user-origin set. Its same-status stop must revoke old authority.
                    h.shared.person_goal_set(h.rt.key(), true);
                }
                let db = h.test.codex.state_db().expect("native goal database");
                if change == "replacement" {
                    db.thread_goals()
                        .replace_thread_goal(
                            h.rt.id,
                            OBJECTIVE,
                            ThreadGoalStatus::Blocked,
                            Some(1_000),
                        )
                        .await
                        .expect("replace with a new goal having the same objective and budget");
                } else if change == "budget" {
                    let current = goal(&h).await;
                    db.thread_goals()
                        .account_thread_goal_usage(
                            h.rt.id,
                            0,
                            current.token_budget.expect("original token budget"),
                            codex_state::GoalAccountingMode::ActiveOrStopped,
                            Some(&current.goal_id),
                        )
                        .await
                        .expect("consume the original goal budget without changing it");
                } else {
                    goals
                        .set_thread_goal(
                            &db,
                            GoalSetRequest {
                                thread_id: h.rt.id,
                                objective: if change == "objective" {
                                    GoalObjectiveUpdate::Set("the person's replacement objective")
                                } else {
                                    GoalObjectiveUpdate::Keep
                                },
                                status: Some(if change == "paused" {
                                    codex_protocol::protocol::ThreadGoalStatus::Paused
                                } else {
                                    codex_protocol::protocol::ThreadGoalStatus::Blocked
                                }),
                                token_budget: GoalTokenBudgetUpdate::Keep,
                                max_goal_token_budget: None,
                            },
                            async { Ok(()) },
                        )
                        .await
                        .expect("new explicit native goal mutation");
                }
                let stopped = goal(&h).await;
                release.send(()).expect("release prepared resume");
                recovering.await.expect("prepared evaluator settles");
                assert_eq!(goal(&h).await, stopped, "new mutation wins: {change}");
                assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
                assert_eq!(
                    responses::received_responses_requests(&h._server)
                        .await
                        .len(),
                    2,
                    "new stop admits no recovery request: {change}"
                );
                finish(&h).await;
            })
            .await
            .unwrap_or_else(|_| panic!("model-block recovery mutation race hung: {change}"));
        }
    });
}
