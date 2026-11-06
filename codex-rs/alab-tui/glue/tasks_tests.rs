// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app::owned_transcript::tests::attach_thread;
use crate::app::test_support::make_test_app;
use crate::app::tests::make_test_app_with_channels;
use crate::bottom_pane::BottomPaneView;
use crate::history_cell::new_plan_update;
use crate::keymap::RuntimeKeymap;
use crate::render::renderable::Renderable;
use codex_app_server_protocol::CommandExecutionOutputDeltaNotification;
use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItem;
use codex_config::types::KeybindingSpec;
use codex_config::types::KeybindingsSpec;
use codex_config::types::TuiKeymap;
use codex_protocol::plan_tool::PlanItemArg;
use codex_protocol::plan_tool::UpdatePlanArgs;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::buffer::Cell;
use ratatui::layout::Rect;
use std::sync::Arc;

#[path = "tasks_dispatch_tests.rs"]
mod disconnected_dispatch;

fn scope() -> interface::Scope {
    let state = Box::leak(Box::new(interface::State::new()));
    let scope = interface::scoped(state);
    interface::configure(true);
    scope
}

fn plan(app: &mut App, steps: &[(&str, StepStatus)]) {
    app.transcript_cells
        .push(Arc::new(new_plan_update(UpdatePlanArgs {
            explanation: None,
            plan: steps
                .iter()
                .map(|(step, status)| PlanItemArg {
                    step: (*step).into(),
                    status: status.clone(),
                })
                .collect(),
        })));
}

fn tasks_text(app: &App) -> String {
    tasks_text_at(app, 100, None)
}

fn tasks_text_at(app: &App, width: u16, height: Option<u16>) -> String {
    let view = app.alab_tasks_view(None, None);
    let area = Rect::new(
        0,
        0,
        width,
        height.unwrap_or_else(|| view.desired_height(width)),
    );
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    buf.content.iter().map(Cell::symbol).collect()
}

fn detail_page(view: &dyn BottomPaneView, height: u16) -> Vec<String> {
    let area = Rect::new(0, 0, 46, height);
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    buf.content
        .chunks(46)
        .map(|row| row.iter().map(Cell::symbol).collect::<String>())
        .skip(if height >= 5 { 2 } else { 1 })
        .take_while(|line| !line.contains("› Back to Tasks"))
        .collect()
}

