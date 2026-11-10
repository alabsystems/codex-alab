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
