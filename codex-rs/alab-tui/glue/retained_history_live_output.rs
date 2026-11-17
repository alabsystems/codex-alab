// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact incremental parser state, including split CRLF and bounded omission buffers.
use super::*;
use crate::terminal_hyperlinks::alab_retained_text::invalid;
use serde::Deserialize;
use serde::Serialize;
use std::io;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::exec_cell) struct Data {
    full_output: String,
    truncated: bool,
    head: Vec<String>,
    tail: VecDeque<String>,
    current: Current,
    completed_lines: usize,
    has_partial_line: bool,
    pending_carriage_return: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Current {
    head: String,
    tail: String,
    omitted_bytes: usize,
}
impl Data {
    pub(in crate::exec_cell) fn capture(actual: &LiveCommandOutput) -> Self {
        Self {
            full_output: actual.full_output.clone(),
            truncated: actual.truncated,
            head: actual.head.clone(),
            tail: actual.tail.clone(),
            current: Current {
                head: actual.current.head.clone(),
                tail: actual.current.tail.clone(),
                omitted_bytes: actual.current.omitted_bytes,
            },
            completed_lines: actual.completed_lines,
            has_partial_line: actual.has_partial_line,
            pending_carriage_return: actual.pending_carriage_return,
        }
    }
    pub(in crate::exec_cell) fn restore(self) -> io::Result<LiveCommandOutput> {
        if self.current.head.len() > LIVE_COMMAND_OUTPUT_LINE_HEAD_BYTES
            || self.current.tail.len() > LIVE_COMMAND_OUTPUT_LINE_TAIL_BYTES
        {
            return Err(invalid("native live output current-line capacity"));
        }
        if !self.truncated {
            if self.full_output.len() > LIVE_COMMAND_OUTPUT_MAX_BYTES
                || !self.head.is_empty()
                || !self.tail.is_empty()
                || !self.current.head.is_empty()
                || !self.current.tail.is_empty()
                || self.current.omitted_bytes != 0
                || self.pending_carriage_return
                || self.completed_lines
                    != self
                        .full_output
                        .bytes()
                        .filter(|byte| *byte == b'\n')
                        .count()
                || self.has_partial_line
                    != (!self.full_output.is_empty() && !self.full_output.ends_with('\n'))
            {
                return Err(invalid("native untruncated output parser state"));
            }
        } else {
            if !self.full_output.is_empty()
                || self.head.len() != self.completed_lines.min(LIVE_COMMAND_OUTPUT_MAX_LINES)
                || self.tail.len()
                    != self
                        .completed_lines
                        .saturating_sub(LIVE_COMMAND_OUTPUT_MAX_LINES)
                        .min(LIVE_COMMAND_OUTPUT_MAX_LINES)
                || self
                    .head
                    .iter()
                    .chain(self.tail.iter())
                    .any(|line| line.len() > LIVE_COMMAND_OUTPUT_MAX_BYTES || line.contains('\n'))
                || self.current.head.contains('\n')
                || self.current.tail.contains('\n')
                || (self.has_partial_line
                    && self.current.head.is_empty()
                    && self.current.tail.is_empty()
                    && self.current.omitted_bytes == 0
                    && !self.pending_carriage_return)
                || (!self.has_partial_line
                    && (!self.current.head.is_empty()
                        || !self.current.tail.is_empty()
                        || self.current.omitted_bytes != 0
                        || self.pending_carriage_return))
            {
                return Err(invalid("native truncated output parser state"));
            }
        }
        Ok(LiveCommandOutput {
            full_output: self.full_output,
            truncated: self.truncated,
            head: self.head,
            tail: self.tail,
            current: LiveCommandOutputLine {
                head: self.current.head,
                tail: self.current.tail,
                omitted_bytes: self.current.omitted_bytes,
            },
            completed_lines: self.completed_lines,
            has_partial_line: self.has_partial_line,
            pending_carriage_return: self.pending_carriage_return,
        })
    }
}

#[cfg(test)]
#[path = "retained_history_live_output_tests.rs"]
mod retained_tests;
