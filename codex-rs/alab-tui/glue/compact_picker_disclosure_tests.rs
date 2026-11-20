// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::custom_terminal::test_support::last_rendered_buffer;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use codex_alab_tui::session::Autopilot;
use codex_alab_tui::session::HarnessWord;
use codex_alab_tui::session::Shared;
use crossterm::event::MouseButton::Left;
use crossterm::event::MouseEventKind::Down;
use crossterm::event::MouseEventKind::Drag;
use crossterm::event::MouseEventKind::Moved;
use crossterm::event::MouseEventKind::Up;
use ratatui::layout::Size;

#[tokio::test]
async fn narrow_picker_keeps_current_mode_and_keyboard_focus_distinct()
-> color_eyre::eyre::Result<()> {
    use crate::app::owned_transcript::tests::buffer_text;
    use ratatui::style::Modifier;

    let root = codex_alab_tui::test_paths::short_tempdir()?;
    let fake = FakeAterm::start(root.path(), Mode::Current).await?;
    let _interface = scope();
    interface::route_aterm_for_tests(fake.endpoint());
    let shared = Box::leak(Box::new(Shared::new()));
    let _host = codex_alab_tui::boxes::scoped(shared);
    let (mut app, _events, _ops) = make_test_app_with_channels().await;
    attach_thread(&mut app, ThreadId::new());
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.set_owned_screen(true)?;
    let paint =
        |app: &mut App, tui: &mut crate::tui::Tui, width| -> color_eyre::eyre::Result<String> {
            let size = Size::new(width, 24);
            tui.terminal = crate::tui::Terminal::with_screen_size_and_cursor_position_for_test(
                ratatui::backend::CrosstermBackend::new(std::io::stdout()),
                size,
                Position::default(),
            );
            app.render_chat_widget_frame(tui, size)?;
            Ok(buffer_text(last_rendered_buffer(&tui.terminal)))
        };

    for (mode, policy, current, full_width, navigate, neighbor) in [
        (
            HarnessWord::Cruise,
            Autopilot::Active,
            "1. Cruise",
            21,
            KeyCode::Down,
            "2. Land",
        ),
        (
            HarnessWord::Land,
            Autopilot::Active,
            "2. Land",
            19,
            KeyCode::Down,
            "3. Off",
        ),
        (
            HarnessWord::Off,
            Autopilot::Passive,
            "3. Off",
            18,
            KeyCode::Up,
            "2. Land",
        ),
    ] {
        shared.set_autopilot_with_word(policy, mode);
        app.chat_widget.apply_external_edit("/alab menu".into());
        app.chat_widget.handle_key_event(KeyCode::Enter.into());
        assert!(
            app.chat_widget
                .active_view_is(interface::AUTOPILOT_PICKER_ID)
        );
        for focused in [true, false] {
            if !focused {
                app.chat_widget.handle_key_event(navigate.into());
            }
            // Keep the same picker open as the long marker first fits, then resize it back.
            for width in (11..=21).chain([11]) {
                let screen = paint(&mut app, &mut tui, width)?;
                let cursor = if focused { '›' } else { ' ' };
                let compact = width < full_width;
                let expected = if compact {
                    format!("{cursor}✓{current}")
                } else {
                    format!("{cursor} {current} (current)")
                };
                assert!(
                    screen.lines().any(|row| row == expected),
                    "{width} × 24: {screen}"
                );
                assert_eq!(
                    screen.matches('✓').count(),
                    usize::from(compact),
                    "{screen}"
                );
                assert_eq!(
                    screen.matches("(current)").count(),
                    usize::from(!compact),
                    "{screen}"
                );
                if !focused {
                    let focus = format!("› {neighbor}");
                    assert!(screen.lines().any(|row| row == focus), "{screen}");
                }
                let buffer = last_rendered_buffer(&tui.terminal);
                let rows =
                    ["1. Cruise", "2. Land", "3. Off"].map(|label| find_text(buffer, label).y);
                assert_eq!(rows, [rows[0], rows[0] + 1, rows[0] + 2], "{screen}");
            }
        }
        if mode == HarnessWord::Land {
            // The check belongs to the already painted Land row after focus moved to Off.
            let check = find_text(last_rendered_buffer(&tui.terminal), "✓2. Land");
            let TuiEvent::Mouse(hover) = mouse(Moved, check.x, check.y) else {
                unreachable!()
            };
            assert!(app.chat_widget.alab_picker_mouse(hover));
            app.render_chat_widget_frame(&mut tui, Size::new(11, 24))?;
            let buffer = last_rendered_buffer(&tui.terminal);
            assert!(buffer[check].modifier.contains(Modifier::UNDERLINED));
            find_text(buffer, "› 3. Off");
        }
        assert_eq!(shared.harness_word(), mode);
        if mode == HarnessWord::Off {
            shared.set_autopilot_with_word(Autopilot::Passive, HarnessWord::AgentOff);
            app.chat_widget.pre_draw_tick();
            let screen = paint(&mut app, &mut tui, 17)?;
            assert!(screen.contains("–  Cruise"), "{screen}");
            assert!(
                !screen.contains('✓') && !screen.contains("(current)"),
                "{screen}"
            );
            app.chat_widget.handle_key_event(KeyCode::Enter.into());
            assert!(
                app.chat_widget
                    .active_view_is(interface::AUTOPILOT_PICKER_ID)
            );
        }
        app.chat_widget.handle_key_event(KeyCode::Esc.into());
    }
    assert!(
        fake.requests()
            .iter()
            .all(|r| !r.contains(" meta set autopilot "))
    );
    tui.set_owned_screen(false)?;
    Ok(())
}

