// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A healthy goal read cannot authorize a native continuation past a newer explicit stop.

use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::upgrade_handoff_tests::wait_for_supervisor;
use super::*;
use core_test_support::responses::received_responses_requests;

async fn wait_for_request(h: &Harness) {
    while received_responses_requests(&h._server).await.is_empty() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[test]
fn a_goal_stopped_after_preparation_prevents_automatic_native_admission() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let mut outcomes = Vec::new();
            for (compact, stopped) in [false, true].into_iter().flat_map(|compact| {
                [
                    None,
                    Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
                    Some(codex_protocol::protocol::ThreadGoalStatus::Blocked),
                ]
                .map(move |stopped| (compact, stopped))
            }) {
                let goals = Arc::new(GoalService::new());
                let h = Harness::new_with_goals(Some(goals.clone())).await;
                h.fake.set_policy_key("upgrade", "false");
                h.fake.set_policy_key("compact_on_context_wall", "true");
                wait_for_supervisor(|| {
                    h.autopilot
                        .sup
                        .policy()
                        .is_some_and(|policy| !policy.upgrade && policy.compact_on_context_wall)
                })
                .await;
                h.immediate_point().await;
                h.hold_model_response().await;
                if compact {
                    // The fixture supplies a context-wall decision; the actual
                    // native Compact operation and its queue acceptance stay real.
                    h.rt.with_state(|state| {
                        state.observe_turn_end(
                            TurnEnd {
                                at: clock(),
                                by_autopilot: true,
                                by_person: false,
                                text: None,
                                earlier: None,
                                questions: false,
                                worked: Duration::from_secs(600),
                                wall: Some(Wall::Context { auto_compact: true }),
                                aborted: false,
                                ran_tools: false,
                                restored: false,
                            },
                            &h.autopilot.timing,
                        )
                    });
                }
                let db = h.test.codex.state_db().expect("real goal database");
                let absent = db
                    .thread_goals()
                    .get_thread_goal(h.rt.id)
                    .await
                    .expect("healthy absent goal");
                let (entered, entry) = tokio::sync::oneshot::channel();
                let (release, blocked) = tokio::sync::oneshot::channel();
                *lock(&h.rt.before_submit) = Some((entered, blocked));
                let pilot = h.autopilot.clone();
                let rt = h.rt.clone();
                let evaluation = tokio::spawn(async move { pilot.evaluate(rt).await });
                entry
                    .await
                    .expect("actual evaluator passed its final healthy goal read");
                let before = h.rt.state();
                let accepted_before = h.rt.accepted_turns.load(Ordering::SeqCst);
                if let Some(status) = stopped {
                    let outcome = goals
                        .set_thread_goal(
                            &db,
                            GoalSetRequest {
                                thread_id: h.rt.id,
                                objective: GoalObjectiveUpdate::Set(
                                    "preserve the person's stopped task",
                                ),
                                status: Some(status),
                                token_budget: GoalTokenBudgetUpdate::Keep,
                                max_goal_token_budget: None,
                            },
                            async { Ok(()) },
                        )
                        .await
                        .expect("real explicit goal stop");
                    outcome.apply_runtime_effects(&goals).await;
                }
                let after_stop = h.rt.state();
                let accepted_after_stop = h.rt.accepted_turns.load(Ordering::SeqCst);
                let saved = db
                    .thread_goals()
                    .get_thread_goal(h.rt.id)
                    .await
                    .expect("read saved explicit stop");
                release
                    .send(())
                    .expect("release prepared automatic continuation");
                evaluation
                    .await
                    .expect("actual evaluator finishes admission");
                if compact && stopped.is_none() {
                    wait_for_supervisor(|| {
                        h.rt.accepted_turns.load(Ordering::SeqCst) != accepted_after_stop
                    })
                    .await;
                }
                let started = h.rt.accepted_turns.load(Ordering::SeqCst) != accepted_after_stop;
                if started && !compact {
                    wait_for_request(&h).await;
                }
                let requests = received_responses_requests(&h._server).await.len();
                let acts = h.fake.acts();
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("owned native shutdown before verdict");
                outcomes.push((
                    compact,
                    stopped,
                    absent,
                    before == after_stop,
                    accepted_before,
                    accepted_after_stop,
                    saved,
                    started,
                    requests,
                    acts,
                ));
            }
            let mut unsafe_stops = Vec::new();
            for (
                compact,
                stopped,
                absent,
                same_point,
                accepted_before,
                accepted_after_stop,
                saved,
                started,
                requests,
                acts,
            ) in outcomes
            {
                assert!(
                    absent.is_none(),
                    "the preparatory read was healthy and absent"
                );
                assert!(
                    same_point && accepted_before == 0 && accepted_after_stop == 0,
                    "the real goal mutation starts no native turn and changes no decision point"
                );
                if let Some(status) = stopped {
                    assert_eq!(
                        saved.map(|goal| goal.status.as_str().to_owned()),
                        Some(match status {
                            codex_protocol::protocol::ThreadGoalStatus::Paused => "paused".into(),
                            codex_protocol::protocol::ThreadGoalStatus::Blocked => "blocked".into(),
                            _ => unreachable!(),
                        })
                    );
                    if started || requests != 0 || !acts.is_empty() {
                        unsafe_stops.push((compact, status, started, requests, acts));
                    }
                } else {
                    assert!(
                        started
                            && (compact || requests == 1)
                            && acts.iter().any(|act| act
                                == if compact {
                                    words::RULE_COMPACT
                                } else {
                                    words::RULE_CONTINUE
                                }),
                        "healthy absence still admits its Core operation"
                    );
                }
            }
            assert!(
                unsafe_stops.is_empty(),
                "explicit stops must defeat the older healthy read: {unsafe_stops:?}"
            );
        })
        .await
        .expect("goal stop native admission hang detector");
    });
}

