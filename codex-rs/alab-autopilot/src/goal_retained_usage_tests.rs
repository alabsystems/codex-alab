// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Positive usage from a failed turn must survive a later manual turn and stop.
use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::native_test;
use super::*;
use codex_protocol::protocol::EventMsg;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::turn_input::TurnInputSubmission;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use sqlx::Connection;

async fn start_person(h: &Harness, text: &str) -> String {
    match h
        .test
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: text.to_owned(),
            text_elements: Vec::new(),
        }]))
        .await
        .unwrap()
    {
        TurnInputSubmission::Started { turn_id, .. } => turn_id,
        other => panic!("expected a new native person turn, got {other:?}"),
    }
}

async fn ended(h: &Harness) {
    core_test_support::wait_for_event_with_timeout(
        &h.test.codex,
        |event| matches!(event, EventMsg::TurnComplete(_)),
        Duration::from_secs(60),
    )
    .await;
    while h.test.codex.has_active_turn().await {
        tokio::task::yield_now().await;
    }
}

async fn fail_first_usage(h: &Harness, tokens: i64) {
    let db = h.test.codex.state_db().unwrap();
    let runtime = h
        .test
        .codex
        .thread_extension_data()
        .get::<codex_goal_extension::GoalRuntimeHandle>()
        .unwrap();
    let _mock = responses::mount_sse_sequence(
        &h._server,
        vec![responses::sse(vec![
            responses::ev_response_created("retained-first"),
            responses::ev_assistant_message(
                "retained-first-message",
                "The first native turn consumed twenty-two tokens.",
            ),
            responses::ev_completed_with_tokens("retained-first", tokens),
        ])],
    )
    .await;
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(db.sqlite().goals_db_path());
    #[expect(
        clippy::disallowed_methods,
        reason = "owned scratch writer injects a definite SQLite BUSY accounting failure with positive model usage"
    )]
    let mut writer = sqlx::SqliteConnection::connect_with(&options)
        .await
        .unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut writer)
        .await
        .unwrap();
    start_person(h, "Do the first part of this task.").await;
    // Core awaits emit_turn_stop_lifecycle before publishing TurnComplete.
    // That hook awaits owned accounting, which finishes its Write before
    // sending the acknowledgement. This event therefore proves a settled
    // failure while the scratch writer is still locked, rather than Writing.
    ended(h).await;
    assert!(
        runtime.alab_goal_persistence_held(),
        "real usage accounting must encounter the bounded BUSY failure"
    );
    let saved = db
        .thread_goals()
        .get_thread_goal(h.rt.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.tokens_used, 0);
    assert_eq!(saved.status, codex_state::ThreadGoalStatus::Active);
    sqlx::query("ROLLBACK").execute(&mut writer).await.unwrap();
    writer.close().await.unwrap();
}

async fn seed(h: &Harness, goals: &GoalService) -> codex_state::ThreadGoal {
    let db = h.test.codex.state_db().unwrap();
    goals
        .set_thread_goal(
            &db,
            GoalSetRequest {
                thread_id: h.rt.id,
                objective: GoalObjectiveUpdate::Set("preserve the first turn's twenty-two tokens"),
                status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                token_budget: GoalTokenBudgetUpdate::Keep,
                max_goal_token_budget: None,
            },
            async { Ok(()) },
        )
        .await
        .unwrap();
    h.test
        .codex
        .thread_extension_data()
        .get::<codex_goal_extension::GoalRuntimeHandle>()
        .unwrap()
        .restore_after_resume()
        .await
        .unwrap();
    db.thread_goals()
        .get_thread_goal(h.rt.id)
        .await
        .unwrap()
        .unwrap()
}

