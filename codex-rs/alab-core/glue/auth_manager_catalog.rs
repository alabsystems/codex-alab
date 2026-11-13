// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual manager constructors participate before any credential reader runs.
use super::*;
use codex_alab_core::manager_catalog as catalog;

pub(super) fn construct<F: std::future::Future<Output = AuthManager>>(
    future: F,
) -> impl std::future::Future<Output = AuthManager> {
    // Box before creating our async state: upstream's constructor future must not
    // be copied inline through each factory/task-local admission wrapper.
    let future = Box::pin(future);
    async move {
        if catalog::current_factory().is_some() {
            let manager = future.await;
            anchor(&manager);
            return manager;
        }
        let factory = catalog::global()
            .factory(codex_alab_core::assignment::current_activity().as_ref())
            .unwrap_or_else(|_| catalog::refused_factory());
        let manager = catalog::factory_scope(Arc::clone(factory.record()), future).await;
        anchor(&manager);
        if factory.admitted() {
            let _ = factory.complete();
        }
        manager
    }
}
pub(super) fn shared<
    F: std::future::Future<Output = Result<Arc<AuthManager>, AuthManagerInitializationError>>,
>(
    future: F,
) -> impl std::future::Future<Output = Result<Arc<AuthManager>, AuthManagerInitializationError>> {
    let future = Box::pin(future);
    async move {
        let factory = catalog::global()
            .factory(codex_alab_core::assignment::current_activity().as_ref())
            .map_err(|_| RefreshTokenError::from(codex_alab_core::managed_auth::unavailable()))?;
        if !factory.admitted() {
            return Err(
                RefreshTokenError::from(codex_alab_core::managed_auth::unavailable()).into(),
            );
        }
        let manager = factory
            .scope(future)
            .await
            .map_err(|_| RefreshTokenError::from(codex_alab_core::managed_auth::unavailable()))??;
        factory
            .complete()
            .map_err(|_| RefreshTokenError::from(codex_alab_core::managed_auth::unavailable()))?;
        Ok(manager)
    }
}
pub(super) fn domain() -> Arc<codex_alab_core::assignment::Domain> {
    if let Some(record) = catalog::current_factory() {
        return record
            .domain()
            .unwrap_or_else(|| catalog::refused_factory().domain());
    }
    let factory = catalog::global()
        .factory(codex_alab_core::assignment::current_activity().as_ref())
        .unwrap_or_else(|_| catalog::refused_factory());
    let domain = factory.domain();
    if factory.admitted() {
        if codex_alab_core::native_source_launch::requested() {
            let _ = factory.defer();
        } else {
            let _ = factory.complete();
        }
    }
    domain
}
pub(super) fn permitted() -> bool {
    catalog::current_factory().is_none_or(|record| record.cache_available())
}
struct Version(
    watch::Receiver<AuthChangeState>,
    Arc<codex_alab_core::manager_source_epoch::Owner>,
);
impl catalog::Version for Version {
    fn source_epoch(
        &self,
    ) -> Result<
        Option<codex_alab_core::manager_source_epoch::Epoch>,
        codex_alab_core::assignment::Refused,
    > {
        self.1.capture().map(Some)
    }
    fn current(&self) -> codex_alab_core::assignment::Generations {
        let state = *self.0.borrow();
        codex_alab_core::assignment::Generations {
            auth: state.generation,
            owner: state.owner_generation,
            source: state.source_generation,
            login: state.login_generation,
        }
    }
    fn changed<'a>(
        &'a self,
        observed: &'a codex_alab_core::assignment::Generations,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        let mut changes = self.0.clone();
        Box::pin(async move {
            let current = {
                let state = *changes.borrow_and_update();
                codex_alab_core::assignment::Generations {
                    auth: state.generation,
                    owner: state.owner_generation,
                    source: state.source_generation,
                    login: state.login_generation,
                }
            };
            if &current != observed {
                return;
            }
            if changes.changed().await.is_err() {
                std::future::pending().await
            }
        })
    }
}
struct Backend {
    epoch: OnceLock<codex_alab_core::manager_source_epoch::Observation>,
    manager: OnceLock<std::sync::Weak<AuthManager>>,
    changes: watch::Receiver<AuthChangeState>,
}
impl catalog::Backend for Backend {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn source_stamp(
        &self,
    ) -> Result<codex_alab_core::assignment::Generations, codex_alab_core::assignment::Refused>
    {
        let epoch = self
            .epoch
            .get()
            .ok_or(codex_alab_core::assignment::Refused::Pending)?;
        if !epoch.active() {
            return epoch
                .retired_stamp()
                .cloned()
                .ok_or(codex_alab_core::assignment::Refused::Pending);
        }
        let state = *self.changes.borrow();
        Ok(codex_alab_core::assignment::Generations {
            auth: state.generation,
            owner: state.owner_generation,
            source: state.source_generation,
            login: state.login_generation,
        })
    }
    fn check_target(
        &self,
        target: &codex_alab_core::assignment_service::Target,
    ) -> Result<(), codex_alab_core::assignment::Refused> {
        self.manager
            .get()
            .and_then(std::sync::Weak::upgrade)
            .ok_or(codex_alab_core::assignment::Refused::Pending)?
            .alab_check_native_target(target)
            .map_err(|_| codex_alab_core::assignment::Refused::Unavailable)
    }
}
fn anchor(manager: &AuthManager) {
    if let Ok(mut cached) = manager.inner.write() {
        manager.alab_bind_cached_epoch(&mut cached.auth);
    }
    if let Some(record) = manager.alab_assignment_domain().catalog()
        && record.backend().is_ok_and(|backend| backend.is_none())
    {
        let epoch = OnceLock::new();
        if let Ok(current) = manager.alab_source_epoch() {
            let _ = epoch.set(current.observation());
        }
        let _ = record.attach(Arc::new(Backend {
            epoch,
            manager: OnceLock::new(),
            changes: manager.auth_change_state_receiver(),
        }));
    }
}
pub(super) fn attach(manager: Arc<AuthManager>) -> Arc<AuthManager> {
    #[cfg(test)]
    let manager = fixture::before_attach(manager);
    anchor(&manager);
    if let Some(record) = manager.alab_assignment_domain().catalog()
        && let Ok(Some(backend)) = record.backend()
        && let Some(backend) = backend.as_any().downcast_ref::<Backend>()
    {
        let _ = backend.manager.set(Arc::downgrade(&manager));
    }
    manager
}

