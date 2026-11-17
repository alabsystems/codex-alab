// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Preserve actual table fragments and alignment Arc identities without re-rendering.
use super::*;
use crate::terminal_hyperlinks::alab_retained_text::Capture;
use crate::terminal_hyperlinks::alab_retained_text::Restored;
use crate::terminal_hyperlinks::alab_retained_text::SourceData;
use crate::terminal_hyperlinks::alab_retained_text::depth;
use crate::terminal_hyperlinks::alab_retained_text::invalid;
use crate::terminal_hyperlinks::alab_retained_text::range;
use codex_alab_tui::retained_arc_graph::Id;
use serde::Deserialize;
use serde::Serialize;
use std::io;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FragmentData {
    output: Range<usize>,
    row: usize,
    column: usize,
    source: SourceData,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TableData {
    table: Id,
    fragments: Vec<FragmentData>,
}
impl TableData {
    pub(crate) fn capture(
        value: &TableLine,
        graph: &mut Capture,
        nesting: usize,
    ) -> io::Result<Self> {
        let nesting = depth(nesting)?;
        Ok(Self {
            table: graph.table(&value.table)?,
            fragments: value
                .fragments
                .iter()
                .map(|value| {
                    Ok(FragmentData {
                        output: value.output.clone(),
                        row: value.row,
                        column: value.column,
                        source: graph.source(&value.source, nesting)?,
                    })
                })
                .collect::<io::Result<Vec<_>>>()?,
        })
    }
    pub(crate) fn restore(&self, graph: &mut Restored, nesting: usize) -> io::Result<TableLine> {
        let nesting = depth(nesting)?;
        let table = graph.table(self.table)?;
        let fragments = self
            .fragments
            .iter()
            .map(|value| {
                if value.column >= table.len() {
                    return Err(invalid("native table fragment column"));
                }
                let source = graph.source(&value.source, nesting)?;
                if value.output.start > value.output.end || value.output.len() != source.range.len()
                {
                    return Err(invalid("native table fragment coordinates"));
                }
                Ok(Fragment {
                    output: value.output.clone(),
                    row: value.row,
                    column: value.column,
                    source,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        Ok(TableLine { table, fragments })
    }
    pub(crate) fn validate_source(value: &TableLine, text: &str) -> io::Result<()> {
        for fragment in &value.fragments {
            range(text, &fragment.output)?;
        }
        Ok(())
    }
}
