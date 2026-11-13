//! Resolves credentials for a gateway, using its configured exporter or OAuth session.

use std::sync::Arc;

use crate::ProviderUnauthorizedRecovery;
use crate::gateway_export::GatewayExport;
use codex_login::AuthManager;
use codex_login::GatewayAuthError;
use codex_login::OAuthSessionManager;
use codex_model_provider_info::GatewayOAuthConfig;
use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::error::CodexErr;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::error::Result;

/// Resolves gateway request credentials and exposes the optional interactive OAuth session.
#[derive(Debug)]
pub struct GatewayAuthManager(GatewayCredentialSource, alab_source::Owner); // ALAB(accounts): retain source selection independently from captured exports.
#[path = "../../alab-core/glue/gateway_source_scope.rs"] pub(crate) mod alab_source; // ALAB(accounts): per-primary source selection uses the canonical upstream cache.

#[derive(Debug)]
enum GatewayCredentialSource {
    Export(Arc<GatewayExport>),
    OAuth(Arc<OAuthSessionManager>),
}

impl GatewayAuthManager {
    pub(crate) fn new(
        provider: &ModelProviderInfo,
        config: &GatewayOAuthConfig,
        owner: &Arc<AuthManager>,
    ) -> std::result::Result<Self, String> {
        let state = crate::shared_state::process_shared_state();
        let source = match config {
            GatewayOAuthConfig::Export {
                credential_export, ..
            } => GatewayCredentialSource::Export(state.gateway_export(
                provider,
                credential_export,
                owner,
                alab_source::CacheKey::new(owner.application_network_policy().for_current_account()), // ALAB(accounts): cache entries retain their exact source/account.
            )),
            GatewayOAuthConfig::OAuth(settings) => GatewayCredentialSource::OAuth(
                state
                    .gateway_oauth_session(settings, &owner.runtime_config())
                    .map_err(|_| "failed to create provider OAuth HTTP client".to_string())?,
            ),
        };
        Ok(Self(source, alab_source::Owner::new(provider, owner))) // ALAB(accounts): no retained credential is rebound by a source transition.
    }

    pub(crate) async fn prepare_retry(&self) -> ProviderUnauthorizedRecovery {
        match &self.0 {
            GatewayCredentialSource::Export(export) => {
                export.invalidate().await;
                ProviderUnauthorizedRecovery::Recovered
            }
            GatewayCredentialSource::OAuth(_) => ProviderUnauthorizedRecovery::NotConfigured,
        }
    }

    /// The session used for explicit sign-in and OAuth status, when configured.
    pub fn oauth_session(&self) -> Option<Arc<OAuthSessionManager>> {
        match &self.0 {
            GatewayCredentialSource::OAuth(session) => Some(Arc::clone(session)),
            GatewayCredentialSource::Export(_) => None,
        }
    }

    pub(crate) fn without_interactive_login(self: &Arc<Self>) -> Arc<Self> {
        let GatewayCredentialSource::OAuth(session) = &self.0 else {
            return Arc::clone(self);
        };
        Arc::new(Self(GatewayCredentialSource::OAuth(Arc::new(
            session.as_ref().clone().without_interactive_login(),
        )), self.1.clone())) // ALAB(accounts): catalog OAuth clones preserve source selection ownership.
    }

    pub async fn resolve_access_token(&self) -> Result<Arc<str>> {
        match &self.0 {
            GatewayCredentialSource::Export(export) => export.alab_owned_token().await, // ALAB(accounts): caller cancellation cannot release source custody before the command ends.
            GatewayCredentialSource::OAuth(session) => session
                .resolve_access_token()
                .await
                .map(Arc::from)
                .map_err(gateway_auth_error),
        }
    }
}

fn gateway_auth_error(error: std::io::Error) -> CodexErr {
    if let Some(login_error) = error
        .get_ref()
        .and_then(|source| source.downcast_ref::<GatewayAuthError>())
    {
        return match login_error {
            GatewayAuthError::LoginRequired => {
                CodexErrorDetails::GatewayLoginRequired(login_error.to_string()).into()
            }
            GatewayAuthError::LoginInProgress => CodexErr::InvalidRequest(login_error.to_string()),
        };
    }
    // Issuer errors may echo arbitrary credentials from configured URLs. Keep the
    // diagnostic safe and bounded, preserving retries for transient failures.
    CodexErr::Io(std::io::Error::new(
        error.kind(),
        "Gateway OAuth authentication failed; check the gateway configuration and credential store.",
    ))
}
