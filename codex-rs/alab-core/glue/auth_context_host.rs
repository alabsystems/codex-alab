// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A host is a control-plane selector, never a credential epoch or reader.
//! Real managers never delegate: factories retain the captured real Arc.
use super::*;
use codex_alab_core::context_host::{Group, Host};

static HOSTS: OnceLock<Arc<Group<AuthManager>>> = OnceLock::new();
fn group() -> &'static Arc<Group<AuthManager>> { HOSTS.get_or_init(|| Arc::new(Group::default())) }

pub(super) fn facade(real: Arc<AuthManager>) -> Arc<AuthManager> {
    if !codex_alab_core::native_source_launch::requested() || real.alab_context_host.is_some() { return real; }
    wrap(real)
}
pub(super) fn wrap(real: Arc<AuthManager>) -> Arc<AuthManager> {
    let host = match group().register(real.clone()) {
        Ok(host) => host,
        Err(_) => Host::closed(real.clone()),
    };
    // These inert upstream fields cannot issue a source: the explicit host role
    // has no catalog registration/member, and its native state is Detached.
    // Public data entrypoints select the real epoch; native proof APIs refuse H.
    Arc::new(AuthManager {
        alab_context_host: Some(host),
        codex_home: real.codex_home.clone(),
        inner: RwLock::new(CachedAuth { auth: None, permanent_refresh_failure: None }),
        // Default receivers delegate to actual real managers. These inert
        // compatibility fields must never mutate a real epoch's watch state.
        auth_change_tx: watch::channel(*real.auth_change_tx.borrow()).0,
        auth_change_state_tx: watch::channel(*real.auth_change_state_tx.borrow()).0,
        alab_assignment_domain: real.alab_assignment_domain.clone(),
        alab_source_member: RwLock::new(None),
        alab_native_source: RwLock::new(super::alab_native_source::State::Detached),
        workspace_routing_resolver: OnceLock::new(),
        enable_codex_api_key_env: false,
        auth_credentials_store_mode: real.auth_credentials_store_mode,
        keyring_backend_kind: real.keyring_backend_kind,
        forced_login_method: real.forced_login_method,
        forced_chatgpt_workspace_id: RwLock::new(None),
        managed_auth_policy: real.managed_auth_policy.clone(),
        chatgpt_base_url: real.chatgpt_base_url.clone(),
        agent_identity_authapi_base_url: real.agent_identity_authapi_base_url.clone(),
        refresh_lock: Arc::new(Semaphore::new(1)),
        unsettled_chatgpt_refresh: Mutex::new(None),
        agent_identity_lock: Semaphore::new(1),
        agent_identity_bootstrap_cooldown: Mutex::default(),
        external_auth: RwLock::new(None),
        workload_identity_selected: false,
        auth_route_config: real.auth_route_config.clone(),
        owns_network_policy: false,
        allowed_auth_modes: RwLock::new(None),
    })
}
impl AuthManager {
    /// Long-lived control-plane observers must rebind their real-manager watch
    /// receivers after this wake, even when A and B's actual counters coincide.
    /// A captured real manager never changes into another manager.
    pub async fn alab_current_manager_changed(&self, previous: &Arc<Self>) -> std::io::Result<()> {
        match &self.alab_context_host {
            Some(host) => host.changed(previous).await,
            None => std::future::pending().await,
        }
    }
    pub fn alab_capture_current_manager(self: &Arc<Self>) -> std::io::Result<Arc<Self>> {
        match &self.alab_context_host {
            None => Ok(self.clone()),
            Some(host) => {
                let actual = host.capture()?;
                if actual.alab_context_host.is_some() { return Err(codex_alab_core::managed_auth::unavailable()); }
                Ok(actual)
            }
        }
    }
    pub(super) fn alab_host_capture(&self) -> Option<std::io::Result<Arc<Self>>> {
        self.alab_context_host.as_ref().map(|host| {
            let actual = host.capture()?;
            if actual.alab_context_host.is_some() { return Err(codex_alab_core::managed_auth::unavailable()); }
            Ok(actual)
        })
    }
    pub(super) fn alab_host_metadata(&self) -> Option<Arc<Self>> {
        self.alab_context_host
            .as_ref()
            .and_then(|host| host.metadata_current().ok())
    }
    pub(super) fn alab_host_policy(&self) -> Option<codex_http_client::NetworkPolicy> {
        self.alab_context_host.as_ref().map(|host| match host.capture() {
            Ok(actual) => actual.application_network_policy(),
            Err(_) => self.auth_route_config.application_network_policy().clone().with_native_source(Err(())),
        })
    }
    pub(super) fn alab_host_factory(&self) -> Option<HttpClientFactory> {
        self.alab_context_host.as_ref().map(|host| match host.capture() {
            Ok(actual) => actual.http_client_factory(),
            Err(_) => {
                self.auth_route_config.http_client_factory().clone().with_network_policy(
                    self.auth_route_config.application_network_policy().clone().with_native_source(Err(())))
            }
        })
    }
    pub(super) fn alab_real_epoch(&self) -> std::io::Result<()> {
        if self.alab_context_host.is_some() { Err(codex_alab_core::managed_auth::unavailable()) } else { Ok(()) }
    }
    pub(super) fn alab_host_expected(&self, expected: &CodexAuth) -> std::io::Result<()> {
        let selected = match self.alab_host_capture() {
            Some(actual) => Some(actual?),
            None => None,
        };
        let actual = selected.as_deref().unwrap_or(self);
        let Some(captured) = expected.alab_credential_epoch().epoch() else {
            return if selected.is_some() || expected.alab_credential_epoch().has_native_member()
                || actual.alab_native_overridden() {
                Err(codex_alab_core::managed_auth::unavailable())
            } else { Ok(()) };
        };
        let epoch = actual.alab_source_epoch().map_err(|_| codex_alab_core::managed_auth::unavailable())?;
        if !captured.same(&epoch) { return Err(codex_alab_core::managed_auth::unavailable()); }
        Ok(())
    }
}

