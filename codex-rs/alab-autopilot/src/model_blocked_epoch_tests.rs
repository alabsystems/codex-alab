// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native host mutations revoke live model proof even when values return unchanged.

use super::*;
use codex_protocol::protocol::HookEventName;
use sqlx::Connection;

async fn host_status(
    h: &Harness,
    goals: &GoalService,
    status: codex_protocol::protocol::ThreadGoalStatus,
) {
    let outcome = goals
        .set_thread_goal(
            &h.test.codex.state_db().expect("native goal database"),
            GoalSetRequest {
                thread_id: h.rt.id,
                objective: GoalObjectiveUpdate::Keep,
                status: Some(status),
                token_budget: GoalTokenBudgetUpdate::Keep,
                max_goal_token_budget: None,
            },
            async { Ok(()) },
        )
        .await
        .expect("persist the automatic native host status");
    // Match ThreadGoalProcessor::set: persist the actual API outcome before
    // runtime effects. Model event notifications do not fabricate history.
    h.test
        .codex
        .append_rollout_items(&[outcome.thread_goal_updated_item()])
        .await
        .expect("append the actual native API outcome");
    outcome.apply_runtime_effects(goals).await;
    h.test
        .codex
        .flush_rollout()
        .await
        .expect("flush the host status");
}

async fn host_round_trip(h: &Harness, goals: &GoalService) {
    let before = goal(h).await;
    host_status(h, goals, codex_protocol::protocol::ThreadGoalStatus::Paused).await;
    assert_eq!(goal(h).await.status, ThreadGoalStatus::Paused);
    host_status(
        h,
        goals,
        codex_protocol::protocol::ThreadGoalStatus::Blocked,
    )
    .await;
    let after = goal(h).await;
    assert_eq!(after.goal_id, before.goal_id);
    assert_eq!(after.objective, before.objective);
    assert_eq!(after.token_budget, before.token_budget);
    assert_eq!(after.status, ThreadGoalStatus::Blocked);
    assert_eq!(
        h.shared.person_goal_stops(h.rt.key()),
        0,
        "automatic native API changes do not use the person-origin fence"
    );
}

#[test]
fn a_native_host_round_trip_revokes_live_model_proof_before_restart() {
    native_test(|| async {
        tokio::time::timeout(HANG, live_round_trip())
            .await
            .expect("live model-source mutation hang detector");
    });
}

async fn live_round_trip() {
    let (h, goals) = harness().await;
    let evaluation = h.rt.hold_eval_when_free().await;
    seed(&h, &goals).await;
    model_stop(&h, "blocked").await;
    let source = h
        .autopilot
        .current_model_goal(&h.rt)
        .await
        .expect("read live native proof")
        .expect("the real Active-to-Blocked commit supplies proof");
    assert_eq!(
        lock(&h.rt.model_blocked_goal)
            .as_ref()
            .map(|binding| &binding.goal),
        Some(&source)
    );
    host_round_trip(&h, &goals).await;
    let stopped = goal(&h).await;
    let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
    let requests = responses::received_responses_requests(&h._server)
        .await
        .len();
    assert_eq!(
        h.autopilot
            .current_model_goal(&h.rt)
            .await
            .expect("recheck live native proof after acknowledged mutations"),
        None,
        "matching final values cannot restore live proof revoked by the native API"
    );
    drop(evaluation);
    due(&h).await;
    evaluate(&h).await;
    assert_eq!(goal(&h).await, stopped);
    assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
    assert_eq!(
        responses::received_responses_requests(&h._server)
            .await
            .len(),
        requests
    );
    finish(&h).await;
}

#[test]
fn a_native_host_round_trip_defeats_a_prepared_model_resume_before_admission() {
    native_test(|| async {
        tokio::time::timeout(HANG, prepared_round_trip())
            .await
            .expect("prepared model-source mutation hang detector");
    });
}

