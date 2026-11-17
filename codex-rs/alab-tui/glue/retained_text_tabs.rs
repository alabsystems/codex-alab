// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact sparse native row tab stops. Never recompute them from expanded text.
use super::*;
use serde::Deserialize;
use serde::Serialize;
use std::io;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StopData {
    offset: usize,
    raw_column: usize,
    width: usize,
    total_width: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct StopsData(Vec<StopData>);
impl StopsData {
    pub(in crate::transcript_view::text) fn capture(value: &TabStops) -> Self {
        Self(
            value
                .0
                .iter()
                .map(|value| StopData {
                    offset: value.offset,
                    raw_column: value.raw_column,
                    width: value.width,
                    total_width: value.total_width,
                })
                .collect(),
        )
    }
    pub(in crate::transcript_view::text) fn restore(&self, source: &str) -> io::Result<TabStops> {
        let mut previous_offset = None;
        let mut previous_raw = None;
        let mut total = 0usize;
        let mut stops = Vec::new();
        for value in &self.0 {
            total = total
                .checked_add(value.width)
                .ok_or_else(|| io::Error::other("native tab width overflow"))?;
            if !source
                .get(value.offset..)
                .is_some_and(|s| s.starts_with('\t'))
                || value.width == 0
                || value.width > 8
                || value.raw_column > usize::from(u16::MAX)
                || value.total_width > usize::from(u16::MAX)
                || previous_offset.is_some_and(|old| old >= value.offset)
                || previous_raw.is_some_and(|old| old > value.raw_column)
                || value.total_width != total
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid original native tab stop",
                ));
            }
            previous_offset = Some(value.offset);
            previous_raw = Some(value.raw_column);
            stops.push(TabStop {
                offset: value.offset,
                raw_column: value.raw_column,
                width: value.width,
                total_width: value.total_width,
            });
        }
        Ok(TabStops(stops))
    }
}
