// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Use the ordinary store-owned resume reader without reloading configuration.
use super::ThreadRequestProcessor;
use codex_protocol::ThreadId;
use codex_rollout::InitialHistory;

impl ThreadRequestProcessor {
    pub(crate) async fn alab_exec_history(
        &self,
        thread: ThreadId,
    ) -> Result<InitialHistory, String> {
        let stored = self
            .read_stored_thread_for_resume(&thread.to_string(), None, false)
            .await
            .map_err(|_| "exec bootstrap stored thread is unavailable".to_owned())?;
        let (history, _) = self
            .load_resume_initial_history_from_stored_thread(stored, None)
            .await
            .map_err(|_| "exec bootstrap stored history is unavailable".to_owned())?;
        if !matches!(&history, InitialHistory::Resumed(resumed) if resumed.conversation_id == thread)
        {
            return Err("exec bootstrap stored history changed identity".into());
        }
        Ok(history)
    }
}
