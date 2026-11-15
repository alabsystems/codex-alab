// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native App routing and actual widget/status/progress controls, isolated so
//! activating ALab cannot change upstream tests in the parent libtest process.

use super::tests::turn;
use crate::app::alab_goal_runtime_fixture as fixture;
use crate::chatwidget::ChatWidget;
use crate::chatwidget::exec_state::UnifiedExecProcessSummary;
use crate::terminal_program_status::ProgramStatus;
use codex_alab_tui::progress::Bar;
use codex_alab_tui::session::ProgramState;
use codex_app_server_client::AppServerEvent;
use codex_app_server_client::CloudOperation;
use codex_app_server_protocol::AlabGoalPersistencePhase as Phase;
use codex_app_server_protocol::AlabGoalRuntimeUpdatedNotification as RuntimeUpdate;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadGoal;
use codex_app_server_protocol::ThreadGoalStatus;
use codex_app_server_protocol::ThreadGoalUpdatedNotification;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::ThreadId;
use std::time::Duration;

const CASE: &str = "ALAB_GOAL_RUNTIME_STATUS_CASE";
const COMPLETE: &str = "ALAB_GOAL_RUNTIME_STATUS_OK";

#[tokio::test]
#[expect(
    clippy::print_stderr,
    reason = "record which isolated native status controls actually executed"
)]
async fn goal_runtime_status_widget_backend_and_reset_controls() {
    let (_, module) = module_path!().split_once("::").unwrap();
    let child = format!("{module}::goal_runtime_status_child");
    for case in ["activity", "backend", "reset"] {
        let output = tokio::time::timeout(
            Duration::from_secs(90),
            tokio::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &child, "--nocapture", "--test-threads=1"])
                .env(CASE, case)
                .env("RUST_MIN_STACK", "8388608")
                .kill_on_drop(true)
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{case}: {stdout}\n{stderr}");
        assert!(
            stdout.contains(&format!("{COMPLETE}:{case}")),
            "isolated control did not execute: {stdout}"
        );
        eprintln!("goal_runtime_status isolated {case}: actual assertions passed");
    }
}

#[tokio::test]
#[expect(
    clippy::print_stdout,
    reason = "parent requires proof that each isolated control executed"
)]
async fn goal_runtime_status_child() {
    let (_, module) = module_path!().split_once("::").unwrap();
    let name = format!("{module}::goal_runtime_status_child");
    if std::env::args().skip(1).collect::<Vec<_>>()
        != ["--exact", &name, "--nocapture", "--test-threads=1"]
    {
        return;
    }
    let Ok(case) = std::env::var(CASE) else {
        return;
    };
    codex_alab_core::activate();
    match case.as_str() {
        "activity" => activity().await,
        "backend" => backend().await,
        "reset" => reset().await,
        _ => panic!("unknown native status control"),
    }
    println!("{COMPLETE}:{case}");
}

fn goal(chat: &mut ChatWidget, thread: ThreadId) {
    chat.set_feature_enabled(crate::Feature::Goals, true);
    chat.handle_server_notification(
        ServerNotification::ThreadGoalUpdated(ThreadGoalUpdatedNotification {
            thread_id: thread.to_string(),
            turn_id: None,
            goal: ThreadGoal {
                thread_id: thread.to_string(),
                objective: "preserve the saved task".into(),
                status: ThreadGoalStatus::Active,
                token_budget: Some(1000),
                tokens_used: 22,
                time_used_seconds: 60,
                created_at: 1,
                updated_at: 1,
            },
        }),
        None,
    );
    turn(chat, "finished", TurnStatus::InProgress);
    turn(chat, "finished", TurnStatus::Completed);
}

fn update(
    thread: ThreadId,
    stream: (&str, u64),
    sequence: u64,
    incarnation: &str,
    revision: u64,
    phase: Option<Phase>,
    writes_pending: bool,
) -> ServerNotification {
    ServerNotification::AlabGoalRuntimeUpdated(RuntimeUpdate {
        thread_id: thread.to_string(),
        transport: stream.0.into(),
        generation: stream.1,
        sequence,
        incarnation: incarnation.into(),
        revision,
        persistence: phase,
        writes_pending,
        reset: false,
    })
}

fn reset_packet(stream: (&str, u64), sequence: u64) -> ServerNotification {
    ServerNotification::AlabGoalRuntimeUpdated(RuntimeUpdate {
        thread_id: String::new(),
        transport: stream.0.into(),
        generation: stream.1,
        sequence,
        incarnation: String::new(),
        revision: 0,
        persistence: None,
        writes_pending: false,
        reset: true,
    })
}

