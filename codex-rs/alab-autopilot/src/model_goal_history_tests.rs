// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Imported model-stop provenance cannot cross durable goal holds or edits.

use super::model_goal_stop_revoked_since;
use codex_alab_core::session::WallGoal;
use codex_history::RolloutItem;
use codex_protocol::ThreadId;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadGoal;
use codex_protocol::protocol::ThreadGoalStatus;
use codex_protocol::protocol::ThreadGoalUpdatedEvent;

const ORIGIN: &str = "the-original-model-stop-turn";
const OBJECTIVE: &str = "finish the original compiler integration";

fn bound() -> WallGoal {
    WallGoal {
        goal_id: "the-original-goal".into(),
        objective: OBJECTIVE.into(),
        token_budget: Some(1_000),
        turn: ORIGIN.into(),
        person_stops: 0,
    }
}

fn started(turn: &str) -> RolloutItem {
    RolloutItem::EventMsg(
        serde_json::from_value(serde_json::json!({
            "type": "task_started",
            "turn_id": turn,
            "model_context_window": 258_400,
            "collaboration_mode_kind": "default",
            "turn_attribution": {"turn_id": turn, "turn_trigger": "goal"},
        }))
        .expect("native turn start"),
    )
}

fn updated(
    id: ThreadId,
    status: ThreadGoalStatus,
    objective: &str,
    token_budget: Option<i64>,
) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::ThreadGoalUpdated(ThreadGoalUpdatedEvent {
        thread_id: id,
        turn_id: None,
        goal: ThreadGoal {
            thread_id: id,
            objective: objective.into(),
            status,
            token_budget,
            tokens_used: 17,
            time_used_seconds: 9,
            created_at: 1,
            // The source format can give several different updates the same
            // second. A stop is not erased by an equal-time later Active row.
            updated_at: 1,
        },
    }))
}

fn matching(id: ThreadId, status: ThreadGoalStatus) -> RolloutItem {
    updated(id, status, OBJECTIVE, Some(1_000))
}

fn person_goal(update: codex_core::context::UserGoalUpdate) -> RolloutItem {
    RolloutItem::ResponseItem(codex_core::context::ContextualUserFragment::into(update).into())
}

#[test]
fn active_and_repeated_blocked_updates_preserve_the_same_model_stop() {
    let id = ThreadId::new();
    let mut accounted = matching(id, ThreadGoalStatus::Active);
    if let RolloutItem::EventMsg(EventMsg::ThreadGoalUpdated(update)) = &mut accounted {
        update.goal.tokens_used = 300;
        update.goal.time_used_seconds = 45;
        update.goal.updated_at = 200;
    }
    let items = [
        // Older holds belong to a time before this binding's native turn.
        matching(id, ThreadGoalStatus::Paused),
        updated(id, ThreadGoalStatus::Active, "an older objective", None),
        started(ORIGIN),
        matching(id, ThreadGoalStatus::Active),
        matching(id, ThreadGoalStatus::Blocked),
        started("an-unrelated-later-question"),
        person_goal(codex_core::context::UserGoalUpdate::Set {
            objective: None,
            status: Some(ThreadGoalStatus::Active),
        }),
        accounted,
        matching(id, ThreadGoalStatus::Active),
        matching(id, ThreadGoalStatus::Blocked),
        matching(id, ThreadGoalStatus::Blocked),
    ];
    assert!(!model_goal_stop_revoked_since(&items, id, &bound()));
}

#[test]
fn every_saved_terminal_or_quota_stop_revokes_custody_even_if_reverted() {
    let id = ThreadId::new();
    for status in [
        ThreadGoalStatus::Paused,
        ThreadGoalStatus::Complete,
        ThreadGoalStatus::UsageLimited,
        ThreadGoalStatus::BudgetLimited,
    ] {
        for later_turn in [false, true] {
            let mut items = vec![started(ORIGIN), matching(id, ThreadGoalStatus::Blocked)];
            if later_turn {
                items.push(started("a-later-native-turn"));
            }
            items.extend([
                matching(id, status),
                matching(id, ThreadGoalStatus::Active),
                matching(id, ThreadGoalStatus::Blocked),
            ]);
            assert!(
                model_goal_stop_revoked_since(&items, id, &bound()),
                "{status:?}, later_turn={later_turn}: restoring the row cannot undo the hold"
            );
        }
    }
}

