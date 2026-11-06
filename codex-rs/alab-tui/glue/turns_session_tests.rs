// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The OSC 7501 report as aterm counts its turns (`ALAB(turns)`), at the session level: a real
//! `ChatWidget` driven by the app-server's notifications, its frames (`pre_draw_tick`) and the
//! live `TurnStarted` arm reaching the two hooks, the words they write taken under a test's
//! scope as the bytes upstream's writer puts on the wire, and read by aterm's rule
//! (`codex_alab_tui::turns::aterm_turns`). Each case is set beside what upstream's report said
//! at the same frames (`desired_program_status`, a word dropped when it stands), which aterm
//! counted wrong.

use super::tests::turn;
use crate::chatwidget::ReplayKind;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use crate::terminal_program_status::ProgramStatus;
use codex_alab_tui::session::ProgramState;
use codex_alab_tui::turns;
use codex_app_server_protocol::McpServerStartupState;
use codex_app_server_protocol::McpServerStatusUpdatedNotification;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::ThreadId;
use pretty_assertions::assert_eq;

/// What upstream's report put on the wire, frame by frame: the words it said, each dropped
/// when it stood already.
#[derive(Default)]
struct Upstream {
    bytes: String,
    last: Option<ProgramState>,
}

impl Upstream {
    fn frame(&mut self, chat: &super::ChatWidget) {
        let state = chat.alab_program_state();
        if self.last != Some(state) {
            self.bytes
                .push_str(&turns::sequence(state).unwrap_or_default());
            self.last = Some(state);
        }
    }
}

/// One frame of the real widget: its hook writes the report, and upstream's words are kept
/// beside.
fn frame(chat: &mut super::ChatWidget, upstream: &mut Upstream) {
    chat.pre_draw_tick();
    upstream.frame(chat);
}

/// One MCP server's startup update for the widget's thread.
fn mcp(chat: &mut super::ChatWidget, name: &str, status: McpServerStartupState) {
    let thread_id = chat.thread_id().map(|id| id.to_string());
    chat.handle_server_notification(
        ServerNotification::McpServerStatusUpdated(McpServerStatusUpdatedNotification {
            thread_id,
            name: name.to_owned(),
            status,
            error: None,
            failure_reason: None,
        }),
        /*replay_kind*/ None,
    );
}

/// O1 THROUGH THE WIDGET: while an MCP server starts, upstream's report says `working`
/// (RED: aterm counted a turn nobody began); the hook writes `idle` throughout. CONTROL: the
/// person's turn that follows is one turn, and its `working` is written.
#[tokio::test]
async fn mcp_startup_writes_no_working_and_begins_no_turn() {
    let scope = turns::scoped();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.thread_id = Some(ThreadId::new());
    let mut upstream = Upstream::default();
    chat.set_mcp_startup_expected_servers(["slow".to_owned()]);
    frame(&mut chat, &mut upstream);
    mcp(&mut chat, "slow", McpServerStartupState::Starting);
    assert_eq!(
        chat.desired_program_status(),
        ProgramStatus::Working,
        "upstream says working while the server starts"
    );
    assert!(
        chat.alab_report_frame(chat.desired_program_status())
            .startup_only
    );
    frame(&mut chat, &mut upstream);
    frame(&mut chat, &mut upstream);
    mcp(&mut chat, "slow", McpServerStartupState::Ready);
    assert!(chat.mcp_startup_status.is_none(), "the startup settled");
    frame(&mut chat, &mut upstream);
    assert_eq!(turns::aterm_turns(&upstream.bytes), 1, "RED: upstream");
    let bytes = scope.take();
    assert_eq!(bytes, turns::sequence(ProgramState::Idle).unwrap());
    assert_eq!(turns::aterm_turns(&bytes), 0);

    chat.queue_user_message("first".into());
    assert!(chat.input_queue.user_turn_pending_start, "submitted");
    turn(&mut chat, "t1", TurnStatus::InProgress);
    frame(&mut chat, &mut upstream);
    turn(&mut chat, "t1", TurnStatus::Completed);
    frame(&mut chat, &mut upstream);
    let bytes = format!(
        "{}{}",
        turns::sequence(ProgramState::Idle).unwrap(),
        scope.take()
    );
    assert_eq!(turns::aterm_turns(&bytes), 1, "CONTROL: the person's turn");
}

