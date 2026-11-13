// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Select the upstream gateway export for the exact primary source of one request.
use super::*;
use codex_api::AuthRefreshFuture;
use codex_api::SharedAuthProvider;
use codex_http_client::NetworkPolicy;
use std::fmt;
use std::sync::Mutex;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CacheKey(NetworkPolicy);

impl CacheKey {
    pub(crate) fn new(policy: NetworkPolicy) -> Self {
        Self(policy)
    }
    pub(crate) fn matches(&self, other: &Self) -> bool {
        self == other
    }
    pub(crate) fn policy(&self) -> NetworkPolicy {
        self.0.clone()
    }
}

impl fmt::Debug for CacheKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GatewaySourceKey([redacted])")
    }
}

type CurrentExport = Option<(CacheKey, Arc<GatewayExport>)>;

#[derive(Clone)]
pub(crate) struct Owner {
    manager: Arc<AuthManager>,
    provider: ModelProviderInfo,
    // Retain the current upstream export; this is not a second token cache.
    current: Arc<Mutex<CurrentExport>>,
}

impl fmt::Debug for Owner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GatewaySourceOwner([redacted])")
    }
}

impl Owner {
    pub(super) fn new(provider: &ModelProviderInfo, manager: &Arc<AuthManager>) -> Self {
        Self {
            manager: Arc::clone(manager),
            provider: provider.clone(),
            current: Arc::default(),
        }
    }

    async fn resolve(&self, primary: &SharedAuthProvider) -> Result<(Arc<str>, Recovery)> {
        let owner = Arc::clone(&self.manager);
        let caller = owner.application_network_policy().for_current_account();
        let policy = match primary
            .alab_source_policy()
            .map_err(|e| CodexErr::Fatal(e.to_string()))?
        {
            Some(selected) => caller.with_account_binding(&selected).ok_or_else(|| {
                CodexErr::Fatal("gateway and primary credential sources differ".into())
            })?,
            None => caller,
        };
        let key = CacheKey::new(policy.clone());
        let scope = self.clone();
        codex_api::alab_auth_source::credential_operation(policy.clone(), async move {
            let GatewayOAuthConfig::Export {
                credential_export, ..
            } = scope.provider.gateway_oauth.as_ref().ok_or_else(|| {
                CodexErr::Fatal("gateway export configuration is unavailable".into())
            })?
            else {
                return Err(CodexErr::Fatal(
                    "gateway export configuration changed".into(),
                ));
            };
            let export = {
                let mut current = scope.current.lock().map_err(|_| {
                    CodexErr::Fatal("gateway source selection is unavailable".into())
                })?;
                if let Some((_, export)) = current
                    .as_ref()
                    .filter(|(captured, _)| captured.matches(&key))
                {
                    Arc::clone(export)
                } else {
                    let export = crate::shared_state::process_shared_state().gateway_export(
                        &scope.provider,
                        credential_export,
                        &owner,
                        key.clone(),
                    );
                    *current = Some((key, Arc::clone(&export)));
                    export
                }
            };
            let token = export.token().await?;
            let recovery = Recovery {
                export,
                rejected: Arc::clone(&token),
                policy,
            };
            Ok((token, recovery))
        })
        .await
        .map_err(|error| CodexErr::Fatal(error.to_string()))?
    }
}

#[derive(Clone)]
pub(crate) struct Recovery {
    export: Arc<GatewayExport>,
    rejected: Arc<str>,
    policy: NetworkPolicy,
}

impl Recovery {
    pub(crate) fn retry(&self) -> AuthRefreshFuture<'_> {
        let captured = self.clone();
        Box::pin(async move {
            codex_api::alab_auth_source::credential_action(captured.policy.clone(), async move {
                captured
                    .export
                    .alab_invalidate_rejected(&captured.rejected)
                    .await
            })
            .await
        })
    }
}

impl GatewayAuthManager {
    pub(crate) async fn alab_resolve_for_primary(
        &self,
        primary: &SharedAuthProvider,
    ) -> Result<(Arc<str>, Option<Recovery>)> {
        match &self.0 {
            GatewayCredentialSource::Export(_) => self
                .1
                .resolve(primary)
                .await
                .map(|(token, recovery)| (token, Some(recovery))),
            GatewayCredentialSource::OAuth(_) => {
                self.resolve_access_token().await.map(|token| (token, None))
            }
        }
    }
}

#[cfg(test)]
#[path = "gateway_source_scope_tests.rs"]
mod tests;