fn compact(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn scroll_task_details(app: &App, key: &str, height: u16) -> String {
    let (title, detail) = app.alab_task_details_content(key);
    let mut view = app.chat_widget.alab_task_details_view(title, detail);
    assert!(view.accepts_input_when_disconnected());
    let first = detail_page(view.as_ref(), height);
    view.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(
        detail_page(view.as_ref(), height),
        first,
        "remapped native navigation retains ownership"
    );
    let mut seen = String::new();
    for _ in 0..80 {
        seen.push_str(&detail_page(view.as_ref(), height).join("\n"));
        view.handle_key_event(KeyEvent::new(KeyCode::F(7), KeyModifiers::NONE));
    }
    compact(&seen)
}

#[tokio::test]
async fn long_native_checklist_and_terminal_details_scroll_in_short_windows() {
    let _scope = scope();
    let mut app = make_test_app().await;
    let mut config = TuiKeymap::default();
    config.list.move_down = Some(KeybindingsSpec::One(KeybindingSpec("f7".into())));
    let runtime = RuntimeKeymap::from_config(&config).unwrap();
    app.chat_widget.apply_keymap_update(config, &runtime);
    let words: Vec<_> = (0..60).map(|i| format!("step-{i:02}-使用者")).collect();
    let step = format!("{}\nPLAN_TAIL_MARKER", words.join(" "));
    plan(&mut app, &[(&step, StepStatus::InProgress)]);
    let key = app.alab_tasks()[0].key.clone();
    for height in [4, 8, 12] {
        let seen = scroll_task_details(&app, &key, height);
        for expected in words.iter().map(String::as_str).chain(["PLAN_TAIL_MARKER"]) {
            assert!(seen.contains(expected), "missing {expected} at 46×{height}");
        }
    }

    app.transcript_cells.clear();
    let command = format!(
        "printf COMMAND_START {} COMMAND_END",
        "argument ".repeat(25)
    );
    app.chat_widget.handle_server_notification(
        ServerNotification::ItemStarted(ItemStartedNotification {
            thread_id: ThreadId::from_u128(710).to_string(),
            turn_id: "turn-1".into(),
            started_at_ms: 0,
            item: ThreadItem::untimed(
                codex_app_server_protocol::ThreadItemPayload::CommandExecution {
                    model_context: None,
                    sandbox_type: None,
                    id: "call-long".into(),
                    command,
                    cwd: app.config.cwd.clone().into(),
                    process_id: Some("proc-long".into()),
                    plugin_id: None,
                    script_path: None,
                    source: CommandExecutionSource::UnifiedExecStartup,
                    status: CommandExecutionStatus::InProgress,
                    command_actions: Vec::new(),
                    aggregated_output: None,
                    exit_code: None,
                    duration_ms: None,
                },
            ),
        }),
        None,
    );
    let output_words: Vec<_> = (0..3)
        .map(|line| {
            (0..18)
                .map(|part| format!("line-{line}-part-{part:02}"))
                .collect::<Vec<_>>()
        })
        .collect();
    let output = format!(
        "{} TERMINAL_TAIL_MARKER",
        output_words
            .iter()
            .map(|words| words.join(" "))
            .collect::<Vec<_>>()
            .join("\n")
    );
    app.chat_widget.handle_server_notification(
        ServerNotification::CommandExecutionOutputDelta(CommandExecutionOutputDeltaNotification {
            thread_id: ThreadId::from_u128(710).to_string(),
            turn_id: "turn-1".into(),
            item_id: "call-long".into(),
            delta: output,
        }),
        None,
    );
    let key = app.alab_tasks()[0].key.clone();
    for height in [4, 8, 12] {
        let seen = scroll_task_details(&app, &key, height);
        for expected in output_words.iter().flatten().map(String::as_str).chain([
            "COMMAND_START",
            "COMMAND_END",
            "TERMINAL_TAIL_MARKER",
        ]) {
            assert!(seen.contains(expected), "missing {expected} at 46×{height}");
        }
    }
}

#[tokio::test]
async fn task_details_back_cancel_and_deferred_off_paths_preserve_the_prompt() {
    let _scope = scope();
    let (mut app, mut events, _operations) = make_test_app_with_channels().await;
    attach_thread(&mut app, ThreadId::from_u128(711));
    app.chat_widget
        .apply_external_edit("unfinished prompt ".into());
    app.chat_widget
        .handle_paste("structured paste ".repeat(200));
    app.chat_widget
        .handle_key_event(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    let mut config = TuiKeymap::default();
    config.list.accept = Some(KeybindingsSpec::One(KeybindingSpec("f11".into())));
    config.list.cancel = Some(KeybindingsSpec::One(KeybindingSpec("q".into())));
    let runtime = RuntimeKeymap::from_config(&config).unwrap();
    app.chat_widget.apply_keymap_update(config, &runtime);
    plan(&mut app, &[("Inspect step", StepStatus::Pending)]);
    let key = app.alab_tasks()[0].key.clone();
    let thread = app.chat_widget.thread_id();
    let draft = app.chat_widget.capture_thread_input_state();
    let area = Rect::new(0, 0, 100, 24);
    let cursor = app.chat_widget.cursor_pos(area);

    for cancel in [
        KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ] {
        app.alab_task_details(thread, &key);
        while events.try_recv().is_ok() {}
        app.chat_widget
            .handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(
            app.chat_widget.has_active_view(),
            "native cancel was remapped"
        );
        app.chat_widget.handle_key_event(cancel);
        assert!(!app.chat_widget.has_active_view());
        assert!(
            events.try_recv().is_err(),
            "cancelling cannot reopen Tasks or submit model work"
        );
        assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
        assert_eq!(app.chat_widget.cursor_pos(area), cursor);
    }

    app.alab_task_details(thread, &key);
    while events.try_recv().is_ok() {}
    app.chat_widget
        .handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        app.chat_widget.has_active_view(),
        "native accept was remapped"
    );
    app.chat_widget
        .handle_key_event(KeyEvent::new(KeyCode::F(11), KeyModifiers::NONE));
    assert!(!app.chat_widget.has_active_view());
    assert!(matches!(
        events.try_recv(),
        Ok(AppEvent::AlabTasksBack { thread_id }) if thread_id == thread
    ));
    assert!(events.try_recv().is_err());
    app.alab_tasks_back(thread);
    assert!(app.chat_widget.alab_task_view_state().is_some());
    app.chat_widget
        .handle_key_event(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
    assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
    assert_eq!(app.chat_widget.cursor_pos(area), cursor);

    interface::set_enabled(false);
    app.alab_task_details(thread, &key);
    app.chat_widget
        .alab_show_task_details("Deferred task", "must remain hidden".into());
    assert!(!app.chat_widget.has_active_view());
    assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
}

#[tokio::test]
async fn disabled_checklist_guidance_respects_config_and_retained_native_plans() {
    let mut app = make_test_app().await;
    for enabled in [false, true] {
        app.config.update_plan_enabled = enabled;
        let shown = tasks_text(&app);
        assert_eq!(shown.contains("Checklist tool is off"), !enabled);
        assert_eq!(shown.contains("/alab help"), !enabled);
        assert!(!shown.contains("[tools.update_plan]"));
        assert!(app.alab_tasks().is_empty(), "guidance is not a task");
    }

    app.config.update_plan_enabled = false;
    let constrained = tasks_text_at(&app, 46, Some(8));
    assert!(
        constrained.contains("Checklist tool is off · /alab help"),
        "the short notice remains visible in a constrained native picker: {constrained}"
    );
    let child = ThreadId::from_u128(303);
    app.agent_navigation
        .upsert(child, Some("Worker".into()), None, false);
    let agent_key = app.alab_tasks()[0].key.clone();
    let shown = tasks_text(&app);
    assert!(shown.contains("Checklist tool is off"));
    assert!(shown.contains("Worker"));
    assert_eq!(app.alab_tasks().len(), 1);
    assert_eq!(app.alab_tasks()[0].key, agent_key);

    plan(&mut app, &[("Retained step", StepStatus::InProgress)]);
    let shown = tasks_text(&app);
    assert!(!shown.contains("Checklist tool is off"));
    assert!(shown.contains("Retained step"));
    let tasks = app.alab_tasks();
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].name, "[~] Retained step");
    assert_eq!(tasks[1].key, agent_key);

    plan(&mut app, &[]);
    assert!(
        !tasks_text(&app).contains("Checklist tool is off"),
        "an explicit empty native plan remains authoritative"
    );
    assert_eq!(app.alab_tasks().len(), 1);
}