/// NEGATIVE CONTROL for O1: a turn that runs while a server still starts is work, so its
/// frames are not the startup's and say `working`.
#[tokio::test]
async fn a_turn_during_mcp_startup_is_reported_as_work() {
    let scope = turns::scoped();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.thread_id = Some(ThreadId::new());
    let mut upstream = Upstream::default();
    chat.set_mcp_startup_expected_servers(["slow".to_owned()]);
    mcp(&mut chat, "slow", McpServerStartupState::Starting);
    frame(&mut chat, &mut upstream);
    chat.queue_user_message("first".into());
    turn(&mut chat, "t1", TurnStatus::InProgress);
    assert!(
        !chat
            .alab_report_frame(chat.desired_program_status())
            .startup_only
    );
    frame(&mut chat, &mut upstream);
    let bytes = scope.take();
    assert!(
        bytes.ends_with(&turns::sequence(ProgramState::Working).unwrap()),
        "{bytes:?}"
    );
    assert_eq!(turns::aterm_turns(&bytes), 1);
}

/// O2 THROUGH THE WIDGET: a message queued while a turn runs is sent the moment the turn ends,
/// and upstream's report stood at `working` from the first submission across both turns (RED:
/// one turn counted). The live `TurnStarted` arm writes the second turn's edge; the first,
/// whose begin a frame showed while it waited for the server, gets no second one.
#[tokio::test]
async fn a_queued_message_sent_as_a_turn_ends_begins_its_own_turn() {
    let scope = turns::scoped();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.thread_id = Some(ThreadId::new());
    let mut upstream = Upstream::default();
    frame(&mut chat, &mut upstream);
    chat.queue_user_message("first".into());
    // A frame shows the submission waiting for the server: its begin.
    frame(&mut chat, &mut upstream);
    turn(&mut chat, "t1", TurnStatus::InProgress);
    frame(&mut chat, &mut upstream);
    chat.queue_user_message("queued follow-up".into());
    assert!(!chat.input_queue.queued_user_messages.is_empty(), "queued");
    turn(&mut chat, "t1", TurnStatus::Completed);
    assert!(
        chat.input_queue.user_turn_pending_start,
        "the follow-up was sent as the turn ended"
    );
    // A frame shows the follow-up waiting: the report stands at `working`.
    frame(&mut chat, &mut upstream);
    turn(&mut chat, "t2", TurnStatus::InProgress);
    frame(&mut chat, &mut upstream);
    turn(&mut chat, "t2", TurnStatus::Completed);
    frame(&mut chat, &mut upstream);
    assert_eq!(turns::aterm_turns(&upstream.bytes), 1, "RED: upstream");
    let bytes = scope.take();
    assert_eq!(turns::aterm_turns(&bytes), 2, "{bytes:?}");
}

