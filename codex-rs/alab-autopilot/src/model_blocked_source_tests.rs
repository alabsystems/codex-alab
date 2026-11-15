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

async fn host_set(
    h: &Harness,
    goals: &GoalService,
    objective: GoalObjectiveUpdate<'_>,
    status: codex_protocol::protocol::ThreadGoalStatus,
) {
    let outcome = goals
        .set_thread_goal(
            &h.test.codex.state_db().expect("native goal database"),
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
    outcome.apply_runtime_effects(goals).await;
    h.test
        .codex
        .flush_rollout()
        .await
        .expect("flush the ordered host/API outcome");
}

pub(super) fn saved_source(h: &Harness) -> WallGoal {
    let recorded = serde_json::to_value(codex_alab_tui::record::Pace::from(
        h.shared.pace(h.rt.key()).expect("native source pace"),
    ))
    .expect("encode native source pace");
    let recorded: codex_alab_tui::record::Pace =
        serde_json::from_value(recorded).expect("decode native source pace");
    Pace::from(recorded)
        .model_blocked_goal
        .expect("the codec preserves the committed model stop")
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
        imported,
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
    let saved = if held {
        assert!(
            old.autopilot
                .current_model_goal(&old.rt)
                .await
                .expect("check the duplicate after native host mutations")
                .is_none(),
            "a duplicate cannot recreate live proof across the native host hold"
        );
        assert!(
            old.shared
                .pace(old.rt.key())
                .is_none_or(|pace| pace.model_blocked_goal.is_none()),
            "revoked native proof must leave the current durable pace"
        );
        // An older selected record can still arrive after the current writer
        // revoked it. The true restart must reject that pre-hold codec source.
        source
    } else {
        let saved = saved_source(&old);
        assert_eq!(
            saved, source,
            "a later duplicate block must preserve the whole original source, including its turn"
        );
        saved
    };
    let h = fresh_image(old, goals).await;
    drop(evaluation);
    verify_import(&h, saved, &original, !held).await;
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
