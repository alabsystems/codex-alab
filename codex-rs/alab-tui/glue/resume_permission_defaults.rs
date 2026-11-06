// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Carry native defaults only when building a local resume request. Direct-choice
//! classification remains upstream's: defaults are not explicit remote overrides.

impl super::ResumePermissions {
    pub(crate) fn with_alab_defaults(mut self, local: bool) -> Self {
        if local && codex_alab_tui::use_config_permissions_on_resume() {
            self.approval_policy = true;
            self.profile = true;
        }
        self
    }
}
