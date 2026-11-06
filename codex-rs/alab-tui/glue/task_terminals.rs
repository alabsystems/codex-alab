// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native terminal summaries and picker state for the local Tasks view.

use super::ChatWidget;

pub(crate) struct TerminalTask {
    pub(crate) key: String,
    pub(crate) command: String,
    pub(crate) output: Vec<String>,
}

impl ChatWidget {
    pub(crate) fn alab_terminal_tasks(&self) -> Vec<TerminalTask> {
        self.unified_exec_processes
            .iter()
            .map(|process| TerminalTask {
                key: process.key.clone(),
                command: process.command_display.clone(),
                output: process.recent_chunks.clone(),
            })
            .collect()
    }

    /// ALAB(session): how many background terminals the footer counts (`unified_exec_processes`).
    pub(crate) fn alab_background_terminals(&self) -> u32 {
        u32::try_from(self.unified_exec_processes.len()).unwrap_or(u32::MAX)
    }

    pub(crate) fn alab_task_list_keymap(&self) -> crate::keymap::ListKeymap {
        self.bottom_pane.list_keymap()
    }

    pub(crate) fn alab_task_view_state(&self) -> Option<(Option<usize>, Option<String>)> {
        self.bottom_pane.alab_task_view_state()
    }
}

#[cfg(test)]
#[path = "task_terminals_tests.rs"]
mod tests;
