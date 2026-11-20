// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Positive usage from a failed turn must survive a later manual turn and stop.
use super::upgrade_handoff_tests::Harness;
use super::upgrade_handoff_tests::NATIVE_HANG;
use super::upgrade_handoff_tests::native_test;
use super::upgrade_handoff_tests::wait_for_supervisor;
use super::*;
use codex_alab_core::session::RecoveryWall;
use codex_protocol::protocol::EventMsg;
use codex_protocol::turn_input::TurnInputRequest;
use codex_protocol::turn_input::TurnInputSubmission;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use sqlx::Connection;
use std::future::Future;
use std::sync::Condvar;
use std::task::Poll;

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

struct RecoveryFlushGate(Arc<(Mutex<bool>, Condvar)>);

impl RecoveryFlushGate {
    fn release(&self) {
        *lock(&self.0.0) = true;
        self.0.1.notify_all();
    }
}

impl Drop for RecoveryFlushGate {
    fn drop(&mut self) {
        self.release();
    }
}

async fn fail_recovery_flush(start_successor: bool) {
    const SUCCESSOR: &str = "My next native turn supersedes the usage wall.";
    const RESPONSE_DELAY: Duration = Duration::from_secs(120);
    let h = Harness::new().await;
    let evaluation = h.rt.hold_eval_when_free().await;
    for (key, value) in [
        ("upgrade", "false"),
        ("limit_wait", "true"),
        ("resume_limits", "true"),
        ("probe_api", "false"),
    ] {
        h.fake.set_policy_key(key, value);
    }
    wait_for_supervisor(|| {
        h.autopilot.sup.policy().is_some_and(|policy| {
            !policy.upgrade && policy.limit_wait && policy.resume_limits && !policy.probe_api
        })
    })
    .await;
    let reset_epoch = now_ms() / 1000 + 24 * 3600;
    let requests = responses::mount_response_handler(&h._server, move |request| {
        if request.body_contains_text(SUCCESSOR) {
            return wiremock::ResponseTemplate::new(200).set_delay(RESPONSE_DELAY);
        }
        wiremock::ResponseTemplate::new(429).set_body_json(serde_json::json!({
            "error": {
                "type": "usage_limit_reached",
                "message": "the original turn reached its usage limit",
                "resets_at": reset_epoch,
                "plan_type": "pro"
            }
        }))
    })
    .await;

    let evaluations = h.rt.evaluations();
    let original = start_person(&h, "Start the task that reaches the usage limit.").await;
    ended(&h).await;
    wait_for_supervisor(|| h.rt.evaluations() > evaluations).await;
    let before = h.rt.state();
    assert_eq!(before.point_turn_id.as_deref(), Some(original.as_str()));
    assert!(matches!(
        before.point.as_ref().and_then(|point| point.wall.as_ref()),
        Some(Wall::Usage { reset_at: Some(at), .. }) if *at > clock() + WRITE_THROUGH_RETRY
    ));
    let policy = h.autopilot.sup.policy().expect("proven policy");
    let decision = decide::decide(&before, &policy, &h.autopilot.timing, clock(), h.rt.jitter);
    assert!(matches!(decision.act, Act::ResumeLimit { .. }));
    assert!(decision.not_before.is_some_and(|at| at > clock()));
    assert_eq!(requests.requests().len(), 1);

    // Reject just the old turn's real recovery flush. Other record boundaries
    // use the fixture's ordinary successful no-record witness.
    let gate = RecoveryFlushGate(Arc::new((Mutex::new(false), Condvar::new())));
    let waiting = Arc::clone(&gate.0);
    let shared = h.shared;
    let old_turn = original.clone();
    let (entered, recorded) = tokio::sync::oneshot::channel();
    let (failed, confirmed_failure) = tokio::sync::oneshot::channel();
    let pending = Mutex::new(Some((entered, failed)));
    assert!(shared.set_flush(Box::new(move || {
        let target = shared.recovery().is_some_and(|recovery| {
            recovery.turn_id == old_turn && matches!(recovery.wall, RecoveryWall::Usage { .. })
        });
        let selected = target.then(|| lock(&pending).take()).flatten();
        let Some((entered, failed)) = selected else {
            return true;
        };
        entered.send(()).expect("observe the old recovery flush");
        let (released, changed) = &*waiting;
        let (ready, _) = changed
            .wait_timeout_while(lock(released), Duration::from_secs(60), |ready| !*ready)
            .expect("recovery flush gate lock");
        assert!(*ready, "the old recovery flush must be released");
        failed.send(()).expect("observe the definitive failure");
        false
    })));
    h.autopilot.set_recovery_write_budget(RESPONSE_DELAY);
    drop(evaluation);
    recorded.await.expect("persist the original provider wait");
    let original_recovery = h.shared.recovery().expect("the published recovery");
    assert_eq!(original_recovery.turn_id, original);

    let mut next = Box::pin(h.rt.hold_eval_when_free());
    std::future::poll_fn(|cx| {
        assert!(next.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
    let successor = if start_successor {
        let successor = start_person(&h, SUCCESSOR).await;
        assert_ne!(successor, original);
        // The successor starts its request only after Core has awaited the
        // real start hook, including recovery removal and timer cancellation.
        wait_for_supervisor(|| requests.requests().len() >= 2).await;
        assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
        assert!(h.test.codex.has_active_turn().await);
        assert!(h.shared.turn_running_on(h.rt.key()));
        assert!(h.shared.recovery().is_none());
        Some(successor)
    } else {
        None
    };
    assert!(!h.rt.timer_pending());
    assert!(lock(&h.rt.fired).is_none());
    let timer_before = h.rt.timer_seq.load(Ordering::SeqCst);
    let earliest = clock() + WRITE_THROUGH_RETRY;
    gate.release();
    confirmed_failure
        .await
        .expect("the selected flush returned false");
    let next = next.await;
    let latest = clock() + WRITE_THROUGH_RETRY;
    assert!(
        h.autopilot
            .sup
            .journal()
            .iter()
            .any(|note| { note.contains("could not persist the provider recovery wait") })
    );
    assert!(h.rt.owed_timer().is_none());
    assert!(lock(&h.rt.fired).is_none());
    assert!(h.shared.owed_act().is_none());
    assert!(h.fake.acts().is_empty());

    if let Some(successor) = successor {
        assert_eq!(h.rt.timer_seq.load(Ordering::SeqCst), timer_before);
        assert!(!h.rt.timer_pending());
        assert!(h.shared.recovery().is_none());
        assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
        assert_eq!(
            h.rt.state().native_origin,
            Some(codex_alab_core::session::NativeOrigin {
                turn_id: successor,
                phase: codex_alab_core::session::NativePhase::Running,
            })
        );
        assert!(h.shared.turn_running_on(h.rt.key()));
        assert_eq!(requests.requests().len(), 2);
    } else {
        assert_eq!(h.rt.timer_seq.load(Ordering::SeqCst), timer_before + 1);
        let due =
            h.rt.timer_due()
                .expect("current recovery gets its own delayed retry");
        assert!((earliest..=latest).contains(&due));
        assert_eq!(h.rt.state().point_at(), before.point_at());
        assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
        assert_eq!(h.shared.recovery(), Some(original_recovery));
        assert_eq!(requests.requests().len(), 1);
    }
    Autopilot::cancel_timer(&h.rt).await;
    drop(next);
    h.test
        .codex
        .shutdown_and_wait()
        .await
        .expect("native Core shutdown");
}

#[test]
fn a_failed_recovery_write_cannot_rearm_after_a_newer_native_turn() {
    native_test(|| async {
        tokio::time::timeout(NATIVE_HANG, fail_recovery_flush(true))
            .await
            .expect("obsolete recovery flush hang detector");
    });
}

#[test]
fn a_failed_recovery_write_rechecks_its_current_point() {
    native_test(|| async {
        tokio::time::timeout(NATIVE_HANG, fail_recovery_flush(false))
            .await
            .expect("current recovery flush hang detector");
    });
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
    let outcome = goals
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
    let setup_goal = db
        .thread_goals()
        .get_thread_goal(h.rt.id)
        .await
        .expect("read setup goal")
        .expect("setup goal exists");
    db.thread_goals()
        .replace_thread_goal_snapshot(&setup_goal)
        .await
        .expect("persist setup-only continuation deferral");
    outcome.apply_runtime_effects(goals).await;
    db.thread_goals()
        .clear_thread_goal_continuation_deferral(h.rt.id)
        .await
        .expect("clear setup-only continuation deferral");
    assert!(
        !goals
            .alab_goal_runtime(h.rt.id)
            .expect("setup goal runtime")
            .alab_goal_persistence_held(),
        "the setup-only continuation deferral must settle mutation custody"
    );
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
fn terminal_finalizer_accounts_exact_post_dispatch_callback_for_every_stopped_status() {
    native_test(|| async {
        tokio::time::timeout(Duration::from_secs(120), async {
            for status in [
                codex_state::ThreadGoalStatus::Blocked,
                codex_state::ThreadGoalStatus::Paused,
                codex_state::ThreadGoalStatus::Complete,
            ] {
                let goals = Arc::new(GoalService::new());
                let h = Harness::new_with_accounted_goals(Arc::clone(&goals)).await;
                let evaluation = h.rt.hold_eval_when_free().await;
                let original = seed(&h, &goals).await;
                let db = h.test.codex.state_db().expect("native goal database");
                let runtime = h
                    .test
                    .codex
                    .thread_extension_data()
                    .get::<codex_goal_extension::GoalRuntimeHandle>()
                    .expect("native goal runtime");
                let turn_id = format!("terminal-callback-{status:?}");
                let write = runtime
                    .alab_debug_begin_dispatched_goal_mutation_with_usage(
                        &turn_id,
                        &original.goal_id,
                        &codex_protocol::protocol::TokenUsage {
                            input_tokens: 23,
                            total_tokens: 23,
                            ..Default::default()
                        },
                    )
                    .expect("construct a post-dispatch callback boundary");
                let updated = db
                    .thread_goals()
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
                    .expect("commit the terminal row")
                    .expect("the original goal accepts its terminal status");
                assert_eq!(updated.status, status);
                db.thread_goals()
                    .alab_durable_goal_barrier()
                    .await
                    .expect("make the terminal row durable");
                drop(
                    write
                        .commit_mutation()
                        .expect("publish the pending-data mutation transition"),
                );
                assert!(
                    runtime.alab_goal_persistence_held(),
                    "pending terminal callback data remains admission-held"
                );

                runtime
                    .alab_debug_account_ending_turn_progress(
                        &turn_id,
                        &format!("{turn_id}:turn-end"),
                    )
                    .await
                    .expect("the terminal status accepts its exact retained callback");
                let settled = db
                    .thread_goals()
                    .get_thread_goal(h.rt.id)
                    .await
                    .expect("read the settled goal")
                    .expect("the original goal remains");
                assert_eq!(settled.goal_id, original.goal_id);
                assert_eq!(settled.status, status);
                assert_eq!(settled.tokens_used, original.tokens_used + 23);
                assert!(
                    !runtime.alab_goal_persistence_held(),
                    "the exact additive owner reconciles terminal callback custody"
                );
                runtime.alab_debug_finish_goal_turn(&turn_id);
                Autopilot::cancel_timer(&h.rt).await;
                drop(evaluation);
                h.shutdown_and_export().await.expect("native shutdown");
            }
        })
        .await
        .expect("terminal callback finalizer hang detector");
    });
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
                let outcome = goals
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
                outcome.apply_runtime_effects(&goals).await;
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
                    let outcome = goals
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
                    outcome.apply_runtime_effects(&goals).await;
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
                    let outcome = goals
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
                    outcome.apply_runtime_effects(&goals).await;
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
                    let outcome = goals.set_thread_goal(&db, GoalSetRequest {
                        thread_id: h.rt.id, objective: GoalObjectiveUpdate::Keep,
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Complete),
                        token_budget: GoalTokenBudgetUpdate::Keep, max_goal_token_budget: None,
                    }, async { Ok(()) }).await.unwrap();
                    outcome.apply_runtime_effects(&goals).await;
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
