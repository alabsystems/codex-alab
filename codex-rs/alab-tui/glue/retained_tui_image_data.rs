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
fn require_file_alias(
    placement: Option<&ImageFile>,
    actual: Option<&Arc<OriginalFile>>,
) -> io::Result<()> {
    match (placement, actual) {
        (None, None) => Ok(()),
        (Some(placement), Some(actual)) if placement.same_actual(actual) => Ok(()),
        _ => Err(native::invalid(
            "native placement differs from its original image-file owner",
        )),
    }
}
fn require_installed_aliases(
    actual: &[ImagePlacement],
    expected: &[ImagePlacement],
) -> io::Result<()> {
    if actual != expected
        || actual.iter().zip(expected).any(|(actual, expected)| {
            match (actual.file.as_ref(), expected.file.as_ref()) {
                (None, None) => false,
                (Some(actual), Some(expected)) => !actual.same_alias(expected),
                _ => true,
            }
        })
    {
        return Err(native::invalid(
            "original assembler did not install exact native image aliases",
        ));
    }
    Ok(())
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
        let first = tui.terminal.alab_image_first_id();
        let current = tui
            .retained_images
            .iter()
            .enumerate()
            .map(|(index, image)| {
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
                let index = snapshots
                    .iter()
                    .rposition(|row| row.key == key && Arc::ptr_eq(&row.image, &image.image))
                    .ok_or_else(|| {
                        native::invalid("native placement absent from actual file/raster census")
                    })?;
                require_file_alias(image.file.as_ref(), snapshots[index].file.as_ref())?;
                Ok(index)
            })
            .collect::<io::Result<Vec<_>>>()?;
        let offer = Offer::prepare(&files)?;
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
                    file: row
                        .file
                        .as_ref()
                        .map(|actual| ImageFile::from_actual(actual.clone()))
                        .transpose()?,
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
/// The caller stores this owner before invoking the original installer. Every
/// callback/postcondition failure remains owned, including a successful result
/// whose later resource handoff failed. This is not a whole-scene ACK.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InstallState {
    Prepared,
    Refused,
    Unknown,
    Installed,
}

pub(crate) struct Installation<T> {
    // Dispose both callback products while every image custody owner is still
    // alive: io::Error may itself own an original error/resource destructor.
    result: Option<T>,
    error: Option<io::Error>,
    prepared: Prepared,
    state: InstallState,
    before_book: Option<Book>,
    before_placements: Vec<ImagePlacement>,
    current_book: Option<Book>,
    current_placements: Vec<ImagePlacement>,
    retired_records: Vec<Arc<Mutex<Record>>>,
    files: Vec<Arc<OriginalFile>>,
    claimed: Option<Arc<codex_alab_tui::upgrade_exec_custody::images::ClaimedFiles>>,
}

pub(crate) struct Refused<T>(Installation<T>);
pub(crate) struct Unknown<T>(Installation<T>);
pub(crate) struct Installed<T>(Installation<T>);
pub(crate) enum InstallOutcome<T> {
    Prepared(Installation<T>),
    Refused(Refused<T>),
    Unknown(Unknown<T>),
    Installed(Installed<T>),
}

impl<T> Refused<T> {
    pub(crate) fn error(&self) -> Option<&io::Error> {
        self.0.error.as_ref()
    }
    /// Only a callback-free refusal has this transition. The original callback
    /// remains in the caller's Option and the prepared owner remains unchanged.
    pub(crate) fn retry(mut self) -> Installation<T> {
        self.0.state = InstallState::Prepared;
        self.0
    }
}
impl<T> Unknown<T> {
    pub(crate) fn error(&self) -> Option<&io::Error> {
        self.0.error.as_ref()
    }
    pub(crate) fn result(&self) -> Option<&T> {
        self.0.result.as_ref()
    }
    // Deliberately no into_prepared/retry: the original callback may have run.
}
impl<T> Installed<T> {
    pub(crate) fn result(&self) -> Option<&T> {
        self.0.result.as_ref()
    }
    // Keep this complete owner with the enclosing scene through its commit.
}

