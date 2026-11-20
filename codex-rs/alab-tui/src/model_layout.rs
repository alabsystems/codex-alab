// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use ratatui::buffer::CellWidth;
use ratatui::style::Style;
use ratatui::text::Span;
use std::ops::Range;

// Ratatui wraps paragraphs one grapheme at a time. Measuring a whole string
// instead counts Arabic lam-alef as one cell and misses halfwidth sound marks.
pub(crate) fn painted_width(text: &str) -> usize {
    Span::raw(text)
        .styled_graphemes(Style::default())
        .map(|g| usize::from(g.symbol.cell_width()))
        .sum()
}

pub(crate) struct ModelBreaks {
    tier: Option<usize>,
    last: usize,
    pub(crate) longest: usize,
}

impl ModelBreaks {
    /// Byte ranges omit the separating spaces but retain the tier's middle dot.
    pub(crate) fn rows(&self, len: usize) -> impl Iterator<Item = Range<usize>> {
        [
            Some(0..self.tier.unwrap_or(self.last)),
            self.tier.map(|tier| tier + 1..self.last),
            Some(self.last + 1..len),
        ]
        .into_iter()
        .flatten()
    }
}

/// Prefer a whole label over two rows. If its prefix must clip, preserve complete trailing words.
pub(crate) fn model_breaks(value: &str, cells: usize, width: usize) -> Option<ModelBreaks> {
    let (last, longest) = two_row_break(value, cells, width)?;
    let mut breaks = ModelBreaks {
        tier: None,
        last,
        longest,
    };
    // Keep a fitting two-row layout. If the prefix would lose part of its tier, fit the
    // settings on two rows of their own, keeping complete words and the dot beside the tier.
    if longest > width
        && let Some((name, _)) = value[..last].split_once(" · ")
        && !name.trim().is_empty()
    {
        let name_width = painted_width(name);
        let start = name.len() + 1;
        let settings = &value[start..];
        if let Some((space, settings_width)) =
            two_row_break(settings, painted_width(settings), width)
            && settings_width <= width
            && space > "· ".len()
        {
            breaks.tier = Some(name.len());
            breaks.last = start + space;
            breaks.longest = name_width.max(settings_width);
        }
    }
    Some(breaks)
}

fn two_row_break(value: &str, cells: usize, width: usize) -> Option<(usize, usize)> {
    if cells <= width || !value.trim().contains(' ') {
        return None;
    }
    let content_end = value.trim_end().len();
    let mut split = None;
    let mut rest = value;
    let mut left_width = 0;
    for grapheme in Span::raw(value).styled_graphemes(Style::default()) {
        let offset = value.len() - rest.len();
        if offset >= content_end || (left_width > width && split.is_some()) {
            break;
        }
        rest = rest.strip_prefix(grapheme.symbol)?;
        if grapheme.symbol == " " && left_width > 0 {
            let right_width = cells - left_width - 1;
            if (1..=width).contains(&right_width) {
                split = Some((offset, left_width.max(right_width)));
                // Once the prefix must clip, this is the longest fitting suffix.
                // When both rows fit, prefer the tier separator or a later word break.
                if left_width > width || rest.starts_with("· ") {
                    break;
                }
            }
        }
        left_width += usize::from(grapheme.symbol.cell_width());
    }
    split
}
