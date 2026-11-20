// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

impl super::QuestionState {
    pub(crate) fn alab_has_pending_questions(&self) -> bool {
        !self.pending.is_empty()
    }
}
