// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Replace process-local EntryKey pointers with bounded references into the
//! actual native cell graph. Live remains the native logical slot; its actual
//! current or pinned layout belongs to the enclosing view codec, not this tag.
use super::*;
use crate::history_cell::alab_retained_cells::Capture;
use crate::history_cell::alab_retained_cells::CellRef;
use crate::history_cell::alab_retained_cells::Restored;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::io;

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum KeyData {
    Cell { original: CellRef },
    Live {},
}

pub(crate) struct CaptureRefs<'a> {
    graph: &'a mut Capture,
    keys: HashMap<usize, CellRef>,
    activity_keys: HashMap<String, ActivityIdData>,
}
impl<'a> CaptureRefs<'a> {
    pub(in crate::transcript_view) fn native_graph(&mut self) -> &mut Capture {
        self.graph
    }
    pub(crate) fn new(graph: &'a mut Capture) -> Self {
        Self {
            graph,
            keys: HashMap::new(),
            activity_keys: HashMap::new(),
        }
    }
    /// Register real current, parked and held-revision Arcs before encoding
    /// keys. The global graph keeps those exact Arcs alive against pointer ABA.
    pub(crate) fn register(&mut self, cells: &[Arc<dyn HistoryCell>]) -> io::Result<()> {
        for cell in cells {
            let pointer = Arc::as_ptr(cell).cast::<()>() as usize;
            let original = self.graph.intern(cell)?;
            self.activity_keys.insert(
                format!("work:{:?}", EntryKey::cell(cell)),
                ActivityIdData::WorkCell { original },
            );
            self.activity_keys.insert(
                format!("turn:{:?}", EntryKey::cell(cell)),
                ActivityIdData::TurnCell { original },
            );
            if self
                .keys
                .insert(pointer, original)
                .is_some_and(|old| old != original)
            {
                return Err(invalid("one actual transcript cell changed graph identity"));
            }
        }
        Ok(())
    }
    pub(in crate::transcript_view) fn intern(
        &mut self,
        cell: &Arc<dyn HistoryCell>,
    ) -> io::Result<CellRef> {
        self.register(std::slice::from_ref(cell))?;
        match self.key(EntryKey::cell(cell))? {
            KeyData::Cell { original } => Ok(original),
            KeyData::Live {} => Err(invalid("actual cell cannot be the logical live slot")),
        }
    }
    /// Resolve an actual weak cache source only after all real App roots were registered.
    pub(in crate::transcript_view) fn weak(
        &self,
        actual: &std::sync::Weak<dyn HistoryCell>,
    ) -> io::Result<Option<CellRef>> {
        actual
            .upgrade()
            .map(|cell| -> io::Result<CellRef> {
                match self.key(EntryKey::cell(&cell))? {
                    KeyData::Cell { original } => Ok(original),
                    KeyData::Live {} => {
                        Err(invalid("weak cache source cannot be the logical live slot"))
                    }
                }
            })
            .transpose()
    }
    /// A pending view clone uses the same global mutable tool-row table.
    pub(in crate::transcript_view) fn dynamic_cell(
        &mut self,
        actual: &crate::history_cell::DynamicToolCallCell,
    ) -> io::Result<crate::history_cell::alab_retained_cells::DynamicData> {
        self.graph.dynamic_cell(actual)
    }
    pub(in crate::transcript_view) fn key(&self, key: EntryKey) -> io::Result<KeyData> {
        match key {
            EntryKey::Cell(pointer) => self
                .keys
                .get(&pointer)
                .copied()
                .map(|original| KeyData::Cell { original })
                .ok_or_else(|| invalid("transcript key has no retained actual cell owner")),
            EntryKey::Live => Ok(KeyData::Live {}),
        }
    }
    /// Only IDs generated from registered actual native cells become references.
    /// Backend/call IDs and unknown old labels remain opaque literal DATA.
    pub(in crate::transcript_view) fn activity(&self, value: &str) -> ActivityIdData {
        self.activity_keys
            .get(value)
            .cloned()
            .unwrap_or_else(|| ActivityIdData::Literal {
                value: value.to_owned(),
            })
    }
    pub(in crate::transcript_view) fn anchor(&self, anchor: Anchor) -> io::Result<AnchorData> {
        Ok(AnchorData {
            key: self.key(anchor.key)?,
            index: u64::try_from(anchor.index).map_err(invalid)?,
            offset: u64::try_from(anchor.offset).map_err(invalid)?,
            row_bias: i64::try_from(anchor.row_bias).map_err(invalid)?,
        })
    }
    pub(in crate::transcript_view) fn position(
        &self,
        position: Position,
    ) -> io::Result<PositionData> {
        match position {
            Position::Latest => Ok(PositionData::Latest {}),
            Position::Reading(anchor) => Ok(PositionData::Reading {
                anchor: self.anchor(anchor)?,
            }),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::transcript_view) enum ActivityIdData {
    Literal { value: String },
    WorkCell { original: CellRef },
    TurnCell { original: CellRef },
}
impl ActivityIdData {
    /// Resolve literals again at final table export: a pure-DATA capture can
    /// refuse a missing root, register it, and retry with the same graph.
    pub(in crate::transcript_view) fn canonical(&self, refs: &CaptureRefs<'_>) -> Self {
        match self {
            Self::Literal { value } => refs.activity(value),
            _ => self.clone(),
        }
    }
    pub(in crate::transcript_view) fn restore(self, graph: &Restored) -> io::Result<String> {
        match self {
            Self::Literal { value } => Ok(value),
            Self::WorkCell { original } => {
                Ok(format!("work:{:?}", EntryKey::cell(&graph.cell(original)?)))
            }
            Self::TurnCell { original } => {
                Ok(format!("turn:{:?}", EntryKey::cell(&graph.cell(original)?)))
            }
        }
    }
}
impl KeyData {
    pub(in crate::transcript_view) fn restore(self, graph: &Restored) -> io::Result<EntryKey> {
        match self {
            Self::Cell { original } => Ok(EntryKey::cell(&graph.cell(original)?)),
            Self::Live {} => Ok(EntryKey::Live),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnchorData {
    key: KeyData,
    index: u64,
    offset: u64,
    row_bias: i64,
}
impl AnchorData {
    pub(in crate::transcript_view) fn restore(self, graph: &Restored) -> io::Result<Anchor> {
        Ok(Anchor {
            key: self.key.restore(graph)?,
            index: usize::try_from(self.index).map_err(invalid)?,
            offset: usize::try_from(self.offset).map_err(invalid)?,
            row_bias: isize::try_from(self.row_bias).map_err(invalid)?,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum PositionData {
    Latest {},
    Reading { anchor: AnchorData },
}
impl PositionData {
    pub(in crate::transcript_view) fn restore(self, graph: &Restored) -> io::Result<Position> {
        match self {
            Self::Latest {} => Ok(Position::Latest),
            Self::Reading { anchor } => Ok(Position::Reading(anchor.restore(graph)?)),
        }
    }
}

#[cfg(test)]
#[path = "retained_transcript_identity_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "retained_transcript_activity_id_tests.rs"]
mod activity_id_tests;
