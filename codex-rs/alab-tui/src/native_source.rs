// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Installs private source discovery independently of presentation and Autopilot.

use codex_alab_core::managed_auth::unavailable;
use codex_alab_core::native_source as core;
use std::sync::Arc;

struct Host;

impl core::Host for Host {
    fn open_member<'a>(&'a self, open_nonce: &'a str) -> core::MemberFuture<'a> {
        Box::pin(async move {
            let endpoint =
                codex_alab_aterm::Endpoint::managed_auth_from_env().ok_or_else(unavailable)?;
            let identity = crate::identity::current().map_err(|_| unavailable())?;
            let birth = crate::identity::proc_start_utc(&identity).ok_or_else(unavailable)?;
            codex_alab_aterm::source_member::open_at(&endpoint, open_nonce, &birth).await
        })
    }

    fn source<'a>(&'a self, operation: &'a str) -> core::SourceFuture<'a> {
        Box::pin(async move {
            let endpoint =
                codex_alab_aterm::Endpoint::managed_auth_from_env().ok_or_else(unavailable)?;
            let identity = crate::identity::current().map_err(|_| unavailable())?;
            let birth = crate::identity::proc_start_utc(&identity).ok_or_else(unavailable)?;
            codex_alab_aterm::native_source::source_at(&endpoint, operation, &birth).await
        })
    }
}

pub(crate) fn install() {
    if codex_alab_core::is_alab_binary() {
        core::install(Arc::new(Host));
    }
}
