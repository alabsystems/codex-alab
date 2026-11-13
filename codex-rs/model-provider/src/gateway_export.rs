//! Resolves and caches the configured gateway command's credential for this provider.

use std::fmt;
use std::sync::Arc;

use codex_http_client::NetworkPolicy;
use codex_model_provider_info::CredentialExportConfig;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use tokio::sync::Mutex;

pub(crate) struct GatewayExport {
    command: CredentialExportConfig,
    policy: NetworkPolicy,
    cached: Mutex<Option<Arc<str>>>,
}

impl fmt::Debug for GatewayExport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GatewayExport").finish_non_exhaustive()
    }
}

impl GatewayExport {
    pub(crate) fn new(command: CredentialExportConfig, policy: NetworkPolicy) -> Self {
        Self {
            command,
            policy,
            cached: Mutex::default(),
        }
    }

    pub(crate) async fn invalidate(&self) {
        *self.cached.lock().await = None;
    }

    #[expect(
        clippy::await_holding_invalid_type,
        reason = "concurrent cache misses must share a single credential export"
    )]
    pub(crate) async fn token(&self) -> Result<Arc<str>> {
        self.alab_check_policy()?; // ALAB(accounts): cache hits cannot bypass source/account retirement.
        let mut cached = self.cached.lock().await;
        self.alab_check_policy()?; // ALAB(accounts): a queued cache reader must still own the selected source.
        if let Some(token) = cached.as_ref() {
            return Ok(Arc::clone(token));
        }
        let policy = self.policy.clone(); // ALAB(accounts): captured exports never revive under a different current account.
        let permit = policy
            .acquire_for_unsupported_sdk()
            .map_err(|error| CodexErr::Fatal(error.to_string()))?;
        let output = permit
            .run(codex_login::run_credential_export(&self.command, "Gateway"))
            .await
            .map_err(|error| CodexErr::Fatal(error.to_string()))?
            .map_err(|error| CodexErr::InvalidRequest(error.to_string()))?;
        let token = std::str::from_utf8(&output)
            .ok()
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.bytes().all(|b| b.is_ascii_graphic()))
            .ok_or_else(|| {
                CodexErr::InvalidRequest(
                    "Gateway credential export must print one access token".into(),
                )
            })?;
        let token = Arc::<str>::from(token);
        *cached = Some(Arc::clone(&token));
        Ok(token)
    }
}

#[path = "../../alab-core/glue/gateway_export_scope.rs"]
mod alab_source; // ALAB(accounts): command completion and rejected-token invalidation keep exact source custody.
