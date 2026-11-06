// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Full-screen overlay cleanup for pet images, compiled inside codex-tui's App.

use super::App;
use crate::tui::Tui;

/// Hide an enabled pet while retaining its frames. A queued disable can arrive while
/// an overlay is open; release its frames on that draw, without waiting for the overlay to close.
/// Picker preview placement is hidden with the overlay and remains available on return.
pub(super) fn hide(app: &App, tui: &mut Tui) -> Result<(), crate::pets::PetImageRenderError> {
    if codex_alab_tui::interface::available() {
        if app.chat_widget.ambient_pet_image_enabled() {
            tui.draw_ambient_pet_image(None)?;
        } else {
            tui.clear_ambient_pet_image()?;
        }
        if app.chat_widget.should_clear_pet_picker_preview_image() {
            tui.draw_pet_picker_preview_image(None)?;
        }
    }
    Ok(())
}
