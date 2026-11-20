// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Preserve finalized output separately from the actual live parser; no rendered reconstruction.
use super::super::live_output::alab_retained_live;
use super::*;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::exec_cell) struct Data {
    exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reported_status: Option<CommandExecutionStatus>,
    aggregated_output: String,
    live_output: Option<alab_retained_live::Data>,
}
impl Data {
    pub(in crate::exec_cell) fn capture(actual: &CommandOutput) -> Self {
        Self {
            exit_code: actual.exit_code,
            reported_status: actual.reported_status.clone(),
            aggregated_output: actual.aggregated_output.clone(),
            live_output: actual
                .live_output
                .as_ref()
                .map(alab_retained_live::Data::capture),
        }
    }
    pub(in crate::exec_cell) fn restore_legacy(
        mut self,
        duration: Option<std::time::Duration>,
    ) -> io::Result<CommandOutput> {
        if self.exit_code.is_none() || self.reported_status.is_some() {
            return Err(io::Error::other("invalid schema 5 command output"));
        }
        // A v5 call had no final result until it had a duration. Its preview
        // carried CommandOutput::default()'s integer zero before any process exit.
        if duration.is_none() {
            self.exit_code = None;
        }
        self.restore()
    }

    pub(in crate::exec_cell) fn restore(self) -> io::Result<CommandOutput> {
        Ok(CommandOutput {
            exit_code: self.exit_code,
            reported_status: self.reported_status,
            aggregated_output: self.aggregated_output,
            live_output: self
                .live_output
                .map(alab_retained_live::Data::restore)
                .transpose()?,
        })
    }
}

/// Field construction stays in the original model's owned descendant; no upstream visibility widening.
pub(in crate::exec_cell) fn restore_exec(
    calls: Vec<ExecCall>,
    details: crate::history_cell::ActivityDetails,
    animations_enabled: bool,
) -> ExecCell {
    ExecCell {
        group: crate::history_cell::ActivityGroup { calls, details },
        animations_enabled,
    }
}
