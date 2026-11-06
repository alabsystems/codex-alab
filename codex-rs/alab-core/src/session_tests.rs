// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use crate::session::BlockedKind;
use crate::session::Frame;
use crate::session::LastTurn;
use crate::session::ProgramState;
use crate::session::Shared;
use pretty_assertions::assert_eq;
use std::path::Path;
use std::time::Duration;
use std::time::Instant;

fn frame<'a>(primary: Option<&'a str>, state: ProgramState) -> Frame<'a> {
    Frame {
        primary_thread: primary,
        displayed_thread: primary,
        rollout_path: Some(Path::new("/r/rollout.jsonl")),
        cwd: Path::new("/work"),
        model: "gpt-5",
        codex_home: Path::new("/home/.codex"),
        embedded: true,
        state,
        draft_empty: true,
        queued_input: false,
        popup_open: false,
        editor_open: false,
        pending_user_turn: false,
    }
}

#[test]
fn an_unchanged_frame_is_not_a_change() {
    let shared = Shared::new();
    let first = frame(Some("t1"), ProgramState::Idle);
    assert!(shared.observe(&first, 10));
    let generation = shared.generation();
    assert!(!shared.observe(&first, 20));
    assert_eq!(shared.generation(), generation);
    assert_eq!(shared.view().state_since_ms, 10);
}

#[test]
fn in_flight_provider_pacing_tracks_only_the_current_primary_native_turn() {
    use crate::session::Autopilot;
    use crate::session::InFlightRetry;
    let shared = Shared::new();
    shared.observe(&frame(Some("t1"), ProgramState::Working), 10);
    let retry = InFlightRetry {
        thread: "t1".into(),
        turn_id: "waiting".into(),
        not_before_ms: 500,
    };
    assert!(!shared.publish_in_flight_retry(retry.clone()));
    shared.turn_started("t1", "waiting");
    assert!(shared.publish_in_flight_retry(retry.clone()));
    assert_eq!(shared.autopilot(), Autopilot::Off);
    for mode in [
        Autopilot::Active,
        Autopilot::Passive,
        Autopilot::Off,
        Autopilot::Active,
    ] {
        shared.set_autopilot(mode);
        assert_eq!(shared.in_flight_retry(), Some(retry.clone()));
    }
    let side = InFlightRetry {
        thread: "side".into(),
        ..retry.clone()
    };
    shared.turn_started("side", "waiting");
    assert!(!shared.publish_in_flight_retry(side));
    assert!(!shared.clear_in_flight_retry("side", "waiting"));
    shared.turn_started("t1", "newer");
    assert_eq!(shared.in_flight_retry(), None);
    assert!(
        !shared.publish_in_flight_retry(retry.clone()),
        "an old running callback cannot cross newer native acceptance"
    );
    let newer = InFlightRetry {
        turn_id: "newer".into(),
        ..retry
    };
    assert!(shared.publish_in_flight_retry(newer.clone()));
    shared.turn_ended("t1", "waiting");
    assert_eq!(shared.in_flight_retry(), Some(newer));
    shared.turn_ended("t1", "newer");
    assert_eq!(shared.in_flight_retry(), None);
}

#[test]
fn in_flight_waits_end_at_manual_stops_and_survive_only_relaunchable_signal_teardown() {
    use crate::session::InFlightRetry;
    for (signal, kept) in [
        (None, false),
        (Some(2), false),
        (Some(1), true),
        (Some(15), true),
    ] {
        let shared = Shared::new();
        shared.observe(&frame(Some("t1"), ProgramState::Working), 10);
        shared.turn_started("t1", "waiting");
        let retry = InFlightRetry {
            thread: "t1".into(),
            turn_id: "waiting".into(),
            not_before_ms: 500,
        };
        assert!(shared.publish_in_flight_retry(retry.clone()));
        if let Some(signal) = signal {
            shared.set_signal(signal);
            if signal == 2 {
                assert!(
                    !shared.publish_in_flight_retry(retry.clone()),
                    "a late native response cannot republish after SIGINT"
                );
            }
        }
        shared.clear_in_flight_retry("t1", "waiting");
        shared.turn_ended("t1", "waiting");
        shared.thread_ended("t1");
        assert_eq!(
            shared.in_flight_retry(),
            kept.then_some(retry),
            "signal={signal:?}"
        );
    }
    let shared = Shared::new();
    shared.observe(&frame(Some("t1"), ProgramState::Working), 10);
    shared.turn_started("t1", "waiting");
    assert!(shared.publish_in_flight_retry(InFlightRetry {
        thread: "t1".into(),
        turn_id: "waiting".into(),
        not_before_ms: 500
    }));
    shared.clear_in_flight_retry("t1", "waiting");
    shared.set_signal(15);
    assert_eq!(
        shared.in_flight_retry(),
        None,
        "a later TERM cannot resurrect a manually cancelled wait"
    );
}

#[test]
fn provider_intent_survives_passivity_and_ends_at_off_or_a_new_target_turn() {
    use crate::session::Autopilot;
    use crate::session::Recovery;
    use crate::session::RecoveryWall;

    let shared = Shared::new();
    shared.set_autopilot(Autopilot::Active);
    let recovery = Recovery {
        thread: "t1".into(),
        turn_id: "failed".into(),
        point_at_ms: 10,
        not_before_ms: 500,
        by_autopilot: true,
        by_person: false,
        wall: RecoveryWall::Usage {
            quota: None,
            reset_at_ms: Some(500),
            reset_label: None,
        },
    };
    assert!(shared.publish_recovery(shared.upgrade_generation(), recovery.clone(), || true));
    shared.set_autopilot(Autopilot::Passive);
    assert_eq!(shared.recovery(), Some(recovery.clone()));
    shared.set_autopilot(Autopilot::Off);
    assert_eq!(shared.recovery(), None);

    shared.set_autopilot(Autopilot::Active);
    assert!(shared.publish_recovery(shared.upgrade_generation(), recovery.clone(), || true));
    let owed = crate::session::OwedAct {
        thread: "t1".into(),
        ..Default::default()
    };
    shared.set_owed_act(Some(owed.clone()));
    shared.turn_started("other-thread", "unrelated");
    assert_eq!(shared.recovery(), Some(recovery.clone()));
    assert_eq!(shared.owed_act(), Some(owed.clone()));
    shared.turn_started("t1", "superseding");
    assert_eq!(shared.recovery(), None);
    assert_eq!(shared.owed_act(), None);
    shared.set_owed_act(Some(owed.clone()));
    assert_eq!(
        shared.owed_act(),
        None,
        "a late record sync cannot restore it"
    );
    assert!(!shared.publish_recovery(shared.upgrade_generation(), recovery, || true));
    shared.turn_ended("t1", "superseding");
    shared.set_owed_act(Some(owed.clone()));
    assert_eq!(shared.owed_act(), Some(owed));
}

