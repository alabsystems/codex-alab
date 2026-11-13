// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A conversation-only Ultra selection must not erase the same model's Ultra default.

use super::App;
use super::ReasoningEffortConfig;

impl App {
    pub(super) fn alab_conversation_default_effort(
        &self,
        model: &str,
    ) -> Option<ReasoningEffortConfig> {
        if codex_alab_core::active()
            && self.config.model.as_deref() == Some(model)
            && self.config.model_reasoning_effort == Some(ReasoningEffortConfig::Ultra)
        {
            Some(ReasoningEffortConfig::Ultra)
        } else {
            self.default_reasoning_effort_for_conversation_model(model)
        }
    }
}

#[cfg(test)]
#[path = "reasoning_defaults_tests.rs"]
mod tests;
