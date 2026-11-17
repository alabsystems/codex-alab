// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::copy_input_guard::alab_retained_graph as guards;
use std::io;

impl App {
    pub(crate) fn alab_capture_copy_graph(
        &self,
        actual: &guards::Census,
    ) -> io::Result<guards::Capture> {
        guards::Capture::for_app(actual, &self.app_event_tx)
    }
    pub(crate) fn alab_bind_retired_copy_sender(
        &self,
        prepared: &mut guards::Prepared,
        reference: guards::SenderRef,
        actual: &guards::Census,
    ) -> io::Result<()> {
        prepared.bind_retired(reference, actual, &self.app_event_tx)
    }
}
