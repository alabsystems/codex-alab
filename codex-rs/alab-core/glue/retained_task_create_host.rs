// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Capture actual same-host processors for the complete CREATE work body.
use crate::request_processors::retained_task_create::BackendCreateExecutor;
use codex_alab_core::native_incoming::Incoming;

impl super::MessageProcessor {
    pub(crate) fn alab_backend_create_executor(&self, incoming: Incoming) -> BackendCreateExecutor {
        BackendCreateExecutor {
            starts: self
                .thread_processor
                .alab_backend_start_executor(incoming.clone()),
            accounts: self.account_processor.clone(),
            catalog: self.catalog_processor.clone(),
            config: self.config_processor.clone(),
            turns: self.turn_processor.alab_backend_initial_turn(incoming),
        }
    }
}
