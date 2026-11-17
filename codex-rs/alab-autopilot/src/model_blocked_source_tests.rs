// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Duplicate blocks preserve their original source; uncertain history stays stopped.

use super::*;
use codex_alab_core::session::Pace;
use codex_alab_core::session::WallGoal;
use codex_history::RolloutItem;
use codex_protocol::models::ResponseItem;
use tokio::sync::Notify;

#[derive(Clone, Copy)]
enum GateAt {
    GetFinish,
    BlockFinish,
}

struct SourceGate {
    at: GateAt,
    entered: Notify,
    release: Notify,
    calls: Mutex<Vec<(String, String, String)>>,
    outcome: Mutex<Option<ToolCallOutcome>>,
}

impl SourceGate {
    fn new(at: GateAt) -> Self {
        Self {
            at,
            entered: Notify::new(),
            release: Notify::new(),
            calls: Mutex::new(Vec::new()),
            outcome: Mutex::new(None),
        }
    }

    fn source_call(&self) -> (String, String) {
        let calls = lock(&self.calls);
        assert_eq!(calls.len(), 2, "the native get precedes the native block");
        assert_eq!(calls[0].0, codex_goal_extension::GET_GOAL_TOOL_NAME);
        assert_eq!(calls[1].0, codex_goal_extension::UPDATE_GOAL_TOOL_NAME);
        assert_eq!(calls[0].2, calls[1].2, "both calls belong to one Core turn");
        (calls[1].1.clone(), calls[1].2.clone())
    }
}

impl ToolLifecycleContributor for SourceGate {
    fn on_tool_start<'a>(&'a self, input: ToolStartInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if !input.tool_name.is_default_namespace()
                || ![
                    codex_goal_extension::GET_GOAL_TOOL_NAME,
                    codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
                ]
                .contains(&input.tool_name.name.as_str())
            {
                return;
            }
            lock(&self.calls).push((
                input.tool_name.name.clone(),
                input.call_id.to_owned(),
                input.turn_id.to_owned(),
            ));
        })
    }

    fn on_tool_finish<'a>(&'a self, input: ToolFinishInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if input.tool_name.is_default_namespace()
                && input.tool_name.name == codex_goal_extension::GET_GOAL_TOOL_NAME
                && matches!(self.at, GateAt::GetFinish)
            {
                // The host mutation completes before the provider can issue
                // its next call. The read-only get retains no mutation permit.
                self.entered.notify_one();
                self.release.notified().await;
            }
            if input.tool_name.is_default_namespace()
                && input.tool_name.name == codex_goal_extension::UPDATE_GOAL_TOOL_NAME
            {
                *lock(&self.outcome) = Some(input.outcome);
                if matches!(self.at, GateAt::BlockFinish) {
                    // Autopilot's observer runs first, after the handler has
                    // dropped its permit and published the exact committed stop.
                    self.entered.notify_one();
                    self.release.notified().await;
                }
            }
        })
    }
}

pub(super) async fn host_set(
    h: &Harness,
    goals: &GoalService,
    objective: GoalObjectiveUpdate<'_>,
    status: codex_protocol::protocol::ThreadGoalStatus,
) {
    let db = h.test.codex.state_db().expect("native goal database");
    let outcome = goals
        .set_thread_goal(
            &db,
            GoalSetRequest {
                thread_id: h.rt.id,
                objective,
                status: Some(status),
                token_budget: GoalTokenBudgetUpdate::Keep,
                max_goal_token_budget: None,
            },
            async { Ok(()) },
        )
        .await
        .expect("persist the automatic host/API goal update");
    // Match ThreadGoalProcessor::set. Model emitter notifications are not
    // rollout events; these are the native host/API mutation outcomes.
    h.test
        .codex
        .append_rollout_items(&[outcome.thread_goal_updated_item()])
        .await
        .expect("append the actual host/API outcome");
    h.test
        .codex
        .flush_rollout()
        .await
        .expect("flush the ordered host/API outcome");
    db.thread_goals()
        .alab_durable_goal_barrier()
        .await
        .expect("make the post-rollout goal row durable");
    // Match the host: runtime effects follow durable rollout and the final
    // database barrier, never merely an accepted buffered append.
    outcome.apply_runtime_effects(goals).await;
}

