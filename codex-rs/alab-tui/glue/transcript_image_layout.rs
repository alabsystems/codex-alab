// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use ratatui::layout::Rect;

use super::TextLayout;
use crate::history_cell::HistoryCell;
use crate::history_cell::UserHistoryCell;
use crate::pets::ImageProtocol;
use crate::transcript_image::ImagePlacement;
use crate::transcript_image::TranscriptImage;

pub(super) fn enabled_for(cell: &dyn HistoryCell) -> bool {
    !cell.as_any().is::<UserHistoryCell>() || codex_alab_tui::interface::active()
}

pub(super) fn load(
    cell: &dyn HistoryCell,
    width: u16,
    protocol: ImageProtocol,
) -> Vec<Arc<TranscriptImage>> {
    if let Some(user) = cell.as_any().downcast_ref::<UserHistoryCell>() {
        return crate::transcript_image::user_images(user, width, protocol);
    }
    cell.image_path()
        .and_then(|path| TranscriptImage::load(path.as_path(), width, protocol))
        .map(Arc::new)
        .into_iter()
        .collect()
}

/// All images in a layout use the same protocol. Probe physical cell size once for Sixel.
pub(super) fn visible(
    images: &[Arc<TranscriptImage>],
    width: u16,
) -> impl Iterator<Item = &Arc<TranscriptImage>> {
    let cell_size = match images.first().map(|image| image.protocol) {
        Some(ImageProtocol::Sixel) => crate::transcript_image::terminal_cell_size(),
        _ => None,
    };
    images.iter().filter(move |image| {
        image.columns <= width.saturating_sub(2)
            && (image.protocol != ImageProtocol::Sixel
                || cell_size == Some((image.cell_width, image.cell_height)))
    })
}

/// Intersect each image's layout rows with the visible slice of its history cell.
pub(super) fn append_placements(
    placements: &mut Vec<ImagePlacement>,
    layout: &TextLayout,
    first_row: usize,
    area: Rect,
) {
    let visible_end = first_row + usize::from(area.height);
    let mut image_start = layout.text_row_count();
    for image in layout.visible_images() {
        if image_start >= visible_end {
            break;
        }
        let image_end = image_start + usize::from(image.rows);
        let start = image_start.max(first_row);
        let end = image_end.min(visible_end);
        if start < end {
            placements.push(ImagePlacement {
                image: Arc::clone(image),
                area: Rect::new(
                    area.x + area.width.saturating_sub(image.columns) / 2,
                    area.y + (start - first_row) as u16,
                    image.columns,
                    (end - start) as u16,
                ),
                first_row: start - image_start,
                file: None,
            });
        }
        image_start = image_end;
    }
}
