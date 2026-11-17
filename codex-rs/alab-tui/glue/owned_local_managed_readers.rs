// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Original managed-loader construction on ordinary targets; actual frozen
//! backend policy DATA on retained targets. No default ConfigBuilder, local
//! credential read or AuthManager is created in the retained branch.
use crate::AppServerTarget;
use crate::app_server_session::AppServerSession;
use codex_config::CloudConfigBundleLoader;
use codex_config::ConfigRequirementsToml;
use codex_login::AuthConfig;
use std::io;

pub(crate) async fn startup_loader(
    target: &AppServerTarget,
    auth: AuthConfig,
    requirements: &ConfigRequirementsToml,
    enable_api_env: bool,
) -> io::Result<CloudConfigBundleLoader> {
    #[cfg(unix)]
    if let AppServerTarget::OwnedLocal { target } = target {
        return Ok(target.snapshot_for_transfer()?.ui_cloud_loader());
    }
    #[cfg(not(unix))]
    let _ = target;
    codex_cloud_config::managed_config_bundle_loader_for_storage(auth, requirements, enable_api_env)
        .await
}

pub(crate) async fn after_login_loader(
    target: &AppServerTarget,
    actual: Option<&AppServerSession>,
    auth: AuthConfig,
    requirements: &ConfigRequirementsToml,
    enable_api_env: bool,
) -> io::Result<CloudConfigBundleLoader> {
    #[cfg(unix)]
    if let AppServerTarget::OwnedLocal { target } = target {
        return retained_after_login(
            target,
            actual.ok_or_else(|| io::Error::other("original post-login session unavailable"))?,
        )
        .await;
    }
    #[cfg(not(unix))]
    let _ = (target, actual);
    codex_cloud_config::managed_config_bundle_loader_for_storage(auth, requirements, enable_api_env)
        .await
}

#[cfg(unix)]
async fn retained_after_login(
    target: &super::owned_local_frontend::OwnedLocalTarget,
    actual: &AppServerSession,
) -> io::Result<CloudConfigBundleLoader> {
    let tenure = actual
        .retained_frontend()
        .ok_or_else(|| io::Error::other("retained loader lost actual physical tenure"))?;
    if &tenure.target != target {
        return Err(io::Error::other("retained loader changed actual target"));
    }
    let request = codex_app_server_protocol::ClientRequest::ConfigRead {
        request_id: codex_app_server_protocol::RequestId::String(format!(
            "retained-managed-{:016x}{:016x}",
            codex_alab_core::native_source::connection_request_prefix(),
            codex_alab_core::native_source::connection_request_prefix()
        )),
        // This new managed-DATA read does not project/rebuild a thread or
        // override any original user cwd/config/policy input.
        params: codex_app_server_protocol::ConfigReadParams {
            include_layers: false,
            cwd: None,
        },
    };
    let actual_data = codex_app_server_client::retained_readers::read_managed_snapshot(
        &actual.request_handle(),
        target.service_data(),
        request,
    )
    .await
    .map_err(io::Error::other)?
    .into_cloud_data();
    let loader = actual_data.frozen_loader();
    target.observe_managed_cloud(actual_data)?;
    Ok(loader)
}

/// Actual existing Remote+Native tenure for observations; ordinary callers keep
/// their original AuthConfig load. This local routing owner is not authority.
#[derive(Clone, Default)]
pub(crate) struct AccountReader {
    #[cfg(unix)]
    actual: Option<(codex_app_server_client::AppServerRequestHandle, String)>,
}
impl std::fmt::Debug for AccountReader {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("AccountReader(<original same-session observation route>)")
    }
}
impl AccountReader {
    pub(crate) fn for_session(actual: &AppServerSession) -> Self {
        #[cfg(unix)]
        if let Some(tenure) = actual.retained_frontend() {
            return Self {
                actual: Some((actual.request_handle(), tenure.target.service_data().into())),
            };
        }
        let _ = actual;
        Self::default()
    }
    pub(crate) async fn observe(
        &self,
        original: &AuthConfig,
    ) -> io::Result<codex_app_server_client::retained_readers::AccountData> {
        #[cfg(unix)]
        if let Some((handle, service)) = &self.actual {
            return codex_app_server_client::retained_readers::read_account(
                handle,
                service,
                observational_request(),
            )
            .await
            .map_err(io::Error::other);
        }
        let auth = original.load_auth(false).await?;
        Ok(codex_app_server_client::retained_readers::AccountData::from_actual(auth.as_ref()))
    }
}
#[cfg(unix)]
fn observational_request() -> codex_app_server_protocol::ClientRequest {
    codex_app_server_protocol::ClientRequest::ConfigRead {
        request_id: codex_app_server_protocol::RequestId::String(format!(
            "retained-reader-{:016x}{:016x}",
            codex_alab_core::native_source::connection_request_prefix(),
            codex_alab_core::native_source::connection_request_prefix()
        )),
        params: codex_app_server_protocol::ConfigReadParams {
            include_layers: false,
            cwd: None,
        },
    }
}

/// Ordinary foreground OAuth keeps the complete original awaited operation.
/// Only the genuine decoded-entry route uses SAME backend frozen policy.
pub(crate) async fn ensure_original_config_server_login(
    original_auth: impl FnOnce() -> io::Result<AuthConfig>,
    requirements: &ConfigRequirementsToml,
) -> io::Result<()> {
    #[cfg(unix)]
    {
        crate::owned_local_entered_readers::ensure_config_server_login(original_auth, requirements)
            .await
    }
    #[cfg(not(unix))]
    {
        codex_cloud_config::ensure_config_server_login(original_auth()?, requirements).await
    }
}