#[test]
fn an_upgrade_commits_only_the_native_state_it_actually_checked() {
    use crate::session::Autopilot;
    let mutations: &[fn(&Shared)] = &[
        |s| s.human_input(100),
        |s| {
            let mut f = frame(Some("t1"), ProgramState::Idle);
            f.draft_empty = false;
            s.observe(&f, 20);
        },
        |s| {
            let mut f = frame(Some("t1"), ProgramState::Idle);
            f.queued_input = true;
            s.observe(&f, 20);
        },
        |s| {
            let mut f = frame(Some("t1"), ProgramState::Idle);
            f.popup_open = true;
            s.observe(&f, 20);
        },
        |s| {
            let mut f = frame(Some("t1"), ProgramState::Idle);
            f.pending_user_turn = true;
            s.observe(&f, 20);
        },
        |s| {
            s.turn_started("background-thread", "new-turn");
        },
        |s| s.note_root_thread("new-thread"),
        |s| s.set_overlay_open(true),
        |s| {
            s.set_signal(15);
        },
        |s| s.set_autopilot(Autopilot::Off),
        |s| s.invalidate_upgrade(),
    ];
    for change in mutations {
        let shared = Shared::new();
        shared.set_autopilot(Autopilot::Active);
        shared.observe(&frame(Some("t1"), ProgramState::Idle), 10);
        let checked = shared.upgrade_generation();
        change(&shared);
        assert!(!shared.commit_upgrade(checked));
        assert!(!shared.exec_pending());
    }
    let shared = Shared::new();
    shared.set_autopilot(Autopilot::Active);
    shared.observe(&frame(Some("t1"), ProgramState::Idle), 10);
    let checked = shared.upgrade_generation();
    // Record-only state is deliberately outside the native fence.
    shared.set_owed_act(Some(crate::session::OwedAct::default()));
    assert!(shared.commit_upgrade(checked));
    assert!(shared.exec_pending());
    assert!(!shared.cancel_upgrade(|| panic!("a committed leave cannot restore its timer")));
}

#[test]
fn cancelling_a_request_after_dispatch_begins_invalidates_its_uncommitted_leave() {
    use crate::session::Autopilot;
    use crate::session::UpgradeRequest;
    use std::sync::Arc;
    use std::sync::mpsc;

    let shared = Arc::new(Shared::new());
    shared.set_autopilot(Autopilot::Active);
    shared.observe(&frame(Some("t1"), ProgramState::Idle), 10);
    let checked = shared.upgrade_generation();
    let (entered, entering) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let committing = Arc::clone(&shared);
    let (request, cancellation) = UpgradeRequest::new(move |ready| {
        entered.send(()).unwrap_or_else(|err| panic!("{err}"));
        released
            .recv_timeout(Duration::from_secs(60))
            .unwrap_or_else(|err| panic!("{err}"));
        ready && committing.commit_upgrade(checked)
    });
    let dispatcher = std::thread::spawn(move || request.decide(true));
    entering
        .recv_timeout(Duration::from_secs(60))
        .unwrap_or_else(|err| panic!("{err}"));
    // This must not wait on a callback invoked under the request's decision lock.
    drop(cancellation);
    assert!(shared.cancel_upgrade(|| {}));
    release.send(()).unwrap_or_else(|err| panic!("{err}"));
    assert!(
        !dispatcher
            .join()
            .unwrap_or_else(|_| panic!("dispatcher panicked"))
    );
    assert!(!shared.exec_pending());
}

#[test]
fn each_fact_moves_the_view() {
    let shared = Shared::new();
    shared.observe(&frame(Some("t1"), ProgramState::Idle), 10);
    let mut next = frame(Some("t1"), ProgramState::Blocked(BlockedKind::Question));
    assert!(shared.observe(&next, 30));
    assert_eq!(shared.view().state_since_ms, 30);
    next.draft_empty = false;
    next.queued_input = true;
    next.popup_open = true;
    next.pending_user_turn = true;
    assert!(shared.observe(&next, 40));
    assert!(!shared.draft_empty());
    assert!(shared.queued_input());
    assert!(shared.popup_open());
    assert!(shared.pending_user_turn());
    assert_eq!(
        shared.view().state_since_ms,
        30,
        "only a state change moves it"
    );
}

/// A thread once registered stays registered for the startup screens' gate
/// (`Shared::any_thread_registered`), though the primary goes back to none (the agents
/// overview detaches it, a reconnect resets it). NEGATIVE CONTROL: before any frame names a
/// thread, none is.
#[test]
fn a_registered_thread_outlives_the_primary_going_back_to_none() {
    let shared = Shared::new();
    shared.observe(&frame(None, ProgramState::Idle), 10);
    assert!(!shared.any_thread_registered(), "nothing registered yet");
    shared.observe(&frame(Some("t1"), ProgramState::Idle), 20);
    shared.observe(&frame(None, ProgramState::Idle), 30);
    assert_eq!(shared.view().primary_thread, None);
    assert!(shared.any_thread_registered());
}

#[test]
fn threads_register_with_their_rollout() {
    let shared = Shared::new();
    assert!(!shared.is_registered("t1"));
    let mut side = frame(Some("t1"), ProgramState::Idle);
    side.displayed_thread = Some("side");
    side.rollout_path = Some(Path::new("/r/side.jsonl"));
    shared.observe(&side, 1);
    assert!(shared.is_registered("t1"));
    assert!(shared.is_registered("side"));
    let view = shared.view();
    assert_eq!(view.primary_thread.as_deref(), Some("t1"));
    assert_eq!(view.displayed_thread.as_deref(), Some("side"));
    assert_eq!(view.primary().and_then(|t| t.rollout_path.clone()), None);
    assert_eq!(
        view.threads["side"].rollout_path.as_deref(),
        Some(Path::new("/r/side.jsonl"))
    );
}

#[test]
fn a_last_turn_is_kept_only_for_registered_threads() {
    let shared = Shared::new();
    let turn = LastTurn {
        turn_id: Some("u1".into()),
        ended_at_ms: 5,
        last_agent_message: Some("The cache lives in memory now.".into()),
        by_autopilot: false,
    };
    assert!(!shared.set_last_turn("t1", turn.clone()));
    shared.observe(&frame(Some("t1"), ProgramState::Idle), 1);
    assert!(shared.set_last_turn("t1", turn.clone()));
    assert_eq!(shared.last_turn("t1"), Some(turn));
}

#[test]
fn the_first_signal_wins() {
    let shared = Shared::new();
    assert_eq!(shared.signal(), None);
    assert!(shared.set_signal(15));
    assert!(!shared.set_signal(1));
    assert_eq!(shared.signal(), Some(15));
}

