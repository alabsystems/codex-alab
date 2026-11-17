// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original Widget owns its private pane. Only actual native panes are shared;
//! decoded timer DATA cannot construct this local ownership relationship.
use super::*;
impl ChatWidget {
    pub(crate) fn alab_handoff_original_startup_editor(
        &mut self,
        pump: &mut crate::bottom_pane::BottomPane,
    ) -> std::io::Result<()> {
        self.bottom_pane
            .alab_handoff_retained_no_primary_editor(pump)?;
        crate::startup_recovery::handed_off(|| self.bottom_pane.composer_recovery_snapshot());
        Ok(())
    }
    pub(crate) fn alab_share_original_startup_clock(
        &mut self,
        pump: &mut crate::bottom_pane::BottomPane,
    ) -> std::io::Result<()> {
        self.bottom_pane.alab_share_delayed_clock_from(pump)
    }
}