/// O2 under work running on: after a turn leaves a background terminal running, the report
/// stays `working`; the person's next message begins its own turn (RED: upstream). NEGATIVE
/// CONTROL: a turn the TUI did not submit (a goal's continuation) under the same standing work
/// writes nothing.
#[tokio::test]
async fn under_standing_work_only_a_submitted_turn_begins_one() {
    use super::super::exec_state::UnifiedExecProcessSummary;
    let scope = turns::scoped();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.thread_id = Some(ThreadId::new());
    let mut upstream = Upstream::default();
    frame(&mut chat, &mut upstream);
    chat.queue_user_message("first".into());
    turn(&mut chat, "t1", TurnStatus::InProgress);
    frame(&mut chat, &mut upstream);
    chat.unified_exec_processes.push(UnifiedExecProcessSummary {
        key: "proc-1".to_owned(),
        call_id: "call-1".to_owned(),
        command_display: "npm run dev".to_owned(),
        recent_chunks: Vec::new(),
    });
    turn(&mut chat, "t1", TurnStatus::Completed);
    frame(&mut chat, &mut upstream);
    assert_eq!(
        chat.alab_program_state(),
        ProgramState::Working,
        "work runs on"
    );

    // Nobody submitted this one: the work runs on, no edge.
    assert!(!chat.input_queue.user_turn_pending_start);
    turn(&mut chat, "t2", TurnStatus::InProgress);
    frame(&mut chat, &mut upstream);
    turn(&mut chat, "t2", TurnStatus::Completed);
    frame(&mut chat, &mut upstream);
    assert_eq!(
        turns::aterm_turns(&scope.take()),
        1,
        "CONTROL: no turn for it"
    );

    // The person's message under the same standing work.
    chat.queue_user_message("next".into());
    assert!(chat.input_queue.user_turn_pending_start, "submitted");
    frame(&mut chat, &mut upstream);
    turn(&mut chat, "t3", TurnStatus::InProgress);
    frame(&mut chat, &mut upstream);
    assert_eq!(turns::aterm_turns(&upstream.bytes), 1, "RED: upstream");
    let bytes = scope.take();
    let edge = [ProgramState::Idle, ProgramState::Working]
        .map(|w| turns::sequence(w).unwrap())
        .concat();
    assert_eq!(bytes, edge, "the person's turn's own edge");
}

/// A REPLAYED TURN START (a thread snapshot, `/resume`, a thread switch) is no live start: the
/// `TurnStarted` arm's hook sits in the live branch only, so a replay writes no edge of its own
/// and its frames say what the thread does.
#[tokio::test]
async fn a_replayed_turn_start_writes_no_edge() {
    let scope = turns::scoped();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.thread_id = Some(ThreadId::new());
    let mut upstream = Upstream::default();
    frame(&mut chat, &mut upstream);
    scope.take();
    let thread_id = chat
        .thread_id()
        .map(|id| id.to_string())
        .unwrap_or_default();
    chat.handle_server_notification(
        ServerNotification::TurnStarted(codex_app_server_protocol::TurnStartedNotification {
            thread_id,
            turn: codex_app_server_protocol::Turn {
                id: "replayed".to_owned(),
                root_turn_id: None,
                items_view: codex_app_server_protocol::TurnItemsView::Full,
                items: Vec::new(),
                status: TurnStatus::InProgress,
                error: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
            },
        }),
        Some(ReplayKind::ThreadSnapshot),
    );
    assert_eq!(scope.take(), "", "no edge at the replay itself");
    frame(&mut chat, &mut upstream);
    assert_eq!(
        scope.take(),
        turns::sequence(ProgramState::Working).unwrap(),
        "the frame says the thread works"
    );
}