async fn prepared_round_trip() {
    let (h, goals) = harness().await;
    let evaluation = h.rt.hold_eval_when_free().await;
    seed(&h, &goals).await;
    model_stop(&h, "blocked").await;
    assert!(
        h.autopilot
            .current_model_goal(&h.rt)
            .await
            .expect("read live native proof")
            .is_some()
    );
    due(&h).await;
    let (entered, preparing) = tokio::sync::oneshot::channel();
    let (release, preparation) = tokio::sync::oneshot::channel();
    *lock(&h.rt.before_submit) = Some((entered, preparation));
    let pilot = Arc::clone(&h.autopilot);
    let rt = Arc::clone(&h.rt);
    drop(evaluation);
    let evaluating = tokio::spawn(async move { pilot.evaluate(rt).await });
    preparing
        .await
        .expect("the real resume passed its preparatory native goal read");
    let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
    let requests = responses::received_responses_requests(&h._server)
        .await
        .len();
    host_round_trip(&h, &goals).await;
    let stopped = goal(&h).await;
    release.send(()).expect("release final native admission");
    evaluating.await.expect("the prepared evaluator settles");
    assert_eq!(
        goal(&h).await,
        stopped,
        "the acknowledged host round-trip must defeat the already prepared resume"
    );
    assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
    assert_eq!(
        responses::received_responses_requests(&h._server)
            .await
            .len(),
        requests
    );
    finish(&h).await;
}

#[test]
fn native_usage_accounting_alone_preserves_live_model_proof_and_recovery() {
    native_test(|| async {
        tokio::time::timeout(HANG, accounting_only())
            .await
            .expect("accounting-only model-source control hang detector");
    });
}

async fn accounting_only() {
    let (h, goals) = harness().await;
    let evaluation = h.rt.hold_eval_when_free().await;
    let original = seed(&h, &goals).await;
    model_stop(&h, "blocked").await;
    let source = h
        .autopilot
        .current_model_goal(&h.rt)
        .await
        .expect("read the original native source")
        .expect("the native block supplied live proof");
    let before = goal(&h).await;
    h.test
        .codex
        .state_db()
        .expect("native goal database")
        .thread_goals()
        .account_thread_goal_usage(
            h.rt.id,
            0,
            1,
            codex_state::GoalAccountingMode::ActiveOrStopped,
            Some(&original.goal_id),
        )
        .await
        .expect("charge native usage without a goal mutation");
    let charged = goal(&h).await;
    assert_eq!(charged.tokens_used, before.tokens_used + 1);
    assert_eq!(charged.status, ThreadGoalStatus::Blocked);
    assert_eq!(
        h.autopilot
            .current_model_goal(&h.rt)
            .await
            .expect("read proof after native accounting"),
        Some(source),
        "ordinary usage must not revoke model-stop proof"
    );
    let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
    drop(evaluation);
    due(&h).await;
    evaluate(&h).await;
    wait_requests(&h, 3).await;
    let resumed = goal(&h).await;
    assert_eq!(resumed.goal_id, original.goal_id);
    assert_eq!(resumed.objective, original.objective);
    assert_eq!(resumed.token_budget, original.token_budget);
    assert_eq!(resumed.tokens_used, charged.tokens_used);
    assert_eq!(resumed.status, ThreadGoalStatus::Active);
    assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
    finish(&h).await;
}

#[test]
fn a_fresh_native_block_after_host_reactivation_establishes_new_live_proof() {
    native_test(|| async {
        tokio::time::timeout(HANG, fresh_block())
            .await
            .expect("fresh native model-source control hang detector");
    });
}

