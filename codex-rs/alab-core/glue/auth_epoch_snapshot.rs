// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Runtime credential provenance is copied with the credential, never read from
//! a mutable manager after selecting a document.
use super::*;
use codex_alab_core::manager_source_epoch::Snapshot as EpochSnapshot;
#[derive(Clone, Default)]
pub struct Snapshot(Option<Arc<CapturedSnapshot>>);
struct CapturedSnapshot {
    origin: Origin,
    policy: codex_http_client::NetworkPolicy,
    legacy_export: bool,
}
enum Origin {
    Manager(EpochSnapshot),
    Reader(Box<codex_alab_core::manager_catalog::ReaderScope>),
}
impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CredentialSourceSnapshot(<local>)")
    }
}
impl PartialEq for Snapshot {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for Snapshot {}
impl Snapshot {
    pub fn epoch(&self) -> Option<&codex_alab_core::manager_source_epoch::Epoch> {
        match self.0.as_ref().map(|capture| &capture.origin) {
            Some(Origin::Manager(epoch)) => epoch.epoch(),
            _ => None,
        }
    }
    fn is_reader(&self) -> bool {
        matches!(
            self.0.as_ref().map(|capture| &capture.origin),
            Some(Origin::Reader(_))
        )
    }
    pub fn legacy_snapshot_export(&self) -> bool {
        !codex_alab_core::native_source_launch::requested() && self.0.as_ref().is_some_and(|capture| {
            capture.legacy_export && matches!(&capture.origin, Origin::Manager(epoch) if epoch.legacy_snapshot_export())
        })
    }
    pub fn has_native_member(&self) -> bool {
        match self.0.as_ref().map(|capture| &capture.origin) {
            Some(Origin::Reader(_)) => true,
            Some(Origin::Manager(epoch)) => epoch.has_native_member(),
            None => false,
        }
    }
    pub(super) fn same_native_member(
        &self,
        actual: &codex_alab_core::native_source_lifetime::Lifetime,
    ) -> bool {
        matches!(self.0.as_ref().map(|capture| &capture.origin),
            Some(Origin::Manager(epoch)) if epoch.same_native_member(actual))
    }
    pub fn native_ready(&self) -> bool {
        match self.0.as_ref().map(|capture| &capture.origin) {
            Some(Origin::Reader(reader)) => reader.live(),
            Some(Origin::Manager(epoch)) => epoch.native_ready(),
            None => true,
        }
    }
    pub fn network_policy(&self) -> std::io::Result<Option<codex_http_client::NetworkPolicy>> {
        let Some(capture) = &self.0 else {
            return Ok(None);
        };
        let mut policy = capture.policy.clone();
        match &capture.origin {
            Origin::Manager(epoch) => {
                let scope = epoch
                    .scope()
                    .map_err(|_| codex_alab_core::managed_auth::unavailable())?
                    .ok_or_else(codex_alab_core::managed_auth::unavailable)?;
                policy = policy.with_manager_source(Ok(scope));
                if let Some(native) = epoch
                    .native_scope()
                    .map_err(|_| codex_alab_core::managed_auth::unavailable())?
                {
                    policy = policy.with_native_source(Ok(native));
                }
            }
            Origin::Reader(reader) => {
                policy = policy
                    .with_reader_source(Ok(reader.as_ref().clone()))
                    .with_native_source(reader.native_scope().map_err(|_| ()));
            }
        }
        if !capture.legacy_export {
            policy = policy.with_credential_destination_required();
        }
        Ok(Some(policy))
    }
}
impl CodexAuth {
    pub(super) fn alab_capture_reader_origin(
        &mut self,
        record: Arc<codex_alab_core::manager_catalog::Record>,
        member: codex_alab_core::native_source_lifetime::Lifetime,
        config: &AuthConfig,
    ) -> std::io::Result<()> {
        if self.alab_credential_epoch().0.is_some() {
            return Err(codex_alab_core::managed_auth::unavailable());
        }
        let reader = codex_alab_core::manager_catalog::ReaderScope::capture(record, member)
            .map_err(|_| codex_alab_core::managed_auth::unavailable())?;
        *self.alab_epoch_mut() = Snapshot(Some(Arc::new(CapturedSnapshot {
            origin: Origin::Reader(Box::new(reader)),
            legacy_export: false,
            policy: config
                .auth_route_config
                .application_network_policy()
                .clone()
                .for_current_account(),
        })));
        Ok(())
    }
    pub(crate) fn alab_cached_login_owner_id(&self) -> Option<String> {
        self.get_current_auth_json()
            .and_then(|document| document.tokens)
            .and_then(|tokens| tokens.login_owner_id)
    }
    fn alab_epoch_mut(&mut self) -> &mut Snapshot {
        match self {
            Self::ApiKey(auth) => &mut auth.alab_epoch,
            Self::Chatgpt(auth) => &mut auth.state.alab_epoch,
            Self::ChatgptAuthTokens(auth) => &mut auth.state.alab_epoch,
            Self::Headers(auth) => &mut auth.alab_epoch,
            Self::AgentIdentity(auth) => &mut auth.alab_epoch,
            Self::PersonalAccessToken(auth) => &mut auth.alab_epoch,
            Self::BedrockApiKey(auth) => &mut auth.alab_epoch,
            Self::BedrockAccessKeys(auth) => &mut auth.alab_epoch,
        }
    }
    pub fn alab_credential_epoch(&self) -> &Snapshot {
        match self {
            Self::ApiKey(auth) => &auth.alab_epoch,
            Self::Chatgpt(auth) => &auth.state.alab_epoch,
            Self::ChatgptAuthTokens(auth) => &auth.state.alab_epoch,
            Self::Headers(auth) => &auth.alab_epoch,
            Self::AgentIdentity(auth) => &auth.alab_epoch,
            Self::PersonalAccessToken(auth) => &auth.alab_epoch,
            Self::BedrockApiKey(auth) => &auth.alab_epoch,
            Self::BedrockAccessKeys(auth) => &auth.alab_epoch,
        }
    }
    /// Binding cannot replace a caller's policy or attribute A credentials to B.
    pub fn alab_bind_http_client_factory(
        &self,
        factory: HttpClientFactory,
    ) -> std::io::Result<HttpClientFactory> {
        let Some(credentials) = self.alab_credential_epoch().network_policy()? else {
            return Ok(factory);
        };
        let policy = factory
            .network_policy()
            .clone()
            .with_account_binding(&credentials)
            .ok_or_else(codex_alab_core::managed_auth::unavailable)?;
        Ok(factory.with_network_policy(policy))
    }
}
impl AuthManager {
    /// Caller owns the actual cache write lock; no credential or host I/O occurs.
    pub(super) fn alab_bind_cached_epoch(&self, auth: &mut Option<CodexAuth>) {
        let member = self
            .alab_source_member
            .read()
            .map(|member| member.clone())
            .map_err(|_| ());
        self.alab_bind_cached_epoch_from_member(auth, member);
    }
    /// The caller pins an actual issued member under its publication guards.
    /// This helper never rereads member metadata or invents an occurrence.
    pub(super) fn alab_bind_cached_epoch_from_member(
        &self,
        auth: &mut Option<CodexAuth>,
        member: Result<Option<codex_alab_core::native_source_lifetime::Lifetime>, ()>,
    ) {
        let Some(auth) = auth else {
            return;
        };
        let Ok(epoch) = self.alab_assignment_domain.capture() else {
            return;
        };
        // A credential clone retains its source even when another manager caches
        // it. Only a fresh producer value or this same Epoch may be recaptured;
        // inserting A into B is not authority to relabel the credential as B.
        if auth.alab_credential_epoch().is_reader() {
            return;
        }
        if auth
            .alab_credential_epoch()
            .epoch()
            .is_some_and(|captured| !captured.same(&epoch))
        {
            return;
        }
        let actual = *self.auth_change_state_tx.borrow();
        let native = member.and_then(|member| match member {
            Some(member) => member
                .attach_domain(epoch.domain())
                .map(|()| Some(member))
                .map_err(|_| ()),
            None => Ok(None),
        });
        // Capture the actual nonactivated source choice. Absence of a member
        // alone cannot turn known native storage or a required constructor into
        // the legacy export contract. This performs no file or host I/O.
        let legacy_export = !codex_alab_core::native_source_launch::requested()
            && !matches!(auth, CodexAuth::Chatgpt(chatgpt) if chatgpt.storage.alab_source_scope().is_some());
        let captured = EpochSnapshot::captured(
            epoch,
            codex_alab_core::assignment::Generations {
                auth: actual.generation,
                owner: actual.owner_generation,
                source: actual.source_generation,
                login: actual.login_generation,
            },
            native,
        );
        *auth.alab_epoch_mut() = Snapshot(Some(Arc::new(CapturedSnapshot {
            origin: Origin::Manager(captured),
            legacy_export,
            policy: self
                .auth_route_config
                .application_network_policy()
                .clone()
                .for_current_account(),
        })));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    fn auth(token: &str, session: Option<&str>) -> CodexAuth {
        let mut claims = serde_json::json!({"jti":token,"https://api.openai.com/auth":{"chatgpt_user_id":"epoch-user"}});
        if let Some(session) = session {
            claims["iss"] = "https://auth.example.invalid".into();
            claims["session_id"] = session.into();
        }
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string());
        CodexAuth::from_external_chatgpt_tokens(
            &format!("header.{payload}.signature"),
            "epoch-account",
            None,
        )
        .unwrap()
    }
    #[test]
    fn native_constructor_presence_cannot_export_an_unresolved_legacy_snapshot() {
        const CHILD: &str = "ALAB_NATIVE_EXPORT_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let full_name = concat!(
                module_path!(),
                "::native_constructor_presence_cannot_export_an_unresolved_legacy_snapshot"
            );
            let name = full_name.split_once("::").unwrap().1;
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", name, "--nocapture"])
                .env(CHILD, "1")
                .env(
                    codex_alab_core::native_source::LAUNCH_ENV,
                    "invalid-constructor",
                )
                .output()
                .unwrap();
            assert!(output.status.success());
            assert!(String::from_utf8_lossy(&output.stdout).contains("running 1 test"));
            return;
        }
        let registry = Arc::new(codex_alab_core::manager_catalog::Registry::default());
        let factory = registry.factory(None).unwrap();
        let epoch = codex_alab_core::manager_source_epoch::Owner::new(factory.domain())
            .capture()
            .unwrap();
        factory.complete().unwrap();
        let snapshot = Snapshot(Some(Arc::new(CapturedSnapshot {
            origin: Origin::Manager(EpochSnapshot::captured(
                epoch,
                codex_alab_core::assignment::Generations {
                    auth: 1,
                    owner: 1,
                    source: 1,
                    login: 1,
                },
                Ok(None),
            )),
            policy: codex_http_client::NetworkPolicy::unmanaged(),
            // Even an earlier legacy capture cannot survive required mode.
            legacy_export: true,
        })));
        assert!(!snapshot.legacy_snapshot_export());
    }
    #[tokio::test]
    async fn a_fresh_cache_read_renews_application_policy_without_renewing_retained_credentials() {
        use codex_http_client::DestinationPolicy;
        use codex_http_client::NetworkPolicyController;
        use codex_http_client::OutboundProxyPolicy;
        let controller = NetworkPolicyController::default();
        let policy = controller.policy();
        assert!(controller.publish(policy.revision(), DestinationPolicy::Unrestricted));
        let factory = HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault)
            .with_network_policy(policy.clone());
        let manager = super::super::alab_manager_catalog::fixture::configured(
            move |manager| {
                manager.auth_route_config = AuthRouteConfig::from_http_client_factory(factory);
            },
            || {
                AuthManager::from_auth_for_testing(CodexAuth::from_api_key(
                    "policy-generation-fixture",
                ))
            },
        );
        let (retained, old_factory) = manager.auth_with_http_client_factory().await.unwrap();
        let url = "https://allowed.example/".parse().unwrap();
        let old_permit = old_factory.network_policy().acquire(&url).unwrap();
        policy.invalidate();
        assert!(controller.publish(policy.revision(), DestinationPolicy::Unrestricted));
        assert!(old_permit.check().is_err());
        let (fresh, fresh_factory) = manager.auth_with_http_client_factory().await.unwrap();
        assert_eq!(fresh.api_key(), Some("policy-generation-fixture"));
        fresh_factory
            .network_policy()
            .acquire(&url)
            .unwrap()
            .check()
            .unwrap();
        assert!(
            retained
                .alab_bind_http_client_factory(fresh_factory)
                .is_err()
        );
        assert!(old_factory.network_policy().acquire(&url).is_err());
    }

    #[test]
    fn snapshot_keeps_its_epoch_through_refresh_but_cannot_adopt_a_new_source_factory() {
        let manager = AuthManager::from_auth_for_testing(auth("one", None));
        let a = manager.auth_cached().unwrap();
        let epoch_a = a.alab_credential_epoch().epoch().unwrap().clone();
        let factory_a = manager.http_client_factory();
        manager.set_cached_auth(Some(auth("two", None)));
        let refreshed = manager.auth_cached().unwrap();
        assert!(
            refreshed
                .alab_credential_epoch()
                .epoch()
                .unwrap()
                .same(&epoch_a)
        );
        assert!(
            a.alab_bind_http_client_factory(manager.http_client_factory())
                .is_ok()
        );
        manager.set_cached_auth(Some(auth("three", Some("new-session"))));
        let b = manager.auth_cached().unwrap();
        assert!(!b.alab_credential_epoch().epoch().unwrap().same(&epoch_a));
        assert!(a.alab_credential_epoch().epoch().unwrap().same(&epoch_a));
        assert!(
            a.alab_bind_http_client_factory(manager.http_client_factory())
                .is_err()
        );
        assert!(b.alab_bind_http_client_factory(factory_a.clone()).is_err());
        assert!(matches!(
            factory_a
                .network_policy()
                .acquire(&"https://example.com/".parse().unwrap()),
            Err(codex_http_client::NetworkPolicyDenied::Revoked)
        ));
        assert!(
            b.alab_bind_http_client_factory(manager.http_client_factory())
                .is_ok()
        );
    }
    #[test]
    fn a_captured_credential_cannot_be_relabelled_by_another_managers_cache() {
        let values = [
            CodexAuth::from_api_key("epoch-test-key"),
            CodexAuth::Headers(AuthHeaders::new(http::HeaderMap::new())),
            auth("copied", Some("source-session")),
            CodexAuth::BedrockApiKey(BedrockApiKeyAuth {
                api_key: "not-a-key".into(),
                region: "us-east-1".into(),
                alab_epoch: Default::default(),
            }),
            CodexAuth::BedrockAccessKeys(BedrockAccessKeysAuth {
                access_key_id: "not-a-key".into(),
                secret_access_key: "not-a-secret".into(),
                session_token: None,
                alab_epoch: Default::default(),
            }),
        ];
        for value in values {
            let source = AuthManager::from_auth_for_testing(value);
            let captured = source.auth_cached().unwrap();
            let epoch = captured.alab_credential_epoch().epoch().unwrap().clone();
            let destination = AuthManager::from_auth_for_testing(captured.clone());
            let copied = destination.auth_cached().unwrap();
            assert!(copied.alab_credential_epoch().epoch().unwrap().same(&epoch));
            assert!(
                !copied
                    .alab_credential_epoch()
                    .epoch()
                    .unwrap()
                    .same(&destination.alab_source_epoch().unwrap())
            );
            assert!(
                copied
                    .alab_bind_http_client_factory(destination.http_client_factory())
                    .is_err()
            );
            assert!(
                copied
                    .alab_bind_http_client_factory(source.http_client_factory())
                    .is_ok()
            );
        }
    }
    #[test]
    fn api_header_and_workload_payload_snapshots_capture_actual_manager_epoch() {
        let values = [
            CodexAuth::from_api_key("epoch-test-key"),
            CodexAuth::Headers(AuthHeaders::new(http::HeaderMap::new())),
            CodexAuth::BedrockApiKey(BedrockApiKeyAuth {
                api_key: "not-a-key".into(),
                region: "us-east-1".into(),
                alab_epoch: Default::default(),
            }),
            CodexAuth::BedrockAccessKeys(BedrockAccessKeysAuth {
                access_key_id: "not-a-key".into(),
                secret_access_key: "not-a-secret".into(),
                session_token: None,
                alab_epoch: Default::default(),
            }),
        ];
        for value in values {
            let manager = AuthManager::from_auth_for_testing(value);
            let auth = manager.auth_cached().unwrap();
            let epoch = auth.alab_credential_epoch().epoch().unwrap();
            assert!(epoch.same(&manager.alab_source_epoch().unwrap()));
            assert!(
                auth.alab_bind_http_client_factory(manager.http_client_factory())
                    .is_ok()
            );
        }
    }
}