/// A THREAD SWITCH BACK TO A RUNNING TURN through the widgets: a switch replaces the
/// `ChatWidget` and replays the thread's snapshot, while the report is the process's. Thread
/// A's turn runs (booked in `Shared`, as the autopilot's `on_turn_start` books it); the TUI
/// shows thread B at rest, then A's replayed snapshot with the turn still running, then B
/// again until A's turn ends, which wakes the TUI. Upstream's report (B's `idle`, then A's
/// `working`) counted a turn nobody began (RED); here B's frames stand in A's turn, and B's
/// rest is written once that turn ends. NEGATIVE CONTROL: B's frames read another thread's
/// turn only while one is booked, and A's own turn is not one.
#[tokio::test]
async fn a_switch_back_to_a_running_turn_begins_no_turn() {
    use codex_alab_tui::boxes::Shared;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU32;
    use std::sync::atomic::Ordering;
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let _boxes = codex_alab_tui::boxes::scoped(shared);
    let woke = Arc::new(AtomicU32::new(0));
    let count = Arc::clone(&woke);
    assert!(shared.set_waker(Box::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    })));
    let scope = turns::scoped();
    let mut upstream = Upstream::default();
    let a_id = ThreadId::new();
    let a_key = a_id.to_string();
    let (mut a, _a_tx, _a_rx, _a_op) = make_chatwidget_manual_with_sender().await;
    a.thread_id = Some(a_id);
    frame(&mut a, &mut upstream);
    a.queue_user_message("long work".into());
    assert!(shared.turn_started(&a_key, "t1"));
    turn(&mut a, "t1", TurnStatus::InProgress);
    frame(&mut a, &mut upstream);
    assert!(!a.alab_report_frame(a.desired_program_status()).elsewhere);

    // The switch to B, a thread at rest.
    let (mut b, _b_tx, _b_rx, _b_op) = make_chatwidget_manual_with_sender().await;
    b.thread_id = Some(ThreadId::new());
    assert!(!turns::in_turn(b.alab_program_state()), "B rests");
    assert!(b.alab_report_frame(b.desired_program_status()).elsewhere);
    frame(&mut b, &mut upstream);
    frame(&mut b, &mut upstream);

    // Back to A: its snapshot replayed, the turn still running.
    let (mut a2, _a2_tx, _a2_rx, _a2_op) = make_chatwidget_manual_with_sender().await;
    a2.thread_id = Some(a_id);
    a2.handle_server_notification(
        ServerNotification::TurnStarted(codex_app_server_protocol::TurnStartedNotification {
            thread_id: a_key.clone(),
            turn: codex_app_server_protocol::Turn {
                id: "t1".to_owned(),
                root_turn_id: None,
                items_view: codex_app_server_protocol::TurnItemsView::Full,
                items: Vec::new(),
                status: TurnStatus::InProgress,
                error: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
            },
        }),
        Some(ReplayKind::ThreadSnapshot),
    );
    assert_eq!(a2.alab_program_state(), ProgramState::Working, "A runs");
    frame(&mut a2, &mut upstream);

    // B again, until A's turn ends.
    frame(&mut b, &mut upstream);
    let before = woke.load(Ordering::SeqCst);
    shared.turn_ended(&a_key, "t1");
    assert_eq!(
        woke.load(Ordering::SeqCst),
        before + 1,
        "the end wakes the TUI"
    );
    assert!(!b.alab_report_frame(b.desired_program_status()).elsewhere);
    frame(&mut b, &mut upstream);

    assert_eq!(turns::aterm_turns(&upstream.bytes), 2, "RED: upstream");
    let bytes = scope.take();
    assert_eq!(turns::aterm_turns(&bytes), 1, "{bytes:?}");
    let rest = turns::sequence(b.alab_program_state()).unwrap();
    assert!(
        bytes.ends_with(&rest),
        "B's rest once A's turn ended: {bytes:?}"
    );
}

/// A TURN'S END IS BOOKED AFTER ITS EDGE: the widget books the end of a turn it takes
/// (`Shared::turn_handled_end`) only after the `turn/started` it took first wrote that turn's
/// edge, so a landing that waits for the booking follows every edge of its land turn on the
/// wire. NEGATIVE CONTROL: a turn whose start the widget took, and whose end it has not, reads
/// unbooked.
#[tokio::test]
async fn a_turns_end_is_booked_after_its_edge() {
    use codex_alab_tui::boxes::Shared;
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let _boxes = codex_alab_tui::boxes::scoped(shared);
    let scope = turns::scoped();
    let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    let id = ThreadId::new();
    let key = id.to_string();
    chat.thread_id = Some(id);
    let mut upstream = Upstream::default();
    frame(&mut chat, &mut upstream);
    scope.take();
    turn(&mut chat, "t1", TurnStatus::InProgress);
    assert!(!shared.tui_handled_end(&key, "t1"), "its start only");
    let edge = scope.take();
    assert_eq!(
        turns::aterm_turns(&edge),
        1,
        "the start wrote its edge: {edge:?}"
    );
    turn(&mut chat, "t1", TurnStatus::Completed);
    assert!(shared.tui_handled_end(&key, "t1"));
    assert!(!shared.tui_handled_end(&key, "t2"));
}

