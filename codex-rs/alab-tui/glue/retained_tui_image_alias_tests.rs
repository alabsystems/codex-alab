// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual local Tui placement/book aliases; no Native grant or frontend admission is fabricated.
use super::*;
use image::Rgba;
use image::RgbaImage;

fn drawn_tui() -> anyhow::Result<crate::tui::Tui> {
    drawn_tui_for(crate::pets::ImageProtocol::KittyLocalFile)
}

fn drawn_tui_for(protocol: crate::pets::ImageProtocol) -> anyhow::Result<crate::tui::Tui> {
    let image = Arc::new(TranscriptImage {
        protocol,
        columns: 1,
        rows: 1,
        pixels: RgbaImage::from_pixel(2, 2, Rgba([100, 20, 3, 255])),
        cell_width: 8,
        cell_height: 16,
    });
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.set_owned_screen(true)?;
    tui.draw(24, |frame| {
        frame.image_placements.push(ImagePlacement {
            image,
            area: Rect::new(2, 2, 1, 1),
            first_row: 0,
            file: None,
        });
    })?;
    Ok(tui)
}

#[tokio::test]
async fn whole_tui_placement_exposes_the_same_completed_original_file_owner() -> anyhow::Result<()>
{
    let mut tui = drawn_tui()?;
    assert_eq!(tui.retained_images.len(), 1);
    let records = tui.alab_image_owner.records()?;
    assert_eq!(records.len(), 1);
    let record = records[0]
        .lock()
        .map_err(|_| io::Error::other("image record interrupted"))?;
    assert_eq!(record.phase, Phase::Flushed);
    assert!(record.completed);
    let original = record
        .file
        .as_ref()
        .ok_or_else(|| io::Error::other("original file missing"))?;
    let placement = tui.retained_images[0]
        .file
        .as_ref()
        .ok_or_else(|| io::Error::other("placement file missing"))?;
    assert!(placement.same_actual(original));
    let path = original.with_native(|file| Ok(file.path().to_path_buf()))?;
    assert_eq!(placement.path(), path.as_path());
    assert!(placement.path().exists());
    drop(record);
    drop(records);
    let captured = Book::capture(&tui, &mut native::Capture::default())?;
    assert_eq!(captured.data.current, [0]);
    assert_eq!(captured.data.rows.len(), 1);
    assert!(captured.data.rows[0].file.is_some());
    assert_eq!(captured.data.rows[0].phase, 4);
    drop(captured);
    tui.draw(24, |_| {})?;
    assert!(tui.alab_image_owner.records()?.is_empty());
    assert!(!path.exists());
    tui.set_owned_screen(false)?;
    Ok(())
}

#[tokio::test]
async fn capture_rejects_missing_or_other_placement_owners_before_offering_files()
-> anyhow::Result<()> {
    let mut tui = drawn_tui()?;
    let original = tui.retained_images[0]
        .file
        .take()
        .ok_or_else(|| io::Error::other("placement file missing"))?;
    assert!(Book::capture(&tui, &mut native::Capture::default()).is_err());
    assert!(original.path().exists());
    let other = OriginalFile::retain_native(tempfile::NamedTempFile::new()?);
    let other_path = other.with_native(|file| Ok(file.path().to_path_buf()))?;
    tui.retained_images[0].file = Some(ImageFile::from_actual(other.clone())?);
    assert!(Book::capture(&tui, &mut native::Capture::default()).is_err());
    assert!(original.path().exists());
    assert!(other_path.exists());
    tui.retained_images[0].file = Some(original);
    drop(Book::capture(&tui, &mut native::Capture::default())?);
    tui.draw(24, |_| {})?;
    tui.set_owned_screen(false)?;
    drop(other);
    assert!(!other_path.exists());
    Ok(())
}

