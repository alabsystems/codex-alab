// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Private native credential-source receipts. These never enter UI or conversation data.

use crate::managed_auth::Revision;
use crate::managed_auth::unavailable;
use crate::managed_auth::valid_hex;
use crate::managed_auth::valid_slot;
use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::OnceLock;

/// Metadata authenticated by the owning native connection, not credential contents.
#[derive(Clone, PartialEq, Eq)]
pub struct Source {
    pub operation: String,
    pub slot: String,
    pub path: PathBuf,
    pub account: String,
    pub user: String,
    pub revision: Revision,
    pub assignment_generation: u64,
    pub native_intent: Option<String>,
}

impl std::fmt::Debug for Source {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeSource(<private>)")
    }
}

impl Source {
    pub fn validate(&self) -> io::Result<()> {
        if !valid_hex(&self.operation, 32)
            || !valid_slot(&self.slot)
            || !self.path.is_absolute()
            || self.path.as_os_str().len() > 4096
            || self
                .path
                .to_str()
                .is_none_or(|path| path.chars().any(char::is_control))
            || self
                .path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
            || !valid_hex(&self.account, 64)
            || !valid_hex(&self.user, 64)
            || self.revision.generation == 0
            || !valid_hex(&self.revision.digest, 64)
            || self
                .native_intent
                .as_ref()
                .is_some_and(|intent| !valid_hex(intent, 32))
        {
            return Err(unavailable());
        }
        Ok(())
    }
}

pub type SourceFuture<'a> = Pin<Box<dyn Future<Output = io::Result<Source>> + Send + 'a>>;

pub trait Host: Send + Sync {
    /// Read the current bound source on an authenticated native connection.
    fn source<'a>(&'a self, operation: &'a str) -> SourceFuture<'a>;
}

static HOST: OnceLock<Arc<dyn Host>> = OnceLock::new();

pub fn install(host: Arc<dyn Host>) {
    let _ = HOST.set(host);
}

pub async fn source(operation: &str) -> io::Result<Source> {
    if !valid_hex(operation, 32) {
        return Err(unavailable());
    }
    let source = HOST
        .get()
        .ok_or_else(unavailable)?
        .source(operation)
        .await?;
    source.validate()?;
    if source.operation != operation {
        return Err(unavailable());
    }
    Ok(source)
}
