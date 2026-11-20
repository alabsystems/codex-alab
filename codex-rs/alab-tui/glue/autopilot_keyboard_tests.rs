// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app_server_session::AppServerSession;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::keymap::RuntimeKeymap;
use crate::tui::Tui;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use codex_alab_tui::session::Autopilot;
use codex_alab_tui::session::HarnessWord;
use codex_alab_tui::session::Shared;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use codex_config::types::KeybindingSpec;
use codex_config::types::KeybindingsSpec;
use codex_config::types::TuiKeymap;
use crossterm::event::KeyEventKind;

fn bind_picker(app: &mut App) -> color_eyre::eyre::Result<()> {
    let mut config = TuiKeymap::default();
    config.chat.open_autopilot = Some(KeybindingsSpec::Many(vec![
        KeybindingSpec("f12".into()),
        KeybindingSpec("ctrl-x 3".into()),
    ]));
    let runtime =
        RuntimeKeymap::from_config(&config).map_err(|error| color_eyre::eyre::eyre!(error))?;
    app.chat_widget.apply_keymap_update(config, &runtime);
    app.keymap = runtime;
    Ok(())
}

async fn keys(
    app: &mut App,
    tui: &mut Tui,
    server: &mut AppServerSession,
    sequence: &[KeyEvent],
) -> color_eyre::eyre::Result<()> {
    for &key in sequence {
        app.handle_tui_event(tui, server, TuiEvent::Key(key))
            .await?;
    }
    Ok(())
}

