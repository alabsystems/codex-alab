// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::DrawCommand;
use super::Terminal;
use ratatui::backend::Backend;
use std::io;
use std::io::Write;

/// Called after drawing and before the terminal commits the diff.
pub(super) fn prepare<B>(term: &mut Terminal<B>, changes: &[DrawCommand]) -> io::Result<()>
where
    B: Backend<Error = io::Error> + Write,
{
    let buffer = &term.buffers[term.current];
    term.alab_banner_art
        .prepare_visible(&mut term.backend, |full, registered| {
            if full {
                registered.with_tiles(buffer.content.iter().map(|cell| (cell.symbol(), cell.fg)))
            } else {
                registered.with_tiles(changes.iter().filter_map(|change| match change {
                    DrawCommand::Put { cell, .. } => Some((cell.symbol(), cell.fg)),
                    _ => None,
                }))
            }
        })
}