fn state(chat: &ChatWidget, expected: ProgramStatus) {
    assert_eq!(chat.desired_program_status(), expected);
    assert_eq!(
        chat.alab_program_state(),
        match expected {
            ProgramStatus::Done => ProgramState::Done,
            ProgramStatus::Working => ProgramState::Working,
            _ => panic!("unexpected control state"),
        }
    );
    assert_eq!(
        Bar::of(chat.alab_program_state(), chat.alab_plan_progress()),
        if expected == ProgramStatus::Done {
            Bar::Hidden
        } else {
            Bar::Indeterminate
        }
    );
    assert_eq!(chat.alab_goal(), Some("preserve the saved task"));
    assert!(
        chat.current_goal_status.as_ref().unwrap().is_active(),
        "passive status never changes the saved Active badge"
    );
}

async fn activity() {
    let (mut app, _events, _ops) = fixture::fixture().await;
    let (session, _commands, _stream) = fixture::backend();
    let owner = session.backend_id();
    let thread = ThreadId::new();
    fixture::select(&mut app, owner, thread);
    goal(&mut app.chat_widget, thread);
    state(&app.chat_widget, ProgramStatus::Working);
    fixture::feed(
        &mut app,
        &session,
        owner,
        owner,
        update(
            thread,
            ("host", 0),
            1,
            "runtime",
            11,
            Some(Phase::Held),
            false,
        ),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Done);
    let scope = codex_alab_tui::turns::scoped();
    app.chat_widget.pre_draw_tick();
    assert!(
        scope
            .take()
            .contains(&codex_alab_tui::turns::sequence(ProgramState::Done).unwrap())
    );
    for (sequence, phase, pending) in [(2, Phase::Writing, true), (3, Phase::Held, true)] {
        fixture::feed(
            &mut app,
            &session,
            owner,
            owner,
            update(
                thread,
                ("host", 0),
                sequence,
                "runtime",
                10 + sequence,
                Some(phase),
                pending,
            ),
        )
        .await;
        state(&app.chat_widget, ProgramStatus::Working);
    }
    fixture::feed(
        &mut app,
        &session,
        owner,
        owner,
        update(
            thread,
            ("host", 0),
            4,
            "runtime",
            14,
            Some(Phase::Held),
            false,
        ),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Done);
    app.chat_widget.input_queue.user_turn_pending_start = true;
    state(&app.chat_widget, ProgramStatus::Working);
    app.chat_widget.input_queue.user_turn_pending_start = false;
    turn(&mut app.chat_widget, "person-turn", TurnStatus::InProgress);
    state(&app.chat_widget, ProgramStatus::Working);
    turn(&mut app.chat_widget, "person-turn", TurnStatus::Completed);
    state(&app.chat_widget, ProgramStatus::Done);
    app.chat_widget.handle_server_notification(
        ServerNotification::McpServerStatusUpdated(
            codex_app_server_protocol::McpServerStatusUpdatedNotification {
                thread_id: Some(thread.to_string()),
                name: "starting".into(),
                status: codex_app_server_protocol::McpServerStartupState::Starting,
                error: None,
                failure_reason: None,
            },
        ),
        None,
    );
    state(&app.chat_widget, ProgramStatus::Working);
    app.chat_widget.handle_server_notification(
        ServerNotification::McpServerStatusUpdated(
            codex_app_server_protocol::McpServerStatusUpdatedNotification {
                thread_id: Some(thread.to_string()),
                name: "starting".into(),
                status: codex_app_server_protocol::McpServerStartupState::Ready,
                error: None,
                failure_reason: None,
            },
        ),
        None,
    );
    state(&app.chat_widget, ProgramStatus::Done);
    app.chat_widget
        .unified_exec_processes
        .push(UnifiedExecProcessSummary {
            key: "owned-background".into(),
            call_id: "call".into(),
            command_display: "owned test process".into(),
            recent_chunks: Vec::new(),
        });
    state(&app.chat_widget, ProgramStatus::Working);
    app.chat_widget.unified_exec_processes.clear();
    state(&app.chat_widget, ProgramStatus::Done);
    fixture::feed(
        &mut app,
        &session,
        owner,
        owner,
        update(
            thread,
            ("host", 0),
            5,
            "runtime",
            15,
            Some(Phase::Ready),
            false,
        ),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Working);
    fixture::feed(
        &mut app,
        &session,
        owner,
        owner,
        update(
            thread,
            ("host", 0),
            4,
            "runtime",
            14,
            Some(Phase::Held),
            false,
        ),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Working);
    fixture::feed(
        &mut app,
        &session,
        owner,
        owner,
        update(
            thread,
            ("host", 0),
            6,
            "new-runtime",
            1,
            Some(Phase::Held),
            false,
        ),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Done);
    fixture::feed(
        &mut app,
        &session,
        owner,
        owner,
        update(thread, ("host", 0), 7, "new-runtime", 1, None, false),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Working);
}