#[test]
fn human_input_stamps_and_wakes() {
    let shared = Shared::new();
    assert_eq!(shared.last_human_input_ms(), None);
    let seen = shared.generation();
    shared.human_input(1234);
    assert_eq!(shared.last_human_input_ms(), Some(1234));
    assert_ne!(shared.wait_change(seen, Duration::from_secs(5)), seen);
}

#[test]
fn a_wait_with_no_change_times_out() {
    let shared = Shared::new();
    let seen = shared.generation();
    assert_eq!(shared.wait_change(seen, Duration::from_millis(20)), seen);
}

#[test]
fn launch_vars_are_the_launchs() {
    let shared = Shared::new();
    let launch = crate::session::Launch {
        vars: vec![("CODEX_HOME".into(), "/h".into())],
        ..Default::default()
    };
    assert!(shared.set_launch(launch));
    assert!(!shared.set_launch(Default::default()));
    let launch = shared.launch().unwrap_or_else(|| panic!("launch"));
    assert_eq!(launch.var("CODEX_HOME"), Some(std::ffi::OsStr::new("/h")));
    assert_eq!(launch.var("PATH"), None);
}

#[test]
fn a_swept_predecessor_is_found_by_its_session() {
    use crate::session::Predecessor;
    let shared = Shared::new();
    assert_eq!(shared.predecessor("s1"), None);
    for (pid, exit) in [(1, Some("signal:2")), (2, None)] {
        shared.note_predecessor(Predecessor {
            pid,
            session_id: "s1".to_string(),
            exit: exit.map(str::to_string),
            updated_at_ms: u64::from(pid),
            ..Default::default()
        });
    }
    // The uniquely newest write stands for the session, independent of sweep order.
    assert_eq!(shared.predecessor("s1").map(|p| p.pid), Some(2));
    assert_eq!(shared.predecessor("s2"), None);
}

#[test]
fn the_launch_witness_is_set_once_and_waited_for() {
    use crate::session::HumanSeen;
    use crate::session::LaunchWitness;
    let shared = Shared::new();
    assert!(!shared.launch_probed());
    assert!(!shared.wait_launch_probed(Duration::from_millis(10)));
    let witness = LaunchWitness {
        hand: Some("lease:aterm-harness@7".to_string()),
        human: HumanSeen::Never,
        read_at_ms: 5,
        server_pid: Some(7),
        hand_pid: crate::session::HandPid::Absent,
    };
    assert!(shared.set_launch_witness(Some(witness.clone())));
    // The first probe's read stands: a later one never replaces it.
    assert!(!shared.set_launch_witness(None));
    assert!(shared.wait_launch_probed(Duration::from_millis(10)));
    assert_eq!(shared.launch_witness(), Some(witness));
    // A probe that read nothing is done too, and witnesses nothing.
    let unread = Shared::new();
    assert!(unread.set_launch_witness(None));
    assert!(unread.launch_probed());
    assert_eq!(unread.launch_witness(), None);
}

#[test]
fn the_carry_on_count_moves_the_record() {
    let shared = Shared::new();
    assert_eq!(shared.carried_on(), 0);
    let generation = shared.generation();
    shared.set_carried_on(1);
    assert_eq!(shared.carried_on(), 1);
    assert_ne!(shared.generation(), generation);
    // The same count again is no change.
    let generation = shared.generation();
    shared.set_carried_on(1);
    assert_eq!(shared.generation(), generation);
    shared.set_carried_on(0);
    assert_eq!(shared.carried_on(), 0);
}

#[test]
fn an_act_that_queues_its_turn_stays_busy_until_the_turn_starts() {
    // G1 (review of 6efbf7518): `Op::Compact` and a goal resume return before their turn
    // starts. The act's guard alone leaves a gap where the record reads rest (the control).
    use crate::session::Unrest;
    let shared = Shared::new();
    let guard = shared.begin_act();
    drop(guard);
    assert_eq!(shared.unrest(), None, "the gap the guard alone leaves");
    // The awaited turn, taken under the guard, covers it.
    let guard = shared.begin_act();
    let _awaited = shared.await_turn("t1", Duration::from_secs(30));
    drop(guard);
    assert_eq!(shared.unrest(), Some(Unrest::Acting));
    // Another thread's turn is not the one awaited.
    shared.turn_started("t2", "x");
    shared.turn_ended("t2", "x");
    assert_eq!(shared.unrest(), Some(Unrest::Acting));
    // Its turn starts: running, then ended, never rest between.
    shared.turn_started("t1", "compact");
    assert_eq!(shared.unrest(), Some(Unrest::TurnRunning));
    shared.turn_ended("t1", "compact");
    assert_eq!(shared.unrest(), None);
    // An act that failed gives the wait up.
    let awaited = shared.await_turn("t1", Duration::from_secs(30));
    assert_eq!(shared.unrest(), Some(Unrest::Acting));
    shared.cancel_await(awaited);
    assert_eq!(shared.unrest(), None);
    // The thread's stop ends it.
    let _awaited = shared.await_turn("t1", Duration::from_secs(30));
    shared.thread_ended("t1");
    assert_eq!(shared.unrest(), None);
    // A turn that never comes is waited for only so long.
    let _awaited = shared.await_turn("t1", Duration::ZERO);
    assert_eq!(shared.unrest(), None, "out of time");
    assert!(shared.await_deadline_ms().is_some());
    assert!(shared.expire_awaited(crate::session::now_ms()));
    assert_eq!(shared.await_deadline_ms(), None);
    assert!(!shared.expire_awaited(crate::session::now_ms()));
}

/// The footer's count is unknown until a frame hands one, then follows each frame both ways:
/// a moved count marks the record changed, the same count again does not.
#[test]
fn background_terminals_are_unknown_until_a_frame_hands_a_count_and_move_both_ways() {
    let shared = Shared::new();
    let now = Instant::now();
    assert_eq!(shared.background_terminals(), None);
    let generation = shared.generation();
    shared.set_background_terminals(2, now);
    assert_eq!(shared.background_terminals(), Some(2));
    assert_ne!(shared.generation(), generation, "a moved count moves");
    let generation = shared.generation();
    shared.set_background_terminals(2, now);
    assert_eq!(shared.generation(), generation, "the same count is unmoved");
    shared.set_background_terminals(0, now);
    assert_eq!(shared.background_terminals(), Some(0), "the count falls");
    assert_ne!(shared.generation(), generation, "a fall moves");
    shared.set_background_terminals(u32::MAX, now);
    assert_eq!(shared.background_terminals(), Some(u32::MAX));
}

