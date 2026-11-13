// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
impl<B> Terminal<B>
where
    B: Backend<Error = io::Error> + Write,
{
    pub(crate) fn alab_visible_history_rows(&self) -> u16 {
        self.visible_history_rows
    }
    pub(crate) fn alab_adopt_viewport(&mut self, view: &codex_alab_tui::upgrade_startup::Snapshot) {
        let [x, y, width, height] = view.viewport;
        let screen = Rect::new(
            0,
            0,
            self.last_known_screen_size.width,
            self.last_known_screen_size.height,
        );
        let area = if view.screen.active {
            screen
        } else {
            Rect::new(x, y, width, height).intersection(screen)
        };
        self.set_viewport_area(area);
        self.visible_history_rows = view.visible_history_rows.min(area.top());
        self.hidden_cursor = view.hidden_cursor;
        self.last_known_cursor_pos.x = view.cursor[0].min(screen.width.saturating_sub(1));
        self.last_known_cursor_pos.y = view.cursor[1].min(screen.height.saturating_sub(1));
    }
}
