// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use codex_alab_release::Result;
use codex_alab_release::space::ResourceStop;
use std::error::Error;
use std::fmt;
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

#[derive(Debug)]
struct StageFailure {
    stage: &'static str,
    status: ExitStatus,
}

impl fmt::Display for StageFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} failed: {}", self.stage, self.status)
    }
}

impl Error for StageFailure {}

pub(super) fn require_success(stage: &'static str, status: ExitStatus) -> Result<()> {
    if status.success() {
        Ok(())
    } else {
        Err(StageFailure { stage, status }.into())
    }
}

/// Match certified-build.sh at each native process boundary. A native signal
/// and the same signal already encoded by a shell have the same outward status.
pub(super) fn exit_code(error: &(dyn Error + 'static)) -> Option<i32> {
    if error.is::<ResourceStop>() {
        return Some(75);
    }
    if let Some(stop) = error.downcast_ref::<codex_alab_release::protected::WorkerCancelled>() {
        return Some(stop.exit_code());
    }
    let status = error.downcast_ref::<StageFailure>()?.status;
    Some(match status.code() {
        Some(code @ (75 | 129..=255)) => code,
        None => status.signal().map_or(1, |signal| 128 + signal),
        _ => 1,
    })
}

#[cfg(test)]
#[path = "stage_status_tests.rs"]
mod tests;
