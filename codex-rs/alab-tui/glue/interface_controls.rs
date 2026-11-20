// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Live experience controls, compiled inside `chatwidget`. The switch preserves
//! the saved Autopilot preference, permissions, selected account and draft.

use super::ChatWidget;
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::history_cell;
use crate::legacy_core::config::edit::ConfigEdit;
use crate::legacy_core::config::edit::ConfigEditsBuilder;
use crate::tui::TuiEvent;
use codex_alab_tui::interface;
use codex_alab_tui::session::HarnessWord;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;

#[path = "retry_notice.rs"]
mod retry_notice;

#[path = "autopilot_menu.rs"]
mod autopilot_menu;

#[path = "autopilot_picker_click.rs"]
mod autopilot_picker_click;

#[derive(Default)]
pub(super) struct Controls {
    picker_click: autopilot_picker_click::PickerClick,
    menu: Option<autopilot_menu::AutopilotMenu>,
    press: Option<(MouseEvent, MouseAction)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MouseAction {
    SetEdition(bool),
    Build,
    Mode(HarnessWord),
    AutopilotMenu,
    AutopilotWords,
    Account,
}

fn send_autopilot_request(
    tx: &AppEventSender,
    request: impl std::future::Future<Output = Result<(), String>> + Send + 'static,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        if let Err(error) = request.await {
            tx.send(AppEvent::InsertHistoryCell(Box::new(
                history_cell::new_error_event(format!("Autopilot: {error}")),
            )));
        }
    });
}

impl ChatWidget {
    pub(crate) fn alab_adopt_tui_preferences(&mut self, tui: &codex_config::types::Tui) {
        self.local_settings.tui = tui.clone();
        self.bottom_pane
            .set_disable_paste_burst(tui.disable_paste_burst.unwrap_or(false));
        self.bottom_pane.set_motion_preferences(tui);
    }

    #[cfg(test)]
    pub(crate) fn alab_fixture_config(&mut self, config: crate::legacy_core::config::Config) {
        self.local_settings = crate::local_settings::LocalSettings::from(&config);
        self.config = config;
        if let Some(model) = self.config.model.clone() {
            self.set_model(&model);
        }
    }

