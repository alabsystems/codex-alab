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

impl App {
    pub(in crate::app) fn alab_observe_interface_pointer(
        &mut self,
        tui: &crate::tui::Tui,
        event: &crate::tui::TuiEvent,
    ) -> bool {
        use crate::tui::TuiEvent;
        use crossterm::event::KeyEventKind;

        match event {
            TuiEvent::Draw => self.alab_clear_covered_switch_hover(tui),
            _ => {
                let pos = match event {
                    TuiEvent::Mouse(mouse)
                        if mouse.kind == MouseEventKind::Moved
                            && mouse.modifiers.is_empty()
                            && self.alab_switch_hover_available(tui) =>
                    {
                        Some((mouse.column, mouse.row).into())
                    }
                    _ => None,
                };
                if interface::hover_named_mode(pos) {
                    tui.frame_requester().schedule_frame();
                }
            }
        }

        let followup = self.chat_widget.alab_picker_click_pending();
        if followup
            || self
                .chat_widget
                .active_view_is(interface::AUTOPILOT_PICKER_ID)
        {
            if self.alab_picker_is_covered()
                || matches!(
                    event,
                    TuiEvent::Resize(_)
                        | TuiEvent::Resume
                        | TuiEvent::FocusLost
                        | TuiEvent::FocusGained
                        | TuiEvent::Paste(_)
                )
                || matches!(event, TuiEvent::Key(key) if key.kind != KeyEventKind::Release)
            {
                self.chat_widget.alab_cancel_picker_mouse();
            }
            if followup {
                let shortcuts_open =
                    self.key_chord_matcher.is_pending() || self.chat_widget.quick_dial_menu_open();
                if self
                    .chat_widget
                    .alab_consume_picker_followup(event, shortcuts_open)
                {
                    return true;
                }
            }
        }
        self.chat_widget.alab_capture_composer_press(event);
        false
    }

    fn alab_picker_is_covered(&self) -> bool {
        self.conversation_frame.chooser_area().is_some() || self.alab_composer_is_covered()
    }

    fn alab_composer_is_covered(&self) -> bool {
        self.overlay.is_some()
            || self.conversation_frame.chooser_covers_transcript()
            || self.sidebar().is_some_and(|pane| pane.close.is_some())
            || (self.startup_protected_input_boundary && self.startup_pending_protected_request)
    }

    fn alab_switch_hover_available(&self, tui: &crate::tui::Tui) -> bool {
        tui.is_owned_screen()
            && tui.is_terminal_focused()
            && tui.mouse_capture != crate::tui::MouseCapture::Disabled
            && !self.alab_composer_is_covered()
            && !self.key_chord_matcher.is_pending()
            && !self.sidebar_has_focus()
            && self.pane_resize.is_none()
            && self.divider_drag.is_none()
            && self.chat_widget.alab_autopilot_input_available()
    }

    pub(in crate::app) fn alab_clear_covered_switch_hover(&self, tui: &crate::tui::Tui) {
        if !self.alab_switch_hover_available(tui) {
            interface::hover_named_mode(None);
        }
    }

    /// The local switch remains available to the conversation while its server reconnects.
    pub(in crate::app) fn alab_open_disconnected_autopilot(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> bool {
        self.reconnect.presentation == crate::app::reconnect::ReconnectPresentation::Conversation
            && !self.alab_picker_is_covered()
            && self.chat_widget.alab_handle_autopilot_input(
                key,
                self.routed_key_activation.take(),
                /*allow_draft_command*/ true,
            )
    }

    pub(in crate::app) fn alab_dispatch_picker_mouse(
        &mut self,
        tui: &mut crate::tui::Tui,
        event: &crate::tui::TuiEvent,
    ) -> color_eyre::eyre::Result<bool> {
        let crate::tui::TuiEvent::Mouse(mouse) = event else {
            return Ok(false);
        };
        if self.alab_picker_is_covered() {
            return Ok(false);
        }
        if self.chat_widget.alab_disclosure_release(*mouse) {
            // Preparing the press can flush a paste and reflow the composer. Resolve
            // the stationary release against the new geometry in either renderer.
            let size = tui.prepare_draw_size()?;
            self.render_chat_widget_frame(tui, size)?;
            self.chat_widget.alab_finish_disclosure(*mouse);
            return Ok(true);
        }
        Ok(self.chat_widget.alab_picker_mouse(*mouse)
            || self.chat_widget.alab_press_disclosure(*mouse))
    }
}

pub(super) fn replay_control_click(app: &mut App, mouse: MouseEvent) -> bool {
    if mouse.kind != MouseEventKind::Down(MouseButton::Left)
        || !(interface::account_mouse_hit(mouse.column, mouse.row)
            || interface::mouse_hit(mouse.column, mouse.row))
    {
        return false;
    }
    app.chat_widget.prepare_composer_mouse(mouse) && app.chat_widget.handle_composer_mouse(mouse)
}

#[cfg(all(test, unix))]
#[path = "live_banner_tests.rs"]
mod live_banner_tests;

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

