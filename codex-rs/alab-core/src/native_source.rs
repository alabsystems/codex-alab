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

pub const LAUNCH_ENV: &str = "ATERM_CODEX_SOURCE_LAUNCH";

/// Request correlation entropy only, never native member or borrowing authority.
pub fn connection_request_prefix() -> u64 {
    rand::random()
}

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
            || (self.assignment_generation == 0) != self.native_intent.is_none()
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

/// Native-issued opaque borrowing context. The server checks the real retained guard;
/// operation/boundary strings alone cannot construct borrowing authority.
pub struct Borrow(zeroize::Zeroizing<String>);
impl Borrow {
    pub fn from_native(value: String) -> io::Result<Self> {
        if !valid_hex(&value, 32) && !valid_hex(&value, 64) {
            return Err(unavailable());
        }
        Ok(Self(zeroize::Zeroizing::new(value)))
    }
    /// Only an authenticated private transport serializes this value. Never log it.
    pub fn private_value(&self) -> &str {
        self.0.as_str()
    }
}
impl std::fmt::Debug for Borrow {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeSourceBorrow(<private>)")
    }
}

#[derive(Clone)]
pub struct Preload {
    pub registration: String,
    pub source: Source,
}
impl Preload {
    pub fn validate(&self, operation: &str) -> io::Result<()> {
        self.source.validate()?;
        if self.source.operation != operation || !valid_hex(&self.registration, 32) {
            return Err(unavailable());
        }
        Ok(())
    }
}
impl std::fmt::Debug for Preload {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeSourcePreload(<private>)")
    }
}

/// Actual selected non-native credential mechanism. This attests no native file reader;
/// it grants no native source, refresh ownership or independent OAuth lineage authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoNativeReader {
    ApiKey,
    External,
    Headers,
    AgentIdentity,
    PersonalAccessToken,
    BedrockApiKey,
    BedrockAccessKeys,
    Workload,
    NoCredentials,
}

/// A no-cache observation is explicit; it cannot be represented by fabricated source generations.
pub enum Loaded {
    /// A verified direct reader has no manager AuthChangeState counters.
    ReaderSource {
        source: Box<Source>,
    },
    Source {
        source: Box<Source>,
        generations: crate::assignment::Generations,
    },
    PolicyUnavailable,
    NoNativeReader(NoNativeReader),
}
impl std::fmt::Debug for Loaded {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeSourceLoaded(<private>)")
    }
}

/// An actual issued registration, including successful constructor completion without a
/// native reader. It is not a SourceHandle or grant proof.
#[derive(Clone)]
pub struct ReadyReceipt {
    pub registration: String,
}
impl ReadyReceipt {
    pub fn validate(&self, expected: Option<&str>) -> io::Result<()> {
        if !valid_hex(&self.registration, 32)
            || expected.is_some_and(|registration| registration != self.registration)
        {
            return Err(unavailable());
        }
        Ok(())
    }
}
impl std::fmt::Debug for ReadyReceipt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeSourceReady(<private>)")
    }
}

pub type ReadyFuture<'a> = Pin<Box<dyn Future<Output = io::Result<ReadyReceipt>> + Send + 'a>>;

pub type MemberFuture<'a> = Pin<Box<dyn Future<Output = io::Result<Arc<dyn Member>>> + Send + 'a>>;
pub type PreloadFuture<'a> = Pin<Box<dyn Future<Output = io::Result<Preload>> + Send + 'a>>;
pub type UnitFuture<'a> = Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'a>>;

/// Metadata-only retirement body. None registration is lost-ACK correlation,
/// never permission to retire a different preload.
#[derive(Clone)]
pub struct Retirement {
    pub operation: String,
    pub preload_operation: Option<String>,
    pub registration: Option<String>,
}
impl Retirement {
    pub fn validate(&self) -> io::Result<()> {
        if !valid_hex(&self.operation, 32)
            || self
                .preload_operation
                .as_ref()
                .is_some_and(|value| !valid_hex(value, 32))
            || self
                .registration
                .as_ref()
                .is_some_and(|value| !valid_hex(value, 32))
        {
            return Err(unavailable());
        }
        Ok(())
    }
}
impl std::fmt::Debug for Retirement {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeMemberRetirement(<private>)")
    }
}

