// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual native Terminal Sixel paint/removal plus original Book frame custody.
//! These lower controls do not construct SDK/Native receipts or a full Tui.
use super::*;
use crate::retained_tui_images::Book;
use crate::transcript_image::ImagePlacement;
use crate::transcript_image::TranscriptImage;
use pretty_assertions::assert_eq;
use std::sync::Arc;
fn image() -> Arc<TranscriptImage> {
    Arc::new(TranscriptImage {
        protocol: crate::pets::ImageProtocol::Sixel,
        columns: 2,
        rows: 2,
        pixels: image::RgbaImage::from_pixel(4, 4, image::Rgba([7, 80, 130, 255])),
        cell_width: 2,
        cell_height: 2,
    })
}
fn native_paint(
    book: &Book,
    terminal: &mut Terminal<CaptureBackend>,
    retained: &mut Vec<ImagePlacement>,
) -> io::Result<()> {
    let size = terminal.last_known_screen_size;
    let original = image();
    let frame = book.begin()?;
    terminal.draw_with_size(size, retained, |frame| {
        frame.image_placements.push(ImagePlacement {
            image: original,
            area: Rect::new(1, 1, 2, 2),
            first_row: 0,
            file: None,
        })
    })?;
    frame.finish()?;
    let records = book.records()?;
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0]
            .lock()
            .map_err(|error| io::Error::other(error.to_string()))?
            .phase,
        crate::retained_tui_images::Phase::Flushed
    );
    assert!(!terminal.backend().output().is_empty());
    Ok(())
}
#[test]
fn actual_sixel_viewport_repaint_retires_raster_only_after_same_outer_write_commits()
-> io::Result<()> {
    let book = Book::default();
    let mut terminal = Terminal::with_options(CaptureBackend::new(8, 4))?;
    terminal.set_viewport_area(Rect::new(0, 0, 8, 4));
    let mut retained = Vec::new();
    native_paint(&book, &mut terminal, &mut retained)?;
    let frame = book.begin()?;
    let size = terminal.last_known_screen_size;
    terminal.draw_with_size(size, &mut retained, |_| {})?;
    assert!(retained.is_empty());
    assert_eq!(book.records()?.len(), 1);
    frame.finish()?;
    assert!(book.records()?.is_empty());
    book.snapshot_ready()?;
    Ok(())
}
#[test]
fn actual_sixel_repaint_keeps_same_raster_if_enclosing_writer_does_not_commit() -> io::Result<()> {
    let book = Book::default();
    let mut terminal = Terminal::with_options(CaptureBackend::new(8, 4))?;
    terminal.set_viewport_area(Rect::new(0, 0, 8, 4));
    let mut retained = Vec::new();
    native_paint(&book, &mut terminal, &mut retained)?;
    let before = book.records()?;
    let frame = book.begin()?;
    let size = terminal.last_known_screen_size;
    terminal.draw_with_size(size, &mut retained, |_| {})?;
    drop(frame);
    let after = book.records()?;
    assert_eq!(after.len(), 1);
    assert!(Arc::ptr_eq(&before[0], &after[0]));
    assert!(book.snapshot_ready().is_err());
    Ok(())
}

#[test]
fn actual_partial_moved_and_empty_viewport_repaint_keep_same_old_sixel_raster() -> io::Result<()> {
    for viewport in [
        Rect::new(0, 0, 2, 4),
        Rect::new(4, 0, 4, 4),
        Rect::new(0, 0, 0, 0),
    ] {
        let book = Book::default();
        let mut terminal = Terminal::with_options(CaptureBackend::new(8, 4))?;
        terminal.set_viewport_area(Rect::new(0, 0, 8, 4));
        let mut retained = Vec::new();
        native_paint(&book, &mut terminal, &mut retained)?;
        let old = book.records()?;
        terminal.set_viewport_area(viewport);
        let frame = book.begin()?;
        let size = terminal.last_known_screen_size;
        terminal.draw_with_size(size, &mut retained, |_| {})?;
        frame.finish()?;
        assert!(retained.is_empty());
        let actual = book.records()?;
        assert_eq!(actual.len(), 1);
        assert!(Arc::ptr_eq(&actual[0], &old[0]));
        book.snapshot_ready()?;
    }
    Ok(())
}
#[test]
fn new_actual_sixel_or_kitty_at_same_synthetic_index_cannot_erase_uncovered_sixel_raster()
-> io::Result<()> {
    for protocol in [
        crate::pets::ImageProtocol::Sixel,
        crate::pets::ImageProtocol::Kitty,
    ] {
        let book = Book::default();
        let mut terminal = Terminal::with_options(CaptureBackend::new(8, 4))?;
        terminal.set_viewport_area(Rect::new(0, 0, 8, 4));
        let mut retained = Vec::new();
        native_paint(&book, &mut terminal, &mut retained)?;
        let old = book.records()?;
        let moved = Rect::new(4, 0, 4, 4);
        terminal.set_viewport_area(moved);
        let original = Arc::new(TranscriptImage {
            protocol,
            columns: 1,
            rows: 1,
            pixels: image::RgbaImage::from_pixel(2, 2, image::Rgba([170, 20, 8, 255])),
            cell_width: 2,
            cell_height: 2,
        });
        let frame = book.begin()?;
        let size = terminal.last_known_screen_size;
        terminal.draw_with_size(size, &mut retained, |frame| {
            frame.image_placements.push(ImagePlacement {
                image: original,
                area: Rect::new(4, 0, 1, 1),
                first_row: 0,
                file: None,
            })
        })?;
        frame.finish()?;
        let actual = book.records()?;
        assert_eq!(actual.len(), 2);
        assert!(actual.iter().any(|row| Arc::ptr_eq(row, &old[0])));
        book.snapshot_ready()?;
    }
    Ok(())
}

#[test]
fn narrow_original_sixel_area_cannot_hide_full_raster_pixels_outside_repaint() -> io::Result<()> {
    let book = Book::default();
    let mut terminal = Terminal::with_options(CaptureBackend::new(8, 4))?;
    terminal.set_viewport_area(Rect::new(0, 0, 8, 4));
    let mut retained = Vec::new();
    let original = image();
    let frame = book.begin()?;
    let size = terminal.last_known_screen_size;
    // The actual native writer emits all four pixel columns (two terminal
    // columns), even though this real placement reserves only one column.
    terminal.draw_with_size(size, &mut retained, |frame| {
        frame.image_placements.push(ImagePlacement {
            image: original,
            area: Rect::new(1, 1, 1, 2),
            first_row: 0,
            file: None,
        })
    })?;
    frame.finish()?;
    let old = book.records()?;
    assert_eq!(old.len(), 1);
    assert_eq!(
        old[0]
            .lock()
            .map_err(|error| io::Error::other(error.to_string()))?
            .phase,
        crate::retained_tui_images::Phase::Flushed
    );
    terminal.set_viewport_area(Rect::new(0, 0, 2, 4));
    let frame = book.begin()?;
    terminal.draw_with_size(size, &mut retained, |_| {})?;
    frame.finish()?;
    let actual = book.records()?;
    assert_eq!(actual.len(), 1);
    assert!(Arc::ptr_eq(&actual[0], &old[0]));
    book.snapshot_ready()?;
    Ok(())
}
