// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! SQLite completeness accounting for observed log-entry enqueue rejection.

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// Mark observed queue loss without blocking or invoking a failure reporter.
/// Existing incompleteness remains sticky across writes and mode changes.
pub fn mark_entry_loss(incomplete: &AtomicBool) {
    if crate::active() {
        incomplete.store(true, Ordering::Relaxed);
    }
}
