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
        last_agent_message: Some("ATERM-TASK-DONE".into()),
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
            ..Default::default()
        });
    }
    // The last one noted (the sweep's order) stands for the session.
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

mod boxes {
    use crate::boxes::BoxAnswer;
    use crate::boxes::BoxKind;
    use crate::boxes::BoxOutcome;
    use crate::boxes::Choice;
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
    // The before-act checks read the composer itself (the human grace holds them).
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
