// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use codex_alab_tui::upgrade_startup::Screen;
impl AlternateScreen {
    pub(in crate::tui) fn alab_checkpoint(&self) -> Screen {
        Screen {
            active: self.active.load(Ordering::Relaxed),
            mouse_active: self.mouse_active.load(Ordering::Relaxed),
            mouse_capture_disabled: self.mouse_capture_disabled.load(Ordering::Relaxed),
            input_configured: self.input_configured.load(Ordering::Relaxed),
            keyboard_active: self.keyboard_active.load(Ordering::Relaxed),
            parent_reports_key_releases: self.parent_reports_key_releases.load(Ordering::Relaxed),
            pointer: self.link_pointer.alab_checkpoint(),
        }
    }
    pub(in crate::tui) fn alab_adopt(&self, snapshot: &Screen) {
        self.active.store(snapshot.active, Ordering::Relaxed);
        self.mouse_active
            .store(snapshot.mouse_active, Ordering::Relaxed);
        self.mouse_capture_disabled
            .store(snapshot.mouse_capture_disabled, Ordering::Relaxed);
        self.input_configured
            .store(snapshot.input_configured, Ordering::Relaxed);
        self.keyboard_active
            .store(snapshot.keyboard_active, Ordering::Relaxed);
        self.parent_reports_key_releases
            .store(snapshot.parent_reports_key_releases, Ordering::Relaxed);
        self.link_pointer.alab_adopt(snapshot.pointer);
    }
}
