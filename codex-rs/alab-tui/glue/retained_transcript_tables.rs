// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! One whole-App view table retains actual cell-slice and activity-array aliases.
//! Cell identities and pinned layouts belong to the shared native graphs.
use super::identity::ActivityIdData;
use super::identity::AnchorData;
use super::identity::CaptureRefs;
use super::identity::KeyData;
use super::identity::PositionData;
use super::*;
use crate::history_cell::alab_retained_cells::Capture as Cells;
use crate::history_cell::alab_retained_cells::CellRef;
use crate::history_cell::alab_retained_cells::Restored as RestoredCells;
use crate::transcript_view::text::alab_retained_layout::Capture as Layouts;
use crate::transcript_view::text::alab_retained_layout::LayoutRef;
use crate::transcript_view::text::alab_retained_layout::Restored as RestoredLayouts;
use crate::transcript_view::turn_details::alab_retained_turns::GroupCapture;
use crate::transcript_view::turn_details::alab_retained_turns::GroupRef;
use crate::transcript_view::turn_details::alab_retained_turns::GroupRestored;
use crate::transcript_view::turn_details::alab_retained_turns::Groups;
use codex_alab_tui::retained_arc_graph::Capture as Arcs;
use codex_alab_tui::retained_arc_graph::Id;
use codex_alab_tui::retained_arc_graph::Restored as RestoredArcs;
use serde::Deserialize;
use serde::Serialize;
use std::io;
const MAX_ARRAYS: usize = 1_000_000;
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(transparent)]
pub(in crate::transcript_view) struct SliceRef(Id);
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(transparent)]
pub(in crate::transcript_view) struct ActivityRef(Id);
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(transparent)]
pub(in crate::transcript_view) struct TextRef(Id);
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    schema: u8,
    slices: Vec<Vec<CellRef>>,
    activities: Vec<Vec<ActivityIdData>>,
    groups: Groups,
}