    pub(super) fn alab_interface_command(&mut self, args: &str) {
        if !interface::available() {
            return;
        }
        let command = args.trim().to_ascii_lowercase();
        // A bare or second until= must still reach the usage message in either presentation.
        let until = cruise_until(&command);
        if !interface::enabled()
            && (until.is_some()
                || matches!(
                    command.as_str(),
                    "cruise" | "land" | "autopilot-off" | "menu" | "autopilot" | "tasks" | "stash"
                ))
        {
            self.add_info_message("Use /alab on to enable enhanced controls.".into(), None);
            return;
        }
        if let Some(when) = until {
            send_autopilot_request(
                &self.app_event_tx,
                interface::set_cruise_until(when.to_owned()),
            );
            return;
        }
        let enabled = match command.as_str() {
            "update" | "version" => {
                self.add_info_message(
                    codex_alab_tui::upgrade::update_report(
                        codex_build_info::BuildInfo::get().build_commit(),
                    ),
                    None,
                );
                return;
            }
            "account" => {
                if interface::enabled() {
                    self.alab_open_account_details();
                } else {
                    self.add_info_message(
                        "Use /alab on to enable the account details view.".into(),
                        None,
                    );
                }
                return;
            }
            "cruise" | "land" | "autopilot-off" => {
                let mode = match command.as_str() {
                    "cruise" => codex_alab_tui::session::HarnessWord::Cruise,
                    "land" => codex_alab_tui::session::HarnessWord::Land,
                    _ => codex_alab_tui::session::HarnessWord::Off,
                };
                self.alab_set_harness_mode(mode);
                return;
            }
            "menu" => {
                self.alab_open_autopilot_menu();
                return;
            }
            "autopilot" => {
                self.add_info_message(interface::autopilot_words(), None);
                return;
            }
            "tasks" => {
                self.app_event_tx.send(AppEvent::OpenAlabTasks);
                return;
            }
            "stash" => {
                self.bottom_pane.alab_stash_prompt();
                return;
            }
            "" => !interface::enabled(),
            "on" => true,
            "off" => false,
            "help" => {
                self.add_info_message(self.alab_interface_help(), None);
                return;
            }
            _ => {
                self.add_error_message(
                    "Usage: /alab [on|off|cruise [until=HH:MM|+DURATION]|land|autopilot-off|autopilot|menu|account|tasks|stash|update|version|help]".into(),
                );
                return;
            }
        };
        let requested = enabled;
        let enabled = interface::set_enabled(requested);
        if enabled != requested {
            self.add_error_message(
                "Could not durably retire the previous recovery source; the interface mode was not changed."
                    .into(),
            );
            self.refresh_status_surfaces();
            self.request_redraw();
            return;
        }
        if !enabled {
            self.alab_close_account_details();
            self.alab_allowance.reset();
            interface::set_status(Default::default());
        }
        self.refresh_status_surfaces();
        self.request_redraw();
        self.app_event_tx.send(AppEvent::AlabModeChanged);

        // Serialize writes and read the latest choice under the lock: rapid
        // clicks cannot leave an older choice as the next launch's preference.
        static SAVE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let path = self.local_settings.user_config_path.clone();
        let (latest, generation) = preference_generation(path.as_path());
        let tx = self.app_event_tx.clone();
        #[expect(
            clippy::await_holding_invalid_type,
            reason = "the guard is the serialization: each write waits for the one before it, so the newest choice is saved last"
        )]
        tokio::spawn(async move {
            let _save = SAVE.lock().await;
            if latest.load(std::sync::atomic::Ordering::SeqCst) != generation {
                return;
            }
            let result = ConfigEditsBuilder::for_config_path(path.as_path())
                .with_edits([ConfigEdit::SetPath {
                    segments: vec!["tui".into(), "alab_interface".into()],
                    value: toml_edit::value(enabled),
                }])
                .apply()
                .await;
            if let Err(error) = result {
                tx.send(AppEvent::InsertHistoryCell(Box::new(
                    history_cell::new_error_event(format!(
                        "Interface changed for this session; could not save its preference: {error}"
                    )),
                )));
            }
        });
    }

    fn alab_interface_help(&self) -> String {
        let mut lines = vec![
            "ALab controls: /alab on selects Enhanced; /alab off restores Vanilla.".to_string(),
        ];
        lines.push("/alab update (or /alab version) shows the running build, installed launch target and any pending update.".into());
        if interface::enabled() {
            lines
                .push("/alab cruise|land|autopilot-off selects the session Autopilot mode.".into());
            lines.push(
                "/alab cruise until=18:00 switches to Land at 18:00 local time. /alab cruise until=+1h30m switches to Land in 90 minutes."
                    .into(),
            );
            lines.push("/alab autopilot says the session's Autopilot mode in words.".into());
            lines.push("Type /alab menu to open the Autopilot picker, including when the terminal handles the mouse in scrollback.".into());
            lines.push(
                "With mouse capture (the full-screen default), click Cruise, Land or Off to select that mode. \
                 Click the compact mode glyph to cycle modes. If ▾ is visible, click it to open the picker. \
                 Right-click or Control-click the status or switch to open the same picker."
                    .into(),
            );
            let shortcut = self
                .bottom_pane
                .keymap()
                .primary_hint(crate::keymap::KeymapContext::Chat, "open_autopilot");
            lines.push(match shortcut {
                Some(key) => format!(
                    "{} opens Autopilot even while a turn is running.",
                    key.display_label()
                ),
                None => {
                    "To open Autopilot during a turn, bind Open Autopilot picker in /keymap.".into()
                }
            });
            lines.push("/alab account shows the selected email, subscription and quota details; click account or allowance text in the footer to open it.".into());
            lines.push("/tasks shows the checklist, local agents and background terminals.".into());
            lines.push("For checklists with the embedded server, set enabled = true under [tools.update_plan] in your Codex config.".into());
            match self.bottom_pane.alab_stash_shortcut_hint() {
                Some(hint) => lines.push(format!(
                    "{} parks a prompt outside search and popups; /stash restores it in an empty prompt.",
                    hint.display_label()
                )),
                None => lines.push("/stash restores a saved prompt in an empty prompt. Your configured keys take priority over the stash shortcut.".into()),
            }
        } else {
            lines.push("Use /alab on to enable account details, /tasks and prompt stash.".into());
        }
        lines.push("/model selects model and effort; /permissions sets access; /plan plans; /ps lists terminals; /diff shows changes; /resume restores sessions.".into());
        lines.push("/status shows context usage, token counts and session details.".into());
        lines.push("/usage opens analytics and reset actions; /skills, /hooks and /mcp manage tools; /keymap configures keyboard shortcuts.".into());
        if let Some(hint) = self.bottom_pane.alab_shortcuts_help_hint() {
            lines.push(format!(
                "With an empty prompt, press {} for keyboard shortcuts.",
                hint.display_label()
            ));
        }
        lines.join("\n")
    }

    fn alab_set_harness_mode(&self, mode: codex_alab_tui::session::HarnessWord) {
        send_autopilot_request(&self.app_event_tx, interface::set_harness_mode(mode));
    }

    fn alab_mouse_action(&self, event: MouseEvent) -> Option<MouseAction> {
        let left = event.kind == MouseEventKind::Down(MouseButton::Left);
        let secondary = event.kind == MouseEventKind::Down(MouseButton::Right)
            || (left && event.modifiers.contains(KeyModifiers::CONTROL));
        if !(left || secondary)
            || !interface::available()
            || !self.bottom_pane.no_modal_or_popup_active()
        {
            return None;
        }
        let (x, y) = (event.column, event.row);
        if secondary {
            interface::autopilot_menu_hit(x, y).then_some(MouseAction::AutopilotMenu)
        } else if interface::build_mouse_hit(x, y) {
            Some(MouseAction::Build)
        } else if let Some(mode) = interface::mode_mouse_hit(x, y) {
            Some(MouseAction::Mode(mode))
        } else if interface::autopilot_words_hit(x, y) {
            Some(MouseAction::AutopilotWords)
        } else if interface::mouse_hit(x, y) {
            Some(MouseAction::SetEdition(!interface::enabled()))
        } else {
            interface::account_mouse_hit(x, y).then_some(MouseAction::Account)
        }
    }

    pub(crate) fn alab_picker_click_pending(&self) -> bool {
        self.alab_controls.picker_click.pending()
    }

    pub(crate) fn alab_inherit_picker_release(&mut self, previous: &mut Self) {
        self.alab_controls.picker_click = previous.alab_controls.picker_click.take_held_release();
    }

    pub(crate) fn alab_consume_picker_followup(
        &mut self,
        event: &TuiEvent,
        shortcuts_open: bool,
    ) -> bool {
        let owns_view = if self.alab_controls.picker_click.expects_picker() {
            self.active_view_is(interface::AUTOPILOT_PICKER_ID)
        } else {
            self.no_modal_or_popup_active()
        };
        if shortcuts_open || !owns_view || self.is_external_writer_view() {
            self.alab_controls.picker_click.invalidate();
        }
        self.alab_controls
            .picker_click
            .consume(event, std::time::Instant::now())
    }

    /// Arm the disclosure only after the composer and higher input owners accept the press.
    pub(crate) fn alab_press_disclosure(&mut self, mouse: MouseEvent) -> bool {
        if mouse.kind != MouseEventKind::Down(MouseButton::Left)
            || !mouse.modifiers.is_empty()
            || !interface::autopilot_disclosure_hit(mouse.column, mouse.row)
            || !self.no_modal_or_popup_active()
            || self.is_external_writer_view()
            || !self.prepare_composer_mouse(mouse)
        {
            return false;
        }
        self.alab_controls
            .picker_click
            .press_disclosure((mouse.column, mouse.row).into(), std::time::Instant::now());
        true
    }

    pub(crate) fn alab_disclosure_release(&self, mouse: MouseEvent) -> bool {
        mouse.kind == MouseEventKind::Up(MouseButton::Left)
            && mouse.modifiers.is_empty()
            && self
                .alab_controls
                .picker_click
                .disclosure_pressed((mouse.column, mouse.row).into())
    }

    pub(crate) fn alab_finish_disclosure(&mut self, mouse: MouseEvent) {
        if self.no_modal_or_popup_active()
            && !self.is_external_writer_view()
            && interface::autopilot_disclosure_hit(mouse.column, mouse.row)
            && interface::autopilot_selection_available()
            && self.prepare_composer_mouse(mouse)
        {
            self.alab_controls.picker_click.disclosure_opened();
            self.alab_open_autopilot_menu();
        } else {
            self.alab_controls.picker_click.invalidate();
        }
    }

    pub(crate) fn alab_cancel_picker_mouse(&mut self) -> bool {
        self.alab_controls.picker_click.invalidate();
        self.bottom_pane.alab_cancel_picker_mouse()
    }

    pub(crate) fn alab_picker_mouse_covered_by_hud(&mut self, event: MouseEvent) {
        if self.active_view_is(interface::AUTOPILOT_PICKER_ID) {
            if event.kind == MouseEventKind::Moved {
                self.bottom_pane.alab_reset_picker_mouse(true);
            } else if !self.alab_cancel_picker_mouse() {
                // Repaint the retired hit geometry even when there was no visible hover.
                self.bottom_pane.request_redraw();
            }
        }
    }

    pub(crate) fn alab_picker_mouse(&mut self, event: MouseEvent) -> bool {
        if !self.bottom_pane.alab_picker_mouse(event) {
            return false;
        }
        let closed = self.bottom_pane.no_modal_or_popup_active();
        self.alab_controls
            .picker_click
            .observe(event, closed, std::time::Instant::now());
        if closed {
            self.on_modal_or_popup_closed();
        }
        true
    }

    pub(crate) fn alab_capture_composer_press(&mut self, event: &TuiEvent) {
        if let TuiEvent::Mouse(mouse) = event
            && matches!(mouse.kind, MouseEventKind::Down(_))
        {
            self.alab_controls.press = self
                .alab_mouse_action(*mouse)
                .map(|action| (*mouse, action));
        }
    }

    pub(super) fn alab_interface_mouse(&mut self, event: MouseEvent) -> bool {
        let Some((press, action)) = self.alab_controls.press else {
            return false;
        };
        if press != event {
            return false;
        }
        self.alab_controls.press = None;
        // A moved control still owns its original press, including a right-click over the draft.
        if self.alab_mouse_action(event) != Some(action) {
            return true;
        }
        match action {
            MouseAction::SetEdition(on) => {
                self.alab_interface_command(if on { "on" } else { "off" })
            }
            MouseAction::Build => self.alab_interface_command("update"),
            MouseAction::Mode(mode) => self.alab_set_harness_mode(mode),
            MouseAction::AutopilotMenu => self.alab_open_autopilot_menu(),
            MouseAction::AutopilotWords => {
                self.add_info_message(interface::autopilot_words(), None)
            }
            MouseAction::Account => self.alab_open_account_details(),
        }
        true
    }
}

