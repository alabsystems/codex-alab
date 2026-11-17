// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual native cache/alias codec only; no Renderer/Entered/Native grant.
use super::*;
fn raster() -> Arc<TranscriptImage> {
    Arc::new(TranscriptImage {
        protocol: ImageProtocol::Kitty,
        columns: 1,
        rows: 1,
        pixels: RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255])),
        cell_width: 8,
        cell_height: 16,
    })
}
#[test]
fn exact_native_cache_keeps_unavailable_non_utf8_pre_epoch_and_same_raster_alias() -> io::Result<()>
{
    let image = raster();
    let missing = PathBuf::from(std::ffi::OsString::from_vec(
        b"/not-opened-on-decode/a\xff.png".to_vec(),
    ));
    let epoch = std::time::UNIX_EPOCH
        .checked_sub(std::time::Duration::new(7, 123))
        .ok_or_else(|| native::invalid("native pre-epoch test range"))?;
    let mut cache = HashMap::new();
    cache.insert(
        TileKey {
            path: missing.clone(),
            version: Some((37, epoch)),
        },
        Some(image.clone()),
    );
    cache.insert(
        TileKey {
            path: PathBuf::from("/not-opened-on-decode/failed.png"),
            version: None,
        },
        None,
    );
    let actual = ComposerImages {
        alab_retained: RenderState::default(),
        protocol: ImageProtocol::Kitty,
        cell_size: (8, 16),
        background: (10, 20, 30),
        cache: RefCell::new(cache),
        placements: RefCell::new(vec![ImagePlacement {
            image: image.clone(),
            area: Rect::new(1, 2, 1, 1),
            first_row: 0,
            file: None,
        }]),
    };
    let mut capture = native::Capture::default();
    let data = Data::capture(&actual, &mut capture)?;
    let shared = native::Restored::new(capture.export()?)?;
    let restored = data.restore_data(&shared)?;
    let cache = restored.cache.borrow();
    let positive = cache
        .get(&TileKey {
            path: missing,
            version: Some((37, epoch)),
        })
        .and_then(Option::as_ref)
        .ok_or_else(|| native::invalid("native positive cache lost"))?;
    assert!(Arc::ptr_eq(
        positive,
        &restored.placements.borrow()[0].image
    ));
    assert_eq!(positive.pixels, image.pixels);
    assert!(
        cache
            .get(&TileKey {
                path: PathBuf::from("/not-opened-on-decode/failed.png"),
                version: None
            })
            .is_some_and(Option::is_none)
    );
    assert_eq!(restored.cell_size, (8, 16));
    assert_eq!(restored.background, (10, 20, 30));
    Ok(())
}
#[test]
fn native_cache_duplicate_missing_observation_and_crop_overflow_refuse() -> io::Result<()> {
    let image = raster();
    let mut cache = HashMap::new();
    cache.insert(
        TileKey {
            path: PathBuf::from("/already-unavailable.png"),
            version: None,
        },
        None,
    );
    let actual = ComposerImages {
        alab_retained: RenderState::default(),
        protocol: ImageProtocol::Kitty,
        cell_size: (8, 16),
        background: (0, 0, 0),
        cache: RefCell::new(cache),
        placements: RefCell::default(),
    };
    let mut graph = native::Capture::default();
    let mut data = Data::capture(&actual, &mut graph)?;
    data.cache.push(data.cache[0].clone());
    let shared = native::Restored::new(graph.export()?)?;
    assert!(data.restore_data(&shared).is_err());
    let data = Data::capture(&actual, &mut native::Capture::default())?;
    let mut raw = serde_json::to_value(data).map_err(native::invalid)?;
    raw["cache"][0]
        .as_object_mut()
        .ok_or_else(|| native::invalid("test cache entry"))?
        .remove("image");
    assert!(serde_json::from_value::<Data>(raw).is_err());
    assert!(validate_placement(&image, Rect::new(0, 0, 1, 1), usize::MAX).is_err());
    Ok(())
}