/// A moved count is held until it has stood the hold (the record writer's interval) or its
/// hold is ended: the same count again restarts nothing, a move restarts it, and its end is a
/// change the writer writes. So a command that ends within the hold costs the record no write.
#[test]
fn a_moved_count_is_held_until_it_has_stood_the_hold() {
    let shared = Shared::new();
    let hold = Duration::from_secs(1);
    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    assert!(!shared.background_terminals_held(t0, hold), "nothing moved");
    assert_eq!(shared.background_terminals_hold_deadline(hold), None);

    shared.set_background_terminals(1, t0);
    assert!(shared.background_terminals_held(at(999), hold));
    assert!(
        !shared.background_terminals_held(at(1000), hold),
        "it has stood the hold"
    );
    shared.set_background_terminals(1, at(500));
    assert_eq!(
        shared.background_terminals_hold_deadline(hold),
        Some(at(1000)),
        "the same count restarts nothing"
    );
    shared.set_background_terminals(0, at(600));
    assert_eq!(
        shared.background_terminals_hold_deadline(hold),
        Some(at(1600)),
        "a move restarts the hold"
    );

    let generation = shared.generation();
    assert!(!shared.expire_background_terminals_hold(at(1599), hold));
    assert_eq!(
        shared.generation(),
        generation,
        "a hold that stands is no change"
    );
    assert!(shared.expire_background_terminals_hold(at(1600), hold));
    assert_ne!(shared.generation(), generation, "its end is a change");
    assert_eq!(shared.background_terminals_hold_deadline(hold), None);
    assert!(
        !shared.background_terminals_held(at(1000), hold),
        "an ended hold holds nothing"
    );
    assert!(
        !shared.expire_background_terminals_hold(at(9999), hold),
        "it ends once"
    );
    assert_eq!(shared.background_terminals(), Some(0));
}

mod boxes {
    use crate::boxes::BoxAnswer;
    use crate::boxes::BoxKind;
    use crate::boxes::BoxOutcome;
    use crate::boxes::Choice;
    use crate::boxes::GoalStop;
    use crate::boxes::OpenBox;
    use crate::session::Shared;
    use pretty_assertions::assert_eq;

    fn plan(id: &str, opened_ms: u64) -> OpenBox {
        OpenBox {
            id: id.to_owned(),
            kind: BoxKind::Plan {
                rows: vec![("Yes, implement this plan".to_owned(), None)],
            },
            opened_ms,
        }
    }

    fn question(id: &str, begun: Option<&str>) -> OpenBox {
        OpenBox {
            id: id.to_owned(),
            kind: BoxKind::Question {
                thread: "t1".to_owned(),
                questions: Vec::new(),
                blocking: true,
                begun: begun.map(str::to_owned),
            },
            opened_ms: 5,
        }
    }

    #[test]
    fn a_box_stays_open_while_each_frame_reports_it_and_closes_on_the_first_that_does_not() {
        let shared = Shared::new();
        assert!(!shared.touch_box("plan-1", None));
        shared.report_box(plan("plan-1", 10));
        assert!(!shared.end_frame_boxes());
        assert!(shared.touch_box("plan-1", None));
        shared.report_box(plan("plan-1", 99));
        assert_eq!(shared.open_box(), Some(plan("plan-1", 10)));
        assert!(!shared.end_frame_boxes());
        assert!(
            shared.end_frame_boxes(),
            "a frame without the box closes it"
        );
        assert_eq!(shared.open_box(), None);
    }

    /// A selection view (the plan box, the goal box) has no id of its own: it keeps the one its
    /// place gives it while its box stays open; once that box closed (answered, or a frame went
    /// without it), a later view in the same place is a new box, and each `Shared` counts its
    /// own (parallel tests share no counter).
    #[test]
    fn a_views_box_id_lasts_while_its_box_is_open_and_belongs_to_its_state() {
        let shared = Shared::new();
        let first = shared.view_box_id("goal", 0x1000);
        assert_eq!(shared.view_box_id("goal", 0x1000), first, "the same view");
        shared.report_box(plan(&first, 1));
        shared.close_box(&first);
        let second = shared.view_box_id("goal", 0x1000);
        assert_ne!(second, first, "an answered box forgets its place");
        shared.report_box(plan(&second, 2));
        let _ = shared.end_frame_boxes();
        assert!(shared.end_frame_boxes());
        let third = shared.view_box_id("goal", 0x1000);
        assert_ne!(third, second, "a box no frame reported forgets its place");
        assert_eq!(third, "goal-3");
        assert_eq!(
            shared.view_box_id("goal", 0x2000),
            "goal-4",
            "another view of the same kind is a new box"
        );
        assert_eq!(
            Shared::new().view_box_id("goal", 0x1000),
            first,
            "each state counts its own"
        );
    }

    #[test]
    fn the_goal_prompt_says_why_the_goal_stopped_once_told() {
        let shared = Shared::new();
        assert_eq!(shared.goal_prompt(), None);
        shared.set_goal_prompt(GoalStop::Blocked);
        assert_eq!(shared.goal_prompt(), Some(GoalStop::Blocked));
        shared.set_goal_prompt(GoalStop::Paused);
        assert_eq!(shared.goal_prompt(), Some(GoalStop::Paused));
    }

    #[test]
    fn a_person_beginning_to_answer_is_a_change_the_touch_does_not_hide() {
        let shared = Shared::new();
        shared.report_box(question("q-1", None));
        assert!(shared.touch_box("q-1", None));
        assert!(!shared.touch_box("q-1", Some("a person typed notes")));
        shared.report_box(question("q-1", Some("a person typed notes")));
        assert!(shared.touch_box("q-1", Some("a person typed notes")));
    }

    #[test]
    fn an_answer_reaches_only_the_box_it_was_decided_for() {
        let shared = Shared::new();
        let answer = BoxAnswer::Question {
            id: "q-1".to_owned(),
            answers: vec![("a".to_owned(), Choice::Option(0))],
        };
        assert!(!shared.post_box_answer(answer.clone()), "no box is open");
        shared.report_box(question("q-1", None));
        assert!(shared.post_box_answer(answer.clone()));
        assert_eq!(shared.take_box_answer("q-2"), None);
        assert!(shared.box_answer_pending());
        assert_eq!(shared.take_box_answer("q-1"), Some(answer.clone()));
        assert_eq!(shared.take_box_answer("q-1"), None, "taken once");

        assert!(shared.post_box_answer(answer));
        let _ = shared.end_frame_boxes();
        assert!(shared.end_frame_boxes());
        assert!(
            !shared.box_answer_pending(),
            "a closed box drops its answer"
        );

        shared.set_box_outcome(BoxOutcome {
            id: "q-1".to_owned(),
            applied: Ok(()),
        });
        assert_eq!(shared.take_box_outcome().map(|o| o.applied), Some(Ok(())));
        assert_eq!(shared.take_box_outcome(), None);
    }

