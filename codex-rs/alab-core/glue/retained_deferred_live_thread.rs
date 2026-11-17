// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::LiveThread;
use crate::RetainedDeferredHistory;
use crate::ThreadStoreResult;

impl LiveThread {
    /// Bounded observation of THIS acquisition, not the current writer for an ID.
    pub async fn retained_local_history(
        &self,
        max_bytes: usize,
    ) -> ThreadStoreResult<crate::RetainedLocalHistory> {
        let writer = self
            .retained_writer
            .as_ref()
            .ok_or(crate::ThreadStoreError::Unsupported {
                operation: "retained_local_history",
            })?;
        self.thread_store
            .retained_local_history(self.thread_id, writer.clone(), max_bytes)
            .await
    }

    /// Read only the actual recorder incarnation returned by THIS acquisition.
    /// Missing support supplies no snapshot; never look up a replacement by ID.
    pub async fn retained_deferred_history(
        &self,
    ) -> ThreadStoreResult<Option<RetainedDeferredHistory>> {
        let Some(writer) = &self.retained_writer else {
            return Ok(None);
        };
        self.thread_store
            .retained_deferred_history(self.thread_id, writer.clone())
            .await
    }
}

impl LiveThread {
    pub async fn retained_deferred_display_history(
        &self,
    ) -> ThreadStoreResult<Option<crate::RetainedDeferredDisplayHistory>> {
        let Some(writer) = &self.retained_writer else {
            return Ok(None);
        };
        self.thread_store
            .retained_deferred_display_history(self.thread_id, writer.clone())
            .await
    }
}
