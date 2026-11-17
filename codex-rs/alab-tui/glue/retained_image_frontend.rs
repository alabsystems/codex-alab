// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Physical delivery closure and exact Entered custody only, never Current/Go.
use super::*;
impl FrontendTenure {
    /// SAME actual controller close completion only, never Current/Go or a
    /// statement that Source work/effects settled. Observers may stop on it.
    pub(crate) fn close_changes(
        &self,
    ) -> tokio::sync::watch::Receiver<Option<std::result::Result<(), io::ErrorKind>>> {
        self.delivery.completion.subscribe()
    }

    pub(crate) fn entered_image_resource(
        &self,
    ) -> io::Result<Option<Arc<codex_alab_tui::frontend_control::EnteredRetainedFrontend>>> {
        if self.delivery.closing() {
            return Err(io::Error::other("image delivery is closing"));
        }
        Ok(self.target.0.entered.clone())
    }
}
