// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native turn indexes and immutable ToolGroup aliases in one enclosing view graph.
//! Pending tools resolve through the SAME global dynamic-row table, not a copied view row.
use super::*;
use crate::history_cell::alab_retained_cells::DynamicData;
use crate::history_cell::alab_retained_cells::RestoreContext;
use crate::history_cell::alab_retained_cells::Restored as RestoredCells;
use crate::transcript_view::alab_retained_view::identity::ActivityIdData;
use crate::transcript_view::alab_retained_view::identity::CaptureRefs;
use crate::transcript_view::alab_retained_view::identity::KeyData;
use crate::transcript_view::alab_retained_view::tables::Capture;
use crate::transcript_view::alab_retained_view::tables::Restore;
use codex_alab_tui::retained_arc_graph::Capture as Arcs;
use codex_alab_tui::retained_arc_graph::Id;
use codex_alab_tui::retained_arc_graph::Restored as RestoredArcs;
use serde::Deserialize;
use serde::Serialize;
use std::io;
const MAX_GROUPS: usize = 1_000_000;
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
fn index(value: u64) -> io::Result<usize> {
    usize::try_from(value).map_err(invalid)
}
fn number(value: usize) -> io::Result<u64> {
    u64::try_from(value).map_err(invalid)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub(in crate::transcript_view) struct GroupRef(Id);
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GroupData {
    #[serde(deserialize_with = "required")]
    anchor: Option<u64>,
    ids: Vec<ActivityIdData>,
    calls: Vec<String>,
    explored: u64,
    ran: u64,
}
impl GroupData {
    fn capture(actual: &ToolGroup, refs: &CaptureRefs<'_>) -> io::Result<Self> {
        Ok(Self {
            anchor: actual.anchor.map(number).transpose()?,
            ids: actual
                .ids
                .iter()
                .map(|value| refs.activity(value))
                .collect(),
            calls: actual.calls.clone(),
            explored: number(actual.explored)?,
            ran: number(actual.ran)?,
        })
    }
    fn restore(self, cells: &RestoredCells) -> io::Result<Arc<ToolGroup>> {
        let explored = index(self.explored)?;
        let ran = index(self.ran)?;
        if explored
            .checked_add(ran)
            .is_none_or(|count| count > self.calls.len())
        {
            return Err(invalid("native tool counters exceed original calls"));
        }
        Ok(Arc::new(ToolGroup {
            anchor: self.anchor.map(index).transpose()?,
            ids: self
                .ids
                .into_iter()
                .map(|value| value.restore(cells))
                .collect::<io::Result<_>>()?,
            calls: self.calls,
            explored,
            ran,
        }))
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::transcript_view) struct Groups {
    entries: Vec<GroupData>,
}
pub(in crate::transcript_view) struct GroupCapture {
    identities: Arcs<ToolGroup>,
    entries: Vec<GroupData>,
}
impl Default for GroupCapture {
    fn default() -> Self {
        Self {
            identities: Arcs::new(MAX_GROUPS),
            entries: Vec::new(),
        }
    }
}
impl GroupCapture {
    pub(in crate::transcript_view) fn group(
        &mut self,
        actual: &Arc<ToolGroup>,
        refs: &CaptureRefs<'_>,
    ) -> io::Result<GroupRef> {
        // Prepare every fallible field before publishing a new Arc identity.
        let data = GroupData::capture(actual, refs)?;
        let (id, fresh) = self.identities.intern(actual)?;
        if fresh {
            if id.0 != self.entries.len() {
                return Err(invalid("unfinished native tool group"));
            }
            self.entries.push(data);
        }
        Ok(GroupRef(id))
    }
    pub(in crate::transcript_view) fn export(&self, refs: &CaptureRefs<'_>) -> Groups {
        Groups {
            entries: self
                .entries
                .iter()
                .cloned()
                .map(|mut group| {
                    group.ids = group.ids.iter().map(|id| id.canonical(refs)).collect();
                    group
                })
                .collect(),
        }
    }
}
pub(in crate::transcript_view) struct GroupRestored(RestoredArcs<ToolGroup>);
impl GroupRestored {
    pub(in crate::transcript_view) fn new(data: Groups, cells: &RestoredCells) -> io::Result<Self> {
        if data.entries.len() > MAX_GROUPS {
            return Err(invalid("native tool group capacity"));
        }
        Ok(Self(RestoredArcs::new(
            data.entries
                .into_iter()
                .map(|group| group.restore(cells))
                .collect::<io::Result<_>>()?,
        )))
    }
    pub(in crate::transcript_view) fn group(
        &self,
        reference: GroupRef,
    ) -> io::Result<Arc<ToolGroup>> {
        self.0.get(reference.0)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::transcript_view) enum RowData {
    Prompt { id: ActivityIdData, label: String },
    Tools { group: GroupRef },
    Hidden {},
}
impl RowData {
    pub(in crate::transcript_view) fn capture(
        actual: &TurnRow,
        graph: &mut Capture<'_>,
    ) -> io::Result<Self> {
        Ok(match actual {
            TurnRow::Prompt { id, label } => Self::Prompt {
                id: graph.activity_id(id),
                label: label.clone(),
            },
            TurnRow::Tools(group) => Self::Tools {
                group: graph.group(group)?,
            },
            TurnRow::Hidden => Self::Hidden {},
        })
    }
    pub(in crate::transcript_view) fn restore(self, graph: &Restore<'_>) -> io::Result<TurnRow> {
        Ok(match self {
            Self::Prompt { id, label } => TurnRow::Prompt {
                id: id.restore(graph.cells)?,
                label,
            },
            Self::Tools { group } => TurnRow::Tools(graph.group(group)?),
            Self::Hidden {} => TurnRow::Hidden,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryData {
    key: KeyData,
    #[serde(deserialize_with = "required")]
    parent: Option<u64>,
    #[serde(deserialize_with = "required")]
    row: Option<RowData>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceData {
    length: u64,
    #[serde(deserialize_with = "required")]
    last: Option<KeyData>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::transcript_view) struct Data {
    schema: u8,
    rows: Vec<EntryData>,
    start: u64,
    #[serde(deserialize_with = "required")]
    source: Option<SourceData>,
    live: (Vec<String>, Vec<String>),
    pending: Vec<(u64, DynamicData)>,
    indexed_snapshot: bool,
}
impl Data {
    pub(in crate::transcript_view) fn capture(
        actual: &TurnDetails,
        graph: &mut Capture<'_>,
    ) -> io::Result<Self> {
        let rows = actual
            .rows
            .iter()
            .map(|entry| {
                Ok(EntryData {
                    key: graph.key(entry.key)?,
                    parent: entry.parent.map(number).transpose()?,
                    row: entry
                        .row
                        .as_ref()
                        .map(|row| RowData::capture(row, graph))
                        .transpose()?,
                })
            })
            .collect::<io::Result<_>>()?;
        let source = actual
            .source
            .map(|(length, last)| {
                Ok::<_, io::Error>(SourceData {
                    length: number(length)?,
                    last: last.map(|key| graph.key(key)).transpose()?,
                })
            })
            .transpose()?;
        let pending = actual
            .pending
            .iter()
            .map(|(at, tool)| Ok((number(*at)?, graph.dynamic_cell(tool)?)))
            .collect::<io::Result<_>>()?;
        Ok(Self {
            schema: 1,
            rows,
            start: number(actual.start)?,
            source,
            live: actual.live.clone(),
            pending,
            indexed_snapshot: actual.indexed_snapshot,
        })
    }
    pub(in crate::transcript_view) fn restore(
        self,
        graph: &Restore<'_>,
        resources: &RestoreContext,
    ) -> io::Result<TurnDetails> {
        if self.schema != 1 {
            return Err(invalid("native turn-details schema"));
        }
        let length = self.rows.len();
        let start = index(self.start)?;
        if start > length || (length != 0 && start == length) {
            return Err(invalid("native turn start outside rows"));
        }
        let rows = self
            .rows
            .into_iter()
            .map(|entry| {
                let parent = entry.parent.map(index).transpose()?;
                if parent.is_some_and(|parent| parent >= length) {
                    return Err(invalid("native turn parent outside rows"));
                }
                Ok(Entry {
                    key: entry.key.restore(graph.cells)?,
                    parent,
                    row: entry.row.map(|row| row.restore(graph)).transpose()?,
                })
            })
            .collect::<io::Result<_>>()?;
        let source = self
            .source
            .map(|source| {
                Ok::<_, io::Error>((
                    index(source.length)?,
                    source
                        .last
                        .map(|key| key.restore(graph.cells))
                        .transpose()?,
                ))
            })
            .transpose()?;
        let pending = self
            .pending
            .into_iter()
            .map(|(at, tool)| {
                let at = index(at)?;
                if at >= length {
                    return Err(invalid("native pending tool outside rows"));
                }
                Ok((at, resources.dynamic_cell(&tool)?))
            })
            .collect::<io::Result<_>>()?;
        Ok(TurnDetails {
            rows,
            start,
            source,
            live: self.live,
            pending,
            indexed_snapshot: self.indexed_snapshot,
        })
    }
}
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    decoder: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(decoder)
}
#[cfg(test)]
#[path = "retained_transcript_turn_details_tests.rs"]
mod tests;
