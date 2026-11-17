// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original transport-file owners are pinned BEFORE native terminal publication.
//! Completion here is original local flush DATA, never a UI/Native ACK.
use crate::pets::KittyPlacement;
use crate::transcript_image::ImagePlacement;
use crate::transcript_image::TranscriptImage;
use ratatui::layout::Rect;
use std::io::Write;
use std::io::{self};
use std::sync::Arc;
use tempfile::NamedTempFile;

#[cfg(unix)]
mod owner {
    use super::*;
    use codex_alab_tui::frontend_control::EnteredRetainedFrontend;
    use codex_alab_tui::upgrade_exec_custody::images::ClaimedFiles;
    use codex_alab_tui::upgrade_exec_custody::images::FileBindings;
    use codex_alab_tui::upgrade_exec_custody::images::OriginalFile;
    use std::cell::Cell;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Mutex;
    const MAX_RECORDS: usize = 1024;
    fn interrupted() -> io::Error {
        io::Error::other("actual Tui image owner interrupted")
    }
    #[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    pub(crate) struct PlacementKey {
        pub(crate) id: u32,
        pub(crate) area: (u16, u16, u16, u16),
        pub(crate) first_row: usize,
    }
    impl PlacementKey {
        fn new(id: u32, area: Rect, first_row: usize) -> Self {
            Self {
                id,
                area: (area.x, area.y, area.width, area.height),
                first_row,
            }
        }
    }
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Phase {
        Started,
        Unavailable,
        NotVisible,
        Queued,
        Flushed,
    }
    pub(crate) struct Record {
        pub(crate) key: PlacementKey,
        pub(crate) image: Arc<TranscriptImage>,
        pub(crate) file: Option<Arc<OriginalFile>>,
        pub(crate) phase: Phase,
        pub(crate) completed: bool,
    }
    #[derive(Default)]
    pub(super) struct State {
        pub(super) records: Vec<Arc<Mutex<Record>>>,
        files: Option<FileBindings>,
    }
    /// Embedded on the actual Tui; no serialized constructor or pathname owner.
    #[derive(Clone, Default)]
    pub(crate) struct Book(pub(super) Arc<Mutex<State>>);
    type PlacementCensus = Vec<(PlacementKey, Arc<TranscriptImage>)>;
    struct Work {
        book: Book,
        current: RefCell<Option<Arc<Mutex<Record>>>>,
        staged: RefCell<Vec<Arc<Mutex<Record>>>>,
        commits: RefCell<Vec<Box<dyn FnOnce() -> io::Result<()>>>>,
        deleted: RefCell<std::collections::HashMap<u32, usize>>,
        repainted: RefCell<Vec<(PlacementKey, Arc<TranscriptImage>, usize)>>,
        writes: RefCell<std::collections::HashMap<usize, usize>>,
        order: Cell<usize>,
        committed: Cell<bool>,
        census: RefCell<Option<PlacementCensus>>,
    }
    thread_local! {static ACTIVE:RefCell<Vec<Rc<Work>>>=const {RefCell::new(Vec::new())};}
    pub(crate) struct Frame {
        work: Rc<Work>,
        nested: bool,
    }
    impl Book {
        pub(crate) fn snapshot_ready(&self) -> io::Result<()> {
            if active().is_some_and(|work| self.same_owner(&work.book)) {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "actual outer image frame is still running",
                ));
            }
            for record in self.records()? {
                if !record.lock().map_err(|_| interrupted())?.completed {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "original image transport write has no completed outer frame",
                    ));
                }
            }
            Ok(())
        }
        pub(crate) fn same_owner(&self, other: &Self) -> bool {
            Arc::ptr_eq(&self.0, &other.0)
        }
        pub(crate) fn bind_files(
            &self,
            entry: Arc<EnteredRetainedFrontend>,
            actual: &Arc<ClaimedFiles>,
        ) -> io::Result<()> {
            let mut state = self.0.lock().map_err(|_| interrupted())?;
            if state.files.is_none() {
                state.files = Some(FileBindings::from_entered(entry));
            }
            actual.install_into(state.files.as_mut().ok_or_else(interrupted)?)
        }

        pub(crate) fn begin(&self) -> io::Result<Frame> {
            if let Some(work) = active().filter(|work| self.same_owner(&work.book)) {
                return Ok(Frame { work, nested: true });
            }
            let work = Rc::new(Work {
                book: self.clone(),
                current: RefCell::new(None),
                staged: RefCell::new(Vec::new()),
                commits: RefCell::new(Vec::new()),
                deleted: RefCell::new(std::collections::HashMap::new()),
                repainted: RefCell::new(Vec::new()),
                writes: RefCell::new(std::collections::HashMap::new()),
                order: Cell::new(0),
                committed: Cell::new(false),
                census: RefCell::new(None),
            });
            ACTIVE.with(|active| active.borrow_mut().push(work.clone()));
            Ok(Frame {
                work,
                nested: false,
            })
        }
        pub(crate) fn records(&self) -> io::Result<Vec<Arc<Mutex<Record>>>> {
            Ok(self.0.lock().map_err(|_| interrupted())?.records.clone())
        }
    }
    impl Frame {
        pub(crate) fn finish(self) -> io::Result<()> {
            // A native clear inside the original outer sync_update shares its
            // real Work; only that outer successful writer may commit custody.
            if self.nested {
                return Ok(());
            }
            let census = self.work.census.borrow_mut().take().ok_or_else(|| {
                io::Error::other("actual terminal did not publish an image census")
            })?;
            let retired = {
                let mut state = self.work.book.0.lock().map_err(|_| interrupted())?;
                let mut keep = Vec::new();
                // Newest native publication wins for an exact actual placement;
                // equal pixels in a distinct Arc never collapse a resource.
                for (key, image) in census {
                    if let Some(record) = state.records.iter().rev().find(|record| {
                        record
                            .lock()
                            .map(|record| record.key == key && Arc::ptr_eq(&record.image, &image))
                            .unwrap_or(false)
                    }) {
                        keep.push(record.clone());
                    } else {
                        return Err(io::Error::other(
                            "native image placement lacks its original transport observation",
                        ));
                    }
                }
                let deleted = self.work.deleted.borrow();
                let repainted = self.work.repainted.borrow();
                let staged = self.work.staged.borrow();
                let writes = self.work.writes.borrow();
                for prior in &state.records {
                    if keep.iter().any(|actual| Arc::ptr_eq(actual, prior)) {
                        continue;
                    }
                    let old = prior.lock().map_err(|_| interrupted())?;
                    let old_order = writes
                        .get(&(Arc::as_ptr(prior) as usize))
                        .copied()
                        .unwrap_or(0);
                    let kitty = old.image.protocol != crate::pets::ImageProtocol::Sixel;
                    let erased = (kitty
                        && deleted
                            .get(&old.key.id)
                            .is_some_and(|order| *order > old_order))
                        || repainted.iter().any(|(key, image, order)| {
                            *key == old.key && Arc::ptr_eq(image, &old.image) && *order > old_order
                        });
                    // Sixel has no protocol image ID. A new synthetic index,
                    // including a Kitty image at that index, cannot erase it.
                    let replaced = kitty
                        && staged.iter().any(|new| {
                            !Arc::ptr_eq(new, prior)
                                && writes
                                    .get(&(Arc::as_ptr(new) as usize))
                                    .is_some_and(|order| *order > old_order)
                                && new
                                    .lock()
                                    .map(|new| {
                                        new.image.protocol != crate::pets::ImageProtocol::Sixel
                                            && new.key.id == old.key.id
                                            && new.phase == Phase::Queued
                                    })
                                    .unwrap_or(false)
                        });
                    // A failed original native vector replacement may lose an
                    // old ID from its desired census. Absence is NOT erasure.
                    // Retire actual published resources only after the exact
                    // original delete/replacement bytes completed this frame.
                    if matches!(old.phase, Phase::Queued | Phase::Flushed) && !erased && !replaced {
                        keep.push(prior.clone());
                    }
                }
                std::mem::replace(&mut state.records, keep)
            };
            for record in self.work.staged.borrow().iter() {
                let mut record = record.lock().map_err(|_| interrupted())?;
                if record.phase == Phase::Queued {
                    record.phase = Phase::Flushed;
                }
                record.completed = true;
            }
            // This is reached only after original outer sync_update success.
            // A failed/partial/unwound frame never executes these callbacks.
            let commits = self.work.commits.replace(Vec::new());
            for commit in commits {
                commit()?;
            }
            self.work.committed.set(true);
            drop(retired); // actual unlink owners drop outside the book lock.
            Ok(())
        }
    }
    impl Drop for Frame {
        fn drop(&mut self) {
            if self.nested {
                return;
            }
            if !self.work.committed.get() {
                // An accepted delete whose enclosing synchronized write failed
                // makes its prior image transport uncertain too. Keep its exact
                // file owner; do not serialize a completed physical cut.
                if let Ok(state) = self.work.book.0.lock() {
                    let deleted = self.work.deleted.borrow();
                    let repainted = self.work.repainted.borrow();
                    let writes = self.work.writes.borrow();
                    for row in &state.records {
                        if let Ok(mut row_data) = row.lock() {
                            let order = writes
                                .get(&(Arc::as_ptr(row) as usize))
                                .copied()
                                .unwrap_or(0);
                            if (row_data.image.protocol != crate::pets::ImageProtocol::Sixel
                                && deleted
                                    .get(&row_data.key.id)
                                    .is_some_and(|deleted| *deleted > order))
                                || repainted.iter().any(|(key, image, repaint)| {
                                    *key == row_data.key
                                        && Arc::ptr_eq(image, &row_data.image)
                                        && *repaint > order
                                })
                            {
                                row_data.completed = false;
                            }
                        }
                    }
                }
            }
            ACTIVE.with(|active| {
                let mut active = active.borrow_mut();
                if active
                    .last()
                    .is_some_and(|work| Rc::ptr_eq(work, &self.work))
                {
                    active.pop();
                }
            });
            // Dropping without successful outer-frame finish retires no book
            // record: partial queued output keeps every actual file Tui-owned.
        }
    }
    fn next_order(work: &Work) -> usize {
        let next = work.order.get().saturating_add(1);
        work.order.set(next);
        next
    }
    pub(crate) fn deleted(id: u32) {
        if let Some(work) = active() {
            let order = next_order(&work);
            work.deleted.borrow_mut().insert(id, order);
        }
    }
    pub(crate) fn viewport_repaint_erases(placement: &ImagePlacement, id: u32, repaint: Rect) {
        // invalidate_viewport marks ONLY this actual buffer. A moved/shrunk
        // viewport supplies no erasure fact for pixels outside its repaint.
        if placement.image.protocol != crate::pets::ImageProtocol::Sixel
            || placement.area.is_empty()
            || placement.image.cell_width == 0
        {
            return;
        }
        // Original Sixel.draw crops vertically, but writes the full raster
        // width even when a changed layout reserves fewer native columns.
        let Ok(columns) = u16::try_from(
            placement
                .image
                .pixels
                .width()
                .div_ceil(placement.image.cell_width),
        ) else {
            return;
        };
        let width = placement.area.width.max(columns);
        if placement.area.x.checked_add(width).is_none()
            || placement
                .area
                .y
                .checked_add(placement.area.height)
                .is_none()
        {
            return;
        }
        let footprint = Rect::new(
            placement.area.x,
            placement.area.y,
            width,
            placement.area.height,
        );
        if footprint.intersection(repaint) != footprint {
            return;
        }
        if let Some(work) = active() {
            let order = next_order(&work);
            work.repainted.borrow_mut().push((
                PlacementKey::new(id, placement.area, placement.first_row),
                placement.image.clone(),
                order,
            ));
        }
    }
    pub(crate) fn defer(commit: Box<dyn FnOnce() -> io::Result<()>>) -> bool {
        let Some(work) = active() else {
            return false;
        };
        let Ok(mut commits) = work.commits.try_borrow_mut() else {
            return false;
        };
        commits.push(commit);
        true
    }
    fn active() -> Option<Rc<Work>> {
        ACTIVE.with(|active| active.borrow().last().cloned())
    }
    pub(crate) struct Placement {
        work: Option<Rc<Work>>,
        prior: Option<Arc<Mutex<Record>>>,
    }
    pub(crate) fn placement(
        image: &Arc<TranscriptImage>,
        area: Rect,
        first_row: usize,
        id: u32,
    ) -> io::Result<Placement> {
        let Some(work) = active() else {
            return Ok(Placement {
                work: None,
                prior: None,
            });
        };
        let record = Arc::new(Mutex::new(Record {
            key: PlacementKey::new(id, area, first_row),
            image: image.clone(),
            file: None,
            phase: Phase::Started,
            completed: false,
        }));
        {
            let mut state = work.book.0.lock().map_err(|_| interrupted())?;
            if state.records.len() >= MAX_RECORDS {
                return Err(io::Error::other("actual image transport record capacity"));
            }
            state.records.push(record.clone());
        }
        work.staged.borrow_mut().push(record.clone());
        let prior = work.current.replace(Some(record));
        Ok(Placement {
            work: Some(work),
            prior,
        })
    }
    impl Drop for Placement {
        fn drop(&mut self) {
            if let Some(work) = &self.work {
                work.current.replace(self.prior.take());
            }
        }
    }
    fn current() -> Option<Arc<Mutex<Record>>> {
        active().and_then(|work| work.current.borrow().clone())
    }
    pub(crate) fn retain(file: Option<NamedTempFile>) -> io::Result<Option<NamedTempFile>> {
        let Some(record) = current() else {
            return Ok(file);
        };
        if let Some(work) = active() {
            let order = next_order(&work);
            work.writes
                .borrow_mut()
                .insert(Arc::as_ptr(&record) as usize, order);
        }
        let mut record = record.lock().map_err(|_| interrupted())?;
        // The SAME original object enters actual Tui custody before ANY command
        // bytes mentioning the link are written, including failure/unwind paths.
        record.file = file.map(OriginalFile::retain_native);
        record.phase = Phase::Queued;
        Ok(None)
    }
    pub(crate) fn note(phase: Phase) -> io::Result<()> {
        if let Some(record) = current() {
            record.lock().map_err(|_| interrupted())?.phase = phase;
        }
        Ok(())
    }
    pub(crate) fn flushed(images: &[ImagePlacement], first: u32) -> io::Result<()> {
        let Some(work) = active() else {
            return Ok(());
        };
        let census = images
            .iter()
            .enumerate()
            .map(|(index, image)| {
                let id = first
                    .checked_add(u32::try_from(index).map_err(io::Error::other)?)
                    .ok_or_else(|| io::Error::other("image placement id overflow"))?;
                Ok((
                    PlacementKey::new(id, image.area, image.first_row),
                    image.image.clone(),
                ))
            })
            .collect::<io::Result<Vec<_>>>()?;
        // This census is only the actual original Backend::flush observation.
        // Phase completion waits for the enclosing synchronized write in finish.
        work.census.replace(Some(census));
        Ok(())
    }
}
#[cfg(unix)]
pub(crate) use owner::Book;
#[cfg(unix)]
pub(crate) use owner::Frame;
#[cfg(unix)]
pub(crate) use owner::Phase;
#[cfg(unix)]
pub(crate) use owner::PlacementKey;
#[cfg(unix)]
pub(crate) use owner::Record;

