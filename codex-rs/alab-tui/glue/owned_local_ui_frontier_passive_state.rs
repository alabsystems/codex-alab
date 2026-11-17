// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Read-only completion DATA correlation; no receipt/grant is constructed.
use super::*;
impl Pending {
    pub(crate) fn abandon_ui(&self) {
        self.unknown();
    }
    pub(crate) fn matches_frontier_data(&self, value: &str) -> bool {
        self.native.frontier_data() == value
    }
    pub(crate) fn received_own_tail(&self) -> bool {
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .tail
            .is_some()
    }
}