/// One native-issued member on a retained authenticated connection. Storage, auth-derived
/// HTTP bodies and WebSockets share its lifetime. EOF is Unknown, never implicit retirement.
pub trait Member: Send + Sync {
    fn id(&self) -> &str;
    fn preload<'a>(&'a self, operation: &'a str, borrow: Option<&'a Borrow>) -> PreloadFuture<'a>;
    fn ready<'a>(
        &'a self,
        operation: &'a str,
        registration: Option<&'a str>,
        loaded: &'a Loaded,
    ) -> ReadyFuture<'a>;
    fn open_derived<'a>(
        &'a self,
        _operation: &'a str,
        _use_receipt: &'a crate::native_source_use::UseReceipt,
    ) -> MemberFuture<'a> {
        Box::pin(async { Err(unavailable()) })
    }
    fn refresh_use<'a>(
        &'a self,
        _request: &'a crate::managed_auth::Request,
        _use_receipt: &'a crate::native_source_use::UseReceipt,
    ) -> crate::managed_auth::RefreshFuture<'a> {
        Box::pin(async { Err(unavailable()) })
    }
    fn preload_use<'a>(
        &'a self,
        _operation: &'a str,
        _use_receipt: &'a crate::native_source_use::UseReceipt,
    ) -> PreloadFuture<'a> {
        Box::pin(async { Err(unavailable()) })
    }
    fn ready_use<'a>(
        &'a self,
        _operation: &'a str,
        _registration: &'a str,
        _loaded: &'a Loaded,
        _use_receipt: &'a crate::native_source_use::UseReceipt,
    ) -> ReadyFuture<'a> {
        Box::pin(async { Err(unavailable()) })
    }
    fn begin_use<'a>(
        &'a self,
        _attempt: &'a crate::native_source_use::UseAttempt,
        _registration: &'a str,
    ) -> crate::native_source_use::UseFuture<'a> {
        Box::pin(async { Err(unavailable()) })
    }
    fn close_use<'a>(
        &'a self,
        _body: &'a crate::native_source_use::UseBody,
    ) -> crate::native_source_use::UseFuture<'a> {
        Box::pin(async { Err(unavailable()) })
    }
    fn end_use<'a>(&'a self, _receipt: &'a crate::native_source_use::UseReceipt) -> UnitFuture<'a> {
        Box::pin(async { Err(unavailable()) })
    }
    fn retire_captured<'a>(&'a self, request: &'a Retirement) -> UnitFuture<'a> {
        Box::pin(async move {
            request.validate()?;
            if request.preload_operation.is_some() {
                return Err(unavailable());
            }
            self.retire(request.registration.as_deref()).await
        })
    }
    /// None retires an issued allocation that never obtained a preload registration.
    fn retire<'a>(&'a self, registration: Option<&'a str>) -> UnitFuture<'a>;
    /// Transport EOF/revocation only; true is not Ready, grant authority or cohort coverage.
    fn live(&self) -> bool;
    fn closed(&self) -> UnitFuture<'_> {
        Box::pin(async move {
            while self.live() {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            Ok(())
        })
    }
}

pub type SourceFuture<'a> = Pin<Box<dyn Future<Output = io::Result<Source>> + Send + 'a>>;

pub trait Host: Send + Sync {
    /// Read the current bound source on an authenticated native connection.
    fn source<'a>(&'a self, operation: &'a str) -> SourceFuture<'a>;
    fn open_member<'a>(&'a self, _open_nonce: &'a str) -> MemberFuture<'a> {
        Box::pin(async { Err(unavailable()) })
    }
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

/// Allocate through the authenticated native server before any managed credential read.
pub async fn open_member(open_nonce: &str) -> io::Result<Arc<dyn Member>> {
    if !valid_hex(open_nonce, 32) {
        return Err(unavailable());
    }
    let member = HOST
        .get()
        .ok_or_else(unavailable)?
        .open_member(open_nonce)
        .await?;
    if !valid_hex(member.id(), 32) || !member.live() {
        return Err(unavailable());
    }
    Ok(member)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    struct AllocationHost(AtomicUsize);
    struct Allocation {
        id: String,
        live: bool,
    }
    impl Host for AllocationHost {
        fn source<'a>(&'a self, _: &'a str) -> SourceFuture<'a> {
            Box::pin(async { Err(unavailable()) })
        }
        fn open_member<'a>(&'a self, _: &'a str) -> MemberFuture<'a> {
            Box::pin(async move {
                let request = self.0.fetch_add(1, Ordering::Relaxed);
                Ok(Arc::new(Allocation {
                    id: if request == 0 {
                        "not-issued-id".into()
                    } else {
                        "b".repeat(32)
                    },
                    live: request != 1,
                }) as Arc<dyn Member>)
            })
        }
    }
    impl Member for Allocation {
        fn id(&self) -> &str {
            &self.id
        }
        fn live(&self) -> bool {
            self.live
        }
        fn preload<'a>(&'a self, _: &'a str, _: Option<&'a Borrow>) -> PreloadFuture<'a> {
            Box::pin(async { Err(unavailable()) })
        }
        fn ready<'a>(&'a self, _: &'a str, _: Option<&'a str>, _: &'a Loaded) -> ReadyFuture<'a> {
            Box::pin(async { Err(unavailable()) })
        }
        fn retire<'a>(&'a self, _: Option<&'a str>) -> UnitFuture<'a> {
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn member_allocation_rejects_wrong_native_id_and_eof_without_fabricating_source() {
        install(Arc::new(AllocationHost(AtomicUsize::new(0))));
        assert!(open_member("invalid-nonce").await.is_err());
        assert!(open_member(&"a".repeat(32)).await.is_err());
        assert!(open_member(&"a".repeat(32)).await.is_err());
        let member = open_member(&"a".repeat(32)).await.unwrap();
        assert_eq!(member.id(), "b".repeat(32));
        assert!(member.preload(&"c".repeat(32), None).await.is_err());
        assert!(
            member
                .ready(&"c".repeat(32), None, &Loaded::PolicyUnavailable)
                .await
                .is_err()
        );
        member.retire(None).await.unwrap();
    }

    #[test]
    fn private_borrow_and_ready_receipts_are_redacted_and_exactly_correlated() {
        let value = "d".repeat(32);
        let borrow = Borrow::from_native(value.clone()).unwrap();
        assert!(!format!("{borrow:?}").contains(&value));
        assert!(Borrow::from_native("operation=not-authority".into()).is_err());
        let receipt = ReadyReceipt {
            registration: "e".repeat(32),
        };
        assert!(receipt.validate(None).is_ok());
        assert!(receipt.validate(Some(&"e".repeat(32))).is_ok());
        assert!(receipt.validate(Some(&"f".repeat(32))).is_err());
        assert!(!format!("{receipt:?}").contains(&receipt.registration));
    }
}
