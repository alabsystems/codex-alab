// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A clear usage reading excuses later preflights of its own turn only while
//! its headroom proves the redemption threshold out of reach.

use super::*;
use codex_protocol::protocol::RateLimitSnapshot;
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

/// What a model response reports: one window of one limit family.
fn response(limit_id: Option<&str>, used: f64, minutes: Option<i64>) -> RateLimitSnapshot {
    let mut snapshot: RateLimitSnapshot = serde_json::from_value(serde_json::json!({
        "limit_id": limit_id, "limit_name": null,
        "primary": {"used_percent": 0.0, "window_minutes": minutes, "resets_at": null},
        "secondary": null, "credits": null, "individual_limit": null,
        "spend_control_reached": null, "plan_type": null, "rate_limit_reached_type": null
    }))
    .unwrap();
    // JSON cannot carry the non-finite values a response header can parse to.
    snapshot.primary.as_mut().unwrap().used_percent = used;
    snapshot
}

/// The usage endpoint reports whole percentages.
fn usage(used: i32, allowed: bool) -> serde_json::Value {
    serde_json::json!({
        "account_id":"account", "user_id":"user", "plan_type":"plus",
        "rate_limit":{"allowed":allowed, "limit_reached":!allowed,
            "primary_window":{"used_percent":used, "limit_window_seconds":18000,
                "reset_at":chrono::Utc::now().timestamp() + 3600, "reset_after_seconds":3600}}
    })
}

struct Fixture {
    server: MockServer,
    home: tempfile::TempDir,
    manager: Arc<AuthManager>,
    body: Arc<Mutex<serde_json::Value>>,
}

impl Fixture {
    /// A healthy account with no earned credit to redeem.
    async fn new() -> Self {
        codex_alab_core::activate();
        let server = MockServer::start().await;
        let body = Arc::new(Mutex::new(usage(50, true)));
        let served = Arc::clone(&body);
        Mock::given(method("GET"))
            .and(path(USAGE))
            .respond_with(move |_: &wiremock::Request| {
                ResponseTemplate::new(200).set_body_json(served.lock().unwrap().clone())
            })
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(CREDITS))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"available_count":0,"credits":[]})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(CONSUME))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let home = tempfile::tempdir().unwrap();
        let auth = codex_login::CodexAuth::from_external_chatgpt_tokens("e30.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjb3VudCIsImNoYXRncHRfdXNlcl9pZCI6InVzZXIiLCJjaGF0Z3B0X3BsYW5fdHlwZSI6InBsdXMifX0.fixture", "account", Some("plus")).unwrap();
        let manager = AuthManager::from_auth_for_testing_with_home(auth, home.path().to_owned());
        Self {
            server,
            home,
            manager,
            body,
        }
    }

    fn serve(&self, body: serde_json::Value) {
        *self.body.lock().unwrap() = body;
    }

    fn factory() -> HttpClientFactory {
        HttpClientFactory::new(codex_http_client::OutboundProxyPolicy::ReqwestDefault)
    }

    /// Each turn binds its own state.
    async fn turn(&self) -> State {
        State::bind(&self.manager).await
    }

    async fn preflight(&self, state: &State, observed: Option<&RateLimitSnapshot>) {
        preflight(
            &self.manager,
            self.home.path(),
            &self.server.uri(),
            Self::factory(),
            "turn",
            state,
            observed,
        )
        .await;
    }

    async fn reads(&self) -> usize {
        self.server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path() == USAGE)
            .count()
    }
}

