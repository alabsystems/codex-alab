// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Apply only the complete memory-history DATA observed from the actual
//! protected response. This never constructs a request or infers emptiness
//! from history mode, target kind, missing cursor, or Native labels.
use crate::app_server_session::AppServerSession;
use crate::app_server_session::history::ThreadHistoryPagination;
use codex_app_server_client::retained_frontier;
use codex_app_server_protocol::Thread;
use codex_protocol::ThreadId;
use color_eyre::eyre::Result;
use color_eyre::eyre::WrapErr;

impl AppServerSession {
    pub(crate) fn alab_install_complete_memory_history(&mut self, thread: &Thread) -> Result<()> {
        let id = ThreadId::from_string(&thread.id)
            .wrap_err("invalid thread id in complete memory history response")?;
        let mut pagination = ThreadHistoryPagination::default();
        pagination.history_mode = thread.history_mode;
        // No continuation exists in this authoritative complete in-memory cut.
        // The same hydrated Thread remains untouched, even when it is empty.
        self.history_pagination.insert(id, pagination);
        Ok(())
    }
    pub(crate) fn alab_hydrate_observed_memory_history(&mut self, thread: &Thread) -> Result<bool> {
        if !retained_frontier::original_complete_memory_history(&self.request_handle(), &thread.id)
        {
            return Ok(false);
        }
        self.alab_install_complete_memory_history(thread)?;
        Ok(true)
    }
}