macro_rules! metadata {
    ($this:expr, $method:ident($($argument:expr),*)) => {
        if let Some(actual) = $this.alab_host_metadata() { return actual.$method($($argument),*); }
    };
}
macro_rules! option {
    ($this:expr, $method:ident($($argument:expr),*)) => {
        if let Some(actual) = $this.alab_host_capture() { return actual.ok().and_then(|actual| actual.$method($($argument),*)); }
    };
}
macro_rules! io {
    ($this:expr, $method:ident($($argument:expr),*)) => {
        if let Some(actual) = $this.alab_host_capture() { return actual.and_then(|actual| actual.$method($($argument),*)); }
    };
}
macro_rules! async_option {
    ($this:expr, $method:ident($($argument:expr),*)) => {
        if let Some(actual) = $this.alab_host_capture() { return match actual { Ok(actual) => Box::pin(actual.$method($($argument),*)).await, Err(_) => None }; }
    };
}
macro_rules! async_result {
    ($this:expr, $method:ident($($argument:expr),*)) => {
        if let Some(actual) = $this.alab_host_capture() { return Box::pin(actual?.$method($($argument),*)).await; }
    };
}
macro_rules! async_bool {
    ($this:expr, $method:ident($($argument:expr),*)) => {
        if let Some(actual) = $this.alab_host_capture() { return match actual { Ok(actual) => Box::pin(actual.$method($($argument),*)).await, Err(_) => false }; }
    };
}
pub(super) use {async_bool, async_option, async_result, io, metadata, option};

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    #[test]
    fn credential_export_rechecks_before_delivering_rendered_response() {
        use std::io::Write as _;
        let a = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("same-fixture"));
        let b = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("same-fixture"));
        let host = wrap(a.clone());
        let original = host.auth_cached().unwrap();
        let changes = host.auth_change_receiver();
        let export = host
            .alab_prepare_credential_export(&original, "same-fixture".into())
            .unwrap();
        let mut output = Vec::new();
        let result = export.write_with(&mut output, |staged, token| {
            writeln!(staged, "credential={token}")?;
            group()
                .publish(&[codex_alab_core::context_host::Replacement {
                    host: host.alab_context_host.as_ref().unwrap(),
                    expected: &a,
                    next: b.clone(),
                }])
                .unwrap();
            Ok(())
        });
        assert!(Arc::ptr_eq(
            &host.alab_capture_current_manager().unwrap(),
            &b
        ));
        assert!(!changes.has_changed().unwrap());
        assert!(result.is_err());
        assert!(output.is_empty(), "no partial protocol record may escape");
        host.alab_context_host.as_ref().unwrap().close();
    }

    #[test]
    fn credential_export_refuses_host_replacement_with_unchanged_watch_counters() {
        let a = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("same-fixture"));
        let b = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("same-fixture"));
        let host = wrap(a.clone());
        let original = host.auth_cached().unwrap();
        let changes = host.auth_change_receiver();
        let export = host
            .alab_prepare_credential_export(&original, "same-fixture".into())
            .unwrap();
        assert!(
            host.alab_validate_credential_export(&original, "same-fixture")
                .is_ok()
        );
        group()
            .publish(&[codex_alab_core::context_host::Replacement {
                host: host.alab_context_host.as_ref().unwrap(),
                expected: &a,
                next: b,
            }])
            .unwrap();
        assert!(!changes.has_changed().unwrap());
        assert!(
            host.alab_prepare_credential_export(&original, "same-fixture".into())
                .is_err()
        );
        assert!(
            host.alab_validate_credential_export(&original, "same-fixture")
                .is_err()
        );
        let mut output = Vec::new();
        assert!(
            export
                .write_with(&mut output, |_, _| panic!("refused source must not render"))
                .is_err()
        );
        assert!(output.is_empty(), "a refused export writes no bearer bytes");
        let current = host.auth_cached().unwrap();
        assert!(
            host.alab_validate_credential_export(&current, "same-fixture")
                .is_ok()
        );
        host.alab_context_host.as_ref().unwrap().close();
        assert!(
            host.alab_validate_credential_export(&current, "same-fixture")
                .is_err()
        );
    }
    #[tokio::test]
    async fn async_host_dispatch_follows_real_epochs_and_refuses_a_closed_host() {
        let a = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("fixture-a"));
        let b = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("fixture-b"));
        let host = wrap(a.clone());
        assert_eq!(host.auth().await.unwrap().get_token().unwrap(), "fixture-a");
        group().publish(&[codex_alab_core::context_host::Replacement {
            host: host.alab_context_host.as_ref().unwrap(), expected: &a, next: b,
        }]).unwrap();
        assert_eq!(host.auth().await.unwrap().get_token().unwrap(), "fixture-b");
        host.alab_context_host.as_ref().unwrap().close();
        assert!(host.auth().await.is_none());
        assert!(!host.reload().await);
        assert!(host.reload_with_cached_auth().await.is_err());
    }
    #[tokio::test]
    async fn pointer_wake_rebinds_actual_receiver_even_when_counters_are_equal() {
        let a = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("fixture-a"));
        let b = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("fixture-b"));
        assert_eq!(*a.auth_change_state_receiver().borrow(), *b.auth_change_state_receiver().borrow());
        let host = wrap(a.clone());
        let original = host.auth_change_state_receiver();
        let wake = host.alab_current_manager_changed(&a);
        tokio::pin!(wake);
        assert!(std::future::poll_fn(|cx| std::task::Poll::Ready(wake.as_mut().poll(cx).is_pending())).await);
        group().publish(&[codex_alab_core::context_host::Replacement {
            host: host.alab_context_host.as_ref().unwrap(), expected: &a, next: b.clone(),
        }]).unwrap();
        wake.await.unwrap();
        let selected = host.alab_capture_current_manager().unwrap();
        let rebound = selected.auth_change_state_receiver();
        assert!(Arc::ptr_eq(&selected, &b));
        assert!(original.same_channel(&a.auth_change_state_receiver()));
        assert!(rebound.same_channel(&b.auth_change_state_receiver()));
        assert!(!original.same_channel(&rebound));
    }
    #[tokio::test]
    async fn typed_pair_follows_the_actual_host_context_and_refuses_a_closed_host() {
        let a = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("pair-a"));
        let b = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("pair-b"));
        let host = wrap(a.clone());
        let (old, old_factory) = host.alab_auth_with_http_client_factory().await.unwrap().unwrap();
        group().publish(&[codex_alab_core::context_host::Replacement {
            host: host.alab_context_host.as_ref().unwrap(), expected: &a, next: b.clone(),
        }]).unwrap();
        let (selected, factory) = host.alab_auth_with_http_client_factory().await.unwrap().unwrap();
        assert!(selected.alab_credential_epoch().epoch().unwrap().same(&b.alab_source_epoch().unwrap()));
        assert!(old.alab_credential_epoch().epoch().unwrap().same(&a.alab_source_epoch().unwrap()));
        assert!(old.alab_bind_http_client_factory(factory).is_err());
        assert!(selected.alab_bind_http_client_factory(old_factory).is_err());
        host.alab_context_host.as_ref().unwrap().close();
        assert!(host.alab_auth_with_http_client_factory().await.is_err());
    }
    #[test]
    fn host_selects_real_context_without_relabeling_captured_auth_or_owning_watch_authority() {
        let a = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("fixture-a"));
        let b = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("fixture-b"));
        let old_auth = a.auth_cached().unwrap();
        let old_state = *a.auth_change_state_receiver().borrow();
        let b_state = *b.auth_change_state_receiver().borrow();
        let host = wrap(a.clone());
        assert!(host.inner.read().unwrap().auth.is_none());
        assert!(host.alab_source_member.read().unwrap().is_none());
        assert!(Arc::ptr_eq(&host.alab_capture_current_manager().unwrap(), &a));
        assert!(host.alab_native_detach().is_err());
        group().publish(&[codex_alab_core::context_host::Replacement {
            host: host.alab_context_host.as_ref().unwrap(), expected: &a, next: b.clone(),
        }]).unwrap();
        assert!(Arc::ptr_eq(&host.alab_capture_current_manager().unwrap(), &b));
        assert_eq!(*host.auth_change_state_receiver().borrow(), b_state);
        assert!(host.auth_change_state_for(&old_auth).is_none());
        assert!(old_auth.alab_credential_epoch().epoch().unwrap().same(&a.alab_source_epoch().unwrap()));
        host.auth_change_state_tx.send_modify(|state| state.generation += 1);
        assert_eq!(*a.auth_change_state_receiver().borrow(), old_state);
        assert_eq!(*b.auth_change_state_receiver().borrow(), b_state);
        host.alab_context_host.as_ref().unwrap().close();
        assert!(host.alab_capture_current_manager().is_err());
        assert!(host.auth_cached().is_none());
        assert!(Arc::ptr_eq(&host.alab_assignment_domain(), &b.alab_assignment_domain()));
    }
}