async fn fresh_block() {
    let (h, goals) = harness().await;
    let evaluation = h.rt.hold_eval_when_free().await;
    let original = seed(&h, &goals).await;
    model_stop(&h, "blocked").await;
    let first = h
        .autopilot
        .current_model_goal(&h.rt)
        .await
        .expect("read the first native source")
        .expect("the original block supplied proof");
    host_status(
        &h,
        &goals,
        codex_protocol::protocol::ThreadGoalStatus::Paused,
    )
    .await;
    assert!(
        h.autopilot
            .current_model_goal(&h.rt)
            .await
            .expect("a native paused row is never resumable")
            .is_none()
    );
    mount_model_stop(&h, "blocked").await;
    host_status(
        &h,
        &goals,
        codex_protocol::protocol::ThreadGoalStatus::Active,
    )
    .await;
    completed(&h).await;
    let source = h
        .autopilot
        .current_model_goal(&h.rt)
        .await
        .expect("read the fresh native source")
        .expect("a new Active-to-Blocked commit establishes new proof");
    assert_ne!(source.turn, first.turn);
    assert_eq!(source.goal_id, original.goal_id);
    assert_eq!(source.objective, original.objective);
    assert_eq!(source.token_budget, original.token_budget);
    let blocked = goal(&h).await;
    let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
    drop(evaluation);
    due(&h).await;
    evaluate(&h).await;
    wait_requests(&h, 5).await;
    let resumed = goal(&h).await;
    assert_eq!(resumed.goal_id, original.goal_id);
    assert_eq!(resumed.objective, original.objective);
    assert_eq!(resumed.token_budget, original.token_budget);
    assert_eq!(resumed.tokens_used, blocked.tokens_used);
    assert_eq!(resumed.status, ThreadGoalStatus::Active);
    assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
    finish(&h).await;
}

#[test]
fn a_host_round_trip_during_a_post_tool_hook_revokes_the_delayed_commit_marker() {
    native_test(|| async {
        tokio::time::timeout(HANG, delayed_marker())
            .await
            .expect("delayed model marker mutation hang detector");
    });
}

async fn delayed_marker() {
    let (h, goals, outcomes) = super::hooks::hook_harness(HookEventName::PostToolUse).await;
    let evaluation = h.rt.hold_eval_when_free().await;
    seed(&h, &goals).await;
    super::hooks::start_goal_tool_until_hook(&h, "blocked", HookEventName::PostToolUse).await;
    assert_eq!(goal(&h).await.status, ThreadGoalStatus::Blocked);
    assert!(lock(&h.rt.model_blocked_goal).is_none());
    host_round_trip(&h, &goals).await;
    super::hooks::interrupt_hook(&h, &outcomes).await;
    assert!(
        h.autopilot
            .current_model_goal(&h.rt)
            .await
            .expect("recheck the marker after later native mutations")
            .is_none(),
        "a delayed tool finish cannot publish proof across later native API writes"
    );
    let stopped = goal(&h).await;
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
        1
    );
    finish(&h).await;
}

#[test]
fn a_late_source_cannot_acquire_an_epoch_after_unseen_native_api_writes() {
    native_test(|| async {
        tokio::time::timeout(HANG, stale_import())
            .await
            .expect("late native source mutation hang detector");
    });
}

async fn stale_import() {
    let (old, goals) = harness().await;
    let old_rt = Arc::clone(&old.rt);
    let old_evaluation = old_rt.hold_eval_when_free().await;
    seed(&old, &goals).await;
    model_stop(&old, "blocked").await;
    let source = super::source::saved_source(&old);
    let h = super::source::fresh_image(old, goals).await;
    drop(old_evaluation);
    let resumed_rt = Arc::clone(&h.rt);
    let evaluation = resumed_rt.hold_eval_when_free().await;
    let history = resumed_history(&h.test.codex)
        .await
        .expect("read the genuine source history before the native writes");
    let goals = h.autopilot.goals.as_ref().expect("native goal service");
    host_round_trip(&h, goals).await;
    h.shared.carry_model_goal(h.rt.key(), Some(source));
    h.autopilot.stage_model_goal_stop(&h.rt);
    // A read can finish with an older snapshot after native writes have
    // settled. Publish that real snapshot without fabricating any events.
    h.autopilot.seed_model_goal_stop(&h.rt, &history);
    assert!(
        h.autopilot
            .current_model_goal(&h.rt)
            .await
            .expect("recheck late source custody")
            .is_none(),
        "late metadata cannot borrow the current epoch after unobserved API writes"
    );
    let stopped = goal(&h).await;
    let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
    drop(evaluation);
    wait_for_supervisor(|| h.rt.scanned.load(Ordering::SeqCst)).await;
    due(&h).await;
    evaluate(&h).await;
    assert_eq!(goal(&h).await, stopped);
    assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
    assert!(
        responses::received_responses_requests(&h._server)
            .await
            .is_empty()
    );
    finish(&h).await;
}

