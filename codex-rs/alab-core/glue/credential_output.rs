// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Source-bound synchronous credential output; rendering never releases bytes.

use crate::AuthManager;
use crate::CodexAuth;
use std::io;
use std::io::Write;
use std::sync::Arc;

/// The source remains admitted until output completes or this value is dropped.
pub struct CredentialExport {
    manager: Arc<AuthManager>,
    auth: CodexAuth,
    token: String,
    _activity: codex_alab_core::assignment::Activity,
}

impl CredentialExport {
    pub(crate) fn new(
        manager: Arc<AuthManager>,
        auth: CodexAuth,
        token: String,
        activity: codex_alab_core::assignment::Activity,
    ) -> Self {
        Self {
            manager,
            auth,
            token,
            _activity: activity,
        }
    }

    /// Preserve the caller's wire format, but validate before rendering and
    /// again before delivering its complete response to the output stream.
    pub fn write_with(
        self,
        writer: &mut dyn Write,
        render: impl FnOnce(&mut Vec<u8>, &str) -> io::Result<()>,
    ) -> io::Result<()> {
        self.manager
            .alab_validate_credential_export(&self.auth, &self.token)?;
        let mut staged = Vec::new();
        render(&mut staged, &self.token)?;
        self.manager
            .alab_validate_credential_export(&self.auth, &self.token)?;
        writer.write_all(&staged)
    }
}