    /// A box opened over the one an answer waits for: the answer was never applied, and the
    /// poster hears so, or the covered box coming back under its own id would wait for this
    /// answer's outcome for good. The box that covers it has nothing pending.
    #[test]
    fn a_box_that_covers_the_answered_one_says_its_answer_was_not_applied() {
        let shared = Shared::new();
        shared.report_box(question("q-1", None));
        assert!(shared.post_box_answer(BoxAnswer::Question {
            id: "q-1".to_owned(),
            answers: vec![("a".to_owned(), Choice::Option(0))],
        }));
        shared.report_box(plan("plan-2", 1));
        assert!(!shared.box_answer_pending());
        let outcome = shared
            .take_box_outcome()
            .unwrap_or_else(|| panic!("an outcome says why"));
        assert_eq!(outcome.id, "q-1");
        assert!(outcome.applied.is_err(), "{outcome:?}");
        // NEGATIVE CONTROL: the same box reported again keeps its answer.
        shared.report_box(question("q-3", None));
        assert!(shared.post_box_answer(BoxAnswer::Question {
            id: "q-3".to_owned(),
            answers: vec![("a".to_owned(), Choice::Option(0))],
        }));
        shared.report_box(question("q-3", Some("a person typed notes")));
        assert!(shared.box_answer_pending());
        assert_eq!(shared.take_box_outcome(), None);
    }

    #[test]
    fn a_persons_input_after_an_answer_was_posted_withdraws_it() {
        let shared = Shared::new();
        shared.report_box(question("q-1", None));
        let answer = BoxAnswer::Question {
            id: "q-1".to_owned(),
            answers: vec![("cache".to_owned(), Choice::Option(1))],
        };
        assert!(shared.post_box_answer(answer));
        // The person's Down key reaches the TUI on the very frame that would apply it.
        shared.human_input(crate::session::now_ms() + 1);
        assert_eq!(shared.take_box_answer("q-1"), None);
        let outcome = shared
            .take_box_outcome()
            .unwrap_or_else(|| panic!("an outcome says why"));
        assert_eq!(outcome.id, "q-1");
        assert!(outcome.applied.is_err());
        assert!(
            !shared.box_answer_pending(),
            "withdrawn, never applied later"
        );
    }

    #[test]
    fn an_expected_own_turn_counts_once() {
        let shared = Shared::new();
        assert!(!shared.take_own_turn("Implement the plan."));
        shared.expect_own_turn("Implement the plan.");
        assert!(!shared.take_own_turn("something else"));
        assert!(shared.take_own_turn("Implement the plan."));
        assert!(!shared.take_own_turn("Implement the plan."));
    }

    #[test]
    fn an_own_turn_that_left_with_an_ide_context_is_still_codex_alabs() {
        let shared = Shared::new();
        shared.expect_own_turn("Implement the plan.");
        assert!(shared.is_own_turn("Implement the plan."));
        assert!(
            shared.is_own_turn("Implement the plan."),
            "asking does not use it up"
        );
        assert!(!shared.take_own_turn(
            "# Context from my IDE setup:\n\n## My request for Codex:\nsomething else"
        ));
        assert!(shared.take_own_turn(
            "# Context from my IDE setup:\n\n## My request for Codex:\nImplement the plan."
        ));
        assert!(!shared.is_own_turn("Implement the plan."));
    }

    #[test]
    fn the_waker_and_the_leave_are_whatever_the_tui_handed_over_first() {
        use std::sync::Arc;
        use std::sync::atomic::AtomicU32;
        use std::sync::atomic::Ordering;
        let shared = Shared::new();
        assert!(!shared.wake());
        assert!(!shared.leave());
        let woke = Arc::new(AtomicU32::new(0));
        let count = Arc::clone(&woke);
        assert!(shared.set_waker(Box::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
        })));
        assert!(!shared.set_waker(Box::new(|| {})));
        assert!(shared.wake());
        assert_eq!(woke.load(Ordering::SeqCst), 1);
        assert!(shared.set_leave(Box::new(|| {})));
        assert!(shared.leave());
    }
}

/// A turn newly running, or no longer running, wakes the TUI: its OSC 7501 report reads a turn
/// of a thread it does not show, whose events draw no frame of their own. A turn already
/// booked, or one already ended, wakes nothing.
#[test]
fn a_turn_starting_or_ending_wakes_the_tui_and_reads_beside_the_displayed_thread() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicU32;
    use std::sync::atomic::Ordering;
    let shared = Shared::new();
    let woke = Arc::new(AtomicU32::new(0));
    let count = Arc::clone(&woke);
    assert!(shared.set_waker(Box::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    })));
    let woken = || woke.load(Ordering::SeqCst);
    assert!(!shared.turn_running_besides(None));
    assert!(shared.turn_started("a", "t1"));
    assert_eq!(woken(), 1, "a start wakes");
    assert!(!shared.turn_started("a", "t1"));
    assert_eq!(woken(), 1, "booked already");
    assert!(shared.turn_running_besides(Some("b")));
    assert!(shared.turn_running_besides(None));
    assert!(
        !shared.turn_running_besides(Some("a")),
        "the displayed thread's own"
    );
    shared.turn_ended("a", "t1");
    assert_eq!(woken(), 2, "an end wakes");
    assert!(!shared.turn_running_besides(Some("b")));
    shared.turn_ended("a", "t1");
    assert_eq!(woken(), 2, "ended already");
    assert!(
        !shared.turn_started("a", "t1"),
        "an ended turn is not booked again"
    );
    assert_eq!(woken(), 2);
    assert!(shared.turn_started("a", "t2"));
    assert_eq!(woken(), 3);
    shared.thread_ended("a");
    assert_eq!(woken(), 4, "a stopped thread's turn ends");
    assert!(!shared.turn_running_besides(Some("b")));
    shared.thread_ended("a");
    assert_eq!(woken(), 4);
}

