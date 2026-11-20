// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Original ChatWidget input remains its real owner while UI startup is held.
//! This inhibitor has no Native/Source authority and changes no message DATA.
use super::ChatWidget;
use crate::AppServerTarget;
use crate::app_server_session::alab_retained_ui::Pending;
use color_eyre::eyre::Result;
use color_eyre::eyre::bail;
use std::sync::Arc;

impl ChatWidget {
    pub(crate) fn alab_bind_retained_startup(&mut self, target: &AppServerTarget) {
        self.alab_retained_startup = match target {
            AppServerTarget::OwnedLocal { target } => Some(target.clone()),
            _ => None,
        };
    }
    pub(super) fn alab_holds_retained_startup(&self) -> bool {
        self.alab_retained_startup
            .as_ref()
            .is_some_and(crate::owned_local_frontend::OwnedLocalTarget::holds_startup_submission)
    }
    pub(crate) fn alab_observed_retained_startup(&mut self, pending: &Arc<Pending>) -> Result<()> {
        let target = self
            .alab_retained_startup
            .as_ref()
            .ok_or_else(|| color_eyre::eyre::eyre!("retained startup lost its actual target"))?;
        if target
            .ui_frontier()
            .is_none_or(|actual| !Arc::ptr_eq(&actual, pending))
            || pending.holds_submission()
        {
            bail!("retained startup has no actual activation observation");
        }
        self.alab_recover_restored_finding_input();
        self.submit_initial_user_message_if_pending();
        self.maybe_send_next_queued_input();
        Ok(())
    }
}

#[path = "owned_local_configured_submission_widget.rs"]
mod configured_submission;

#[path = "retained_startup_widget_clock.rs"]
mod startup_clock;
