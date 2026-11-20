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

    async fn recover_blocked_turn(&self) -> bool {
        tokio::time::timeout(
            Duration::from_secs(5),
            recover_weekly(
                &self.manager,
                self.home.path(),
                &self.server.uri(),
                Self::factory(),
                "blocked-turn",
                &self.state,
                crate::Wall::default(),
            ),
        )
        .await
        .expect("a confirmed block should return to native scheduling")
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

/// Both the initial check and repeated polls at an exhausted wall stay silent.
#[tokio::test]
async fn empty_earned_reset_checks_stay_quiet_before_and_during_recovery() {
    let mut fixture = Fixture::new(100, 25 * 3600, false).await;
    Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"available_count":0,"credits":[]})),
        )
        .expect(6)
        .mount(&fixture.server)
        .await;
    let config = Settings {
        home: fixture.home.path().to_owned(),
        base: fixture.server.uri(),
        factory: Fixture::factory(),
    };
    for turn in ["first-turn", "second-turn"] {
        fixture.preflight().await;
        assert!(fixture.messages().is_empty());
        for _ in 0..2 {
            let outcome = recover_with(
                &fixture.manager,
                &config,
                fixture.state.owner.as_ref().unwrap(),
                turn,
                Purpose::Recover,
                Some(&fixture.state),
            )
            .await;
            assert!(matches!(
                outcome,
                Recovery::NoCredit {
                    conclusive: true,
                    ..
                }
            ));
            let messages = fixture.messages();
            assert!(messages.is_empty(), "{messages}");
        }
        fixture.state = State::bind(&fixture.manager).await;
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        fixture.state.set_notice_sender(sender);
        fixture.notices = receiver;
    }
    assert!(fixture.posts().await.is_empty());
}

#[tokio::test]
async fn equivalent_reset_inventories_stay_quiet_and_changed_expiry_is_reported() {
    let mut fixture = Fixture::new(100, 3600, false).await;
    let config = Settings {
        home: fixture.home.path().to_owned(),
        base: fixture.server.uri(),
        factory: Fixture::factory(),
    };
    let grant = |id: &str, expiry: &str| {
        serde_json::json!({
            "id":id, "reset_type":"codex_rate_limits", "status":"available",
            "granted_at":"2026-01-01T00:00:00Z", "expires_at":expiry
        })
    };
    let earlier = grant("A", "2030-01-01T00:00:00Z");
    let later = grant("B", "2030-02-01T00:00:00Z");
    for (rows, expected) in [
        (
            [later.clone(), earlier.clone()],
            Some("2030-01-01T00:00:00+00:00, 2030-02-01T00:00:00+00:00"),
        ),
        ([grant("A", "2029-12-31T19:00:00-05:00"), later], None),
        (
            [earlier, grant("B", "2030-03-01T00:00:00Z")],
            Some("2030-03-01T00:00:00+00:00"),
        ),
    ] {
        let _list = Mock::given(method("GET"))
            .and(path(CREDITS))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "available_count":2, "credits":rows
            })))
            .expect(1)
            .mount_as_scoped(&fixture.server)
            .await;
        let outcome = recover_with(
            &fixture.manager,
            &config,
            fixture.state.owner.as_ref().unwrap(),
            "notice-turn",
            Purpose::Recover,
            Some(&fixture.state),
        )
        .await;
        assert!(matches!(
            outcome,
            Recovery::NoCredit {
                conclusive: true,
                ..
            }
        ));
        let messages = fixture.messages();
        match expected {
            Some(expiry) => assert!(messages.contains(expiry), "{messages}"),
            None => assert!(messages.is_empty(), "{messages}"),
        }
    }
    assert!(fixture.posts().await.is_empty());
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
async fn reset_inventory_outage_is_silent_before_work_and_reported_at_a_wall() {
    let mut fixture = Fixture::new(100, 72 * 3600, false).await;
    Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(ResponseTemplate::new(503))
        .mount(&fixture.server)
        .await;
    fixture.preflight().await;
    assert!(fixture.messages().is_empty());
    let config = Settings {
        home: fixture.home.path().to_owned(),
        base: fixture.server.uri(),
        factory: Fixture::factory(),
    };
    let outcome = recover_with(
        &fixture.manager,
        &config,
        fixture.state.owner.as_ref().unwrap(),
        "notice-turn",
        Purpose::Recover,
        Some(&fixture.state),
    )
    .await;
    assert!(matches!(
        outcome,
        Recovery::NoCredit {
            conclusive: false,
            ..
        }
    ));
    let messages = fixture.messages();
    assert!(messages.contains("could not be read"), "{messages}");
    assert!(!messages.contains("0 available"), "{messages}");
    assert!(fixture.posts().await.is_empty());
}