fn recorded_source(h: &Harness) -> Option<WallGoal> {
    let recorded = serde_json::to_value(codex_alab_tui::record::Pace::from(
        h.shared.pace(h.rt.key()).expect("native source pace"),
    ))
    .expect("encode native source pace");
    let recorded: codex_alab_tui::record::Pace =
        serde_json::from_value(recorded).expect("decode native source pace");
    Pace::from(recorded).model_blocked_goal
}

pub(super) fn saved_source(h: &Harness) -> WallGoal {
    recorded_source(h).expect("the codec preserves the committed model stop")
}

fn durable_source(h: &Harness) -> Option<WallGoal> {
    let pid = h
        .shared
        .launch()
        .map_or_else(std::process::id, |launch| launch.pid);
    let path = codex_alab_tui::record::path_for(
        &codex_alab_tui::record::sessions_dir(h.test.codex_home_path()),
        pid,
    );
    codex_alab_tui::record::read(&path)
        .and_then(|record| record.pace)
        .map(Pace::from)
        .and_then(|pace| pace.model_blocked_goal)
}

pub(super) async fn fresh_image(old: Harness, goals: Arc<GoalService>) -> Harness {
    let old_shared = old.shared;
    let home = Arc::clone(&old.test.home);
    let rollout = old.test.codex.rollout_path().expect("native rollout");
    old.test
        .codex
        .flush_rollout()
        .await
        .expect("flush the previous native image");
    Autopilot::cancel_timer(&old.rt).await;
    old.test
        .codex
        .shutdown_and_wait()
        .await
        .expect("stop the previous native image");
    drop(old);
    drop(goals);

    let goals = Arc::new(GoalService::new());
    let h = Harness::resume_with_accounted_goals(Arc::clone(&goals), home, rollout).await;
    let (h, _goals) = configured_harness(h, goals).await;
    assert!(!std::ptr::eq(h.shared, old_shared));
    assert!(h.rt.resumed.load(Ordering::SeqCst));
    h
}

async fn verify_import(
    h: &Harness,
    saved: WallGoal,
    original: &codex_state::ThreadGoal,
    recover: bool,
) {
    let resumed_rt = Arc::clone(&h.rt);
    let evaluation = resumed_rt.hold_eval_when_free().await;
    h.shared.carry_model_goal(h.rt.key(), Some(saved.clone()));
    let imported = h
        .autopilot
        .current_model_goal(&h.rt)
        .await
        .expect("validate the native source after real restart");
    assert_eq!(
        imported.map(|bound| bound.goal),
        recover.then_some(saved),
        "restart must preserve the original source and honor conservative stop boundaries"
    );
    let before = goal(h).await;
    let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
    drop(evaluation);
    wait_for_supervisor(|| h.rt.scanned.load(Ordering::SeqCst)).await;
    due(h).await;
    evaluate(h).await;
    if recover {
        wait_requests(h, 1).await;
        let resumed = goal(h).await;
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
        )
        .await;
        assert_eq!(
            serde_json::to_value(started).expect("native goal attribution")["turn_attribution"]["turn_trigger"],
            "goal"
        );
        let requests = responses::received_responses_requests(&h._server).await;
        assert_eq!(requests.len(), 1);
        assert!(requests[0].body_contains_text(OBJECTIVE));
        assert!(requests[0].body_contains_text(words::MODEL_BLOCKED_GOAL_CONTINUE));
    } else {
        assert_eq!(goal(h).await, before);
        assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
        assert!(
            responses::received_responses_requests(&h._server)
                .await
                .is_empty()
        );
    }
}

#[derive(Clone, Copy, Debug)]
enum HostBoundary {
    PauseBefore,
    EditBefore,
    PauseAfter,
}