impl AuthManager {
    pub(super) fn alab_catalog_network_policy(
        &self,
        policy: codex_http_client::NetworkPolicy,
    ) -> codex_http_client::NetworkPolicy {
        policy.with_manager_source(
            catalog::Scope::capture(
                self.alab_assignment_domain(),
                Arc::new(Version(
                    self.auth_change_state_receiver(),
                    Arc::clone(&self.alab_assignment_domain),
                )),
            )
            .map_err(|_| ()),
        )
    }
    pub(super) fn alab_catalog_cache_available(&self) -> bool {
        self.alab_assignment_domain.capture().is_ok()
            && self
                .alab_assignment_domain()
                .catalog()
                .is_some_and(|record| record.cache_available())
    }
    pub(super) async fn alab_catalog_ensure(&self) -> std::io::Result<()> {
        let record = self
            .alab_assignment_domain()
            .catalog()
            .cloned()
            .ok_or_else(codex_alab_core::managed_auth::unavailable)?;
        if record.cache_available() {
            self.alab_resume_initial_source().await?;
            return self.alab_reconcile_native_work().await;
        }
        let factory = catalog::global()
            .admit_deferred(&record)
            .map_err(|_| codex_alab_core::managed_auth::unavailable())?;
        catalog::factory_scope(Arc::clone(&record), async {
            let config = AuthConfig {
                codex_home: self.codex_home.clone(),
                auth_credentials_store_mode: self.auth_credentials_store_mode,
                keyring_backend_kind: self.keyring_backend_kind,
                forced_login_method: self.forced_login_method,
                chatgpt_base_url: self.chatgpt_base_url.clone(),
                forced_chatgpt_workspace_id: self.effective_chatgpt_workspaces(),
                managed_auth_policy: self.managed_auth_policy.clone(),
                auth_route_config: self.auth_route_config.clone(),
            };
            let modes = self
                .allowed_auth_modes
                .read()
                .map_err(|_| codex_alab_core::managed_auth::unavailable())?
                .clone();
            let selected = self
                .has_external_auth()
                .then_some(codex_alab_core::native_source::NoNativeReader::External);
            let initial = alab_native_member::Initial::begin().await;
            let supplied = self
                .inner
                .read()
                .map_err(|_| codex_alab_core::managed_auth::unavailable())?
                .auth
                .clone();
            let auth = if !codex_alab_core::native_source_launch::requested() && supplied.is_some()
            {
                supplied
            } else {
                initial
                    .load(
                        &config,
                        self.enable_codex_api_key_env,
                        modes.as_deref(),
                        selected,
                    )
                    .await?
            };
            self.set_cached_auth(auth);
            initial.finish(self, selected).await;
            Ok::<(), std::io::Error>(())
        })
        .await?;
        factory
            .complete()
            .map_err(|_| codex_alab_core::managed_auth::unavailable())?;
        Ok(())
    }
}