#[tokio::test]
async fn account_usage_lookup_failure_is_silent_before_work() {
    let mut fixture = Fixture::new(100, 72 * 3600, false).await;
    fixture.server.reset().await;
    Mock::given(method("GET"))
        .and(path(USAGE))
        .respond_with(ResponseTemplate::new(503))
        .expect(2)
        .mount(&fixture.server)
        .await;
    fixture.preflight().await;
    fixture.preflight().await;
    assert!(fixture.messages().is_empty());
    assert!(fixture.posts().await.is_empty());
}

#[tokio::test]
async fn healthy_preflight_does_not_print_reset_decisions() {
    let mut fixture = Fixture::new(25, 72 * 3600, true).await;
    fixture.preflight().await;
    assert!(fixture.messages().is_empty());
    assert!(fixture.posts().await.is_empty());
}

#[tokio::test]
async fn passive_spending_restrictions_stay_in_account_status() {
    let mut fixture = Fixture::new(100, 72 * 3600, false).await;
    fixture.body.lock().unwrap()["credits"] = serde_json::json!({
        "has_credits":true, "unlimited":false, "balance":"25"
    });
    for (spend_control, reason) in [
        (true, "rate_limit_reached"),
        (false, "workspace_member_usage_limit_reached"),
    ] {
        {
            let mut body = fixture.body.lock().unwrap();
            body["spend_control"] = serde_json::json!({"reached":spend_control});
            body["rate_limit_reached_type"] = serde_json::json!({"type":reason});
        }
        fixture.preflight().await;
        let notices = fixture.messages();
        assert!(notices.is_empty(), "{notices}");
        assert!(fixture.state.clear().is_none());
    }
    let requests = fixture.server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| request.url.path() == USAGE));

    // A restriction reported for another account cannot settle this account's check.
    fixture.body.lock().unwrap()["account_id"] = serde_json::json!("other");
    fixture.preflight().await;
    assert!(fixture.messages().contains("Reset recovery stopped"));
    assert!(fixture.posts().await.is_empty());
}

#[tokio::test]
async fn an_active_quota_wall_reports_the_confirmed_credit_block_once() {
    let mut fixture = Fixture::new(100, 72 * 3600, false).await;
    fixture.body.lock().unwrap()["credits"] = serde_json::json!({
        "has_credits":true, "unlimited":false, "balance":"25"
    });
    for (kind, control, expected) in [
        (
            "rate_limit_reached",
            true,
            "Credit retry skipped. The account spending limit was reached.",
        ),
        (
            "workspace_member_credits_depleted",
            true,
            "Credit retry skipped. The workspace is out of credits.",
        ),
        (
            "workspace_owner_usage_limit_reached",
            false,
            "Credit retry skipped. The workspace spending limit was reached.",
        ),
        (
            "workspace_owner_credits_depleted",
            false,
            "Credit retry skipped. The workspace is out of credits.",
        ),
        (
            "workspace_member_usage_limit_reached",
            true,
            "Credit retry skipped. Your workspace spending limit was reached.",
        ),
    ] {
        {
            let mut body = fixture.body.lock().unwrap();
            body["rate_limit_reached_type"] = serde_json::json!({"type":kind});
            body["spend_control"] = serde_json::json!({"reached":control});
        }
        for _ in 0..2 {
            assert!(!fixture.recover_blocked_turn().await);
        }
        assert_eq!(
            fixture.messages(),
            format!("[codex-alab credits] {expected}")
        );
    }
    let requests = fixture.server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 10);
    assert!(requests.iter().all(|request| request.url.path() == USAGE));

    // A block cannot stand in for the status of a reset still awaiting acknowledgement.
    let account = digest(&format!("{}\0account\0user", fixture.server.uri()));
    let dir = fixture
        .home
        .path()
        .join(format!("weekly-reset-{account}.d"));
    let op = begin(&dir, &account, &mut Ledger::default(), 42, "pending-reset").unwrap();
    assert!(!fixture.recover_blocked_turn().await);
    assert!(fixture.messages().contains("Reset recovery stopped"));
    let ledger = load::<Ledger>(&dir.join("state.json")).unwrap().unwrap();
    let pending = ledger.current.unwrap();
    assert_eq!(pending.key, op.key);
    assert!(pending.outcome.pending());
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
async fn policy_preservation_is_silent_without_listing_earned_resets() {
    for used in [99, 100] {
        for remaining in [3600, 24 * 3600 - 1, 24 * 3600] {
            let mut fixture = Fixture::new(used, remaining, true).await;
            fixture.offer_credits().await;
            fixture.preflight().await;
            assert!(fixture.messages().is_empty());
            assert!(
                fixture
                    .server
                    .received_requests()
                    .await
                    .unwrap()
                    .iter()
                    .all(|request| request.url.path() != CREDITS)
            );
            assert!(fixture.state.clear().is_none());
            assert!(fixture.posts().await.is_empty());
            fixture.preflight().await;
            assert!(fixture.messages().is_empty());
        }
    }
}