#[test]
fn a_lookup_during_a_native_duplicate_write_preserves_its_original_custody() {
    native_test(|| async {
        tokio::time::timeout(HANG, writing_duplicate())
            .await
            .expect("in-flight native duplicate custody hang detector");
    });
}

async fn writing_duplicate() {
    let (h, goals) = harness().await;
    let evaluation = h.rt.hold_eval_when_free().await;
    seed(&h, &goals).await;
    model_stop(&h, "blocked").await;
    let source = h
        .autopilot
        .current_model_goal(&h.rt)
        .await
        .expect("read the original model source")
        .expect("the original native block is bound");
    let epoch = goals
        .alab_model_goal_epoch(h.rt.id)
        .expect("the actual native mutation owner");
    let db = h.test.codex.state_db().expect("native goal database");
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(db.sqlite().goals_db_path());
    #[expect(
        clippy::disallowed_methods,
        reason = "an owned scratch transaction holds the real native duplicate SQL write"
    )]
    let mut writer = sqlx::SqliteConnection::connect_with(&options)
        .await
        .expect("open the owned scratch writer");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut writer)
        .await
        .expect("hold the native goal write");
    responses::mount_sse_sequence(
        &h._server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("queued-duplicate"),
                responses::ev_function_call(
                    "queued-duplicate-call",
                    codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
                    r#"{"status":"blocked"}"#,
                ),
                // No new token delta obscures the status mutation with a
                // separate accounting write before the handler reaches SQL.
                responses::ev_completed("queued-duplicate"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("queued-duplicate-final"),
                responses::ev_assistant_message(
                    "queued-duplicate-message",
                    "The same obstacle remains.",
                ),
                responses::ev_completed("queued-duplicate-final"),
            ]),
        ],
    )
    .await;
    h.test
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Report whether the original goal is still blocked.".into(),
            text_elements: Vec::new(),
        }]))
        .await
        .expect("start the actual native duplicate");
    while goals.alab_model_goal_epoch(h.rt.id).as_ref() == Some(&epoch) {
        tokio::task::yield_now().await;
    }
    let runtime = h
        .test
        .codex
        .thread_extension_data()
        .get::<codex_goal_extension::GoalRuntimeHandle>()
        .expect("native goal runtime");
    assert!(runtime.alab_goal_persistence_held());
    let during = h.autopilot.current_model_goal(&h.rt).await;
    assert!(
        !matches!(during, Ok(Some(_))),
        "an unacknowledged status mutation cannot authorize recovery"
    );
    sqlx::query("ROLLBACK")
        .execute(&mut writer)
        .await
        .expect("release the actual native duplicate write");
    writer.close().await.expect("close the scratch writer");
    completed(&h).await;
    assert_eq!(
        h.autopilot
            .current_model_goal(&h.rt)
            .await
            .expect("read custody after the real duplicate commits"),
        Some(source),
        "a passive lookup cannot erase the original source of a valid duplicate"
    );
    let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
    drop(evaluation);
    due(&h).await;
    evaluate(&h).await;
    wait_requests(&h, 5).await;
    assert_eq!(goal(&h).await.status, ThreadGoalStatus::Active);
    assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
    finish(&h).await;
}

#[test]
fn a_replaced_native_runtime_owner_cannot_retoken_old_live_model_proof() {
    native_test(|| async {
        tokio::time::timeout(HANG, replaced_native_owner())
            .await
            .expect("native owner replacement custody hang detector");
    });
}

