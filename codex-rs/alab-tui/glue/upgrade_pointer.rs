// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
impl LinkPointer {
    pub(in crate::tui) fn alab_checkpoint(&self) -> u8 {
        self.0.load(Ordering::Relaxed)
    }
    pub(in crate::tui) fn alab_adopt(&self, value: u8) {
        self.0.store(value, Ordering::Relaxed);
    }
}