#[test]
fn original_render_key_survives_failed_outer_frame_and_retires_only_after_real_write_commit()
-> io::Result<()> {
    use crate::retained_tui_images::Book;
    use crate::retained_tui_images::draw_owned;
    use crate::retained_tui_images::flushed;
    use std::io::Write;
    let dir = tempfile::tempdir()?;
    let existing = dir.path().join("existing.png");
    let unavailable = dir.path().join("unavailable.png");
    image::RgbaImage::from_pixel(2, 2, Rgba([2, 60, 170, 255]))
        .save(&existing)
        .map_err(native::invalid)?;
    let attachments = vec![
        LocalImageAttachment {
            placeholder: "[Image 1]".into(),
            path: existing.clone(),
        },
        LocalImageAttachment {
            placeholder: "[Image 2]".into(),
            path: unavailable.clone(),
        },
    ];
    let textarea = TextArea::new();
    let area = Rect::new(0, 0, 20, 3);
    let mut buffer = Buffer::empty(area);
    let source = ComposerImages {
        alab_retained: RenderState::default(),
        protocol: ImageProtocol::Kitty,
        cell_size: (8, 16),
        background: (0, 0, 0),
        cache: RefCell::default(),
        placements: RefCell::default(),
    };
    source.render(area, &mut buffer, &attachments, &textarea);
    assert_eq!(
        source
            .cache
            .borrow()
            .values()
            .filter(|image| image.is_some())
            .count(),
        1
    );
    let mut graph = native::Capture::default();
    let data = Data::capture(&source, &mut graph)?;
    let shared = native::Restored::new(graph.export()?)?;
    std::fs::remove_file(&existing)?;
    image::RgbaImage::from_pixel(2, 2, Rgba([200, 1, 1, 255]))
        .save(&unavailable)
        .map_err(native::invalid)?;
    let restored = data.restore_data(&shared)?;
    let book = Book::default();
    let failed = book.begin()?;
    restored.render(area, &mut buffer, &attachments, &textarea);
    drop(failed);
    let preserved = |cache: &HashMap<TileKey, Option<Arc<TranscriptImage>>>| {
        let positive = cache
            .iter()
            .find(|(key, _)| key.path == existing)
            .is_some_and(|(_, image)| image.is_some());
        let negative = cache
            .iter()
            .find(|(key, _)| key.path == unavailable)
            .is_some_and(|(_, image)| image.is_none());
        positive && negative
    };
    assert!(preserved(&restored.cache.borrow()));
    assert!(
        restored
            .alab_retained
            .state
            .lock()
            .map_err(native::invalid)?
            .replay
            .is_some()
    );
    let completed = book.begin()?;
    restored.render(area, &mut buffer, &attachments, &textarea);
    assert!(preserved(&restored.cache.borrow()));
    let placements = restored
        .placements
        .borrow()
        .iter()
        .map(ImagePlacement::registration)
        .collect::<Vec<_>>();
    let mut terminal_bytes = Vec::new();
    for (index, item) in placements.iter().enumerate() {
        let id = 0xC100 + u32::try_from(index).map_err(native::invalid)?;
        let _file = draw_owned(
            &item.image,
            &mut terminal_bytes,
            item.area,
            item.first_row,
            crate::pets::KittyPlacement::Cursor(id),
            id,
        )
        .map_err(native::invalid)?;
    }
    terminal_bytes.flush()?;
    flushed(&placements, 0xC100)?;
    completed.finish()?;
    assert!(
        restored
            .alab_retained
            .state
            .lock()
            .map_err(native::invalid)?
            .replay
            .is_none()
    );
    // Only a later genuine native render resumes its ordinary metadata policy.
    restored.render(area, &mut buffer, &attachments, &textarea);
    let cache = restored.cache.borrow();
    assert!(
        cache
            .iter()
            .find(|(key, _)| key.path == existing)
            .is_some_and(|(_, image)| image.is_none())
    );
    assert!(
        cache
            .iter()
            .find(|(key, _)| key.path == unavailable)
            .is_some_and(|(_, image)| image.is_some())
    );
    Ok(())
}
