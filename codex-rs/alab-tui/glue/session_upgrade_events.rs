// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Only this original Session's facade and already received configured FIFO.
use super::AppServerSession;
use std::io;

impl AppServerSession {
    pub(crate) fn alab_upgrade_events_pending(&self) -> io::Result<bool> {
        let actual = self.connection_owner.as_ref().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "conditional upgrade event observation requires an owning Session",
            )
        })?;
        let pending = actual.alab_upgrade_events_pending()?;
        // This FIFO exists only on Unix. Non-Unix has no corresponding field;
        // both platforms still inspect their actual physical facade above.
        #[cfg(unix)]
        let pending = pending || !self.alab_configured_events.is_empty();
        Ok(pending)
    }
}

#[cfg(all(test, unix))]
#[path = "session_upgrade_events_tests.rs"]
mod tests;
