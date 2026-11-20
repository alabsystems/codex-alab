// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Real native image.draw/write +same actual local file-book scheduling. These
//! lower controls do not instantiate Native/SDK/Entered or full Tui readiness.
use super::*;
use crate::pets::ImageProtocol;
use image::Rgba;
use image::RgbaImage;
struct Refuse {
    written: usize,
    limit: usize,
}
impl Write for Refuse {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.written >= self.limit {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "intentional actual terminal write refusal",
            ));
        }
        let take = bytes.len().min(self.limit - self.written);
        self.written += take;
        Ok(take)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn raster() -> Arc<TranscriptImage> {
    Arc::new(TranscriptImage {
        protocol: ImageProtocol::KittyLocalFile,
        columns: 1,
        rows: 1,
        pixels: RgbaImage::from_pixel(2, 2, Rgba([100, 20, 3, 255])),
        cell_width: 8,
        cell_height: 16,
    })
}
#[test]
fn original_unlink_owner_is_in_actual_tui_book_before_the_first_failing_terminal_write()
-> io::Result<()> {
    let book = Book::default();
    let frame = book.begin()?;
    let image = raster();
    let mut output = Refuse {
        written: 0,
        limit: 0,
    };
    let result = draw_owned(
        &image,
        &mut output,
        Rect::new(1, 1, 1, 1),
        0,
        KittyPlacement::Cursor(0xC100),
        0xC100,
    );
    drop(frame);
    assert!(result.is_err());
    assert!(book.snapshot_ready().is_err());
    assert_eq!(output.written, 0);
    let records = book.records()?;
    assert_eq!(records.len(), 1);
    let record = records[0]
        .lock()
        .map_err(|error| io::Error::other(error.to_string()))?;
    assert_eq!(record.phase, Phase::Queued);
    let file = record
        .file
        .as_ref()
        .ok_or_else(|| io::Error::other("actual original file owner lost on write error"))?;
    file.with_native(|original| {
        assert!(original.path().exists());
        assert!(original.as_file().metadata()?.len() > 0);
        Ok(())
    })?;
    Ok(())
}
#[test]
fn partial_original_terminal_write_keeps_same_file_even_when_observation_scope_is_dropped()
-> io::Result<()> {
    let book = Book::default();
    let frame = book.begin()?;
    let image = raster();
    let mut output = Refuse {
        written: 0,
        limit: 12,
    };
    let result = draw_owned(
        &image,
        &mut output,
        Rect::new(1, 1, 1, 1),
        0,
        KittyPlacement::Cursor(0xC100),
        0xC100,
    );
    let records = book.records()?;
    let file = records[0]
        .lock()
        .map_err(|error| io::Error::other(error.to_string()))?
        .file
        .clone()
        .ok_or_else(|| io::Error::other("actual original file owner missing"))?;
    let path = file.with_native(|original| Ok(original.path().to_path_buf()))?;
    drop(frame);
    drop(records);
    drop(file);
    assert!(result.is_err());
    assert!(book.snapshot_ready().is_err());
    assert!(output.written > 0);
    assert!(path.exists());
    drop(book);
    assert!(!path.exists());
    Ok(())
}
#[test]
fn real_completed_local_write_and_clear_retire_only_after_same_frame_finishes() -> io::Result<()> {
    let book = Book::default();
    let image = raster();
    let frame = book.begin()?;
    let mut output = Vec::new();
    let result = draw_owned(
        &image,
        &mut output,
        Rect::new(1, 1, 1, 1),
        0,
        KittyPlacement::Cursor(0xC100),
        0xC100,
    )
    .map_err(io::Error::other)?;
    let file = result.ok_or_else(|| io::Error::other("actual placement alias missing"))?;
    assert!(file.path().exists());
    let records = book.records()?;
    assert!(
        file.same_actual(
            records[0]
                .lock()
                .map_err(|error| io::Error::other(error.to_string()))?
                .file
                .as_ref()
                .ok_or_else(|| io::Error::other("actual original file missing"))?,
        )
    );
    drop(records);
    output.flush()?;
    let placement = ImagePlacement {
        image,
        area: Rect::new(1, 1, 1, 1),
        first_row: 0,
        file: Some(file),
    };
    flushed(&[placement], 0xC100)?;
    frame.finish()?;
    let records = book.records()?;
    let record = records[0]
        .lock()
        .map_err(|error| io::Error::other(error.to_string()))?;
    assert_eq!(record.phase, Phase::Flushed);
    let path = record
        .file
        .as_ref()
        .ok_or_else(|| io::Error::other("actual original file missing"))?
        .with_native(|file| Ok(file.path().to_path_buf()))?;
    drop(record);
    drop(records);
    let clearing = book.begin()?;
    output.write_all(crate::pets::kitty_delete_image(0xC100).as_bytes())?;
    deleted(0xC100);
    output.flush()?;
    flushed(&[], 0xC100)?;
    assert!(path.exists());
    clearing.finish()?;
    assert!(!path.exists());
    assert!(book.records()?.is_empty());
    Ok(())
}

#[test]
fn nested_original_clear_cannot_retire_files_before_the_same_outer_writer_commits() -> io::Result<()>
{
    let book = Book::default();
    let image = raster();
    let mut bytes = Vec::new();
    let initial = book.begin()?;
    let file = draw_owned(
        &image,
        &mut bytes,
        Rect::new(1, 1, 1, 1),
        0,
        KittyPlacement::Cursor(0xC100),
        0xC100,
    )
    .map_err(io::Error::other)?
    .ok_or_else(|| io::Error::other("actual placement alias missing"))?;
    bytes.flush()?;
    flushed(
        &[ImagePlacement {
            image,
            area: Rect::new(1, 1, 1, 1),
            first_row: 0,
            file: Some(file),
        }],
        0xC100,
    )?;
    initial.finish()?;
    let records = book.records()?;
    let path = records[0]
        .lock()
        .map_err(|error| io::Error::other(error.to_string()))?
        .file
        .as_ref()
        .ok_or_else(|| io::Error::other("actual file owner absent"))?
        .with_native(|file| Ok(file.path().to_path_buf()))?;
    drop(records);
    let failed_outer = book.begin()?;
    let nested = book.begin()?;
    bytes.write_all(crate::pets::kitty_delete_image(0xC100).as_bytes())?;
    deleted(0xC100);
    bytes.flush()?;
    flushed(&[], 0xC100)?;
    nested.finish()?;
    assert!(path.exists());
    drop(failed_outer);
    assert!(path.exists());
    // A subsequent native census by itself cannot infer the lost delete won.
    let absent = book.begin()?;
    bytes.flush()?;
    flushed(&[], 0xC100)?;
    absent.finish()?;
    assert!(path.exists());
    let outer = book.begin()?;
    let nested = book.begin()?;
    bytes.write_all(crate::pets::kitty_delete_image(0xC100).as_bytes())?;
    deleted(0xC100);
    bytes.flush()?;
    flushed(&[], 0xC100)?;
    nested.finish()?;
    assert!(path.exists());
    outer.finish()?;
    assert!(!path.exists());
    Ok(())
}
