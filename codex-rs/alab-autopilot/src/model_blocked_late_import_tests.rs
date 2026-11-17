// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Early duplicate blocks keep late source evidence available, without creating authority.

use super::*;
use codex_alab_core::session::Pace;
use codex_alab_core::session::WallGoal;

async fn host_pause(h: &Harness, goals: &GoalService) {
    super::source::host_set(
        h,
        goals,
        GoalObjectiveUpdate::Keep,
        codex_protocol::protocol::ThreadGoalStatus::Paused,
    )
    .await;
    assert_eq!(goal(h).await.status, ThreadGoalStatus::Paused);
}

#[derive(Clone, Copy, Debug)]
enum EarlierState {
    Blocked,
    Paused,
    PausedThenBlocked,
}

async fn resumed_image(
    earlier: EarlierState,
) -> (Harness, Arc<GoalService>, WallGoal, codex_state::ThreadGoal) {
    let goals = Arc::new(GoalService::new());
    let old = Harness::new_with_accounted_goals(Arc::clone(&goals)).await;
    let (old, goals) = configured_harness(old, goals).await;
    let old_rt = Arc::clone(&old.rt);
    let evaluation = old_rt.hold_eval_when_free().await;
    let original = seed(&old, &goals).await;
    model_stop(&old, "blocked").await;
    let recorded = serde_json::to_value(codex_alab_tui::record::Pace::from(
        old.shared
            .pace(old.rt.key())
            .expect("original model stop pace"),
    ))
    .expect("encode the saved source");
    let recorded: codex_alab_tui::record::Pace =
        serde_json::from_value(recorded).expect("decode the saved source");
    let saved = Pace::from(recorded)
        .model_blocked_goal
        .expect("the source contains native model-stop proof");
    assert_eq!(saved.person_stops, 0);
    match earlier {
        EarlierState::Blocked => {}
        EarlierState::Paused => {
            mount_model_stop(&old, "paused").await;
            start_and_complete(&old).await;
            assert_eq!(goal(&old).await.status, ThreadGoalStatus::Paused);
        }
        EarlierState::PausedThenBlocked => host_pause(&old, &goals).await,
    }
    if matches!(earlier, EarlierState::PausedThenBlocked) {
        mount_model_stop(&old, "blocked").await;
        start_and_complete(&old).await;
        assert_eq!(goal(&old).await.status, ThreadGoalStatus::Blocked);
    }
    let old_shared = old.shared;
    let home = Arc::clone(&old.test.home);
    let rollout = old.test.codex.rollout_path().expect("native rollout");
    Autopilot::cancel_timer(&old.rt).await;
    old.test
        .codex
        .shutdown_and_wait()
        .await
        .expect("stop previous native image");
    drop(evaluation);
    drop(old);
    drop(goals);

    let goals = Arc::new(GoalService::new());
    let h = Harness::resume_with_accounted_goals(Arc::clone(&goals), home, rollout).await;
    let (h, goals) = configured_harness(h, goals).await;
    assert!(
        !std::ptr::eq(h.shared, old_shared),
        "a fresh process has fresh import custody"
    );
    assert_eq!(goal(&h).await.goal_id, original.goal_id);
    assert_eq!(goal(&h).await.thread_id, original.thread_id);
    assert!(
        h.rt.resumed.load(Ordering::SeqCst),
        "Core actually resumed this thread"
    );
    assert!(h.shared.carried_model_goal(h.rt.key()).is_none());
    assert!(lock(&h.rt.model_blocked_goal).is_none());
    if matches!(earlier, EarlierState::Paused) {
        let history = resumed_history(&h.test.codex)
            .await
            .expect("read the actual model-pause history");
        assert!(
            !crate::resume_history::model_goal_stop_revoked_since(&history, h.rt.id, &saved),
            "the notification-only model pause must exercise prior-status retirement independently of the history veto"
        );
    }
    (h, goals, saved, original)
}

async fn duplicate_block(h: &Harness) {
    mount_model_stop(h, "blocked").await;
    start_and_complete(h).await;
    h.test
        .codex
        .flush_rollout()
        .await
        .expect("flush the early duplicate");
    assert_eq!(goal(h).await.status, ThreadGoalStatus::Blocked);
    assert!(
        lock(&h.rt.model_blocked_goal).is_none(),
        "an already blocked row and duplicate tool call cannot create authority"
    );
    assert!(
        h.shared
            .pace(h.rt.key())
            .is_none_or(|pace| pace.model_blocked_goal.is_none())
    );
}

async fn scan_finished(h: &Harness) {
    wait_for_supervisor(|| h.rt.scanned.load(Ordering::SeqCst)).await;
}

#[test]
fn a_durable_host_mutation_fence_retires_older_restart_proof() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, _goals, saved, _original) = resumed_image(EarlierState::Blocked).await;
            codex_alab_core::goal_chronology_fence::arm(
                h.test.codex_home_path(),
                &h.rt.id.to_string(),
            )
            .expect("arm unresolved host chronology");
            h.shared.carry_model_goal(h.rt.key(), Some(saved));

            assert!(
                h.autopilot
                    .current_model_goal(&h.rt)
                    .await
                    .expect("fenced import is a settled refusal")
                    .is_none()
            );
            assert!(h.shared.carried_model_goal(h.rt.key()).is_none());
            codex_alab_core::goal_chronology_fence::clear(
                h.test.codex_home_path(),
                &h.rt.id.to_string(),
            )
            .expect("clear test fence");
            finish(&h).await;
        })
        .await
        .expect("durable host-mutation fence hang detector");
    });
}

