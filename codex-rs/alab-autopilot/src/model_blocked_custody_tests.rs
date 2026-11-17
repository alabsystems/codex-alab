// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Retiring a stale reader's model stop cannot publish over its replacement.

use super::*;
use codex_alab_core::model_goal_stops::Custody;
use codex_alab_core::session::Shared;
use codex_alab_core::session::WallGoal;

fn synthetic_binding(goal: WallGoal) -> model_blocked_goal::ModelGoalBinding {
    // Keep this passive fixture's owner alive for the duration of the test
    // process; MutationEpoch deliberately holds only a weak owner reference.
    let state: &'static Arc<codex_alab_core::goal_persistence::State> = Box::leak(Box::new(
        Arc::new(codex_alab_core::goal_persistence::State::default()),
    ));
    model_blocked_goal::ModelGoalBinding {
        goal,
        epoch: state.mutation_epoch().expect("fresh synthetic epoch"),
        mode_epoch: codex_alab_core::mode_epoch().expect("live mode epoch"),
    }
}

fn binding(goal: &str, turn: &str) -> model_blocked_goal::ModelGoalBinding {
    synthetic_binding(WallGoal {
        goal_id: goal.into(),
        objective: OBJECTIVE.into(),
        token_budget: Some(1_000),
        chronology_fence_generation: None,
        turn: turn.into(),
        person_stops: 0,
    })
}

/// This is the actual rejected native-resume path, suspended after its last
/// authority snapshot. A replacement or a newer binding must own publication
/// even when the older evaluator has already passed that snapshot.
#[test]
fn a_rejected_model_resume_cannot_erase_a_newer_binding_after_its_last_check() {
    native_test(|| rejected_resume_custody(false));
}

#[test]
fn a_rejected_model_resume_cannot_publish_over_a_replacement_after_its_last_check() {
    native_test(|| rejected_resume_custody(true));
}

async fn rejected_resume_custody(replacement: bool) {
    tokio::time::timeout(HANG, async {
        let (h, goals) = harness().await;
        let evaluation = h.rt.hold_eval_when_free().await;
        let original = seed(&h, &goals).await;
        model_stop(&h, "blocked").await;
        due(&h).await;
        let (preparing, prepared) = tokio::sync::oneshot::channel();
        let (allow_prepare, preparation) = tokio::sync::oneshot::channel();
        *lock(&h.rt.before_submit) = Some((preparing, preparation));
        let (checking, checked) = tokio::sync::oneshot::channel();
        let (allow_cleanup, cleanup) = tokio::sync::oneshot::channel();
        *lock(&h.rt.before_changed_goal_recheck) = Some((checking, cleanup));
        let pilot = Arc::clone(&h.autopilot);
        let old = Arc::clone(&h.rt);
        drop(evaluation);
        let evaluating = tokio::spawn(async move { pilot.evaluate(old).await });
        prepared
            .await
            .expect("the original native resume was prepared");
        let changed = h
            .test
            .codex
            .state_db()
            .expect("native goal database")
            .thread_goals()
            .update_thread_goal(
                h.rt.id,
                codex_state::GoalUpdate {
                    objective: Some("the updated original objective".to_owned()),
                    status: Some(ThreadGoalStatus::Blocked),
                    token_budget: None,
                    expected_goal_id: Some(original.goal_id),
                },
            )
            .await
            .expect("a goal edit rejects the prepared resume")
            .expect("the original goal remains");
        allow_prepare.send(()).expect("release native preparation");
        checked
            .await
            .expect("the rejected resume passed its final authority snapshot");

        let current = if replacement {
            let next = Arc::new(ThreadRt::new(
                h.rt.id,
                h.rt.config.clone(),
                h.rt.token_budget,
                Duration::ZERO,
            ));
            // Native registration retires the old instance while holding
            // the same gate that a durable publication must retain.
            h.rt.stopped.store(true, Ordering::SeqCst);
            h.test
                .codex
                .thread_extension_data()
                .insert_arc(Arc::clone(&next));
            h.autopilot.keep_thread(&next);
            next
        } else {
            Arc::clone(&h.rt)
        };
        let newer = model_blocked_goal::ModelGoalBinding {
            goal: WallGoal {
                goal_id: changed.goal_id,
                objective: changed.objective,
                token_budget: changed.token_budget,
                chronology_fence_generation: None,
                turn: "the-newer-native-model-stop".to_owned(),
                person_stops: h.shared.person_goal_stops(current.key()),
            },
            epoch: goals
                .alab_model_goal_epoch(current.id)
                .expect("the newer stop belongs to the registered native owner"),
            mode_epoch: codex_alab_core::mode_epoch().expect("live mode epoch"),
        };
        *lock(&current.model_blocked_goal) = Some(newer.clone());
        h.shared
            .set_model_goal_custody(current.key(), Custody::Known(newer.passive()));
        current.with_state(|state| state.short_streak = 7);
        h.autopilot.sync_pace(&current);
        let kept = h
            .shared
            .pace(current.key())
            .expect("new custody is durable");
        // Keep any independently queued evaluation from starting another
        // native turn while this test checks the older publication.
        let rollout = h.test.codex.rollout_path();
        h.shared.observe(
            &codex_alab_core::session::Frame {
                primary_thread: Some(current.key()),
                displayed_thread: Some(current.key()),
                rollout_path: rollout.as_deref(),
                cwd: h.test.cwd_path(),
                model: "gpt-5.5",
                codex_home: h.test.codex_home_path(),
                embedded: true,
                state: codex_alab_core::session::ProgramState::Idle,
                draft_empty: false,
                queued_input: false,
                popup_open: false,
                editor_open: false,
                pending_user_turn: false,
            },
            now_ms(),
        );
        allow_cleanup
            .send(())
            .expect("release the old rejection cleanup");
        evaluating.await.expect("the old evaluator settles");
        assert_eq!(
            lock(&current.model_blocked_goal).clone(),
            Some(newer.clone()),
            "replacement={replacement}: an older rejection erased the current binding"
        );
        let after = h
            .shared
            .pace(current.key())
            .expect("current custody remains");
        assert_eq!(
            after.model_blocked_goal,
            Some(newer.goal.clone()),
            "replacement={replacement}: an older rejection erased durable custody"
        );
        if replacement {
            assert_eq!(after, kept, "the detached runtime may publish nothing");
            assert!(
                !h.rt.timer_pending(),
                "a detached decision may arm no retry"
            );
        }
        Autopilot::cancel_timer(&current).await;
        finish(&h).await;
    })
    .await
    .unwrap_or_else(|error| {
        panic!("replacement={replacement}: rejection custody hang detector: {error}")
    });
}

