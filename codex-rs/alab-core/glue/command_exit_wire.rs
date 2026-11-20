// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Preserve observations alongside the required exit code and duration on the legacy wire.
use crate::items::{CommandExecutionItem, CommandExecutionStatus};
use crate::protocol::ExecCommandEndEvent;

impl CommandExecutionItem {
    pub(crate) fn legacy_exit_integer(&self) -> i32 {
        self.exit_code
            .unwrap_or(if self.status == CommandExecutionStatus::Completed {
                0
            } else {
                -1
            })
    }
}

impl ExecCommandEndEvent {
    pub fn observed_exit_code(&self) -> Option<i32> {
        (!self.exit_code_unavailable).then_some(self.exit_code)
    }

    pub fn observed_duration(&self) -> Option<std::time::Duration> {
        (!self.duration_unavailable).then_some(self.duration)
    }
}
