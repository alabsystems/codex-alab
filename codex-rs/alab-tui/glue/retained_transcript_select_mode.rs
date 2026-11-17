// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original keyboard mark/search-origin DATA over the enclosing view's SAME native cell graph.
//! This does not create Selection, search/editor/clipboard state, a layout or resource permission.
use super::*;
use crate::transcript_view::alab_retained_view::identity::AnchorData;
use crate::transcript_view::alab_retained_view::tables::Capture;
use crate::transcript_view::alab_retained_view::tables::Restore;
use serde::Deserialize;
use serde::Serialize;
use std::io;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::transcript_view) struct Data {
    schema: u8,
    #[serde(deserialize_with = "required")]
    mark: Option<MarkData>,
    #[serde(deserialize_with = "required")]
    preferred_column: Option<u16>,
    #[serde(deserialize_with = "required")]
    search_origin: Option<OriginData>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MarkData {
    anchor: AnchorData,
    active: AnchorData,
    #[serde(deserialize_with = "required")]
    anchor_edges: Option<(AnchorData, AnchorData)>,
    #[serde(deserialize_with = "required")]
    active_edges: Option<(AnchorData, AnchorData)>,
    unit: UnitData,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OriginData {
    #[serde(deserialize_with = "required")]
    mark: Option<MarkData>,
    #[serde(deserialize_with = "required")]
    preferred_column: Option<u16>,
    start: AnchorData,
    end: AnchorData,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum UnitData {
    Character {},
    Word {},
    Line {},
}
impl UnitData {
    fn capture(actual: SelectionUnit) -> Self {
        match actual {
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
fn capture_edges(
    actual: Option<(Anchor, Anchor)>,
    graph: &Capture<'_>,
) -> io::Result<Option<(AnchorData, AnchorData)>> {
    actual
        .map(|(start, end)| -> io::Result<_> { Ok((graph.anchor(start)?, graph.anchor(end)?)) })
        .transpose()
}
fn restore_edges(
    data: Option<(AnchorData, AnchorData)>,
    graph: &Restore<'_>,
) -> io::Result<Option<(Anchor, Anchor)>> {
    data.map(|(start, end)| -> io::Result<_> {
        Ok((start.restore(graph.cells)?, end.restore(graph.cells)?))
    })
    .transpose()
}
impl MarkData {
    fn capture(actual: Mark, graph: &Capture<'_>) -> io::Result<Self> {
        Ok(Self {
            anchor: graph.anchor(actual.anchor)?,
            active: graph.anchor(actual.active)?,
            anchor_edges: capture_edges(actual.anchor_edges, graph)?,
            active_edges: capture_edges(actual.active_edges, graph)?,
            unit: UnitData::capture(actual.unit),
        })
    }
    fn restore(self, graph: &Restore<'_>) -> io::Result<Mark> {
        Ok(Mark {
            anchor: self.anchor.restore(graph.cells)?,
            active: self.active.restore(graph.cells)?,
            anchor_edges: restore_edges(self.anchor_edges, graph)?,
            active_edges: restore_edges(self.active_edges, graph)?,
            unit: self.unit.restore(),
        })
    }
}
impl OriginData {
    fn capture(actual: SelectSearchOrigin, graph: &Capture<'_>) -> io::Result<Self> {
        Ok(Self {
            mark: actual
                .mark
                .map(|mark| MarkData::capture(mark, graph))
                .transpose()?,
            preferred_column: actual.preferred_column,
            start: graph.anchor(actual.start)?,
            end: graph.anchor(actual.end)?,
        })
    }
    fn restore(self, graph: &Restore<'_>) -> io::Result<SelectSearchOrigin> {
        Ok(SelectSearchOrigin {
            mark: self.mark.map(|mark| mark.restore(graph)).transpose()?,
            preferred_column: self.preferred_column,
            start: self.start.restore(graph.cells)?,
            end: self.end.restore(graph.cells)?,
        })
    }
}
impl Data {
    /// Register actual current/held/parked cell owners in this SAME graph first.
    pub(in crate::transcript_view) fn capture(
        actual: &SelectMode,
        graph: &Capture<'_>,
    ) -> io::Result<Self> {
        Ok(Self {
            schema: 1,
            mark: actual
                .mark
                .map(|mark| MarkData::capture(mark, graph))
                .transpose()?,
            preferred_column: actual.preferred_column,
            search_origin: actual
                .search_origin
                .map(|origin| OriginData::capture(origin, graph))
                .transpose()?,
        })
    }
    pub(in crate::transcript_view) fn restore(self, graph: &Restore<'_>) -> io::Result<SelectMode> {
        if self.schema != 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "native select-mode schema",
            ));
        }
        Ok(SelectMode {
            mark: self.mark.map(|mark| mark.restore(graph)).transpose()?,
            preferred_column: self.preferred_column,
            search_origin: self
                .search_origin
                .map(|origin| origin.restore(graph))
                .transpose()?,
        })
    }
}
fn required<'de, D, T>(decoder: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(decoder)
}

#[cfg(test)]
#[path = "retained_transcript_select_mode_tests.rs"]
mod controls;
