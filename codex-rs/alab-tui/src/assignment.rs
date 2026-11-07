// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native assignment transport is separate from account presentation.

use codex_alab_core::assignment_service as core;
use std::io;
use std::sync::Arc;

struct Host;
impl core::Host for Host {
    fn command<'a>(&'a self, command: &'a core::Command) -> core::ReplyFuture<'a> {
        Box::pin(async move {
            let unavailable = || io::Error::other("Native account assignment must be reconciled.");
            let endpoint =
                codex_alab_aterm::Endpoint::managed_auth_from_env().ok_or_else(unavailable)?;
            let identity = crate::identity::current().map_err(|_| unavailable())?;
            let birth = crate::identity::proc_start_utc(&identity).ok_or_else(unavailable)?;
            codex_alab_aterm::assignment::command_at(&endpoint, command, &birth).await
        })
    }
}
pub(crate) fn install() {
    if codex_alab_core::is_alab_binary() {
        core::install(Arc::new(Host));
    }
}
