// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Historical tail caches use already registered real strong cell owners.
use super::*;
use crate::history_cell::alab_retained_cells as cells;
use crate::streaming::alab_retained::required;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct TailData {
    #[serde(deserialize_with = "required")]
    cell: Option<cells::CellRef>,
    lines: Vec<native::HyperlinkData>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct UsageData {
    thread_id: ThreadId,
    tail: TailData,
}
impl TailData {
    fn capture_parts(
        cell: &Weak<dyn HistoryCell>,
        lines: &[HyperlinkLine],
        graph: &cells::Capture,
        text: &mut native::Capture,
    ) -> io::Result<Self> {
        Ok(Self {
            cell: graph.weak_reference(cell)?,
            lines: lines
                .iter()
                .map(|line| text.line(line))
                .collect::<io::Result<_>>()?,
        })
    }
    pub(in crate::app) fn capture(
        actual: &RenderedHistoryTail,
        graph: &cells::Capture,
        text: &mut native::Capture,
    ) -> io::Result<Self> {
        let RenderedHistoryTail { cell, lines } = actual;
        Self::capture_parts(cell, lines, graph, text)
    }
    pub(in crate::app) fn restore(
        self,
        graph: &cells::Restored,
        text: &mut native::Restored,
    ) -> io::Result<RenderedHistoryTail> {
        Ok(RenderedHistoryTail {
            cell: graph.weak_reference(self.cell)?,
            lines: self
                .lines
                .iter()
                .map(|line| text.line(line))
                .collect::<io::Result<_>>()?,
        })
    }
}
impl UsageData {
    pub(in crate::app) fn capture(
        actual: &ThreadUsageStatusHistory,
        graph: &cells::Capture,
        text: &mut native::Capture,
    ) -> io::Result<Self> {
        let ThreadUsageStatusHistory {
            thread_id,
            cell,
            lines,
        } = actual;
        Ok(Self {
            thread_id: *thread_id,
            tail: TailData::capture_parts(cell, lines, graph, text)?,
        })
    }
    pub(in crate::app) fn restore(
        self,
        graph: &cells::Restored,
        text: &mut native::Restored,
    ) -> io::Result<ThreadUsageStatusHistory> {
        let tail = self.tail.restore(graph, text)?;
        Ok(ThreadUsageStatusHistory {
            thread_id: self.thread_id,
            cell: tail.cell,
            lines: tail.lines,
        })
    }
}

#[cfg(test)]
mod controls {
    use super::*;
    #[test]
    fn retained_native_history_tails_keep_original_shared_owner_and_expired_weak_edges()
    -> io::Result<()> {
        let root: Arc<dyn HistoryCell> =
            Arc::new(crate::history_cell::PlainHistoryCell::new(vec![
                Line::from("actual root"),
            ]));
        let stale: Arc<dyn HistoryCell> =
            Arc::new(crate::history_cell::PlainHistoryCell::new(vec![
                Line::from("retired root"),
            ]));
        let expired = Arc::downgrade(&stale);
        drop(stale);
        let line = HyperlinkLine::from(Line::from("cached native tail"));
        let tail = RenderedHistoryTail {
            cell: Arc::downgrade(&root),
            lines: vec![line.clone()],
        };
        let usage = ThreadUsageStatusHistory {
            thread_id: ThreadId::new(),
            cell: Arc::downgrade(&root),
            lines: vec![line.clone()],
        };
        let old = RenderedHistoryTail {
            cell: expired,
            lines: vec![line],
        };
        let mut cells = cells::Capture::default();
        let root_ref = cells.intern(&root)?;
        let mut text = native::Capture::default();
        let tail = TailData::capture(&tail, &cells, &mut text)?;
        let usage = UsageData::capture(&usage, &cells, &mut text)?;
        let old = TailData::capture(&old, &cells, &mut text)?;
        assert!(old.cell.is_none());
        let raw = cells.encode(&mut text, 1_048_576)?;
        let mut text = native::Restored::new(text.export()?)?;
        let cells = cells::Restored::decode(
            &raw,
            1_048_576,
            &mut text,
            &mut cells::RestoreContext::default(),
        )?;
        let tail = tail.restore(&cells, &mut text)?;
        let usage = usage.restore(&cells, &mut text)?;
        let old = old.restore(&cells, &mut text)?;
        let root = cells.cell(root_ref)?;
        assert!(Arc::ptr_eq(&root, &tail.cell.upgrade().unwrap()));
        assert!(tail.cell.ptr_eq(&usage.cell));
        assert!(old.cell.upgrade().is_none());
        assert_eq!(tail.lines[0].line, usage.lines[0].line);
        assert_eq!(tail.lines[0].line, old.lines[0].line);
        Ok(())
    }
}