/// A TURN THE TUI SAW (`Shared::turn_seen`: a sub-agent's, one under a remote app-server)
/// reads beside the displayed thread in its own book (`turn_seen_besides`, which only holds a
/// report's turn), and wakes the TUI as it starts and ends, but never reaches the autopilot's
/// book, the record, the acts or the upgrade (`turn_running_besides`, `turn_running`, `unrest`,
/// `turn_running_on`). Let go where its end may not come (`turns_seen_keep`). NEGATIVE
/// CONTROLS: a start or end seen again, and a keep that drops nothing, wake nothing.
#[test]
fn a_turn_the_tui_saw_reads_beside_the_displayed_thread_only() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicU32;
    use std::sync::atomic::Ordering;
    let shared = Shared::new();
    let woke = Arc::new(AtomicU32::new(0));
    let count = Arc::clone(&woke);
    assert!(shared.set_waker(Box::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    })));
    let woken = || woke.load(Ordering::SeqCst);
    shared.turn_seen("sub", "s1", true);
    assert_eq!(woken(), 1, "a seen start wakes");
    shared.turn_seen("sub", "s1", true);
    assert_eq!(woken(), 1, "seen already");
    assert!(shared.turn_seen_besides(Some("root")));
    assert!(shared.turn_seen_besides(None));
    assert!(!shared.turn_seen_besides(Some("sub")), "its own frame");
    assert!(
        !shared.turn_running_besides(Some("root")),
        "never the autopilot's book"
    );
    assert!(!shared.turn_running());
    assert!(!shared.turn_running_on("sub"));
    assert_eq!(shared.unrest(), None, "the record is unmoved");
    shared.turn_seen("sub", "s1", false);
    assert_eq!(woken(), 2, "a seen end wakes");
    shared.turn_seen("sub", "s1", false);
    assert_eq!(woken(), 2, "ended already");
    assert!(!shared.turn_seen_besides(Some("root")));
    shared.turn_seen("sub", "s2", true);
    shared.turn_seen("other", "o1", true);
    assert_eq!(woken(), 4);
    shared.turns_seen_keep(|_| true);
    assert_eq!(woken(), 4, "nothing let go");
    shared.turns_seen_keep(|thread| thread != "sub");
    assert_eq!(woken(), 5, "a let go wakes");
    assert!(!shared.turn_seen_besides(Some("other")));
    assert!(shared.turn_seen_besides(Some("root")));
    shared.turns_seen_keep(|_| false);
    assert!(!shared.turn_seen_besides(None));
}

/// The ends the TUI's chat widget took are read by thread and turn, the newest 64 of them,
/// and a turn's start leaves its latest turn to read beside them.
#[test]
fn the_ends_the_tui_took_are_kept_by_thread_and_turn() {
    let shared = Shared::new();
    assert!(!shared.tui_handled_end("root", "t0"));
    assert_eq!(shared.latest_turn("root"), None);
    assert!(shared.turn_started("root", "t0"));
    assert_eq!(shared.latest_turn("root").as_deref(), Some("t0"));
    shared.turn_handled_end("root", "t0");
    assert!(shared.tui_handled_end("root", "t0"));
    assert!(!shared.tui_handled_end("other", "t0"), "another thread's");
    shared.turn_handled_end("root", "t1");
    shared.turn_handled_end("root", "t0");
    for n in 2..64 {
        shared.turn_handled_end("root", &format!("t{n}"));
    }
    assert!(shared.tui_handled_end("root", "t0"), "64 kept");
    shared.turn_handled_end("root", "t64");
    assert!(
        !shared.tui_handled_end("root", "t0"),
        "the oldest goes, its repeat taking no place of its own"
    );
    assert!(shared.tui_handled_end("root", "t1"));
    assert!(shared.tui_handled_end("root", "t64"));
}

#[test]
fn the_sweep_is_awaited_and_only_root_threads_are_the_autopilots() {
    let shared = Shared::new();
    assert!(!shared.wait_swept(Duration::from_millis(10)));
    shared.set_swept();
    assert!(shared.wait_swept(Duration::from_millis(10)));
    shared.observe(&frame(Some("t1"), ProgramState::Idle), 1);
    assert!(!shared.is_root_thread("t1"), "registered, not yet kept");
    shared.note_root_thread("t1");
    shared.note_root_thread("t2");
    assert!(shared.is_root_thread("t1"));
    assert!(
        !shared.is_root_thread("t2"),
        "kept, never registered by the TUI"
    );
}

#[test]
fn an_overlay_or_the_editor_takes_the_screen() {
    let shared = Shared::new();
    assert!(!shared.screen_taken());
    shared.set_overlay_open(true);
    assert!(shared.screen_taken());
    shared.set_overlay_open(false);
    let mut editing = frame(Some("t1"), ProgramState::Idle);
    editing.editor_open = true;
    shared.observe(&editing, 1);
    assert!(shared.screen_taken());
    shared.observe(&frame(Some("t1"), ProgramState::Idle), 2);
    assert!(!shared.screen_taken());
}

#[test]
fn the_release_hook_is_the_first_one_set() {
    let shared = Shared::new();
    assert!(
        shared.release_claim(Duration::from_millis(1)),
        "none set: nothing held"
    );
    assert!(shared.set_release(Box::new(|_| false)));
    assert!(!shared.set_release(Box::new(|_| true)));
    assert!(!shared.release_claim(Duration::from_millis(1)));
}

#[test]
fn terminal_context_is_read_only_under_a_proven_claim() {
    use crate::session::TerminalContext;
    let shared = Shared::new();
    assert!(!shared.context_available());
    assert_eq!(shared.terminal_context(Duration::from_millis(1)), None);
    let given = TerminalContext {
        prompt: "# Context from my terminal (aterm)\n".to_string(),
        summary: "selection 1 line".to_string(),
    };
    let answer = given.clone();
    assert!(shared.set_context_reader(Box::new(move |_| Some(answer.clone()))));
    assert!(!shared.set_context_reader(Box::new(|_| None)));
    // A reader alone is not enough: no claim, no context (and the reader is never called).
    assert!(!shared.context_available());
    assert_eq!(shared.terminal_context(Duration::from_millis(1)), None);
    shared.set_claim(Some("codex-alab@42".to_string()));
    assert!(shared.context_available());
    assert_eq!(
        shared.terminal_context(Duration::from_millis(1)),
        Some(given)
    );
    shared.set_claim(None);
    assert!(!shared.context_available());
}