#[test]
fn clear_reading_freshness_scales_with_headroom() {
    let at = |used: f64| Clear {
        read_at: 1000,
        read: std::time::Instant::now(),
        used,
    };
    // (used, the last second the reading holds): four seconds a point, capped at sixty.
    for (used, last) in [
        (0.0, 1059),
        (80.0, 1059),
        (85.0, 1039),
        (90.0, 1019),
        (94.0, 1003),
        (94.9, 1000),
    ] {
        assert!(at(used).holds(1000, None), "{used}% at once");
        assert!(at(used).holds(last, None), "{used}% at {last}");
        assert!(!at(used).holds(last + 1, None), "{used}% after {last}");
    }
    for used in [
        95.0,
        95.1,
        100.0,
        250.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        assert!(!at(used).holds(1000, None), "{used}% is never reused");
    }
    // A clock that stepped back past the reading proves nothing about its age.
    for now in [999, 0, i64::MIN] {
        assert!(!at(0.0).holds(now, None));
    }
    // One that stepped back inside the reading's age cannot stretch it either:
    // the monotonic age is held to the same window.
    let second = Duration::from_secs(1);
    assert!(at(0.0).holds_after(1005, 59 * second, None));
    assert!(
        !at(0.0).holds_after(1005, 60 * second, None),
        "five seconds old by a stepped clock, sixty in fact"
    );
    assert!(at(94.0).holds_after(1000, Duration::from_millis(3999), None));
    assert!(!at(94.0).holds_after(1000, 4 * second, None));
    assert!(!at(10.0).holds_after(1000, 20 * second, Some(90.0)));
    // The wall clock still counts host sleep, which the monotonic clock may not.
    assert!(!at(0.0).holds_after(1060, Duration::ZERO, None));

    // Response evidence shortens the window and never extends it.
    assert!(at(10.0).holds(1019, Some(90.0)));
    assert!(!at(10.0).holds(1020, Some(90.0)));
    assert!(at(90.0).holds(1019, Some(10.0)));
    assert!(!at(90.0).holds(1020, Some(10.0)));
    for observed in [95.0, 96.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(!at(10.0).holds(1000, Some(observed)), "{observed}%");
    }

    // Only the ordinary bucket's windows are evidence.
    for id in [Some("codex"), None] {
        assert_eq!(response_used(&response(id, 90.0, Some(300))), Some(90.0));
        assert_eq!(response_used(&response(id, 90.0, Some(10080))), Some(90.0));
        // A window of unknown length may be an ordinary one.
        assert_eq!(response_used(&response(id, 96.0, None)), Some(96.0));
        assert_eq!(response_used(&response(id, 96.0, Some(60))), None);
        assert_eq!(
            response_used(&response(id, f64::NAN, Some(300))),
            Some(100.0)
        );
    }
    assert_eq!(
        response_used(&response(Some("codex_other"), 99.0, Some(300))),
        None
    );
    let mut both = response(Some("codex"), 20.0, Some(300));
    both.secondary = response(None, 91.0, Some(10080)).primary;
    assert_eq!(response_used(&both), Some(91.0));
    both.primary = None;
    both.secondary = None;
    assert_eq!(response_used(&both), None, "an empty codex snapshot");
    assert!(!at(10.0).holds(
        1000,
        response_used(&response(Some("codex"), f64::INFINITY, Some(300)))
    ));

    // The authoritative reading counts the windows `usage_below` judges.
    let mut read = RateLimitsWithResetCredits {
        rate_limits: vec![response(Some("codex"), 40.0, Some(10080))],
        ordinary_usage_allowed: Some(true),
        rate_limit_reset_credits: None,
        account_id: Some("account".into()),
        user_id: Some("user".into()),
        rate_limit_upsell: None,
    };
    read.rate_limits[0].secondary = read.rate_limits[0].primary.take();
    assert_eq!(ordinary_used(&read), Some(40.0));
    read.rate_limits[0].primary = response(None, 70.0, Some(300)).primary;
    assert_eq!(ordinary_used(&read), Some(70.0));
    read.rate_limits[0].primary = response(None, 99.0, Some(60)).primary;
    assert_eq!(ordinary_used(&read), Some(40.0));
    read.rate_limits[0].limit_id = Some("premium".into());
    assert_eq!(ordinary_used(&read), None);
}

