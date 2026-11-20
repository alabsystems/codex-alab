// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The native transcript is written once as styled terminal lines. Half-block cells
//! stay anchored to the message as it scrolls into terminal-owned history.

use crate::app::App;
use crate::history_cell::HistoryCell;
use crate::history_cell::HistoryRenderMode;
use crate::history_cell::UserHistoryCell;
use crate::pets::ImageProtocol;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_palette::rgb_color;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;

impl App {
    pub(in crate::app) fn local_image_protocol(&self) -> Option<ImageProtocol> {
        self.chat_widget.pet_image_support().protocol().filter(|_| {
            !self.app_server_target.uses_remote_workspace()
                && crate::projectless::has_only_local_environments(&self.environment_manager)
        })
    }

    pub(super) fn native_cell_lines(
        &self,
        cell: &dyn HistoryCell,
        width: u16,
    ) -> Vec<HyperlinkLine> {
        let mode = self.chat_widget.history_render_mode();
        let mut lines = cell.display_hyperlink_lines_for_mode(width, mode);
        let Some(user) = cell.as_any().downcast_ref::<UserHistoryCell>() else {
            return lines;
        };
        if mode == HistoryRenderMode::Rich
            && codex_alab_tui::interface::active()
            && width > 2
            && !(user.local_image_paths.is_empty() && user.remote_image_urls.is_empty())
            && self.local_image_protocol().is_some()
        {
            // Two square samples per character approximate the terminal's 1:2 cell aspect.
            let columns = u32::from(width.saturating_sub(2).min(48));
            let background = crate::terminal_palette::default_bg().unwrap_or((48, 48, 48));
            for pixels in crate::transcript_image::user_image_pixels(user, columns, 12) {
                let color_at = |x, y| {
                    let Some(pixel) = pixels.get_pixel_checked(x, y) else {
                        return rgb_color(background);
                    };
                    let [r, g, b, alpha] = pixel.0;
                    rgb_color(crate::color::blend(
                        (r, g, b),
                        background,
                        f32::from(alpha) / 255.0,
                    ))
                };
                for y in (0..pixels.height()).step_by(2) {
                    let mut row = vec![Span::raw("  ")];
                    for x in 0..pixels.width() {
                        let style = Style::default().fg(color_at(x, y)).bg(color_at(x, y + 1));
                        row.push(Span::styled("▀", style));
                    }
                    lines.push(HyperlinkLine::new(Line::from(row)));
                }
            }
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppServerTarget;
    use crate::pets::PetImageSupport;

    #[tokio::test]
    async fn pasted_image_in_native_history_survives_reflow() -> color_eyre::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("paste.png");
        let pixels = image::RgbaImage::from_fn(3, 2, |_x, y| {
            if y == 0 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            }
        });
        pixels.save(&path)?;
        let mut app = crate::app::test_support::make_test_app().await;
        app.chat_widget
            .set_pet_image_support_for_tests(PetImageSupport::Supported(ImageProtocol::Kitty));
        use codex_alab_tui::interface;
        let _scope = interface::scoped(Box::leak(Box::new(interface::State::new())));
        interface::set_enabled(true);
        let mut tui = crate::tui::test_support::make_test_tui()?;
        tui.set_owned_screen(false)?;
        let width = app
            .chat_widget
            .history_wrap_width(tui.terminal.last_known_screen_size.width);
        let cell = crate::history_cell::new_user_prompt(String::new(), vec![], vec![path], vec![]);
        app.insert_history_cell(&mut tui, Box::new(cell));
        let live = tui.pending_history_lines_for_test();
        let image_row = live.last().unwrap();
        assert_eq!(image_row.line.to_string(), "  ▀▀▀");
        for pixel in &image_row.line.spans[1..] {
            assert_eq!(pixel.style.fg, Some(rgb_color((255, 0, 0))));
            assert_eq!(pixel.style.bg, Some(rgb_color((0, 0, 255))));
        }
        assert_eq!(app.render_transcript_lines_for_reflow(width).lines, live);
        app.chat_widget.set_raw_output_mode(true);
        let raw = app.render_transcript_lines_for_reflow(width).lines;
        assert!(raw.iter().all(|line| !line.line.to_string().contains('▀')));
        app.chat_widget.set_raw_output_mode(false);
        app.app_server_target = AppServerTarget::Remote {
            endpoint: crate::resolve_remote_addr("ws://127.0.0.1:4500")?,
        };
        assert_eq!(
            app.render_transcript_lines_for_reflow(width).lines,
            live[..live.len() - 1]
        );
        Ok(())
    }
}
