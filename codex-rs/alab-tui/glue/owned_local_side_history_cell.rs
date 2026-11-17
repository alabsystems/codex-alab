// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual original item context is carried with the SAME FIFO cell.
use super::*;
use crate::app::AlabSideCellSource;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
#[derive(Default)]
pub(super) struct SourceContext(Arc<Mutex<Option<AlabSideCellSource>>>);
pub(super) struct Guard {
    state: Arc<Mutex<Option<AlabSideCellSource>>>,
    prior: Option<AlabSideCellSource>,
}
impl SourceContext {
    pub(super) fn enter(
        &self,
        enabled: bool,
        thread: Option<ThreadId>,
        turn: &str,
        item: &ThreadItemPayload,
    ) -> Guard {
        let prior = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(AlabSideCellSource {
                thread,
                turn: turn.to_owned(),
                payload: item.clone(),
            });
        if !enabled {
            *self.0.lock().unwrap_or_else(PoisonError::into_inner) = None;
        }
        Guard {
            state: self.0.clone(),
            prior,
        }
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = self.prior.take();
    }
}
impl ChatWidget {
    pub(super) fn alab_current_history_source(&self) -> Option<AlabSideCellSource> {
        self.alab_history_source
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    pub(super) fn alab_emit_sourced_history(&self, cell: Box<dyn HistoryCell>) {
        let source = self
            .alab_history_source
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        self.app_event_tx.send(match source {
            Some(source) => AppEvent::AlabInsertSideHistory { source, cell },
            None => AppEvent::InsertHistoryCell(cell),
        });
    }
}

impl ChatWidget {
    pub(crate) fn alab_side_command_projection(
        command: &str,
        actions: &[codex_app_server_protocol::CommandAction],
    ) -> (
        Vec<String>,
        Vec<codex_protocol::parse_command::ParsedCommand>,
    ) {
        command_execution_command_and_parsed(command, actions)
    }
}
