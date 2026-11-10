// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Constructor membership is independent of startup files and presentation. The
//! manager's cache cannot escape until its actual selected source is acknowledged.

use super::*;
use codex_alab_core::native_source::NoNativeReader;
use codex_alab_core::native_source::Source;
use codex_alab_core::native_source_lifetime::Lifetime;

tokio::task_local! {
    static SELECTED_SOURCE: Option<NoNativeReader>;
}
pub(super) fn selected_source() -> Option<NoNativeReader> {
    SELECTED_SOURCE
        .try_with(|selected| *selected)
        .ok()
        .flatten()
}
pub(super) async fn configured_source<F: std::future::Future>(
    selected: Option<NoNativeReader>,
    future: F,
) -> F::Output {
    SELECTED_SOURCE.scope(selected, future).await
}

pub(super) enum Initial {
    Deferred,
    Disabled,
    Opened {
        member: Lifetime,
        preload: Mutex<Option<Box<codex_alab_core::native_source::Preload>>>,
    },
    Refused,
}
impl Initial {
    pub async fn begin() -> Self {
        if !super::alab_manager_catalog::permitted() {
            return Self::Deferred;
        }
        match codex_alab_core::native_source_launch::allocate_manager().await {
            Ok(None) => Self::Disabled,
            Ok(Some(allocation)) => Lifetime::new(allocation)
                .map(|member| Self::Opened {
                    member,
                    preload: Mutex::new(None),
                })
                .unwrap_or(Self::Refused),
            Err(_) => Self::Refused,
        }
    }
    pub async fn load(
        &self,
        config: &AuthConfig,
        enable_api_env: bool,
        allowed_modes: Option<&[AuthMode]>,
        selected: Option<NoNativeReader>,
    ) -> std::io::Result<Option<CodexAuth>> {
        match self {
            Self::Disabled => {
                config
                    .load_auth_with_modes(enable_api_env, allowed_modes)
                    .await
            }
            Self::Deferred | Self::Refused => Err(codex_alab_core::managed_auth::unavailable()),
            Self::Opened { member, preload } => {
                if selected.is_some() {
                    return Ok(None);
                }
                // API environment credentials are selected before file/keyring readers.
                // Verify this exact branch's policy before reading the environment; never
                // use legacy load's later fallback to inspect a ChatGPT source unregistered.
                if enable_api_env
                    && allowed_modes.is_none_or(|modes| modes.contains(&AuthMode::ApiKey))
                    && auth_mode_is_allowed(Some(&config.allowed_login_methods()), AuthMode::ApiKey)
                    && let Some(key) = read_codex_api_key_from_env()
                {
                    let auth = CodexAuth::from_api_key(key.as_str());
                    if !config.allows_auth(&auth) {
                        return Err(codex_alab_core::managed_auth::unavailable());
                    }
                    return Ok(Some(auth));
                }
                let operation = format!("{:032x}", rand::random::<u128>());
                let actual = member.preload(&operation, None).await?;
                *preload
                    .lock()
                    .map_err(|_| codex_alab_core::managed_auth::unavailable())? =
                    Some(Box::new(actual));
                // The manager is constructed with no cache; its verified Bound reader
                // loads the private document under its actual policies before exposure.
                Ok(None)
            }
        }
    }