pub(crate) struct Capture<'a> {
    identity: CaptureRefs<'a>,
    layouts: &'a mut Layouts,
    slices: Arcs<[Arc<dyn HistoryCell>]>,
    slice_data: Vec<Vec<CellRef>>,
    activities: Arcs<[String]>,
    activity_data: Vec<Vec<ActivityIdData>>,
    groups: GroupCapture,
}
impl<'a> Capture<'a> {
    /// All widget, queue and line bodies borrow these same two native graphs.
    /// All borrowed editors still belong to the caller's one enclosing encode.
    pub(crate) fn with_native<T>(
        &mut self,
        capture: impl FnOnce(
            &mut Cells,
            &mut crate::terminal_hyperlinks::alab_retained_text::Capture,
        ) -> io::Result<T>,
    ) -> io::Result<T> {
        capture(self.identity.native_graph(), self.layouts.lines())
    }
    pub(crate) fn root(&mut self, actual: &Arc<dyn HistoryCell>) -> io::Result<CellRef> {
        self.identity.intern(actual)
    }
    pub(crate) fn new(cells: &'a mut Cells, layouts: &'a mut Layouts) -> Self {
        Self {
            identity: CaptureRefs::new(cells),
            layouts,
            slices: Arcs::new(MAX_ARRAYS),
            slice_data: Vec::new(),
            activities: Arcs::new(MAX_ARRAYS),
            activity_data: Vec::new(),
            groups: GroupCapture::default(),
        }
    }
    pub(crate) fn register(&mut self, cells: &[Arc<dyn HistoryCell>]) -> io::Result<()> {
        self.identity.register(cells)
    }
    pub(in crate::transcript_view) fn weak(
        &self,
        actual: &std::sync::Weak<dyn HistoryCell>,
    ) -> io::Result<Option<CellRef>> {
        self.identity.weak(actual)
    }
    pub(in crate::transcript_view) fn line(
        &mut self,
        actual: &crate::terminal_hyperlinks::HyperlinkLine,
    ) -> io::Result<crate::terminal_hyperlinks::alab_retained_text::HyperlinkData> {
        self.layouts.lines().line(actual)
    }
    pub(in crate::transcript_view) fn text(&mut self, actual: &Arc<str>) -> io::Result<TextRef> {
        self.layouts.lines().string(actual).map(TextRef)
    }
    pub(in crate::transcript_view) fn identity(&self) -> &CaptureRefs<'_> {
        &self.identity
    }
    pub(in crate::transcript_view) fn activity_id(&self, actual: &str) -> ActivityIdData {
        self.identity.activity(actual)
    }
    pub(in crate::transcript_view) fn group(
        &mut self,
        actual: &Arc<crate::transcript_view::turn_details::ToolGroup>,
    ) -> io::Result<GroupRef> {
        self.groups.group(actual, &self.identity)
    }
    pub(in crate::transcript_view) fn dynamic_cell(
        &mut self,
        actual: &crate::history_cell::DynamicToolCallCell,
    ) -> io::Result<crate::history_cell::alab_retained_cells::DynamicData> {
        self.identity.dynamic_cell(actual)
    }
    pub(in crate::transcript_view) fn key(&self, key: EntryKey) -> io::Result<KeyData> {
        self.identity.key(key)
    }
    pub(in crate::transcript_view) fn anchor(&self, anchor: Anchor) -> io::Result<AnchorData> {
        self.identity.anchor(anchor)
    }
    pub(in crate::transcript_view) fn position(
        &self,
        position: Position,
    ) -> io::Result<PositionData> {
        self.identity.position(position)
    }
    pub(in crate::transcript_view) fn layout(
        &mut self,
        actual: &Arc<TextLayout>,
    ) -> io::Result<LayoutRef> {
        self.layouts.intern(actual)
    }
    pub(in crate::transcript_view) fn slice(
        &mut self,
        actual: &Arc<[Arc<dyn HistoryCell>]>,
    ) -> io::Result<SliceRef> {
        // Resolve fallible children before publishing the array identity. A
        // refused child must not leave an interned slice without its payload.
        let children = actual
            .iter()
            .map(|cell| self.identity.intern(cell))
            .collect::<io::Result<_>>()?;
        let (id, fresh) = self.slices.intern(actual)?;
        if fresh {
            if id.0 != self.slice_data.len() {
                return Err(std::io::Error::other("unfinished actual native cell slice"));
            }
            self.slice_data.push(children);
        }
        Ok(SliceRef(id))
    }
    pub(in crate::transcript_view) fn activity(
        &mut self,
        actual: &Arc<[String]>,
    ) -> io::Result<ActivityRef> {
        let (id, fresh) = self.activities.intern(actual)?;
        if fresh {
            if id.0 != self.activity_data.len() {
                return Err(std::io::Error::other(
                    "unfinished actual native activity array",
                ));
            }
            self.activity_data.push(
                actual
                    .iter()
                    .map(|value| self.identity.activity(value))
                    .collect(),
            );
        }
        Ok(ActivityRef(id))
    }
    pub(crate) fn export(&self) -> Data {
        Data {
            schema: 2,
            slices: self.slice_data.clone(),
            activities: self
                .activity_data
                .iter()
                .map(|values| {
                    values
                        .iter()
                        .map(|value| value.canonical(&self.identity))
                        .collect()
                })
                .collect(),
            groups: self.groups.export(&self.identity),
        }
    }
}
pub(crate) struct Restore<'a> {
    pub(in crate::transcript_view) cells: &'a RestoredCells,
    pub(in crate::transcript_view) layouts: &'a RestoredLayouts,
    slices: RestoredArcs<[Arc<dyn HistoryCell>]>,
    activities: RestoredArcs<[String]>,
    groups: GroupRestored,
}
impl<'a> Restore<'a> {
    pub(crate) fn cell(&self, id: CellRef) -> io::Result<Arc<dyn HistoryCell>> {
        self.cells.cell(id)
    }
    pub(crate) fn new(
        data: Data,
        cells: &'a RestoredCells,
        layouts: &'a RestoredLayouts,
    ) -> io::Result<Self> {
        if data.schema != 2 || data.slices.len() > MAX_ARRAYS || data.activities.len() > MAX_ARRAYS
        {
            return Err(std::io::Error::other("native view table schema/capacity"));
        }
        let slices = data
            .slices
            .into_iter()
            .map(|slice| {
                slice
                    .into_iter()
                    .map(|id| cells.cell(id))
                    .collect::<io::Result<Vec<_>>>()
                    .map(Arc::from)
            })
            .collect::<io::Result<_>>()?;
        let activities = data
            .activities
            .into_iter()
            .map(|values| {
                values
                    .into_iter()
                    .map(|value| value.restore(cells))
                    .collect::<io::Result<Vec<_>>>()
                    .map(Arc::from)
            })
            .collect::<io::Result<_>>()?;
        Ok(Self {
            cells,
            layouts,
            slices: RestoredArcs::new(slices),
            activities: RestoredArcs::new(activities),
            groups: GroupRestored::new(data.groups, cells)?,
        })
    }
    pub(in crate::transcript_view) fn text(&self, reference: TextRef) -> io::Result<Arc<str>> {
        self.layouts.text(reference.0)
    }
    pub(in crate::transcript_view) fn group(
        &self,
        reference: GroupRef,
    ) -> io::Result<Arc<crate::transcript_view::turn_details::ToolGroup>> {
        self.groups.group(reference)
    }
    pub(in crate::transcript_view) fn slice(
        &self,
        id: SliceRef,
    ) -> io::Result<Arc<[Arc<dyn HistoryCell>]>> {
        self.slices.get(id.0)
    }
    pub(in crate::transcript_view) fn activity(
        &self,
        id: ActivityRef,
    ) -> io::Result<Arc<[String]>> {
        self.activities.get(id.0)
    }
}

#[cfg(test)]
#[path = "retained_transcript_table_text_tests.rs"]
mod text_controls;
