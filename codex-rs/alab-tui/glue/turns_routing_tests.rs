// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The turns the TUI routes, booked for the OSC 7501 report (`ALAB(turns)`, parity-5, T; CONTRACT
//! §6 "a thread switch"): a real `App` with the embedded app-server's session, its routing of
//! the app-server's notifications reaching the `routed` and `stream` hooks, its frames'
//! `observe`, and the report its chat widget writes (`alab_report_program_status`, onto a
//! test's tape). The autopilot books only its own (root) threads' turns, in process; a
//! sub-agent's turn, and every turn under a remote app-server, reach this process only as
//! notifications, and they hold the report's turn without beginning one.

use super::observe;
use crate::app::tests::make_test_app_with_channels;
use codex_alab_tui::boxes;
use codex_alab_tui::session::ProgramState;
use codex_alab_tui::turns;
use codex_alab_tui::turns::Frame;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadClosedNotification;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::ThreadId;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

fn turn(id: &str, status: TurnStatus) -> Turn {
    Turn {
        id: id.to_owned(),
        root_turn_id: None,
        items_view: TurnItemsView::Full,
        items: Vec::new(),
        status,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }
}

fn started(thread: ThreadId, id: &str) -> AppServerEvent {
    AppServerEvent::ServerNotification(Box::new(ServerNotification::TurnStarted(
        TurnStartedNotification {
            thread_id: thread.to_string(),
            turn: turn(id, TurnStatus::InProgress),
        },
    )))
}

fn completed(thread: ThreadId, id: &str) -> AppServerEvent {
    AppServerEvent::ServerNotification(Box::new(ServerNotification::TurnCompleted(
        TurnCompletedNotification {
            thread_id: thread.to_string(),
            turn: turn(id, TurnStatus::Completed),
        },
    )))
}

/// The report the shown thread's frame writes now, onto the test's tape.
fn frame(app: &crate::app::App) {
    let chat = &app.chat_widget;
    assert!(chat.alab_report_program_status(chat.desired_program_status()));
}

/// A frame of another widget, showing a thread whose turn runs (the running sub-agent, shown).
fn shown_running() {
    let result = turns::frame(
        Frame {
            state: ProgramState::Working,
            startup_only: false,
            rest: ProgramState::Idle,
            submitted: false,
            elsewhere: false,
            held: false,
        },
        |_| unreachable!("scoped"),
    );
    assert!(matches!(result, Some(Ok(()))), "{result:?}");
}

fn closed(thread: ThreadId) -> AppServerEvent {
    AppServerEvent::ServerNotification(Box::new(ServerNotification::ThreadClosed(
        ThreadClosedNotification {
            thread_id: thread.to_string(),
        },
    )))
}

