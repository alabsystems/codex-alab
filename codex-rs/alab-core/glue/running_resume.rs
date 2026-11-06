// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Fence the live metadata projection before an ALab running resume reads cursors.
//! The caller has already read a stored thread, so this never activates an untouched
//! or ephemeral thread. Upstream's inactive and legacy paths keep their own barriers.

use std::future::Future;
use std::io;

use codex_app_server_protocol::JSONRPCErrorError;
use codex_core::CodexThread;
use codex_thread_store::ThreadStoreError;

pub(super) async fn prepare_projection(
    thread: &CodexThread,
    paginated: bool,
) -> Result<(), JSONRPCErrorError> {
    prepare_when(codex_alab_core::active(), paginated, || {
        thread.checkpoint_preparation()
    })
    .await
}

async fn prepare_when<F: Future<Output = io::Result<()>>>(
    active: bool,
    paginated: bool,
    checkpoint: impl FnOnce() -> F,
) -> Result<(), JSONRPCErrorError> {
    if active && paginated {
        checkpoint().await.map_err(resume_error)?;
    }
    Ok(())
}

fn resume_error(error: io::Error) -> JSONRPCErrorError {
    match error
        .get_ref()
        .and_then(|source| source.downcast_ref::<ThreadStoreError>())
    {
        Some(error) => super::thread_store_resume_read_error(error.clone()),
        None => super::internal_error(format!("failed to read thread: {error}")),
    }
}

#[cfg(test)]
#[path = "running_resume_tests.rs"]
mod tests;
