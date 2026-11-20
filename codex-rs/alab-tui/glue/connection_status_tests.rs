// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app::owned_transcript::tests::attach_thread;
use crate::app::reconnect::ReconnectPresentation;
use crate::app::reconnect::reconnect;
use crate::app::tests::make_test_app_with_channels;
use crate::bottom_pane::ComposerRenderOptions;
use crate::chatwidget::ChatWidget;
use crate::render::renderable::Renderable;
use crate::status::StatusAccountDisplay;
use crate::tui::TuiEvent;
use chrono::Local;
use codex_alab_tui::banner::Depth;
use codex_alab_tui::banner::Theme;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::RateLimitSnapshot;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::ThreadId;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use futures::SinkExt;
use futures::StreamExt;
use ratatui::buffer::Buffer;
use ratatui::buffer::Cell;
use ratatui::layout::Rect;
use serde_json::json;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

fn scope(enabled: bool) -> interface::Scope {
    let state = Box::leak(Box::new(interface::State::new()));
    let scope = interface::scoped(state);
    interface::configure(enabled);
    scope
}

fn account() -> Option<StatusAccountDisplay> {
    Some(StatusAccountDisplay::ChatGpt {
        email: Some("selected@example.test".into()),
        plan: Some("Pro".into()),
        workspace: None,
    })
}

fn usage(used: i32) -> RateLimitSnapshot {
    serde_json::from_value(json!({
        "limitId": "codex",
        "primary": {"usedPercent": 18, "windowDurationMins": 300,
            "resetsAt": Local::now().timestamp() + 3600},
        "secondary": {"usedPercent": used, "windowDurationMins": 10_080,
            "resetsAt": Local::now().timestamp() + 86_400}
    }))
    .unwrap()
}

fn turn(chat: &mut ChatWidget, thread: ThreadId, working: bool) {
    let turn = Turn {
        id: "connection-test-turn".into(),
        root_turn_id: None,
        items_view: TurnItemsView::Full,
        items: Vec::new(),
        status: if working {
            TurnStatus::InProgress
        } else {
            TurnStatus::Completed
        },
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    };
    chat.handle_server_notification(
        if working {
            ServerNotification::TurnStarted(TurnStartedNotification {
                thread_id: thread.to_string(),
                turn,
            })
        } else {
            ServerNotification::TurnCompleted(TurnCompletedNotification {
                thread_id: thread.to_string(),
                turn,
            })
        },
        None,
    );
}