async fn replaced_native_owner() {
    let (h, goals) = harness().await;
    let evaluation = h.rt.hold_eval_when_free().await;
    seed(&h, &goals).await;
    model_stop(&h, "blocked").await;
    let before = goals
        .alab_model_goal_epoch(h.rt.id)
        .expect("the original native owner");
    let mut builder = codex_extension_api::ExtensionRegistryBuilder::<Config>::new();
    codex_goal_extension::install_with_backend(
        &mut builder,
        h.test.codex.state_db().expect("native goal database"),
        codex_analytics::AnalyticsEventsClient::disabled(),
        None,
        Arc::downgrade(&h.test.thread_manager),
        Arc::clone(&goals),
        |config: &Config| codex_goal_extension::GoalExtensionConfig {
            enabled: true,
            max_goal_token_budget: config.max_goal_token_budget,
        },
    );
    let registry = builder.build();
    let session = ExtensionData::new("replacement-native-session");
    let replacement = ExtensionData::new(h.rt.id.to_string());
    for contributor in registry.thread_lifecycle_contributors() {
        contributor
            .on_thread_start(ThreadStartInput {
                config: &h.test.config,
                session_source: &codex_protocol::protocol::SessionSource::Exec,
                persistent_thread_state_available: true,
                environments: &[],
                mcp_resource_client: None,
                extension_metrics: None,
                session_store: &session,
                thread_store: &replacement,
            })
            .await;
    }
    let after = goals
        .alab_model_goal_epoch(h.rt.id)
        .expect("the actual replacement native owner is registered");
    assert_ne!(
        after, before,
        "actual native registration installed a distinct mutation owner"
    );
    assert!(
        h.autopilot
            .current_model_goal(&h.rt)
            .await
            .expect("validate the original proof against the registered owner")
            .is_none(),
        "an old live binding cannot borrow a replacement owner's epoch"
    );
    let stopped = goal(&h).await;
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
        2
    );
    finish(&h).await;
}

struct RestoreEnhanced;

impl Drop for RestoreEnhanced {
    fn drop(&mut self) {
        codex_alab_core::set_enhanced(true);
    }
}

#[derive(Clone, Copy, Debug)]
enum VanillaMutation {
    HostApi,
    ModelTools,
}

async fn vanilla_mutation(change: VanillaMutation) {
    let (h, goals) = harness().await;
    let evaluation = h.rt.hold_eval_when_free().await;
    seed(&h, &goals).await;
    model_stop(&h, "blocked").await;
    assert!(
        h.autopilot
            .current_model_goal(&h.rt)
            .await
            .expect("read the original Enhanced proof")
            .is_some()
    );
    // native_test isolates this exact test in its own process. Restore mode
    // during unwinding too, before any native fixture teardown runs.
    let restore = RestoreEnhanced;
    codex_alab_core::set_enhanced(false);
    assert!(!codex_alab_core::active());
    match change {
        VanillaMutation::HostApi => host_round_trip(&h, &goals).await,
        VanillaMutation::ModelTools => {
            mount_goal_calls(
                &h,
                vec![
                    (
                        codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
                        serde_json::json!({"status": "paused"}),
                    ),
                    (
                        codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
                        serde_json::json!({"status": "blocked"}),
                    ),
                ],
            )
            .await;
            start_and_complete(&h).await;
            assert_eq!(goal(&h).await.status, ThreadGoalStatus::Blocked);
        }
    }
    drop(restore);
    assert!(codex_alab_core::active());
    assert!(
        h.autopilot
            .current_model_goal(&h.rt)
            .await
            .expect("recheck old proof after returning to Enhanced")
            .is_none(),
        "a mutation while Vanilla cannot restore an earlier Enhanced binding: {change:?}"
    );
    let stopped = goal(&h).await;
    let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
    let requests = responses::received_responses_requests(&h._server)
        .await
        .len();
    drop(evaluation);
    due(&h).await;
    evaluate(&h).await;
    assert_eq!(goal(&h).await, stopped);
    assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
    assert_eq!(
        responses::received_responses_requests(&h._server)
            .await
            .len(),
        requests
    );
    finish(&h).await;
}

