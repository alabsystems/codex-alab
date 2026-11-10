// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_alab_core::assignment::FreezeError;
use codex_alab_core::assignment::Intent;
use codex_alab_core::assignment::Phase;
use codex_alab_core::assignment::Selection;
use std::sync::Arc;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

struct Fixture {
    server: MockServer,
    _home: tempfile::TempDir,
    manager: Arc<AuthManager>,
    state: State,
    config: Settings,
}
impl Fixture {
    async fn new() -> Self {
        codex_alab_core::activate();
        let server = MockServer::start().await;
        let home = tempfile::tempdir().unwrap();
        let auth = codex_login::CodexAuth::from_external_chatgpt_tokens("e30.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjb3VudCIsImNoYXRncHRfdXNlcl9pZCI6InVzZXIiLCJjaGF0Z3B0X3BsYW5fdHlwZSI6InBsdXMifX0.fixture", "account", Some("plus")).unwrap();
        let manager = AuthManager::from_auth_for_testing_with_home(auth, home.path().to_owned());
        let state = State::bind(&manager).await;
        let config = Settings {
            home: home.path().to_owned(),
            base: server.uri(),
            factory: HttpClientFactory::new(codex_http_client::OutboundProxyPolicy::ReqwestDefault),
        };
        Self {
            server,
            _home: home,
            manager,
            state,
            config,
        }
    }
    fn usage() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "account_id":"account", "user_id":"user", "plan_type":"plus",
            "rate_limit":{"allowed":true,"limit_reached":false,
                "secondary_window":{"used_percent":0,"limit_window_seconds":604800,
                    "reset_at":chrono::Utc::now().timestamp() + 3600,"reset_after_seconds":3600}}
        }))
    }
}
fn intent() -> Intent {
    Intent {
        operation: "a".repeat(32),
        thread: "00000000-0000-0000-0000-000000000000".into(),
        selection: Selection::Auto {
            revision: "b".repeat(64),
        },
    }
}

#[tokio::test]
async fn quota_attempts_wait_for_pending_assignment_without_http() {
    let fixture = Fixture::new().await;
    Mock::given(method("GET"))
        .and(path("/api/codex/usage"))
        .respond_with(Fixture::usage())
        .expect(1)
        .mount(&fixture.server)
        .await;
    let domain = fixture.manager.alab_assignment_domain();
    domain.request(intent()).unwrap();
    for purpose in [Purpose::Recover, Purpose::Preflight] {
        assert_eq!(
            recover(
                &fixture.manager,
                &fixture.config,
                fixture.state.owner.as_ref().unwrap(),
                "turn",
                purpose
            )
            .await,
            Recovery::Wait
        );
    }
    assert!(fixture.server.received_requests().await.unwrap().is_empty());
    domain.cancel_pending(&intent().operation).unwrap();
    assert_eq!(
        recover(
            &fixture.manager,
            &fixture.config,
            fixture.state.owner.as_ref().unwrap(),
            "turn",
            Purpose::Recover
        )
        .await,
        Recovery::Restored
    );
}

#[tokio::test]
async fn source_retirement_drains_a_quota_http_attempt_until_cancellation() {
    let fixture = Fixture::new().await;
    let posted = Arc::new(tokio::sync::Notify::new());
    let observed = Arc::clone(&posted);
    Mock::given(method("GET"))
        .and(path("/api/codex/usage"))
        .respond_with(move |_: &wiremock::Request| {
            observed.notify_one();
            Fixture::usage().set_delay(Duration::from_secs(30))
        })
        .expect(1)
        .mount(&fixture.server)
        .await;
    let domain = fixture.manager.alab_assignment_domain();
    let retirement = {
        let attempt = recover(
            &fixture.manager,
            &fixture.config,
            fixture.state.owner.as_ref().unwrap(),
            "turn",
            Purpose::Recover,
        );
        tokio::pin!(attempt);
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::select! {
                _ = &mut attempt => panic!("quota HTTP completed before retirement"),
                _ = posted.notified() => {}
            }
        })
        .await
        .unwrap();
        domain.request(intent()).unwrap();
        let retirement = domain.try_retire(&intent().operation).unwrap();
        match retirement.freeze() {
            Err(FreezeError::Pending(retirement)) => retirement,
            _ => panic!("source froze while its quota HTTP attempt was still running"),
        }
        // Dropping the quota attempt must release its source resource too.
    };
    let _boundary = tokio::time::timeout(Duration::from_secs(5), retirement.drained())
        .await
        .expect("cancelled quota attempt kept source retirement blocked")
        .unwrap();
    assert_eq!(domain.phase(), Phase::Frozen);
}

#[tokio::test]
async fn source_retirement_can_finish_while_quota_wait_sleeps() {
    let fixture = Fixture::new().await;
    Mock::given(method("GET"))
        .and(path("/api/codex/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "account_id":"account", "user_id":"user", "plan_type":"plus",
            "rate_limit":{"allowed":false,"limit_reached":true,
                "secondary_window":{"used_percent":100,"limit_window_seconds":604800,
                    "reset_at":chrono::Utc::now().timestamp() + 3600,"reset_after_seconds":3600}}
        })))
        .expect(1)
        .mount(&fixture.server)
        .await;
    let listed = Arc::new(tokio::sync::Notify::new());
    let observed = Arc::clone(&listed);
    Mock::given(method("GET"))
        .and(path("/api/codex/rate-limit-reset-credits"))
        .respond_with(move |_: &wiremock::Request| {
            observed.notify_one();
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "available_count":0,"credits":[]
            }))
        })
        .expect(1)
        .mount(&fixture.server)
        .await;
    let waiting = wait_for_reset(
        &fixture.manager,
        &fixture.config,
        fixture.state.owner.as_ref().unwrap(),
        "turn",
        Duration::from_secs(30),
        None,
    );
    tokio::pin!(waiting);
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::select! {
            _ = &mut waiting => panic!("quota wait ended before a credit was available"),
            _ = listed.notified() => {}
        }
    })
    .await
    .unwrap();
    let domain = fixture.manager.alab_assignment_domain();
    domain.request(intent()).unwrap();
    let retirement = domain.try_retire(&intent().operation).unwrap();
    let _boundary = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::select! {
            _ = &mut waiting => panic!("retirement should not require ending the quota wait"),
            boundary = retirement.drained() => boundary.unwrap()
        }
    })
    .await
    .expect("quota polling sleep retained its source lease");
    assert_eq!(domain.phase(), Phase::Frozen);
    assert_eq!(fixture.server.received_requests().await.unwrap().len(), 2);
}