#[test]
fn a_persons_native_input_still_starts_while_a_goal_mutation_owns_its_permit() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            for compact in [false, true] {
                let goals = Arc::new(GoalService::new());
                let h = Harness::new_with_goals(Some(goals.clone())).await;
                h.immediate_point().await;
                h.hold_model_response().await;
                let db = h.test.codex.state_db().expect("real goal database");
                let (held, holding) = tokio::sync::oneshot::channel();
                let (release, released) = tokio::sync::oneshot::channel();
                let service = goals.clone();
                let person_db = db.clone();
                let id = h.rt.id;
                let stopping = tokio::spawn(async move {
                    service
                        .set_thread_goal(
                            &person_db,
                            GoalSetRequest {
                                thread_id: id,
                                objective: GoalObjectiveUpdate::Set("the person's paused task"),
                                status: Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
                                token_budget: GoalTokenBudgetUpdate::Keep,
                                max_goal_token_budget: None,
                            },
                            async {
                                held.send(())
                                    .expect("real GoalService mutation permit held");
                                released.await.expect("release recorded owner instruction");
                                Ok(())
                            },
                        )
                        .await
                        .expect("real owner stop")
                });
                holding
                    .await
                    .expect("real GoalService holds its native permit");
                let submitted = if compact {
                    h.test
                        .codex
                        .submit(Op::Compact)
                        .await
                        .expect("actual explicit Compact remains accepted");
                    wait_for_supervisor(|| h.rt.accepted_turns.load(Ordering::SeqCst) != 0).await;
                    None
                } else {
                    Some(
                        h.test
                            .codex
                            .start_or_steer_turn(TurnInputRequest::user_input(vec![
                                UserInput::Text {
                                    text: "The person's explicit correction.".into(),
                                    text_elements: Vec::new(),
                                },
                            ]))
                            .await
                            .expect("actual owner input remains admitted"),
                    )
                };
                let owner_reached = if compact {
                    h.rt.accepted_turns.load(Ordering::SeqCst) != 0
                } else {
                    wait_for_request(&h).await;
                    received_responses_requests(&h._server)
                        .await
                        .iter()
                        .flat_map(|request| request.message_input_texts("user"))
                        .any(|text| text == "The person's explicit correction.")
                };
                release.send(()).expect("finish actual owner goal mutation");
                stopping
                    .await
                    .expect("owner mutation completes")
                    .apply_runtime_effects(&goals)
                    .await;
                let acts = h.fake.acts();
                h.test
                    .codex
                    .shutdown_and_wait()
                    .await
                    .expect("owned native shutdown before verdict");
                assert!(submitted.is_none_or(|submitted| matches!(
                    submitted,
                    codex_protocol::turn_input::TurnInputSubmission::Started { .. }
                )));
                assert!(
                    owner_reached,
                    "a paused goal must not reject or rewrite actual owner input"
                );
                assert!(
                    acts.is_empty(),
                    "the actual owner input is not an automatic act"
                );
            }
        })
        .await
        .expect("owner native input goal-permit hang detector");
    });
}

#[test]
fn a_missing_expected_goal_runtime_holds_and_rechecks_automatic_input() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(60), async {
            let h = Harness::new_with_goals(Some(Arc::new(GoalService::new()))).await;
            h.fake.set_policy_key("upgrade", "false");
            wait_for_supervisor(|| {
                h.autopilot
                    .sup
                    .policy()
                    .is_some_and(|policy| !policy.upgrade)
            })
            .await;
            h.immediate_point().await;
            h.hold_model_response().await;
            // Remove only this owned fixture's extension entry; the real healthy
            // database still exists, so missing goal custody cannot mean no goal.
            h.test
                .codex
                .thread_extension_data()
                .remove::<codex_goal_extension::GoalRuntimeHandle>();
            h.autopilot.clone().evaluate(h.rt.clone()).await;
            let native_starts = h.rt.accepted_turns.load(Ordering::SeqCst);
            let requests = received_responses_requests(&h._server).await.len();
            let recheck = h.rt.timer_pending();
            let acts = h.fake.acts();
            h.test
                .codex
                .shutdown_and_wait()
                .await
                .expect("owned native shutdown before verdict");
            assert!(
                native_starts == 0 && requests == 0 && acts.is_empty(),
                "missing native goal custody grants no automatic input"
            );
            assert!(recheck, "runtime recovery is checked again");
        })
        .await
        .expect("missing native goal runtime hang detector");
    });
}