/// A SUB-AGENT'S TURN THE TUI ROUTES HOLDS THE REPORT'S TURN AND BEGINS NONE: the TUI shows its
/// primary thread at rest while a sub-agent it routes starts a turn, which writes nothing
/// (nobody in this process carries that turn, and aterm would reopen a landing at its begin
/// that nobody then lands again: parity-5 review, T-subagent-turn-reopens-landing; RED with
/// the seen turn read as the autopilot's own). The person looks at the running sub-agent and
/// back at the primary: the report stands in that turn, so the switch back begins none, and
/// the rest comes once the sub-agent's turn ends, which wakes the TUI. A seen turn whose end may
/// never reach the hook is let go: the thread's close, a lagged or disconnected stream, a
/// thread the TUI no longer routes. NEGATIVE CONTROLS: the shown thread's own turn is no
/// other's, and a thread the TUI does not route books nothing. Without the `routed` hook (RED)
/// the switch back to the primary wrote its rest, and a second look at the sub-agent wrote
/// `working` again, a turn aterm counted though nobody began one.
#[tokio::test]
async fn a_routed_sub_agent_turn_holds_the_reports_turn_and_begins_none()
-> color_eyre::eyre::Result<()> {
    let shared: &'static boxes::Shared = Box::leak(Box::new(boxes::Shared::new()));
    let _boxes = boxes::scoped(shared);
    let tape = turns::scoped();
    let woke = Arc::new(AtomicU32::new(0));
    let count = Arc::clone(&woke);
    assert!(shared.set_waker(Box::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    })));
    let (mut app, _events, _ops) = make_test_app_with_channels().await;
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let thread = server.start_thread(&app.config).await?;
    let primary = thread.session.thread_id;
    app.active_thread_id = Some(primary);
    app.primary_thread_id = Some(primary);
    app.chat_widget.handle_thread_session_quiet(thread.session);
    let shown = primary.to_string();
    frame(&app);
    let rest = tape.take();
    assert_eq!(
        turns::aterm_turns(&rest),
        0,
        "the primary thread rests: {rest:?}"
    );
    assert!(!rest.is_empty());

    // A sub-agent the TUI routes (its event channel stands) starts a turn: nothing is written.
    let sub = ThreadId::new();
    app.ensure_thread_channel(sub);
    let before = woke.load(Ordering::SeqCst);
    app.handle_app_server_event(&server, started(sub, "s1"))
        .await;
    assert_eq!(
        woke.load(Ordering::SeqCst),
        before + 1,
        "the start wakes the TUI"
    );
    assert!(shared.turn_seen_besides(Some(&shown)));
    frame(&app);
    frame(&app);
    let begun = tape.take();
    assert_eq!(
        turns::aterm_turns(&format!("{rest}{begun}")),
        0,
        "a seen turn begins none under the shown rest: {begun:?}"
    );
    // NEGATIVE CONTROL: the sub-agent's own frame reads no other thread's turn.
    assert!(!shared.turn_seen_besides(Some(&sub.to_string())));
    // The autopilot's book, the record, the acts and the upgrade never read it.
    assert!(!shared.turn_running_besides(Some(&shown)));
    assert!(!shared.turn_running());
    // A frame's look keeps a thread the TUI routes.
    observe(&app);
    assert!(shared.turn_seen_besides(Some(&shown)));

    // The person looks at the running sub-agent, then back at the primary at rest, and again.
    shown_running();
    frame(&app);
    shown_running();
    frame(&app);
    let looked = tape.take();
    assert_eq!(
        turns::aterm_turns(&format!("{rest}{looked}")),
        1,
        "one look at a running turn, no second: {looked:?}"
    );
    assert_eq!(
        looked,
        turns::sequence(ProgramState::Working).unwrap_or_default()
    );

    // Its end: the rest is said again.
    let before = woke.load(Ordering::SeqCst);
    app.handle_app_server_event(&server, completed(sub, "s1"))
        .await;
    assert_eq!(
        woke.load(Ordering::SeqCst),
        before + 1,
        "the end wakes the TUI"
    );
    assert!(!shared.turn_seen_besides(Some(&shown)));
    frame(&app);
    let ended = tape.take();
    assert!(
        !ended.is_empty() && turns::aterm_turns(&ended) == 0,
        "the rest: {ended:?}"
    );

    // The thread's close lets its turn go.
    app.handle_app_server_event(&server, started(sub, "s2"))
        .await;
    assert!(shared.turn_seen_besides(Some(&shown)));
    app.handle_app_server_event(&server, closed(sub)).await;
    assert!(
        !shared.turn_seen_besides(Some(&shown)),
        "a closed thread runs no turn"
    );

    // A lagged stream may have lost the end: no seen turn is read as running.
    app.ensure_thread_channel(sub);
    app.handle_app_server_event(&server, started(sub, "s3"))
        .await;
    assert!(shared.turn_seen_besides(Some(&shown)));
    app.handle_app_server_event(&server, AppServerEvent::Lagged { skipped: 3 })
        .await;
    assert!(
        !shared.turn_seen_besides(Some(&shown)),
        "a lag lets the seen turns go"
    );

    // So may a stream that ended (a remote app-server's connection lost, then taken up again).
    app.handle_app_server_event(&server, started(sub, "s4"))
        .await;
    assert!(shared.turn_seen_besides(Some(&shown)));
    app.handle_app_server_event(
        &server,
        AppServerEvent::Disconnected {
            message: "lost".to_owned(),
        },
    )
    .await;
    assert!(
        !shared.turn_seen_besides(Some(&shown)),
        "a disconnect lets the seen turns go"
    );

    // A thread the TUI stops routing: its end can no longer come, so a frame's look lets go.
    app.ensure_thread_channel(sub);
    app.handle_app_server_event(&server, started(sub, "s5"))
        .await;
    assert!(shared.turn_seen_besides(Some(&shown)));
    app.thread_event_channels.remove(&sub);
    app.agent_navigation.remove(sub);
    observe(&app);
    assert!(
        !shared.turn_seen_besides(Some(&shown)),
        "an unrouted thread's turn goes"
    );

    // NEGATIVE CONTROL: a thread the TUI does not route books nothing.
    let stranger = ThreadId::new();
    app.handle_app_server_event(&server, started(stranger, "x1"))
        .await;
    assert!(!shared.turn_seen_besides(Some(&shown)));
    Ok(())
}

