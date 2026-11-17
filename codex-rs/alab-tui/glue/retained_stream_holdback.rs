// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Preserve incremental table lookbehind, including held partial rows.
use super::*;
use crate::streaming::alab_retained::boundary;
use crate::streaming::alab_retained::required;
use crate::table_detect::alab_retained_fence as fence;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    source_offset: usize,
    fence_tracker: fence::Data,
    #[serde(deserialize_with = "required")]
    previous_line: Option<(usize, fence::Kind, bool)>,
    #[serde(deserialize_with = "required")]
    pending_header_start: Option<usize>,
    #[serde(deserialize_with = "required")]
    confirmed_table_start: Option<usize>,
    in_table: bool,
}
impl Data {
    pub(crate) fn capture(actual: &TableHoldbackScanner) -> Self {
        let TableHoldbackScanner {
            source_offset,
            fence_tracker,
            previous_line,
            pending_header_start,
            confirmed_table_start,
            in_table,
        } = actual;
        Self {
            source_offset: *source_offset,
            fence_tracker: fence::Data::capture(fence_tracker),
            previous_line: previous_line.map(|x| {
                (
                    x.source_start,
                    fence::Kind::capture(x.fence_kind),
                    x.is_header,
                )
            }),
            pending_header_start: *pending_header_start,
            confirmed_table_start: *confirmed_table_start,
            in_table: *in_table,
        }
    }
    pub(crate) fn restore(self, source: &str) -> io::Result<TableHoldbackScanner> {
        for index in [
            Some(self.source_offset),
            self.previous_line.as_ref().map(|x| x.0),
            self.pending_header_start,
            self.confirmed_table_start,
        ]
        .into_iter()
        .flatten()
        {
            boundary(source, index)?;
        }
        Ok(TableHoldbackScanner {
            source_offset: self.source_offset,
            fence_tracker: self.fence_tracker.restore()?,
            previous_line: self.previous_line.map(|(source_start, kind, is_header)| {
                PreviousLineState {
                    source_start,
                    fence_kind: kind.restore(),
                    is_header,
                }
            }),
            pending_header_start: self.pending_header_start,
            confirmed_table_start: self.confirmed_table_start,
            in_table: self.in_table,
        })
    }
}
