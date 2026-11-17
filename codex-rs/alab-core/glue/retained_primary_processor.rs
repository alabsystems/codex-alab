// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The existing real processor/context, not a fabricated request or client.
use super::*;
impl ThreadRequestProcessor {
    pub(crate) async fn alab_acquire_primary(
        &self,
        id: ThreadId,
        actual: Arc<CodexThread>,
        input: &codex_alab_core::native_incoming::Incoming,
    ) -> Result<
        super::super::thread_lifecycle::alab_primary_lifetime::PrimaryLease,
        JSONRPCErrorError,
    > {
        super::super::thread_lifecycle::alab_primary_lifetime::PrimaryLease::acquire(
            &self.listener_task_context(),
            id,
            actual,
            input,
        )
        .await
    }
}