#[test]
fn an_early_duplicate_model_block_keeps_late_native_restart_proof_available() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, _goals, saved, original) = resumed_image(EarlierState::Blocked).await;
            let evaluation = h.rt.hold_eval_when_free().await;
            duplicate_block(&h).await;
            let before = goal(&h).await;
            let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);

            // The writer's selected source arrives only after the native tool's
            // completion. It carries evidence, never an old automatic prompt.
            h.shared.carry_model_goal(h.rt.key(), Some(saved.clone()));
            assert_eq!(
                h.shared.carried_model_goal(h.rt.key()),
                Some(saved.clone()),
                "a duplicate block must not retire a source it has not seen"
            );
            assert_eq!(
                h.autopilot
                    .current_model_goal(&h.rt)
                    .await
                    .expect("validate full native history")
                    .map(|bound| bound.goal),
                Some(saved)
            );
            assert_eq!(goal(&h).await, before, "proof adoption alone reactivates nothing");
            drop(evaluation);
            scan_finished(&h).await;
            due(&h).await;
            evaluate(&h).await;
            wait_requests(&h, 3).await;
            let resumed = goal(&h).await;
            assert_eq!(resumed.goal_id, original.goal_id);
            assert_eq!(resumed.objective, original.objective);
            assert_eq!(resumed.token_budget, original.token_budget);
            assert_eq!(resumed.tokens_used, before.tokens_used);
            assert_eq!(resumed.status, ThreadGoalStatus::Active);
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
            let started = core_test_support::wait_for_event_with_timeout(
                &h.test.codex,
                |event| matches!(event, EventMsg::TurnStarted(_)),
                HANG,
            ).await;
            assert_eq!(
                serde_json::to_value(started).expect("native attribution")["turn_attribution"]["turn_trigger"],
                "goal"
            );
            let requests = responses::received_responses_requests(&h._server).await;
            assert_eq!(requests.len(), 3);
            assert!(requests[2].body_contains_text(words::MODEL_BLOCKED_GOAL_CONTINUE));
            finish(&h).await;
        }).await.expect("late duplicate model-stop proof hang detector");
    });
}

#[derive(Clone, Copy, Debug)]
enum StoppedCase {
    NoProof,
    PausedBeforeDuplicate,
    PersonStopped,
    HistoricalPause,
    PauseClosedImport,
}

async fn stopped(case: StoppedCase) {
    tokio::time::timeout(HANG, async {
        let earlier = match case {
            StoppedCase::PausedBeforeDuplicate => EarlierState::Paused,
            StoppedCase::HistoricalPause => EarlierState::PausedThenBlocked,
            _ => EarlierState::Blocked,
        };
        let (h, goals, saved, original) = resumed_image(earlier).await;
        let evaluation = h.rt.hold_eval_when_free().await;
        if matches!(case, StoppedCase::PauseClosedImport) {
            mount_model_stop(&h, "paused").await;
            start_and_complete(&h).await;
            assert_eq!(goal(&h).await.status, ThreadGoalStatus::Paused);
        }
        duplicate_block(&h).await;
        if matches!(case, StoppedCase::PersonStopped) {
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
                .expect("persist the person's stop before the source arrives");
            outcome.apply_runtime_effects(&goals).await;
        }
        if !matches!(case, StoppedCase::NoProof) {
            h.shared.carry_model_goal(h.rt.key(), Some(saved));
        }
        assert_eq!(
            h.autopilot
                .current_model_goal(&h.rt)
                .await
                .expect("validate the late source"),
            None,
            "{case:?}: a late source cannot cross this stop boundary"
        );
        let before = goal(&h).await;
        assert_eq!(before.goal_id, original.goal_id);
        assert!(lock(&h.rt.model_blocked_goal).is_none());
        assert!(
            h.shared
                .pace(h.rt.key())
                .is_none_or(|pace| pace.model_blocked_goal.is_none())
        );
        let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
        let requests = responses::received_responses_requests(&h._server)
            .await
            .len();
        drop(evaluation);
        scan_finished(&h).await;
        due(&h).await;
        evaluate(&h).await;
        assert_eq!(goal(&h).await, before, "{case:?}: no native goal resume");
        assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
        assert_eq!(
            responses::received_responses_requests(&h._server)
                .await
                .len(),
            requests
        );
        finish(&h).await;
    })
    .await
    .unwrap_or_else(|error| panic!("{case:?}: late-source stop control hang detector: {error}"));
}

#[test]
fn an_early_duplicate_without_source_proof_remains_blocked() {
    native_test(|| stopped(StoppedCase::NoProof));
}

#[test]
fn a_paused_goal_cannot_gain_late_recovery_from_its_first_block() {
    native_test(|| stopped(StoppedCase::PausedBeforeDuplicate));
}

#[test]
fn a_persons_stop_before_the_late_source_arrives_remains_authoritative() {
    native_test(|| stopped(StoppedCase::PersonStopped));
}

#[test]
fn a_late_source_predating_a_host_api_pause_cannot_recover_after_a_duplicate_block() {
    native_test(|| stopped(StoppedCase::HistoricalPause));
}

#[test]
fn a_duplicate_block_cannot_reopen_an_import_closed_by_a_model_pause() {
    native_test(|| stopped(StoppedCase::PauseClosedImport));
}
