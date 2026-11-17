// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native Markdown-copy fields, including private runs. No Markdown reparsing.
use super::*;
use crate::markdown_copy::table::alab_retained_table::TableData;
use crate::terminal_hyperlinks::alab_retained_text::Capture;
use crate::terminal_hyperlinks::alab_retained_text::Restored;
use crate::terminal_hyperlinks::alab_retained_text::invalid;
use crate::terminal_hyperlinks::alab_retained_text::range;
use codex_alab_tui::retained_arc_graph::Id;
use serde::Deserialize;
use serde::Serialize;
use std::io;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum InlineData {
    Link { text: Id },
    Delimiter { value: String },
    Code {},
    Literal {},
    Ignored {},
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CopyData {
    prefix: String,
    continuation: String,
    item_prefix: String,
    code: bool,
    code_source: Option<Id>,
    quote_source: Option<Id>,
    table: Option<TableData>,
    table_cell: bool,
    rule: bool,
    heading: usize,
    hard_break: bool,
    omit: bool,
    runs: Vec<(Range<usize>, Vec<InlineData>)>,
    literal_ranges: Vec<Range<usize>>,
}
impl CopyData {
    pub(crate) fn capture(
        value: &CopyLine,
        graph: &mut Capture,
        nesting: usize,
    ) -> io::Result<Self> {
        let runs = value
            .runs
            .iter()
            .map(|(range, stack)| {
                let stack = stack
                    .iter()
                    .map(|item| {
                        Ok(match item {
                            Inline::Link(text) => InlineData::Link {
                                text: graph.string(text)?,
                            },
                            Inline::Delimiter(value) => InlineData::Delimiter {
                                value: value.to_string(),
                            },
                            Inline::Code => InlineData::Code {},
                            Inline::Literal | Inline::Ignored => {
                                return Err(invalid("non-stored native inline marker"));
                            }
                        })
                    })
                    .collect::<io::Result<Vec<_>>>()?;
                Ok((range.clone(), stack))
            })
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Self {
            prefix: value.prefix.clone(),
            continuation: value.continuation.clone(),
            item_prefix: value.item_prefix.clone(),
            code: value.code,
            code_source: value
                .code_source
                .as_ref()
                .map(|v| graph.string(v))
                .transpose()?,
            quote_source: value
                .quote_source
                .as_ref()
                .map(|v| graph.string(v))
                .transpose()?,
            table: value
                .table
                .as_ref()
                .map(|v| TableData::capture(v, graph, nesting))
                .transpose()?,
            table_cell: value.table_cell,
            rule: value.rule,
            heading: value.heading,
            hard_break: value.hard_break,
            omit: value.omit,
            runs,
            literal_ranges: value.literal_ranges.clone(),
        })
    }
    pub(crate) fn restore(&self, graph: &mut Restored, nesting: usize) -> io::Result<CopyLine> {
        if self.heading == 1 || self.heading > 7 {
            return Err(invalid("invalid native heading prefix length"));
        }
        let runs = self
            .runs
            .iter()
            .map(|(range, stack)| {
                if stack.len() > MAX_COPY_DEPTH {
                    return Err(invalid("native inline copy nesting"));
                }
                let stack = stack
                    .iter()
                    .map(|item| {
                        Ok(match item {
                            InlineData::Link { text } => Inline::Link(graph.string(*text)?),
                            InlineData::Delimiter { value } => {
                                Inline::Delimiter(match value.as_str() {
                                    "_" => "_",
                                    "**" => "**",
                                    "~~" => "~~",
                                    _ => return Err(invalid("unknown native copy delimiter")),
                                })
                            }
                            InlineData::Code {} => Inline::Code,
                            InlineData::Literal {} | InlineData::Ignored {} => {
                                return Err(invalid("non-stored native inline marker"));
                            }
                        })
                    })
                    .collect::<io::Result<Vec<_>>>()?;
                Ok((range.clone(), stack))
            })
            .collect::<io::Result<Vec<_>>>()?;
        Ok(CopyLine {
            prefix: self.prefix.clone(),
            continuation: self.continuation.clone(),
            item_prefix: self.item_prefix.clone(),
            code: self.code,
            code_source: self.code_source.map(|v| graph.string(v)).transpose()?,
            quote_source: self.quote_source.map(|v| graph.string(v)).transpose()?,
            table: self
                .table
                .as_ref()
                .map(|v| v.restore(graph, nesting))
                .transpose()?,
            table_cell: self.table_cell,
            rule: self.rule,
            heading: self.heading,
            hard_break: self.hard_break,
            omit: self.omit,
            runs,
            literal_ranges: self.literal_ranges.clone(),
        })
    }
    pub(crate) fn validate_source(value: &CopyLine, text: &str) -> io::Result<()> {
        if !text.is_char_boundary(value.heading) {
            return Err(invalid("invalid native heading UTF-8 boundary"));
        }
        let mut end = 0;
        for (value, _) in &value.runs {
            range(text, value)?;
            if value.start < end {
                return Err(invalid("unordered native copy runs"));
            }
            end = value.end;
        }
        let mut end = 0;
        for value in &value.literal_ranges {
            range(text, value)?;
            if value.start < end {
                return Err(invalid("unordered native literal ranges"));
            }
            end = value.end;
        }
        if let Some(table) = &value.table {
            TableData::validate_source(table, text)?;
        }
        Ok(())
    }
}