#[test]
fn rest_is_broken_by_each_turn_act_or_owed_input() {
    use crate::boxes::BoxAnswer;
    use crate::session::Unrest;
    let shared = Shared::new();
    shared.observe(&frame(Some("t1"), ProgramState::Idle), 1);
    // Negative control: an idle frame with nothing standing is at rest.
    assert_eq!(shared.unrest(), None);

    assert!(shared.turn_started("t1", "turn-1"));
    assert_eq!(shared.unrest(), Some(Unrest::TurnRunning));
    // Only that thread runs one (negative control: another thread is at its point).
    assert!(shared.turn_running_on("t1"));
    assert!(!shared.turn_running_on("t2"));
    shared.turn_ended("t1", "turn-1");
    assert_eq!(shared.unrest(), None);
    assert!(!shared.turn_running_on("t1"));
    // A start seen after its own end (hooks out of order) is not counted again.
    assert!(!shared.turn_started("t1", "turn-1"));
    assert_eq!(shared.unrest(), None);
    assert!(shared.turn_started("t1", "turn-2"));
    shared.thread_ended("t1");
    assert_eq!(shared.unrest(), None, "a stopped thread runs no turn");

    {
        let _act = shared.begin_act();
        assert_eq!(shared.unrest(), Some(Unrest::Acting));
        let _second = shared.begin_act();
        assert!(shared.acting());
    }
    assert_eq!(shared.unrest(), None, "the guards' drop ends the act");

    shared.set_carry_on_owed(true);
    assert_eq!(shared.unrest(), Some(Unrest::CarryOnOwed));
    shared.set_carry_on_owed(false);

    let mut pending = frame(Some("t1"), ProgramState::Idle);
    pending.pending_user_turn = true;
    shared.observe(&pending, 2);
    assert_eq!(shared.unrest(), Some(Unrest::UserTurnPending));
    let mut queued = frame(Some("t1"), ProgramState::Idle);
    queued.queued_input = true;
    shared.observe(&queued, 3);
    assert_eq!(shared.unrest(), Some(Unrest::QueuedInput));
    shared.observe(&frame(Some("t1"), ProgramState::Idle), 4);

    shared.set_exec_pending(true);
    assert_eq!(shared.unrest(), Some(Unrest::Upgrading));
    shared.set_exec_pending(false);

    shared.report_box(crate::boxes::OpenBox {
        id: "box-1".to_string(),
        kind: crate::boxes::BoxKind::Plan { rows: Vec::new() },
        opened_ms: 0,
    });
    assert!(shared.post_box_answer(BoxAnswer::Plan {
        id: "box-1".to_string(),
        row: 0,
    }));
    assert_eq!(shared.unrest(), Some(Unrest::BoxAnswer));
    assert!(shared.withdraw_box_answer());
    assert_eq!(shared.unrest(), None);
}

#[test]
fn a_key_reads_as_a_draft_until_the_next_frame_draws_it() {
    let shared = Shared::new();
    shared.observe(&frame(Some("t1"), ProgramState::Idle), 1);
    assert!(shared.record_draft_empty());
    let generation = shared.generation();
    shared.human_input(5);
    assert!(shared.generation() != generation, "the writer is woken");
    assert!(
        !shared.record_draft_empty(),
        "a person's first key is a draft to the record before its echo"
    );
    // The last frame's composer is still empty; the conservative before-act
    // guard also sees the pending key until the next frame.
    assert!(shared.draft_empty());
    // The next frame shows the composer still empty (an arrow key): rest again.
    assert!(shared.observe(&frame(Some("t1"), ProgramState::Idle), 6));
    assert!(shared.record_draft_empty());
    // Negative control: a frame that shows a draft keeps it a draft.
    shared.human_input(7);
    let mut typed = frame(Some("t1"), ProgramState::Idle);
    typed.draft_empty = false;
    shared.observe(&typed, 8);
    assert!(!shared.record_draft_empty());
}

#[test]
fn the_write_through_is_the_writers_or_nothing() {
    let shared = Shared::new();
    // No record kept (outside aterm): nothing to write, so nothing holds an act back.
    assert!(shared.flush_record());
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let seen = std::sync::Arc::clone(&calls);
    assert!(shared.set_flush(Box::new(move || {
        seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        false
    })));
    assert!(
        !shared.set_flush(Box::new(|| true)),
        "only the first is kept"
    );
    assert!(!shared.flush_record(), "a failed write is told");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

/// The harness word is the TUI's chrome alone: a change wakes a frame, an unchanged word wakes
/// nothing, and neither moves the record's or the upgrade's generation nor the record's
/// Autopilot.
#[test]
fn the_harness_word_wakes_the_tui_and_leaves_the_record_alone() {
    use crate::session::Autopilot;
    use crate::session::HarnessWord;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU32;
    use std::sync::atomic::Ordering;
    let shared = Shared::new();
    let woke = Arc::new(AtomicU32::new(0));
    let count = Arc::clone(&woke);
    assert!(shared.set_waker(Box::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    })));
    shared.set_autopilot(Autopilot::Passive);
    let woken_before = woke.load(Ordering::SeqCst);
    let (generation, upgrade) = (shared.generation(), shared.upgrade_generation());
    assert_eq!(shared.harness_word(), HarnessWord::Unknown);
    for (word, woken) in [
        (HarnessWord::Off, 1),
        (HarnessWord::Off, 1),
        (HarnessWord::Cruise, 2),
        (HarnessWord::Land, 3),
        (HarnessWord::Land, 3),
        (HarnessWord::Unknown, 4),
    ] {
        shared.set_harness_word(word);
        assert_eq!(shared.harness_word(), word);
        assert_eq!(
            woke.load(Ordering::SeqCst) - woken_before,
            woken,
            "{word:?}"
        );
    }
    assert_eq!(shared.generation(), generation);
    assert_eq!(shared.upgrade_generation(), upgrade);
    assert_eq!(shared.autopilot(), Autopilot::Passive);
}

/// The Autopilot and its word are one cell: published together they are read together
/// ([`Shared::harness`]), the Autopilot alone keeps the word, and the word alone keeps the
/// Autopilot. Only the Autopilot moves the generations. [`Shared::set_acting_word`] moves the
/// word under an Active Autopilot only, deciding in the same store.
#[test]
fn the_autopilot_and_its_word_publish_as_one() {
    use crate::session::Autopilot;
    use crate::session::HarnessWord;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU32;
    use std::sync::atomic::Ordering;
    let shared = Shared::new();
    assert_eq!(shared.harness(), (Autopilot::Off, HarnessWord::Unknown));
    let woke = Arc::new(AtomicU32::new(0));
    let count = Arc::clone(&woke);
    assert!(shared.set_waker(Box::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    })));
    let woken = || woke.load(Ordering::SeqCst);

    let generation = shared.generation();
    shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Land);
    assert_eq!(shared.harness(), (Autopilot::Active, HarnessWord::Land));
    assert_eq!(shared.generation(), generation.wrapping_add(1));
    assert_eq!(woken(), 1);
    // The word alone: a frame, no generation.
    shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Cruise);
    assert_eq!(shared.harness(), (Autopilot::Active, HarnessWord::Cruise));
    assert_eq!(shared.generation(), generation.wrapping_add(1));
    assert_eq!(woken(), 2);
    // Nothing moved: nothing asked.
    shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Cruise);
    assert_eq!(woken(), 2);
    // The Autopilot alone keeps the word; the word alone keeps the Autopilot.
    shared.set_autopilot(Autopilot::Passive);
    assert_eq!(shared.harness(), (Autopilot::Passive, HarnessWord::Cruise));
    shared.set_harness_word(HarnessWord::Off);
    assert_eq!(shared.harness(), (Autopilot::Passive, HarnessWord::Off));
    assert_eq!(shared.generation(), generation.wrapping_add(2));

    // A mode moved under the acting claim never relabels a passive session.
    let before = woken();
    shared.set_acting_word(HarnessWord::Land);
    assert_eq!(shared.harness(), (Autopilot::Passive, HarnessWord::Off));
    assert_eq!(woken(), before);
    shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Off);
    shared.set_acting_word(HarnessWord::Land);
    assert_eq!(shared.harness(), (Autopilot::Active, HarnessWord::Land));
    assert_eq!(woken(), before + 2);
    shared.set_acting_word(HarnessWord::Land);
    assert_eq!(woken(), before + 2);

    // Every pair round-trips through the one cell.
    for autopilot in [Autopilot::Off, Autopilot::Passive, Autopilot::Active] {
        for word in [
            HarnessWord::Unknown,
            HarnessWord::Cruise,
            HarnessWord::Land,
            HarnessWord::Off,
            HarnessWord::HarnessOff,
            HarnessWord::AgentOff,
            HarnessWord::Unhosted,
        ] {
            shared.set_autopilot_with_word(autopilot, word);
            assert_eq!(shared.harness(), (autopilot, word));
            assert_eq!(shared.autopilot(), autopilot);
            assert_eq!(shared.harness_word(), word);
        }
    }
}