fn retain_placements(actual: &[ImagePlacement]) -> Vec<ImagePlacement> {
    actual
        .iter()
        .map(|placement| ImagePlacement {
            image: placement.image.clone(),
            area: placement.area,
            first_row: placement.first_row,
            file: placement.file.clone(),
        })
        .collect()
}
fn record_files(records: &[Arc<Mutex<Record>>]) -> io::Result<Vec<Arc<OriginalFile>>> {
    records
        .iter()
        .map(|record| {
            record
                .lock()
                .map(|record| record.file.clone())
                .map_err(native::invalid)
        })
        .collect::<io::Result<Vec<_>>>()
        .map(|files| files.into_iter().flatten().collect())
}
fn publish_records(
    tui: &mut crate::tui::Tui,
    records: &[Arc<Mutex<Record>>],
) -> io::Result<Vec<Arc<Mutex<Record>>>> {
    let mut state = tui.alab_image_owner.0.lock().map_err(native::invalid)?;
    Ok(std::mem::replace(&mut state.records, records.to_vec()))
}

type Claimed = Arc<codex_alab_tui::upgrade_exec_custody::images::ClaimedFiles>;
/// Private lower resource seam. Production always supplies the actual Binding;
/// tests can check local custody without constructing an Entered/Native grant.
struct InstallChecks<'a> {
    authorize: &'a mut dyn FnMut(&crate::tui::Tui) -> io::Result<Option<Claimed>>,
    handoff: &'a mut dyn FnMut(&[Arc<OriginalFile>]) -> io::Result<()>,
    recheck: &'a mut dyn FnMut(&crate::tui::Tui) -> io::Result<()>,
    invalidate: &'a mut dyn FnMut(),
}
struct InstallAttempt<'a, 'b, T> {
    owner: &'a mut Installation<T>,
    tui: &'a mut crate::tui::Tui,
    invalidate: &'b mut dyn FnMut(),
}
impl<T> Drop for InstallAttempt<'_, '_, T> {
    fn drop(&mut self) {
        // Also runs if the original callback unwinds. The caller-stored owner
        // still retains Prepared; this keeps the actual target/placement owners.
        self.owner.current_book = Some(self.tui.alab_image_owner.clone());
        self.owner.current_placements = retain_placements(&self.tui.retained_images);
        if self.owner.state != InstallState::Installed {
            (self.invalidate)();
        }
    }
}

impl Prepared {
    pub(crate) fn begin_install<T>(self) -> Installation<T> {
        Installation {
            prepared: self,
            state: InstallState::Prepared,
            error: None,
            result: None,
            before_book: None,
            before_placements: Vec::new(),
            current_book: None,
            current_placements: Vec::new(),
            retired_records: Vec::new(),
            files: Vec::new(),
            claimed: None,
        }
    }
}
impl<T> Installation<T> {
    pub(crate) fn state(&self) -> InstallState {
        self.state
    }
    pub(crate) fn into_outcome(self) -> InstallOutcome<T> {
        match self.state {
            InstallState::Prepared => InstallOutcome::Prepared(self),
            InstallState::Refused => InstallOutcome::Refused(Refused(self)),
            InstallState::Unknown => InstallOutcome::Unknown(Unknown(self)),
            InstallState::Installed => InstallOutcome::Installed(Installed(self)),
        }
    }

