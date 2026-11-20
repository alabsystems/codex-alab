// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;
use std::sync::Arc;

use base64::Engine;
use image::RgbaImage;

use super::TranscriptImage;
use super::terminal_cell_size;
use crate::history_cell::UserHistoryCell;
use crate::pets::ImageProtocol;

fn inline_thumbnail(url: &str, max_width: u32, max_height: u32) -> Option<RgbaImage> {
    // A replay can contain the bytes submitted to the model in place of the paste's local path.
    // Only inline image data is a pixel source. Ordinary URLs remain text labels.
    let (header, encoded) = url.split_once(',')?;
    let mut parts = header.split(';');
    let mime = parts.next()?;
    if !mime.get(..11)?.eq_ignore_ascii_case("data:image/")
        || !parts.any(|part| part.eq_ignore_ascii_case("base64"))
        || encoded.len() > base64::encoded_len(32 * 1024 * 1024, true)?
    {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(32 * 1024 * 1024);
    let image = crate::image_decode::decode_oriented(reader, limits).ok()?;
    Some(
        image
            .thumbnail(max_width.min(image.width()), max_height.min(image.height()))
            .to_rgba8(),
    )
}

/// Follow the same remote-then-local order as the numbered prompt attachments.
pub(crate) fn user_image_pixels(
    user: &UserHistoryCell,
    max_width: u32,
    max_height: u32,
) -> impl Iterator<Item = RgbaImage> + '_ {
    user.remote_image_urls
        .iter()
        .filter_map(move |url| inline_thumbnail(url, max_width, max_height))
        .chain(
            user.local_image_paths
                .iter()
                .filter_map(move |path| TranscriptImage::load_pixels(path, max_width, max_height)),
        )
}

pub(crate) fn user_images(
    user: &UserHistoryCell,
    width: u16,
    protocol: ImageProtocol,
) -> Vec<Arc<TranscriptImage>> {
    let columns = width.saturating_sub(2).min(24);
    if columns == 0 || (user.remote_image_urls.is_empty() && user.local_image_paths.is_empty()) {
        return Vec::new();
    }
    let (cell_width, cell_height) = match protocol {
        ImageProtocol::Kitty | ImageProtocol::KittyLocalFile => (8, 16),
        ImageProtocol::Sixel => match terminal_cell_size() {
            Some(size) => size,
            None => return Vec::new(),
        },
    };
    // Sixel encodes physical pixels. Keep its raster within the transcript encoder's budget.
    let pixel_width = (u32::from(columns) * cell_width).min(if protocol == ImageProtocol::Sixel {
        160
    } else {
        192
    });
    let pixel_height = (4 * cell_height).min(80);
    user_image_pixels(user, pixel_width, pixel_height)
        .map(|pixels| {
            Arc::new(TranscriptImage {
                protocol,
                columns: pixels.width().div_ceil(cell_width) as u16,
                rows: pixels.height().div_ceil(cell_height) as u16,
                pixels,
                cell_width,
                cell_height,
            })
        })
        .collect()
}
