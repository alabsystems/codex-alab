// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! NoPrimary App captures the actual configured holder or actual widget state;
//! it never reconstructs an initial message from preserved CLI arguments.
use super::*;
use crate::owned_local_entered_readers::constructor::InitialMessageData;
use std::io;

impl App {
    pub(crate) fn alab_capture_no_primary_initial_message(
        &self,
    ) -> io::Result<crate::owned_local_ui_transfer::NativeSection> {
        if self.primary_thread_id.is_some() {
            return Err(io::Error::other(
                "NoPrimary capture cannot stand in for a selected App",
            ));
        }
        let held = self
            ._alab_configured
            .as_deref()
            .and_then(crate::startup_draft::alab_configured::Pending::capture_initial_message);
        if held.is_some() && self.chat_widget.initial_user_message.is_some() {
            return Err(io::Error::other(
                "configured holder and widget both own an initial message",
            ));
        }
        let original = held
            .as_ref()
            .map(InitialMessageData::message_for_capture)
            .unwrap_or_else(|| self.chat_widget.initial_user_message.clone());
        InitialMessageData::from_widget(&self.chat_widget, &original)?.section()
    }
}
