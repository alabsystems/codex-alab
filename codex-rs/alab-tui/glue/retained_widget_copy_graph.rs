// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::copy_input_guard::alab_retained_graph as guards;
use std::io;

impl ChatWidget {
    /// Bind the existing native fixture to the actual App test channel.
    #[cfg(test)]
    pub(crate) fn alab_set_test_app_event_sender(&mut self, actual: AppEventSender) {
        self.app_event_tx = actual;
    }

    pub(crate) fn alab_capture_copy_sender(
        &self,
        capture: &mut guards::Capture,
    ) -> io::Result<guards::SenderRef> {
        capture.sender(&self.app_event_tx)
    }
    pub(crate) fn alab_capture_copy_pause(
        &self,
        capture: &guards::Capture,
    ) -> io::Result<Option<guards::GuardRef>> {
        capture.weak(&self.input_queue.transcript_copy)
    }
    pub(crate) fn alab_bind_copy_sender(
        &self,
        prepared: &mut guards::Prepared,
        sender: guards::SenderRef,
    ) -> io::Result<()> {
        prepared.bind(sender, &self.app_event_tx)
    }
    pub(crate) fn alab_restore_copy_pause(
        &mut self,
        restored: &guards::Restored,
        original: Option<guards::GuardRef>,
    ) -> io::Result<()> {
        if self.input_queue.transcript_copy.upgrade().is_some() {
            return Err(io::Error::other(
                "actual destination already owns a live copy pause",
            ));
        }
        self.input_queue.transcript_copy = restored.weak(original)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "retained_copy_input_graph_tests.rs"]
mod tests;