pub(crate) fn draw_owned(
    image: &Arc<TranscriptImage>,
    writer: &mut impl Write,
    area: Rect,
    first_row: usize,
    placement: KittyPlacement,
    id: u32,
) -> anyhow::Result<Option<NamedTempFile>> {
    #[cfg(unix)]
    let _actual = owner::placement(image, area, first_row, id)?;
    #[cfg(not(unix))]
    let _ = id;
    image.draw(writer, area, first_row, placement)
}
pub(crate) fn retain_before_write(
    file: Option<NamedTempFile>,
) -> io::Result<Option<NamedTempFile>> {
    #[cfg(unix)]
    {
        owner::retain(file)
    }
    #[cfg(not(unix))]
    {
        Ok(file)
    }
}
pub(crate) fn unavailable() -> io::Result<()> {
    #[cfg(unix)]
    owner::note(Phase::Unavailable)?;
    Ok(())
}
pub(crate) fn not_visible() -> io::Result<()> {
    #[cfg(unix)]
    owner::note(Phase::NotVisible)?;
    Ok(())
}
pub(crate) fn flushed(images: &[ImagePlacement], first: u32) -> io::Result<()> {
    #[cfg(unix)]
    owner::flushed(images, first)?;
    #[cfg(not(unix))]
    let _ = (images, first);
    Ok(())
}

