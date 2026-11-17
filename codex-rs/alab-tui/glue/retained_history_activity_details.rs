// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original chronological detail edges use the SAME global native-cell graph.
use super::*;
use crate::history_cell::alab_retained_cells::CaptureContext;
use crate::history_cell::alab_retained_cells::CellRef;
use crate::history_cell::alab_retained_cells::RestoreContext;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::io;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Data {
    entries: Vec<(usize, CellRef)>,
}
impl Data {
    pub(crate) fn capture(
        actual: &ActivityDetails,
        context: &mut CaptureContext<'_>,
        calls: usize,
    ) -> io::Result<Self> {
        if actual.entries.len() > native::MAX_NODES {
            return Err(native::invalid("activity detail capacity"));
        }
        let mut entries = Vec::with_capacity(actual.entries.len());
        for (position, actual) in &actual.entries {
            if *position > calls {
                return Err(native::invalid(
                    "activity detail lies outside original calls",
                ));
            }
            entries.push((*position, context.cell(actual)?));
        }
        Ok(Self { entries })
    }
    pub(crate) fn restore(
        self,
        shared: &mut native::Restored,
        resources: &mut RestoreContext,
        calls: usize,
        nesting: usize,
    ) -> io::Result<ActivityDetails> {
        if self.entries.len() > native::MAX_NODES {
            return Err(native::invalid("activity detail capacity"));
        }
        let mut entries = Vec::with_capacity(self.entries.len());
        for (position, reference) in self.entries {
            if position > calls {
                return Err(native::invalid(
                    "activity detail lies outside original calls",
                ));
            }
            entries.push((position, resources.cell(reference, shared, nesting)?));
        }
        Ok(ActivityDetails { entries })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec_cell::ExecCell;
    use crate::exec_cell::new_active_exec_command;
    use crate::history_cell::alab_retained_cells::Capture;
    use crate::history_cell::alab_retained_cells::Restored;
    #[test]
    fn external_unique_exec_box_uses_the_exact_finished_global_detail_arc() -> io::Result<()> {
        let detail: Arc<dyn HistoryCell> =
            Arc::new(crate::history_cell::ReasoningSummaryCell::new(
                "original".into(),
                "exact global detail".into(),
                std::path::Path::new("/actual"),
                true,
            ));
        let mut exec = new_active_exec_command(
            "unique-call".into(),
            vec!["cat".into()],
            vec![],
            codex_app_server_protocol::CommandExecutionSource::Agent,
            None,
            false,
        );
        exec.group.details.push(1, detail.clone());
        exec.group.details.push(1, detail.clone());
        let original: Box<dyn HistoryCell> = Box::new(exec);
        let mut capture = Capture::default();
        let reference = capture.intern(&detail)?;
        let mut text = native::Capture::default();
        let unique = capture.unique(original.as_ref(), &mut text)?;
        let bytes = capture.encode(&mut text, 1_048_576)?;
        let mut text = native::Restored::new(text.export()?)?;
        let mut resources = RestoreContext::default();
        let graph = Restored::decode(&bytes, 1_048_576, &mut text, &mut resources)?;
        let root = graph.cell(reference)?;
        let unique = graph.restore_unique(unique, &mut text, &mut resources)?;
        let exec = unique
            .as_any()
            .downcast_ref::<ExecCell>()
            .ok_or_else(|| native::invalid("unique native Exec Box changed type"))?;
        assert!(Arc::ptr_eq(&exec.group.details.entries[0].1, &root));
        assert!(Arc::ptr_eq(&exec.group.details.entries[1].1, &root));
        assert!(!Arc::ptr_eq(&root, &detail));
        Ok(())
    }
    #[test]
    fn same_original_detail_arc_remains_shared_with_a_root_and_both_activity_groups()
    -> io::Result<()> {
        let detail: Arc<dyn HistoryCell> =
            Arc::new(crate::history_cell::ReasoningSummaryCell::new(
                "actual".into(),
                "exact original reasoning".into(),
                std::path::Path::new("/actual"),
                true,
            ));
        let mut exec = new_active_exec_command(
            "call".into(),
            vec!["cat".into()],
            vec![],
            codex_app_server_protocol::CommandExecutionSource::Agent,
            None,
            false,
        );
        exec.group.details.push(1, detail.clone());
        exec.group.details.push(1, detail.clone());
        let root: Arc<dyn HistoryCell> = Arc::new(exec);
        let mut captured = Capture::default();
        let root_id = captured.intern(&root)?;
        let detail_id = captured.intern(&detail)?;
        let mut shared = native::Capture::default();
        let bytes = captured.encode(&mut shared, 1_048_576)?;
        let mut shared = native::Restored::new(shared.export()?)?;
        let restored = Restored::decode(
            &bytes,
            1_048_576,
            &mut shared,
            &mut RestoreContext::default(),
        )?;
        let root = restored.cell(root_id)?;
        let detail = restored.cell(detail_id)?;
        let exec = root
            .as_any()
            .downcast_ref::<ExecCell>()
            .ok_or_else(|| native::invalid("original native exec type changed"))?;
        assert!(Arc::ptr_eq(&exec.group.details.entries[0].1, &detail));
        assert!(Arc::ptr_eq(&exec.group.details.entries[1].1, &detail));
        Ok(())
    }
}
