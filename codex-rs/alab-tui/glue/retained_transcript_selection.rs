// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact native ranges staged in the one shared UI marker graph.
//! Prepared selections stay private until the same real worker binding validates
//! their historical request and owner identities. No copy is submitted or replayed.
use super::*;
use crate::clipboard_copy::worker::alab_retained_clipboard::ScopeCapture;
use crate::clipboard_copy::worker::alab_retained_clipboard::ScopeRef;
use crate::clipboard_copy::worker::alab_retained_clipboard::ScopeRestored;
use crate::transcript_view::alab_retained_view::identity::AnchorData;
use crate::transcript_view::alab_retained_view::identity::PositionData;
use crate::transcript_view::alab_retained_view::snapshots;
use crate::transcript_view::alab_retained_view::tables;
use serde::Deserialize;
use serde::Serialize;
use std::io;
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum UnitData {
    Character {},
    Word {},
    Line {},
}
impl UnitData {
    fn capture(value: SelectionUnit) -> Self {
        match value {
            SelectionUnit::Character => Self::Character {},
            SelectionUnit::Word => Self::Word {},
            SelectionUnit::Line => Self::Line {},
        }
    }
    fn restore(self) -> SelectionUnit {
        match self {
            Self::Character {} => SelectionUnit::Character,
            Self::Word {} => SelectionUnit::Word,
            Self::Line {} => SelectionUnit::Line,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingData {
    id: u64,
    start: AnchorData,
    end: AnchorData,
    position: PositionData,
    detailed: bool,
    follow: bool,
    clear_selection: bool,
    characters: u64,
}
impl PendingData {
    fn capture(value: &PendingCopy, graph: &tables::Capture<'_>) -> io::Result<Self> {
        Ok(Self {
            id: value.id,
            start: graph.anchor(value.start)?,
            end: graph.anchor(value.end)?,
            position: graph.position(value.position)?,
            detailed: value.detailed,
            follow: value.follow,
            clear_selection: value.clear_selection,
            characters: u64::try_from(value.characters).map_err(invalid)?,
        })
    }
    fn prepare(self, graph: &tables::Restore<'_>) -> io::Result<PendingCopy> {
        Ok(PendingCopy {
            id: self.id,
            start: self.start.restore(graph.cells)?,
            end: self.end.restore(graph.cells)?,
            position: self.position.restore(graph.cells)?,
            detailed: self.detailed,
            follow: self.follow,
            clear_selection: self.clear_selection,
            characters: usize::try_from(self.characters).map_err(invalid)?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::transcript_view) struct Data {
    schema: u8,
    #[serde(deserialize_with = "required")]
    pending_copy: Option<PendingData>,
    #[serde(deserialize_with = "required")]
    primary_owner: Option<ScopeRef>,
    snapshot: snapshots::Data,
    start: AnchorData,
    end: AnchorData,
    dragging: bool,
    moved: bool,
    moved_vertically: bool,
    pointer_origin_row: u16,
    resume_on_empty: bool,
    #[serde(deserialize_with = "required")]
    pointer: Option<(u16, u16)>,
    #[serde(deserialize_with = "required")]
    pressed_link: Option<String>,
    origin: (AnchorData, AnchorData),
    unit: UnitData,
    #[serde(deserialize_with = "required")]
    preferred_column: Option<u16>,
}
pub(in crate::transcript_view) struct Prepared {
    selection: Selection,
}
impl Prepared {
    pub(in crate::transcript_view) fn validate(
        &self,
        actual: &crate::clipboard_copy::worker::alab_retained_clipboard::BoundClipboard,
    ) -> io::Result<()> {
        actual.validate_selection(
            self.selection
                .pending_copy
                .as_ref()
                .map(|pending| pending.id),
            self.selection.primary_owner.as_ref(),
        )
    }
    pub(in crate::transcript_view) fn into_native(self) -> Selection {
        self.selection
    }
}

impl Data {
    pub(in crate::transcript_view) fn capture(
        actual: &Selection,
        graph: &mut tables::Capture<'_>,
        scopes: &mut ScopeCapture,
    ) -> io::Result<Self> {
        let snapshot = snapshots::Data::capture(&actual.snapshot, graph)?;
        Ok(Self {
            schema: 1,
            pending_copy: actual
                .pending_copy
                .as_ref()
                .map(|value| PendingData::capture(value, graph))
                .transpose()?,
            primary_owner: actual
                .primary_owner
                .as_ref()
                .map(|owner| scopes.strong(owner))
                .transpose()?,
            snapshot,
            start: graph.anchor(actual.start)?,
            end: graph.anchor(actual.end)?,
            dragging: actual.dragging,
            moved: actual.moved,
            moved_vertically: actual.moved_vertically,
            pointer_origin_row: actual.pointer_origin_row,
            resume_on_empty: actual.resume_on_empty,
            pointer: actual.pointer.map(|point| (point.x, point.y)),
            pressed_link: actual.pressed_link.clone(),
            origin: (
                graph.anchor(actual.origin.0)?,
                graph.anchor(actual.origin.1)?,
            ),
            unit: UnitData::capture(actual.unit),
            preferred_column: actual.preferred_column,
        })
    }
    pub(in crate::transcript_view) fn prepare(
        self,
        graph: &tables::Restore<'_>,
        scopes: &ScopeRestored,
    ) -> io::Result<Prepared> {
        if self.schema != 1 {
            return Err(invalid("native selection schema"));
        }
        Ok(Prepared {
            selection: Selection {
                pending_copy: self
                    .pending_copy
                    .map(|value| value.prepare(graph))
                    .transpose()?,
                primary_owner: self
                    .primary_owner
                    .map(|reference| scopes.strong(reference))
                    .transpose()?,
                snapshot: self.snapshot.restore(graph)?,
                start: self.start.restore(graph.cells)?,
                end: self.end.restore(graph.cells)?,
                dragging: self.dragging,
                moved: self.moved,
                moved_vertically: self.moved_vertically,
                pointer_origin_row: self.pointer_origin_row,
                resume_on_empty: self.resume_on_empty,
                pointer: self.pointer.map(|(x, y)| ScreenPosition::new(x, y)),
                pressed_link: self.pressed_link,
                origin: (
                    self.origin.0.restore(graph.cells)?,
                    self.origin.1.restore(graph.cells)?,
                ),
                unit: self.unit.restore(),
                preferred_column: self.preferred_column,
            },
        })
    }
}
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    decoder: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(decoder)
}

pub(in crate::transcript_view) fn validate_native(
    selection: &Selection,
    actual: &crate::clipboard_copy::worker::alab_retained_clipboard::BoundClipboard,
) -> io::Result<()> {
    actual.validate_selection(
        selection.pending_copy.as_ref().map(|pending| pending.id),
        selection.primary_owner.as_ref(),
    )
}