/// Original native clear, retaining actual file slots until real backend success.
pub(crate) fn clear(tui: &mut crate::tui::Tui) -> io::Result<()> {
    #[cfg(unix)]
    let frame = tui.alab_image_owner.begin()?;
    if tui.retained_images.is_empty() {
        return Ok(());
    }
    tui.terminal.clear_frame_images(&mut tui.retained_images)?;
    #[cfg(unix)]
    frame.finish()?;
    Ok(())
}

#[cfg(unix)]
#[path = "retained_tui_image_data.rs"]
mod data;
#[cfg(unix)]
pub(crate) use data::Captured;
#[cfg(unix)]
pub(crate) use data::Data;
#[cfg(unix)]
pub(crate) use data::Prepared;
#[cfg(all(unix, test))]
#[path = "retained_tui_images_tests.rs"]
mod tests;

#[cfg(unix)]
pub(crate) fn defer_resource_commit(commit: impl FnOnce() -> io::Result<()> + 'static) -> bool {
    owner::defer(Box::new(commit))
}

/// Called only AFTER original native writer accepted this exact delete command.
/// It remains provisional until the SAME outer synchronized frame completes.
pub(crate) fn deleted(id: u32) {
    #[cfg(unix)]
    owner::deleted(id);
    #[cfg(not(unix))]
    let _ = id;
}

/// The original Sixel removal branch invalidates its whole viewport before
/// repainting. This records only that provisional native action: the same
/// Backend::flush and successful outer writer must still precede retirement.
pub(crate) fn viewport_repaint_erases(placement: &ImagePlacement, id: u32, repaint: Rect) {
    #[cfg(unix)]
    owner::viewport_repaint_erases(placement, id, repaint);
    #[cfg(not(unix))]
    let _ = (placement, id, repaint);
}