// Test configuration must precede publication of the catalog's Weak manager.
// Source identity is independent of credential/read revisions. This comparison grants
// no native authority: native storage remains fenced by its verified member/scope.
fn native_identity_changed(previous: Option<&CodexAuth>, current: Option<&CodexAuth>) -> bool {
    let descriptor = |auth: Option<&CodexAuth>| match auth {
        Some(CodexAuth::Chatgpt(auth)) => auth.storage.alab_source_scope(),
        _ => None,
    };
    let member = |auth: Option<&CodexAuth>| match auth {
        Some(CodexAuth::Chatgpt(auth)) => auth.storage.alab_source_lifetime(),
        _ => None,
    };
    let changed_member = match (member(previous), member(current)) {
        (Some(left), Some(right)) => !left.same(&right),
        (None, None) => false,
        _ => true,
    };
    changed_member
        || match (descriptor(previous), descriptor(current)) {
            (Some(left), Some(right)) => {
                left.slot != right.slot
                    || left.path != right.path
                    || left.account != right.account
                    || left.user != right.user
                    || left.assignment_generation != right.assignment_generation
                    || left.native_intent != right.native_intent
            }
            (None, None) => false,
            _ => true,
        }
}
fn source_replaced(previous: Option<&CodexAuth>, current: Option<&CodexAuth>) -> bool {
    if native_identity_changed(previous, current) {
        return true;
    }
    let (Some(previous), Some(current)) = (previous, current) else {
        return true;
    };
    // These are already-loaded cache documents under the writer, not credential
    // extraction or a new file read while native Ready is being updated.
    let tokens = |auth: &CodexAuth| {
        auth.get_current_auth_json()
            .and_then(|document| document.tokens)
    };
    let old_tokens = tokens(previous);
    let new_tokens = tokens(current);
    if let (Some(left), Some(right)) = (&old_tokens, &new_tokens)
        && (left.oauth_provenance != right.oauth_provenance
            || left.id_token.chatgpt_account_is_fedramp
                != right.id_token.chatgpt_account_is_fedramp)
    {
        return true;
    }
    let native = |auth: &CodexAuth| match auth {
        CodexAuth::Chatgpt(auth) => auth.storage.alab_source_lifetime(),
        _ => None,
    };
    match (native(previous), native(current)) {
        (Some(left), Some(right)) => {
            return !left.same(&right)
                || !matches!((&old_tokens, &new_tokens),
                (Some(left), Some(right)) if left.login_owner_id == right.login_owner_id
                    && super::alab_auth_refresh_lock::managed_grant_relation(left, right) == super::alab_auth_refresh_lock::ManagedGrantRelation::Same);
        }
        (Some(_), None) | (None, Some(_)) => return true,
        (None, None) => {}
    }
    if !same_owner(Some(previous), Some(current)) || !same_login(Some(previous), Some(current)) {
        return true;
    }
    // When present, an actual OAuth session/realm change is a new source. Missing
    // claims do not establish native continuity and are never used as a grant.
    #[derive(serde::Deserialize, PartialEq, Eq)]
    struct Session {
        iss: Option<String>,
        session_id: Option<String>,
        client_id: Option<String>,
        azp: Option<String>,
    }
    let session = |auth: &CodexAuth| {
        auth.get_token()
            .ok()
            .and_then(|token| crate::token_data::decode_jwt_payload::<Session>(&token).ok())
            .filter(|claims| {
                claims.iss.is_some()
                    || claims.session_id.is_some()
                    || claims.client_id.is_some()
                    || claims.azp.is_some()
            })
    };
    session(previous) != session(current)
}
impl AuthManager {
    pub fn alab_source_epoch(
        &self,
    ) -> Result<codex_alab_core::manager_source_epoch::Epoch, codex_alab_core::assignment::Refused>
    {
        if let Some(actual) = self.alab_host_capture() {
            return actual
                .map_err(|_| codex_alab_core::assignment::Refused::Unavailable)?
                .alab_source_epoch();
        }
        self.alab_assignment_domain.capture()
    }
    pub(super) fn alab_catalog_auth_publication(
        &self,
        previous: Option<&CodexAuth>,
        current: Option<&CodexAuth>,
    ) -> Option<codex_alab_core::manager_source_epoch::Publication<'_>> {
        let mut publication = self.alab_assignment_domain.begin().ok()?;
        if (!Self::auths_equal_for_refresh(previous, current)
            || native_identity_changed(previous, current))
            && source_replaced(previous, current)
        {
            let old = self.alab_assignment_domain.domain();
            let Some(backend) = old.catalog()?.backend().ok()? else {
                // Constructor cache is not yet published: its real factory still owns
                // admission and will attach this exact manager before exposure.
                return Some(publication);
            };
            let backend = backend.as_any().downcast_ref::<Backend>()?;
            let factory = catalog::global()
                .factory(codex_alab_core::assignment::current_activity().as_ref())
                .ok()?;
            if !factory.admitted() {
                return None;
            }
            let domain = factory.domain();
            let manager = OnceLock::new();
            if let Some(weak) = backend.manager.get() {
                let _ = manager.set(weak.clone());
            }
            let destination = Arc::new(Backend {
                epoch: OnceLock::new(),
                manager,
                changes: self.auth_change_state_receiver(),
            });
            factory.record().attach(destination.clone()).ok()?;
            factory.complete().ok()?;
            let actual = *self.auth_change_state_tx.borrow();
            let epoch = publication
                .replace(
                    domain,
                    codex_alab_core::assignment::Generations {
                        auth: actual.generation,
                        owner: actual.owner_generation,
                        source: actual.source_generation,
                        login: actual.login_generation,
                    },
                )
                .ok()?;
            destination.epoch.set(epoch.observation()).ok()?;
        }
        Some(publication)
    }
}