#[tokio::test]
async fn malformed_usage_is_not_a_successful_policy_preservation() {
    let mut fixture = Fixture::new(-1, 72 * 3600, true).await;
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
async fn an_account_change_during_selection_cannot_release_a_credit_retry() {
    let mut fixture = Fixture::new(100, 25 * 3600, false).await;
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
        let config = Settings {
            home: fixture.home.path().to_owned(),
            base: fixture.server.uri(),
            factory: Fixture::factory(),
        };
        fixture.body.lock().unwrap()["credits"] = serde_json::json!({
            "has_credits":true, "unlimited":false, "balance":"30966.04065"
        });
        let attempt = recover_with(
            &fixture.manager,
            &config,
            fixture.state.owner.as_ref().unwrap(),
            "paid-turn",
            Purpose::Recover,
            Some(&fixture.state),
        );
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
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), attempt)
                .await
                .unwrap(),
            Recovery::Stop
        );
    }
    assert!(fixture.messages().contains("Reset recovery stopped"));
    assert!(fixture.state.clear().is_none());
    assert!(fixture.posts().await.is_empty());
}

#[tokio::test]
async fn ordinary_wall_retries_once_on_existing_usage_credits_without_consuming_a_reset() {
    for remaining in [3600, 25 * 3600] {
        let mut fixture = Fixture::new(100, remaining, false).await;
        {
            let mut body = fixture.body.lock().unwrap();
            body["rate_limit_reached_type"] = serde_json::json!({"type":"rate_limit_reached"});
            body["spend_control"] = serde_json::json!({"reached":false});
            body["credits"] = serde_json::json!({
                "has_credits":true, "unlimited":false, "balance":"30966.04065"
            });
        }
        let body = Arc::clone(&fixture.body);
        Mock::given(method("GET"))
            .and(path(CREDITS))
            .respond_with(move |_: &wiremock::Request| {
                body.lock().unwrap()["credits"]["balance"] = serde_json::json!("30965.02000");
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"available_count":0,"credits":[]}))
            })
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
                    "paid-turn",
                    &fixture.state,
                    crate::Wall::default(),
                ),
            )
            .await
            .unwrap()
        );
        let balance = if remaining == 3600 {
            "30966.04065"
        } else {
            "30965.02000"
        };
        assert_eq!(
            fixture.messages(),
            format!(
                "[codex-alab credits] Retrying this request once with usage credits (reported balance {balance})."
            )
        );
        // After the provider refuses the retry, a resumed turn still needs the
        // current quota wall. No further read can authorize a second paid retry.
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = Arc::clone(&reads);
        let body = fixture.body.lock().unwrap().clone();
        let reset = body["rate_limit"]["secondary_window"]["reset_at"]
            .as_i64()
            .unwrap();
        Mock::given(method("GET"))
            .and(path(USAGE))
            .respond_with(move |_: &wiremock::Request| {
                if observed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0 {
                    ResponseTemplate::new(200).set_body_json(body.clone())
                } else {
                    ResponseTemplate::new(503)
                }
            })
            .with_priority(1)
            .mount(&fixture.server)
            .await;
        let resumed = State::bind(&fixture.manager).await;
        let (sender, mut notices) = tokio::sync::mpsc::unbounded_channel();
        resumed.set_notice_sender(sender);
        let (handed, mut walls) = tokio::sync::mpsc::unbounded_channel();
        let host = codex_alab_core::quota::WallHandoff::new(move |wall| {
            std::future::ready(handed.send(wall).is_ok())
        });
        let retry = tokio::time::timeout(
            Duration::from_secs(5),
            recover_weekly(
                &fixture.manager,
                fixture.home.path(),
                &fixture.server.uri(),
                Fixture::factory(),
                "paid-turn",
                &resumed,
                crate::Wall {
                    reset_at: Some(reset),
                    handoff: Some(&host),
                },
            ),
        )
        .await
        .expect("spent paid retry held a ready account handoff");
        assert!(!retry);
        assert_eq!(
            walls.try_recv().unwrap(),
            SpentWall {
                window: SpentWindow::Week,
                resets_at: reset
            }
        );
        assert!(walls.try_recv().is_err());
        assert_eq!(reads.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert!(notices.try_recv().is_err());
        assert!(fixture.posts().await.is_empty());
    }
}

