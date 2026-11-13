// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual HTTP refusal at a composed primary/gateway boundary; no native image proof.

use super::*;
use codex_api::Compression;
use codex_api::ResponsesClient;
use codex_http_client::ClientRouteClass;
use codex_http_client::DestinationPolicy;
use codex_http_client::HttpClientFactory;
use codex_http_client::NetworkPolicy;
use codex_http_client::NetworkPolicyController;
use codex_http_client::OutboundProxyPolicy;
use codex_http_client::ReqwestTransport;
use codex_login::default_client::ClientRedirectPolicy;
use codex_login::default_client::create_client_for_route;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;

struct Primary {
    policy: NetworkPolicy,
    reads: Arc<AtomicUsize>,
}

impl AuthProvider for Primary {
    fn alab_source_policy(&self) -> std::result::Result<Option<NetworkPolicy>, AuthError> {
        Ok(Some(self.policy.clone()))
    }

    fn add_auth_headers(&self, headers: &mut HeaderMap) -> std::result::Result<(), AuthError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        headers.insert(
            http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer fixture-primary"),
        );
        Ok(())
    }
}

async fn server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(
                    "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"fixture\"}}\n\n",
                ),
        )
        .mount(&server)
        .await;
    server
}

fn client(
    server: &MockServer,
    primary: NetworkPolicy,
    caller: NetworkPolicy,
) -> (ResponsesClient<ReqwestTransport>, Arc<AtomicUsize>) {
    let info = ModelProviderInfo::create_openai_provider(Some(server.uri()));
    let api = crate::resolve_api_provider(&info, None).unwrap();
    let factory =
        HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault).with_network_policy(caller);
    let transport = ReqwestTransport::from_http_client(
        create_client_for_route(
            &factory,
            &api.url_for_path("responses"),
            ClientRouteClass::Api,
            ClientRedirectPolicy::Reject,
        )
        .unwrap(),
    );
    let reads = Arc::new(AtomicUsize::new(0));
    let (name, value) = gateway_header(
        &GatewayOAuthDelivery::Cookie {
            name: "gateway".into(),
        },
        "fixture-gateway",
    )
    .unwrap();
    let auth = Arc::new(CombinedAuth {
        alab_recovery: None,
        primary: Arc::new(Primary {
            policy: primary,
            reads: reads.clone(),
        }),
        name,
        value,
    });
    (ResponsesClient::new(transport, api, auth), reads)
}

#[tokio::test]
async fn combined_auth_refuses_unavailable_primary_before_materialization_or_http() {
    let server = server().await;
    let controller = NetworkPolicyController::default();
    let (client, reads) = client(&server, controller.policy(), NetworkPolicy::unmanaged());
    let result = client
        .stream(
            serde_json::json!({"model": "fixture", "input": []}),
            HeaderMap::new(),
            Compression::None,
            None,
        )
        .await;
    assert!(
        result.is_err(),
        "missing primary permission must refuse the actual request"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn combined_auth_keeps_foreign_caller_refusal_and_retired_account_refusal() {
    let server = server().await;
    let a = NetworkPolicyController::default();
    let policy = a.policy();
    assert!(a.publish(policy.revision(), DestinationPolicy::Unrestricted));
    let source = policy.clone().for_current_account();
    let b = NetworkPolicyController::default();
    assert!(b.publish(b.policy().revision(), DestinationPolicy::Unrestricted));
    let (foreign, reads) = client(&server, source.clone(), b.policy());
    assert!(
        foreign
            .stream(
                serde_json::json!({}),
                HeaderMap::new(),
                Compression::None,
                None
            )
            .await
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert!(server.received_requests().await.unwrap().is_empty());

    let (matching, reads) = client(&server, source, policy.clone());
    let response = matching
        .stream(
            serde_json::json!({}),
            HeaderMap::new(),
            Compression::None,
            None,
        )
        .await
        .unwrap();
    drop(response);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].headers["authorization"],
        "Bearer fixture-primary"
    );
    assert_eq!(requests[0].headers["cookie"], "gateway=fixture-gateway");
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    policy.invalidate();
    assert!(a.publish(policy.revision(), DestinationPolicy::Unrestricted));
    assert!(
        matching
            .stream(
                serde_json::json!({}),
                HeaderMap::new(),
                Compression::None,
                None
            )
            .await
            .is_err()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
