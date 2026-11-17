// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual original widget fields and original confirmation comparison policy.
use super::*;
use crate::chatwidget::UserMessage;
impl ChatWidget {
    pub(crate) fn alab_original_confirmation_is_held(&self) -> bool {
        self.alab_holds_retained_startup()
    }
    pub(crate) fn alab_validate_configured_input_destination(
        &self,
        has_initial: bool,
        has_confirmation: bool,
    ) -> std::io::Result<()> {
        if (has_initial && self.initial_user_message.is_some())
            || (has_confirmation && self.input_queue.startup_submission.is_some())
        {
            return Err(std::io::Error::other(
                "configured inputs cannot overwrite newer original widget input",
            ));
        }
        Ok(())
    }
    pub(crate) fn alab_install_configured_inputs(
        &mut self,
        initial: Option<Option<UserMessage>>,
        confirmation: Option<crate::bottom_pane::ComposerDraftSnapshot>,
    ) {
        if let Some(initial) = initial {
            self.initial_user_message = initial;
        }
        if let Some(confirmation) = confirmation {
            self.input_queue.startup_submission = Some(confirmation);
        }
        // Original method cancels confirmation after edits and retains all
        // protected-input/session/turn gates. It never regenerates CLI DATA.
        self.restore_startup_input_when_ready(&mut None, &mut false);
    }
}

#[cfg(test)]
#[path = "owned_local_configured_submission_tests.rs"]
mod tests;
