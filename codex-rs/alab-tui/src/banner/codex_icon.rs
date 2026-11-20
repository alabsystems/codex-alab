// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! OpenAI's circular Codex prompt mark. The native tile rasterizes the `CODEX` path in
//! `tui/src/empty_state_animation/paths.rs` (upstream commit 5c5308fc9a). The cell
//! version samples the same ring and uses type for the two small strokes at its center.

use super::Banner;
use ratatui::text::Span;

pub(super) const WIDTH: usize = 10;
pub(super) const HEIGHT: usize = 5;

const ROWS: [&str; HEIGHT] = [
    "  ⢀⣠⣤⣤⣄⡀  ",
    " ⣰⠏    ⠹⣆ ",
    " ⣿  ❯_  ⣿ ",
    " ⠹⣆    ⣰⠏ ",
    "  ⠈⠙⠛⠛⠋⠁  ",
];

// The same ring sampled at twelve by twelve Braille dots for a narrow split.
const COMPACT_ROWS: [&str; super::artwork::COMPACT_MARK_HEIGHT] = ["⢠⠖⠋⠙⠲⡄", "⣏ ❯_ ⣹", "⠘⠦⣄⣠⠴⠃"];

pub(super) fn compact_row(banner: &Banner, index: usize) -> Span<'static> {
    let Some(fallback) = COMPACT_ROWS.get(index) else {
        return Span::raw(" ".repeat(super::artwork::COMPACT_MARK_WIDTH));
    };
    super::artwork::native_compact_mark(banner, index).unwrap_or_else(|| Span::raw(*fallback))
}

pub(super) fn row(banner: &Banner, index: usize) -> Span<'static> {
    let Some(fallback) = ROWS.get(index) else {
        return Span::raw(" ".repeat(WIDTH));
    };
    super::artwork::native_mark(banner, index).unwrap_or_else(|| Span::raw(*fallback))
}