/// A waker that counts the frames asked for.
fn counting_waker(shared: &Shared) -> std::sync::Arc<std::sync::atomic::AtomicU32> {
    use std::sync::atomic::Ordering;
    let woke = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let count = std::sync::Arc::clone(&woke);
    assert!(shared.set_waker(Box::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    })));
    woke
}

/// THE LINE'S TASK FACE IS THE PRIMARY THREAD'S (gap 12 of the parity sweep, 2026-10-29): each
/// thread's face is kept, and the line reads the one the TUI's primary is, following the frame
/// that moves it. NEGATIVE CONTROL: a face of another thread is never the primary's.
#[test]
fn the_lines_task_face_is_the_primary_threads() {
    use crate::session::TaskFace;
    let shared = Shared::new();
    assert_eq!(shared.task_face(), TaskFace::Open, "no primary yet");
    shared.set_task_face("t1", TaskFace::Landed);
    shared.set_task_face("t2", TaskFace::Theirs);
    assert_eq!(shared.task_face(), TaskFace::Open, "no primary yet");
    shared.observe(&frame(Some("t2"), ProgramState::Idle), 10);
    assert_eq!(shared.task_face(), TaskFace::Theirs);
    shared.observe(&frame(Some("t1"), ProgramState::Idle), 20);
    assert_eq!(shared.task_face(), TaskFace::Landed);
    shared.observe(&frame(Some("t3"), ProgramState::Idle), 30);
    assert_eq!(shared.task_face(), TaskFace::Open, "t3 holds none");
}

/// AN OPEN FACE LEAVES NO ENTRY, AND ONLY A CHANGE ASKS FOR A FRAME: the TUI's chrome alone reads
/// the face, so a move wakes it and a repeat does not, and neither moves the record's or the
/// upgrade's generation. NEGATIVE CONTROL: an Open over none is no change (an entry kept for
/// Open would be one).
#[test]
fn an_open_face_leaves_no_entry_and_wakes_only_on_change() {
    use crate::session::TaskFace;
    use std::sync::atomic::Ordering;
    let shared = Shared::new();
    shared.observe(&frame(Some("t1"), ProgramState::Idle), 10);
    let woke = counting_waker(&shared);
    let (generation, upgrade) = (shared.generation(), shared.upgrade_generation());
    for (face, woken) in [
        (TaskFace::Open, 0),
        (TaskFace::Landed, 1),
        (TaskFace::Landed, 1),
        (TaskFace::Theirs, 2),
        (TaskFace::Open, 3),
        (TaskFace::Open, 3),
    ] {
        shared.set_task_face("t1", face);
        assert_eq!(shared.task_face(), face);
        assert_eq!(woke.load(Ordering::SeqCst), woken, "{face:?}");
    }
    assert_eq!(shared.generation(), generation);
    assert_eq!(shared.upgrade_generation(), upgrade);
}

/// THE LAND TIMER ROUND-TRIPS, NONE BEING 0, AND ONLY A CHANGE ASKS FOR A FRAME. NEGATIVE
/// CONTROL: the same timer again asks for nothing.
#[test]
fn land_at_round_trips_and_none_is_zero() {
    use std::sync::atomic::Ordering;
    let shared = Shared::new();
    let woke = counting_waker(&shared);
    assert_eq!(shared.land_at(), None);
    for (at, woken) in [
        (None, 0),
        (Some(1_767_000_000), 1),
        (Some(1_767_000_000), 1),
        (Some(1_767_003_600), 2),
        (None, 3),
        (Some(0), 3),
    ] {
        shared.set_land_at(at);
        assert_eq!(shared.land_at(), at.filter(|&at| at > 0));
        assert_eq!(woke.load(Ordering::SeqCst), woken, "{at:?}");
    }
}

#[test]
fn renewal_custody_survives_its_accepted_turn_but_not_new_native_work() {
    use crate::session::{NativeOrigin, NativePhase, OwedAct, AccountHandoff};
    let marker = OwedAct {
        thread: "thread".into(),
        native_origin: Some(NativeOrigin { turn_id: "wall".into(), phase: NativePhase::Complete }),
        renewal: Some(AccountHandoff { purpose: crate::account_handoff::Purpose::Login, destination: None, openai_scope: true, local: false, account: "a".repeat(64), user: "b".repeat(64),
            receipt: Some("c".repeat(32)), turn: Some("wall".into()), accepted: false, starts: 1, goal_id: None }),
        ..Default::default()
    };
    let shared = Shared::new();
    assert!(shared.turn_started("thread", "wall"));
    shared.turn_ended("thread", "wall");
    assert!(shared.publish_renewal(shared.upgrade_generation(), marker.clone()));
    assert!(shared.admit_recovered_turn("thread", "wall"));
    assert!(shared.turn_started("thread", "wall"));
    assert!(!shared.turn_started("thread", "wall"), "duplicate notification is inert");
    shared.set_owed_act(None);
    assert!(shared.owed_act().and_then(|owed| owed.renewal).is_some_and(|r| r.accepted));
    assert!(!shared.clear_renewal(&"d".repeat(32)));
    assert!(shared.clear_renewal(&"c".repeat(32)));
    assert!(shared.owed_act().is_none());

    let shared = Shared::new();
    let generation = shared.upgrade_generation();
    assert!(shared.publish_renewal(generation, marker.clone()));
    shared.turn_started("thread", "a-new-human-turn");
    assert!(shared.owed_act().is_none());
    assert!(!shared.publish_renewal(generation, marker));
}