#[test]
fn terminal_paths_flush_retained_tokens_before_stopping_a_later_manual_turn() {
    native_test(|| async {
        for tool_stop in [false, true] {
            tokio::time::timeout(Duration::from_secs(60), async {
                let goals = Arc::new(GoalService::new());
                let mut h = Harness::new_with_accounted_goals(goals.clone()).await;
                h.retain_artifacts();
                let evaluation = h.rt.hold_eval_when_free().await;
                let original = seed(&h, &goals).await;
                fail_first_usage(&h, 22).await;
                let db = h.test.codex.state_db().unwrap();
                let runtime = h
                    .test
                    .codex
                    .thread_extension_data()
                    .get::<codex_goal_extension::GoalRuntimeHandle>()
                    .unwrap();
                let expected_tokens = if tool_stop {
                    let _mock = responses::mount_sse_sequence(
                        &h._server,
                        vec![
                            responses::sse(vec![
                                responses::ev_response_created("retained-stop-tool"),
                                responses::ev_function_call(
                                    "retained-stop-call",
                                    codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
                                    r#"{"status":"complete"}"#,
                                ),
                                responses::ev_completed_with_tokens("retained-stop-tool", 10),
                            ]),
                            responses::sse(vec![
                                responses::ev_response_created("retained-final"),
                                responses::ev_assistant_message(
                                    "retained-final-message",
                                    "The goal is complete.",
                                ),
                                responses::ev_completed("retained-final"),
                            ]),
                        ],
                    )
                    .await;
                    start_person(&h, "Complete the goal with update_goal now.").await;
                    ended(&h).await;
                    32
                } else {
                    h.hold_model_response().await;
                    let turn = start_person(&h, "Continue in a new manual turn.").await;
                    while responses::received_responses_requests(&h._server)
                        .await
                        .len()
                        < 2
                    {
                        tokio::task::yield_now().await;
                    }
                    runtime
                        .usage_limit_active_goal_for_turn(&turn)
                        .await
                        .unwrap();
                    runtime
                        .usage_limit_active_goal_for_turn(&turn)
                        .await
                        .unwrap();
                    22
                };
                let stopped = db
                    .thread_goals()
                    .get_thread_goal(h.rt.id)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(stopped.goal_id, original.goal_id);
                assert_eq!(stopped.objective, original.objective);
                assert_eq!(
                    stopped.tokens_used, expected_tokens,
                    "old usage must be accounted exactly once before the terminal status"
                );
                assert_eq!(
                    stopped.status,
                    if tool_stop {
                        codex_state::ThreadGoalStatus::Complete
                    } else {
                        codex_state::ThreadGoalStatus::UsageLimited
                    }
                );
                // Another explicit native mutation must not charge the old turn again.
                goals
                    .set_thread_goal(
                        &db,
                        GoalSetRequest {
                            thread_id: h.rt.id,
                            objective: GoalObjectiveUpdate::Keep,
                            status: Some(if tool_stop {
                                codex_protocol::protocol::ThreadGoalStatus::Complete
                            } else {
                                codex_protocol::protocol::ThreadGoalStatus::UsageLimited
                            }),
                            token_budget: GoalTokenBudgetUpdate::Keep,
                            max_goal_token_budget: None,
                        },
                        async { Ok(()) },
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    db.thread_goals()
                        .get_thread_goal(h.rt.id)
                        .await
                        .unwrap()
                        .unwrap()
                        .tokens_used,
                    expected_tokens
                );
                h.shutdown_and_export().await.unwrap();
                drop(evaluation);
            })
            .await
            .expect("retained usage before terminal status hang detector");
        }
    });
}

