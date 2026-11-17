// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Mandatory whole-App DATA leaf for an actual pending native startup editor.
//! The original source remains owned; reading bytes cannot retire or apply it.
use super::*;
impl App {
    pub(crate) fn alab_capture_pending_startup_editor(
        &self,
        session: &crate::app_server_session::AppServerSession,
        max_bytes: usize,
    ) -> std::io::Result<Option<Box<serde_json::value::RawValue>>> {
        self._alab_startup_editor
            .as_ref()
            .map(|owner| owner.capture(session, max_bytes))
            .transpose()
    }
    pub(crate) fn alab_has_pending_startup_editor(&self) -> bool {
        self._alab_startup_editor.is_some()
    }
}