async fn backend() {
    let (mut app, _events, _ops) = fixture::fixture().await;
    let (a, _a_commands, _a_stream) = fixture::backend();
    let (b, _b_commands, _b_stream) = fixture::backend();
    let owner_a = a.backend_id();
    let owner_b = b.backend_id();
    let first = ThreadId::new();
    let second = ThreadId::new();
    let shown_b = ThreadId::new();
    fixture::select(&mut app, owner_a, first);
    goal(&mut app.chat_widget, first);
    fixture::feed(
        &mut app,
        &a,
        owner_a,
        owner_a,
        update(first, ("a", 0), 1, "a-runtime", 1, Some(Phase::Held), false),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Done);
    fixture::select(&mut app, owner_a, second);
    goal(&mut app.chat_widget, second);
    fixture::feed(
        &mut app,
        &a,
        owner_a,
        owner_a,
        update(
            second,
            ("a", 0),
            100,
            "second",
            1,
            Some(Phase::Ready),
            false,
        ),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Working);
    fixture::select(&mut app, owner_a, first);
    goal(&mut app.chat_widget, first);
    fixture::observe(&app);
    state(&app.chat_widget, ProgramStatus::Done);
    fixture::feed(
        &mut app,
        &b,
        owner_a,
        owner_b,
        update(
            first,
            ("b", 0),
            200,
            "collision",
            1,
            Some(Phase::Ready),
            false,
        ),
    )
    .await;
    fixture::feed(
        &mut app,
        &b,
        owner_a,
        owner_a,
        update(
            first,
            ("b", 0),
            201,
            "forged-owner",
            1,
            Some(Phase::Ready),
            false,
        ),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Done);
    fixture::select(&mut app, owner_b, shown_b);
    goal(&mut app.chat_widget, shown_b);
    app.chat_widget.insert_str("keep the displayed draft");
    let draft = app.chat_widget.composer_text_with_pending();
    fixture::feed(
        &mut app,
        &a,
        owner_b,
        owner_a,
        update(
            first,
            ("a", 0),
            2,
            "a-runtime",
            2,
            Some(Phase::Ready),
            false,
        ),
    )
    .await;
    assert_eq!(app.chat_widget.composer_text_with_pending(), draft);
    state(&app.chat_widget, ProgramStatus::Working);
    fixture::select(&mut app, owner_a, first);
    goal(&mut app.chat_widget, first);
    fixture::observe(&app);
    state(&app.chat_widget, ProgramStatus::Working);
    fixture::feed(
        &mut app,
        &a,
        owner_a,
        owner_a,
        update(first, ("a", 0), 3, "a-runtime", 3, Some(Phase::Held), false),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Done);
    fixture::event(
        &mut app,
        &a,
        owner_a,
        owner_a,
        AppServerEvent::Lagged { skipped: 1 },
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Working);
}