#[test]
fn same_original_stopped_goal_recovers_but_replacement_keeps_retained_usage_held() {
    native_test(|| async {
        for status in [
            Some(codex_state::ThreadGoalStatus::Blocked),
            Some(codex_state::ThreadGoalStatus::Complete),
            None,
        ] {
            tokio::time::timeout(Duration::from_secs(60), async {
                let goals = Arc::new(GoalService::new());
                let mut h = Harness::new_with_accounted_goals(goals.clone()).await;
                h.retain_artifacts();
                let evaluation = h.rt.hold_eval_when_free().await;
                let original = seed(&h, &goals).await;
                fail_first_usage(&h, 22).await;
                let db = h.test.codex.state_db().unwrap();
                let runtime = h
                    .test
                    .codex
                    .thread_extension_data()
                    .get::<codex_goal_extension::GoalRuntimeHandle>()
                    .unwrap();
                if let Some(status) = status {
                    db.thread_goals()
                        .update_thread_goal(
                            h.rt.id,
                            codex_state::GoalUpdate {
                                objective: None,
                                status: Some(status),
                                token_budget: None,
                                expected_goal_id: Some(original.goal_id.clone()),
                            },
                        )
                        .await
                        .unwrap();
                    goals
                        .set_thread_goal(
                            &db,
                            GoalSetRequest {
                                thread_id: h.rt.id,
                                objective: GoalObjectiveUpdate::Keep,
                                status: Some(
                                    if status == codex_state::ThreadGoalStatus::Complete {
                                        codex_protocol::protocol::ThreadGoalStatus::Complete
                                    } else {
                                        codex_protocol::protocol::ThreadGoalStatus::Paused
                                    },
                                ),
                                token_budget: GoalTokenBudgetUpdate::Keep,
                                max_goal_token_budget: None,
                            },
                            async { Ok(()) },
                        )
                        .await
                        .unwrap();
                    let recovered = db
                        .thread_goals()
                        .get_thread_goal(h.rt.id)
                        .await
                        .unwrap()
                        .unwrap();
                    assert_eq!(recovered.goal_id, original.goal_id);
                    assert_eq!(recovered.tokens_used, 22);
                    assert!(!runtime.alab_goal_persistence_held());
                    let (checkpoint, transfer) = runtime
                        .alab_upgrade_goal_state_checkpoint(h.rt.id)
                        .await
                        .expect("recovered idle Core turn must release its native liveness marker");
                    assert_eq!(checkpoint.unwrap(), recovered);
                    drop(transfer);
                } else {
                    let replacement = db
                        .thread_goals()
                        .replace_thread_goal(
                            h.rt.id,
                            "a different goal must not inherit old usage",
                            codex_state::ThreadGoalStatus::Active,
                            None,
                        )
                        .await
                        .unwrap();
                    h.hold_model_response().await;
                    let turn = start_person(&h, "A manual turn on the replacement goal.").await;
                    while responses::received_responses_requests(&h._server)
                        .await
                        .len()
                        < 2
                    {
                        tokio::task::yield_now().await;
                    }
                    for _ in 0..2 {
                        assert!(
                            runtime
                                .usage_limit_active_goal_for_turn(&turn)
                                .await
                                .is_err(),
                            "repeated refusal must retain the original pending snapshot"
                        );
                        assert_eq!(
                            db.thread_goals()
                                .get_thread_goal(h.rt.id)
                                .await
                                .unwrap()
                                .unwrap(),
                            replacement
                        );
                        assert!(runtime.alab_goal_persistence_held());
                    }
                }
                h.shutdown_and_export().await.unwrap();
                drop(evaluation);
            })
            .await
            .expect("same versus replaced retained goal recovery hang detector");
        }
    });
}

