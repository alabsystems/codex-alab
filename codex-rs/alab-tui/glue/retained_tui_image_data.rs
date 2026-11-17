// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original real Tui image transport observations +SAME shared raster aliases.
//! No image file is opened on decode and no terminal output is regenerated.
use super::owner::Book;
use super::owner::Phase;
use super::owner::PlacementKey;
use super::owner::Record;
use super::*;
use crate::terminal_hyperlinks::alab_retained_text as native;
use codex_alab_tui::retained_arc_graph::Id;
use codex_alab_tui::upgrade_exec_custody::images::FileRef;
use codex_alab_tui::upgrade_exec_custody::images::Offer;
use codex_alab_tui::upgrade_exec_custody::images::OriginalFile;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Mutex;
const MAX: usize = 1024;
fn required<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(d)
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Row {
    key: PlacementKey,
    image: Id,
    #[serde(deserialize_with = "required")]
    file: Option<FileRef>,
    phase: u8,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Data {
    schema: u8,
    rows: Vec<Row>,
    current: Vec<usize>,
}
pub(crate) struct Captured {
    offer: Offer,
    data: Data,
}
impl Captured {
    pub(crate) fn offer(&self) -> &Offer {
        &self.offer
    }
    pub(crate) fn data(&self) -> &Data {
        &self.data
    }
}
struct Snapshot {
    key: PlacementKey,
    image: Arc<TranscriptImage>,
    file: Option<Arc<OriginalFile>>,
    phase: Phase,
}
impl Book {
    pub(crate) fn capture(
        tui: &crate::tui::Tui,
        shared: &mut native::Capture,
    ) -> io::Result<Captured> {
        tui.alab_image_owner.snapshot_ready()?;
        let held = tui.alab_image_owner.records()?;
        let mut snapshots = Vec::with_capacity(held.len());
        for record in held {
            let record = record.lock().map_err(native::invalid)?;
            snapshots.push(Snapshot {
                key: record.key,
                image: record.image.clone(),
                file: record.file.clone(),
                phase: record.phase,
            });
        }
        if snapshots.len() > MAX || tui.retained_images.len() > MAX {
            return Err(native::invalid("Tui image owner census capacity"));
        }
        let files = snapshots
            .iter()
            .filter_map(|row| row.file.clone())
            .collect::<Vec<_>>();
        let offer = Offer::prepare(&files)?;
        let first = tui.terminal.alab_image_first_id();
        let current = tui
            .retained_images
            .iter()
            .enumerate()
            .map(|(index, image)| {
                if image.file.is_some() {
                    return Err(native::invalid(
                        "native image file was not retained before publication",
                    ));
                }
                let id = first
                    .checked_add(u32::try_from(index).map_err(native::invalid)?)
                    .ok_or_else(|| native::invalid("Tui image id overflow"))?;
                let key = PlacementKey {
                    id,
                    area: (
                        image.area.x,
                        image.area.y,
                        image.area.width,
                        image.area.height,
                    ),
                    first_row: image.first_row,
                };
                snapshots
                    .iter()
                    .rposition(|row| row.key == key && Arc::ptr_eq(&row.image, &image.image))
                    .ok_or_else(|| {
                        native::invalid("native placement absent from actual file/raster census")
                    })
            })
            .collect::<io::Result<Vec<_>>>()?;
        let rows = snapshots
            .into_iter()
            .map(|row| {
                validate(&row.image, row.key)?;
                Ok(Row {
                    key: row.key,
                    image: shared.image(&row.image)?,
                    file: row
                        .file
                        .as_ref()
                        .map(|file| offer.file_ref(file))
                        .transpose()?,
                    phase: match row.phase {
                        Phase::Started => 0,
                        Phase::Unavailable => 1,
                        Phase::NotVisible => 2,
                        Phase::Queued => 3,
                        Phase::Flushed => 4,
                    },
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Captured {
            offer,
            data: Data {
                schema: 1,
                rows,
                current,
            },
        })
    }
}
pub(crate) struct Prepared {
    records: Vec<Arc<Mutex<Record>>>,
    placements: Vec<ImagePlacement>,
    keys: Vec<PlacementKey>,
}
impl Data {
    fn prepare_data(
        self,
        shared: &native::Restored,
        mut file: impl FnMut(FileRef) -> io::Result<Arc<OriginalFile>>,
    ) -> io::Result<Prepared> {
        if self.schema != 1
            || self.rows.len() > MAX
            || self.current.len() > MAX
            || self.current.iter().any(|i| *i >= self.rows.len())
        {
            return Err(native::invalid("Tui image census schema/ref/capacity"));
        }
        let records = self
            .rows
            .into_iter()
            .map(|row| {
                let image = shared.image(row.image)?;
                validate(&image, row.key)?;
                let phase = match row.phase {
                    0 | 3 => {
                        return Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "original image transport remains incomplete; real claims stay owned",
                        ));
                    }
                    1 => Phase::Unavailable,
                    2 => Phase::NotVisible,
                    4 => Phase::Flushed,
                    _ => {
                        return Err(native::invalid(
                            "unknown actual Tui image publication phase",
                        ));
                    }
                };
                let file = row.file.map(&mut file).transpose()?;
                if file.is_some() && image.protocol != crate::pets::ImageProtocol::KittyLocalFile {
                    return Err(native::invalid(
                        "unlink owner belongs to another native transport",
                    ));
                }
                if file.is_some()
                    && matches!(
                        phase,
                        Phase::Started | Phase::Unavailable | Phase::NotVisible
                    )
                {
                    return Err(native::invalid(
                        "unlink owner lacks its actual prepared payload observation",
                    ));
                }
                Ok(Arc::new(Mutex::new(Record {
                    key: row.key,
                    image,
                    file,
                    phase,
                    completed: true,
                })))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let mut seen = std::collections::HashSet::new();
        let mut keys = Vec::new();
        let placements = self
            .current
            .into_iter()
            .map(|index| {
                if !seen.insert(index) {
                    return Err(native::invalid(
                        "duplicate original current placement reference",
                    ));
                }
                let row = records[index].lock().map_err(native::invalid)?;
                keys.push(row.key);
                let (x, y, width, height) = row.key.area;
                Ok(ImagePlacement {
                    image: row.image.clone(),
                    area: Rect::new(x, y, width, height),
                    first_row: row.key.first_row,
                    file: None,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Prepared {
            records,
            placements,
            keys,
        })
    }
    pub(crate) fn prepare(
        self,
        shared: &native::Restored,
        renderer: &crate::retained_image_renderer::Binding,
    ) -> io::Result<Prepared> {
        renderer.check()?;
        let files = renderer.files()?;
        self.prepare_data(shared, |id| files.file(id))
    }
}
impl Prepared {
    /// The actual whole-Tui assembler must install original terminal+placement
    /// resources synchronously before first image paint. A decoder supplies no
    /// install closure. Existing live image publication cannot be overwritten.
    pub(crate) fn install_with<T>(
        self,
        tui: &mut crate::tui::Tui,
        renderer: &crate::retained_image_renderer::Binding,
        install: impl FnOnce(&mut crate::tui::Tui, &[ImagePlacement]) -> io::Result<T>,
    ) -> io::Result<T> {
        renderer.check()?;
        if !renderer.belongs_to(tui) || !tui.alab_image_owner.records()?.is_empty() {
            return Err(native::invalid(
                "image assembler target already has actual image publications",
            ));
        }
        let first = tui.terminal.alab_image_first_id();
        for (index, image) in self.placements.iter().enumerate() {
            let expected = first
                .checked_add(u32::try_from(index).map_err(native::invalid)?)
                .ok_or_else(|| native::invalid("image id overflow"))?;
            let key = self.keys[index];
            if key.id != expected
                || key.area
                    != (
                        image.area.x,
                        image.area.y,
                        image.area.width,
                        image.area.height,
                    )
                || key.first_row != image.first_row
            {
                return Err(native::invalid(
                    "actual renderer image placement generation differs",
                ));
            }
        }
        let result = install(tui, &self.placements)?;
        if tui.retained_images != self.placements {
            return Err(native::invalid(
                "original assembler did not install exact native image aliases",
            ));
        }
        let retired = {
            let mut state = tui.alab_image_owner.0.lock().map_err(native::invalid)?;
            std::mem::replace(&mut state.records, self.records)
        };
        drop(retired);
        let records = tui.alab_image_owner.records()?;
        let files = records
            .iter()
            .map(|record| {
                record
                    .lock()
                    .map(|record| record.file.clone())
                    .map_err(native::invalid)
            })
            .collect::<io::Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        renderer.files()?.finish_scene_handoff(&files)?;
        renderer.check()?;
        Ok(result)
    }
}
fn validate(image: &TranscriptImage, key: PlacementKey) -> io::Result<()> {
    let first = u32::try_from(key.first_row).map_err(native::invalid)?;
    let bottom = first
        .checked_add(u32::from(key.area.3))
        .ok_or_else(|| native::invalid("retained image crop row overflow"))?;
    let mul = if image.protocol == crate::pets::ImageProtocol::Sixel {
        image.cell_height
    } else {
        image.pixels.height()
    };
    if image.rows == 0 || first.checked_mul(mul).is_none() || bottom.checked_mul(mul).is_none() {
        return Err(native::invalid(
            "retained image crop multiplication overflow",
        ));
    }
    Ok(())
}