async fn source_scenario(case: HostBoundary) {
    let at = if matches!(case, HostBoundary::PauseAfter) {
        GateAt::BlockFinish
    } else {
        GateAt::GetFinish
    };
    let gate = Arc::new(SourceGate::new(at));
    let goals = Arc::new(GoalService::new());
    let old = Harness::new_with_accounted_goals_and_tool_observer(
        Arc::clone(&goals),
        Arc::clone(&gate) as Arc<dyn ToolLifecycleContributor>,
    )
    .await;
    let (old, goals) = configured_harness(old, goals).await;
    let old_rt = Arc::clone(&old.rt);
    let evaluation = old_rt.hold_eval_when_free().await;
    let original = seed(&old, &goals).await;
    mount_goal_calls(
        &old,
        vec![
            (
                codex_goal_extension::GET_GOAL_TOOL_NAME,
                serde_json::json!({}),
            ),
            (
                codex_goal_extension::UPDATE_GOAL_TOOL_NAME,
                serde_json::json!({"status": "blocked"}),
            ),
        ],
    )
    .await;
    old.test
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Check the original goal, then report the current obstacle.".into(),
            text_elements: Vec::new(),
        }]))
        .await
        .expect("start the same native turn");
    gate.entered.notified().await;
    assert!(old.test.codex.has_active_turn().await);
    let before_hold = matches!(case, HostBoundary::PauseAfter).then(|| saved_source(&old));
    let active = codex_protocol::protocol::ThreadGoalStatus::Active;
    match case {
        HostBoundary::PauseBefore => {
            host_set(
                &old,
                &goals,
                GoalObjectiveUpdate::Keep,
                codex_protocol::protocol::ThreadGoalStatus::Paused,
            )
            .await;
            host_set(&old, &goals, GoalObjectiveUpdate::Keep, active).await;
        }
        HostBoundary::EditBefore => {
            host_set(
                &old,
                &goals,
                GoalObjectiveUpdate::Set("Inspect the earlier host-selected compiler objective"),
                active,
            )
            .await;
            host_set(&old, &goals, GoalObjectiveUpdate::Set(OBJECTIVE), active).await;
        }
        HostBoundary::PauseAfter => {
            host_set(
                &old,
                &goals,
                GoalObjectiveUpdate::Keep,
                codex_protocol::protocol::ThreadGoalStatus::Paused,
            )
            .await;
            host_set(
                &old,
                &goals,
                GoalObjectiveUpdate::Keep,
                codex_protocol::protocol::ThreadGoalStatus::Blocked,
            )
            .await;
        }
    }
    assert_eq!(
        old.rt.accepted_turns.load(Ordering::SeqCst),
        1,
        "host updates do not open another turn"
    );
    assert_eq!(
        old.shared.person_goal_stops(old.rt.key()),
        0,
        "automatic API outcomes are not person-origin instructions"
    );
    gate.release.notify_one();
    completed(&old).await;
    let (source_call, source_turn) = gate.source_call();
    assert_eq!(
        *lock(&gate.outcome),
        Some(ToolCallOutcome::Completed { success: true })
    );
    let blocked = goal(&old).await;
    assert_eq!(blocked.goal_id, original.goal_id);
    assert_eq!(blocked.objective, original.objective);
    assert_eq!(blocked.token_budget, original.token_budget);
    assert_eq!(blocked.status, ThreadGoalStatus::Blocked);
    let saved = before_hold.unwrap_or_else(|| saved_source(&old));
    assert_eq!(saved.turn, source_turn);
    old.test
        .codex
        .flush_rollout()
        .await
        .expect("flush the native source turn");
    let history = resumed_history(&old.test.codex)
        .await
        .expect("read the actual ordered rollout");
    let call = history
        .iter()
        .position(|item| {
            let RolloutItem::ResponseItem(envelope) = item else {
                return false;
            };
            matches!(&envelope.item, ResponseItem::FunctionCall { call_id, .. } if call_id == &source_call)
        })
        .expect("the real blocked tool call is durable");
    let output = history
        .iter()
        .position(|item| {
            let RolloutItem::ResponseItem(envelope) = item else {
                return false;
            };
            matches!(&envelope.item, ResponseItem::FunctionCallOutput { call_id, .. } if call_id.as_deref() == Some(source_call.as_str()))
        })
        .expect("the real blocked tool output is durable");
    let boundary = history
        .iter()
        .position(|item| {
            let RolloutItem::EventMsg(EventMsg::ThreadGoalUpdated(update)) = item else {
                return false;
            };
            update.thread_id == old.rt.id
                && (update.goal.status == codex_protocol::protocol::ThreadGoalStatus::Paused
                    || update.goal.objective != OBJECTIVE)
        })
        .expect("the real host boundary is durable");
    if matches!(case, HostBoundary::PauseAfter) {
        assert!(
            call < boundary && boundary < output,
            "the host hold follows the actual committed source, before output delivery"
        );
    } else {
        assert!(
            boundary < call && call < output,
            "the fresh native block follows the earlier host boundary"
        );
    }
    let h = fresh_image(old, goals).await;
    drop(evaluation);
    // Without a durable, non-reusable native request receipt, the earlier
    // same-turn boundary cannot safely be distinguished from a later one.
    verify_import(&h, saved, &original, false).await;
    finish(&h).await;
}

