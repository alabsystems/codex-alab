// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compiled inside the actual TUI terminal owner. Source transfer retains its
//! input and protocol modes while still releasing stderr descriptor ownership.
use super::EventBroker;

pub(super) fn preserve(events: Option<&EventBroker>) -> std::io::Result<bool> {
    // SAME actual editor owns modes/input/stdio; even the original panic path
    // must not finish/replace its saved stderr descriptor or flush its TTY.
    if codex_alab_tui::terminal_modes::editor_owns_terminal() {
        return Ok(true);
    }
    if !codex_alab_tui::terminal_modes::preserve_on_exit() {
        return Ok(false);
    }
    if let Some(events) = events {
        events.pause_events();
    }
    super::terminal_stderr::finish()?;
    Ok(true)
}
