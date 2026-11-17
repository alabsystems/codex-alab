// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original collector cursor, including its uncommitted tail; never finalize on capture.
use super::*;
#[cfg(test)]
use crate::streaming::alab_retained::PathData;
use crate::streaming::alab_retained::boundary;
use crate::streaming::alab_retained::invalid;
use crate::streaming::alab_retained::required;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    buffer: String,
    committed_source_len: usize,
    #[serde(deserialize_with = "required")]
    width: Option<usize>,
    #[cfg(test)]
    committed_line_count: usize,
    #[cfg(test)]
    cwd: PathData,
}
impl Data {
    pub(crate) fn capture(actual: &MarkdownStreamCollector) -> Self {
        let MarkdownStreamCollector {
            buffer,
            committed_source_len,
            width,
            #[cfg(test)]
            committed_line_count,
            #[cfg(test)]
            cwd,
        } = actual;
        Self {
            buffer: buffer.clone(),
            committed_source_len: *committed_source_len,
            width: *width,
            #[cfg(test)]
            committed_line_count: *committed_line_count,
            #[cfg(test)]
            cwd: PathData::capture(cwd),
        }
    }
    pub(crate) fn restore(self) -> io::Result<MarkdownStreamCollector> {
        boundary(&self.buffer, self.committed_source_len)?;
        if self.committed_source_len != 0
            && !self.buffer[..self.committed_source_len].ends_with('\n')
        {
            return Err(invalid(
                "original stream collector committed an incomplete line",
            ));
        }
        Ok(MarkdownStreamCollector {
            buffer: self.buffer,
            committed_source_len: self.committed_source_len,
            width: self.width,
            #[cfg(test)]
            committed_line_count: self.committed_line_count,
            #[cfg(test)]
            cwd: self.cwd.restore()?,
        })
    }
}