/// A SIDE THREAD'S END REACHES THE BOOK WITH NO PRIMARY (parity-5 review, T-stale-seen-turn): a
/// side conversation survives `reset_thread_event_state` (an archive or a delete of the current
/// task, then the agents overview with no thread attached), so it stays owned and the frame's
/// let-go keeps its seen turn, while upstream drops every thread notification before the
/// `routed` hook until a primary is attached. Its `turn/completed` is booked by the `stream`
/// hook, which no routing return passes by; RED with the end booked at `routed` (the ended
/// turn read as running ever after, so every rest stood in a turn and aterm refused the
/// session's `theirs` act for good). NEGATIVE CONTROL: with its primary standing, the end is
/// let go as before, and a turn that starts with no primary attached is not booked.
#[tokio::test]
async fn a_side_threads_end_reaches_the_book_with_no_primary() -> color_eyre::eyre::Result<()> {
    let shared: &'static boxes::Shared = Box::leak(Box::new(boxes::Shared::new()));
    let _boxes = boxes::scoped(shared);
    let (mut app, _events, _ops) = make_test_app_with_channels().await;
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let thread = server.start_thread(&app.config).await?;
    let primary = thread.session.thread_id;
    app.active_thread_id = Some(primary);
    app.primary_thread_id = Some(primary);
    app.chat_widget.handle_thread_session_quiet(thread.session);
    let side = ThreadId::new();
    app.ensure_thread_channel(side);
    app.side_threads
        .insert(side, crate::app::side::SideThreadState::new(primary));

    // NEGATIVE CONTROL: with the primary standing, the side thread's end lets its turn go.
    app.handle_app_server_event(&server, started(side, "x0"))
        .await;
    assert!(shared.turn_seen_besides(Some(&primary.to_string())));
    app.handle_app_server_event(&server, completed(side, "x0"))
        .await;
    observe(&app);
    assert!(!shared.turn_seen_besides(None));

    // The side thread's next turn starts; the current task is archived meanwhile.
    app.handle_app_server_event(&server, started(side, "x1"))
        .await;
    assert!(shared.turn_seen_besides(Some(&primary.to_string())));
    app.reset_thread_event_state().await;
    assert!(app.primary_thread_id.is_none());
    assert!(
        app.owns_thread_for_routing(side),
        "the side thread stays owned"
    );
    observe(&app);
    assert!(shared.turn_seen_besides(None), "its turn still runs");

    // Its end, with no primary: booked all the same.
    app.handle_app_server_event(&server, completed(side, "x1"))
        .await;
    observe(&app);
    assert!(
        !shared.turn_seen_besides(None),
        "the side thread's ended turn is not read as running"
    );
    // Another task picked later: nothing stale stands beside it.
    let next = ThreadId::new();
    app.primary_thread_id = Some(next);
    app.active_thread_id = Some(next);
    observe(&app);
    assert!(!shared.turn_seen_besides(Some(&next.to_string())));

    // NEGATIVE CONTROL: a start dropped with no primary attached books nothing (its end could
    // not be told from one that never came).
    app.primary_thread_id = None;
    app.active_thread_id = None;
    app.handle_app_server_event(&server, started(side, "x2"))
        .await;
    assert!(!shared.turn_seen_besides(None));
    Ok(())
}
