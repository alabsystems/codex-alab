// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::LogicalLineSource;
use super::TextLayout;
use crate::history_cell::HistoryCell;
use crate::history_cell::SessionInfoCell;
use codex_alab_tui::banner::artwork::image_columns;
use std::ops::Range;
use std::sync::Arc;

impl TextLayout {
    /// Both PRIMARY and rich clipboard serialization read this range. Use the held layout,
    /// so resizing, live session updates and changes in graphics capability cannot move it.
    pub(in crate::transcript_view) fn launch_card_copy(
        self: Arc<Self>,
        cell: Option<&Arc<dyn HistoryCell>>,
        range: Range<usize>,
    ) -> Option<(Arc<Self>, Range<usize>)> {
        if cell
            .and_then(|cell| cell.as_any().downcast_ref::<SessionInfoCell>())
            .is_some_and(SessionInfoCell::has_codex_launch_card)
        {
            self.card_copy_range(range)
        } else {
            Some((self, range))
        }
    }

    fn card_copy_range(self: Arc<Self>, range: Range<usize>) -> Option<(Arc<Self>, Range<usize>)> {
        let selected = self.text.get(range.clone())?;
        if selected.is_ascii() || !self.text.contains('\u{10eeee}') {
            return Some((self, range));
        }
        // Restored layouts store the selectable string, painted spans and rich sources
        // independently. Only shared byte coordinates can identify artwork in all three.
        if !self.copy_sources_agree() {
            return Some(self.plain_copy(selected));
        }
        let mut copy = None;
        let mut removed = 0;
        let mut offset = 0;
        // The opening card is the first part. Fullscreen session layout separates later
        // parts with an empty logical line. User-provided facts keep their own span styles.
        for (index, line) in self.logical.iter().enumerate() {
            if offset >= range.end || line.origin.text.is_empty() {
                break;
            }
            for (column, span) in line.line.line.spans.iter().enumerate() {
                let len = span.content.len();
                let start = range.start.saturating_sub(offset).min(len);
                let end = range.end.saturating_sub(offset).min(len);
                offset += len;
                if start < end
                    && let Some(columns) = image_columns(span)
                {
                    // Every tile in a validated protocol row has the same encoded length.
                    let Some(part) = span.content.get(start..end) else {
                        return Some(self.plain_copy(selected));
                    };
                    let tile_bytes = len / columns;
                    let blanks = end.div_ceil(tile_bytes) - start / tile_bytes;
                    let lines = copy.get_or_insert_with(|| self.logical.clone());
                    let projected = &mut lines[index];
                    projected.line.line.spans[column]
                        .content
                        .to_mut()
                        .replace_range(start..end, &" ".repeat(blanks));
                    projected.origin = LogicalLineSource::from_line(&projected.line.line);
                    removed += part.len() - blanks;
                }
            }
            offset += 1; // The hard break between logical lines.
        }
        let Some(lines) = copy else {
            return Some((self, range));
        };
        let projected = Self::from_logical(lines, self.width);
        let projected_range = range.start..range.end.checked_sub(removed)?;
        if projected.text.get(projected_range.clone()).is_none() {
            return Some(self.plain_copy(selected));
        }
        Some((Arc::new(projected), projected_range))
    }

    fn copy_sources_agree(&self) -> bool {
        let mut held = self.text.as_str();
        for (index, line) in self.logical.iter().enumerate() {
            if index > 0 {
                let Some(rest) = held.strip_prefix('\n') else {
                    return false;
                };
                held = rest;
            }
            let Some(origin) = line.origin.text.get(line.origin.range.clone()) else {
                return false;
            };
            let Some(rest) = held.strip_prefix(origin) else {
                return false;
            };
            let painted = line
                .line
                .line
                .spans
                .iter()
                .flat_map(|span| span.content.bytes());
            if !origin.bytes().eq(painted) {
                return false;
            }
            held = rest;
        }
        held.is_empty()
    }

    fn plain_copy(&self, selected: &str) -> (Arc<Self>, Range<usize>) {
        let lines = selected.split('\n').map(|s| s.to_owned().into()).collect();
        (Arc::new(Self::new(lines, self.width)), 0..selected.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard_copy::CopyFormat;
    use crate::history_cell::AgentMarkdownCell;
    use color_eyre::eyre::OptionExt;
    use ratatui::style::Color;
    use ratatui::style::Style;
    use ratatui::text::Line;
    use ratatui::text::Span;
    use std::path::Path;

    #[test]
    fn disagreeing_retained_launch_sources_copy_only_valid_held_text() -> color_eyre::Result<()> {
        let glyph = '\u{10eeee}';
        let tiles: String = ['\u{305}', '\u{30d}', '\u{30e}', '\u{310}']
            .into_iter()
            .flat_map(|col| [glyph, '\u{305}', col])
            .collect();
        let art = Span::styled(tiles, Style::default().fg(Color::from_u32(0xa1ab05)));
        assert_eq!(image_columns(&art), Some(4));
        let valid = Arc::new(TextLayout::new(vec![Line::from(art.clone()).into()], 80));
        // A restored range can begin after the base code point, inside one tile's marks.
        let (partial, range) = valid.card_copy_range(4..6).ok_or_eyre("coordinate mark")?;
        assert_eq!(partial.text.get(range), Some(" "));
        let marked = AgentMarkdownCell::new("**next**".into(), Path::new("/"));
        let markdown = TextLayout::new(marked.retained_hyperlink_lines(80, false), 80);
        for case in 0..6 {
            let mut held = TextLayout::new(
                vec!["a".into(), Line::from(art.clone()).into(), "tail".into()],
                80,
            );
            match case {
                // A valid held endpoint now cuts into a multibyte painted tile.
                0 => held.text.insert(0, 'x'),
                1 => held.logical[1].origin = LogicalLineSource::new("different".into()),
                2 => held.logical[0].line.line.spans[0].content = "b".into(),
                3 => held.text.replace_range(1..2, " "),
                4 => held.text.push('!'),
                _ => held.logical[2].line.line.spans.clear(),
            }
            let at = held.text.find(glyph).ok_or_eyre("tile in held text")?;
            let held = Arc::new(held);
            assert!(held.clone().card_copy_range(at + 1..at + 4).is_none());
            let ordinary = held
                .clone()
                .card_copy_range(0..1)
                .ok_or_eyre("ordinary range")?;
            assert!(Arc::ptr_eq(&held, &ordinary.0));
            let (plain, range) = held
                .card_copy_range(at..at + glyph.len_utf8())
                .ok_or_eyre("valid held range")?;
            let wanted = glyph.to_string();
            assert_eq!(plain.text.get(range.clone()), Some(wanted.as_str()));
            let mut lines = Vec::new();
            plain.copy_lines(range, "", &mut lines);
            markdown.copy_lines(0..markdown.text.len(), "\n", &mut lines);
            let selected = format!("{wanted}\nnext");
            let (rich, format) = crate::markdown_copy::selection(&lines, &selected);
            assert!(matches!(format, CopyFormat::Markdown));
            assert!(
                rich.contains(&wanted) && rich.contains("**next**"),
                "{rich:?}"
            );
        }
        Ok(())
    }
}