#[tokio::test]
async fn clear_preflight_reuses_one_usage_read_within_a_turn() {
    let fixture = Fixture::new().await;
    let state = fixture.turn().await;
    for _ in 0..3 {
        fixture.preflight(&state, None).await;
    }
    assert_eq!(fixture.reads().await, 1, "one reading serves the turn");

    // Lower or foreign response limits change nothing.
    for observed in [
        response(Some("codex"), 40.0, Some(300)),
        response(Some("codex_other"), 99.0, Some(300)),
    ] {
        fixture.preflight(&state, Some(&observed)).await;
    }
    assert_eq!(fixture.reads().await, 1);

    // A response past the threshold overrides the reading at once.
    fixture
        .preflight(&state, Some(&response(Some("codex"), 96.0, Some(300))))
        .await;
    assert_eq!(fixture.reads().await, 2);

    // A reused reading takes no custody lock: a peer holding it for its own
    // usage read costs an unskipped preflight its five-second lock wait.
    let account = digest(&format!("{}\0account\0user", fixture.server.uri()));
    let lock = fixture.home.path().join(format!("weekly-reset-{account}"));
    let peer = codex_alab_core::auth_lock::AuthLock::try_take(&lock)
        .unwrap()
        .expect("the finished read released the custody lock");
    tokio::time::timeout(Duration::from_secs(2), fixture.preflight(&state, None))
        .await
        .expect("a reused reading waited for the custody lock");
    assert_eq!(fixture.reads().await, 2);
    drop(peer);

    // The next turn binds its own state and reads for itself.
    let next = fixture.turn().await;
    fixture.preflight(&next, None).await;
    assert_eq!(fixture.reads().await, 3);
    fixture.preflight(&next, None).await;
    fixture.preflight(&state, None).await;
    assert_eq!(fixture.reads().await, 3);
}

#[tokio::test]
async fn pressure_and_recovery_never_reuse_a_reading() {
    let fixture = Fixture::new().await;

    // One point of headroom is four seconds of clearance, and no more.
    fixture.serve(usage(94, true));
    let state = fixture.turn().await;
    let before = chrono::Utc::now().timestamp();
    fixture.preflight(&state, None).await;
    let after = chrono::Utc::now().timestamp();
    let clear = (*state.clear()).expect("a permitted reading clears its turn");
    assert_eq!(clear.used, 94.0);
    assert!((before..=after).contains(&clear.read_at));
    assert!(clear.holds(clear.read_at + 3, None));
    assert!(!clear.holds(clear.read_at + 4, None));
    let mut reads = fixture.reads().await;
    assert_eq!(reads, 1);

    // A reading that does not permit the request never clears the turn, and
    // replaces a clearance the turn already held.
    let mut other_account = usage(50, true);
    other_account["account_id"] = "other".into();
    let mut spend_denied = usage(50, true);
    spend_denied["spend_control"] = serde_json::json!({"reached": true});
    let pressure = response(Some("codex"), 96.0, Some(300));
    for (name, body) in [
        ("at the threshold", usage(95, true)),
        ("exhausted", usage(100, false)),
        ("ordinary usage denied", usage(50, false)),
        ("another account", other_account),
        ("spend control reached", spend_denied),
    ] {
        fixture.serve(usage(50, true));
        let state = fixture.turn().await;
        fixture.preflight(&state, None).await;
        assert!(state.clear().is_some(), "{name}: healthy start");
        fixture.serve(body);
        fixture.preflight(&state, Some(&pressure)).await;
        assert_eq!(*state.clear(), None, "{name}: forced read");
        for _ in 0..2 {
            fixture.preflight(&state, None).await;
            assert_eq!(*state.clear(), None, "{name}: repeated read");
        }
        reads += 4;
        assert_eq!(fixture.reads().await, reads, "{name}: every request read");
    }

    // A wall contradicts the clearance even when recovery then finds the
    // allowance usable: the next request reads for itself.
    fixture.serve(usage(50, true));
    let state = fixture.turn().await;
    for _ in 0..2 {
        fixture.preflight(&state, None).await;
    }
    reads += 1;
    assert_eq!(fixture.reads().await, reads);
    assert!(state.clear().is_some());
    assert!(
        recover_weekly(
            &fixture.manager,
            fixture.home.path(),
            &fixture.server.uri(),
            Fixture::factory(),
            "walled-turn",
            &state,
            crate::Wall::default(),
        )
        .await
    );
    assert_eq!(*state.clear(), None, "recovery kept a clear reading");
    reads += 1;
    assert_eq!(fixture.reads().await, reads);
    fixture.preflight(&state, None).await;
    assert_eq!(fixture.reads().await, reads + 1);
    assert!(state.clear().is_some());
}