    pub async fn finish(self, manager: &AuthManager, selected: Option<NoNativeReader>) {
        let (lifetime, prepared) = match self {
            Self::Disabled | Self::Deferred => return,
            Self::Refused => {
                manager.alab_refuse_initial_source();
                return;
            }
            Self::Opened { member, preload } => {
                let prepared = preload.into_inner().ok().flatten();
                (member, prepared)
            }
        };
        if lifetime
            .attach_domain(manager.alab_assignment_domain())
            .is_err()
        {
            manager.alab_refuse_initial_source();
            return;
        }
        if let Ok(mut member) = manager.alab_source_member.write() {
            *member = Some(lifetime.clone());
        } else {
            manager.alab_refuse_initial_source();
            return;
        }
        let kind = selected.or_else(|| {
            manager
                .inner
                .read()
                .ok()
                .and_then(|cached| cached.auth.as_ref().and_then(no_native_reader))
        });
        if manager
            .alab_begin_initial_source(lifetime.clone(), kind)
            .is_err()
        {
            return;
        }
        if kind.is_some() {
            let _ = manager.alab_resume_initial_source().await;
            return;
        }
        // Missing/failed selection is not NoCredentials, and must not initiate a new
        // preload after the original attempt failed or became uncertain.
        let preload = match prepared {
            Some(preload) => preload,
            None => return,
        };
        let operation = preload.source.operation.clone();
        if let Err(error) = manager
            .alab_initialize_member_source(preload.source, lifetime.clone())
            .await
        {
            // Preserve the actual candidate/cache/counters and frozen Ready report
            // through uncertainty. PendingInitial cannot expose a credential or
            // fall back to startup; a later auth admission resumes this same report.
            if error.kind() == std::io::ErrorKind::PermissionDenied
                && manager.inner.read().is_ok_and(|cache| cache.auth.is_none())
                && lifetime.unavailable(&operation).await.is_ok()
            {
                manager.alab_refuse_initial_source();
            }
        }
    }
}
fn no_native_reader(auth: &CodexAuth) -> Option<NoNativeReader> {
    match auth {
        CodexAuth::ApiKey(_) => Some(NoNativeReader::ApiKey),
        CodexAuth::BedrockApiKey(_) => Some(NoNativeReader::BedrockApiKey),
        CodexAuth::BedrockAccessKeys(_) => Some(NoNativeReader::BedrockAccessKeys),
        CodexAuth::Headers(_) => Some(NoNativeReader::Headers),
        CodexAuth::AgentIdentity(_) => Some(NoNativeReader::AgentIdentity),
        CodexAuth::PersonalAccessToken(_) => Some(NoNativeReader::PersonalAccessToken),
        CodexAuth::Chatgpt(_) | CodexAuth::ChatgptAuthTokens(_) => None,
    }
}
impl AuthManager {
    pub(super) fn alab_native_network_policy(
        &self,
        policy: codex_http_client::NetworkPolicy,
    ) -> codex_http_client::NetworkPolicy {
        let policy = self.alab_catalog_network_policy(policy);
        match self.alab_source_member.read() {
            Ok(member) => match member.as_ref() {
                Some(member) => policy.with_native_source(
                    member
                        .attach_domain(self.alab_assignment_domain())
                        .and_then(|()| member.network_scope())
                        .map_err(|_| ()),
                ),
                None => policy,
            },
            Err(_) => policy.with_native_source(Err(())),
        }
    }
    fn alab_refuse_initial_source(&self) {
        if let Ok(mut source) = self.alab_native_source.write() {
            *source = super::alab_native_source::State::Detached;
        }
        if let Ok(mut cached) = self.inner.write() {
            self.set_cached_auth_locked(&mut cached, None);
        }
    }
    pub(super) fn alab_has_native_member(&self) -> bool {
        self.alab_source_member
            .read()
            .map_or(true, |member| member.is_some())
    }
    pub(super) fn alab_seal_native_member(&self) -> std::io::Result<()> {
        if let Some(member) = self
            .alab_source_member
            .read()
            .map_err(|_| codex_alab_core::managed_auth::unavailable())?
            .as_ref()
        {
            member.seal();
        }
        Ok(())
    }
    pub(super) fn alab_unrelated_load(
        &self,
        allow_external: bool,
    ) -> Option<std::io::Result<Option<CodexAuth>>> {
        let member = match self.alab_source_member.read() {
            Ok(member) => member.clone(),
            Err(_) => return Some(Err(codex_alab_core::managed_auth::unavailable())),
        }?;
        let kind = match member.unrelated_kind() {
            Ok(Some(kind)) => kind,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if !member.ready() {
            return Some(Err(codex_alab_core::managed_auth::unavailable()));
        }
        if allow_external && self.has_external_auth() {
            return None;
        }
        let cached = match self.inner.read() {
            Ok(cached) => cached.auth.clone(),
            Err(_) => return Some(Err(codex_alab_core::managed_auth::unavailable())),
        };
        let allowed = cached.as_ref().is_some_and(|auth| {
            no_native_reader(auth) == Some(kind)
                && self.allowed_auth_modes.read().is_ok_and(|modes| {
                    modes
                        .as_ref()
                        .is_none_or(|modes| modes.contains(&auth.auth_mode()))
                })
                && validate_auth_restrictions(
                    Some(&self.allowed_login_methods()),
                    self.effective_chatgpt_workspaces().as_deref(),
                    auth,
                )
                .is_ok()
        });
        Some(if allowed {
            Ok(cached)
        } else {
            member.seal();
            Err(codex_alab_core::managed_auth::unavailable())
        })
    }
    pub(super) fn alab_member_for_source(
        &self,
        source: &Source,
    ) -> std::io::Result<Option<Lifetime>> {
        let member = self
            .alab_source_member
            .read()
            .map_err(|_| codex_alab_core::managed_auth::unavailable())?
            .clone();
        if let Some(member) = &member {
            member.permits_storage(source)?;
        }
        Ok(member)
    }
}

/// Direct configuration readers have no AuthManager counters. API environment
/// snapshots can use native's real unrelated-source completion; managed direct
/// readers require a distinct native reader proof and must not forge manager Ready.
pub(super) async fn direct_config_auth(
    config: &AuthConfig,
    enable_api_env: bool,
    allowed_modes: Option<&[AuthMode]>,
) -> Option<std::io::Result<Option<CodexAuth>>> {
    if !codex_alab_core::native_source_launch::requested() {
        return None;
    }
    Some(super::alab_reader_catalog::load(config, enable_api_env, allowed_modes).await)
}

pub(super) async fn read_direct(
    config: &AuthConfig,
    enable_api_env: bool,
    allowed_modes: Option<&[AuthMode]>,
    record: &Arc<codex_alab_core::manager_catalog::Record>,
    domain: Arc<codex_alab_core::assignment::Domain>,
) -> std::io::Result<Option<CodexAuth>> {
    let allocation = codex_alab_core::native_source_launch::allocate_manager()
        .await?
        .ok_or_else(codex_alab_core::managed_auth::unavailable)?;
    let member = Lifetime::new(allocation)?;
    member.attach_domain(domain)?;
    let reader = super::alab_reader_catalog::Reader::begin(record, config, allowed_modes, &member)?;
    if enable_api_env
        && allowed_modes.is_none_or(|modes| modes.contains(&AuthMode::ApiKey))
        && auth_mode_is_allowed(Some(&config.allowed_login_methods()), AuthMode::ApiKey)
        && let Some(key) = read_codex_api_key_from_env()
    {
        let mut auth = CodexAuth::from_api_key(key.as_str());
        if !config.allows_auth(&auth) {
            return Err(codex_alab_core::managed_auth::unavailable());
        }
        member.unrelated_ready(NoNativeReader::ApiKey).await?;
        if let CodexAuth::ApiKey(api) = &mut auth {
            api.alab_source = Some(member.clone());
        }
        reader.complete(&member, &auth)?;
        auth.alab_capture_reader_origin(record.clone(), member.clone(), config)?;
        return Ok(Some(auth));
    }
    let operation = format!("{:032x}", rand::random::<u128>());
    let preload = member.preload(&operation, None).await?;
    match super::alab_native_source::direct_reader(
        config,
        allowed_modes,
        preload.source,
        member.clone(),
    )
    .await
    {
        Ok(mut auth) => {
            reader.complete(&member, &auth)?;
            auth.alab_capture_reader_origin(record.clone(), member.clone(), config)?;
            Ok(Some(auth))
        }
        Err(error) => {
            if error.kind() == std::io::ErrorKind::PermissionDenied {
                let _ = member.unavailable(&operation).await;
            }
            Err(error)
        }
    }
}

impl CodexAuth {
    pub(super) fn alab_credential_access(&self) -> std::io::Result<()> {
        if !self.alab_credential_epoch().native_ready() {
            return Err(codex_alab_core::managed_auth::unavailable());
        }
        let member = match self {
            Self::ApiKey(api) => api.alab_source.clone(),
            Self::Chatgpt(auth) => {
                auth.storage.alab_source_access()?;
                auth.storage.alab_source_lifetime()
            }
            _ => None,
        };
        if member.as_ref().is_some_and(|member| !member.ready()) {
            return Err(codex_alab_core::managed_auth::unavailable());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "auth_native_member_tests.rs"]
pub(super) mod tests;

// Metadata observers cannot keep a credential reader alive or authorize a read.
// Only the explicit debug qualification child consumes these accessors.
#[cfg(debug_assertions)]
impl AuthManager {
    pub fn alab_debug_close_native_work(&self) -> std::io::Result<()> {
        if let Some(actual) = self.alab_host_capture() {
            return actual?.alab_debug_close_native_work();
        }
        self.alab_native_member()?
            .ok_or_else(codex_alab_core::managed_auth::unavailable)?
            .debug_close_work()
    }
    pub fn alab_debug_native_retirement(
        &self,
    ) -> std::io::Result<codex_alab_core::native_reader_retirement::Observer> {
        if let Some(actual) = self.alab_host_metadata() {
            return actual.alab_debug_native_retirement();
        }
        self.alab_source_member
            .read()
            .map_err(|_| codex_alab_core::managed_auth::unavailable())?
            .as_ref()
            .map(Lifetime::retirement_observer)
            .ok_or_else(codex_alab_core::managed_auth::unavailable)
    }
}
#[cfg(debug_assertions)]
impl CodexAuth {
    pub fn alab_debug_native_retirement(
        &self,
    ) -> std::io::Result<codex_alab_core::native_reader_retirement::Observer> {
        let member = match self {
            Self::ApiKey(api) => api.alab_source.clone(),
            Self::Chatgpt(auth) => auth.storage.alab_source_lifetime(),
            _ => None,
        };
        member
            .as_ref()
            .map(Lifetime::retirement_observer)
            .ok_or_else(codex_alab_core::managed_auth::unavailable)
    }
}