async fn reset() {
    let (mut app, _events, _ops) = fixture::fixture().await;
    let (session, mut commands, _stream) = fixture::backend();
    let owner = session.backend_id();
    let thread = ThreadId::new();
    let other = ThreadId::new();
    fixture::select(&mut app, owner, thread);
    goal(&mut app.chat_widget, thread);
    // First observation is new1; this thread has never observed old0.
    fixture::feed(
        &mut app,
        &session,
        owner,
        owner,
        update(
            thread,
            ("host", 1),
            101,
            "runtime",
            11,
            Some(Phase::Held),
            false,
        ),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Done);
    fixture::feed(
        &mut app,
        &session,
        owner,
        owner,
        update(
            thread,
            ("host", 0),
            80,
            "runtime-old",
            80,
            Some(Phase::Ready),
            false,
        ),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Done);
    // Backend-wide generation order also fences a previously unseen thread,
    // before any reset control has arrived on the wire.
    fixture::select(&mut app, owner, other);
    goal(&mut app.chat_widget, other);
    fixture::feed(
        &mut app,
        &session,
        owner,
        owner,
        update(
            other,
            ("host", 0),
            90,
            "unseen-old",
            90,
            Some(Phase::Held),
            false,
        ),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Working);
    fixture::feed(
        &mut app,
        &session,
        owner,
        owner,
        update(
            other,
            ("host", 1),
            102,
            "other-new",
            1,
            Some(Phase::Held),
            false,
        ),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Done);
    fixture::feed(
        &mut app,
        &session,
        owner,
        owner,
        update(
            other,
            ("host", 0),
            95,
            "other-old",
            95,
            Some(Phase::Ready),
            false,
        ),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Done);
    fixture::select(&mut app, owner, thread);
    goal(&mut app.chat_widget, thread);
    fixture::observe(&app);
    state(&app.chat_widget, ProgramStatus::Done);
    tokio::time::timeout(
        Duration::from_secs(5),
        fixture::feed(
            &mut app,
            &session,
            owner,
            owner,
            reset_packet(("host", 0), 100),
        ),
    )
    .await
    .unwrap();
    state(&app.chat_widget, ProgramStatus::Done);
    let command0 = tokio::time::timeout(Duration::from_secs(5), commands.recv())
        .await
        .unwrap()
        .unwrap();
    let CloudOperation::Request(request) = &command0.operation else {
        panic!("resync uses only the existing native read");
    };
    assert!(
        matches!(request.as_ref(), codex_app_server_protocol::ClientRequest::ThreadGoalGet { params, .. } if params.thread_id == thread.to_string())
    );
    assert!(
        fixture::refresh_pending(&app),
        "goal/get DB response is deliberately still gated"
    );
    fixture::feed(
        &mut app,
        &session,
        owner,
        owner,
        update(
            thread,
            ("host", 1),
            103,
            "runtime",
            11,
            Some(Phase::Held),
            false,
        ),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Done);
    assert!(
        fixture::refresh_pending(&app),
        "UI drains current Held before the DB response"
    );
    fixture::feed(
        &mut app,
        &session,
        owner,
        owner,
        reset_packet(("host", 0), 100),
    )
    .await;
    assert!(
        commands.try_recv().is_err(),
        "duplicate reset creates no read or model request"
    );
    // A newer reset supersedes the one outstanding read. A delayed old reset
    // cannot replace that newer refresh or schedule a third request.
    fixture::feed(
        &mut app,
        &session,
        owner,
        owner,
        reset_packet(("host", 1), 200),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Working);
    let command1 = tokio::time::timeout(Duration::from_secs(5), commands.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(&command1.operation, CloudOperation::Request(request) if matches!(request.as_ref(), codex_app_server_protocol::ClientRequest::ThreadGoalGet { .. }))
    );
    for _ in 0..10 {
        if command0.response.is_closed() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(command0.response.is_closed());
    fixture::feed(
        &mut app,
        &session,
        owner,
        owner,
        reset_packet(("host", 0), 100),
    )
    .await;
    assert!(commands.try_recv().is_err());
    assert!(fixture::refresh_pending(&app));
    fixture::feed(
        &mut app,
        &session,
        owner,
        owner,
        update(
            thread,
            ("host", 2),
            201,
            "runtime",
            11,
            Some(Phase::Held),
            false,
        ),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Done);
    fixture::feed(
        &mut app,
        &session,
        owner,
        owner,
        update(
            thread,
            ("host", 0),
            1000,
            "late-old",
            1000,
            Some(Phase::Ready),
            false,
        ),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Done);
    fixture::select(&mut app, owner, other);
    goal(&mut app.chat_widget, other);
    fixture::observe(&app);
    state(&app.chat_widget, ProgramStatus::Working);
    fixture::feed(
        &mut app,
        &session,
        owner,
        owner,
        update(
            other,
            ("host", 0),
            2000,
            "different-thread-old",
            2000,
            Some(Phase::Held),
            false,
        ),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Working);
    fixture::feed(
        &mut app,
        &session,
        owner,
        owner,
        update(
            other,
            ("host", 2),
            202,
            "different-thread-new",
            1,
            Some(Phase::Held),
            false,
        ),
    )
    .await;
    state(&app.chat_widget, ProgramStatus::Done);
    assert!(commands.try_recv().is_err());
    for _ in 0..10 {
        if command1.response.is_closed() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        command1.response.is_closed(),
        "switch cancels the superseded request-only view"
    );
}
