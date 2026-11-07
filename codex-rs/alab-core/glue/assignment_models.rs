// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Catalog I/O belongs to the same real manager boundary as model and Apps traffic.

pub(super) fn admit(
    manager: Option<&codex_login::AuthManager>,
) -> codex_protocol::error::Result<Option<codex_alab_core::assignment::Activity>> {
    manager
        .map(|manager| {
            manager
                .alab_assignment_domain()
                .admit_current_resource()
                .map_err(|_| {
                    codex_protocol::error::CodexErr::InvalidRequest(
                        "Account assignment is pending; catalog preparation has been deferred."
                            .into(),
                    )
                })
        })
        .transpose()
}
