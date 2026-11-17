// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Derive the pre-App callback graph from SAME mandatory decoded E1 owner.
use super::*;
impl Constructor {
    pub(crate) fn prepare_startup_callbacks(
        &self,
        session: &crate::app_server_session::AppServerSession,
        pending: &Arc<crate::startup_draft::alab_configured::Pending>,
    ) -> io::Result<Option<Arc<crate::app::RetainedStartupCallbacks>>> {
        self.check_current_scope()?;
        self.require_early_no_primary()?;
        let Some(actual) = &self._actual else {
            return Ok(None);
        };
        let UiData::NoPrimary { pending_views, .. } = actual.decoded().ui_data() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "configured startup cannot substitute for an entered selected App graph",
            ));
        };
        crate::app::RetainedStartupCallbacks::restore(
            session,
            pending,
            pending_views.body(),
            self.prepare_reply_owner(session)?,
        )
        .map(Some)
    }
}