#[tokio::test]
async fn collapsed_disclosure_opens_the_picker_without_activating_a_stationary_second_click()
-> color_eyre::eyre::Result<()> {
    for (owned, width) in [(true, 28), (false, 28), (true, 11), (false, 11)] {
        let root = codex_alab_tui::test_paths::short_tempdir()?;
        let fake = FakeAterm::start(root.path(), Mode::Current).await?;
        let _interface = scope();
        interface::route_aterm_for_tests(fake.endpoint());
        let shared = Box::leak(Box::new(Shared::new()));
        let _host = codex_alab_tui::boxes::scoped(shared);
        shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Cruise);
        let (mut app, _events, mut ops) = make_test_app_with_channels().await;
        attach_thread(&mut app, ThreadId::new());
        let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let size = Size::new(width, 24);
        let mut tui = crate::tui::test_support::make_test_tui()?;
        tui.terminal = crate::tui::Terminal::with_screen_size_and_cursor_position_for_test(
            ratatui::backend::CrosstermBackend::new(std::io::stdout()),
            size,
            Position::default(),
        );
        tui.set_owned_screen(owned)?;
        app.chat_widget
            .apply_external_edit("unfinished draft".into());
        app.chat_widget.handle_key_event(KeyCode::Left.into());
        let draft = app.chat_widget.capture_thread_input_state();

        for disabled_while_held in [false, true] {
            app.render_chat_widget_frame(&mut tui, size)?;
            let arrow = find_text(last_rendered_buffer(&tui.terminal), "▾");
            app.handle_tui_event(&mut tui, &mut server, mouse(Down(Left), arrow.x, arrow.y))
                .await?;
            if disabled_while_held {
                // A host update can remove the action without an intervening user event.
                shared.set_autopilot_with_word(Autopilot::Passive, HarnessWord::AgentOff);
            } else {
                for x in [arrow.x + 1, arrow.x] {
                    app.handle_tui_event(&mut tui, &mut server, mouse(Drag(Left), x, arrow.y))
                        .await?;
                }
            }
            app.handle_tui_event(&mut tui, &mut server, mouse(Up(Left), arrow.x, arrow.y))
                .await?;
            assert!(!app.chat_widget.has_active_view());
            assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
            assert!(
                fake.requests()
                    .iter()
                    .all(|r| !r.contains(" meta set autopilot "))
            );
            if disabled_while_held {
                assert!(!interface::autopilot_disclosure_hit(arrow.x, arrow.y));
                shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Cruise);
            }
        }

        for escape in [true, false] {
            app.render_chat_widget_frame(&mut tui, size)?;
            let arrow = find_text(last_rendered_buffer(&tui.terminal), "▾");
            assert!(interface::autopilot_disclosure_hit(arrow.x, arrow.y));
            if width == 11 {
                assert_eq!(
                    interface::mode_mouse_hit(arrow.x - 1, arrow.y),
                    Some(HarnessWord::Land)
                );
                assert!(interface::mouse_hit(arrow.x + 1, arrow.y));
            }
            for kind in [Down(Left), Up(Left)] {
                app.handle_tui_event(&mut tui, &mut server, mouse(kind, arrow.x, arrow.y))
                    .await?;
            }
            assert!(
                app.chat_widget
                    .active_view_is(interface::AUTOPILOT_PICKER_ID)
            );
            app.render_chat_widget_frame(&mut tui, size)?;
            let picker = last_rendered_buffer(&tui.terminal);
            let off = find_text(picker, "3. Off");
            if width == 28 {
                find_text(picker, "Cruise (current)");
            }
            if escape {
                if owned && width == 28 {
                    // Here the picker puts a selectable row directly under the opener.
                    let TuiEvent::Mouse(hover) = mouse(Moved, arrow.x, arrow.y) else {
                        unreachable!()
                    };
                    assert!(app.chat_widget.alab_picker_mouse(hover));
                }
                for kind in [Down(Left), Up(Left)] {
                    app.handle_tui_event(&mut tui, &mut server, mouse(kind, arrow.x, arrow.y))
                        .await?;
                }
                assert!(
                    app.chat_widget
                        .active_view_is(interface::AUTOPILOT_PICKER_ID)
                );
                assert!(
                    fake.requests()
                        .iter()
                        .all(|r| !r.contains(" meta set autopilot "))
                );
                app.handle_tui_event(&mut tui, &mut server, TuiEvent::Key(KeyCode::Esc.into()))
                    .await?;
            } else {
                assert_ne!(off, arrow);
                for kind in [Down(Left), Up(Left)] {
                    app.handle_tui_event(&mut tui, &mut server, mouse(kind, off.x, off.y))
                        .await?;
                }
            }
            assert!(!app.chat_widget.has_active_view());
            assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
        }
        let expected = format!("@{} meta set autopilot off", fake.endpoint().sid);
        assert!(
            fake.wait_for(|host| host.requests().contains(&expected))
                .await
        );
        let mode_writes: Vec<_> = fake
            .requests()
            .into_iter()
            .filter(|r| r.contains(" meta set autopilot "))
            .collect();
        assert_eq!(mode_writes, [expected]);
        assert_eq!(shared.harness_word(), HarnessWord::Cruise);
        assert!(
            !std::iter::from_fn(|| ops.try_recv().ok())
                .any(|op| matches!(op, crate::app_command::AppCommand::UserTurn { .. }))
        );
        tui.set_owned_screen(false)?;
        server.shutdown().await?;
    }
    Ok(())
}