async fn source_case(case: HostBoundary) {
    tokio::time::timeout(HANG, source_scenario(case))
        .await
        .unwrap_or_else(|error| panic!("{case:?}: same-turn model-source hang detector: {error}"));
}

#[test]
fn an_earlier_same_turn_host_pause_keeps_restart_import_conservatively_stopped() {
    native_test(|| source_case(HostBoundary::PauseBefore));
}

#[test]
fn an_earlier_same_turn_goal_edit_keeps_restart_import_conservatively_stopped() {
    native_test(|| source_case(HostBoundary::EditBefore));
}

#[test]
fn a_same_turn_host_hold_after_the_committed_model_block_defeats_restart_recovery() {
    native_test(|| source_case(HostBoundary::PauseAfter));
}

#[test]
fn a_live_host_round_trip_revokes_the_old_model_stop_without_a_restart() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let evaluation = h.rt.hold_eval_when_free().await;
            let original = seed(&h, &goals).await;
            model_stop(&h, "blocked").await;
            assert!(
                lock(&h.rt.model_blocked_goal).is_some(),
                "the real model block initially owns live recovery"
            );
            let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
            let requests = responses::received_responses_requests(&h._server)
                .await
                .len();

            host_set(
                &h,
                &goals,
                GoalObjectiveUpdate::Keep,
                codex_protocol::protocol::ThreadGoalStatus::Paused,
            )
            .await;
            host_set(
                &h,
                &goals,
                GoalObjectiveUpdate::Keep,
                codex_protocol::protocol::ThreadGoalStatus::Blocked,
            )
            .await;
            let stopped = goal(&h).await;
            assert_eq!(stopped.goal_id, original.goal_id);
            assert_eq!(stopped.objective, original.objective);
            assert_eq!(stopped.token_budget, original.token_budget);
            assert_eq!(stopped.status, ThreadGoalStatus::Blocked);
            assert_eq!(
                h.shared.person_goal_stops(h.rt.key()),
                0,
                "automatic host/API outcomes are not person-origin instructions"
            );

            assert_eq!(
                h.autopilot
                    .current_model_goal(&h.rt)
                    .await
                    .expect("read live model-goal standing"),
                None,
                "an acknowledged native mutation round-trip must fence old model proof"
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
                requests,
                "stale live proof must not open a provider request"
            );
            finish(&h).await;
        })
        .await
        .expect("live host round-trip model-stop fence hang detector");
    });
}