#[tokio::test]
async fn connected_servers_do_not_infer_checklist_exposure_from_client_config() {
    let mut app = make_test_app().await;
    app.config.update_plan_enabled = false;
    let endpoint = crate::RemoteAppServerEndpoint::WebSocket {
        websocket_url: "ws://127.0.0.1:8765".into(),
        auth_token: None,
    };
    for target in [
        crate::AppServerTarget::Remote {
            endpoint: endpoint.clone(),
        },
        crate::AppServerTarget::LocalDaemon {
            endpoint,
            allow_embedded_fallback: false,
        },
    ] {
        app.app_server_target = target;
        let shown = tasks_text(&app);
        assert!(!shown.contains("Checklist tool is off"));
        assert!(!shown.contains("/alab help"));
        assert!(app.alab_tasks().is_empty());

        plan(&mut app, &[("Server checklist", StepStatus::Pending)]);
        assert!(tasks_text(&app).contains("Server checklist"));
        assert_eq!(app.alab_tasks()[0].name, "[ ] Server checklist");
        app.transcript_cells.clear();
    }
}

#[tokio::test]
async fn newest_native_checklist_wins_and_an_empty_update_clears_old_steps() {
    let mut app = make_test_app().await;
    plan(&mut app, &[("Old step", StepStatus::Pending)]);
    plan(
        &mut app,
        &[
            ("New step", StepStatus::InProgress),
            ("Finished", StepStatus::Completed),
        ],
    );
    let tasks = app.alab_tasks();
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].name, "[~] New step");
    assert_eq!(tasks[1].summary, "Completed");
    let selected_key = tasks[0].key.clone();
    plan(
        &mut app,
        &[
            ("Finished", StepStatus::Completed),
            ("New step", StepStatus::InProgress),
        ],
    );
    assert_eq!(
        app.alab_tasks()[1].key,
        selected_key,
        "reordering retains the selected task identity"
    );
    plan(&mut app, &[]);
    assert!(app.alab_tasks().is_empty());
    app.transcript_cells.clear();
    assert!(app.alab_tasks().is_empty());
}

