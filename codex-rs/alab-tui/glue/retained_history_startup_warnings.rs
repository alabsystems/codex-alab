// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact startup-warning identity/detail collections, including pre-session state.
use super::*;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) struct Data {
    schema: u8,
    messages: Vec<String>,
    other_sources: BTreeSet<String>,
    mcp_servers: BTreeSet<String>,
    mcp_details: BTreeMap<String, Vec<String>>,
    sign_in_servers: BTreeSet<String>,
    pending_header: bool,
}
impl Data {
    pub(in crate::history_cell) fn capture(
        actual: &dyn HistoryCell,
        _graph: &mut native::Capture,
        depth: usize,
    ) -> io::Result<Option<Self>> {
        native::depth(depth)?;
        let Some(actual) = actual.as_any().downcast_ref::<StartupWarningsCell>() else {
            return Ok(None);
        };
        Ok(Some(Self {
            schema: 1,
            messages: actual.messages.clone(),
            other_sources: actual.other_sources.clone(),
            mcp_servers: actual.mcp_servers.clone(),
            mcp_details: actual.mcp_details.clone(),
            sign_in_servers: actual.sign_in_servers.clone(),
            pending_header: actual.pending_header,
        }))
    }
    pub(in crate::history_cell) fn restore(
        self,
        _graph: &mut native::Restored,
        depth: usize,
    ) -> io::Result<Box<dyn HistoryCell>> {
        native::depth(depth)?;
        if self.schema != 1
            || [
                self.messages.len(),
                self.other_sources.len(),
                self.mcp_servers.len(),
                self.mcp_details.len(),
                self.sign_in_servers.len(),
            ]
            .into_iter()
            .any(|n| n > native::MAX_NODES)
        {
            return Err(native::invalid("native startup warning schema/capacity"));
        }
        Ok(Box::new(StartupWarningsCell {
            messages: self.messages,
            other_sources: self.other_sources,
            mcp_servers: self.mcp_servers,
            mcp_details: self.mcp_details,
            sign_in_servers: self.sign_in_servers,
            pending_header: self.pending_header,
        }))
    }
}