#[tokio::test]
async fn capture_rejects_a_file_alias_on_an_original_non_file_transport() -> anyhow::Result<()> {
    let mut tui = drawn_tui_for(crate::pets::ImageProtocol::Kitty)?;
    assert!(tui.retained_images[0].file.is_none());
    drop(Book::capture(&tui, &mut native::Capture::default())?);
    tui.retained_images[0].file = Some(ImageFile::from_native(tempfile::NamedTempFile::new()?)?);
    assert!(Book::capture(&tui, &mut native::Capture::default()).is_err());
    tui.retained_images[0].file = None;
    tui.draw(24, |_| {})?;
    tui.set_owned_screen(false)?;
    Ok(())
}

#[tokio::test]
async fn install_postcondition_rejects_equal_pixels_with_missing_or_other_file_owners()
-> anyhow::Result<()> {
    let mut tui = drawn_tui()?;
    let placement = &tui.retained_images[0];
    let expected = [ImagePlacement {
        image: placement.image.clone(),
        area: placement.area,
        first_row: placement.first_row,
        file: placement.file.clone(),
    }];
    let path = expected[0]
        .file
        .as_ref()
        .ok_or_else(|| io::Error::other("placement file missing"))?
        .path()
        .to_path_buf();
    require_installed_aliases(&tui.retained_images, &expected)?;
    tui.retained_images[0].file = None;
    assert!(tui.retained_images.as_slice() == expected.as_slice());
    assert!(require_installed_aliases(&tui.retained_images, &expected).is_err());
    let other = OriginalFile::retain_native(tempfile::NamedTempFile::new()?);
    tui.retained_images[0].file = Some(ImageFile::from_actual(other)?);
    assert!(tui.retained_images.as_slice() == expected.as_slice());
    assert!(require_installed_aliases(&tui.retained_images, &expected).is_err());
    tui.retained_images[0].file = expected[0].file.clone();
    require_installed_aliases(&tui.retained_images, &expected)?;
    let registration = [expected[0].registration()];
    assert!(require_installed_aliases(&tui.retained_images, &registration).is_err());
    tui.draw(24, |_| {})?;
    assert!(
        path.exists(),
        "the same original owner is still held by the expected alias"
    );
    drop(expected);
    assert!(!path.exists());
    tui.set_owned_screen(false)?;
    Ok(())
}

#[tokio::test]
async fn prepared_placement_reuses_the_supplied_original_resource_without_opening_a_file()
-> anyhow::Result<()> {
    let mut tui = drawn_tui()?;
    let original = tui.retained_images[0]
        .file
        .as_ref()
        .ok_or_else(|| io::Error::other("placement file missing"))?
        .actual
        .clone();
    let mut graph = native::Capture::default();
    let captured = Book::capture(&tui, &mut graph)?;
    let reference = captured.data.rows[0].file;
    let shared = native::Restored::new(graph.export()?)?;
    // Exercise decoder preparation with this actual existing owner. This is not
    // an Entered/Native grant or an invocation of the whole renderer installer.
    let prepared = captured.data.clone().prepare_data(&shared, |file| {
        assert_eq!(Some(file), reference);
        Ok(original.clone())
    })?;
    assert_eq!(prepared.placements.len(), 1);
    let placement = prepared.placements[0]
        .file
        .as_ref()
        .ok_or_else(|| io::Error::other("restored placement file missing"))?;
    assert!(placement.same_actual(&original));
    assert!(Arc::ptr_eq(
        prepared.records[0]
            .lock()
            .map_err(|_| io::Error::other("image record interrupted"))?
            .file
            .as_ref()
            .ok_or_else(|| io::Error::other("restored book file missing"))?,
        &original,
    ));
    let path = original.with_native(|file| Ok(file.path().to_path_buf()))?;
    assert_eq!(placement.path(), path.as_path());
    drop(prepared);
    drop(captured);
    drop(original);
    tui.draw(24, |_| {})?;
    assert!(!path.exists());
    tui.set_owned_screen(false)?;
    Ok(())
}
