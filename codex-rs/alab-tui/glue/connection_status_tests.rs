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

fn open_inspector_from_painted_account(chat: &mut ChatWidget) {
    let area = Rect::new(0, 0, 120, 1);
    let mut buffer = Buffer::empty(area);
    interface::render_status_current(area, &mut buffer, Depth::Mono, Theme::default());
    let column = (0..120)
        .find(|column| interface::account_mouse_hit(*column, 0))
        .unwrap();
    assert!(chat.handle_composer_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row: 0,
        modifiers: KeyModifiers::NONE,
    }));
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