fn draw_inspector(chat: &ChatWidget) -> String {
    let area = Rect::new(0, 0, 46, 8);
    let mut buffer = Buffer::empty(area);
    chat.bottom_pane_renderable(ComposerRenderOptions::default())
        .render(area, &mut buffer);
    buffer
        .content
        .chunks(46)
        .map(|row| row.iter().map(Cell::symbol).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn press_painted_control(chat: &mut ChatWidget, button: MouseButton, column: u16) {
    let press = MouseEvent {
        kind: MouseEventKind::Down(button),
        column,
        row: 0,
        modifiers: KeyModifiers::NONE,
    };
    chat.alab_capture_composer_press(&TuiEvent::Mouse(press));
    assert!(chat.prepare_composer_mouse(press));
    assert!(chat.handle_composer_mouse(press));
}

fn open_inspector_from_painted_account(chat: &mut ChatWidget) {
    let area = Rect::new(0, 0, 120, 1);
    let mut buffer = Buffer::empty(area);
    interface::render_status_current(area, &mut buffer, Depth::Mono, Theme::default());
    let column = (0..120)
        .find(|column| interface::account_mouse_hit(*column, 0))
        .unwrap();
    press_painted_control(chat, MouseButton::Left, column);
    assert!(chat.active_view_is("alab-account"));
}

#[tokio::test]
async fn native_disconnect_and_timeout_keep_scrolled_account_warning_and_preserve_native_state()
-> color_eyre::eyre::Result<()> {
    let _scope = scope(true);
    let (mut parked, _tx, _parked_events, _parked_ops) =
        crate::chatwidget::tests::make_chatwidget_manual_with_sender().await;
    parked.park_voice();
    let (mut app, mut events, mut ops) = make_test_app_with_channels().await;
    let thread = ThreadId::from_u128(901);
    attach_thread(&mut app, thread);
    app.chat_widget
        .update_account_state(account(), None, true, true);
    app.chat_widget.on_rate_limit_snapshot(Some(usage(27)));
    app.chat_widget
        .apply_external_edit("preserved prompt ".into());
    app.chat_widget
        .handle_paste("structured paste ".repeat(200));
    let draft = app.chat_widget.capture_thread_input_state();
    let current = interface::status_snapshot().unwrap();
    app.chat_widget.hold_rate_limit_recovery();
    open_inspector_from_painted_account(&mut app.chat_widget);
    draw_inspector(&app.chat_widget);
    app.chat_widget
        .handle_key_event(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    let bottom = draw_inspector(&app.chat_widget);
    assert!(bottom.contains("Usage checked:"), "{bottom}");
    assert!(
        !bottom.contains("Account: selected"),
        "the inspector is scrolled: {bottom}"
    );

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    app.app_server_target = crate::AppServerTarget::Remote {
        endpoint: crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?))?,
    };
    drop(listener);
    while events.try_recv().is_ok() {}
    while ops.try_recv().is_ok() {}
    assert!(app.begin_reconnect());
    assert_eq!(
        interface::connection_state(),
        interface::ConnectionState::Reconnecting
    );
    let mut expected = current.clone();
    expected.activity = Some("Reconnecting".into());
    assert_eq!(interface::status_snapshot(), Some(expected.clone()));
    let reconnecting = draw_inspector(&app.chat_widget);
    assert!(reconnecting.contains("Reconnecting"), "{reconnecting}");
    assert!(
        !reconnecting.contains("Cached account and quota data."),
        "the new notice must not reset the existing scroll position: {reconnecting}"
    );
    app.chat_widget
        .handle_disconnected_view_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE), None);
    let reconnecting_end = draw_inspector(&app.chat_widget);
    assert!(
        reconnecting_end.contains("Usage checked:"),
        "{reconnecting_end}"
    );

    // Exercise the real bounded native retry, then the startup loop's exhausted-result transition.
    tokio::time::pause();
    let started = tokio::time::Instant::now();
    let failed = reconnect(
        crate::backend_id::BackendId::new(),
        app.app_server_target.clone(),
        app.config.clone(),
        app.local_settings.clone(),
        Some(thread),
        None,
        crate::dynamic_tools_mcp::ThreadToolTransport::Dynamic,
        ReconnectPresentation::Conversation,
    )
    .await;
    assert!(failed.is_err());
    assert_eq!(started.elapsed().as_secs(), 120);
    tokio::time::resume();
    app.reconnect.failed = true;
    app.alab_refresh_connection_state();
    app.chat_widget.reconnect_failed();
    assert_eq!(
        interface::connection_state(),
        interface::ConnectionState::Offline
    );
    expected.activity = Some("Offline".into());
    assert_eq!(interface::status_snapshot(), Some(expected));
    let offline = draw_inspector(&app.chat_widget);
    assert!(offline.contains("Offline"), "{offline}");
    assert!(
        offline.contains("Usage checked:"),
        "scroll remains at End: {offline}"
    );
    app.chat_widget
        .handle_disconnected_view_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE), None);
    let top = draw_inspector(&app.chat_widget);
    assert!(top.contains("Cached account and quota data."), "{top}");
    assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
    assert!(
        app.chat_widget.hold_rate_limit_recovery(),
        "display cannot release native recovery"
    );
    assert!(ops.try_recv().is_err(), "display cannot submit model work");

    // Selected and hidden widget refreshes cannot overwrite connection-owned activity.
    turn(&mut app.chat_widget, thread, true);
    assert_eq!(
        interface::status_snapshot().unwrap().activity.as_deref(),
        Some("Offline")
    );
    let selected = interface::status_snapshot();
    parked.update_account_state(Some(StatusAccountDisplay::ApiKey), None, false, false);
    parked.on_rate_limit_snapshot(Some(usage(99)));
    assert_eq!(interface::status_snapshot(), selected);
    Ok(())
}

