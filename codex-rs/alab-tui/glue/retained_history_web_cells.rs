// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual native web-search model and original animation epoch, without tool invocation.
use super::*;
use crate::terminal_hyperlinks::alab_retained_text as native;
use codex_alab_tui::upgrade_editor_clock::Snapshot;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) struct Data {
    call_id: String,
    query: String,
    action: Option<WebSearchAction>,
    start_time: Snapshot,
    completed: bool,
    animations_enabled: bool,
}
impl Data {
    pub(in crate::history_cell) fn capture(actual: &dyn HistoryCell) -> io::Result<Option<Self>> {
        actual
            .as_any()
            .downcast_ref::<WebSearchCell>()
            .map(|actual| {
                Ok(Self {
                    call_id: actual.call_id.clone(),
                    query: actual.query.clone(),
                    action: actual.action.clone(),
                    start_time: Snapshot::capture(actual.start_time).map_err(native::invalid)?,
                    completed: actual.completed,
                    animations_enabled: actual.animations_enabled,
                })
            })
            .transpose()
    }
    pub(in crate::history_cell) fn restore(self) -> io::Result<Box<dyn HistoryCell>> {
        Ok(Box::new(WebSearchCell {
            call_id: self.call_id,
            query: self.query,
            action: self.action,
            start_time: self.start_time.restore().map_err(native::invalid)?,
            completed: self.completed,
            animations_enabled: self.animations_enabled,
        }))
    }
}
