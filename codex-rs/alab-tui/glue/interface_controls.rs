// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Live experience controls, compiled inside `chatwidget`. The switch preserves
//! the saved Autopilot preference, permissions, selected account and draft.

use super::ChatWidget;
use crate::app_event::AppEvent;
use crate::history_cell;
use crate::legacy_core::config::edit::ConfigEdit;
use crate::legacy_core::config::edit::ConfigEditsBuilder;
use codex_alab_tui::interface;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;

#[path = "retry_notice.rs"]
mod retry_notice;

#[path = "autopilot_menu.rs"]
mod autopilot_menu;

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
        let enabled = match args.trim().to_ascii_lowercase().as_str() {
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
                let mode = match args.trim().to_ascii_lowercase().as_str() {
                    "cruise" => codex_alab_tui::session::HarnessWord::Cruise,
                    "land" => codex_alab_tui::session::HarnessWord::Land,
                    _ => codex_alab_tui::session::HarnessWord::Off,
                };
                if interface::enabled() {
                    self.alab_set_harness_mode(mode);
                }
                return;
            }
            // ATERM'S TIMED CRUISE (gap 12 of the parity sweep): aterm reads `<when>` and
            // refuses what it cannot read; a bare or second `until=` is the usage line's.
            words if cruise_until(words).is_some() => {
                if interface::enabled()
                    && let Some(when) = cruise_until(words)
                {
                    self.alab_set_cruise_until(when.to_string());
                }
                return;
            }
            "menu" if interface::enabled() => {
                self.alab_open_autopilot_menu();
                return;
            }
            "autopilot" if interface::enabled() => {
                self.add_info_message(interface::autopilot_words(), None);
                return;
            }
            "tasks" if interface::enabled() => {
                self.app_event_tx.send(AppEvent::OpenAlabTasks);
                return;
            }
            "stash" if interface::enabled() => {
                self.bottom_pane.alab_stash_prompt();
                return;
            }
            "tasks" | "stash" | "menu" | "autopilot" => {
                self.add_info_message("Use /alab on to enable enhanced controls.".into(), None);
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
        interface::set_enabled(enabled);
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
                "/alab cruise until=HH:MM|+DURATION cruises, then lands then (aterm's timer)."
                    .into(),
            );
            lines.push("/alab autopilot says the session's Autopilot mode in words.".into());
            lines.push("/alab menu opens Autopilot controls; right-click or control-click the mode on the line does too.".into());
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
        let tx = self.app_event_tx.clone();
        let request = interface::set_harness_mode(mode);
        tokio::spawn(async move {
            if let Err(error) = request.await {
                tx.send(AppEvent::InsertHistoryCell(Box::new(
                    history_cell::new_error_event(format!("Could not change Autopilot: {error}")),
                )));
            }
        });
    }

    /// `/alab cruise until=<when>`'s request; an error is said as a mode's is.
    fn alab_set_cruise_until(&self, when: String) {
        let tx = self.app_event_tx.clone();
        let request = interface::set_cruise_until(when);
        tokio::spawn(async move {
            if let Err(error) = request.await {
                tx.send(AppEvent::InsertHistoryCell(Box::new(
                    history_cell::new_error_event(format!("Could not change Autopilot: {error}")),
                )));
            }
        });
    }

    pub(super) fn alab_interface_mouse(&mut self, event: MouseEvent) -> bool {
        let left = event.kind == MouseEventKind::Down(MouseButton::Left);
        // The native Autopilot menu from the line's cells: a right-click, or a
        // control-click (macOS's secondary click). A right-click anywhere else stays upstream's
        // right-click paste.
        let menu = event.kind == MouseEventKind::Down(MouseButton::Right)
            || (left && event.modifiers.contains(KeyModifiers::CONTROL));
        if !interface::available()
            || !(left || menu)
            || self.bottom_pane.has_active_view()
            || !self.bottom_pane.no_modal_or_popup_active()
        {
            return false;
        }
        if menu && interface::autopilot_menu_hit(event.column, event.row) {
            self.alab_open_autopilot_menu();
            return true;
        }
        if !left {
            return false;
        }
        if interface::build_mouse_hit(event.column, event.row) {
            self.alab_interface_command("update");
            true
        } else if let Some(mode) = interface::mode_mouse_hit(event.column, event.row) {
            self.alab_set_harness_mode(mode);
            true
        } else if interface::autopilot_words_hit(event.column, event.row) {
            // `⊘ off`: aterm's Settings hold the harness off, which no click here turns.
            self.add_info_message(interface::autopilot_words(), None);
            true
        } else if interface::mouse_hit(event.column, event.row) {
            self.alab_interface_command("");
            true
        } else if interface::account_mouse_hit(event.column, event.row) {
            self.alab_open_account_details();
            true
        } else {
            false
        }
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

    #[tokio::test]
    async fn help_tracks_remapped_chorded_disabled_and_off_shortcuts() {
        let state = Box::leak(Box::new(interface::State::new()));
        let _scope = interface::scoped(state);
        interface::configure(true);
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        let original = chat.alab_interface_help();
        assert!(original.contains("/alab account"));
        assert!(original.contains("With an empty prompt, press ?"));
        assert!(original.contains("parks a prompt outside search"));

        let mut config = TuiKeymap::default();
        config.composer.history_search_next = Some(KeybindingsSpec::Many(Vec::new()));
        config.editor.move_left = Some(KeybindingsSpec::One(KeybindingSpec("ctrl-s".into())));
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
        assert!(!chat.alab_interface_mouse(click));
        assert!(!interface::enabled());
        click.kind = MouseEventKind::Down(MouseButton::Left);
        assert!(chat.alab_interface_mouse(click));
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
            !chat.alab_interface_mouse(click),
            "old switch target invalid after change"
        );
        interface::render(area, &mut buf, Depth::Mono, Theme::default());
        let button = interface::button_rect(area).expect("on button visible");
        click.column = button.x;
        assert!(chat.alab_interface_mouse(click));
        assert!(!interface::enabled());
        saved_interface_choice(config_path.as_path(), false).await;
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
        assert!(!chat.alab_interface_mouse(event));
        event.kind = MouseEventKind::Down(MouseButton::Left);
        event.row = 19;
        assert!(!chat.alab_interface_mouse(event));
        event.row = 20;
        assert!(chat.alab_interface_mouse(event));
        assert_eq!(chat.bottom_pane.active_view_id(), Some("alab-account"));
        assert!(!chat.alab_interface_mouse(event));
        chat.bottom_pane
            .handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(chat.bottom_pane.active_view_id(), None);
        assert_eq!(
            chat.bottom_pane.composer_text(),
            "keep this while inspecting"
        );
        interface::set_enabled(false);
        assert!(!chat.alab_interface_mouse(event));
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
    async fn autopilot_menu_uses_the_native_picker_without_a_host_menu_request() {
        use codex_alab_aterm::fake::FakeAterm;
        use codex_alab_aterm::fake::Mode;
        let dir = tempfile::tempdir().expect("scratch");
        let fake = FakeAterm::start(dir.path(), Mode::Current)
            .await
            .expect("fake");
        let _scopes = routed(&fake);
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        let before = fake.requests();
        chat.alab_interface_command("menu");
        assert!(
            chat.bottom_pane
                .selected_index_for_present_view("alab-autopilot")
                .is_some()
        );
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
    async fn native_timer_editor_sends_a_deadline_without_submitting_a_model_turn() {
        use codex_alab_aterm::fake::FakeAterm;
        use codex_alab_aterm::fake::Mode;
        let dir = tempfile::tempdir().expect("scratch");
        let fake = FakeAterm::start(dir.path(), Mode::Current)
            .await
            .expect("fake");
        let _scopes = routed(&fake);
        let (mut chat, _tx, _rx, mut op_rx) = make_chatwidget_manual_with_sender().await;
        while op_rx.try_recv().is_ok() {}
        chat.alab_open_autopilot_timer();
        chat.bottom_pane.handle_paste("+4h".into());
        chat.bottom_pane
            .handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let timed = format!(
            "@{} meta set autopilot cruise until=+4h",
            fake.endpoint().sid
        );
        assert!(fake.wait_for(|fake| fake.requests().contains(&timed)).await);
        assert!(
            op_rx.try_recv().is_err(),
            "editing a timer never sends a model operation"
        );
    }
}
