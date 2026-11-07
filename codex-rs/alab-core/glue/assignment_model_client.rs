// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

impl super::ModelClient {
    /// The coordinator calls this only after all admitted requests have drained.
    pub(crate) fn alab_retire_account_transport(&self) {
        let mut websocket = self.take_cached_websocket_session();
        websocket.reset(Some("other"));
        drop(websocket);
        self.state
            .last_inference_tools
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }
}