/// `cruise until=<when>`'s `<when>`: one `until=` token whose value is visible ASCII, the
/// control line's own alphabet; `None` for any other form.
fn cruise_until(words: &str) -> Option<&str> {
    let mut words = words.split_ascii_whitespace();
    if words.next() != Some("cruise") {
        return None;
    }
    let when = words.next()?.strip_prefix("until=")?;
    (words.next().is_none() && !when.is_empty() && when.bytes().all(|b| b.is_ascii_graphic()))
        .then_some(when)
}

// Different local profiles must not cancel each other's saved choices. Weak counters
// expire once all writes for a path settle; the next change removes unused entries.
fn preference_generation(
    path: &std::path::Path,
) -> (std::sync::Arc<std::sync::atomic::AtomicU64>, u64) {
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::OnceLock;
    use std::sync::Weak;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;
    static PATHS: OnceLock<Mutex<std::collections::BTreeMap<std::path::PathBuf, Weak<AtomicU64>>>> =
        OnceLock::new();
    let mut paths = PATHS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    paths.retain(|_, generation| generation.strong_count() > 0);
    let latest = paths.get(path).and_then(Weak::upgrade).unwrap_or_else(|| {
        let latest = Arc::new(AtomicU64::new(0));
        paths.insert(path.to_owned(), Arc::downgrade(&latest));
        latest
    });
    let generation = latest.fetch_add(1, Ordering::SeqCst) + 1;
    (latest, generation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
    use codex_alab_tui::banner::Depth;
    use codex_alab_tui::banner::Theme;
    use codex_config::types::KeybindingSpec;
    use codex_config::types::KeybindingsSpec;
    use codex_config::types::TuiKeymap;
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn press_control(chat: &mut ChatWidget, event: MouseEvent) -> bool {
        chat.alab_capture_composer_press(&TuiEvent::Mouse(event));
        chat.alab_interface_mouse(event)
    }

    #[tokio::test]
    async fn shortcut_reference_shows_the_current_autopilot_binding_in_enhanced() {
        use crate::chatwidget::tests::render_bottom_popup;

        let _scope = interface::scoped(Box::leak(Box::new(interface::State::new())));
        interface::configure(true);
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        chat.handle_key_event(KeyCode::Char('?').into());
        assert!(chat.shortcut_overlay_visible());
        let reference = |chat: &ChatWidget| {
            let text = render_bottom_popup(chat, 116);
            assert!(text.contains("Keyboard shortcuts"), "{text}");
            text.lines()
                .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert!(!reference(&chat).contains("Open Autopilot"));
        let chord = format!(
            "{} 3",
            crate::key_hint::ctrl(KeyCode::Char('x')).display_label()
        );
        for (binding, label) in [("f19", "f19"), ("ctrl-x 3", chord.as_str())] {
            let mut config = TuiKeymap::default();
            config.chat.open_autopilot = Some(KeybindingsSpec::One(KeybindingSpec(binding.into())));
            let runtime = crate::keymap::RuntimeKeymap::from_config(&config).unwrap();
            chat.apply_keymap_update(config, &runtime);
            let shown = reference(&chat);
            assert!(
                shown.contains(&format!("{label} Open Autopilot")),
                "{shown}"
            );
        }
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 12))
            .expect("test terminal");
        terminal
            .draw(|frame| {
                crate::render::renderable::Renderable::render(
                    &chat,
                    frame.area(),
                    frame.buffer_mut(),
                );
            })
            .expect("draw clipped reference");
        let clipped = terminal.backend().to_string();
        for hint in ["resize to see all", "/keymap customize", "esc close"] {
            assert!(clipped.contains(hint), "{clipped}");
        }
        interface::set_enabled(false);
        assert!(!reference(&chat).contains("Open Autopilot"));
        interface::set_enabled(true);
        assert!(reference(&chat).contains(&format!("{chord} Open Autopilot")));
        let config = TuiKeymap::default();
        let runtime = crate::keymap::RuntimeKeymap::from_config(&config).unwrap();
        chat.apply_keymap_update(config, &runtime);
        assert!(!reference(&chat).contains("Open Autopilot"));
    }

    #[tokio::test]
    async fn help_tracks_remapped_chorded_disabled_and_off_shortcuts() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::configure(true);
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        let original = chat.alab_interface_help();
        assert!(original.contains("/alab account"));
        assert!(original.contains("/alab cruise until=18:00 switches to Land at 18:00 local time"));
        assert!(original.contains("/alab cruise until=+1h30m switches to Land in 90 minutes"));
        assert!(original.contains("With an empty prompt, press ?"));
        assert!(original.contains("parks a prompt outside search"));
        assert!(original.contains("bind Open Autopilot picker in /keymap"));

        let mut config = TuiKeymap::default();
        config.composer.history_search_next = Some(KeybindingsSpec::Many(Vec::new()));
        config.chat.open_autopilot = Some(KeybindingsSpec::One(KeybindingSpec("ctrl-s 3".into())));
        for (binding, expected) in [
            (
                KeybindingsSpec::One(KeybindingSpec("f20".into())),
                Some("f20"),
            ),
            (
                KeybindingsSpec::One(KeybindingSpec("f20 f21".into())),
                Some("f20 f21"),
            ),
            (KeybindingsSpec::Many(Vec::new()), None),
        ] {
            config.composer.toggle_shortcuts = Some(binding);
            let runtime = crate::keymap::RuntimeKeymap::from_config(&config).unwrap();
            chat.bottom_pane.set_keymap_bindings(&runtime);
            let help = chat.alab_interface_help();
            let prefix = crate::key_hint::ctrl(KeyCode::Char('s')).display_label();
            assert!(help.contains(&format!(
                "{prefix} 3 opens Autopilot even while a turn is running."
            )));
            assert!(!help.contains("press ?"), "{help}");
            assert!(!help.contains("parks a prompt"), "{help}");
            assert!(help.contains("Your configured keys take priority"));
            if let Some(shortcut) = expected {
                assert!(help.contains(&format!(
                    "With an empty prompt, press {shortcut} for keyboard shortcuts."
                )));
            } else {
                assert!(!help.contains("With an empty prompt, press"));
                assert!(help.contains("/keymap configures keyboard shortcuts"));
            }
        }

        interface::set_enabled(false);
        let off = chat.alab_interface_help();
        assert!(off.contains("Use /alab on to enable"));
        assert!(!off.contains("click account"));
        assert!(!off.contains("parks a prompt"));
        for hint in [
            "Type /alab menu to open the Autopilot picker",
            "the terminal handles the mouse in scrollback",
            "mouse capture (the full-screen default)",
            "click Cruise, Land or Off to select",
            "Click the compact mode glyph to cycle modes",
            "If ▾ is visible, click it to open the picker",
            "Right-click or Control-click the status or switch",
        ] {
            assert!(original.contains(hint), "{hint}");
            assert!(!off.contains(hint), "{hint}");
        }
    }

    #[tokio::test]
    async fn bare_alab_matches_its_native_slash_popup_hint_in_both_editions() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::configure(true);
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        chat.bottom_pane.set_disable_paste_burst(true);
        let command = crate::slash_command::SlashCommand::Alab;
        for (was_on, hint) in [
            (
                true,
                "turn ALab off · /alab version · /alab menu for Autopilot",
            ),
            (false, "turn ALab on · /alab version"),
        ] {
            for ch in "/alab".chars() {
                chat.handle_key_event(KeyCode::Char(ch).into());
            }
            assert_eq!(chat.bottom_pane.composer_text(), "/alab");
            assert_eq!(command.description(), hint);
            chat.handle_key_event(KeyCode::Enter.into());
            assert_ne!(interface::enabled(), was_on);
            assert!(chat.bottom_pane.composer_text().is_empty());
        }
    }

    #[tokio::test]
    async fn toggles_preserve_the_draft_and_invalid_arguments_preserve_the_choice() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::configure(true);
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        chat.bottom_pane
            .set_composer_text("keep this draft".into(), Vec::new(), Vec::new());
        chat.alab_interface_command("off");
        assert!(!interface::enabled());
        assert_eq!(chat.bottom_pane.composer_text(), "keep this draft");
        chat.alab_interface_command("invalid");
        assert!(!interface::enabled());
        chat.alab_interface_command("on");
        assert!(interface::enabled());
        assert_eq!(chat.bottom_pane.composer_text(), "keep this draft");
    }

    #[tokio::test]
    async fn secondary_clicks_leave_edition_and_build_actions_to_primary_presses() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::configure(true);
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        let area = Rect::new(2, 3, 80, 4);

        for enhanced in [true, false] {
            interface::set_enabled(enhanced);
            interface::render(
                area,
                &mut Buffer::empty(area),
                Depth::Mono,
                Theme::default(),
            );
            let edition = interface::button_rect(area).expect("edition switch").x;
            let mut targets = vec![(edition, MouseAction::SetEdition(!enhanced))];
            if enhanced {
                let build = (area.x..area.right())
                    .find(|&x| interface::build_mouse_hit(x, area.y))
                    .expect("build label");
                targets.push((build, MouseAction::Build));
            }
            for (column, primary) in targets {
                let click = MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column,
                    row: area.y,
                    modifiers: KeyModifiers::NONE,
                };
                assert!(chat.alab_mouse_action(click) == Some(primary));
                for secondary in [
                    MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Right),
                        ..click
                    },
                    MouseEvent {
                        modifiers: KeyModifiers::CONTROL,
                        ..click
                    },
                ] {
                    assert!(!press_control(&mut chat, secondary));
                    assert_eq!(interface::enabled(), enhanced);
                    assert_eq!(chat.bottom_pane.active_view_id(), None);
                }
            }
        }
    }

    #[tokio::test]
    async fn off_button_click_restores_enhancements_and_preserves_the_draft() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::configure(false);
        let (mut chat, _tx, mut rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        let config_path = chat.local_settings.user_config_path.clone();
        chat.bottom_pane.set_composer_text(
            "keep my unsubmitted draft".into(),
            Vec::new(),
            Vec::new(),
        );
        let area = Rect::new(2, 8, 100, 3);
        let mut buf = Buffer::empty(area);
        interface::render(area, &mut buf, Depth::Mono, Theme::default());
        let button = interface::button_rect(area).expect("off button visible");
        let mut click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: button.x,
            row: button.y,
            modifiers: KeyModifiers::NONE,
        };
        assert!(!press_control(&mut chat, click));
        assert!(!interface::enabled());
        click.kind = MouseEventKind::Down(MouseButton::Left);
        assert!(press_control(&mut chat, click));
        assert!(interface::enabled());
        let saved = saved_interface_choice(config_path.as_path(), true).await;
        let restored = Box::leak(Box::new(interface::State::new()));
        {
            let _restored_scope = interface::scoped(restored);
            interface::configure(saved["tui"]["alab_interface"].as_bool().unwrap());
            assert!(
                interface::enabled(),
                "saved on choice survives a new interface instance"
            );
        }
        assert_eq!(
            chat.bottom_pane.composer_text(),
            "keep my unsubmitted draft"
        );
        assert!(
            std::iter::from_fn(|| rx.try_recv().ok())
                .any(|event| matches!(event, AppEvent::AlabModeChanged))
        );
        assert!(
            !press_control(&mut chat, click),
            "old switch target invalid after change"
        );
        // At the narrowest interactive width both states share their right edge.
        let area = Rect { width: 8, ..area };
        let mut buf = Buffer::empty(area);
        interface::render(area, &mut buf, Depth::Mono, Theme::default());
        click.column = area.right() - 3;
        assert_eq!(buf[(click.column, click.row)].symbol(), "]");
        assert!(press_control(&mut chat, click));
        assert!(!interface::enabled());
        saved_interface_choice(config_path.as_path(), false).await;

        let mut buf = Buffer::empty(area);
        interface::render(area, &mut buf, Depth::Mono, Theme::default());
        let row: String = (area.x..area.right())
            .map(|x| buf[(x, area.y)].symbol())
            .collect();
        assert_eq!(row, " [off]  ");
        // Click the extra cell needed for the recovery label.
        click.column = area.x + 1;
        assert!(press_control(&mut chat, click));
        assert!(interface::enabled());
        saved_interface_choice(config_path.as_path(), true).await;
        assert_eq!(
            chat.bottom_pane.composer_text(),
            "keep my unsubmitted draft"
        );
    }

    async fn saved_interface_choice(path: &std::path::Path, expected: bool) -> toml::Value {
        tokio::time::timeout(std::time::Duration::from_secs(60), async {
            loop {
                if let Ok(text) = tokio::fs::read_to_string(path).await
                    && let Ok(value) = toml::from_str::<toml::Value>(&text)
                    && value
                        .get("tui")
                        .and_then(|tui| tui.get("alab_interface"))
                        .and_then(toml::Value::as_bool)
                        == Some(expected)
                {
                    return value;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("interface preference write settled")
    }

    #[tokio::test]
    async fn account_command_closes_when_off_and_never_enables_the_interface() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::configure(true);
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        chat.bottom_pane.set_composer_text(
            "unsubmitted account draft".into(),
            Vec::new(),
            Vec::new(),
        );

        chat.alab_interface_command("account");
        assert_eq!(chat.bottom_pane.active_view_id(), Some("alab-account"));
        chat.alab_interface_command("off");
        assert_eq!(chat.bottom_pane.active_view_id(), None);
        assert!(!interface::enabled());
        chat.alab_interface_command("account");
        assert_eq!(chat.bottom_pane.active_view_id(), None);
        assert!(!interface::enabled());
        assert_eq!(
            chat.bottom_pane.composer_text(),
            "unsubmitted account draft"
        );
    }

    #[tokio::test]
    async fn account_footer_clicks_respect_button_modal_and_draft_ownership() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::configure(true);
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        chat.bottom_pane.set_composer_text(
            "keep this while inspecting".into(),
            Vec::new(),
            Vec::new(),
        );
        interface::set_status(interface::InterfaceStatus {
            account: "selected@example.test".into(),
            weekly: Some("weekly 82% left".into()),
            ..Default::default()
        });
        let area = Rect::new(2, 20, 100, 1);
        let mut buf = Buffer::empty(area);
        interface::render_status_current(area, &mut buf, Depth::Mono, Theme::default());
        let mut event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: 2,
            row: 20,
            modifiers: KeyModifiers::NONE,
        };
        assert!(!press_control(&mut chat, event));
        event.kind = MouseEventKind::Down(MouseButton::Left);
        event.modifiers = KeyModifiers::CONTROL;
        assert!(!press_control(&mut chat, event));
        assert_eq!(chat.bottom_pane.active_view_id(), None);
        event.modifiers = KeyModifiers::NONE;
        event.row = 19;
        assert!(!press_control(&mut chat, event));
        event.row = 20;
        assert!(press_control(&mut chat, event));
        assert_eq!(chat.bottom_pane.active_view_id(), Some("alab-account"));
        assert!(!press_control(&mut chat, event));
        chat.bottom_pane
            .handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(chat.bottom_pane.active_view_id(), None);
        assert_eq!(
            chat.bottom_pane.composer_text(),
            "keep this while inspecting"
        );
        interface::set_enabled(false);
        assert!(!press_control(&mut chat, event));
    }

    /// The text of each history cell sent so far.
    fn said(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> Vec<String> {
        let mut said = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let AppEvent::InsertHistoryCell(cell) = event {
                said.push(cell_text(cell.as_ref()));
            }
        }
        said
    }

    fn cell_text(cell: &dyn history_cell::HistoryCell) -> String {
        cell.display_lines(/*width*/ 1000)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// An enhanced interface whose requests go to `fake` (never a real aterm), over a session
    /// wearing the Land word, and that session's facts.
    fn routed(
        fake: &codex_alab_aterm::fake::FakeAterm,
    ) -> (
        interface::Scope,
        codex_alab_tui::boxes::Scope,
        &'static codex_alab_tui::session::Shared,
    ) {
        use codex_alab_tui::session::Autopilot;
        use codex_alab_tui::session::HarnessWord;
        let state = Box::leak(Box::new(interface::State::new()));
        let scope = interface::scoped(state);
        let shared = Box::leak(Box::new(codex_alab_tui::session::Shared::new()));
        let automation = codex_alab_tui::boxes::scoped(shared);
        interface::configure(true);
        interface::route_aterm_for_tests(fake.endpoint());
        shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Land);
        (scope, automation, shared)
    }

    #[tokio::test]
    async fn refused_switch_and_unconfirmed_timer_name_the_selection_in_history() {
        use codex_alab_aterm::fake::FakeAterm;
        use codex_alab_aterm::fake::Mode;
        use codex_alab_tui::session::HarnessWord;

        let dir = tempfile::tempdir().expect("scratch");
        let fake = FakeAterm::start(dir.path(), Mode::Current)
            .await
            .expect("fake");
        let (_scope, _automation, shared) = routed(&fake);
        let (mut chat, _tx, mut rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        said(&mut rx);
        let area = Rect::new(0, 0, 100, 3);
        interface::render(
            area,
            &mut Buffer::empty(area),
            Depth::Mono,
            Theme::default(),
        );
        let off = (0..area.width)
            .find(|x| interface::mode_mouse_hit(*x, 0) == Some(HarnessWord::Off))
            .expect("Off segment");

        for (attempt, expected) in ["refused off", "did not confirm cruise until=+1h"]
            .into_iter()
            .enumerate()
        {
            if attempt == 0 {
                let command = format!("@{} meta set autopilot off", fake.endpoint().sid);
                fake.reply_once(&command, "ERR mode locked for this session\n");
                assert!(press_control(
                    &mut chat,
                    MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        column: off,
                        row: 0,
                        modifiers: KeyModifiers::NONE,
                    }
                ));
            } else {
                fake.drop_next_reply("meta set autopilot cruise until=+1h");
                chat.alab_interface_command("menu");
                chat.bottom_pane.handle_key_event(KeyCode::Char('4').into());
            }
            let message = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    if let AppEvent::InsertHistoryCell(cell) = rx.recv().await.expect("app event") {
                        break cell_text(cell.as_ref());
                    }
                }
            })
            .await
            .expect("Autopilot feedback");
            assert!(message.contains(expected), "{message}");
            assert_eq!(shared.harness_word(), HarnessWord::Land);
        }
    }

    #[tokio::test]
    async fn disabled_autopilot_commands_explain_how_to_enable_them() {
        use codex_alab_aterm::fake::FakeAterm;
        use codex_alab_aterm::fake::Mode;
        let dir = tempfile::tempdir().expect("scratch");
        let fake = FakeAterm::start(dir.path(), Mode::Current)
            .await
            .expect("fake");
        let _scopes = routed(&fake);
        let (mut chat, _tx, mut rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        interface::set_enabled(false);
        said(&mut rx);

        let hint = "Use /alab on to enable enhanced controls.";
        for (command, expected) in [
            ("cruise", hint),
            ("land", hint),
            ("autopilot-off", hint),
            ("cruise until=+2h", hint),
            ("cruise until=", "Usage: /alab"),
            ("cruise until=+2h until=+4h", "Usage: /alab"),
        ] {
            chat.alab_interface_command(command);
            let messages = said(&mut rx);
            assert!(
                matches!(messages.as_slice(), [message] if message.contains(expected)),
                "{command}: {messages:?}"
            );
        }
        assert!(!interface::enabled());
        tokio::task::yield_now().await;
        assert!(
            fake.requests()
                .iter()
                .all(|request| !request.contains("meta set autopilot"))
        );
    }

    /// `/alab cruise until=<when>` REACHES ATERM AS ITS TIMED CRUISE, and a bare, doubled or
    /// unprintable `until=` gets the usage line. NEGATIVE CONTROL: plain `/alab cruise` sends
    /// plain cruise.
    #[tokio::test]
    async fn cruise_until_reaches_aterm_and_a_bare_until_is_refused() {
        use codex_alab_aterm::fake::FakeAterm;
        use codex_alab_aterm::fake::Mode;
        let dir = tempfile::tempdir().expect("scratch dir");
        let fake = FakeAterm::start(dir.path(), Mode::Current)
            .await
            .expect("fake aterm");
        let _scopes = routed(&fake);
        let (mut chat, _tx, mut rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        let sid = fake.endpoint().sid;
        said(&mut rx);
        chat.alab_interface_command("cruise until=18:00");
        assert_eq!(said(&mut rx), Vec::<String>::new());
        let timed = format!("@{sid} meta set autopilot cruise until=18:00");
        assert!(fake.wait_for(|fake| fake.requests().contains(&timed)).await);
        for refused in [
            "cruise until=",
            "cruise until=18:00 until=19:00",
            "cruise until=18:00 land",
            "cruise until=18\u{7}00",
        ] {
            chat.alab_interface_command(refused);
            let said = said(&mut rx);
            assert!(
                said.iter()
                    .any(|said| said
                        .contains("Usage: /alab [on|off|cruise [until=HH:MM|+DURATION]|")),
                "{refused:?}: {said:?}"
            );
        }
        // NEGATIVE CONTROL.
        chat.alab_interface_command("cruise");
        let plain = format!("@{sid} meta set autopilot cruise");
        assert!(fake.wait_for(|fake| fake.requests().contains(&plain)).await);
        let writes: Vec<_> = fake
            .requests()
            .into_iter()
            .filter(|request| request.contains("meta set autopilot"))
            .collect();
        assert_eq!(writes, [timed, plain]);
    }

    #[tokio::test]
    async fn secondary_clicks_on_switch_borders_open_the_picker_without_selecting_a_mode() {
        use crate::render::renderable::Renderable;
        use codex_alab_aterm::fake::FakeAterm;
        use codex_alab_aterm::fake::Mode;

        let dir = codex_alab_tui::test_paths::short_tempdir().expect("private socket root");
        let fake = FakeAterm::start(dir.path(), Mode::Current)
            .await
            .expect("fake");
        let (_scope, _automation, shared) = routed(&fake);
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        let before = fake.requests();
        for width in [56, 100] {
            let area = Rect::new(0, 0, width, chat.desired_height(width));
            let mut buf = Buffer::empty(area);
            chat.render(area, &mut buf);
            let left = area
                .positions()
                .find(|p| buf[*p].symbol() == "▏")
                .expect("left switch edge");
            let right = (left.x..area.right())
                .find(|&x| buf[(x, left.y)].symbol() == "▕")
                .expect("right switch edge");
            let borders: Vec<_> = (left.x..=right)
                .filter(|&x| matches!(buf[(x, left.y)].symbol(), "▏" | "│" | "[" | "]" | "▕"))
                .collect();
            assert_eq!(borders.len(), 4);
            for column in borders {
                let left_press = MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column,
                    row: left.y,
                    modifiers: KeyModifiers::NONE,
                };
                assert!(!press_control(&mut chat, left_press));
                for event in [
                    MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Right),
                        ..left_press
                    },
                    MouseEvent {
                        modifiers: KeyModifiers::CONTROL,
                        ..left_press
                    },
                ] {
                    assert!(press_control(&mut chat, event));
                    assert_eq!(
                        chat.bottom_pane
                            .selected_index_for_present_view("alab-autopilot"),
                        Some(1)
                    );
                    chat.bottom_pane
                        .handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
                    assert_eq!(chat.bottom_pane.active_view_id(), None);
                    chat.render(area, &mut buf);
                }
            }
        }
        assert_eq!(
            shared.harness_word(),
            codex_alab_tui::session::HarnessWord::Land
        );
        assert_eq!(fake.requests(), before);
    }

    #[tokio::test]
    async fn narrow_autopilot_status_leads_to_the_native_picker_without_a_host_request() {
        use crate::chatwidget::tests::render_bottom_popup;
        use crate::render::renderable::Renderable;
        use codex_alab_aterm::fake::FakeAterm;
        use codex_alab_aterm::fake::Mode;
        let dir = codex_alab_tui::test_paths::short_tempdir().expect("private socket root");
        let fake = FakeAterm::start(dir.path(), Mode::Current)
            .await
            .expect("fake");
        let _scopes = routed(&fake);
        let (mut chat, _tx, mut rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        chat.bottom_pane.set_disable_paste_burst(true);
        let width = 10;
        assert!(render_bottom_popup(&chat, width).contains("[on]"));
        let (column, row) = (0..chat.desired_height(width))
            .find_map(|y| {
                (0..width)
                    .find(|&x| interface::autopilot_menu_hit(x, y))
                    .map(|x| (x, y))
            })
            .expect("the compact mode glyph opens the picker");
        let before = fake.requests();
        assert!(press_control(
            &mut chat,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Right),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
        ));
        assert_eq!(
            chat.bottom_pane
                .selected_index_for_present_view("alab-autopilot"),
            Some(1)
        );
        chat.bottom_pane
            .handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        let width = 9; // The edition switch fits, while the mode is keyboard-only.
        assert!(render_bottom_popup(&chat, width).contains("[on]"));
        for y in 0..chat.desired_height(width) {
            assert!((0..width).all(|x| !interface::autopilot_menu_hit(x, y)));
        }
        let submit = |chat: &mut ChatWidget, command: &str| {
            for ch in command.chars() {
                chat.handle_key_event(KeyCode::Char(ch).into());
            }
            chat.handle_key_event(KeyCode::Enter.into());
        };
        submit(&mut chat, "/alab autopilot");
        assert!(
            said(&mut rx)
                .iter()
                .any(|text| text.contains("Autopilot: land.")
                    && text.contains("Choose a mode with /alab menu."))
        );
        submit(&mut chat, "/alab menu");
        assert_eq!(
            fake.requests(),
            before,
            "opening a native picker requires no aterm request"
        );
        assert_eq!(
            chat.bottom_pane
                .selected_index_for_present_view("alab-autopilot"),
            Some(1)
        );
        chat.bottom_pane
            .handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        chat.bottom_pane
            .handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let off = format!("@{} meta set autopilot off", fake.endpoint().sid);
        assert!(fake.wait_for(|fake| fake.requests().contains(&off)).await);
        assert!(
            fake.requests()
                .iter()
                .all(|request| !request.contains(" open "))
        );
    }
    #[tokio::test]
    async fn an_expired_timer_has_no_cancel_shortcut_that_can_restart_cruise() {
        use codex_alab_aterm::fake::FakeAterm;
        use codex_alab_aterm::fake::Mode;
        use codex_alab_tui::session::Autopilot;
        use codex_alab_tui::session::HarnessWord;
        let dir = tempfile::tempdir().expect("scratch");
        let fake = FakeAterm::start(dir.path(), Mode::Current)
            .await
            .expect("fake");
        let (_scope, _automation, shared) = routed(&fake);
        shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Cruise);
        shared.set_land_at(Some(2_000_000_000));
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        let before = fake.requests();
        chat.alab_interface_command("menu");
        shared.set_land_at(None);
        shared.set_autopilot_with_word(Autopilot::Active, HarnessWord::Land);
        // The former "Cancel scheduled landing" row sent plain Cruise even after landing.
        chat.bottom_pane.handle_key_event(KeyCode::Char('8').into());
        assert_eq!(chat.bottom_pane.active_view_id(), Some("alab-autopilot"));
        assert_eq!(fake.requests(), before);
        // Choosing the Cruise mode itself remains an explicit request.
        chat.bottom_pane.handle_key_event(KeyCode::Char('1').into());
        let cruise = format!("@{} meta set autopilot cruise", fake.endpoint().sid);
        assert!(
            fake.wait_for(|fake| fake.requests().contains(&cruise))
                .await
        );
        assert!(!chat.bottom_pane.has_active_view());
    }

    #[tokio::test]
    async fn open_autopilot_picker_tracks_reports_and_preserves_keyboard_focus() {
        use crate::chatwidget::tests::render_bottom_popup;
        use codex_alab_aterm::fake::FakeAterm;
        use codex_alab_aterm::fake::Mode;
        use codex_alab_tui::session::HarnessWord;

        let dir = tempfile::tempdir().expect("scratch");
        let fake = FakeAterm::start(dir.path(), Mode::Current)
            .await
            .expect("fake");
        let (_interface, _automation, shared) = routed(&fake);
        let (mut chat, _tx, mut rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        let selected = |chat: &ChatWidget| {
            chat.bottom_pane
                .selected_index_for_present_view("alab-autopilot")
        };
        let press =
            |chat: &mut ChatWidget, key| chat.bottom_pane.handle_key_event(KeyEvent::from(key));

        shared.set_harness_word(HarnessWord::Unknown);
        chat.alab_interface_command("menu");
        press(&mut chat, KeyCode::Enter);
        assert!(render_bottom_popup(&chat, 48).contains("no mode reported by aterm"));
        shared.set_harness_word(HarnessWord::Off);
        chat.pre_draw_tick();
        assert_eq!(selected(&chat), Some(2));
        let off = render_bottom_popup(&chat, 48);
        assert!(
            off.contains("Off. Timers switch to Land.") && off.contains("Off (current)"),
            "{off}"
        );

        // Before navigation the cursor follows the reported mode. Navigating back to the
        // current row still counts as a deliberate choice when the report next changes.
        shared.set_harness_word(HarnessWord::Land);
        chat.pre_draw_tick();
        assert_eq!(selected(&chat), Some(1));
        press(&mut chat, KeyCode::Up);
        press(&mut chat, KeyCode::Down);
        shared.set_harness_word(HarnessWord::Off);
        chat.pre_draw_tick();
        assert_eq!(selected(&chat), Some(1));

        press(&mut chat, KeyCode::Esc);
        chat.alab_interface_command("menu");
        press(&mut chat, KeyCode::Char('7'));
        assert!(
            std::iter::from_fn(|| rx.try_recv().ok())
                .any(|event| matches!(event, AppEvent::OpenAlabAutopilotTimer))
        );
        chat.alab_open_autopilot_timer();
        shared.set_harness_word(HarnessWord::Cruise);
        shared.set_land_at(Some(1_900_000_000));
        chat.pre_draw_tick();
        assert!(render_bottom_popup(&chat, 48).contains("Cruise until"));
        press(&mut chat, KeyCode::Esc);
        let cruise = render_bottom_popup(&chat, 48);
        assert!(
            cruise.contains("Cruise. Switches to Land") && cruise.contains("Cruise (current)"),
            "{cruise}"
        );
        assert_eq!(selected(&chat), Some(6));
        shared.set_land_at(None);
        chat.pre_draw_tick();
        assert!(!render_bottom_popup(&chat, 48).contains("Cruise. Switches to Land"));
        assert_eq!(selected(&chat), Some(6));

        press(&mut chat, KeyCode::Char('2'));
        let land = format!("@{} meta set autopilot land", fake.endpoint().sid);
        assert!(fake.wait_for(|fake| fake.requests().contains(&land)).await);
        let writes: Vec<_> = fake
            .requests()
            .into_iter()
            .filter(|request| request.contains("meta set autopilot"))
            .collect();
        assert_eq!(writes, [land]);
    }

    #[tokio::test]
    async fn unavailable_autopilot_timer_returns_to_its_picker_without_submitting() {
        use crate::chatwidget::tests::render_bottom_popup;
        use codex_alab_aterm::fake::FakeAterm;
        use codex_alab_aterm::fake::Mode;

        let dir = tempfile::tempdir().unwrap();
        let fake = FakeAterm::start(dir.path(), Mode::Current).await.unwrap();
        let (_interface, _automation, shared) = routed(&fake);
        let (mut chat, _tx, mut events, _operations) = make_chatwidget_manual_with_sender().await;
        chat.bottom_pane
            .set_composer_text("my draft".into(), vec![], vec![]);
        // Construct a valid request first. A rejected UI action must not supersede it.
        let queued = interface::set_harness_mode(HarnessWord::Land);
        for unavailable in [
            HarnessWord::HarnessOff,
            HarnessWord::AgentOff,
            HarnessWord::Unhosted,
            HarnessWord::Unknown,
        ] {
            shared.set_harness_word(HarnessWord::Land);
            chat.alab_interface_command("menu");
            chat.bottom_pane.handle_key_event(KeyCode::Char('7').into());
            assert!(
                std::iter::from_fn(|| events.try_recv().ok())
                    .any(|event| matches!(event, AppEvent::OpenAlabAutopilotTimer))
            );
            chat.alab_open_autopilot_timer();
            chat.bottom_pane.handle_paste("+37m".into());
            assert!(render_bottom_popup(&chat, 80).contains("+37m"));
            shared.set_harness_word(unavailable);
            if unavailable == HarnessWord::HarnessOff {
                chat.pre_draw_tick();
            }
            chat.bottom_pane.handle_key_event(KeyCode::Enter.into());
            assert_eq!(
                chat.bottom_pane.active_view_id(),
                Some("alab-autopilot"),
                "{unavailable:?}"
            );
            chat.pre_draw_tick();
            let parent = render_bottom_popup(&chat, 120);
            assert!(
                parent.contains(&interface::AutopilotStatus::read().picker_subtitle()),
                "{parent}"
            );
            chat.bottom_pane.handle_key_event(KeyCode::Esc.into());
            assert_eq!(chat.bottom_pane.composer_text(), "my draft");
        }
        queued.await.unwrap();
        let writes: Vec<_> = fake
            .requests()
            .into_iter()
            .filter(|r| r.contains(" meta set autopilot "))
            .collect();
        assert_eq!(
            writes,
            [format!("@{} meta set autopilot land", fake.endpoint().sid)]
        );
    }

    #[tokio::test]
    async fn unavailable_autopilot_picker_rechecks_shortcuts_and_pending_timer_open() {
        use codex_alab_aterm::fake::FakeAterm;
        use codex_alab_aterm::fake::Mode;

        let dir = tempfile::tempdir().unwrap();
        let fake = FakeAterm::start(dir.path(), Mode::Current).await.unwrap();
        let (_interface, _automation, shared) = routed(&fake);
        let (mut chat, _tx, mut events, _operations) = make_chatwidget_manual_with_sender().await;
        let mut keymap = crate::keymap::RuntimeKeymap::defaults();
        keymap.list.cancel = vec![crate::key_hint::plain(KeyCode::Char('q'))];
        chat.bottom_pane.set_keymap_bindings(&keymap);
        let queued = interface::set_harness_mode(HarnessWord::Off);
        chat.alab_interface_command("menu");
        shared.set_harness_word(HarnessWord::AgentOff);
        // The picker still displays the choices from the preceding selectable report.
        for key in std::iter::once(KeyCode::Enter).chain(('1'..='7').map(KeyCode::Char)) {
            chat.bottom_pane.handle_key_event(key.into());
            assert_eq!(chat.bottom_pane.active_view_id(), Some("alab-autopilot"));
            assert!(
                !std::iter::from_fn(|| events.try_recv().ok())
                    .any(|event| matches!(event, AppEvent::OpenAlabAutopilotTimer))
            );
        }
        shared.set_harness_word(HarnessWord::Land);
        chat.pre_draw_tick();
        chat.bottom_pane.handle_key_event(KeyCode::Char('7').into());
        assert!(
            std::iter::from_fn(|| events.try_recv().ok())
                .any(|event| matches!(event, AppEvent::OpenAlabAutopilotTimer))
        );
        shared.set_harness_word(HarnessWord::Unhosted);
        chat.alab_open_autopilot_timer();
        assert_eq!(chat.bottom_pane.active_view_id(), Some("alab-autopilot"));
        chat.pre_draw_tick();
        let disabled = crate::chatwidget::tests::render_bottom_popup(&chat, 48);
        assert!(disabled.contains("q back"), "{disabled}");
        assert!(!disabled.contains("enter select"), "{disabled}");
        assert_eq!(disabled.matches('–').count(), 7, "{disabled}");
        chat.bottom_pane.handle_key_event(KeyCode::Esc.into());
        assert_eq!(chat.bottom_pane.active_view_id(), Some("alab-autopilot"));
        chat.bottom_pane.handle_key_event(KeyCode::Char('q').into());
        assert!(chat.bottom_pane.no_modal_or_popup_active());
        queued.await.unwrap();
        let writes: Vec<_> = fake
            .requests()
            .into_iter()
            .filter(|r| r.contains(" meta set autopilot "))
            .collect();
        assert_eq!(
            writes,
            [format!("@{} meta set autopilot off", fake.endpoint().sid)]
        );
    }

    #[tokio::test]
    async fn invalid_cruise_deadline_keeps_the_editor_and_can_be_corrected_in_place() {
        use crate::chatwidget::tests::render_bottom_popup;
        use codex_alab_aterm::fake::FakeAterm;
        use codex_alab_aterm::fake::Mode;

        let dir = tempfile::tempdir().unwrap();
        let fake = FakeAterm::start(dir.path(), Mode::Current).await.unwrap();
        let _scopes = routed(&fake);
        let (mut chat, _tx, mut events, _operations) = make_chatwidget_manual_with_sender().await;
        chat.bottom_pane
            .set_composer_text("unfinished draft".into(), vec![], vec![]);
        chat.alab_interface_command("menu");
        chat.bottom_pane.handle_key_event(KeyCode::Char('7').into());
        assert!(
            std::iter::from_fn(|| events.try_recv().ok())
                .any(|event| matches!(event, AppEvent::OpenAlabAutopilotTimer))
        );
        chat.alab_open_autopilot_timer();
        let queued = interface::set_harness_mode(HarnessWord::Land);
        chat.bottom_pane.handle_paste("+37xm".into());
        chat.bottom_pane.handle_key_event(KeyCode::Left.into());
        chat.bottom_pane.handle_key_event(KeyCode::Enter.into());
        let editor = render_bottom_popup(&chat, 44);
        assert!(editor.contains("+37xm"), "{editor}");
        assert!(
            editor.contains("HH:MM") && editor.contains("+1h30m"),
            "{editor}"
        );

        // The invalid Enter leaves both the cursor and the last valid request in place.
        queued.await.unwrap();
        let land = format!("@{} meta set autopilot land", fake.endpoint().sid);
        assert!(fake.requests().contains(&land));
        chat.bottom_pane.handle_key_event(KeyCode::Backspace.into());
        chat.bottom_pane.handle_key_event(KeyCode::Enter.into());
        let timed = format!(
            "@{} meta set autopilot cruise until=+37m",
            fake.endpoint().sid
        );
        assert!(fake.wait_for(|fake| fake.requests().contains(&timed)).await);
        let writes: Vec<_> = fake
            .requests()
            .into_iter()
            .filter(|request| request.contains(" meta set autopilot "))
            .collect();
        assert_eq!(writes, [land, timed]);
        assert!(!chat.bottom_pane.has_active_view());
        assert_eq!(chat.bottom_pane.composer_text(), "unfinished draft");
    }

    #[tokio::test]
    async fn custom_timer_escape_restores_the_picker_and_submit_preserves_the_draft() {
        use crate::chatwidget::tests::render_bottom_popup;
        use crate::render::renderable::Renderable;
        use codex_alab_aterm::fake::FakeAterm;
        use codex_alab_aterm::fake::Mode;
        let dir = tempfile::tempdir().expect("scratch");
        let fake = FakeAterm::start(dir.path(), Mode::Current)
            .await
            .expect("fake");
        let _scopes = routed(&fake);
        let (mut chat, _tx, mut rx, mut op_rx) = make_chatwidget_manual_with_sender().await;
        chat.bottom_pane
            .set_composer_text("unfinished draft".into(), vec![], vec![]);
        while op_rx.try_recv().is_ok() {}
        let before = fake.requests();
        chat.alab_interface_command("menu");
        let area = Rect::new(2, 3, 80, chat.desired_height(80));
        let mut buf = Buffer::empty(area);
        chat.render(area, &mut buf);
        let row = (area.y..area.bottom())
            .find(|&y| {
                let text: String = (area.x..area.right())
                    .map(|x| buf[(x, y)].symbol())
                    .collect();
                text.contains("Cruise until…")
            })
            .expect("custom timer choice is visible");
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            assert!(chat.alab_picker_mouse(MouseEvent {
                kind,
                column: area.x + 7,
                row,
                modifiers: KeyModifiers::NONE,
            }));
        }
        assert!(
            std::iter::from_fn(|| rx.try_recv().ok())
                .any(|event| matches!(event, AppEvent::OpenAlabAutopilotTimer))
        );
        chat.alab_open_autopilot_timer();
        for width in [16, 22, 26, 27, 28] {
            let editor = render_bottom_popup(&chat, width);
            assert!(editor.contains("18:00 or +2h"), "{width}: {editor}");
            if width < 27 {
                let mut lines = editor.lines().map(str::trim);
                assert!(
                    lines.any(|line| line == "enter submit"),
                    "{width}: {editor}"
                );
                assert_eq!(lines.next(), Some("esc back"), "{width}: {editor}");
            } else {
                assert!(
                    editor.contains("enter submit · esc back"),
                    "{width}: {editor}"
                );
            }
        }
        chat.bottom_pane.handle_paste("+2h".into());
        chat.bottom_pane.handle_key_event(KeyCode::Esc.into());
        assert_eq!(chat.bottom_pane.active_view_id(), Some("alab-autopilot"));
        assert_eq!(
            chat.bottom_pane
                .selected_index_for_present_view("alab-autopilot"),
            Some(6)
        );
        assert_eq!(fake.requests(), before);
        chat.bottom_pane.handle_key_event(KeyCode::Enter.into());
        assert!(
            std::iter::from_fn(|| rx.try_recv().ok())
                .any(|event| matches!(event, AppEvent::OpenAlabAutopilotTimer))
        );
        chat.alab_open_autopilot_timer();
        chat.bottom_pane.handle_paste("+4h".into());
        chat.bottom_pane
            .handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let timed = format!(
            "@{} meta set autopilot cruise until=+4h",
            fake.endpoint().sid
        );
        assert!(fake.wait_for(|fake| fake.requests().contains(&timed)).await);
        assert!(!chat.bottom_pane.has_active_view());
        assert_eq!(chat.bottom_pane.composer_text(), "unfinished draft");
        // An already-dismissed menu cannot open an editor through a delayed app event.
        chat.alab_interface_command("menu");
        chat.bottom_pane.handle_key_event(KeyCode::Char('7').into());
        assert!(
            std::iter::from_fn(|| rx.try_recv().ok())
                .any(|event| matches!(event, AppEvent::OpenAlabAutopilotTimer))
        );
        chat.bottom_pane.handle_key_event(KeyCode::Esc.into());
        chat.alab_open_autopilot_timer();
        assert!(!chat.bottom_pane.has_active_view());
        assert!(
            op_rx.try_recv().is_err(),
            "editing a timer never sends a model operation"
        );
    }
}
