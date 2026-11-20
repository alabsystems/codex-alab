// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A Windows session header can enter inline history before the protected color probe runs.
//! The queue freezes its facts at insertion. Only its artwork can acquire the reported ink.

use super::PendingHistoryLines;
use super::Tui;
use crate::insert_history::HistoryLineWrapPolicy;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_palette;
use codex_alab_tui::banner::Banner;
use codex_alab_tui::banner::Harness;
use ratatui::text::Line;
use std::ops::Range;

pub(crate) struct PendingLaunchArt {
    pub(crate) rows: Range<usize>,
    pub(crate) banner: Banner,
    pub(crate) width: u16,
    pub(crate) harness: Option<Harness>,
    pub(crate) details: Vec<Line<'static>>,
}

impl Tui {
    pub(crate) fn queue_inline_launch(
        &mut self,
        lines: Vec<HyperlinkLine>,
        wrap_policy: HistoryLineWrapPolicy,
        mut art: PendingLaunchArt,
    ) {
        let added = lines.len();
        self.insert_history_hyperlink_lines_with_wrap_policy(lines, wrap_policy);
        if let Some(batch) = self.pending_history_lines.last_mut() {
            // Normal insertion may have appended to an existing same-policy batch.
            let offset = batch.lines.len() - added;
            art.rows = offset + art.rows.start..offset + art.rows.end;
            batch.launch_art.push(art);
        }
    }
}

impl PendingHistoryLines {
    pub(super) fn with_launch_palette(mut self) -> Self {
        for art in self.launch_art.drain(..) {
            let Some(banner) = art
                .banner
                .with_late_background(terminal_palette::default_bg)
            else {
                continue;
            };
            let Some(rows) = self.lines.get_mut(art.rows) else {
                continue;
            };
            let painted = banner.session_lines(art.width, art.harness, art.details);
            if rows.len() == painted.len()
                && rows
                    .iter()
                    .zip(&painted)
                    .all(|(row, line)| row.line.to_string() == line.to_string())
            {
                for (row, line) in rows.iter_mut().zip(painted) {
                    row.line = line;
                }
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_alab_tui::banner::Depth;
    use codex_alab_tui::banner::Style;
    use codex_alab_tui::banner::Theme;

    #[tokio::test]
    async fn coalesced_launch_art_keeps_its_row_ranges_until_discard() -> std::io::Result<()> {
        let mut tui = terminal_palette::with_test_default_colors(
            None,
            crate::tui::test_support::make_test_tui,
        )?;
        let policy = HistoryLineWrapPolicy::PreWrap;
        let art = |rows| PendingLaunchArt {
            rows,
            banner: Banner::new(Style::Codex, Depth::TrueColor, Theme::default(), "0.11"),
            width: 80,
            harness: None,
            details: Vec::new(),
        };
        tui.insert_history_lines(vec!["prefix".into()]);
        tui.queue_inline_launch(
            vec!["".into(), "first".into(), "tip".into()],
            policy,
            art(1..2),
        );
        tui.queue_inline_launch(
            vec!["".into(), "second".into(), "continued".into()],
            policy,
            art(1..3),
        );
        tui.insert_history_lines(vec!["suffix".into()]);

        let [batch] = tui.pending_history_lines.as_slice() else {
            panic!("same-policy history must share one batch");
        };
        let [first, second] = batch.launch_art.as_slice() else {
            panic!("both launch records must belong to that batch");
        };
        assert_eq!(first.rows, 2..3);
        assert_eq!(second.rows, 5..7);
        assert_eq!(
            batch.lines,
            [
                "prefix",
                "",
                "first",
                "tip",
                "",
                "second",
                "continued",
                "suffix"
            ]
            .map(HyperlinkLine::from)
        );
        tui.clear_pending_history_lines();
        assert!(tui.pending_history_lines.is_empty());
        Ok(())
    }
}
