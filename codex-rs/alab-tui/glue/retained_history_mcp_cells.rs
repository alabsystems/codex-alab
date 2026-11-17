// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original MCP call and inventory loading state, including actual monotonic animation clocks.
pub(in crate::history_cell) use super::computer_activity::alab_retained_computer::Data as ComputerData;
use super::result::alab_retained_result;
use super::*;
use crate::terminal_hyperlinks::alab_retained_text as native;
use codex_alab_tui::upgrade_editor_clock::Snapshot;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) enum Data {
    Call {
        original: Box<Call>,
    },
    Inventory {
        start_time: Snapshot,
        animations_enabled: bool,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::history_cell) struct Call {
    call_id: String,
    server: String,
    tool: String,
    arguments: Option<serde_json::Value>,
    start_time: Snapshot,
    duration: Option<Duration>,
    result: Option<Result<alab_retained_result::Data, String>>,
    animations_enabled: bool,
}
impl Call {
    pub(in crate::history_cell) fn capture(actual: &McpToolCallCell) -> io::Result<Self> {
        Ok(Self {
            call_id: actual.call_id.clone(),
            server: actual.invocation.server.clone(),
            tool: actual.invocation.tool.clone(),
            arguments: actual.invocation.arguments.clone(),
            start_time: Snapshot::capture(actual.start_time).map_err(native::invalid)?,
            duration: actual.duration,
            result: actual.result.as_ref().map(|result| match result {
                Ok(result) => Ok(alab_retained_result::Data::capture(result)),
                Err(error) => Err(error.clone()),
            }),
            animations_enabled: actual.animations_enabled,
        })
    }
    pub(in crate::history_cell) fn restore(self) -> io::Result<McpToolCallCell> {
        let result = self
            .result
            .map(|result| match result {
                Ok(result) => result.restore().map(Ok),
                Err(error) => Ok(Err(error)),
            })
            .transpose()?;
        Ok(McpToolCallCell {
            call_id: self.call_id,
            invocation: McpInvocation {
                server: self.server,
                tool: self.tool,
                arguments: self.arguments,
            },
            start_time: self.start_time.restore().map_err(native::invalid)?,
            duration: self.duration,
            result,
            animations_enabled: self.animations_enabled,
        })
    }
}
impl Data {
    pub(in crate::history_cell) fn capture(actual: &dyn HistoryCell) -> io::Result<Option<Self>> {
        if let Some(actual) = actual.as_any().downcast_ref::<McpToolCallCell>() {
            return Ok(Some(Self::Call {
                original: Box::new(Call::capture(actual)?),
            }));
        }
        actual
            .as_any()
            .downcast_ref::<McpInventoryLoadingCell>()
            .map(|actual| {
                Ok(Self::Inventory {
                    start_time: Snapshot::capture(actual.start_time).map_err(native::invalid)?,
                    animations_enabled: actual.animations_enabled,
                })
            })
            .transpose()
    }
    pub(in crate::history_cell) fn restore(self) -> io::Result<Box<dyn HistoryCell>> {
        match self {
            Self::Call { original } => original
                .restore()
                .map(|actual| Box::new(actual) as Box<dyn HistoryCell>),
            Self::Inventory {
                start_time,
                animations_enabled,
            } => Ok(Box::new(McpInventoryLoadingCell {
                start_time: start_time.restore().map_err(native::invalid)?,
                animations_enabled,
            })),
        }
    }
}