#[cfg(test)]
#[path = "auth_manager_catalog_fixture.rs"]
pub(super) mod fixture;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn external_tokens_supply_runtime_actor_credential() {
        use base64::Engine;

        let home = tempfile::tempdir().unwrap();
        let claims = serde_json::json!({
            "iss": "https://auth.openai.com/", "session_id": "fixture-login",
            "https://api.openai.com/auth": {
                "user_id": "fixture-user", "chatgpt_account_id": "fixture-account"
            }
        });
        let token = format!(
            "header.{}.signature",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        login_with_chatgpt_auth_tokens(home.path(), &token, "fixture-account", None).unwrap();
        let controller = codex_http_client::NetworkPolicyController::default();
        let policy = controller.policy();
        let manager = AuthManager::new(
            home.path().to_path_buf(),
            false,
            AuthCredentialsStoreMode::Ephemeral,
            None,
            None,
            AuthKeyringBackendKind::Direct,
            AuthRouteConfig::from_http_client_factory(
                HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault)
                    .with_network_policy(policy.clone()),
            ),
        )
        .await;
        assert!(controller.publish(
            policy.revision(),
            codex_http_client::DestinationPolicy::Unrestricted
        ));
        assert!(
            manager.auth_cached().is_some(),
            "external token was not loaded"
        );
        assert!(
            manager.auth().await.is_some(),
            "external auth became unavailable"
        );
        assert!(
            manager.alab_catalog_cache_available(),
            "catalog refused the loaded manager"
        );
        assert!(
            manager.alab_member_cache_snapshot().is_some(),
            "loaded source/cache pair was refused"
        );
        assert!(
            manager.auth_with_http_client_factory().await.is_some(),
            "runtime actor credential/factory pair was unavailable"
        );
        let captured = manager.auth_cached().unwrap();
        policy.invalidate();
        assert!(controller.publish(
            policy.revision(),
            codex_http_client::DestinationPolicy::Unrestricted
        ));
        let (_, refreshed_factory) = manager.auth_with_http_client_factory().await.unwrap();
        assert!(
            captured.alab_bind_http_client_factory(refreshed_factory).is_err(),
            "a fresh cache read revived the retained credential"
        );
        manager.reload_with_cached_auth().await.unwrap();
        assert!(
            manager.auth_with_http_client_factory().await.is_some(),
            "fresh credential capture after explicit reload was unavailable"
        );
        assert!(
            captured
                .alab_bind_http_client_factory(manager.http_client_factory())
                .is_err(),
            "reload revived the old credential capture"
        );
    }
    #[test]
    fn snapshot_retains_its_actual_epoch_but_catalog_backend_cannot_keep_the_domain_alive() {
        let manager =
            AuthManager::from_auth_for_testing(CodexAuth::from_api_key("epoch-cycle-fixture"));
        let domain = manager.alab_assignment_domain();
        let weak = Arc::downgrade(&domain);
        let record = domain.catalog().unwrap().clone();
        let snapshot = manager.auth_cached().unwrap();
        drop(domain);
        drop(manager);
        assert!(
            weak.upgrade().is_some(),
            "actual retained snapshot owns its source"
        );
        drop(snapshot);
        assert!(
            weak.upgrade().is_none(),
            "Domain/Record/Backend must not own a cycle"
        );
        assert_eq!(record.phase().unwrap(), catalog::Phase::Unknown);
        assert!(record.domain().is_none());
    }
    #[test]
    fn real_sync_managers_defer_cache_and_restore_only_after_explicit_pre_effect_cancel() {
        const CHILD: &str = "ALAB_CATALOG_REGRESSION_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let name = format!(
                "{}::real_sync_managers_defer_cache_and_restore_only_after_explicit_pre_effect_cancel",
                module_path!().split_once("::").unwrap().1
            );
            assert!(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", &name, "--nocapture"])
                    .env(CHILD, "1")
                    .status()
                    .unwrap()
                    .success()
            );
            return;
        }
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let existing =
                    AuthManager::from_auth_for_testing(CodexAuth::from_api_key("test-catalog-key"));
                let record = existing.alab_assignment_domain().catalog().unwrap().clone();
                assert_eq!(record.phase().unwrap(), catalog::Phase::Ready);
                assert!(existing.auth_cached().is_some());
                let seal = catalog::global().seal().unwrap();
                let deferred = AuthManager::from_auth_for_testing(CodexAuth::from_api_key(
                    "test-deferred-key",
                ));
                assert_eq!(
                    deferred
                        .alab_assignment_domain()
                        .catalog()
                        .unwrap()
                        .phase()
                        .unwrap(),
                    catalog::Phase::Deferred
                );
                assert!(deferred.auth_cached().is_none());
                assert!(deferred.alab_catalog_ensure().await.is_err());
                assert!(existing.alab_assignment_domain().admit_work(None).is_err());
                seal.cancel_before_effects().unwrap();
                deferred.alab_catalog_ensure().await.unwrap();
                assert!(deferred.auth_cached().is_some());
                assert_eq!(
                    deferred
                        .alab_assignment_domain()
                        .catalog()
                        .unwrap()
                        .phase()
                        .unwrap(),
                    catalog::Phase::Ready
                );
            });
    }
}