#[test]
fn a_settled_foreign_host_generation_revokes_identical_live_model_proof() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let evaluation = h.rt.hold_eval_when_free().await;
            let original = seed(&h, &goals).await;
            model_stop(&h, "blocked").await;
            let bound = lock(&h.rt.model_blocked_goal)
                .clone()
                .expect("the real model block initially owns live recovery");
            let runtime = goals.alab_goal_runtime(h.rt.id).unwrap();
            assert_eq!(
                runtime.alab_goal_mutation_epoch(),
                Some(bound.epoch.clone())
            );

            // Simulate another process completing a host round trip whose final
            // SQLite fields equal the old row. Its process-local epoch cannot
            // advance this runtime; only the settled durable serial witnesses it.
            let codex_home = h.test.codex_home_path();
            let foreign =
                codex_alab_core::goal_chronology_fence::arm(codex_home, h.rt.key()).unwrap();
            assert!(
                codex_alab_core::goal_chronology_fence::clear_if_current(
                    codex_home,
                    h.rt.key(),
                    foreign,
                )
                .unwrap()
            );
            assert_eq!(
                codex_alab_core::goal_chronology_fence::read_active(codex_home, h.rt.key())
                    .unwrap(),
                None,
            );
            assert_eq!(runtime.alab_goal_mutation_epoch(), Some(bound.epoch));
            let stopped = goal(&h).await;
            assert_eq!(stopped.goal_id, original.goal_id);
            assert_eq!(stopped.objective, original.objective);
            assert_eq!(stopped.token_budget, original.token_budget);
            assert_eq!(stopped.status, ThreadGoalStatus::Blocked);
            assert_eq!(
                h.autopilot
                    .current_model_goal(&h.rt)
                    .await
                    .expect("read live model-goal standing after foreign settlement"),
                None,
                "a settled foreign mutation serial permanently invalidates older proof"
            );
            drop(evaluation);
            finish(&h).await;
        })
        .await
        .expect("settled foreign model-stop chronology hang detector");
    });
}

#[test]
fn a_vanilla_host_round_trip_cannot_resurrect_an_old_model_stop() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let evaluation = h.rt.hold_eval_when_free().await;
            seed(&h, &goals).await;
            model_stop(&h, "blocked").await;
            assert!(lock(&h.rt.model_blocked_goal).is_some());

            codex_alab_core::set_enhanced(false);
            host_set(
                &h,
                &goals,
                GoalObjectiveUpdate::Keep,
                codex_protocol::protocol::ThreadGoalStatus::Paused,
            )
            .await;
            host_set(
                &h,
                &goals,
                GoalObjectiveUpdate::Keep,
                codex_protocol::protocol::ThreadGoalStatus::Blocked,
            )
            .await;
            codex_alab_core::set_enhanced(true);

            assert_eq!(
                h.autopilot
                    .current_model_goal(&h.rt)
                    .await
                    .expect("read standing after the vanilla mutation round trip"),
                None,
                "mode and native mutation epochs both fence the pre-vanilla source"
            );
            drop(evaluation);
            finish(&h).await;
        })
        .await
        .expect("vanilla host round-trip model-stop fence hang detector");
    });
}

#[test]
fn a_mode_epoch_cannot_be_paired_with_a_source_that_its_transition_has_not_cleared() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let evaluation = h.rt.hold_eval_when_free().await;
            seed(&h, &goals).await;
            model_stop(&h, "blocked").await;
            let old = lock(&h.rt.model_blocked_goal)
                .clone()
                .expect("live pre-transition model source");
            // The native harness isolates ordinary pace state in a private
            // Shared. A real mode transition owns the process-wide store, so
            // seed that production store explicitly before testing its fence.
            let process_shared = codex_alab_core::session::shared();
            process_shared.set_pace_at_mode(
                h.rt.key(),
                h.shared.pace(h.rt.key()).expect("seeded source pace"),
                Some(old.mode_epoch),
            );
            assert!(
                process_shared
                    .pace(h.rt.key())
                    .is_some_and(|pace| pace.model_blocked_goal.is_some()),
                "the process-wide store contains the source before transition"
            );

            codex_alab_core::set_enhanced(false);
            codex_alab_core::set_enhanced(true);
            let fresh = codex_alab_core::mode_epoch().expect("fresh Enhanced generation");
            assert_ne!(fresh, old.mode_epoch);
            assert!(
                process_shared
                    .pace(h.rt.key())
                    .is_none_or(|pace| pace.model_blocked_goal.is_none()),
                "the process-wide transition clears durable pace first"
            );

            // Recreate the old racy midpoint: the local observer has published
            // the fresh generation, but its old source has not been cleared yet.
            // Every source reader must wait on the enclosing transition lock.
            let sources = lock(&h.rt.model_goal_sources);
            *lock(&h.rt.model_goal_mode_epoch) = Some(fresh);
            assert_eq!(lock(&h.rt.model_blocked_goal).as_ref(), Some(&old));

            let (started_tx, started_rx) = std::sync::mpsc::channel();
            let (finished_tx, finished_rx) = std::sync::mpsc::channel();
            let autopilot = Arc::clone(&h.autopilot);
            let rt = Arc::clone(&h.rt);
            let publisher = std::thread::spawn(move || {
                started_tx.send(()).expect("announce the pace publisher");
                autopilot.sync_pace(&rt);
                finished_tx.send(()).expect("announce pace publication");
            });
            started_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("the concurrent publisher started");
            let entered = Instant::now();
            while h.autopilot.threads.try_lock().is_ok() {
                assert!(
                    entered.elapsed() < Duration::from_secs(1),
                    "the publisher never reached the source transition"
                );
                std::thread::yield_now();
            }
            assert!(
                matches!(
                    finished_rx.try_recv(),
                    Err(std::sync::mpsc::TryRecvError::Empty)
                ),
                "no reader may observe the fresh generation beside the old source"
            );

            lock(&h.rt.model_blocked_goal).take();
            lock(&h.rt.model_goal_import).goal.take();
            h.shared.retire_carried_model_goal(h.rt.key());
            drop(sources);
            finished_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("the publisher continues after the transition commits");
            publisher.join().expect("join the pace publisher");

            assert!(lock(&h.rt.model_blocked_goal).is_none());
            assert!(
                h.shared
                    .pace(h.rt.key())
                    .is_none_or(|pace| pace.model_blocked_goal.is_none()),
                "the fresh pace cannot reinsert the pre-transition source"
            );
            drop(evaluation);
            finish(&h).await;
        })
        .await
        .expect("mode/source transition serialization hang detector");
    });
}