#[test]
fn a_new_manual_turn_cannot_drop_shared_usage_from_a_zero_token_failed_turn() {
    native_test(|| async {
        for plan in [false, true] {
            tokio::time::timeout(Duration::from_secs(60), async {
                let goals = Arc::new(GoalService::new());
                let mut h = Harness::new_with_accounted_goals(goals.clone()).await;
                h.retain_artifacts();
                let evaluation = h.rt.hold_eval_when_free().await;
                let original = seed(&h, &goals).await;
                let runtime = h
                    .test
                    .codex
                    .thread_extension_data()
                    .get::<codex_goal_extension::GoalRuntimeHandle>()
                    .unwrap();
                let baseline = codex_alab_core::goal_clock::Instant::now()
                    .checked_sub(Duration::from_secs(60))
                    .unwrap();
                runtime
                    .alab_restore_upgrade_goal_clock(
                        h.rt.id,
                        &original,
                        codex_alab_core::goal_clock::checkpoint(&baseline).unwrap(),
                    )
                    .await
                    .unwrap();
                fail_first_usage(&h, 0).await;
                let db = h.test.codex.state_db().unwrap();
                if plan {
                    let response = responses::sse(vec![
                        responses::ev_response_created("retained-plan"),
                        responses::ev_assistant_message(
                            "retained-plan-message",
                            "The plan is ready.",
                        ),
                        responses::ev_completed_with_tokens("retained-plan", 0),
                    ]);
                    wiremock::Mock::given(wiremock::matchers::method("POST"))
                        .respond_with(
                            wiremock::ResponseTemplate::new(200)
                                .set_body_raw(response, "text/event-stream")
                                .set_delay(Duration::from_secs(2)),
                        )
                        .up_to_n_times(1)
                        .mount(&h._server)
                        .await;
                    let mut request = TurnInputRequest::user_input(vec![UserInput::Text {
                        text: "Make a plan without changing the active goal.".to_owned(),
                        text_elements: Vec::new(),
                    }]);
                    request.thread_settings.collaboration_mode =
                        Some(codex_protocol::config_types::CollaborationMode {
                            mode: codex_protocol::config_types::ModeKind::Plan,
                            settings: codex_protocol::config_types::Settings {
                                model: h.test.session_configured.model.clone(),
                                reasoning_effort: None,
                                developer_instructions: None,
                            },
                        });
                    assert!(matches!(
                        h.test.codex.start_or_steer_turn(request).await.unwrap(),
                        TurnInputSubmission::Started { .. }
                    ));
                    while responses::received_responses_requests(&h._server)
                        .await
                        .len()
                        < 2
                    {
                        tokio::task::yield_now().await;
                    }
                    let pre_plan = db
                        .thread_goals()
                        .get_thread_goal(h.rt.id)
                        .await
                        .unwrap()
                        .unwrap();
                    assert!(
                        pre_plan.time_used_seconds >= 60,
                        "the original positive clock must be acknowledged before Plan model input"
                    );
                    ended(&h).await;
                    goals
                        .set_thread_goal(
                            &db,
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
                        .unwrap();
                    let recovered = db
                        .thread_goals()
                        .get_thread_goal(h.rt.id)
                        .await
                        .unwrap()
                        .unwrap();
                    assert_eq!(recovered.goal_id, original.goal_id);
                    assert_eq!(recovered.tokens_used, 0);
                    assert!(
                        recovered.time_used_seconds >= 60,
                        "a Plan turn must preserve the old positive shared clock"
                    );
                    assert_eq!(
                        recovered.time_used_seconds, pre_plan.time_used_seconds,
                        "the delayed Plan response must not add Plan seconds to old goal usage"
                    );
                    assert_eq!(recovered.status, codex_state::ThreadGoalStatus::Paused);
                    assert!(!runtime.alab_goal_persistence_held());
                    let (checkpoint, transfer) = runtime
                        .alab_upgrade_goal_state_checkpoint(h.rt.id)
                        .await
                        .unwrap();
                    assert_eq!(checkpoint.unwrap(), recovered);
                    drop(transfer);
                    h.shutdown_and_export().await.unwrap();
                    drop(evaluation);
                    return;
                }
                let replacement = db
                    .thread_goals()
                    .replace_thread_goal(
                        h.rt.id,
                        "raw replacement cannot erase unacknowledged shared time",
                        codex_state::ThreadGoalStatus::Active,
                        None,
                    )
                    .await
                    .unwrap();
                h.hold_model_response().await;
                let turn = start_person(&h, "Continue manually after an outside goal write.").await;
                while responses::received_responses_requests(&h._server)
                    .await
                    .len()
                    < 2
                {
                    tokio::task::yield_now().await;
                }
                for _ in 0..2 {
                    assert!(
                        runtime
                            .usage_limit_active_goal_for_turn(&turn)
                            .await
                            .is_err()
                    );
                    assert!(
                        goals
                            .set_thread_goal(
                                &db,
                                GoalSetRequest {
                                    thread_id: h.rt.id,
                                    objective: GoalObjectiveUpdate::Keep,
                                    status: Some(
                                        codex_protocol::protocol::ThreadGoalStatus::Paused
                                    ),
                                    token_budget: GoalTokenBudgetUpdate::Keep,
                                    max_goal_token_budget: None,
                                },
                                async { Ok(()) },
                            )
                            .await
                            .is_err()
                    );
                    assert_eq!(
                        db.thread_goals()
                            .get_thread_goal(h.rt.id)
                            .await
                            .unwrap()
                            .unwrap(),
                        replacement
                    );
                    assert!(runtime.alab_goal_persistence_held());
                    assert!(
                        runtime
                            .alab_upgrade_goal_state_checkpoint(h.rt.id)
                            .await
                            .is_err()
                    );
                }
                h.shutdown_and_export().await.unwrap();
                drop(evaluation);
            })
            .await
            .expect("zero-root-token shared accounting quarantine hang detector");
        }
    });
}

#[test]
fn rejected_current_and_idle_usage_survive_native_unchanged_outcomes() {
    native_test(|| async {
        for idle in [false, true] {
            tokio::time::timeout(Duration::from_secs(60), async {
                let goals = Arc::new(GoalService::new());
                let mut h = Harness::new_with_accounted_goals(goals.clone()).await;
                h.retain_artifacts();
                let evaluation = h.rt.hold_eval_when_free().await;
                let original = seed(&h, &goals).await;
                let db = h.test.codex.state_db().unwrap();
                let runtime = h.test.codex.thread_extension_data()
                    .get::<codex_goal_extension::GoalRuntimeHandle>().unwrap();
                if idle {
                    let baseline = codex_alab_core::goal_clock::Instant::now()
                        .checked_sub(Duration::from_secs(60)).unwrap();
                    runtime.alab_restore_upgrade_goal_clock(h.rt.id, &original,
                        codex_alab_core::goal_clock::checkpoint(&baseline).unwrap()).await.unwrap();
                } else {
                    let first = responses::sse(vec![
                        responses::ev_response_created("retained-live-tool"),
                        responses::ev_function_call("retained-live-stop",
                            codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
                            r#"{"status":"complete"}"#),
                        responses::ev_completed_with_tokens("retained-live-tool", 22),
                    ]);
                    let last = responses::sse(vec![
                        responses::ev_response_created("retained-live-wait"),
                        responses::ev_completed("retained-live-wait"),
                    ]);
                    responses::mount_response_sequence(&h._server, vec![
                        responses::sse_response(first),
                        responses::sse_response(last).set_delay(Duration::from_secs(60)),
                    ]).await;
                }
                let options = sqlx::sqlite::SqliteConnectOptions::new().filename(db.sqlite().goals_db_path());
                #[expect(clippy::disallowed_methods, reason = "owned scratch lock proves rejected positive accounting before changing the native row status")]
                let mut writer = sqlx::SqliteConnection::connect_with(&options).await.unwrap();
                sqlx::query("BEGIN IMMEDIATE").execute(&mut writer).await.unwrap();
                if idle {
                    assert!(runtime.alab_upgrade_goal_state_checkpoint(h.rt.id).await.is_err());
                } else {
                    start_person(&h, "Complete this goal using update_goal.").await;
                    while responses::received_responses_requests(&h._server).await.len() < 2 {
                        tokio::task::yield_now().await;
                    }
                    // The second request contains the failed tool result, so its
                    // additive owner has acknowledged definite BUSY noncommit.
                }
                assert!(runtime.alab_goal_persistence_held());
                let unchanged = db.thread_goals().get_thread_goal(h.rt.id).await.unwrap().unwrap();
                assert_eq!(unchanged.goal_id, original.goal_id);
                assert_eq!(unchanged.tokens_used, 0);
                assert_eq!(unchanged.time_used_seconds, 0);
                sqlx::query("ROLLBACK").execute(&mut writer).await.unwrap();
                writer.close().await.unwrap();
                if idle {
                    let replacement = db.thread_goals().replace_thread_goal(h.rt.id,
                        "raw replacement after idle-only accounting failure",
                        codex_state::ThreadGoalStatus::Active, None).await.unwrap();
                    for _ in 0..2 {
                        assert!(goals.set_thread_goal(&db, GoalSetRequest {
                            thread_id: h.rt.id, objective: GoalObjectiveUpdate::Keep,
                            status: Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
                            token_budget: GoalTokenBudgetUpdate::Keep, max_goal_token_budget: None,
                        }, async { Ok(()) }).await.is_err());
                        assert_eq!(db.thread_goals().get_thread_goal(h.rt.id).await.unwrap().unwrap(), replacement);
                        assert!(runtime.alab_goal_persistence_held());
                    }
                    assert!(responses::received_responses_requests(&h._server).await.is_empty());
                } else {
                    db.thread_goals().update_thread_goal(h.rt.id, codex_state::GoalUpdate {
                        objective: None, status: Some(codex_state::ThreadGoalStatus::Complete),
                        token_budget: None, expected_goal_id: Some(original.goal_id.clone()),
                    }).await.unwrap();
                    h.test.codex.submit(codex_protocol::protocol::Op::Interrupt).await.unwrap();
                    core_test_support::wait_for_event_with_timeout(&h.test.codex,
                        |event| matches!(event, EventMsg::TurnAborted(_)), Duration::from_secs(60)).await;
                    while h.test.codex.has_active_turn().await { tokio::task::yield_now().await; }
                    assert!(runtime.alab_goal_persistence_held());
                    goals.set_thread_goal(&db, GoalSetRequest {
                        thread_id: h.rt.id, objective: GoalObjectiveUpdate::Keep,
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Complete),
                        token_budget: GoalTokenBudgetUpdate::Keep, max_goal_token_budget: None,
                    }, async { Ok(()) }).await.unwrap();
                    let recovered = db.thread_goals().get_thread_goal(h.rt.id).await.unwrap().unwrap();
                    assert_eq!(recovered.goal_id, original.goal_id);
                    assert_eq!(recovered.status, codex_state::ThreadGoalStatus::Complete);
                    assert_eq!(recovered.tokens_used, 22,
                        "ActiveOnly Unchanged during abort cannot discard original positive usage");
                    assert!(!runtime.alab_goal_persistence_held());
                    let (checkpoint, transfer) = runtime.alab_upgrade_goal_state_checkpoint(h.rt.id).await.unwrap();
                    assert_eq!(checkpoint.unwrap(), recovered);
                    drop(transfer);
                }
                h.shutdown_and_export().await.unwrap();
                drop(evaluation);
            }).await.expect("positive Unchanged retention hang detector");
        }
    });
}