#[test]
fn native_api_mutations_while_vanilla_revoke_earlier_enhanced_model_proof() {
    native_test(|| async {
        tokio::time::timeout(HANG, vanilla_mutation(VanillaMutation::HostApi))
            .await
            .expect("Vanilla native API mutation hang detector");
    });
}

#[test]
fn real_model_goal_tools_while_vanilla_revoke_earlier_enhanced_model_proof() {
    native_test(|| async {
        tokio::time::timeout(HANG, vanilla_mutation(VanillaMutation::ModelTools))
            .await
            .expect("Vanilla native model mutation hang detector");
    });
}

async fn vanilla_instruction(cancel: bool) {
    let (h, goals) = harness().await;
    let evaluation = h.rt.hold_eval_when_free().await;
    seed(&h, &goals).await;
    model_stop(&h, "blocked").await;
    let source = h
        .autopilot
        .current_model_goal(&h.rt)
        .await
        .expect("read the original native model source")
        .expect("the real block supplied proof");
    let before = goal(&h).await;
    let restore = RestoreEnhanced;
    codex_alab_core::set_enhanced(false);
    let (recorded, recording) = tokio::sync::oneshot::channel();
    let service = Arc::clone(&goals);
    let thread = Arc::clone(&h.test.codex);
    let db = thread.state_db().expect("native goal database");
    let id = h.rt.id;
    let mutation = tokio::spawn(async move {
        service
            .set_thread_goal(
                &db,
                GoalSetRequest {
                    thread_id: id,
                    objective: GoalObjectiveUpdate::Keep,
                    status: Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
                    token_budget: GoalTokenBudgetUpdate::Keep,
                    max_goal_token_budget: None,
                },
                async move {
                    // This is the app-server's actual native recorder, before
                    // goal SQL. It retains user provenance even if the request
                    // fails or its instruction observer is later cancelled.
                    thread
                        .record_user_goal_update(codex_core::context::UserGoalUpdate::Set {
                            objective: None,
                            status: Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
                        })
                        .await
                        .map_err(|error| GoalServiceError::Internal(error.to_string()))?;
                    thread
                        .flush_rollout()
                        .await
                        .map_err(|error| GoalServiceError::Internal(error.to_string()))?;
                    recorded.send(()).expect("native goal instruction recorded");
                    if cancel {
                        std::future::pending::<Result<(), GoalServiceError>>().await
                    } else {
                        Err(GoalServiceError::Internal(
                            "owned fixture fails after native instruction persistence".into(),
                        ))
                    }
                },
            )
            .await
    });
    recording
        .await
        .expect("the actual native goal fragment precedes SQL");
    if cancel {
        mutation.abort();
        assert!(
            mutation
                .await
                .expect_err("cancel the pending request")
                .is_cancelled()
        );
    } else {
        assert!(matches!(
            mutation.await.expect("the failed request settles"),
            Err(GoalServiceError::Internal(_))
        ));
    }
    assert_eq!(goal(&h).await, before, "the request reached no goal SQL");
    assert_eq!(h.shared.person_goal_stops(h.rt.key()), 0);
    drop(restore);
    let history = resumed_history(&h.test.codex)
        .await
        .expect("read the actual persisted native user instruction");
    assert!(crate::resume_history::person_goal_stop_since(
        &history,
        &source.turn
    ));
    assert!(
        h.autopilot
            .current_model_goal(&h.rt)
            .await
            .expect("recheck proof after the unfinished Vanilla instruction")
            .is_none(),
        "an unsuccessful native instruction must revoke earlier Enhanced proof"
    );
    let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
    drop(evaluation);
    due(&h).await;
    evaluate(&h).await;
    assert_eq!(goal(&h).await, before);
    assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
    assert_eq!(
        responses::received_responses_requests(&h._server)
            .await
            .len(),
        2
    );
    finish(&h).await;
}

#[test]
fn a_failed_native_instruction_while_vanilla_revokes_earlier_model_proof() {
    native_test(|| async {
        tokio::time::timeout(HANG, vanilla_instruction(false))
            .await
            .expect("failed Vanilla goal instruction hang detector");
    });
}