#[test]
fn pace_publication_keeps_local_model_provenance_locked_until_the_shared_commit() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let evaluation = h.rt.hold_eval_when_free().await;
            seed(&h, &goals).await;
            model_stop(&h, "blocked").await;
            let expected = lock(&h.rt.model_blocked_goal)
                .as_ref()
                .expect("live model source")
                .goal
                .clone();

            let (entered_tx, entered_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            *lock(&h.autopilot.before_pace_source_publish) = Some(Box::new(move || {
                entered_tx
                    .send(())
                    .expect("announce the captured pace source");
                release_rx
                    .recv_timeout(Duration::from_secs(1))
                    .expect("release the captured pace source");
            }));

            let autopilot = Arc::clone(&h.autopilot);
            let rt = Arc::clone(&h.rt);
            let publisher = std::thread::spawn(move || autopilot.sync_pace(&rt));
            entered_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("pace publication reached the shared-source boundary");
            assert!(
                h.rt.model_goal_sources.try_lock().is_err(),
                "the local source must remain locked until its shared pace commits"
            );
            release_tx.send(()).expect("release pace publication");
            publisher.join().expect("join pace publication");
            assert_eq!(recorded_source(&h), Some(expected));

            drop(evaluation);
            finish(&h).await;
        })
        .await
        .expect("pace source publication lock hang detector");
    });
}

#[test]
fn a_vanilla_goal_tool_round_trip_advances_the_native_epoch() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let evaluation = h.rt.hold_eval_when_free().await;
            seed(&h, &goals).await;
            model_stop(&h, "blocked").await;
            let runtime = h
                .test
                .codex
                .thread_extension_data()
                .get::<codex_goal_extension::GoalRuntimeHandle>()
                .expect("native goal runtime");
            let before = runtime
                .alab_goal_mutation_epoch()
                .expect("pre-vanilla model epoch");

            codex_alab_core::set_enhanced(false);
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
            let after = runtime
                .alab_goal_mutation_epoch()
                .expect("post-tool vanilla model epoch");
            assert_ne!(after, before, "vanilla goal tools retain mutation fencing");
            codex_alab_core::set_enhanced(true);

            assert_eq!(goal(&h).await.status, ThreadGoalStatus::Blocked);
            assert_eq!(
                h.autopilot
                    .current_model_goal(&h.rt)
                    .await
                    .expect("read standing after vanilla model tools"),
                None,
                "the old source cannot borrow the tools' same final row"
            );
            drop(evaluation);
            finish(&h).await;
        })
        .await
        .expect("vanilla model-tool round-trip fence hang detector");
    });
}

