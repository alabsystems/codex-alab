// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exercise the real stored-auth check and Attach path without native/global state.

use super::*;
use codex_alab_core::assignment::Generations;
use codex_alab_core::assignment::Refused;
use codex_alab_core::assignment_service::Target;
use codex_alab_core::manager_catalog::Backend;
use codex_alab_core::manager_catalog::Registry;
use codex_alab_core::manager_catalog::Scope;
use codex_alab_core::manager_catalog::Version;
use codex_http_client::HttpClientFactory;
use codex_http_client::NetworkPolicy;
use codex_http_client::OutboundProxyPolicy;
use codex_login::AuthCredentialsStoreMode;
use codex_login::AuthRouteConfig;
use std::future::Future;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

// Synthetic unsigned JWT with no expiration; no real credential or account is read.
const TOKEN: &str = "eyJhbGciOiJub25lIn0.eyJzdWIiOiJmaXh0dXJlLXVzZXIiLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF91c2VyX2lkIjoiZml4dHVyZS11c2VyIiwiY2hhdGdwdF9hY2NvdW50X2lkIjoiZml4dHVyZS1hY2NvdW50In19.fixture";
const ATTACH: &str = "/oaiproto.codex_app_server_backend.ThreadService/Attach";

struct Owner(watch::Sender<Generations>);

impl Version for Owner {
    fn current(&self) -> Generations {
        self.0.borrow().clone()
    }

    fn changed<'a>(
        &'a self,
        observed: &'a Generations,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let mut changes = self.0.subscribe();
            let _ = changes.wait_for(|current| current != observed).await;
        })
    }
}

impl Backend for Owner {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn check_target(&self, _: &Target) -> Result<(), Refused> {
        Ok(())
    }

    fn source_stamp(&self) -> Result<Generations, Refused> {
        Ok(self.current())
    }
}

#[tokio::test]
async fn cloud_idle_recovery_reads_and_attaches_only_under_its_original_source() {
    assert!(
        std::env::var_os(codex_login::auth::CODEX_ACCESS_TOKEN_ENV_VAR).is_none(),
        "the isolated storage fixture must not read an external access token"
    );
    // The first case drains while Recovery is still idle. The other two drive
    // the real reconnect before draining; each owns a fresh retirement lifecycle.
    for (changed_source, reconnect) in [(false, false), (false, true), (true, true)] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(ATTACH))
            // Terminal rejection ends the real Attach path without retry delays.
            .respond_with(ResponseTemplate::new(403))
            .expect(if reconnect && !changed_source { 1 } else { 0 })
            .mount(&server)
            .await;
        let home = tempfile::tempdir().unwrap();
        let auth_path = home.path().join("auth.json");
        assert!(codex_alab_core::home::auth_file(home.path()) == auth_path);
        assert!(!codex_alab_core::native_source_launch::requested());
        std::fs::write(
            auth_path,
            serde_json::to_vec(&serde_json::json!({
                "auth_mode": "chatgpt",
                "last_refresh": "2026-11-12T00:00:00Z",
                "tokens": {
                    "id_token": TOKEN,
                    "access_token": TOKEN,
                    "refresh_token": "synthetic-refresh",
                    "account_id": "fixture-account",
                    "login_owner_id": "fixture-login"
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let factory = HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault);
        let storage = AuthConfig {
            codex_home: home.path().to_owned(),
            auth_credentials_store_mode: AuthCredentialsStoreMode::File,
            keyring_backend_kind: Default::default(),
            forced_login_method: None,
            chatgpt_base_url: None,
            forced_chatgpt_workspace_id: None,
            managed_auth_policy: Default::default(),
            auth_route_config: AuthRouteConfig::from_http_client_factory(factory.clone()),
        };
        let registry = Arc::new(Registry::default());
        let admitted = registry.factory(None).unwrap();
        let domain = admitted.domain();
        let owner = Arc::new(Owner(
            watch::channel(Generations {
                auth: 1,
                owner: 1,
                login: 1,
                source: 1,
            })
            .0,
        ));
        admitted.record().attach(owner.clone()).unwrap();
        let record = admitted.complete().unwrap();
        assert_eq!(
            record.phase(),
            Ok(codex_alab_core::manager_catalog::Phase::Ready)
        );
        assert!(record.backend().unwrap().is_some());
        let policy = NetworkPolicy::unmanaged().with_manager_source(Ok(Scope::capture(
            domain,
            owner.clone(),
        )
        .unwrap()));
        let (observer, _, _) = crate::CloudClient::channel(/*capacity*/ 1);
        let handle = observer.request_handle();
        let mut recovery = Recovery {
            state: handle.reconnect.clone(),
            pending: None,
            installed: None,
            observed_idle: Some(true),
            client: Arc::new(
                cloud::Client::new(
                    &factory,
                    &server.uri(),
                    cloud::Credentials::new(TOKEN, "fixture-account").unwrap(),
                )
                .unwrap(),
            ),
            login: Arc::new(Login {
                storage,
                mode: AuthMode::Chatgpt,
                token: TOKEN.into(),
                account: "fixture-account".into(),
                permit: policy
                    .alab_idle_permit(&server.uri().parse().unwrap())
                    .unwrap(),
            }),
        };
        if changed_source {
            let mut next = owner.current();
            next.source += 1;
            owner.0.send_replace(next);
        }
        // Identical stored token/account still passes the real value check. It
        // cannot establish that the original source is authorized to reconnect.
        assert!(recovery.login.current().await);
        if reconnect {
            recovery.offer("fixture-thread");
            assert!(handle.reconnect_idle_thread("fixture-thread"));
            let (_, open) = recovery.pending.take().unwrap();
            let outcome = tokio::time::timeout(Duration::from_secs(12), open)
                .await
                .expect("the existing eight-second recovery budget remains bounded");
            assert!(outcome.is_none());
        }
        let seal = registry.seal().unwrap();
        let mut drain = std::pin::pin!(seal.drain_resources());
        assert_eq!(
            drain.as_mut().poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Ok(())),
            "idle or completed Recovery retains no source activity"
        );
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            usize::from(reconnect && !changed_source)
        );
    }
}