#[tokio::test]
async fn real_connection_adoption_restores_latest_native_activity_and_off_keeps_tracking()
-> color_eyre::eyre::Result<()> {
    let _scope = scope(false);
    let (mut app, mut events, mut ops) = make_test_app_with_channels().await;
    app.config.model = Some("gpt-test".into());
    app.chat_widget
        .update_account_state(account(), None, true, true);
    app.chat_widget.on_rate_limit_snapshot(Some(usage(27)));
    app.chat_widget
        .apply_external_edit("draft across connection adoption".into());
    let draft_text = app.chat_widget.composer_text_with_pending();
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    app.app_server_target = crate::AppServerTarget::Remote {
        endpoint: crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?))?,
    };
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut socket = tokio_tungstenite::accept_async(stream).await?;
        let mut methods = Vec::new();
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            let Ok(JSONRPCMessage::Request(request)) = serde_json::from_str(&text) else {
                continue;
            };
            methods.push(request.method.clone());
            let mut response = match request.method.as_str() {
                "initialize" => json!({"result": {"userAgent": "alab-connection-test/1.0.0"}}),
                "account/read" => {
                    json!({"result": {"account": {"type": "chatgpt", "email": "selected@example.test", "planType": "pro"}, "requiresOpenaiAuth": true}})
                }
                "model/list" => json!({"result": {"data": [], "nextCursor": null}}),
                "collaborationMode/list" => json!({"result": {"data": []}}),
                "configRequirements/read" => json!({"result": {"requirements": null}}),
                _ => {
                    json!({"error": {"code": -32601, "message": "unsupported isolated fixture method"}})
                }
            };
            response["id"] = json!(request.id);
            socket
                .send(Message::Text(response.to_string().into()))
                .await?;
        }
        Ok::<_, color_eyre::Report>(methods)
    });
    let mut session = crate::start_embedded_app_server_for_picker(&app.config).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    while events.try_recv().is_ok() {}
    while ops.try_recv().is_ok() {}
    assert!(app.begin_reconnect());
    assert!(!interface::enabled());
    assert_eq!(
        interface::connection_state(),
        interface::ConnectionState::Reconnecting
    );
    let native_off = crate::chatwidget::tests::render_bottom_popup(&app.chat_widget, 100);
    assert!(
        native_off.contains("Reconnecting to server"),
        "{native_off}"
    );
    interface::set_enabled(true);
    app.chat_widget.refresh_status_surfaces();
    assert_eq!(
        interface::status_snapshot().unwrap().activity.as_deref(),
        Some("Reconnecting")
    );
    interface::set_enabled(false);

    let connected = reconnect(
        session.backend_id(),
        app.app_server_target.clone(),
        app.config.clone(),
        app.local_settings.clone(),
        None,
        None,
        session.thread_tool_transport(),
        ReconnectPresentation::Overview,
    )
    .await?;
    app.finish_reconnect(&mut tui, &mut session, &mut events, connected, "1.0.0")
        .await?;
    assert_eq!(
        interface::connection_state(),
        interface::ConnectionState::Connected
    );
    assert!(!interface::enabled());
    assert!(!app.reconnect.offline);
    assert_eq!(app.chat_widget.composer_text_with_pending(), draft_text);
    let off_snapshot = interface::status_snapshot();

    let thread = ThreadId::from_u128(902);
    attach_thread(&mut app, thread);
    while events.try_recv().is_ok() {}
    while ops.try_recv().is_ok() {}
    turn(&mut app.chat_widget, thread, true);
    assert_eq!(interface::status_snapshot(), off_snapshot);
    turn(&mut app.chat_widget, thread, false);
    assert_eq!(interface::status_snapshot(), off_snapshot);
    interface::set_enabled(true);
    app.chat_widget.refresh_status_surfaces();
    assert_eq!(
        interface::status_snapshot().unwrap().activity.as_deref(),
        Some("Ready")
    );
    assert_eq!(
        interface::status_snapshot().unwrap().account,
        "selected@example.test"
    );
    assert_eq!(
        interface::status_snapshot().unwrap().weekly,
        None,
        "native adoption clears old account usage"
    );
    assert!(
        ops.try_recv().is_err(),
        "connection presentation cannot submit a model turn"
    );
    session.shutdown().await?;
    let methods = server.await??;
    assert!(methods.iter().any(|method| method == "account/read"));
    assert!(!methods.iter().any(|method| method == "turn/start"));
    Ok(())
}