#[tokio::test]
async fn disconnected_default_submit_opens_only_the_local_autopilot_slash_command()
-> color_eyre::eyre::Result<()> {
    let root = codex_alab_tui::test_paths::short_tempdir()?;
    let fake = FakeAterm::start(root.path(), Mode::Current).await?;
    let _interface = scope();
    interface::route_aterm_for_tests(fake.endpoint());
    let shared = Box::leak(Box::new(Shared::new()));
    let _host = codex_alab_tui::boxes::scoped(shared);
    shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Cruise);
    let (mut app, mut events, mut ops) = make_test_app_with_channels().await;
    attach_thread(&mut app, ThreadId::new());
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    app.app_server_target = crate::AppServerTarget::Remote {
        endpoint: crate::resolve_remote_addr("ws://127.0.0.1:8765")?,
    };
    assert!(app.begin_reconnect());
    assert!(app.keymap.chat.open_autopilot.is_empty());
    while events.try_recv().is_ok() {}
    while ops.try_recv().is_ok() {}
    let host_requests = fake.requests();
    let enter = KeyEvent::from(KeyCode::Enter);
    let held = KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Repeat);
    let release =
        KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Release);
    for text in [
        "ordinary prompt",
        "/alab",
        "/alab land",
        "/alab menu extra",
        "/alab men",
        " /alab menu",
        "/alab menu\n",
        "/alab menu\nsecond line",
    ] {
        app.chat_widget.apply_external_edit(text.into());
        let draft = app.chat_widget.composer_draft_snapshot();
        keys(&mut app, &mut tui, &mut server, &[enter]).await?;
        assert_eq!(app.chat_widget.composer_draft_snapshot(), draft, "{text:?}");
        assert!(!app.chat_widget.has_active_view());
    }

    app.chat_widget.apply_external_edit("/alab menu".into());
    app.chat_widget
        .set_remote_image_urls(vec!["https://example.invalid/image.png".into()]);
    let attached = app.chat_widget.composer_draft_snapshot();
    keys(&mut app, &mut tui, &mut server, &[enter]).await?;
    assert_eq!(app.chat_widget.composer_draft_snapshot(), attached);
    assert!(!app.chat_widget.has_active_view());
    app.chat_widget.set_remote_image_urls(vec![]);
    app.startup_protected_input_boundary = true;
    app.startup_pending_protected_request = true;
    let protected = app.chat_widget.composer_draft_snapshot();
    keys(&mut app, &mut tui, &mut server, &[enter]).await?;
    assert_eq!(app.chat_widget.composer_draft_snapshot(), protected);
    assert!(!app.chat_widget.has_active_view());
    app.startup_protected_input_boundary = false;
    app.startup_pending_protected_request = false;
    assert!(!interface::set_enabled(false));
    let vanilla = app.chat_widget.composer_draft_snapshot();
    keys(&mut app, &mut tui, &mut server, &[enter]).await?;
    assert_eq!(app.chat_widget.composer_draft_snapshot(), vanilla);
    assert!(!app.chat_widget.has_active_view());
    assert!(interface::set_enabled(true));

    app.chat_widget.apply_external_edit(String::new());
    for ch in "/alab menu".chars() {
        keys(&mut app, &mut tui, &mut server, &[KeyCode::Char(ch).into()]).await?;
    }
    assert_eq!(app.chat_widget.composer_text_with_pending(), "/alab menu");
    keys(&mut app, &mut tui, &mut server, &[held]).await?;
    assert!(!app.chat_widget.has_active_view());
    keys(&mut app, &mut tui, &mut server, &[enter]).await?;
    let picker = interface::AUTOPILOT_PICKER_ID;
    assert_eq!(
        app.chat_widget.selected_index_for_present_view(picker),
        Some(0)
    );
    keys(&mut app, &mut tui, &mut server, &[held, release]).await?;
    assert!(app.chat_widget.active_view_is(picker));
    keys(&mut app, &mut tui, &mut server, &[KeyCode::Esc.into()]).await?;
    assert!(!app.chat_widget.has_active_view());
    assert_eq!(app.chat_widget.composer_text_with_pending(), "");
    assert!(app.reconnect.offline);
    assert_eq!(fake.requests(), host_requests);
    assert!(ops.try_recv().is_err());
    assert!(
        !std::iter::from_fn(|| events.try_recv().ok())
            .any(|event| matches!(event, crate::app_event::AppEvent::CodexOp(_)))
    );
    // Once normal composer input resumes, Up recalls the submitted command from local history.
    app.chat_widget.handle_key_event(KeyCode::Up.into());
    assert_eq!(app.chat_widget.composer_text_with_pending(), "/alab menu");
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn physical_autopilot_bindings_preserve_the_draft_and_queue_while_busy_or_disconnected()
-> color_eyre::eyre::Result<()> {
    let direct = KeyEvent::from(KeyCode::F(12));
    let prefix = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
    let third = KeyEvent::from(KeyCode::Char('3'));
    for (busy, disconnected) in [(false, false), (true, false), (false, true)] {
        let root = codex_alab_tui::test_paths::short_tempdir()?;
        let fake = FakeAterm::start(root.path(), Mode::Current).await?;
        let _interface = scope();
        interface::route_aterm_for_tests(fake.endpoint());
        let shared = Box::leak(Box::new(Shared::new()));
        let _host = codex_alab_tui::boxes::scoped(shared);
        shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Cruise);

        let (mut app, _events, mut ops) = make_test_app_with_channels().await;
        let thread = ThreadId::new();
        attach_thread(&mut app, thread);
        let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        if busy {
            app.chat_widget.handle_server_notification(
                ServerNotification::TurnStarted(TurnStartedNotification {
                    thread_id: thread.to_string(),
                    turn: Turn {
                        id: "running".into(),
                        root_turn_id: None,
                        items: Vec::new(),
                        items_view: Default::default(),
                        status: TurnStatus::InProgress,
                        error: None,
                        started_at: None,
                        completed_at: None,
                        duration_ms: None,
                    },
                }),
                /*replay_kind*/ None,
            );
            app.chat_widget
                .apply_external_edit("queued follow-up".into());
            keys(&mut app, &mut tui, &mut server, &[KeyCode::Tab.into()]).await?;
            assert_eq!(
                app.chat_widget.queued_user_message_texts(),
                ["queued follow-up"]
            );
        }
        assert_eq!(app.chat_widget.is_user_turn_pending_or_running(), busy);
        app.chat_widget
            .apply_external_edit("Review Cruise\nand Land".into());
        keys(&mut app, &mut tui, &mut server, &[KeyCode::Left.into()]).await?;
        let draft = app.chat_widget.composer_draft_snapshot();
        assert_eq!(draft.cursor, draft.text.len() - 1);
        let queue = app.chat_widget.queued_user_message_texts();
        if disconnected {
            app.app_server_target = crate::AppServerTarget::Remote {
                endpoint: crate::resolve_remote_addr("ws://127.0.0.1:8765")?,
            };
            assert!(app.begin_reconnect());
            let size = ratatui::layout::Size::new(28, 24);
            tui.terminal = crate::tui::Terminal::with_screen_size_and_cursor_position_for_test(
                ratatui::backend::CrosstermBackend::new(std::io::stdout()),
                size,
                Position::default(),
            );
            app.render_chat_widget_frame(&mut tui, size)?;
            let arrow = find_text(
                crate::custom_terminal::test_support::last_rendered_buffer(&tui.terminal),
                "▾",
            );
            assert!(interface::autopilot_disclosure_hit(arrow.x, arrow.y));
            for kind in [
                MouseEventKind::Down(MouseButton::Left),
                MouseEventKind::Up(MouseButton::Left),
            ] {
                app.handle_tui_event(&mut tui, &mut server, mouse(kind, arrow.x, arrow.y))
                    .await?;
            }
            assert!(
                app.chat_widget
                    .active_view_is(interface::AUTOPILOT_PICKER_ID)
            );
            keys(&mut app, &mut tui, &mut server, &[KeyCode::Esc.into()]).await?;
        }
        let writes = || {
            fake.requests()
                .into_iter()
                .filter(|r| r.contains(" meta set "))
                .collect::<Vec<_>>()
        };

        // The default keymap leaves the picker unbound.
        keys(&mut app, &mut tui, &mut server, &[direct]).await?;
        assert!(!app.chat_widget.has_active_view());
        bind_picker(&mut app)?;
        for sequence in [&[direct][..], &[prefix, third][..]] {
            keys(&mut app, &mut tui, &mut server, sequence).await?;
            let picker = interface::AUTOPILOT_PICKER_ID;
            assert_eq!(
                app.chat_widget.selected_index_for_present_view(picker),
                Some(0),
                "fresh physical shortcut in busy={busy}, disconnected={disconnected}"
            );
            let last = *sequence.last().unwrap();
            let repeat = KeyEvent::new_with_kind(last.code, last.modifiers, KeyEventKind::Repeat);
            let released =
                KeyEvent::new_with_kind(last.code, last.modifiers, KeyEventKind::Release);
            // In the picker a fresh '3' chooses Off. A held chord completion may not do so.
            keys(&mut app, &mut tui, &mut server, &[repeat, released]).await?;
            assert_eq!(
                app.chat_widget.selected_index_for_present_view(picker),
                Some(0)
            );
            keys(&mut app, &mut tui, &mut server, &[KeyCode::Esc.into()]).await?;
            assert!(!app.chat_widget.has_active_view());
            assert_eq!(app.chat_widget.composer_draft_snapshot(), draft);
            assert_eq!(app.chat_widget.queued_user_message_texts(), queue);
            assert!(writes().is_empty());
        }
        keys(&mut app, &mut tui, &mut server, &[direct, third]).await?;
        let off = format!("@{} meta set autopilot off", fake.endpoint().sid);
        assert!(fake.wait_for(|host| host.requests().contains(&off)).await);
        assert_eq!(writes(), [off]);
        assert_eq!(app.chat_widget.composer_draft_snapshot(), draft);
        assert_eq!(app.chat_widget.queued_user_message_texts(), queue);
        if disconnected {
            assert!(app.reconnect.offline);
            assert!(matches!(
                app.handle_tui_event(
                    &mut tui,
                    &mut server,
                    TuiEvent::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
                )
                .await?,
                crate::app::AppRunControl::Exit(crate::app::ExitReason::UserRequested)
            ));
        }
        assert!(
            !std::iter::from_fn(|| ops.try_recv().ok())
                .any(|op| matches!(op, crate::app_command::AppCommand::UserTurn { .. }))
        );
        server.shutdown().await?;
    }
    Ok(())
}

