// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use crate::ModelCatalogFactory;
use crate::SharedModelProvider;
use crate::models_endpoint::OpenAiModelsEndpoint;
use crate::provider_uses_first_party_auth_path;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_model_provider_info::ModelProviderInfo;
use codex_models_manager::manager::SharedModelsManager;
use codex_protocol::error::CodexErr;
use codex_protocol::openai_models::ModelsResponse;
use sha2::Digest;
use sha2::Sha256;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultApi {
    Commercial,
    FedRamp,
}

impl DefaultApi {
    fn from_base(base: &str) -> Option<Self> {
        let url = Url::parse(base).ok()?;
        if url.scheme() != "https"
            || url.port_or_known_default() != Some(443)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path().trim_end_matches('/') != "/v1"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return None;
        }
        let host = url.host_str()?;
        match host.strip_suffix('.').unwrap_or(host) {
            "api.openai.com" => Some(Self::Commercial),
            "gov.api.openai.com" => Some(Self::FedRamp),
            _ => None,
        }
    }

    /// The configured inference URL stays intact. Only the Codex metadata client
    /// can use the account's default catalog, after matching its actual credential.
    pub fn configured(info: &ModelProviderInfo) -> Option<Self> {
        if !codex_alab_core::active()
            || !info.is_openai()
            || !provider_uses_first_party_auth_path(info)
            || info.gateway_oauth.is_some()
            || info.model_catalog_url.is_some()
        {
            return None;
        }
        Self::from_base(info.base_url.as_deref()?)
    }

    fn matches(self, auth: Option<&CodexAuth>) -> bool {
        auth.is_some_and(|auth| {
            auth.is_api_key_auth() && auth.is_fedramp_account() == (self == Self::FedRamp)
        })
    }
}

/// Fixed and embedded host catalogs retain their existing construction policy.
pub fn build(
    factory: &dyn ModelCatalogFactory,
    registered_api: bool,
    provider: SharedModelProvider,
    catalog: Option<ModelsResponse>,
    discover: bool,
) -> SharedModelsManager {
    if registered_api && let Some(mut settings) = factory.alab_openai_catalog_settings() {
        settings.alab_registered_openai_api = true;
        return settings.build(provider, catalog, discover);
    }
    factory.build(provider, catalog, discover)
}

impl OpenAiModelsEndpoint {
    pub(crate) fn alab_registered_api(
        mut self,
        registered: bool,
        info: &ModelProviderInfo,
    ) -> Self {
        if registered {
            self.alab_default_api = DefaultApi::configured(info);
        }
        self
    }
}

pub(crate) fn identity(
    info: &ModelProviderInfo,
    auth: Option<&CodexAuth>,
    registered: Option<DefaultApi>,
) -> Result<String, CodexErr> {
    let key = crate::models_identity::identity(info, auth)?;
    if registered.is_none() {
        return Ok(key);
    }
    let mut hash = Sha256::new();
    hash.update(b"codex-alab/registered-openai-api-catalog/v1\0");
    hash.update(key.as_bytes());
    Ok(format!("{:x}", hash.finalize()))
}

pub(crate) fn supports_explicit(api: Option<DefaultApi>, manager: Option<&AuthManager>) -> bool {
    api.is_some_and(|api| api.matches(manager.and_then(AuthManager::auth_cached).as_ref()))
}

/// Run after request authentication. A credential replacement between eligibility
/// and fetch cannot redirect model discovery to another account's default endpoint.
pub(crate) fn admit(
    info: &ModelProviderInfo,
    registered: Option<DefaultApi>,
    auth: Option<&CodexAuth>,
) -> Result<bool, CodexErr> {
    let Some(api) = registered else {
        return Ok(info.is_openai() && info.base_url.is_none());
    };
    if api.matches(auth) {
        Ok(true)
    } else {
        Err(CodexErr::InvalidRequest(
            "The current API key uses a different default endpoint from the configured inference URL."
                .to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_catalog_rechecks_the_captured_key_and_registered_owner() {
        let info =
            ModelProviderInfo::create_openai_provider(Some("https://api.openai.com/v1".into()));
        let public = CodexAuth::from_api_key("sk-proj-synthetic");
        let gov = CodexAuth::from_api_key("sk-proj-fedramp-synthetic");
        let account = AuthManager::from_auth_for_testing(public.clone());
        let registered = Some(DefaultApi::Commercial);

        assert!(supports_explicit(registered, Some(&account)));
        assert!(admit(&info, registered, Some(&public)).unwrap());
        assert!(admit(&info, registered, Some(&gov)).is_err());
        assert!(!admit(&info, None, Some(&public)).unwrap());
        assert_ne!(
            identity(&info, Some(&public), registered).unwrap(),
            identity(&info, Some(&public), None).unwrap()
        );
    }

    #[test]
    fn explicit_default_api_catalog_requires_the_matching_route_and_key() {
        let public = CodexAuth::from_api_key("sk-proj-synthetic");
        let gov = CodexAuth::from_api_key("sk-proj-fedramp-synthetic");
        for (url, api) in [
            ("https://api.openai.com/v1", DefaultApi::Commercial),
            ("https://API.OPENAI.COM.:443/v1/", DefaultApi::Commercial),
            ("https://gov.api.openai.com/v1", DefaultApi::FedRamp),
        ] {
            assert_eq!(DefaultApi::from_base(url), Some(api));
            assert_eq!(api.matches(Some(&public)), api == DefaultApi::Commercial);
            assert_eq!(api.matches(Some(&gov)), api == DefaultApi::FedRamp);
            assert!(!api.matches(None));
        }
        for url in [
            "https://proxy.example/v1",
            "https://api.openai.com.proxy.example/v1",
            "https://api.openai.com@proxy.example/v1",
            "https://proxy.example@api.openai.com/v1",
            "http://api.openai.com/v1",
            "https://api.openai.com:8443/v1",
            "https://api.openai.com/V1",
            "https://api.openai.com/v1?tenant=custom",
            "https://api.openai.com/v1#custom",
            "https://eu.api.openai.com/v1",
        ] {
            assert_eq!(DefaultApi::from_base(url), None, "{url}");
        }
    }
}