#[tokio::test]
async fn disconnected_autopilot_deadline_can_be_cancelled_and_submitted_without_touching_the_draft()
-> color_eyre::eyre::Result<()> {
    use codex_alab_aterm::fake::FakeAterm;
    use codex_alab_aterm::fake::Mode;
    use codex_alab_tui::session::Autopilot;
    use codex_alab_tui::session::HarnessWord;
    use codex_alab_tui::session::Shared;

    let dir = tempfile::tempdir()?;
    let fake = FakeAterm::start(dir.path(), Mode::Current).await?;
    let _interface = scope(true);
    interface::route_aterm_for_tests(fake.endpoint());
    let shared = Box::leak(Box::new(Shared::new()));
    let _automation = codex_alab_tui::boxes::scoped(shared);
    shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Land);
    let (mut app, mut events, mut operations) = make_test_app_with_channels().await;
    attach_thread(&mut app, ThreadId::from_u128(905));
    app.chat_widget
        .apply_external_edit("unfinished prompt".into());
    let draft = app.chat_widget.capture_thread_input_state();
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    app.app_server_target = crate::AppServerTarget::Remote {
        endpoint: crate::resolve_remote_addr("ws://127.0.0.1:8765")?,
    };
    let mut tui = crate::tui::test_support::make_test_tui()?;
    while events.try_recv().is_ok() {}
    while operations.try_recv().is_ok() {}

    let area = Rect::new(0, 0, 80, 3);
    interface::render(
        area,
        &mut Buffer::empty(area),
        Depth::Mono,
        Theme::default(),
    );
    let column = (0..area.width)
        .find(|x| interface::autopilot_menu_hit(*x, 0))
        .expect("mode control");
    press_painted_control(&mut app.chat_widget, MouseButton::Right, column);
    let interrupt = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    for (key, duration, finish) in [
        (KeyCode::Char('7'), "2h", KeyEvent::from(KeyCode::Esc)),
        (KeyCode::Enter, "3h", interrupt),
        (KeyCode::Enter, "4h", KeyEvent::from(KeyCode::Enter)),
    ] {
        app.handle_tui_event(&mut tui, &mut server, TuiEvent::Key(key.into()))
            .await?;
        let open = events.try_recv()?;
        assert!(matches!(open, AppEvent::OpenAlabAutopilotTimer));
        app.handle_event(&mut tui, &mut server, open).await?;
        if !app.reconnect.offline {
            // Disconnect with an editor already on screen. Its text still belongs to the
            // timer, and cancelling returns to its parent without changing the policy.
            assert!(app.begin_reconnect());
        }
        app.handle_tui_event(
            &mut tui,
            &mut server,
            TuiEvent::Key(KeyCode::Char('+').into()),
        )
        .await?;
        app.handle_tui_event(&mut tui, &mut server, TuiEvent::Paste(duration.into()))
            .await?;
        let editor = crate::chatwidget::tests::render_bottom_popup(&app.chat_widget, 64);
        assert!(
            editor.contains("Cruise until") && editor.contains(&format!("+{duration}")),
            "{editor}"
        );
        let control = app
            .handle_tui_event(&mut tui, &mut server, TuiEvent::Key(finish))
            .await?;
        assert!(matches!(control, crate::app::AppRunControl::Continue));
        assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
        if finish.code != KeyCode::Enter {
            assert!(app.chat_widget.active_view_is("alab-autopilot"));
            assert!(
                fake.requests()
                    .iter()
                    .all(|request| !request.contains("meta set autopilot"))
            );
        }
    }

    let timed = format!(
        "@{} meta set autopilot cruise until=+4h",
        fake.endpoint().sid
    );
    assert!(fake.wait_for(|host| host.requests().contains(&timed)).await);
    let writes: Vec<_> = fake
        .requests()
        .into_iter()
        .filter(|r| r.contains("meta set autopilot"))
        .collect();
    assert_eq!(writes, [timed]);
    assert!(app.reconnect.offline);
    assert!(!app.chat_widget.has_active_view());
    assert!(
        operations.try_recv().is_err(),
        "timer input must never submit model work"
    );
    assert!(
        std::iter::from_fn(|| events.try_recv().ok())
            .all(|event| !matches!(event, AppEvent::CodexOp(_))),
        "deadline submission must never queue a model operation"
    );
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn disconnected_autopilot_deadline_policy_keeps_native_custom_review_server_bound()
-> color_eyre::eyre::Result<()> {
    let (mut app, mut events, mut operations) = make_test_app_with_channels().await;
    attach_thread(&mut app, ThreadId::from_u128(906));
    app.app_server_target = crate::AppServerTarget::Remote {
        endpoint: crate::resolve_remote_addr("ws://127.0.0.1:8765")?,
    };
    app.chat_widget.apply_external_edit("ordinary draft".into());
    let draft = app.chat_widget.capture_thread_input_state();
    app.chat_widget.show_review_custom_prompt();
    app.chat_widget.handle_paste("inspect this change".into());
    let editor = crate::chatwidget::tests::render_bottom_popup(&app.chat_widget, 64);
    assert!(
        editor.contains("Custom review instructions") && editor.contains("inspect this change")
    );
    while events.try_recv().is_ok() {}
    while operations.try_recv().is_ok() {}

    // The ordinary review prompt would submit a server operation. Reconnecting
    // discards that modal and preserves the separate conversation draft.
    assert!(app.begin_reconnect());
    assert!(!app.chat_widget.has_active_view());
    assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
    assert!(
        std::iter::from_fn(|| events.try_recv().ok()).all(|ev| !matches!(ev, AppEvent::CodexOp(_)))
    );
    assert!(operations.try_recv().is_err());
    Ok(())
}
