// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual unprinted queue; canonical owner and retained output source may differ.
//! No terminal output/replay or fresh cell is manufactured by this codec.
use super::*;
use crate::history_cell::alab_retained_cells::Capture;
use crate::history_cell::alab_retained_cells::CellRef;
use crate::history_cell::alab_retained_cells::Restored;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingData {
    source: CellRef,
    #[serde(deserialize_with = "required")]
    owner: Option<CellRef>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct Data {
    schema: u8,
    pending: Vec<PendingData>,
}
impl Data {
    /// Part of the enclosing App root prepass, before ANY weak edge is encoded.
    pub(in crate::app) fn register_roots(
        actual: &NativeHistory,
        graph: &mut Capture,
    ) -> io::Result<()> {
        for entry in &actual.pending {
            graph.intern(&entry.source)?;
        }
        Ok(())
    }
    pub(in crate::app) fn capture(actual: &NativeHistory, graph: &mut Capture) -> io::Result<Self> {
        let pending = actual
            .pending
            .iter()
            .map(|entry| {
                Ok(PendingData {
                    source: graph.intern(&entry.source)?,
                    owner: graph.weak_reference(&entry.owner)?,
                })
            })
            .collect::<io::Result<_>>()?;
        Ok(Self { schema: 1, pending })
    }
    pub(in crate::app) fn restore(self, graph: &Restored) -> io::Result<NativeHistory> {
        if self.schema != 1 {
            return Err(io::Error::other("native pending history schema"));
        }
        let pending = self
            .pending
            .into_iter()
            .map(|entry| {
                Ok(PendingCell {
                    source: graph.cell(entry.source)?,
                    owner: graph.weak_reference(entry.owner)?,
                })
            })
            .collect::<io::Result<_>>()?;
        Ok(NativeHistory { pending })
    }
}
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}
#[cfg(test)]
#[path = "retained_native_history_queue_tests.rs"]
mod tests;
