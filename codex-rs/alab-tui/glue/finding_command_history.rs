// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use crate::app::App;
use crate::chatwidget::AlabPendingCommand;
use crate::history_cell::HistoryCell;
use crate::pager_overlay::Overlay;
use crate::tui::Tui;
use codex_protocol::ThreadId;
use color_eyre::eyre::Result;
use std::sync::Arc;

impl App {
    #[cfg(unix)]
    pub(in crate::app) fn alab_rebind_finding_commands(&mut self) {
        for cell in &self.transcript_cells {
            if let Some(pending) = cell.as_any().downcast_ref::<AlabPendingCommand>() {
                self.chat_widget.alab_restore_finding_command(pending);
            }
        }
    }

    pub(super) fn alab_finish_finding_command(
        &mut self,
        tui: &mut Tui,
        thread_id: ThreadId,
        turn_id: &str,
        call_id: &str,
        replacement: Arc<dyn HistoryCell>,
        #[cfg(unix)] source: Option<crate::app::AlabSideCellSource>,
    ) -> Result<()> {
        if self.chat_widget.thread_id() != Some(thread_id) {
            return Ok(());
        }
        let Some(index) = self.transcript_cells.iter().position(|cell| {
            cell.as_any()
                .downcast_ref::<AlabPendingCommand>()
                .is_some_and(|pending| pending.matches(thread_id, turn_id, call_id))
        }) else {
            return Ok(());
        };

        let range = index..index + 1;
        let queued = self.native_history.contains(&self.transcript_cells[index]);
        #[cfg(unix)]
        let before = source.as_ref().map(|_| self.transcript_cells.clone());
        self.native_history
            .consolidate(&self.transcript_cells[range.clone()], &replacement);
        if tui.is_owned_screen() {
            self.transcript_view
                .replace_group(&self.transcript_cells, range.clone(), &replacement);
        } else {
            self.transcript_view
                .replace_range(&self.transcript_cells, range.clone(), &replacement);
        }
        if let Some(Overlay::Transcript(overlay)) = &mut self.overlay {
            overlay.regroup_cells(range, Arc::clone(&replacement));
        }
        self.transcript_cells[index] = replacement;
        #[cfg(unix)]
        if let Some(before) = before {
            self.alab_record_side_cell_change(source, &before);
        }

        // A queued cell can print the true outcome on its first emission. Once printed,
        // the finding and any later commands may already occupy the end of scrollback.
        tui.frame_requester().schedule_frame();
        if !queued && !tui.is_owned_screen() {
            self.finish_required_stream_reflow(tui)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "finding_command_history_tests.rs"]
mod tests;