#[tokio::test]
async fn an_existing_picker_and_the_vanilla_composer_keep_keyboard_ownership()
-> color_eyre::eyre::Result<()> {
    let _interface = scope();
    let shared = Box::leak(Box::new(Shared::new()));
    let _host = codex_alab_tui::boxes::scoped(shared);
    shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Cruise);
    let (mut app, _events, _ops) = make_test_app_with_channels().await;
    attach_thread(&mut app, ThreadId::new());
    bind_picker(&mut app)?;
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let direct = KeyEvent::from(KeyCode::F(12));
    let prefix = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
    let third = KeyEvent::from(KeyCode::Char('3'));
    for disconnected in [false, true] {
        interface::set_enabled(true);
        app.chat_widget.show_selection_view(SelectionViewParams {
            view_id: Some("existing-list"),
            allow_input_when_disconnected: true,
            items: vec![SelectionItem {
                name: "Existing choice".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        if disconnected {
            app.app_server_target = crate::AppServerTarget::Remote {
                endpoint: crate::resolve_remote_addr("ws://127.0.0.1:8765")?,
            };
            assert!(app.begin_reconnect());
        }
        keys(&mut app, &mut tui, &mut server, &[direct, prefix]).await?;
        assert!(app.chat_widget.active_view_is("existing-list"));
        keys(&mut app, &mut tui, &mut server, &[third]).await?;
        assert!(
            !app.chat_widget
                .active_view_is(interface::AUTOPILOT_PICKER_ID)
        );
        keys(&mut app, &mut tui, &mut server, &[KeyCode::Esc.into()]).await?;
        assert!(!interface::set_enabled(false));
        for sequence in [&[direct][..], &[prefix, third][..]] {
            keys(&mut app, &mut tui, &mut server, sequence).await?;
            assert!(
                !app.chat_widget
                    .active_view_is(interface::AUTOPILOT_PICKER_ID)
            );
        }
    }
    server.shutdown().await?;
    Ok(())
}
