// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native credential scope augments existing destination restrictions; it never widens them.
use super::*;
use codex_alab_core::native_source_lifetime::NetworkScope;
use codex_alab_core::native_source_lifetime::NetworkUse;

#[derive(Clone, Default, PartialEq, Eq)]
pub(super) struct Binding(Option<Result<NetworkScope, ()>>);
impl Binding {
    pub(super) const NONE: Self = Self(None);
    pub(super) fn is_bound(&self) -> bool {
        self.0.is_some()
    }
    pub(super) fn bind(&self, credentials: &Self) -> Option<Self> {
        match (&self.0, &credentials.0) {
            (Some(Ok(left)), Some(Ok(right))) if left == right => Some(credentials.clone()),
            (Some(left), Some(right)) if left != right => None,
            (None, Some(_)) => Some(credentials.clone()),
            _ => Some(self.clone()),
        }
    }
    pub(super) fn acquire(&self) -> Result<Option<NetworkUse>, NetworkPolicyDenied> {
        match &self.0 {
            None => Ok(None),
            Some(Ok(scope)) => scope
                .acquire()
                .map(Some)
                .map_err(|_| NetworkPolicyDenied::Unavailable),
            Some(Err(())) => Err(NetworkPolicyDenied::Unavailable),
        }
    }
}
impl NetworkPolicy {
    pub fn with_native_source(mut self, source: Result<NetworkScope, ()>) -> Self {
        let next = Binding(Some(source));
        self.alab_native = self
            .alab_native
            .bind(&next)
            .unwrap_or(Binding(Some(Err(()))));
        self
    }
}
pub(super) fn check(permit: &NetworkPermit) -> Result<(), NetworkPolicyDenied> {
    if permit
        .alab_native
        .as_ref()
        .is_some_and(|source| !source.live())
    {
        Err(NetworkPolicyDenied::Revoked)
    } else {
        Ok(())
    }
}
pub(super) async fn revoked(
    permit: &NetworkPermit,
    application: impl std::future::Future<Output = ()>,
) {
    let source = async {
        match &permit.alab_native {
            Some(source) => source.revoked().await,
            None => std::future::pending().await,
        }
    };
    tokio::select! { _ = application => {}, _ = source => {} }
}

pub(super) async fn prepare(permit: &NetworkPermit) -> Result<(), NetworkPolicyDenied> {
    if let Some(source) = &permit.alab_native {
        source
            .prepare()
            .await
            .map_err(|_| NetworkPolicyDenied::Unavailable)?;
    }
    check(permit)
}

pub(super) async fn run<T>(
    permit: &NetworkPermit,
    operation: impl std::future::Future<Output = T>,
) -> Result<T, NetworkPolicyDenied> {
    prepare(permit).await?;
    Ok(operation.await)
}
