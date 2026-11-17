// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Child of SAME original constructor/Decoded owner. Runs after actual App
//! reply-owner bind and before first event drain. This retains DATA/operations;
//! actual Source capture/replay/draw/ACK still must occur before application.
use super::*;
impl Constructor {
    pub(crate) fn bind_callback_transfer(
        &self,
        app: &mut crate::app::App,
        session: &crate::app_server_session::AppServerSession,
    ) -> io::Result<()> {
        self.check_current_scope()?;
        let Some(actual) = &self._actual else {
            return Ok(());
        };
        let section = match actual.decoded().ui_data() {
            UiData::NoPrimary { pending_views, .. } | UiData::Selected { pending_views, .. } => {
                pending_views
            }
        };
        let _retained = app.alab_restore_callback_section(session, section.body())?;
        Ok(())
    }
}
