// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact native bookmark DATA. Decode does not call restore_bookmark, rewrap,
//! render, change presentation or resolve a current terminal/renderer epoch.
use super::*;
use crate::transcript_view::alab_retained_view::identity::KeyData;
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
enum ModeData {
    Rich {},
    Raw {},
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HeaderData {
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    key: KeyData,
    row: u64,
}
impl HeaderData {
    fn capture(
        actual: prompt_header::SuppressedHeader,
        graph: &tables::Capture<'_>,
    ) -> io::Result<Self> {
        Ok(Self {
            x: actual.viewport.x,
            y: actual.viewport.y,
            width: actual.viewport.width,
            height: actual.viewport.height,
            key: graph.key(actual.key)?,
            row: u64::try_from(actual.row).map_err(invalid)?,
        })
    }
    fn restore(self, graph: &tables::Restore<'_>) -> io::Result<prompt_header::SuppressedHeader> {
        Ok(prompt_header::SuppressedHeader {
            viewport: Rect {
                x: self.x,
                y: self.y,
                width: self.width,
                height: self.height,
            },
            key: self.key.restore(graph.cells)?,
            row: usize::try_from(self.row).map_err(invalid)?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Data {
    schema: u8,
    position: PositionData,
    #[serde(deserialize_with = "required")]
    saved_position: Option<PositionData>,
    #[serde(deserialize_with = "required")]
    suppressed_prompt_header: Option<HeaderData>,
    detailed: bool,
    mode: ModeData,
    #[serde(deserialize_with = "required")]
    snapshot: Option<snapshots::Data>,
    #[serde(deserialize_with = "required")]
    last_tail: Option<KeyData>,
    unseen_activity: bool,
}
impl Data {
    pub(crate) fn register_roots(
        actual: &TranscriptBookmark,
        graph: &mut tables::Capture<'_>,
    ) -> io::Result<()> {
        if let Some(snapshot) = &actual.snapshot {
            graph.register(&snapshot.cells)?;
        }
        Ok(())
    }
    pub(crate) fn capture(
        actual: &TranscriptBookmark,
        graph: &mut tables::Capture<'_>,
    ) -> io::Result<Self> {
        // Held original cells register before any bookmark key is translated.
        let snapshot = actual
            .snapshot
            .as_ref()
            .map(|snapshot| snapshots::Data::capture(snapshot, graph))
            .transpose()?;
        Ok(Self {
            schema: 1,
            position: graph.position(actual.position)?,
            saved_position: actual
                .saved_position
                .map(|position| graph.position(position))
                .transpose()?,
            suppressed_prompt_header: actual
                .suppressed_prompt_header
                .map(|header| HeaderData::capture(header, graph))
                .transpose()?,
            detailed: actual.detailed,
            mode: match actual.mode {
                HistoryRenderMode::Rich => ModeData::Rich {},
                HistoryRenderMode::Raw => ModeData::Raw {},
            },
            snapshot,
            last_tail: actual.last_tail.map(|key| graph.key(key)).transpose()?,
            unseen_activity: actual.unseen_activity,
        })
    }
    pub(crate) fn restore(self, graph: &tables::Restore<'_>) -> io::Result<TranscriptBookmark> {
        if self.schema != 1 {
            return Err(invalid("native bookmark schema"));
        }
        Ok(TranscriptBookmark {
            position: self.position.restore(graph.cells)?,
            saved_position: self
                .saved_position
                .map(|position| position.restore(graph.cells))
                .transpose()?,
            suppressed_prompt_header: self
                .suppressed_prompt_header
                .map(|header| header.restore(graph))
                .transpose()?,
            detailed: self.detailed,
            mode: match self.mode {
                ModeData::Rich {} => HistoryRenderMode::Rich,
                ModeData::Raw {} => HistoryRenderMode::Raw,
            },
            snapshot: self
                .snapshot
                .map(|snapshot| snapshot.restore(graph))
                .transpose()?,
            last_tail: self
                .last_tail
                .map(|key| key.restore(graph.cells))
                .transpose()?,
            unseen_activity: self.unseen_activity,
        })
    }
}
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    decoder: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(decoder)
}
#[cfg(test)]
#[path = "retained_transcript_bookmark_tests.rs"]
mod tests;
