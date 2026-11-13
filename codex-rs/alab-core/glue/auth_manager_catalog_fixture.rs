// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Test configuration precedes publication; application-policy probes exclude credential leases.

use super::*;
/// Observe the shared application controller without a credential-epoch lease.
/// Source-scoped transports separately retire on credential publication.
pub(crate) fn application_policy_factory(manager: &AuthManager) -> HttpClientFactory {
    manager
        .auth_route_config
        .http_client_factory()
        .clone()
        .with_network_policy(
            manager
                .auth_route_config
                .application_network_policy()
                .clone()
                .for_current_account(),
        )
}
type Configure = Box<dyn FnOnce(&mut AuthManager)>;
thread_local! {
    static CONFIGURE: std::cell::RefCell<Option<Configure>> = const { std::cell::RefCell::new(None) };
}
pub(crate) fn configured(
    configure: impl FnOnce(&mut AuthManager) + 'static,
    construct: impl FnOnce() -> Arc<AuthManager>,
) -> Arc<AuthManager> {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            CONFIGURE.with(|slot| {
                slot.borrow_mut().take();
            });
        }
    }
    CONFIGURE.with(|slot| {
        assert!(slot.borrow().is_none(), "nested fixture configuration");
        *slot.borrow_mut() = Some(Box::new(configure));
    });
    let _reset = Reset;
    let manager = construct();
    CONFIGURE.with(|slot| assert!(slot.borrow().is_none(), "fixture was not attached"));
    manager
}
pub(super) fn before_attach(mut manager: Arc<AuthManager>) -> Arc<AuthManager> {
    if let Some(configure) = CONFIGURE.with(|slot| slot.borrow_mut().take()) {
        configure(Arc::get_mut(&mut manager).expect("fixture configuration before publication"));
    }
    manager
}

/// Observe destination policy separately from a captured credential epoch.
/// Each credential publication retires its old transport, while only the
/// primary owner can invalidate the shared application-policy permit.
pub(crate) struct ProviderPolicyProbe(codex_http_client::NetworkPermit);

impl ProviderPolicyProbe {
    pub(crate) fn new(policy: codex_http_client::NetworkPolicy, allowed: &url::Url) -> Self {
        Self(policy.for_current_account().acquire(allowed).unwrap())
    }

    pub(crate) fn renew(
        &self,
        manager: &AuthManager,
        old_transport: &HttpClientFactory,
        old_permit: &codex_http_client::NetworkPermit,
        allowed: &url::Url,
        blocked: &url::Url,
    ) -> (HttpClientFactory, codex_http_client::NetworkPermit) {
        self.0
            .check()
            .expect("provider publication preserves primary policy");
        assert!(old_permit.check().is_err(), "retired credential permit");
        assert!(
            old_transport.network_policy().acquire(allowed).is_err(),
            "retired credential transport"
        );
        let transport = manager.auth_cached().unwrap()
            .alab_bind_http_client_factory(manager.http_client_factory()).unwrap();
        let permit = transport.network_policy().acquire(allowed).unwrap();
        assert!(transport.network_policy().acquire(blocked).is_err());
        (transport, permit)
    }

    pub(crate) fn assert_revoked(&self) {
        assert!(
            self.0.check().is_err(),
            "primary replacement retires application policy"
        );
    }
}