#[tokio::test]
async fn local_agents_remain_available_without_a_daemon_and_main_is_not_duplicated() {
    let mut app = make_test_app().await;
    let root = ThreadId::from_u128(301);
    let child = ThreadId::from_u128(302);
    app.primary_thread_id = Some(root);
    app.agent_navigation
        .upsert(root, Some("Main".into()), None, false);
    app.agent_navigation
        .upsert(child, Some("Worker".into()), Some("review".into()), false);
    app.agent_navigation.set_running(child, true);
    let tasks = app.alab_tasks();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].agent, Some(child));
    assert_eq!(tasks[0].summary, "Working");
    app.agent_navigation.mark_closed(child);
    assert_eq!(app.alab_tasks()[0].summary, "Closed");
}

#[tokio::test]
async fn task_inspection_and_cancellation_preserve_a_draft() {
    let _scope = scope();
    let mut app = make_test_app().await;
    app.chat_widget
        .apply_external_edit("unfinished prompt".into());
    plan(&mut app, &[("Inspect this step", StepStatus::Pending)]);
    let key = app.alab_tasks()[0].key.clone();
    let thread = app.chat_widget.thread_id();
    app.alab_open_tasks();
    assert!(app.chat_widget.has_active_view());
    app.alab_task_details(thread, &key);
    assert_eq!(
        app.chat_widget.composer_text_with_pending(),
        "unfinished prompt"
    );
    app.chat_widget
        .handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(
        app.chat_widget.composer_text_with_pending(),
        "unfinished prompt"
    );
}

#[tokio::test]
async fn delayed_refresh_cannot_reopen_a_closed_view_or_replace_newer_search_input() {
    let _scope = scope();
    let mut app = make_test_app().await;
    plan(
        &mut app,
        &[
            ("Build", StepStatus::Pending),
            ("Review", StepStatus::Pending),
        ],
    );
    app.alab_open_tasks();
    let thread = app.chat_widget.thread_id();
    let (index, query) = app.chat_widget.alab_task_view_state().unwrap();
    let key = app.alab_tasks()[index.unwrap()].key.clone();
    let lifetime = Arc::new(());
    app.chat_widget.handle_paste("Review".into());
    let current = app.chat_widget.alab_task_view_state();
    assert_ne!(current, Some((index, query.clone())));
    app.alab_refresh_tasks(
        Arc::downgrade(&lifetime),
        thread,
        Some(key.clone()),
        index,
        query.clone(),
    );
    assert_eq!(app.chat_widget.alab_task_view_state(), current);
    app.chat_widget
        .handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.chat_widget
        .handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.chat_widget.alab_task_view_state().is_none());
    app.alab_refresh_tasks(Arc::downgrade(&lifetime), thread, Some(key), index, query);
    assert!(app.chat_widget.alab_task_view_state().is_none());
}

#[tokio::test]
async fn queued_refresh_from_a_closed_picker_cannot_move_a_reopened_picker() {
    let _scope = scope();
    let mut app = make_test_app().await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    app.app_event_tx = crate::app_event_sender::AppEventSender::new(tx);
    plan(
        &mut app,
        &[
            ("Build", StepStatus::Pending),
            ("Review", StepStatus::Pending),
        ],
    );
    let mut original = app.alab_tasks_view(None, None);
    original.pre_draw_tick(std::time::Instant::now() + std::time::Duration::from_secs(1));
    let event = rx.try_recv().expect("queued refresh");
    app.chat_widget.show_bottom_pane_view(original);
    app.chat_widget
        .handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    plan(
        &mut app,
        &[
            ("Review", StepStatus::Pending),
            ("Build", StepStatus::Pending),
        ],
    );
    app.alab_open_tasks();
    let before = app.chat_widget.alab_task_view_state();
    assert_eq!(before.as_ref().unwrap().0, Some(0));
    let AppEvent::AlabTasksRefresh {
        view,
        thread_id,
        selected_key,
        selected_index,
        query,
    } = event
    else {
        panic!("expected queued Tasks refresh");
    };
    assert!(
        view.upgrade().is_none(),
        "closed picker has no live refresh owner"
    );
    app.alab_refresh_tasks(view, thread_id, selected_key, selected_index, query);
    assert_eq!(app.chat_widget.alab_task_view_state(), before);
}
