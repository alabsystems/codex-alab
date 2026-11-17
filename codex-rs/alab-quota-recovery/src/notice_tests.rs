// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! User-visible reset decisions exercise the same HTTP flow that owns redemption.

use super::*;
use std::sync::Arc;
use std::sync::Mutex;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

const USAGE: &str = "/api/codex/usage";
const CREDITS: &str = "/api/codex/rate-limit-reset-credits";
const CONSUME: &str = "/api/codex/rate-limit-reset-credits/consume";

fn usage(used: i32, remaining: i64, allowed: bool) -> serde_json::Value {
    serde_json::json!({
        "account_id":"account", "user_id":"user", "plan_type":"plus",
        "rate_limit":{"allowed":allowed,"limit_reached":!allowed,
            "secondary_window":{"used_percent":used,"limit_window_seconds":604800,
                "reset_at":chrono::Utc::now().timestamp() + remaining,
                "reset_after_seconds":remaining}}
    })
}

struct Fixture {
    server: MockServer,
    home: tempfile::TempDir,
    manager: Arc<AuthManager>,
    body: Arc<Mutex<serde_json::Value>>,
    state: State,
    notices: tokio::sync::mpsc::UnboundedReceiver<String>,
}

impl Fixture {
    async fn new(used: i32, remaining: i64, allowed: bool) -> Self {
        codex_alab_core::activate();
        let server = MockServer::start().await;
        let body = Arc::new(Mutex::new(usage(used, remaining, allowed)));
        let served = Arc::clone(&body);
        Mock::given(method("GET"))
            .and(path(USAGE))
            .respond_with(move |_: &wiremock::Request| {
                ResponseTemplate::new(200).set_body_json(served.lock().unwrap().clone())
            })
            .mount(&server)
            .await;
        let home = tempfile::tempdir().unwrap();
        let auth = codex_login::CodexAuth::from_external_chatgpt_tokens(
            "e30.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjb3VudCIsImNoYXRncHRfdXNlcl9pZCI6InVzZXIiLCJjaGF0Z3B0X3BsYW5fdHlwZSI6InBsdXMifX0.fixture",
            "account",
            Some("plus"),
        )
        .unwrap();
        let manager = AuthManager::from_auth_for_testing_with_home(auth, home.path().to_owned());
        let state = State::bind(&manager).await;
        let (sender, notices) = tokio::sync::mpsc::unbounded_channel();
        state.set_notice_sender(sender);
        Self {
            server,
            home,
            manager,
            body,
            state,
            notices,
        }
    }

    fn factory() -> HttpClientFactory {
        HttpClientFactory::new(codex_http_client::OutboundProxyPolicy::ReqwestDefault)
    }

    async fn offer_credits(&self) {
        Mock::given(method("GET"))
            .and(path(CREDITS))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "available_count":2,
                "credits":[
                    {"id":"later","reset_type":"codex_rate_limits","status":"available",
                        "granted_at":"2026-01-01T00:00:00Z","expires_at":"2030-02-01T00:00:00Z"},
                    {"id":"earlier","reset_type":"codex_rate_limits","status":"available",
                        "granted_at":"2026-01-01T00:00:00Z","expires_at":"2030-01-01T00:00:00Z"}
                ]
            })))
            .mount(&self.server)
            .await;
    }

    async fn preflight(&self) {
        preflight(
            &self.manager,
            self.home.path(),
            &self.server.uri(),
            Self::factory(),
            "notice-turn",
            &self.state,
            None,
        )
        .await;
    }

    fn messages(&mut self) -> String {
        let mut notices = Vec::new();
        while let Ok(notice) = self.notices.try_recv() {
            notices.push(notice);
        }
        notices.join("\n")
    }

    async fn posts(&self) -> Vec<serde_json::Value> {
        self.server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| request.url.path() == CONSUME)
            .map(|request| serde_json::from_slice(&request.body).unwrap())
            .collect()
    }
}