    /// Original terminal/placement installer, synchronously before first paint.
    /// Borrow the caller's Option: preflight cannot consume its FnOnce captures.
    pub(crate) fn install_with<F>(
        &mut self,
        tui: &mut crate::tui::Tui,
        renderer: &crate::retained_image_renderer::Binding,
        install: &mut Option<F>,
    ) -> InstallState
    where
        F: FnOnce(&mut crate::tui::Tui, &[ImagePlacement]) -> io::Result<T>,
    {
        let mut authorize = |tui: &crate::tui::Tui| {
            renderer.check()?;
            if !renderer.belongs_to(tui) {
                return Err(native::invalid(
                    "image assembler changed actual Tui binding",
                ));
            }
            Ok(Some(renderer.files()?.clone()))
        };
        let mut handoff =
            |files: &[Arc<OriginalFile>]| renderer.files()?.finish_scene_handoff(files);
        let mut recheck = |tui: &crate::tui::Tui| {
            renderer.check()?;
            if !renderer.belongs_to(tui) {
                return Err(native::invalid(
                    "image assembler changed actual Tui binding",
                ));
            }
            Ok(())
        };
        let watch = renderer.restore_watch();
        let mut invalidate = || watch.invalidate();
        self.install_bound(
            tui,
            install,
            &mut InstallChecks {
                authorize: &mut authorize,
                handoff: &mut handoff,
                recheck: &mut recheck,
                invalidate: &mut invalidate,
            },
        )
    }

    fn install_bound<F>(
        &mut self,
        tui: &mut crate::tui::Tui,
        install: &mut Option<F>,
        checks: &mut InstallChecks<'_>,
    ) -> InstallState
    where
        F: FnOnce(&mut crate::tui::Tui, &[ImagePlacement]) -> io::Result<T>,
    {
        if self.state != InstallState::Prepared {
            return self.state;
        }
        self.error = None;
        let preflight = (|| {
            if install.is_none() {
                return Err(native::invalid("original image installer is absent"));
            }
            let claimed = (checks.authorize)(tui)?;
            if !tui.alab_image_owner.records()?.is_empty() {
                return Err(native::invalid(
                    "image assembler target already has actual image publications",
                ));
            }
            if self.prepared.keys.len() != self.prepared.placements.len() {
                return Err(native::invalid("image assembler placement census differs"));
            }
            let first = tui.terminal.alab_image_first_id();
            for (index, image) in self.prepared.placements.iter().enumerate() {
                let expected = first
                    .checked_add(u32::try_from(index).map_err(native::invalid)?)
                    .ok_or_else(|| native::invalid("image id overflow"))?;
                let key = self.prepared.keys[index];
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
            Ok(claimed)
        })();
        self.claimed = match preflight {
            Ok(claimed) => claimed,
            Err(error) => {
                self.error = Some(error);
                self.state = InstallState::Refused;
                return self.state;
            }
        };
        self.before_book = Some(tui.alab_image_owner.clone());
        self.before_placements = retain_placements(&tui.retained_images);
        // Set before taking FnOnce or entering original code. Unwind remains
        // Unknown in the caller's stored owner, never callback-free Refused.
        self.state = InstallState::Unknown;
        let attempt = InstallAttempt {
            owner: self,
            tui,
            invalidate: &mut *checks.invalidate,
        };
        let Some(install) = install.take() else {
            attempt.owner.error = Some(native::invalid("original image installer disappeared"));
            return attempt.owner.state;
        };
        match install(attempt.tui, &attempt.owner.prepared.placements) {
            Ok(result) => attempt.owner.result = Some(result),
            Err(error) => {
                attempt.owner.error = Some(error);
                return attempt.owner.state;
            }
        }
        let postcondition = (|| {
            if attempt
                .owner
                .before_book
                .as_ref()
                .is_none_or(|book| !book.same_owner(&attempt.tui.alab_image_owner))
            {
                return Err(native::invalid("image installer replaced its actual book"));
            }
            require_installed_aliases(
                &attempt.tui.retained_images,
                &attempt.owner.prepared.placements,
            )?;
            attempt.owner.retired_records =
                publish_records(attempt.tui, &attempt.owner.prepared.records)?;
            attempt.owner.files = record_files(&attempt.tui.alab_image_owner.records()?)?;
            (checks.handoff)(&attempt.owner.files)?;
            (checks.recheck)(attempt.tui)
        })();
        match postcondition {
            Ok(()) => attempt.owner.state = InstallState::Installed,
            Err(error) => attempt.owner.error = Some(error),
        }
        attempt.owner.state
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

#[cfg(test)]
#[path = "retained_tui_image_alias_tests.rs"]
mod alias_tests;

#[cfg(test)]
#[path = "retained_tui_image_install_tests.rs"]
mod install_tests;