#[tokio::test]
async fn a_plan_renewal_during_the_reset_list_preserves_the_turns_paid_retry() {
    let mut fixture = Fixture::new(100, 25 * 3600, false).await;
    let config = Settings {
        home: fixture.home.path().to_owned(),
        base: fixture.server.uri(),
        factory: Fixture::factory(),
    };
    let credits = serde_json::json!({"has_credits":true, "unlimited":false, "balance":"25"});
    fixture.body.lock().unwrap()["credits"] = credits.clone();
    let exhausted = fixture.body.lock().unwrap().clone();
    let body = Arc::clone(&fixture.body);
    let empty_list = ResponseTemplate::new(200)
        .set_body_json(serde_json::json!({"available_count":0,"credits":[]}));
    let reset_list = empty_list.clone();
    let renewal = Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(move |_: &wiremock::Request| {
            let mut renewed = usage(3, 7 * 24 * 3600, true);
            renewed["credits"] = credits.clone();
            *body.lock().unwrap() = renewed;
            reset_list.clone()
        })
        .expect(1)
        .mount_as_scoped(&fixture.server)
        .await;
    let recover = || {
        recover_with(
            &fixture.manager,
            &config,
            fixture.state.owner.as_ref().unwrap(),
            "same-turn",
            Purpose::Recover,
            Some(&fixture.state),
        )
    };
    assert_eq!(recover().await, Recovery::Restored);
    drop(renewal);
    // Replaying the restored request uses its existing ordinary retry receipt.
    assert_eq!(recover().await, Recovery::Spent);
    *fixture.body.lock().unwrap() = exhausted;
    Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(empty_list)
        .expect(2)
        .mount(&fixture.server)
        .await;
    assert_eq!(recover().await, Recovery::RetryWithUsageCredits);
    assert!(matches!(
        recover().await,
        Recovery::NoCredit {
            conclusive: true,
            ..
        }
    ));
    let messages = fixture.messages();
    assert!(
        messages.starts_with("[codex-alab reset] Allowance restored"),
        "{messages}"
    );
    assert_eq!(
        messages.matches("Retrying this request once").count(),
        1,
        "{messages}"
    );
    assert!(fixture.posts().await.is_empty());
}

#[tokio::test]
async fn spend_control_after_listing_preserves_the_paid_retry_until_spending_reopens() {
    let mut fixture = Fixture::new(100, 25 * 3600, false).await;
    let config = Settings {
        home: fixture.home.path().to_owned(),
        base: fixture.server.uri(),
        factory: Fixture::factory(),
    };
    fixture.body.lock().unwrap()["credits"] = serde_json::json!({
        "has_credits":true, "unlimited":false, "balance":"25"
    });
    let body = fixture.body.clone();
    let empty_list = ResponseTemplate::new(200)
        .set_body_json(serde_json::json!({"available_count":0,"credits":[]}));
    let blocked_list = empty_list.clone();
    let restriction = Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(move |_: &wiremock::Request| {
            body.lock().unwrap()["spend_control"] = serde_json::json!({"reached":true});
            blocked_list.clone()
        })
        .expect(1)
        .mount_as_scoped(&fixture.server)
        .await;
    let recover = || {
        recover_with(
            &fixture.manager,
            &config,
            fixture.state.owner.as_ref().unwrap(),
            "paid-turn",
            Purpose::Recover,
            Some(&fixture.state),
        )
    };
    assert_eq!(recover().await, Recovery::Wait);
    assert!(fixture.notices.try_recv().is_err());
    drop(restriction);

    fixture.body.lock().unwrap()["spend_control"] = serde_json::json!({"reached":false});
    Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(empty_list)
        .expect(2)
        .mount(&fixture.server)
        .await;
    assert_eq!(recover().await, Recovery::RetryWithUsageCredits);
    assert!(matches!(
        recover().await,
        Recovery::NoCredit {
            conclusive: true,
            ..
        }
    ));
    assert_eq!(
        fixture.messages(),
        "[codex-alab credits] Retrying this request once with usage credits (reported balance 25)."
    );
    assert!(fixture.posts().await.is_empty());
}
