// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original remote facade queues only; no socket reads, filtering or worker joins.
use super::RemoteAppServerClient;

impl RemoteAppServerClient {
    pub(crate) fn alab_raw_upgrade_events_pending(&self) -> bool {
        !self.pending_events.is_empty() || !self.event_rx.is_empty()
    }
}

#[cfg(test)]
#[path = "upgrade_remote_events_tests.rs"]
mod tests;
