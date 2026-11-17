// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::request_processors::RetainedFrontierBinding;
impl MessageProcessor {
    pub(crate) fn alab_make_frontier(
        self: &Arc<Self>,
        bound: codex_alab_aterm::frontend_control::retained_service::BoundBackend,
    ) -> (Arc<RetainedFrontierBinding>, std::io::Result<()>) {
        let actual = RetainedFrontierBinding::new(bound, Arc::new(self.thread_processor.clone()));
        let installed = self
            .thread_processor
            .alab_install_frontier(&actual)
            .and_then(|()| actual.start_configured_custody());
        (actual, installed)
    }
    pub(crate) fn alab_stop_frontier(&self) {
        self.thread_processor.alab_stop_frontier();
    }
}

#[cfg(test)]
impl MessageProcessor {
    /// SAME actual effective-config policy object; test custody only.
    pub(crate) fn alab_configured_processor_for_tests(
        &self,
    ) -> crate::request_processors::ConfigRequestProcessor {
        self.config_processor.clone()
    }
}
