// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compiled inside tui: restoration precedes all normal startup queries/output.
use super::*;
#[cfg(unix)]
use codex_alab_tui::upgrade_startup::Snapshot;

#[cfg(unix)]
impl Tui {
    pub(crate) fn alab_upgrade_startup_snapshot(&self) -> Snapshot {
        Snapshot {
            version: 1,
            cursor: [
                self.terminal.last_known_cursor_pos.x,
                self.terminal.last_known_cursor_pos.y,
            ],
            colors: crate::terminal_palette::alab_cached_colors(),
            enhanced_keys: self.enhanced_keys_supported,
            terminal_app_over_ssh: self.terminal_app_over_ssh,
            reports_key_releases: keyboard_modes::reports_key_releases(),
            da1_supported: crate::terminal_probe::alab_da1_supported(),
            tmux_image_base: self.terminal.tmux_kitty_image_base,
            viewport: rect(self.terminal.viewport_area),
            saved_viewport: self.alt_saved_viewport.map(rect),
            visible_history_rows: self.terminal.alab_visible_history_rows(),
            hidden_cursor: self.terminal.hidden_cursor,
            screen: ALTERNATE_SCREEN.alab_checkpoint(),
            focused: self.terminal_focused.load(Ordering::Relaxed),
            owned_screen: self.owned_screen,
            alt_screen_enabled: self.alt_screen_enabled,
            mouse_capture: match self.mouse_capture {
                MouseCapture::Inherit => 0,
                MouseCapture::Enabled => 1,
                MouseCapture::Disabled => 2,
            },
        }
    }
}
#[cfg(unix)]
pub(super) fn restore() -> Result<Option<InitializedTerminal>> {
    let Some(owner) = codex_alab_tui::upgrade_startup::begin() else {
        return Ok(None);
    };
    let startup = owner.snapshot();
    // The original termios owner was adopted before this initialization. Enabling
    // the new Crossterm cache must still go through that same native wrapper.
    let mut guard = TerminalInitializationGuard { active: true };
    ALTERNATE_SCREEN.alab_adopt(&startup.screen);
    crate::terminal_probe::alab_set_da1_supported(startup.da1_supported);
    keyboard_modes::set_reports_key_releases(startup.reports_key_releases);
    enable_raw_mode()?;
    set_panic_hook();
    let colors = startup
        .colors
        .map(|[fg, bg]| crate::terminal_probe::DefaultColors {
            fg: (fg[0], fg[1], fg[2]),
            bg: (bg[0], bg[1], bg[2]),
        });
    crate::terminal_palette::set_default_colors_from_startup_probe(colors);
    let mut terminal = CustomTerminal::with_options_and_cursor_position(
        CrosstermBackend::new(stdout()),
        Position::new(startup.cursor[0], startup.cursor[1]),
    )?;
    terminal.tmux_kitty_image_base = startup.tmux_image_base;
    terminal.alab_adopt_viewport(startup);
    let initialized = InitializedTerminal {
        terminal,
        enhanced_keys_supported: startup.enhanced_keys,
        terminal_app_over_ssh: startup.terminal_app_over_ssh,
        stderr_guard: terminal_stderr::TerminalStderrGuard::install()?,
    };
    owner.complete();
    guard.active = false;
    Ok(Some(initialized))
}

#[cfg(unix)]
fn rect(area: Rect) -> [u16; 4] {
    [area.x, area.y, area.width, area.height]
}
#[allow(unused_mut)]
pub(super) fn finish(mut tui: Tui) -> Tui {
    #[cfg(unix)]
    if let Some(view) = codex_alab_tui::upgrade_startup::take_initialized() {
        tui.alt_saved_viewport = view
            .saved_viewport
            .map(|[x, y, w, h]| Rect::new(x, y, w, h));
        tui.alt_screen_active
            .store(view.screen.active, Ordering::Relaxed);
        tui.terminal_focused.store(view.focused, Ordering::Relaxed);
        tui.record_owned_screen(view.owned_screen);
        tui.alt_screen_enabled = view.alt_screen_enabled;
        tui.mouse_capture = match view.mouse_capture {
            1 => MouseCapture::Enabled,
            2 => MouseCapture::Disabled,
            _ => MouseCapture::Inherit,
        };
        tui.terminal_app_over_ssh = view.terminal_app_over_ssh;
    }
    tui
}

#[cfg(unix)]
pub fn restore_after_failed_upgrade() -> std::io::Result<()> {
    if let Some(view) = codex_alab_tui::upgrade_startup::take_for_failure() {
        ALTERNATE_SCREEN.alab_adopt(&view.screen);
        crate::terminal_probe::alab_set_da1_supported(view.da1_supported);
        keyboard_modes::set_reports_key_releases(view.reports_key_releases);
    }
    // This is an explicit handoff back to the shell, unlike successful exec.
    // Use the same quarantine and protocol cleanup as ordinary TUI shutdown.
    let events = EventBroker::new();
    restore_after_exit_with_events(Some(&events))
}

#[cfg(all(test, unix))]
#[path = "upgrade_startup_tests.rs"]
mod tests;