#[test]
fn a_goal_edit_then_return_to_the_original_values_does_not_restore_old_custody() {
    let id = ThreadId::new();
    for edit in [
        updated(
            id,
            ThreadGoalStatus::Active,
            "a different objective",
            Some(1_000),
        ),
        updated(id, ThreadGoalStatus::Blocked, OBJECTIVE, Some(2_000)),
        updated(id, ThreadGoalStatus::Active, OBJECTIVE, None),
    ] {
        let items = [
            started(ORIGIN),
            matching(id, ThreadGoalStatus::Blocked),
            edit,
            matching(id, ThreadGoalStatus::Active),
            matching(id, ThreadGoalStatus::Blocked),
        ];
        assert!(model_goal_stop_revoked_since(&items, id, &bound()));
    }
}

#[test]
fn another_threads_goal_events_cannot_revoke_this_threads_model_stop() {
    let id = ThreadId::new();
    let other = ThreadId::new();
    let mut items = vec![started(ORIGIN), matching(id, ThreadGoalStatus::Blocked)];
    for status in [
        ThreadGoalStatus::Paused,
        ThreadGoalStatus::Complete,
        ThreadGoalStatus::UsageLimited,
        ThreadGoalStatus::BudgetLimited,
    ] {
        items.push(updated(other, status, "another thread's objective", None));
    }
    items.push(matching(id, ThreadGoalStatus::Blocked));
    assert!(!model_goal_stop_revoked_since(&items, id, &bound()));
}

#[test]
fn missing_origin_and_host_annotated_person_stops_reject_import() {
    use codex_core::context::UserGoalUpdate;

    let id = ThreadId::new();
    assert!(model_goal_stop_revoked_since(&[], id, &bound()));
    assert!(model_goal_stop_revoked_since(
        &[
            started("another-turn"),
            matching(id, ThreadGoalStatus::Blocked)
        ],
        id,
        &bound(),
    ));
    for instruction in [
        UserGoalUpdate::Clear,
        UserGoalUpdate::Set {
            objective: None,
            status: Some(ThreadGoalStatus::Blocked),
        },
        UserGoalUpdate::Set {
            objective: None,
            status: Some(ThreadGoalStatus::Paused),
        },
        UserGoalUpdate::Set {
            objective: Some("the person's replacement objective".into()),
            status: None,
        },
    ] {
        let items = [
            started(ORIGIN),
            matching(id, ThreadGoalStatus::Blocked),
            person_goal(instruction),
            matching(id, ThreadGoalStatus::Active),
            matching(id, ThreadGoalStatus::Blocked),
        ];
        assert!(model_goal_stop_revoked_since(&items, id, &bound()));
    }
}

#[test]
fn a_later_block_in_the_origin_turn_cannot_hide_an_earlier_hold() {
    let id = ThreadId::new();
    let items = [
        started(ORIGIN),
        matching(id, ThreadGoalStatus::Paused),
        matching(id, ThreadGoalStatus::Active),
        matching(id, ThreadGoalStatus::Blocked),
        matching(id, ThreadGoalStatus::Blocked),
    ];
    assert!(model_goal_stop_revoked_since(&items, id, &bound()));
}

#[test]
fn conflicting_goal_thread_metadata_fails_closed_for_the_target_thread() {
    let id = ThreadId::new();
    let mut conflict = matching(id, ThreadGoalStatus::Active);
    let RolloutItem::EventMsg(EventMsg::ThreadGoalUpdated(update)) = &mut conflict else {
        unreachable!("goal update fixture");
    };
    update.goal.thread_id = ThreadId::new();
    assert!(model_goal_stop_revoked_since(
        &[started(ORIGIN), conflict],
        id,
        &bound(),
    ));
}
