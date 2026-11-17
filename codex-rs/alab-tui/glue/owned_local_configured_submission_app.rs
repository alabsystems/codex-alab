// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
impl App {
    pub(crate) fn alab_release_configured_inputs(
        &mut self,
        session: &crate::app_server_session::AppServerSession,
        pending: &Arc<crate::app_server_session::alab_retained_ui::Pending>,
    ) -> Result<()> {
        let Some(configured) = self._alab_configured.clone() else {
            return Ok(());
        };
        let Some((thread, primary)) = pending.projection_target() else {
            bail!("configured input lost primary projection");
        };
        if !primary {
            return Ok(());
        } // Side completion never submits startup input.
        if self.app_server_backend_id != Some(session.backend_id())
            || self
                .primary_thread_id
                .is_none_or(|id| id.to_string() != thread)
            || self.chat_widget.thread_id() != self.primary_thread_id
        {
            bail!("configured startup input changed original primary");
        }
        configured.release_original_inputs(session, pending, &mut self.chat_widget)?;
        self._alab_configured = None;
        Ok(())
    }
}
