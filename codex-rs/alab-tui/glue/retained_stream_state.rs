// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original FIFO queued lines and exact enqueue instants, not replayed requests.
use super::*;
use crate::markdown_stream::alab_retained_collector as collector;
use crate::streaming::alab_retained::invalid;
use crate::terminal_hyperlinks::alab_retained_text as native;
use codex_alab_tui::upgrade_editor_clock::Snapshot;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    collector: collector::Data,
    queued_lines: Vec<(native::HyperlinkData, Snapshot)>,
    has_seen_delta: bool,
}
impl Data {
    pub(crate) fn capture(actual: &StreamState, shared: &mut native::Capture) -> io::Result<Self> {
        let StreamState {
            collector,
            queued_lines,
            has_seen_delta,
        } = actual;
        Ok(Self {
            collector: collector::Data::capture(collector),
            queued_lines: queued_lines
                .iter()
                .map(|QueuedLine { line, enqueued_at }| {
                    Ok((
                        shared.line(line)?,
                        Snapshot::capture(*enqueued_at).map_err(invalid)?,
                    ))
                })
                .collect::<io::Result<_>>()?,
            has_seen_delta: *has_seen_delta,
        })
    }
    pub(crate) fn restore(self, shared: &mut native::Restored) -> io::Result<StreamState> {
        Ok(StreamState {
            collector: self.collector.restore()?,
            queued_lines: self
                .queued_lines
                .iter()
                .map(|(line, at)| {
                    Ok(QueuedLine {
                        line: shared.line(line)?,
                        enqueued_at: at.restore().map_err(invalid)?,
                    })
                })
                .collect::<io::Result<_>>()?,
            has_seen_delta: self.has_seen_delta,
        })
    }
}