/// A refusal explains the decision using the credit inventory, even when the
/// natural refresh is too near to permit an automatic redemption.
#[tokio::test]
async fn reset_notice_lists_count_and_expirations_when_preserving_credits() {
    let mut fixture = Fixture::new(100, 24 * 3600, true).await;
    fixture.offer_credits().await;
    fixture.preflight().await;
    let messages = fixture.messages();
    assert!(
        messages.contains("Considered earned usage reset"),
        "{messages}"
    );
    assert!(messages.contains("2 available"), "{messages}");
    assert!(messages.contains("2030-01-01"), "{messages}");
    assert!(messages.contains("2030-02-01"), "{messages}");
    assert!(messages.contains("24 hours"), "{messages}");
    assert!(!messages.contains("Reset recovery stopped"), "{messages}");
    assert!(fixture.posts().await.is_empty());
    fixture.preflight().await;
    assert!(
        fixture.messages().is_empty(),
        "an unchanged decision must not repeat before every model request"
    );
}

/// The transcript must describe both the spending decision and the confirmed
/// result, while the HTTP assertion proves the earliest-expiring credit was used.
#[tokio::test]
async fn reset_notice_reports_automatic_redemption_and_restored_usage() {
    let mut fixture = Fixture::new(100, 25 * 3600, false).await;
    fixture.offer_credits().await;
    let served = Arc::clone(&fixture.body);
    Mock::given(method("POST"))
        .and(path(CONSUME))
        .respond_with(move |_: &wiremock::Request| {
            *served.lock().unwrap() = usage(0, 7 * 24 * 3600, true);
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"code":"reset","windows_reset":2}))
        })
        .expect(1)
        .mount(&fixture.server)
        .await;
    assert!(
        tokio::time::timeout(
            Duration::from_secs(5),
            recover_weekly(
                &fixture.manager,
                fixture.home.path(),
                &fixture.server.uri(),
                Fixture::factory(),
                "notice-turn",
                &fixture.state,
                crate::Wall::default(),
            ),
        )
        .await
        .unwrap()
    );
    let messages = fixture.messages();
    assert!(
        messages.contains("Considered earned usage reset"),
        "{messages}"
    );
    assert!(messages.contains("2 available"), "{messages}");
    assert!(messages.contains("2030-01-01"), "{messages}");
    assert!(messages.contains("2030-02-01"), "{messages}");
    assert!(messages.to_lowercase().contains("automatic"), "{messages}");
    assert!(messages.to_lowercase().contains("restor"), "{messages}");
    let posts = fixture.posts().await;
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0]["credit_id"], "earlier");
}

#[tokio::test]
async fn reset_notice_reports_unread_inventory_without_claiming_no_credits() {
    let mut fixture = Fixture::new(100, 72 * 3600, true).await;
    Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(ResponseTemplate::new(503))
        .mount(&fixture.server)
        .await;
    fixture.preflight().await;
    let messages = fixture.messages().to_lowercase();
    assert!(messages.contains("reset"), "{messages}");
    assert!(
        messages.contains("unavailable")
            || messages.contains("could not")
            || messages.contains("unknown"),
        "{messages}"
    );
    assert!(!messages.contains("0 available"), "{messages}");
    assert!(fixture.posts().await.is_empty());
}

#[tokio::test]
async fn healthy_preflight_does_not_print_reset_decisions() {
    let mut fixture = Fixture::new(25, 72 * 3600, true).await;
    fixture.preflight().await;
    assert!(fixture.messages().is_empty());
    assert!(fixture.posts().await.is_empty());
}

/// Crossing the conservative read threshold requires fresh reads, while the
/// authenticated usage is still below the threshold for considering a reset.
#[tokio::test]
async fn preflight_between_clearance_and_redemption_thresholds_stays_quiet() {
    for used in [95, 96, 98] {
        let mut fixture = Fixture::new(used, 72 * 3600, true).await;
        fixture.preflight().await;
        assert!(fixture.messages().is_empty(), "used={used}");
        assert!(fixture.state.clear().is_none(), "used={used}");
        assert!(fixture.posts().await.is_empty());
    }
}

