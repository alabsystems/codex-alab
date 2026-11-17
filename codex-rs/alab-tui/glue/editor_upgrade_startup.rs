// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Runtime constructors preserve their screen and leave input in the shared broker.
use super::*;

pub(crate) fn deferred() -> bool {
    #[cfg(unix)]
    {
        codex_alab_tui::upgrade_editor_startup::deferred()
    }
    #[cfg(not(unix))]
    {
        false
    }
}
pub(super) fn clear_screen(terminal: &mut crate::tui::Terminal) -> io::Result<()> {
    if deferred() || preserves_entered_surface() {
        Ok(())
    } else {
        terminal.clear()
    }
}
pub(super) fn alt_screen(tui: &mut Tui, enabled: bool) {
    if !deferred() && !preserves_entered_surface() {
        tui.set_alt_screen_enabled(enabled);
    }
}
pub(super) fn owned_screen(tui: &mut Tui, enabled: bool) -> io::Result<()> {
    if deferred() || preserves_entered_surface() {
        Ok(())
    } else {
        tui.set_owned_screen(enabled)
    }
}
impl StartupDraftPump {
    /// Runtime restore has a native editor owner, never a flattened provisional draft.
    pub(crate) fn alab_into_startup_draft(self) -> Option<ComposerDraftSnapshot> {
        if deferred() {
            None
        } else {
            Some(self.into_draft())
        }
    }
}

impl Tui {
    pub(crate) fn alab_startup_owned_screen(&mut self, enabled: bool) -> io::Result<()> {
        owned_screen(self, enabled)
    }
}

#[cfg(test)]
#[path = "editor_upgrade_startup_tests.rs"]
mod tests;

fn preserves_entered_surface() -> bool {
    #[cfg(unix)]
    {
        crate::owned_local_entered_readers::preserves_entered_no_primary_surface()
    }
    #[cfg(not(unix))]
    {
        false
    }
}
