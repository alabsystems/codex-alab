// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::CodexThread;
use codex_thread_store::RetainedDeferredHistory;
use codex_thread_store::ThreadStoreResult;

impl CodexThread {
    /// Some only for this exact Core's actual live deferred writer. Ephemeral sessions
    /// have no such writer; their separate real in-memory journal remains authoritative.
    pub async fn retained_deferred_history(
        &self,
    ) -> ThreadStoreResult<Option<RetainedDeferredHistory>> {
        let Some(live_thread) = self.session.live_thread() else {
            return Ok(None);
        };
        live_thread.retained_deferred_history().await
    }
}

impl CodexThread {
    /// Same actual acquisition token and original frozen inherited lineage.
    /// Materialized/resumed writers return None; error never falls back empty.
    pub async fn retained_deferred_display_history(
        &self,
    ) -> ThreadStoreResult<Option<codex_thread_store::RetainedDeferredDisplayHistory>> {
        let Some(live_thread) = self.session.live_thread() else {
            return Ok(None);
        };
        live_thread.retained_deferred_display_history().await
    }
}
