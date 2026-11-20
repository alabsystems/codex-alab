// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original Cloud queue and queued-loss publication accounting, without consuming either.
use super::CloudClient;
use std::io;
use std::sync::TryLockError;

impl CloudClient {
    pub(crate) fn alab_raw_upgrade_events_pending(&self) -> io::Result<bool> {
        // report_loss records before sending its event. The map therefore also
        // covers that original publication window, even while events is empty.
        let losses = match self.stream_losses.0.try_lock() {
            Ok(losses) => losses,
            Err(TryLockError::WouldBlock) => {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "conditional upgrade original Cloud loss publication is busy",
                ));
            }
            Err(TryLockError::Poisoned(_)) => {
                return Err(io::Error::other(
                    "conditional upgrade original Cloud loss publication is poisoned",
                ));
            }
        };
        if losses
            .values()
            .any(|entry| entry.remaining == 0 || entry.superseded > entry.remaining)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "conditional upgrade original Cloud loss accounting is inconsistent",
            ));
        }
        // Superseded losses still await the ordinary reader's filtering. Leave
        // their counters and events unchanged; a fresh attempt can follow it.
        Ok(!losses.is_empty() || !self.events.is_empty())
    }
}

#[cfg(test)]
#[path = "upgrade_cloud_events_tests.rs"]
mod tests;
