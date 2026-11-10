// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Credential materialization is deferred until the actual source-use admission.
use super::*;
use codex_http_client::NetworkPolicy;
use codex_http_client::NetworkPolicyDenied;
#[derive(Clone)]
struct SnapshotProvider {
    snapshot: CodexAuth,
    delegate: Option<SharedAuthProvider>,
}
pub(super) fn deferred(auth: &CodexAuth) -> Option<SharedAuthProvider> {
    auth.alab_credential_epoch().epoch()?;
    Some(Arc::new(SnapshotProvider {
        snapshot: auth.clone(),
        delegate: None,
    }))
}
pub(super) fn wrap(auth: &CodexAuth, delegate: SharedAuthProvider) -> SharedAuthProvider {
    if auth.alab_credential_epoch().epoch().is_none() {
        return delegate;
    }
    Arc::new(SnapshotProvider {
        snapshot: auth.clone(),
        delegate: Some(delegate),
    })
}
pub(super) fn same_epoch(expected: &CodexAuth, current: &CodexAuth) -> bool {
    // No native membership means only the preexisting account-scoped legacy
    // provider contract. It is not native SameGrant/independence evidence.
    if !expected.alab_credential_epoch().has_native_member()
        && !current.alab_credential_epoch().has_native_member()
    {
        return true;
    }
    match (
        expected.alab_credential_epoch().epoch(),
        current.alab_credential_epoch().epoch(),
    ) {
        (Some(expected), Some(current)) => expected.same(current),
        _ => false,
    }
}
pub(super) fn manager_policy(
    provider: &AuthManagerAuthProvider,
) -> Result<Option<NetworkPolicy>, AuthError> {
    provider
        .current_auth()
        .ok_or_else(|| AuthError::Transient("managed authentication is unavailable".into()))?
        .alab_credential_epoch()
        .network_policy()
        .map_err(|_| AuthError::Policy(NetworkPolicyDenied::Unavailable))
}
impl SnapshotProvider {
    fn policy(&self) -> Result<NetworkPolicy, AuthError> {
        if let Some(delegate) = &self.delegate {
            return delegate
                .alab_source_policy()?
                .ok_or(AuthError::Policy(NetworkPolicyDenied::Unavailable));
        }
        self.snapshot
            .alab_credential_epoch()
            .network_policy()
            .map_err(|_| AuthError::Policy(NetworkPolicyDenied::Unavailable))?
            .ok_or(AuthError::Policy(NetworkPolicyDenied::Unavailable))
    }
    fn materialize(&self) -> Result<SharedAuthProvider, AuthError> {
        if let Some(delegate) = &self.delegate {
            return Ok(delegate.clone());
        }
        Ok(match &self.snapshot {
            CodexAuth::AgentIdentity(auth) => {
                Arc::new(AgentIdentityAuthProvider { auth: auth.clone() })
            }
            CodexAuth::Headers(auth) => Arc::new(HeaderAuthProvider { auth: auth.clone() }),
            CodexAuth::BedrockApiKey(_) | CodexAuth::BedrockAccessKeys(_) => {
                return Err(AuthError::Policy(NetworkPolicyDenied::UnsupportedTransport));
            }
            _ => Arc::new(BearerAuthProvider {
                token: Some(
                    self.snapshot
                        .get_token()
                        .map_err(|_| AuthError::Policy(NetworkPolicyDenied::Unavailable))?,
                ),
                account_id: self.snapshot.get_account_id(),
                is_fedramp_account: self.snapshot.is_fedramp_account(),
            }),
        })
    }
}
impl AuthProvider for SnapshotProvider {
    fn alab_source_policy(&self) -> Result<Option<NetworkPolicy>, AuthError> {
        self.policy().map(Some)
    }
    fn add_auth_headers(&self, headers: &mut HeaderMap) -> Result<(), AuthError> {
        codex_api::alab_auth_source::with_policy_sync(self.policy()?, || {
            self.materialize()?.add_auth_headers(headers)
        })
    }
    fn auth_header_telemetry(&self) -> Result<codex_api::AuthHeaderTelemetry, AuthError> {
        // Describes the selected mode without extracting credentials or signing.
        let attached = match &self.snapshot {
            CodexAuth::Headers(auth) => auth.headers().contains_key(http::header::AUTHORIZATION),
            _ => true,
        };
        Ok(codex_api::AuthHeaderTelemetry {
            attached,
            name: attached.then_some("authorization"),
        })
    }
    fn apply_auth_headers(&self, headers: HeaderMap) -> AuthHeadersFuture<'_> {
        let owned = self.clone();
        Box::pin(async move {
            codex_api::alab_auth_source::select_headers(owned.policy()?, async move {
                owned.materialize()?.apply_auth_headers(headers).await
            })
            .await
        })
    }
    fn apply_auth(&self, request: codex_http_client::Request) -> codex_api::AuthProviderFuture<'_> {
        let owned = self.clone();
        Box::pin(async move {
            codex_api::alab_auth_source::select_request(owned.policy()?, async move {
                owned.materialize()?.apply_auth(request).await
            })
            .await
        })
    }
    fn refresh_after_unauthorized(&self, rejected: HeaderMap) -> codex_api::AuthRefreshFuture<'_> {
        let owned = self.clone();
        Box::pin(async move {
            let Ok(policy) = owned.policy() else {
                return false;
            };
            codex_api::alab_auth_source::credential_action(policy, async move {
                match &owned.delegate {
                    Some(delegate) => delegate.refresh_after_unauthorized(rejected).await,
                    None => false,
                }
            })
            .await
        })
    }
}