/// A RESUME OVER A PAUSED GOAL: "Resume paused goal?" opens with nothing running (a resume,
/// aterm's relaunch, the live upgrade's re-exec), and codex-alab answers it a grace later, so
/// its report stays at rest while it stands: aterm counts no turn, and a landed task stays
/// landed. A `blocked` after that rest would begin a turn as aterm counts turns, and aterm
/// would reopen the landing with nobody to land it again.
#[tokio::test]
async fn a_resume_over_a_paused_goal_begins_no_turn() {
    use codex_alab_tui::boxes::BoxKind;
    use codex_alab_tui::boxes::GoalStop;
    use codex_alab_tui::boxes::Shared;
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let _boxes = codex_alab_tui::boxes::scoped(shared);
    let scope = turns::scoped();
    let (mut chat, _tx, mut rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.thread_id = Some(ThreadId::new());
    let mut upstream = Upstream::default();
    frame(&mut chat, &mut upstream);
    shared.set_goal_prompt(GoalStop::Paused);
    chat.show_resume_paused_goal_prompt(ThreadId::new(), "Finish the paused goal.".to_owned());
    while rx.try_recv().is_ok() {}
    frame(&mut chat, &mut upstream);
    frame(&mut chat, &mut upstream);
    assert!(
        shared
            .open_box()
            .is_some_and(|open| matches!(open.kind, BoxKind::Goal { .. })),
        "the goal box stands, reported to codex-alab"
    );
    let bytes = scope.take();
    assert_eq!(bytes, turns::sequence(ProgramState::Idle).unwrap());
    assert_eq!(turns::aterm_turns(&bytes), 0, "{bytes:?}");
}

/// THE PLAN BOX KEEPS THE REPORT AT REST: Plan mode's "Implement this plan?" opens as a live
/// turn ends, before the next frame (`maybe_prompt_plan_implementation`, in the `TurnCompleted`
/// arm), and the report says that turn's `done`, as upstream's does: aterm counts the one turn,
/// takes a person's own exchange as `theirs` (a `left-to-person@v1` act, which aterm takes only
/// at rest), and a box left to a person shows through codex-alab's `plan:` attention. A
/// person's key acknowledges the `done`, so declining the plan leaves `idle`.
#[tokio::test]
async fn the_plan_box_at_a_turns_end_keeps_the_report_at_rest() {
    use codex_alab_tui::boxes::Shared;
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let _boxes = codex_alab_tui::boxes::scoped(shared);
    let scope = turns::scoped();
    let (mut chat, _tx, mut rx, _op_rx) = make_chatwidget_manual_with_sender().await;
    chat.thread_id = Some(ThreadId::new());
    let mut upstream = Upstream::default();
    frame(&mut chat, &mut upstream);
    chat.queue_user_message("plan the landing".into());
    turn(&mut chat, "t1", TurnStatus::InProgress);
    frame(&mut chat, &mut upstream);
    turn(&mut chat, "t1", TurnStatus::Completed);
    // In Plan mode the `TurnCompleted` arm opens it here, before any frame.
    chat.open_plan_implementation_prompt();
    while rx.try_recv().is_ok() {}
    frame(&mut chat, &mut upstream);
    assert!(chat.has_active_view(), "the plan box stands");
    let bytes = scope.take();
    assert_eq!(turns::aterm_turns(&bytes), 1, "the planning turn only: {bytes:?}");
    assert!(
        bytes.ends_with(&turns::sequence(ProgramState::Done).unwrap()),
        "the turn's done, at rest: {bytes:?}"
    );

    // A person's key, as the App takes it: acknowledged, then given to the box.
    chat.acknowledge_program_status();
    chat.handle_key_event(KeyEvent::from(KeyCode::Esc));
    assert!(!chat.has_active_view(), "the person closed the plan box");
    assert_eq!(chat.desired_program_status(), ProgramStatus::Idle);
    frame(&mut chat, &mut upstream);
    let bytes = scope.take();
    assert_eq!(bytes, turns::sequence(ProgramState::Idle).unwrap());
    assert_eq!(turns::aterm_turns(&bytes), 0);
}