#[tokio::test]
async fn a_replaced_runtime_cannot_retire_the_current_runtimes_durable_model_stop() {
    for old_marked_stopped in [false, true] {
        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        let sup = Supervisor::new(crate::SupervisorOptions {
            shared,
            endpoint: Arc::new(|| None),
            client: codex_alab_aterm::Options::default(),
        });
        let autopilot = Autopilot::new(sup, None, None, Timing::default(), |_| true);
        let id = ThreadId::new();
        let old = Arc::new(ThreadRt::new(id, None, false, Duration::ZERO));
        let current = Arc::new(ThreadRt::new(id, None, false, Duration::ZERO));
        let old_bound = binding("old-goal", "old-model-block");
        let current_bound = binding("replacement-goal", "replacement-model-block");
        *lock(&old.model_blocked_goal) = Some(old_bound.clone());
        *lock(&current.model_blocked_goal) = Some(current_bound.clone());
        shared.set_model_goal_custody(current.key(), Custody::Known(current_bound.passive()));
        old.with_state(|state| state.short_streak = 3);
        current.with_state(|state| state.short_streak = 7);
        autopilot.keep_thread(&old);
        autopilot.keep_thread(&current);
        old.stopped.store(old_marked_stopped, Ordering::SeqCst);
        assert!(Arc::ptr_eq(
            &autopilot
                .thread_rt(&id.to_string())
                .expect("current registration"),
            &current,
        ));
        autopilot.sync_pace(&current);
        let kept = shared
            .pace(current.key())
            .expect("replacement already published its own durable binding");
        assert_eq!(kept.model_blocked_goal, Some(current_bound.goal.clone()));

        // Every pace writer, including an older evaluator's hour or act
        // bookkeeping, must retain the registration through publication.
        autopilot.sync_pace(&old);
        assert_eq!(
            shared.pace(current.key()),
            Some(kept.clone()),
            "old pace writer clobbered its replacement: old_stopped={old_marked_stopped}"
        );

        // A goal read that began on the old registration may finish after
        // replacement. Clearing that old observation must publish nothing under
        // the shared thread key now owned by the replacement.
        autopilot.retire_model_goal_stop(&old, &old_bound);
        assert_eq!(
            shared.pace(current.key()),
            Some(kept.clone()),
            "old runtime rewrote its replacement's pace: old_stopped={old_marked_stopped}"
        );
        assert_eq!(
            lock(&current.model_blocked_goal).clone(),
            Some(current_bound.clone()),
        );

        // Neither a stale stop in the current runtime nor a legitimate current
        // retirement may erase the rest of the current registration's pace.
        autopilot.retire_model_goal_stop(&current, &old_bound);
        assert_eq!(shared.pace(current.key()), Some(kept.clone()));
        autopilot.retire_model_goal_stop(&current, &current_bound);
        let mut retired = kept;
        retired.model_blocked_goal = None;
        assert_eq!(shared.pace(current.key()), Some(retired));
        assert!(lock(&current.model_blocked_goal).is_none());
    }
}