#[test]
fn a_host_round_trip_defeats_a_prepared_model_block_resume() {
    native_test(|| async {
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
                .expect("real resume passed its preparatory goal read");
            let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
            let requests = responses::received_responses_requests(&h._server)
                .await
                .len();

            host_set(
                &h,
                &goals,
                GoalObjectiveUpdate::Keep,
                codex_protocol::protocol::ThreadGoalStatus::Paused,
            )
            .await;
            host_set(
                &h,
                &goals,
                GoalObjectiveUpdate::Keep,
                codex_protocol::protocol::ThreadGoalStatus::Blocked,
            )
            .await;
            let stopped = goal(&h).await;

            release.send(()).expect("release the prepared resume");
            recovering.await.expect("prepared evaluator settles");
            assert_eq!(
                goal(&h).await,
                stopped,
                "the later acknowledged mutation round-trip wins"
            );
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
            assert_eq!(
                responses::received_responses_requests(&h._server)
                    .await
                    .len(),
                requests,
                "the prepared stale resume must not open a provider request"
            );
            assert_eq!(
                h.autopilot
                    .current_model_goal(&h.rt)
                    .await
                    .expect("read model-goal standing after the race"),
                None
            );
            finish(&h).await;
        })
        .await
        .expect("prepared host round-trip model-stop fence hang detector");
    });
}

#[test]
fn a_mode_round_trip_defeats_a_prepared_model_block_resume() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let evaluation = h.rt.hold_eval_when_free().await;
            seed(&h, &goals).await;
            model_stop(&h, "blocked").await;
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
                .expect("real resume passed its preparatory goal read");
            let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
            let requests = responses::received_responses_requests(&h._server)
                .await
                .len();

            codex_alab_core::set_enhanced(false);
            codex_alab_core::set_enhanced(true);
            release.send(()).expect("release the prepared resume");
            recovering.await.expect("prepared evaluator settles");

            assert_eq!(goal(&h).await.status, ThreadGoalStatus::Blocked);
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted);
            assert_eq!(
                responses::received_responses_requests(&h._server)
                    .await
                    .len(),
                requests,
                "a pre-mode-boundary resume cannot open a provider request"
            );
            assert_eq!(
                h.autopilot
                    .current_model_goal(&h.rt)
                    .await
                    .expect("read model goal after the mode race"),
                None
            );
            finish(&h).await;
        })
        .await
        .expect("prepared mode-boundary model-stop fence hang detector");
    });
}

#[test]
fn a_fresh_model_block_after_host_activity_establishes_new_epoch_authority() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (h, goals) = harness().await;
            let evaluation = h.rt.hold_eval_when_free().await;
            let original = seed(&h, &goals).await;
            host_set(
                &h,
                &goals,
                GoalObjectiveUpdate::Keep,
                codex_protocol::protocol::ThreadGoalStatus::Paused,
            )
            .await;
            mount_model_stop(&h, "blocked").await;
            host_set(
                &h,
                &goals,
                GoalObjectiveUpdate::Keep,
                codex_protocol::protocol::ThreadGoalStatus::Active,
            )
            .await;
            completed(&h).await;
            let requests = responses::received_responses_requests(&h._server).await;
            assert_eq!(
                requests.len(),
                2,
                "the host resume started exactly one goal turn"
            );
            assert!(requests[0].body_contains_text(OBJECTIVE));
            assert!(
                requests[0].body_contains_text("Continue working toward the active thread goal.")
            );
            let bound = h
                .autopilot
                .current_model_goal(&h.rt)
                .await
                .expect("read the fresh model stop")
                .expect("a later genuine Active-to-Blocked transition binds");
            assert_eq!(bound.goal_id, original.goal_id);
            assert_eq!(bound.objective, original.objective);
            assert_eq!(bound.token_budget, original.token_budget);

            let accepted = h.rt.accepted_turns.load(Ordering::SeqCst);
            drop(evaluation);
            due(&h).await;
            evaluate(&h).await;
            wait_requests(&h, 3).await;
            assert_eq!(goal(&h).await.status, ThreadGoalStatus::Active);
            assert_eq!(h.rt.accepted_turns.load(Ordering::SeqCst), accepted + 1);
            finish(&h).await;
        })
        .await
        .expect("fresh post-host model-stop epoch hang detector");
    });
}

