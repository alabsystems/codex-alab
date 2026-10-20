// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The rainbow kitty composer border's codex-tui side.
//!
//! Not a file of the codex-alab-tui crate: codex-tui compiles it as
//! `bottom_pane::chat_composer::composer_layout::alab_border` through a `#[path]` module at the
//! end of `tui/src/bottom_pane/chat_composer/composer_layout.rs` (`ALAB(banner)`), because the
//! colour level and background codex probed live in `terminal_palette`, a private module of
//! codex-tui. Its tests run as codex-tui `--lib` tests, so `alab/check.sh` gates them.
//! Everything that draws lives in `codex_alab_tui::banner`; this file only maps codex's probe
//! onto the banner's `Depth` and `Theme`.
//!
//! The one call is `render(composer_rect, buf)` at the end of
//! `ChatComposer::render_voice_strip`, which `render_with_options` calls once the textarea is
//! drawn and right before the sparkle and the interactive-footer dim. So the border lands over
//! the voice strip and under both. The hook sits there, not in `chat_composer.rs`, so that
//! upstream file stays byte-identical to upstream: the publication engine's forbidden-content
//! guard exempts upstream's own test data only in files ALab leaves untouched, and
//! `composer_layout.rs` carries none. Never under upstream's own unit tests: the harness state
//! is unknown unless a session is claimed.

use crate::terminal_palette;
use crate::terminal_palette::StdoutColorLevel;
use codex_alab_tui::banner::Depth;
use codex_alab_tui::banner::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// Draws the border along the top row of `composer_rect` while the codex-alab harness is on
/// (`codex_alab_tui::banner::harness_state()` is `Some(true)`), in the colour depth and theme
/// codex probed for this terminal; otherwise leaves `buf` as upstream drew it.
pub(super) fn render(composer_rect: Rect, buf: &mut Buffer) {
    if codex_alab_tui::banner::harness_state() != Some(true) {
        return;
    }
    let depth = match terminal_palette::effective_stdout_color_level() {
        StdoutColorLevel::TrueColor => Depth::TrueColor,
        StdoutColorLevel::Ansi256 => Depth::Ansi256,
        StdoutColorLevel::Ansi16 => Depth::Ansi16,
        // NO_COLOR, TERM=dumb, not a terminal.
        StdoutColorLevel::Unknown => Depth::Mono,
    };
    let theme = Theme::from_bg(terminal_palette::default_bg());
    codex_alab_tui::banner::render_composer_border(composer_rect, buf, depth, theme);
}

#[cfg(test)]
mod tests {
    use crate::bottom_pane::ChatComposer;
    use crate::bottom_pane::chat_composer::tests::new_test_composer;
    use crate::render::renderable::Renderable;
    use codex_alab_tui::session::Autopilot;
    use codex_alab_tui::session::Shared;
    use pretty_assertions::assert_eq;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    const BADGE: &str = "✦ alab kitty · codex-alab harness ● on";

    /// The composer drawn at `width`, one string per row.
    fn rows(composer: &ChatComposer, width: u16) -> Vec<String> {
        let area = Rect::new(0, 0, width, composer.desired_height(width));
        let mut buf = Buffer::empty(area);
        composer.render(area, &mut buf);
        (area.top()..area.bottom())
            .map(|y| {
                (area.left()..area.right())
                    .map(|x| buf[(x, y)].symbol())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn the_border_follows_the_harness() {
        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        let _scope = codex_alab_tui::boxes::scoped(shared);
        let (composer, _rx) = new_test_composer();

        // Unclaimed: upstream's composer, untouched.
        let upstream = rows(&composer, 80);
        assert!(
            !upstream.iter().any(|row| row.contains("codex-alab")),
            "{upstream:?}"
        );

        // Harness on: one row, the composer's top one, becomes the rule and the badge.
        shared.set_autopilot(Autopilot::Active);
        let on = rows(&composer, 80);
        let changed: Vec<usize> = (0..on.len()).filter(|&y| on[y] != upstream[y]).collect();
        assert_eq!(changed.len(), 1, "{on:?}");
        let border = &on[changed[0]];
        assert!(border.contains(BADGE), "{border:?}");
        assert!(border.contains("──"), "{border:?}");

        // Toggled off mid-session: upstream's composer again.
        shared.set_autopilot(Autopilot::Passive);
        assert_eq!(rows(&composer, 80), upstream);
    }
}
