// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Read only the original facade-owned event buffers; never poll their consumers.
use super::AppServerClient;
use super::InProcessAppServerClient;
use std::io;

impl AppServerClient {
    /// A conditional admission precondition, not transport liveness or an H1 cut.
    /// Unavailable queue observations refuse; true leaves every original event queued.
    pub fn alab_upgrade_events_pending(&self) -> io::Result<bool> {
        match self {
            Self::InProcess(actual) => Ok(actual.alab_raw_upgrade_events_pending()),
            Self::Remote(actual) => Ok(actual.alab_raw_upgrade_events_pending()),
            Self::Cloud(actual) => actual.alab_raw_upgrade_events_pending(),
        }
    }
}

impl InProcessAppServerClient {
    fn alab_raw_upgrade_events_pending(&self) -> bool {
        !self.event_rx.is_empty()
    }
}

#[cfg(test)]
#[path = "upgrade_client_events_tests.rs"]
mod tests;
