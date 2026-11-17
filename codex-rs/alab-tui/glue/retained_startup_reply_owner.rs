// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! SAME actual pre-App reply owner, later transferred into the original App.
use super::*;
impl Constructor {
    pub(crate) fn prepare_reply_owner(
        &self,
        session: &crate::app_server_session::AppServerSession,
    ) -> io::Result<crate::app::RetainedReplyOwner> {
        self.check_current_scope()?;
        let mut slot = self
            .reply_owner
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(actual) = &*slot {
            let _same = actual.callback_owner(session)?;
            return Ok(actual.clone());
        }
        let owner = crate::app::RetainedReplyOwner::for_session(session)?;
        *slot = Some(owner.clone());
        Ok(owner)
    }
    pub(crate) fn adopt_reply_owner(
        &self,
        app: &mut crate::app::App,
        session: &crate::app_server_session::AppServerSession,
    ) -> io::Result<()> {
        app.alab_adopt_retained_reply_owner(session, self.prepare_reply_owner(session)?)
    }
    pub(crate) fn requires_configured_projection(&self) -> io::Result<bool> {
        self.check_current_scope()?;
        Ok(self
            ._actual
            .as_ref()
            .is_none_or(|actual| matches!(actual.decoded().ui_data(), UiData::NoPrimary { .. })))
    }
}
