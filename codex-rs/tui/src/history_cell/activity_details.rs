//! Retain hidden reasoning between activity calls without breaking their compact group.
//!
//! Positions count preceding calls. Prepending history shifts those positions while sharing the
//! original immutable detail cells, so expanded output keeps its chronological source content.
//! Raw projection follows each cell's contract, retaining terminal input while hiding reasoning.

use super::HistoryCell;
use super::HistoryRenderMode;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::plain_hyperlink_lines;
use std::sync::Arc;

#[derive(Debug, Default, Clone)]
pub(crate) struct ActivityDetails {
    entries: Vec<(usize, Arc<dyn HistoryCell>)>,
}

impl ActivityDetails {
    pub(crate) fn push(&mut self, after_calls: usize, cell: Arc<dyn HistoryCell>) {
        self.entries.push((after_calls, cell));
    }

    pub(crate) fn prepend(&mut self, mut older: Self, older_calls: usize) {
        for (position, _) in &mut self.entries {
            *position += older_calls;
        }
        older.entries.append(&mut self.entries);
        *self = older;
    }

    /// Split a group's transcript details at the same boundary as its calls.
    pub(crate) fn split_after(&mut self, calls: usize) -> Self { // ALAB(misalignment-review): keep original positional details when separating unfinished commands.
        let first_later = self.entries.partition_point(|(position, _)| *position <= calls);
        let mut entries = self.entries.split_off(first_later);
        for (position, _) in &mut entries {
            *position -= calls;
        }
        Self { entries }
    }

    pub(crate) fn lines_after(
        &self,
        after_calls: usize,
        width: u16,
        mode: HistoryRenderMode,
    ) -> Vec<HyperlinkLine> {
        self.entries
            .iter()
            .filter(|(position, _)| *position == after_calls)
            .flat_map(|(_, cell)| {
                let mut lines = match mode {
                    HistoryRenderMode::Rich => cell.transcript_hyperlink_lines(width),
                    HistoryRenderMode::Raw => plain_hyperlink_lines(cell.raw_lines()),
                };
                if mode == HistoryRenderMode::Rich && !lines.is_empty() {
                    lines.insert(/*index*/ 0, HyperlinkLine::from(""));
                }
                lines
            })
            .collect()
    }
}

#[cfg(unix)]
#[path = "../../../alab-tui/glue/retained_history_activity_details.rs"]
pub(in crate::history_cell) mod alab_retained_details; // ALAB(retained-ui): preserve exact native activity state and shared graph identity.