async fn duplicate_source(held: bool) {
    let (old, goals) = harness().await;
    let old_rt = Arc::clone(&old.rt);
    let evaluation = old_rt.hold_eval_when_free().await;
    let original = seed(&old, &goals).await;
    model_stop(&old, "blocked").await;
    let source = saved_source(&old);
    if held {
        host_set(
            &old,
            &goals,
            GoalObjectiveUpdate::Keep,
            codex_protocol::protocol::ThreadGoalStatus::Paused,
        )
        .await;
        host_set(
            &old,
            &goals,
            GoalObjectiveUpdate::Keep,
            codex_protocol::protocol::ThreadGoalStatus::Blocked,
        )
        .await;
    }
    mount_model_stop(&old, "blocked").await;
    start_and_complete(&old).await;
    let saved = recorded_source(&old);
    if held {
        assert_eq!(
            saved, None,
            "a native mutation round-trip retires stale durable model-stop authority"
        );
        assert_eq!(
            old.autopilot
                .current_model_goal(&old.rt)
                .await
                .expect("read model-goal standing after the held duplicate"),
            None
        );
        assert!(
            old.shared
                .pace(old.rt.key())
                .is_none_or(|pace| pace.model_blocked_goal.is_none()),
            "revoked native proof must leave the current durable pace"
        );
    } else {
        assert_eq!(
            saved.as_ref(),
            Some(&source),
            "a contiguous duplicate block preserves the whole original source, including its turn"
        );
    }
    let h = fresh_image(old, goals).await;
    drop(evaluation);
    verify_import(&h, saved.unwrap_or(source), &original, !held).await;
    finish(&h).await;
}

#[test]
fn a_later_duplicate_block_preserves_its_original_source_and_recovers_after_restart() {
    native_test(|| async {
        tokio::time::timeout(HANG, duplicate_source(false))
            .await
            .expect("duplicate source recovery hang detector");
    });
}

#[test]
fn a_later_duplicate_block_cannot_move_its_source_past_a_native_host_hold() {
    native_test(|| async {
        tokio::time::timeout(HANG, duplicate_source(true))
            .await
            .expect("duplicate source host-hold hang detector");
    });
}

#[test]
fn a_transient_duplicate_chronology_read_failure_durably_revokes_the_old_source() {
    native_test(|| async {
        tokio::time::timeout(HANG, async {
            let (old, goals) = harness().await;
            let old_rt = Arc::clone(&old.rt);
            let evaluation = old_rt.hold_eval_when_free().await;
            let writer: &'static codex_alab_tui::session::Writer =
                Box::leak(Box::new(codex_alab_tui::session::Writer::new(
                    old.shared,
                    codex_alab_tui::record::sessions_dir(old.test.codex_home_path()),
                    Duration::ZERO,
                    None,
                )));
            writer
                .write_now()
                .expect("write the initial crash-recovery record");
            writer.install_write_through();
            seed(&old, &goals).await;
            model_stop(&old, "blocked").await;
            let stale = saved_source(&old);
            assert_eq!(durable_source(&old), Some(stale));

            *lock(&old.autopilot.model_goal_generation_read_failure) =
                Some("injected transient chronology read failure".into());
            mount_model_stop(&old, "blocked").await;
            start_and_complete(&old).await;
            assert_eq!(
                recorded_source(&old),
                None,
                "an unproven duplicate revokes live source authority"
            );
            assert_eq!(
                durable_source(&old),
                None,
                "the exact revocation reaches the crash record before acknowledgement"
            );

            let h = fresh_image(old, goals).await;
            drop(evaluation);
            assert_eq!(
                h.autopilot
                    .current_model_goal(&h.rt)
                    .await
                    .expect("read model-goal standing after restart"),
                None,
                "clearing the transient read failure cannot resurrect the old crash source"
            );
            finish(&h).await;
        })
        .await
        .expect("transient duplicate chronology read hang detector");
    });
}