#[test]
fn a_cancelled_native_instruction_while_vanilla_revokes_earlier_model_proof() {
    native_test(|| async {
        tokio::time::timeout(HANG, vanilla_instruction(true))
            .await
            .expect("cancelled Vanilla goal instruction hang detector");
    });
}

#[derive(Clone, Copy, Debug)]
enum PendingHistory {
    Valid,
    MissingOrigin,
    PersonStop,
}

async fn duplicate_before_known_history(case: PendingHistory) {
    let (mut h, goals) = harness().await;
    let old_rt = Arc::clone(&h.rt);
    let old_evaluation = old_rt.hold_eval_when_free().await;
    seed(&h, &goals).await;
    model_stop(&h, "blocked").await;
    let source = super::restart::next_image(&mut h).await;
    let current_rt = Arc::clone(&h.rt);
    let evaluation = current_rt.hold_eval_when_free().await;
    h.rt.resumed.store(true, Ordering::SeqCst);
    h.autopilot.seed_pace(&h.rt, None);
    h.rt.scanned.store(true, Ordering::SeqCst);
    h.autopilot
        .set_history_error("the replacement has not verified its source history");
    assert!(lock(&h.rt.model_blocked_goal).is_none());
    mount_model_stop(&h, "blocked").await;
    start_and_complete(&h).await;
    assert!(
        lock(&h.rt.model_blocked_goal).is_none(),
        "an early duplicate transfers pending custody but cannot skip history validation"
    );
    if matches!(case, PendingHistory::PersonStop) {
        h.test
            .codex
            .record_user_goal_update(codex_core::context::UserGoalUpdate::Set {
                objective: None,
                status: Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
            })
            .await
            .expect("persist the actual native person's goal instruction");
    }
    h.test
        .codex
        .flush_rollout()
        .await
        .expect("flush the real duplicate history");
    let mut history = resumed_history(&h.test.codex)
        .await
        .expect("read the actual native replacement history");
    if matches!(case, PendingHistory::MissingOrigin) {
        history.retain(|item| {
            !matches!(item, codex_history::RolloutItem::EventMsg(EventMsg::TurnStarted(started))
                if started.turn_id == source.turn)
        });
    }
    h.autopilot.set_history(history);
    let recover = matches!(case, PendingHistory::Valid);
    assert_eq!(
        h.autopilot
            .current_model_goal(&h.rt)
            .await
            .expect("validate the pending original source after the duplicate"),
        recover.then_some(source),
        "the early duplicate must retain the original history boundary: {case:?}"
    );
    let stopped = goal(&h).await;
    let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
    drop(old_evaluation);
    drop(evaluation);
    due(&h).await;
    evaluate(&h).await;
    if recover {
        wait_requests(&h, 5).await;
        assert_eq!(goal(&h).await.status, ThreadGoalStatus::Active);
        assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
    } else {
        assert_eq!(goal(&h).await, stopped);
        assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
        assert_eq!(
            responses::received_responses_requests(&h._server)
                .await
                .len(),
            4
        );
    }
    finish(&h).await;
}

#[test]
fn a_duplicate_before_replacement_history_validation_preserves_valid_pending_proof() {
    native_test(|| async {
        tokio::time::timeout(HANG, duplicate_before_known_history(PendingHistory::Valid))
            .await
            .expect("pending known duplicate positive control hang detector");
    });
}

#[test]
fn a_duplicate_before_replacement_history_validation_cannot_hide_missing_origin() {
    native_test(|| async {
        tokio::time::timeout(
            HANG,
            duplicate_before_known_history(PendingHistory::MissingOrigin),
        )
        .await
        .expect("pending known duplicate missing-origin hang detector");
    });
}

#[test]
fn a_duplicate_before_replacement_history_validation_cannot_cross_a_persons_stop() {
    native_test(|| async {
        tokio::time::timeout(
            HANG,
            duplicate_before_known_history(PendingHistory::PersonStop),
        )
        .await
        .expect("pending known duplicate person-stop hang detector");
    });
}
