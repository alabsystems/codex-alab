// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The native selection layer replays stationary clicks after release. Route only
//! painted edition controls through the existing composer ownership checks; drags
//! remain native text selection, including account/email drag-to-copy.

use super::App;
use codex_alab_tui::interface;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;

pub(super) fn replay_control_click(app: &mut App, mouse: MouseEvent) -> bool {
    if mouse.kind != MouseEventKind::Down(MouseButton::Left)
        || !(interface::account_mouse_hit(mouse.column, mouse.row)
            || interface::mouse_hit(mouse.column, mouse.row))
    {
        return false;
    }
    app.chat_widget.prepare_composer_mouse(mouse) && app.chat_widget.handle_composer_mouse(mouse)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::owned_transcript::tests::attach_thread;
    use crate::app::tests::make_test_app_with_channels;
    use crate::status::StatusAccountDisplay;
    use crate::tui::TuiEvent;
    use codex_alab_tui::banner::Depth;
    use codex_alab_tui::banner::Theme;
    use codex_app_server_protocol::RateLimitSnapshot;
    use codex_config::types::CopyOnSelect;
    use codex_protocol::ThreadId;
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Position;
    use ratatui::layout::Rect;

    fn scope() -> interface::Scope {
        let state = Box::leak(Box::new(interface::State::new()));
        let scope = interface::scoped(state);
        interface::configure(true);
        scope
    }

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> TuiEvent {
        TuiEvent::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }

    fn find_text(buffer: &Buffer, text: &str) -> Position {
        buffer
            .area
            .positions()
            .find(|position| {
                (position.x..buffer.area.right())
                    .map(|x| buffer[(x, position.y)].symbol())
                    .collect::<String>()
                    .starts_with(text)
            })
            .unwrap_or_else(|| panic!("painted text not found: {text}"))
    }

    #[tokio::test]
    async fn failed_periodic_read_ages_visible_quota_without_clearing_native_usage_or_recovery()
    -> color_eyre::eyre::Result<()> {
        use crate::app_event::AppEvent;
        use crate::app_event::RateLimitRefreshOrigin;
        use codex_app_server_protocol::GetAccountRateLimitsResponse;

        let _scope = scope();
        let (mut app, mut events, _operations) = make_test_app_with_channels().await;
        app.chat_widget.update_account_state(
            Some(StatusAccountDisplay::ChatGpt {
                email: Some("person@example.test".to_string()),
                plan: Some("Pro".to_string()),
                workspace: None,
            }),
            None,
            true,
            true,
        );
        let usage = |used| {
            serde_json::from_value::<RateLimitSnapshot>(serde_json::json!({
                "limitId": "codex",
                "primary": { "usedPercent": 18, "windowDurationMins": 300 },
                "secondary": { "usedPercent": used, "windowDurationMins": 10_080 }
            }))
            .expect("account usage")
        };
        app.chat_widget.on_rate_limit_snapshot(Some(usage(27)));
        assert_eq!(
            interface::status_snapshot().unwrap().weekly.as_deref(),
            Some("weekly 73% left")
        );
        app.chat_widget.alab_set_usage_captured_at_for_test(
            chrono::Local::now() - chrono::Duration::minutes(16),
        );
        app.chat_widget.hold_rate_limit_recovery();
        let mut server = Box::pin(crate::start_embedded_app_server_for_picker(
            app.chat_widget.config_ref(),
        ))
        .await?;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        let (request_id, hard_stop_generation) = app
            .rate_limit_refresh_state
            .start(
                RateLimitRefreshOrigin::Periodic,
                &mut app.rate_limit_hard_stop_generation,
            )
            .unwrap();
        app.handle_event(
            &mut tui,
            &mut server,
            AppEvent::RateLimitsLoaded {
                request_id,
                origin: RateLimitRefreshOrigin::Periodic,
                hard_stop_generation,
                result: Err("transient periodic read failure".to_string()),
            },
        )
        .await?;
        let stale = interface::status_snapshot().unwrap();
        assert!(stale.usage_stale);
        assert_eq!(stale.account, "person@example.test");
        assert_eq!(stale.plan.as_deref(), Some("Pro"));
        assert_eq!(stale.weekly.as_deref(), Some("weekly stale"));
        assert_eq!(stale.five_hour.as_deref(), Some("5h stale"));
        assert!(stale.usage_checked_at.is_some());
        assert!(
            app.chat_widget.hold_rate_limit_recovery(),
            "a display-only age change cannot authorize queued model work"
        );
        // The native /status card still owns and classifies the accepted snapshot.
        while events.try_recv().is_ok() {}
        app.chat_widget.add_status_output(false, None);
        let native = match events.try_recv()? {
            AppEvent::InsertHistoryCell(cell) => cell
                .display_lines(120)
                .into_iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
            other => panic!("expected native status output, got {other:?}"),
        };
        assert!(native.contains("73% left"), "{native}");

        let (request_id, hard_stop_generation) = app
            .rate_limit_refresh_state
            .start(
                RateLimitRefreshOrigin::Periodic,
                &mut app.rate_limit_hard_stop_generation,
            )
            .unwrap();
        app.handle_event(
            &mut tui,
            &mut server,
            AppEvent::RateLimitsLoaded {
                request_id,
                origin: RateLimitRefreshOrigin::Periodic,
                hard_stop_generation,
                result: Ok(GetAccountRateLimitsResponse {
                    ordinary_usage_allowed: Some(true),
                    account_id: None,
                    rate_limit_upsell: None,
                    rate_limits: usage(61),
                    rate_limits_by_limit_id: None,
                    rate_limit_reset_credits: None,
                }),
            },
        )
        .await?;
        let refreshed = interface::status_snapshot().unwrap();
        assert!(!refreshed.usage_stale);
        assert_eq!(refreshed.weekly.as_deref(), Some("weekly 39% left"));
        assert_eq!(refreshed.five_hour.as_deref(), Some("5h 82% left"));
        assert_eq!(refreshed.account, stale.account);
        assert_eq!(refreshed.plan, stale.plan);
        assert_ne!(refreshed.usage_checked_at, stale.usage_checked_at);
        assert!(!app.chat_widget.hold_rate_limit_recovery());
        server.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn replay_only_handles_painted_left_control_presses() {
        let _scope = scope();
        let (mut app, _events, _operations) = make_test_app_with_channels().await;
        interface::set_status(interface::InterfaceStatus {
            account: "person@example.test".to_string(),
            plan: Some("Pro".to_string()),
            weekly: Some("weekly 73% left".to_string()),
            branch: Some("BRANCH-X".to_string()),
            model: Some("MODEL-X".to_string()),
            ..Default::default()
        });
        let area = Rect::new(0, 3, 180, 1);
        let mut buffer = Buffer::empty(area);
        interface::render_status_current(area, &mut buffer, Depth::Mono, Theme::default());
        let account = find_text(&buffer, "person@example.test");
        let branch = find_text(&buffer, "BRANCH-X");
        let model = find_text(&buffer, "MODEL-X");
        app.chat_widget
            .rendered_selection
            .borrow_mut()
            .register(area, false);
        for event in [
            mouse(
                MouseEventKind::Down(MouseButton::Right),
                account.x,
                account.y,
            ),
            mouse(
                MouseEventKind::Drag(MouseButton::Left),
                account.x,
                account.y,
            ),
            mouse(MouseEventKind::Up(MouseButton::Left), account.x, account.y),
            mouse(MouseEventKind::ScrollDown, account.x, account.y),
            mouse(MouseEventKind::Down(MouseButton::Left), branch.x, branch.y),
            mouse(MouseEventKind::Down(MouseButton::Left), model.x, model.y),
        ] {
            let TuiEvent::Mouse(mouse) = event else {
                unreachable!()
            };
            assert!(!replay_control_click(&mut app, mouse));
            assert_eq!(
                app.chat_widget.rendered_selection.borrow().areas,
                vec![area]
            );
        }
        assert!(replay_control_click(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: account.x,
                row: account.y,
                modifiers: KeyModifiers::NONE,
            }
        ));
        let selection = app.chat_widget.rendered_selection.borrow();
        assert_eq!(
            selection.areas,
            vec![area],
            "replay does not reset native selection state"
        );
        assert!(app.chat_widget.has_active_view());
    }

    #[tokio::test]
    async fn full_owned_screen_route_opens_account_and_quota_clicks_and_preserves_native_selection()
    -> color_eyre::eyre::Result<()> {
        let _scope = scope();
        let (mut app, _events, _operations) = make_test_app_with_channels().await;
        attach_thread(&mut app, ThreadId::new());
        app.local_settings.tui.copy_on_select = CopyOnSelect::Never;
        app.chat_widget.update_account_state(
            Some(StatusAccountDisplay::ChatGpt {
                email: Some("a@b.test".to_string()),
                plan: Some("Pro".to_string()),
                workspace: None,
            }),
            None,
            true,
            true,
        );
        let usage: RateLimitSnapshot = serde_json::from_value(serde_json::json!({
            "limitId": "codex",
            "primary": { "usedPercent": 18, "windowDurationMins": 300 },
            "secondary": { "usedPercent": 27, "windowDurationMins": 10_080 }
        }))?;
        app.chat_widget.on_rate_limit_snapshot(Some(usage));
        app.chat_widget
            .apply_external_edit("unfinished prompt".to_string());
        app.chat_widget
            .handle_key_event(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        let draft = app.chat_widget.capture_thread_input_state();
        let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        tui.set_owned_screen(true)?;
        let size = tui.terminal.size()?;
        app.render_owned_transcript(&mut tui, size)?;
        let cursor = tui.terminal.last_known_cursor_pos;

        for text in ["a@b.test", "weekly 73% left"] {
            app.render_owned_transcript(&mut tui, size)?;
            let target = find_text(
                crate::custom_terminal::test_support::last_rendered_buffer(&tui.terminal),
                text,
            );
            assert!(interface::account_mouse_hit(target.x, target.y));
            app.handle_tui_event(
                &mut tui,
                &mut server,
                mouse(MouseEventKind::Down(MouseButton::Left), target.x, target.y),
            )
            .await?;
            assert!(
                !app.chat_widget.has_active_view(),
                "press remains eligible for native drag-to-copy"
            );
            app.handle_tui_event(
                &mut tui,
                &mut server,
                mouse(MouseEventKind::Up(MouseButton::Left), target.x, target.y),
            )
            .await?;
            assert!(app.chat_widget.has_active_view());
            assert!(
                crate::chatwidget::tests::render_bottom_popup(&app.chat_widget, 80)
                    .contains("Account details")
            );
            assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
            app.handle_tui_event(
                &mut tui,
                &mut server,
                TuiEvent::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            )
            .await?;
            app.render_owned_transcript(&mut tui, size)?;
            assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
            assert_eq!(tui.terminal.last_known_cursor_pos, cursor);
        }

        // Dragging the account itself must copy text rather than opening its details.
        app.render_owned_transcript(&mut tui, size)?;
        let account = find_text(
            crate::custom_terminal::test_support::last_rendered_buffer(&tui.terminal),
            "a@b.test",
        );
        for event in [
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                account.x,
                account.y,
            ),
            mouse(
                MouseEventKind::Drag(MouseButton::Left),
                account.x + 6,
                account.y,
            ),
            mouse(
                MouseEventKind::Up(MouseButton::Left),
                account.x + 6,
                account.y,
            ),
        ] {
            app.handle_tui_event(&mut tui, &mut server, event).await?;
        }
        assert!(!app.chat_widget.has_active_view());
        assert!(
            app.chat_widget
                .rendered_selection
                .borrow()
                .view
                .has_selection_range()
        );
        assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
        app.handle_tui_event(
            &mut tui,
            &mut server,
            TuiEvent::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        )
        .await?;

        // Other fields in the very same footer retain the native selection gesture.
        app.render_owned_transcript(&mut tui, size)?;
        let buffer = crate::custom_terminal::test_support::last_rendered_buffer(&tui.terminal);
        let passive = find_text(buffer, "gpt-test");
        assert!(!interface::account_mouse_hit(passive.x, passive.y));
        for event in [
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                passive.x,
                passive.y,
            ),
            mouse(
                MouseEventKind::Drag(MouseButton::Left),
                passive.x + 3,
                passive.y,
            ),
            mouse(
                MouseEventKind::Up(MouseButton::Left),
                passive.x + 3,
                passive.y,
            ),
        ] {
            assert!(app.handle_rendered_selection_event(&mut tui, &event)?);
        }
        assert!(
            app.chat_widget
                .rendered_selection
                .borrow()
                .view
                .has_selection_range()
        );
        assert_eq!(app.chat_widget.capture_thread_input_state(), draft);

        {
            let enabled = false;
            app.render_owned_transcript(&mut tui, size)?;
            let buffer = crate::custom_terminal::test_support::last_rendered_buffer(&tui.terminal);
            let button = buffer
                .area
                .positions()
                .find(|position| interface::mouse_hit(position.x, position.y))
                .expect("painted interface button");
            app.handle_tui_event(
                &mut tui,
                &mut server,
                mouse(MouseEventKind::Down(MouseButton::Left), button.x, button.y),
            )
            .await?;
            assert_eq!(interface::enabled(), enabled);
            assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
            assert!(
                !app.chat_widget
                    .rendered_selection
                    .borrow()
                    .view
                    .has_active_interaction()
            );
        }

        app.render_owned_transcript(&mut tui, size)?;
        let buffer = crate::custom_terminal::test_support::last_rendered_buffer(&tui.terminal);
        let button = find_text(buffer, "[ALab off]");
        assert!(interface::mouse_hit(button.x, button.y));
        app.handle_tui_event(
            &mut tui,
            &mut server,
            mouse(MouseEventKind::Down(MouseButton::Left), button.x, button.y),
        )
        .await?;
        assert!(
            interface::enabled(),
            "visible recovery button restores enhancements"
        );
        assert_eq!(app.chat_widget.capture_thread_input_state(), draft);

        tui.set_owned_screen(false)?;
        server.shutdown().await?;
        Ok(())
    }
}