/// The quiet outcome requires a fully checked account and ledger. A real
/// failure must remain visible even below the redemption usage threshold.
#[tokio::test]
async fn preflight_below_redemption_threshold_still_reports_a_broken_ledger() {
    let mut fixture = Fixture::new(96, 72 * 3600, true).await;
    let account = digest(&format!("{}\0account\0user", fixture.server.uri()));
    let dir = fixture
        .home
        .path()
        .join(format!("weekly-reset-{account}.d"));
    private_dir(&dir).unwrap();
    std::fs::write(dir.join("state.json"), b"invalid ledger").unwrap();
    fixture.preflight().await;
    assert!(fixture.messages().contains("Reset recovery stopped"));
    assert!(fixture.posts().await.is_empty());
}

#[tokio::test]
async fn policy_preservation_reports_the_decision_without_a_verification_failure() {
    for used in [99, 100] {
        for remaining in [3600, 24 * 3600] {
            let mut fixture = Fixture::new(used, remaining, true).await;
            fixture.offer_credits().await;
            fixture.preflight().await;
            let messages = fixture.messages();
            assert!(messages.contains("2 available"), "{messages}");
            assert!(messages.contains("2030-01-01"), "{messages}");
            assert!(messages.contains("2030-02-01"), "{messages}");
            assert!(messages.contains("within 24 hours"), "{messages}");
            assert!(!messages.contains("Reset recovery stopped"), "{messages}");
            assert!(fixture.state.clear().is_none());
            assert!(fixture.posts().await.is_empty());
            fixture.preflight().await;
            assert!(fixture.messages().is_empty());
        }
    }
}

#[tokio::test]
async fn malformed_usage_is_not_a_successful_policy_preservation() {
    let mut fixture = Fixture::new(-1, 48 * 3600, true).await;
    fixture.body.lock().unwrap()["rate_limit"]["primary_window"] = serde_json::json!({
        "used_percent":99,"limit_window_seconds":18000,
        "reset_at":chrono::Utc::now().timestamp() + 3600,"reset_after_seconds":3600
    });
    fixture.offer_credits().await;
    fixture.preflight().await;
    assert!(fixture.messages().contains("Reset recovery stopped"));
    assert!(fixture.state.clear().is_none());
    assert!(fixture.posts().await.is_empty());
}

struct ReplacementAuth(codex_login::CodexAuth);

impl codex_login::ExternalAuth for ReplacementAuth {
    fn resolve(&self) -> codex_login::ExternalAuthFuture<'_, codex_login::CodexAuth> {
        Box::pin(async { Ok(self.0.clone()) })
    }

    fn refresh(
        &self,
        _: codex_login::ExternalAuthRefreshContext,
    ) -> codex_login::ExternalAuthFuture<'_, codex_login::CodexAuth> {
        self.resolve()
    }
}

#[tokio::test]
async fn an_account_change_during_inventory_is_not_policy_preservation() {
    let mut fixture = Fixture::new(99, 24 * 3600, true).await;
    let listed = Arc::new(tokio::sync::Notify::new());
    let observed = Arc::clone(&listed);
    Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(move |_: &wiremock::Request| {
            observed.notify_one();
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"available_count":0,"credits":[]}))
                .set_delay(Duration::from_secs(2))
        })
        .expect(1)
        .mount(&fixture.server)
        .await;
    {
        let attempt = fixture.preflight();
        tokio::pin!(attempt);
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::select! {
                _ = &mut attempt => panic!("preflight finished before inventory was observed"),
                _ = listed.notified() => {}
            }
        })
        .await
        .unwrap();
        let auth = codex_login::CodexAuth::from_external_chatgpt_tokens(
            "e30.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjb3VudCIsImNoYXRncHRfdXNlcl9pZCI6InVzZXIiLCJjaGF0Z3B0X3BsYW5fdHlwZSI6InBsdXMifX0.fixture",
            "destination-account", Some("plus"),
        ).unwrap();
        fixture
            .manager
            .set_external_auth(Arc::new(ReplacementAuth(auth)))
            .await
            .unwrap();
        assert_eq!(identity(&fixture.manager).unwrap().0, "destination-account");
        tokio::time::timeout(Duration::from_secs(5), attempt)
            .await
            .unwrap();
    }
    assert!(fixture.messages().contains("Reset recovery stopped"));
    assert!(fixture.state.clear().is_none());
    assert!(fixture.posts().await.is_empty());
}
