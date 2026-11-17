// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Keep the same mandatory constructor/Entry DATA with the original pump.
use super::*;
use crate::owned_local_entered_readers::constructor::Constructor;
use std::sync::Arc;

impl StartupDraft {
    pub(crate) fn alab_retain_constructor(&mut self, actual: Arc<Constructor>) -> io::Result<()> {
        self.pump.alab_retain_constructor(actual)
    }
}
impl StartupDraftPump {
    fn alab_retain_constructor(&mut self, actual: Arc<Constructor>) -> io::Result<()> {
        actual.check_current_scope()?;
        if let Some(original) = &self.alab_constructor {
            if Arc::ptr_eq(original, &actual) {
                original.require_early_no_primary()?;
                return Ok(());
            }
            return Err(io::Error::other(
                "startup already owns a different original constructor",
            ));
        }
        self.alab_constructor = Some(actual.clone());
        actual.install_early_no_primary(self)?;
        Ok(())
    }
    /// Normal lower callers may build a pump directly. They retain the exact
    /// original CLI builder here before App's first await. An entered path must
    /// already own its mandatory constructor and can never reconstruct it here.
    pub(crate) fn alab_constructor_owner(
        &mut self,
        original: impl FnOnce() -> Option<crate::chatwidget::UserMessage>,
    ) -> io::Result<Arc<Constructor>> {
        if let Some(actual) = &self.alab_constructor {
            actual.check_current_scope()?;
            return Ok(actual.clone());
        }
        let actual = Constructor::direct_original(original)?;
        self.alab_retain_constructor(actual.clone())?;
        Ok(actual)
    }
    pub(crate) fn alab_cancel_original_initial_message(&mut self) -> io::Result<()> {
        if self.alab_constructor.is_none() {
            // A direct ordinary startup can reach this real consent branch
            // before App::run. Its actual cancellation is authoritative Empty.
            // An entered scope is refused by direct_original, never recreated.
            self.alab_retain_constructor(Constructor::direct_original(|| None)?)?;
        }
        let actual = self.alab_constructor.as_ref().ok_or_else(|| {
            io::Error::other("startup initial-message owner missing at original cancellation")
        })?;
        actual.cancel_from_original_policy()
    }
    /// Before Widget construction, the actual pump owns the original seed.
    /// After hold, SAME configured Pending owns the actual post-filter message.
    pub(crate) fn alab_capture_no_primary_initial_message(
        &self,
    ) -> io::Result<crate::owned_local_ui_transfer::NativeSection> {
        let actual = self.alab_constructor.as_ref().ok_or_else(|| {
            io::Error::other("NoPrimary lacks mandatory original initial-message custody")
        })?;
        actual.check_current_scope()?;
        let data = match self
            .alab_configured
            .as_deref()
            .and_then(alab_configured::Pending::capture_initial_message)
        {
            Some(data) => data,
            None => actual.capture()?,
        };
        data.section()
    }
}

impl StartupDraftPump {
    pub(crate) fn alab_has_constructor(&self, actual: &Arc<Constructor>) -> bool {
        self.alab_constructor
            .as_ref()
            .is_some_and(|original| Arc::ptr_eq(original, actual))
    }
    pub(crate) fn alab_restored_destination_changed(
        &self,
        actual: &crate::resume_picker::SessionSelection,
    ) -> io::Result<bool> {
        match &self.alab_constructor {
            Some(original) => original.restored_destination_changed(actual),
            None => Ok(false),
        }
    }
}

impl StartupDraftPump {
    pub(crate) fn alab_cancel_captured_confirmation(&self) {
        if let Some(actual) = &self.alab_constructor {
            actual.cancel_original_confirmation();
        }
    }
}

impl StartupDraft {
    pub(crate) fn alab_retain_prepared_entered_constructor(&mut self) -> io::Result<()> {
        if let Some(actual) = crate::owned_local_entered_readers::prepared_entered_constructor()? {
            self.pump.alab_retain_constructor(actual)?;
        }
        Ok(())
    }
}

impl StartupDraftPump {
    pub(crate) fn alab_share_entered_startup_clock(
        &mut self,
        widget: &mut crate::chatwidget::ChatWidget,
    ) -> io::Result<()> {
        let Some(actual) = &self.alab_constructor else {
            return Ok(());
        };
        if !actual.owns_entered_no_primary_editor()? {
            return Ok(());
        }
        // Actual pump keeps accepting input through original run_until/flush.
        // The actual widget shares this SAME live state before App replay.
        widget.alab_share_original_startup_clock(&mut self.bottom_pane)
    }
}