#[tokio::test]
async fn a_replaced_runtimes_late_tool_completion_cannot_publish_over_its_replacement() {
    codex_alab_core::activate();
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let sup = Supervisor::new(crate::SupervisorOptions {
        shared,
        endpoint: Arc::new(|| None),
        client: codex_alab_aterm::Options::default(),
    });
    let autopilot = Autopilot::new(sup, None, None, Timing::default(), |_| true);
    let id = ThreadId::new();
    let old = Arc::new(ThreadRt::new(id, None, false, Duration::ZERO));
    let current = Arc::new(ThreadRt::new(id, None, false, Duration::ZERO));
    let old_bound = binding("old-goal", "late-tool-turn");
    let current_bound = binding("replacement-goal", "replacement-model-block");
    *lock(&old.model_blocked_goal) = Some(old_bound.clone());
    *lock(&current.model_blocked_goal) = Some(current_bound.clone());
    shared.set_model_goal_custody(current.key(), Custody::Known(current_bound.passive()));
    old.with_state(|state| state.short_streak = 3);
    current.with_state(|state| state.short_streak = 7);
    autopilot.keep_thread(&old);
    autopilot.keep_thread(&current);
    old.stopped.store(true, Ordering::SeqCst);
    autopilot.sync_pace(&current);
    let kept = shared
        .pace(current.key())
        .expect("replacement has durable model-stop custody");
    let session = ExtensionData::new("late-tool-host");
    let thread = ExtensionData::new(id.to_string());
    thread.insert_arc(Arc::clone(&old));
    let turn = ExtensionData::new("late-tool-turn");
    turn.insert(TurnTrack(Mutex::new(TrackData::default())));
    let call = "late-tool-call";
    let name = codex_extension_api::ToolName::plain(codex_goal_extension::UPDATE_GOAL_TOOL_NAME)
        .with_default_namespace();
    codex_alab_core::model_goal_invocation::Invocation::new()
        .scope(async {
            codex_alab_core::model_goal_stops::committed(
                thread.level_id(),
                turn.level_id(),
                call,
                codex_alab_core::model_goal_stops::CommittedMutation {
                    transition: Arc::new(codex_alab_core::goal_persistence::State::default())
                        .begin_mutation()
                        .commit_mutation()
                        .expect("synthetic committed transition"),
                    stop: Some(codex_alab_core::model_goal_stops::CommittedStop {
                        goal: old_bound.goal,
                        previous_status: codex_alab_core::model_goal_stops::PreviousStatus::Active,
                    }),
                    mode_epoch: codex_alab_core::mode_epoch().expect("enhanced mode epoch"),
                },
            );

            autopilot
                .model_goal_tool_finish(ToolFinishInput {
                    session_store: &session,
                    thread_store: &thread,
                    turn_store: &turn,
                    turn_id: turn.level_id(),
                    call_id: call,
                    tool_name: &name,
                    source: codex_extension_api::ToolCallSource::Direct,
                    outcome: ToolCallOutcome::Completed { success: true },
                })
                .await;
            assert_eq!(
                shared.pace(current.key()),
                Some(kept),
                "an old tool observer must not publish under a replacement's thread key"
            );
            assert_eq!(
                lock(&current.model_blocked_goal).clone(),
                Some(current_bound)
            );
            assert!(
                codex_alab_core::model_goal_stops::take(thread.level_id(), turn.level_id(), call)
                    .is_none(),
                "the old callback still consumes its own source fact"
            );
        })
        .await;
}
