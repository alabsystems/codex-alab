// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original held revisions retain actual cells, layouts and activity-array aliases.
use super::identity::KeyData;
use super::tables::ActivityRef;
use super::tables::Capture;
use super::tables::Restore;
use super::tables::SliceRef;
use super::*;
use crate::transcript_view::text::alab_retained_layout::LayoutRef;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::transcript_view) struct Data {
    schema: u8,
    cells: SliceRef,
    pinned: Vec<(KeyData, LayoutRef)>,
    activities: Vec<(KeyData, ActivityRef)>,
    #[serde(deserialize_with = "required")]
    search_live: Option<LayoutRef>,
}
impl Data {
    pub(in crate::transcript_view) fn capture(
        actual: &ViewSnapshot,
        graph: &mut Capture<'_>,
    ) -> io::Result<Self> {
        let cells = graph.slice(&actual.cells)?;
        // Keys must be registered from actual current/parked/held cells first;
        // a stale pointer with no real native cell owner never manufactures one.
        let pinned = actual
            .pinned
            .iter()
            .map(|(key, layout)| Ok((graph.key(*key)?, graph.layout(layout)?)))
            .collect::<io::Result<_>>()?;
        let activities = actual
            .activities
            .iter()
            .map(|(key, ids)| Ok((graph.key(*key)?, graph.activity(ids)?)))
            .collect::<io::Result<_>>()?;
        let search_live = actual
            .search_live
            .as_ref()
            .map(|layout| graph.layout(layout))
            .transpose()?;
        Ok(Self {
            schema: 1,
            cells,
            pinned,
            activities,
            search_live,
        })
    }
    pub(in crate::transcript_view) fn restore(
        self,
        graph: &Restore<'_>,
    ) -> io::Result<ViewSnapshot> {
        if self.schema != 1 {
            return Err(std::io::Error::other("native view snapshot schema"));
        }
        let cells = graph.slice(self.cells)?;
        let mut pinned = std::collections::HashMap::new();
        for (key, layout) in self.pinned {
            let key = key.restore(graph.cells)?;
            let layout = graph.layouts.layout(layout)?;
            if pinned.insert(key, layout).is_some() {
                return Err(std::io::Error::other("duplicate actual pinned native key"));
            }
        }
        let mut activities = std::collections::HashMap::new();
        for (key, ids) in self.activities {
            let key = key.restore(graph.cells)?;
            let ids = graph.activity(ids)?;
            if activities.insert(key, ids).is_some() {
                return Err(std::io::Error::other(
                    "duplicate actual native activity key",
                ));
            }
        }
        let search_live = self
            .search_live
            .map(|layout| graph.layouts.layout(layout))
            .transpose()?;
        Ok(ViewSnapshot {
            cells,
            pinned,
            activities,
            search_live,
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
#[path = "retained_transcript_snapshot_tests.rs"]
mod tests;
