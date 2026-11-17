// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual early provisional-editor installation, before first input pumping.
use super::*;
use crate::startup_draft::alab_retained_transfer::early;
impl Constructor {
    pub(crate) fn install_early_no_primary(
        self: &Arc<Self>,
        pump: &mut crate::startup_draft::StartupDraftPump,
    ) -> io::Result<()> {
        self.check_current_scope()?;
        let Some(actual) = &self._actual else {
            return Ok(());
        };
        let UiData::NoPrimary {
            startup,
            pending_views,
            ..
        } = actual.decoded().ui_data()
        else {
            return Ok(());
        };
        if !pump.alab_has_constructor(self) {
            return Err(invalid("early startup changed SAME original constructor"));
        }
        let mut stored = self
            .early_startup
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if stored.is_some() {
            return Err(invalid(
                "entered startup editor was already installed on its original pump",
            ));
        }
        let (bytes, generations) = crate::app::RetainedStartupCallbacks::limits_data(
            pending_views.body(),
            actual.entry().service_data(),
        )?;
        let clock = crate::app::prepare_retained_delayed_clock(
            pending_views.body(),
            actual.entry().service_data(),
            bytes,
        )?;
        let installed = early::Prepared::decode(startup.body(), bytes, generations)?
            .install_with_clock(pump, clock)?;
        *stored = Some(installed); // Only after actual native graph mutation.
        Ok(())
    }
    pub(crate) fn require_early_no_primary(&self) -> io::Result<()> {
        self.check_current_scope()?;
        if self
            ._actual
            .as_ref()
            .is_some_and(|actual| matches!(actual.decoded().ui_data(), UiData::NoPrimary { .. }))
            && self
                .early_startup
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_none()
        {
            return Err(invalid(
                "entered NoPrimary callback cut precedes real original editor installation",
            ));
        }
        Ok(())
    }
    pub(crate) fn restored_destination_changed(
        &self,
        actual: &crate::resume_picker::SessionSelection,
    ) -> io::Result<bool> {
        self.check_current_scope()?;
        Ok(self
            .early_startup
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
            .is_some_and(|installed| installed.original_destination_changed(actual)))
    }
}

impl Constructor {
    pub(crate) fn owns_entered_no_primary_editor(&self) -> io::Result<bool> {
        self.check_current_scope()?;
        let selected = self
            ._actual
            .as_ref()
            .is_some_and(|actual| matches!(actual.decoded().ui_data(), UiData::NoPrimary { .. }));
        if selected {
            self.require_early_no_primary()?;
        }
        Ok(selected)
    }
}