    mod disclosure {
        include!("compact_picker_disclosure_tests.rs");
    }

    mod keyboard {
        include!("autopilot_keyboard_tests.rs");
    }

    mod wide_switch_hover {
        include!("wide_switch_hover_tests.rs");
    }

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

    // The closing bracket stays in place when a pending update shortens the edition label.
    fn painted_edition_end(buffer: &Buffer) -> Position {
        buffer
            .area
            .positions()
            .find(|pos| buffer[*pos].symbol() == "]" && interface::mouse_hit(pos.x, pos.y))
            .expect("closing bracket in the painted edition button")
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
            app.chat_widget.alab_capture_composer_press(&event);
            let TuiEvent::Mouse(mouse) = event else {
                unreachable!()
            };
            assert!(!replay_control_click(&mut app, mouse));
            assert_eq!(
                app.chat_widget.rendered_selection.borrow().areas,
                vec![area]
            );
        }
        let press = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: account.x,
            row: account.y,
            modifiers: KeyModifiers::NONE,
        };
        app.chat_widget
            .alab_capture_composer_press(&TuiEvent::Mouse(press));
        assert!(replay_control_click(&mut app, press));
        let selection = app.chat_widget.rendered_selection.borrow();
        assert_eq!(
            selection.areas,
            vec![area],
            "replay does not reset native selection state"
        );
        assert!(app.chat_widget.has_active_view());
    }

    #[tokio::test]
    async fn paste_reflow_cannot_move_the_edition_switch_under_an_existing_press()
    -> color_eyre::eyre::Result<()> {
        let _scope = scope();
        let (mut app, _events, _operations) = make_test_app_with_channels().await;
        attach_thread(&mut app, ThreadId::new());
        let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        tui.set_owned_screen(true)?;
        let size = tui.terminal.size()?;
        let initial = "draft: ";
        let pasted = "first line\nsecond line";
        let draft = format!("{initial}{pasted}");

        // Locate the control in the two-line layout without assuming a particular footer height.
        app.chat_widget.apply_external_edit(draft.clone());
        app.render_owned_transcript(&mut tui, size)?;
        let target = painted_edition_end(
            crate::custom_terminal::test_support::last_rendered_buffer(&tui.terminal),
        );
        // An empty draft spends the extra row on the "?" hint; an existing draft reflows.
        app.chat_widget.apply_external_edit(initial.to_string());
        app.render_owned_transcript(&mut tui, size)?;
        let last = crate::custom_terminal::test_support::last_rendered_buffer(&tui.terminal);
        let previous = painted_edition_end(last);
        assert!(
            target.y < previous.y,
            "switch stayed at {previous:?} with target {target:?}"
        );
        assert_eq!(last[target].symbol(), " ");

        app.handle_tui_event(&mut tui, &mut server, TuiEvent::Paste(pasted.to_string()))
            .await?;
        let last = crate::custom_terminal::test_support::last_rendered_buffer(&tui.terminal);
        assert_eq!(
            last[target].symbol(),
            " ",
            "paste has not painted the new geometry"
        );
        assert!(!interface::mouse_hit(target.x, target.y));
        let press = || mouse(MouseEventKind::Down(MouseButton::Left), target.x, target.y);
        app.handle_tui_event(&mut tui, &mut server, press()).await?;
        assert!(
            interface::enabled(),
            "press on blank cell {target:?} toggled the edition"
        );
        assert_eq!(app.chat_widget.composer_text_with_pending(), draft);
        app.handle_tui_event(
            &mut tui,
            &mut server,
            mouse(MouseEventKind::Up(MouseButton::Left), target.x, target.y),
        )
        .await?;
        assert!(interface::enabled());

        app.render_owned_transcript(&mut tui, size)?;
        let last = crate::custom_terminal::test_support::last_rendered_buffer(&tui.terminal);
        assert_eq!(painted_edition_end(last), target);
        app.handle_tui_event(&mut tui, &mut server, press()).await?;
        assert!(!interface::enabled(), "the newly painted switch must work");
        assert_eq!(app.chat_widget.composer_text_with_pending(), draft);
        tui.set_owned_screen(false)?;
        server.shutdown().await?;
        Ok(())
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

    #[tokio::test]
    async fn double_clicking_a_timer_preset_cannot_toggle_the_edition_underneath()
    -> color_eyre::eyre::Result<()> {
        use crate::custom_terminal::test_support::last_rendered_buffer;
        use codex_alab_aterm::fake::FakeAterm;
        use codex_alab_aterm::fake::Mode;
        use codex_alab_tui::session::Autopilot;
        use codex_alab_tui::session::HarnessWord;
        use codex_alab_tui::session::Shared;
        use crossterm::event::MouseButton::Left;
        use crossterm::event::MouseButton::Right;
        use crossterm::event::MouseEventKind::Down;
        use crossterm::event::MouseEventKind::Up;

        let root = codex_alab_tui::test_paths::short_tempdir()?;
        let fake = FakeAterm::start(root.path(), Mode::Current).await?;
        let _interface = scope();
        interface::route_aterm_for_tests(fake.endpoint());
        let shared = Box::leak(Box::new(Shared::new()));
        let _automation = codex_alab_tui::boxes::scoped(shared);
        shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Land);
        let (mut app, _events, _ops) = make_test_app_with_channels().await;
        attach_thread(&mut app, ThreadId::new());
        app.chat_widget
            .apply_external_edit("unfinished prompt".into());
        app.chat_widget.handle_key_event(KeyCode::Left.into());
        let draft = app.chat_widget.capture_thread_input_state();
        let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        tui.set_owned_screen(true)?;
        let size = tui.terminal.size()?;
        app.render_chat_widget_frame(&mut tui, size)?;
        let buffer = last_rendered_buffer(&tui.terminal);
        let end = painted_edition_end(buffer);
        let edition = Position::new(end.x - 1, end.y);
        assert!(interface::mouse_hit(edition.x, edition.y));
        let mode = find_text(buffer, "Cruise");
        for kind in [Down(Right), Up(Right)] {
            app.handle_tui_event(&mut tui, &mut server, mouse(kind, mode.x, mode.y))
                .await?;
        }
        app.render_chat_widget_frame(&mut tui, size)?;
        let preset = find_text(last_rendered_buffer(&tui.terminal), "Cruise for 4 hours");
        assert_eq!(preset.y, edition.y, "timer row covers the edition switch");
        for kind in [Down(Left), Up(Left)] {
            app.handle_tui_event(&mut tui, &mut server, mouse(kind, edition.x, edition.y))
                .await?;
        }
        assert!(!app.chat_widget.has_active_view());

        app.render_chat_widget_frame(&mut tui, size)?;
        assert!(interface::mouse_hit(edition.x, edition.y));
        for kind in [Down(Left), Up(Left)] {
            app.handle_tui_event(&mut tui, &mut server, mouse(kind, edition.x, edition.y))
                .await?;
        }
        assert!(
            interface::enabled(),
            "second click reached the edition switch"
        );
        assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
        let request = format!(
            "@{} meta set autopilot cruise until=+4h",
            fake.endpoint().sid
        );
        assert!(
            fake.wait_for(|fake| fake.requests().contains(&request))
                .await
        );
        let writes: Vec<_> = fake
            .requests()
            .into_iter()
            .filter(|request| request.contains(" meta set autopilot "))
            .collect();
        assert_eq!(writes, [request]);

        // The adjacent cell of this same control still responds to a fresh click.
        assert!(interface::mouse_hit(end.x, end.y));
        for kind in [Down(Left), Up(Left)] {
            app.handle_tui_event(&mut tui, &mut server, mouse(kind, end.x, end.y))
                .await?;
        }
        assert!(!interface::enabled());
        assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
        tui.set_owned_screen(false)?;
        server.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn autopilot_timer_choices_fit_native_picker_widths() -> color_eyre::eyre::Result<()> {
        use crate::app::owned_transcript::tests::buffer_text;
        use crate::custom_terminal::test_support::last_rendered_buffer;
        use codex_alab_tui::session::Autopilot;
        use codex_alab_tui::session::HarnessWord;
        use codex_alab_tui::session::Shared;
        use ratatui::layout::Size;

        let _interface = scope();
        let shared = Box::leak(Box::new(Shared::new()));
        let _automation = codex_alab_tui::boxes::scoped(shared);
        shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Land);
        let (mut app, _events, _ops) = make_test_app_with_channels().await;
        attach_thread(&mut app, ThreadId::new());
        app.chat_widget.apply_external_edit("/alab menu".into());
        app.chat_widget.handle_key_event(KeyCode::Enter.into());

        for width in [14, 48, 50, 80] {
            let size = Size::new(width, 24);
            let mut tui = crate::tui::test_support::make_test_tui()?;
            tui.terminal = crate::tui::Terminal::with_screen_size_and_cursor_position_for_test(
                ratatui::backend::CrosstermBackend::new(std::io::stdout()),
                size,
                Position::default(),
            );
            tui.set_owned_screen(true)?;
            app.render_chat_widget_frame(&mut tui, size)?;
            let screen = buffer_text(last_rendered_buffer(&tui.terminal));
            tui.set_owned_screen(false)?;
            if width == 14 {
                assert_eq!(screen.matches("Autopilot").count(), 1, "{screen}");
                for name in ["1. Cruise", "2. Land", "3. Off"] {
                    assert!(screen.contains(name), "{screen}");
                }
                continue;
            }
            assert_eq!(
                screen.matches("Timers switch to Land.").count(),
                1,
                "{width} columns: {screen}"
            );
            let mut rows = screen
                .lines()
                .skip_while(|line| !line.contains("Cruise for 1 hour"));
            for label in [
                "Cruise for 1 hour",
                "Cruise for 2 hours",
                "Cruise for 4 hours",
                "Cruise until…",
            ] {
                assert!(
                    rows.next().is_some_and(|line| line.contains(label)),
                    "{width} columns: {label} in {screen}"
                );
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn hovering_a_timer_preset_underlines_it_and_leaves_keyboard_focus_on_its_row()
    -> color_eyre::eyre::Result<()> {
        use crate::custom_terminal::test_support::last_rendered_buffer;
        use codex_alab_aterm::fake::FakeAterm;
        use codex_alab_aterm::fake::Mode;
        use codex_alab_tui::session::Autopilot;
        use codex_alab_tui::session::HarnessWord;
        use codex_alab_tui::session::Shared;
        use ratatui::style::Modifier;

        for owned in [true, false] {
            let root = codex_alab_tui::test_paths::short_tempdir()?;
            let fake = FakeAterm::start(root.path(), Mode::Current).await?;
            let _interface = scope();
            interface::route_aterm_for_tests(fake.endpoint());
            let shared = Box::leak(Box::new(Shared::new()));
            let _automation = codex_alab_tui::boxes::scoped(shared);
            shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Land);
            let (mut app, _events, _ops) = make_test_app_with_channels().await;
            attach_thread(&mut app, ThreadId::new());
            let mut server =
                Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
            let mut tui = crate::tui::test_support::make_test_tui()?;
            tui.set_owned_screen(owned)?;
            let size = tui.terminal.size()?;
            app.chat_widget.apply_external_edit("/alab menu".into());
            app.chat_widget.handle_key_event(KeyCode::Enter.into());
            app.chat_widget
                .apply_external_edit("unfinished prompt".into());
            let draft = app.chat_widget.capture_thread_input_state();
            app.handle_tui_event(&mut tui, &mut server, TuiEvent::Key(KeyCode::Down.into()))
                .await?;
            app.render_chat_widget_frame(&mut tui, size)?;
            let before = last_rendered_buffer(&tui.terminal).clone();
            let name = "Cruise for 4 hours";
            let preset = find_text(&before, name);
            let header = find_text(&before, "Land. Timers switch to Land.");
            let focus = find_text(&before, "› 3. Off");
            let current = find_text(&before, "Land (current)");

            for (point, hovered) in [(preset, true), (header, false), (preset, true)] {
                let event = mouse(MouseEventKind::Moved, point.x, point.y);
                app.handle_tui_event(&mut tui, &mut server, event).await?;
                app.render_chat_widget_frame(&mut tui, size)?;
                let after = last_rendered_buffer(&tui.terminal);
                assert_eq!(find_text(after, "› 3. Off"), focus);
                assert_eq!(find_text(after, "Land (current)"), current);
                assert_eq!(find_text(after, name), preset);
                for offset in 0..name.len() as u16 {
                    let pos = Position::new(preset.x + offset, preset.y);
                    assert!(!before[pos].modifier.contains(Modifier::UNDERLINED));
                    assert_eq!(after[pos].modifier.contains(Modifier::UNDERLINED), hovered);
                }
                assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
                assert!(
                    fake.requests()
                        .iter()
                        .all(|r| !r.contains(" meta set autopilot "))
                );
            }
            app.handle_tui_event(&mut tui, &mut server, TuiEvent::Key(KeyCode::Up.into()))
                .await?;
            app.render_chat_widget_frame(&mut tui, size)?;
            let keyboard = last_rendered_buffer(&tui.terminal);
            find_text(keyboard, "› 2. Land (current)");
            assert!(!keyboard[preset].modifier.contains(Modifier::UNDERLINED));
            assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
            tui.set_owned_screen(false)?;
            server.shutdown().await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn wheel_reveals_a_timer_preset_before_a_click_sets_it() -> color_eyre::eyre::Result<()> {
        use crate::app::owned_transcript::tests::buffer_text;
        use crate::custom_terminal::test_support::last_rendered_buffer;
        use codex_alab_aterm::fake::FakeAterm;
        use codex_alab_aterm::fake::Mode;
        use codex_alab_tui::session::Autopilot;
        use codex_alab_tui::session::HarnessWord;
        use codex_alab_tui::session::Shared;
        use ratatui::layout::Size;

        for owned in [true, false] {
            let root = codex_alab_tui::test_paths::short_tempdir()?;
            let fake = FakeAterm::start(root.path(), Mode::Current).await?;
            let _interface = scope();
            interface::route_aterm_for_tests(fake.endpoint());
            let shared = Box::leak(Box::new(Shared::new()));
            let _automation = codex_alab_tui::boxes::scoped(shared);
            shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Land);
            let (mut app, _events, _ops) = make_test_app_with_channels().await;
            attach_thread(&mut app, ThreadId::new());
            let mut server =
                Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
            let mut tui = crate::tui::test_support::make_test_tui()?;
            let size = Size::new(80, 10);
            tui.terminal = crate::tui::Terminal::with_screen_size_and_cursor_position_for_test(
                ratatui::backend::CrosstermBackend::new(std::io::stdout()),
                size,
                Position::default(),
            );
            tui.set_owned_screen(owned)?;
            app.chat_widget.apply_external_edit("/alab menu".into());
            app.chat_widget.handle_key_event(KeyCode::Enter.into());
            app.chat_widget
                .apply_external_edit("unfinished prompt".into());
            let draft = app.chat_widget.capture_thread_input_state();
            app.render_chat_widget_frame(&mut tui, size)?;

            let preset = "Cruise for 4 hours";
            let buffer = last_rendered_buffer(&tui.terminal);
            assert!(buffer.area.bottom() <= size.height);
            let first_page = buffer_text(buffer);
            for label in ["1. Cruise", "› 2. Land (current)", "3. Off", "↓"] {
                assert!(first_page.contains(label), "missing {label}: {first_page}");
            }
            assert!(!first_page.contains(preset));
            let point = find_text(buffer, "3. Off");
            for _ in 0..7 {
                let wheel = mouse(MouseEventKind::ScrollDown, point.x, point.y);
                app.handle_tui_event(&mut tui, &mut server, wheel).await?;
                app.render_chat_widget_frame(&mut tui, size)?;
                if buffer_text(last_rendered_buffer(&tui.terminal)).contains(preset) {
                    break;
                }
            }
            assert!(
                app.chat_widget
                    .active_view_is(interface::AUTOPILOT_PICKER_ID)
            );
            let writes = || {
                fake.requests()
                    .into_iter()
                    .filter(|r| r.contains(" meta set autopilot "))
                    .collect::<Vec<_>>()
            };
            assert!(writes().is_empty(), "scrolling must not set host policy");

            let scrolled = last_rendered_buffer(&tui.terminal);
            assert!(buffer_text(scrolled).contains('↑'));
            let target = find_text(scrolled, preset);
            for kind in [
                MouseEventKind::Down(MouseButton::Left),
                MouseEventKind::Up(MouseButton::Left),
            ] {
                app.handle_tui_event(&mut tui, &mut server, mouse(kind, target.x, target.y))
                    .await?;
            }
            let request = format!(
                "@{} meta set autopilot cruise until=+4h",
                fake.endpoint().sid
            );
            assert!(
                fake.wait_for(|fake| fake.requests().contains(&request))
                    .await
            );
            assert!(!app.chat_widget.has_active_view());
            assert_eq!(app.chat_widget.capture_thread_input_state(), draft);

            app.chat_widget.apply_external_edit("/alab menu".into());
            app.chat_widget.handle_key_event(KeyCode::Enter.into());
            app.render_chat_widget_frame(&mut tui, size)?;
            find_text(last_rendered_buffer(&tui.terminal), "3. Off");
            app.handle_tui_event(
                &mut tui,
                &mut server,
                TuiEvent::Key(KeyCode::Char('3').into()),
            )
            .await?;
            let off = format!("@{} meta set autopilot off", fake.endpoint().sid);
            assert!(fake.wait_for(|fake| fake.requests().contains(&off)).await);
            assert_eq!(writes(), [request, off]);
            assert!(!app.chat_widget.has_active_view());
            tui.set_owned_screen(false)?;
            server.shutdown().await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn autopilot_picker_selects_only_a_stationary_painted_row() -> color_eyre::eyre::Result<()>
    {
        use crate::custom_terminal::test_support::last_rendered_buffer;
        use codex_alab_aterm::fake::FakeAterm;
        use codex_alab_aterm::fake::Mode;
        use codex_alab_tui::session::Autopilot;
        use codex_alab_tui::session::HarnessWord;
        use codex_alab_tui::session::Shared;
        use crossterm::event::MouseButton::Left;
        use crossterm::event::MouseButton::Right;
        use crossterm::event::MouseEventKind::Down;
        use crossterm::event::MouseEventKind::Drag;
        use crossterm::event::MouseEventKind::Up;

        for owned in [true, false] {
            let root = codex_alab_tui::test_paths::short_tempdir()?;
            let fake = FakeAterm::start(root.path(), Mode::Current).await?;
            let _interface = scope();
            interface::route_aterm_for_tests(fake.endpoint());
            let shared = Box::leak(Box::new(Shared::new()));
            let _automation = codex_alab_tui::boxes::scoped(shared);
            shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Land);
            let (mut app, _events, mut ops) = make_test_app_with_channels().await;
            attach_thread(&mut app, ThreadId::new());
            let mut server =
                Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
            let mut tui = crate::tui::test_support::make_test_tui()?;
            tui.set_owned_screen(owned)?;
            let size = tui.terminal.size()?;
            assert_eq!(size.width, 80);

            if owned {
                app.chat_widget
                    .apply_external_edit("unfinished prompt".into());
                app.chat_widget.handle_key_event(KeyCode::Left.into());
                app.render_chat_widget_frame(&mut tui, size)?;
                let buffer = last_rendered_buffer(&tui.terminal);
                let target = buffer
                    .area
                    .positions()
                    .find(|p| interface::autopilot_menu_hit(p.x, p.y))
                    .expect("painted composer mode control");
                for kind in [Down(Right), Up(Right)] {
                    app.handle_tui_event(&mut tui, &mut server, mouse(kind, target.x, target.y))
                        .await?;
                }
            } else {
                app.chat_widget.apply_external_edit("/alab menu".into());
                app.chat_widget.handle_key_event(KeyCode::Enter.into());
                app.chat_widget
                    .apply_external_edit("unfinished inline prompt".into());
            }
            assert!(
                app.chat_widget
                    .active_view_is(interface::AUTOPILOT_PICKER_ID)
            );
            assert!(
                app.chat_widget
                    .composer_text_with_pending()
                    .contains("unfinished")
            );
            let draft = app.chat_widget.capture_thread_input_state();
            app.render_chat_widget_frame(&mut tui, size)?;
            let buffer = last_rendered_buffer(&tui.terminal);
            let off = find_text(buffer, "Wait for your messages");
            let header = find_text(buffer, "Land. Timers switch to Land.");
            for (kind, position) in [
                (Up(Left), off),
                (Down(Left), header),
                (Up(Left), header),
                (Down(Left), off),
                (Drag(Left), header),
                (Drag(Left), off),
                (Up(Left), off),
            ] {
                app.handle_tui_event(&mut tui, &mut server, mouse(kind, position.x, position.y))
                    .await?;
                assert!(
                    app.chat_widget
                        .active_view_is(interface::AUTOPILOT_PICKER_ID)
                );
            }

            app.handle_tui_event(&mut tui, &mut server, mouse(Down(Left), off.x, off.y))
                .await?;
            shared.set_harness_word(HarnessWord::Unknown);
            app.chat_widget.pre_draw_tick();
            app.render_chat_widget_frame(&mut tui, size)?;
            app.handle_tui_event(&mut tui, &mut server, mouse(Up(Left), off.x, off.y))
                .await?;
            assert!(
                app.chat_widget
                    .active_view_is(interface::AUTOPILOT_PICKER_ID)
            );
            assert!(
                fake.requests()
                    .iter()
                    .all(|r| !r.contains(" meta set autopilot "))
            );

            for (mode, status, settings) in [
                (HarnessWord::Unknown, "no mode reported by aterm.", false),
                (HarnessWord::HarnessOff, "The harness is off.", true),
                (HarnessWord::AgentOff, "codex-alab Autopilot is off.", true),
                (
                    HarnessWord::Unhosted,
                    "This aterm has no control socket.",
                    true,
                ),
            ] {
                shared.set_harness_word(mode);
                app.chat_widget.pre_draw_tick();
                app.render_chat_widget_frame(&mut tui, size)?;
                let screen = crate::app::owned_transcript::tests::buffer_text(
                    last_rendered_buffer(&tui.terminal),
                );
                let subtitle = interface::AutopilotStatus::read().picker_subtitle();
                assert!(
                    screen.lines().any(|row| row.contains(&subtitle)),
                    "{screen}"
                );
                assert_eq!(screen.matches(status).count(), 1, "{screen}");
                assert_eq!(
                    screen.matches("Settings ▸ Harness").count(),
                    usize::from(settings),
                    "{screen}"
                );
                assert!(screen.contains("esc back"), "{screen}");
                assert!(!screen.contains("enter select"), "{screen}");
                for key in [KeyCode::Enter, KeyCode::Char('1')] {
                    app.handle_tui_event(&mut tui, &mut server, TuiEvent::Key(key.into()))
                        .await?;
                    assert!(
                        app.chat_widget
                            .active_view_is(interface::AUTOPILOT_PICKER_ID)
                    );
                }
                assert!(
                    fake.requests()
                        .iter()
                        .all(|r| !r.contains(" meta set autopilot "))
                );
            }

            shared.set_harness_word(HarnessWord::Land);
            app.chat_widget.pre_draw_tick();
            app.render_chat_widget_frame(&mut tui, size)?;
            let restored = crate::app::owned_transcript::tests::buffer_text(last_rendered_buffer(
                &tui.terminal,
            ));
            assert!(
                restored.contains("enter select") && restored.contains("Land (current)"),
                "{restored}"
            );
            let off = find_text(
                last_rendered_buffer(&tui.terminal),
                "Wait for your messages",
            );
            for kind in [Down(Left), Up(Left), Up(Left)] {
                app.handle_tui_event(&mut tui, &mut server, mouse(kind, off.x, off.y))
                    .await?;
            }
            assert!(
                !app.chat_widget.has_active_view(),
                "stationary click should close the picker (owned={owned})"
            );
            let request = format!("@{} meta set autopilot off", fake.endpoint().sid);
            assert!(
                fake.wait_for(|fake| fake.requests().contains(&request))
                    .await
            );
            let writes: Vec<_> = fake
                .requests()
                .into_iter()
                .filter(|r| r.contains(" meta set autopilot "))
                .collect();
            assert_eq!(writes, [request]);
            assert_eq!(app.chat_widget.capture_thread_input_state(), draft);
            assert!(
                !std::iter::from_fn(|| ops.try_recv().ok())
                    .any(|op| matches!(op, crate::app_command::AppCommand::UserTurn { .. }))
            );
            tui.set_owned_screen(false)?;
            server.shutdown().await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn stationary_footer_release_only_opens_the_account_pressed_on_mouse_down()
    -> color_eyre::eyre::Result<()> {
        let _scope = scope();
        let (mut app, _events, _operations) = make_test_app_with_channels().await;
        attach_thread(&mut app, ThreadId::new());
        let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        tui.set_owned_screen(true)?;
        let size = tui.terminal.size()?;
        let account = |email: &str| {
            Some(StatusAccountDisplay::ChatGpt {
                email: Some(email.to_string()),
                plan: None,
                workspace: None,
            })
        };
        let short = "a@b.test";
        let long = "long.account.name.for.the.status.row@example.test";
        app.chat_widget
            .update_account_state(account(short), None, true, true);
        app.render_owned_transcript(&mut tui, size)?;
        let target = find_text(
            crate::custom_terminal::test_support::last_rendered_buffer(&tui.terminal),
            "gpt-test",
        );
        for (column, before, after, replacement) in [
            (target.x, false, true, long),
            (target.x + 1, true, false, short),
        ] {
            assert_eq!(interface::account_mouse_hit(column, target.y), before);
            app.handle_tui_event(
                &mut tui,
                &mut server,
                mouse(MouseEventKind::Down(MouseButton::Left), column, target.y),
            )
            .await?;
            assert!(!app.chat_widget.has_active_view());
            app.chat_widget
                .update_account_state(account(replacement), None, true, true);
            app.render_owned_transcript(&mut tui, size)?;
            assert_eq!(interface::account_mouse_hit(column, target.y), after);
            app.handle_tui_event(
                &mut tui,
                &mut server,
                mouse(MouseEventKind::Up(MouseButton::Left), column, target.y),
            )
            .await?;
            assert!(
                !app.chat_widget.has_active_view(),
                "the footer changed under the press"
            );
        }

        app.render_owned_transcript(&mut tui, size)?;
        let account_start = find_text(
            crate::custom_terminal::test_support::last_rendered_buffer(&tui.terminal),
            short,
        );
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            app.handle_tui_event(
                &mut tui,
                &mut server,
                mouse(kind, account_start.x, account_start.y),
            )
            .await?;
        }
        assert!(
            crate::chatwidget::tests::render_bottom_popup(&app.chat_widget, 80)
                .contains("Account details")
        );
        tui.set_owned_screen(false)?;
        server.shutdown().await?;
        Ok(())
    }
}
