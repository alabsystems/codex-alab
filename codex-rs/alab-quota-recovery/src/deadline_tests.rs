// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Backend outages must not take scheduling past a known natural quota reset.

use super::*;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[tokio::test]
async fn a_usage_outage_returns_the_known_reset_to_native_scheduling() {
    outage_at_reset(false).await;
}

#[tokio::test]
async fn an_in_flight_usage_request_cannot_hold_the_known_reset() {
    outage_at_reset(true).await;
}

async fn outage_at_reset(slow_request: bool) {
    codex_alab_core::activate();
    let server = MockServer::start().await;
    let mut response = ResponseTemplate::new(503);
    if slow_request {
        response = response.set_delay(Duration::from_secs(60));
    }
    Mock::given(method("GET"))
        .and(path("/api/codex/usage"))
        .respond_with(response)
        .expect(1)
        .mount(&server)
        .await;
    for endpoint in [
        "/api/codex/rate-limit-reset-credits",
        "/api/codex/rate-limit-reset-credits/consume",
    ] {
        Mock::given(path(endpoint))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
    }
    let home = tempfile::tempdir().unwrap();
    let auth = codex_login::CodexAuth::from_external_chatgpt_tokens("e30.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjb3VudCIsImNoYXRncHRfdXNlcl9pZCI6InVzZXIiLCJjaGF0Z3B0X3BsYW5fdHlwZSI6InBsdXMifX0.fixture", "account", Some("plus")).unwrap();
    let manager = AuthManager::from_auth_for_testing_with_home(auth, home.path().to_owned());
    let state = State::bind(&manager).await;
    let config = Settings {
        home: home.path().to_owned(),
        base: server.uri(),
        factory: HttpClientFactory::new(codex_http_client::OutboundProxyPolicy::ReqwestDefault),
    };
    let reset = chrono::Utc::now().timestamp() + 1;
    let recovered = tokio::time::timeout(
        Duration::from_secs(3),
        wait_for_reset(
            &manager,
            &config,
            state.owner.as_ref().unwrap(),
            "original-turn",
            Duration::from_secs(60),
            Some(reset),
        ),
    )
    .await
    .expect("backend recovery held the request beyond its natural reset");
    assert!(
        !recovered,
        "the native quota policy must own the deadline retry"
    );
}
