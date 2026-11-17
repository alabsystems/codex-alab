// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exact Tui instance and successful terminal draw completion, not permission.
use super::Tui;
use std::sync::Arc;

pub(super) struct Draws {
    owner: Arc<()>,
    sequence: Option<u64>,
}
impl Default for Draws {
    fn default() -> Self {
        Self {
            owner: Arc::new(()),
            sequence: Some(0),
        }
    }
}
pub(crate) struct DrawMark {
    owner: Arc<()>,
    sequence: Option<u64>,
}
impl Draws {
    /// Called only after the original stdout synchronized terminal draw returns
    /// success. Saturation fails closed rather than wrapping a prior receipt.
    pub(super) fn completed(&mut self) {
        self.sequence = self.sequence.and_then(|value| value.checked_add(1));
    }
}
impl Tui {
    pub(crate) fn alab_draw_checkpoint(&self) -> DrawMark {
        DrawMark {
            owner: self.alab_retained_draws.owner.clone(),
            sequence: self.alab_retained_draws.sequence,
        }
    }
    pub(crate) fn alab_completed_draw_after(&self, before: &DrawMark) -> bool {
        Arc::ptr_eq(&before.owner, &self.alab_retained_draws.owner)
            && matches!((before.sequence, self.alab_retained_draws.sequence), (Some(before), Some(after)) if after > before)
    }
}

impl Tui {
    pub(crate) fn alab_is_current_draw(&self, actual: &DrawMark) -> bool {
        Arc::ptr_eq(&actual.owner, &self.alab_retained_draws.owner)
            && matches!((actual.sequence, self.alab_retained_draws.sequence), (Some(before), Some(after)) if before == after)
    }
}
