// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A spent allowance with no earned credit ends the original request for a
//! host that would leave that wall for another account: a spent week, or a
//! spent five-hour window beside a usable week, whatever reset the 429 named.
//! The host is told which, with the reset the usage read names. Every other
//! wall keeps polling.

use super::*;
use codex_alab_core::quota::WallHandoff;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

const USAGE: &str = "/api/codex/usage";
const CREDITS: &str = "/api/codex/rate-limit-reset-credits";
const CONSUME: &str = "/api/codex/rate-limit-reset-credits/consume";

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

    async fn wait(
        &self,
        interval: Duration,
        reset_at: Option<i64>,
        handoff: Option<&WallHandoff>,
    ) -> bool {
        wait_for_reset(
            &self.manager,
            &self.config,
            self.state.owner.as_ref().unwrap(),
            "turn",
            interval,
            reset_at,
            handoff,
        )
        .await
    }

    async fn posts(&self) -> usize {
        self.server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.method.as_str() == "POST")
            .count()
    }
}

/// The account's usage with one window, spent unless `healthy`.
fn usage(weekly: bool, healthy: bool, reset: i64) -> ResponseTemplate {
    let window = serde_json::json!({
        "used_percent": if healthy { 0 } else { 100 },
        "limit_window_seconds": if weekly { 604_800 } else { 18_000 },
        "reset_at": reset,
        "reset_after_seconds": 3600,
    });
    let mut limit = serde_json::json!({"allowed": healthy, "limit_reached": !healthy});
    limit[if weekly {
        "secondary_window"
    } else {
        "primary_window"
    }] = window;
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "account_id": "account", "user_id": "user", "plan_type": "plus", "rate_limit": limit,
    }))
}

fn credits(ids: &[&str]) -> ResponseTemplate {
    let credits: Vec<_> = ids
        .iter()
        .map(|id| {
            serde_json::json!({"id": id, "reset_type": "codex_rate_limits",
                "status": "available", "granted_at": "2026-01-01T00:00:00Z",
                "expires_at": "2030-01-01T00:00:00Z"})
        })
        .collect();
    ResponseTemplate::new(200)
        .set_body_json(serde_json::json!({"available_count": ids.len(), "credits": credits}))
}

/// The walls a host was asked about, in order.
#[derive(Clone, Default)]
struct Asked(Arc<Mutex<Vec<SpentWall>>>);

impl Asked {
    fn walls(&self) -> Vec<SpentWall> {
        self.0.lock().unwrap().clone()
    }

    fn count(&self) -> usize {
        self.walls().len()
    }
}

/// A host answering `ready` whatever the wall, and what it was asked.
fn host(ready: bool) -> (WallHandoff, Asked) {
    let asked = Asked::default();
    let record = asked.clone();
    let handoff = WallHandoff::new(move |wall| {
        let record = record.clone();
        async move {
            record.0.lock().unwrap().push(wall);
            ready
        }
    });
    (handoff, asked)
}

/// Both windows spent: the five-hour one resets at `five_hour`, the week at `week`.
fn usage_of_both(five_hour: i64, week: i64) -> ResponseTemplate {
    let window = |seconds: i64, reset: i64| {
        serde_json::json!({
            "used_percent": 100, "limit_window_seconds": seconds,
            "reset_at": reset, "reset_after_seconds": 3600,
        })
    };
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "account_id": "account", "user_id": "user", "plan_type": "plus",
        "rate_limit": {
            "allowed": false, "limit_reached": true,
            "primary_window": window(18_000, five_hour),
            "secondary_window": window(604_800, week),
        },
    }))
}

/// The usage of an account whose five-hour window is spent and resets at
/// `reset`, on a week 40% used that resets days later: what a real account at
/// a five-hour wall reports.
fn usage_of_spent_five_hours(reset: i64) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "account_id": "account", "user_id": "user", "plan_type": "plus",
        "rate_limit": {
            "allowed": false, "limit_reached": true,
            "primary_window": {
                "used_percent": 100, "limit_window_seconds": 18_000,
                "reset_at": reset, "reset_after_seconds": 3600,
            },
            "secondary_window": {
                "used_percent": 40, "limit_window_seconds": 604_800,
                "reset_at": reset + 4 * 24 * 3600, "reset_after_seconds": 349_200,
            },
        },
    }))
}

/// A spent five-hour window resetting at `reset` beside a usable week
/// ([`usage_of_spent_five_hours`]), an empty credit list, and no redemption
/// permitted.
async fn creditless_five_hours(reset: i64) -> Fixture {
    creditless_five_hours_reading(usage_of_spent_five_hours(reset)).await
}

/// [`creditless_five_hours`] for this usage reply.
async fn creditless_five_hours_reading(read: ResponseTemplate) -> Fixture {
    let fixture = Fixture::new().await;
    Mock::given(method("GET"))
        .and(path(USAGE))
        .respond_with(read)
        .expect(1)
        .mount(&fixture.server)
        .await;
    Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(credits(&[]))
        .expect(0)
        .mount(&fixture.server)
        .await;
    Mock::given(method("POST"))
        .and(path(CONSUME))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&fixture.server)
        .await;
    fixture
}

fn hour_ahead() -> i64 {
    chrono::Utc::now().timestamp() + 3600
}

fn distant_week_reset() -> i64 {
    chrono::Utc::now().timestamp() + 72 * 3600
}

/// A spent week whose credit list is `list`, with no redemption permitted.
async fn creditless_week(list: ResponseTemplate, reset: i64) -> Fixture {
    let fixture = Fixture::new().await;
    Mock::given(method("GET"))
        .and(path(USAGE))
        .respond_with(usage(true, false, reset))
        .expect(1)
        .mount(&fixture.server)
        .await;
    Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(list)
        .expect(1)
        .mount(&fixture.server)
        .await;
    Mock::given(method("POST"))
        .and(path(CONSUME))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&fixture.server)
        .await;
    fixture
}

#[tokio::test]
async fn weekly_wall_without_credit_yields_to_a_ready_account_handoff() {
    let reset = distant_week_reset();
    let fixture = creditless_week(credits(&[]), reset).await;
    let (handoff, asked) = host(true);
    let recovered = tokio::time::timeout(
        Duration::from_secs(5),
        fixture.wait(Duration::from_secs(3600), Some(reset), Some(&handoff)),
    )
    .await
    .expect("a creditless weekly wall kept the request from a ready account handoff");
    assert!(
        !recovered,
        "the turn must end on its original usage error, not retry the request"
    );
    let walls = asked.walls();
    assert_eq!(walls.len(), 1);
    assert_eq!(
        walls[0],
        SpentWall {
            window: SpentWindow::Week,
            resets_at: reset,
        },
        "the host is told the week and the moment it resets"
    );

    // NEGATIVE CONTROL: the same wall with no host to hand to keeps polling.
    let fixture = creditless_week(credits(&[]), reset).await;
    assert!(
        tokio::time::timeout(
            Duration::from_secs(2),
            fixture.wait(Duration::from_secs(3600), Some(reset), None),
        )
        .await
        .is_err(),
        "without a handoff the original request waits for a late credit"
    );
}

/// A creditless ordinary wall still reaches account handoff more than a day away.
#[tokio::test]
async fn a_distant_creditless_weekly_wall_still_reaches_handoff() {
    let fixture = Fixture::new().await;
    let reset = chrono::Utc::now().timestamp() + 25 * 3600;
    Mock::given(method("GET"))
        .and(path(USAGE))
        .respond_with(usage(true, false, reset))
        .expect(1)
        .mount(&fixture.server)
        .await;
    Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(credits(&[]))
        .expect(1)
        .mount(&fixture.server)
        .await;
    let (handoff, asked) = host(true);
    let recovered = tokio::time::timeout(
        Duration::from_secs(60),
        fixture.wait(Duration::from_secs(3600), Some(reset), Some(&handoff)),
    )
    .await
    .expect("credit unavailability kept the weekly wall from a ready account handoff");
    assert!(!recovered, "handoff preserves the original usage error");
    assert_eq!(
        asked.walls(),
        [SpentWall {
            window: SpentWindow::Week,
            resets_at: reset,
        }],
    );
    assert_eq!(fixture.posts().await, 0);
}

/// Polling must survive unavailable credits, with or without an available host.
/// The next usage response restores allowance without consuming any credit.
#[tokio::test]
async fn a_distant_creditless_wall_keeps_polling_until_restored() {
    for with_host in [false, true] {
        let fixture = Fixture::new().await;
        let reset = chrono::Utc::now().timestamp() + 25 * 3600;
        let reads = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&reads);
        Mock::given(method("GET"))
            .and(path(USAGE))
            .respond_with(move |_: &wiremock::Request| {
                usage(true, observed.fetch_add(1, Ordering::SeqCst) != 0, reset)
            })
            .expect(2)
            .mount(&fixture.server)
            .await;
        Mock::given(method("GET"))
            .and(path(CREDITS))
            .respond_with(credits(&[]))
            .expect(1)
            .mount(&fixture.server)
            .await;
        let (handoff, asked) = host(false);
        assert!(
            tokio::time::timeout(
                Duration::from_secs(60),
                fixture.wait(Duration::ZERO, Some(reset), with_host.then_some(&handoff)),
            )
            .await
            .expect("creditless wall stopped observing restored allowance"),
            "credit unavailability must keep the original request polling, host={with_host}",
        );
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        assert_eq!(asked.count(), usize::from(with_host));
        assert_eq!(fixture.posts().await, 0);
    }
}

/// An uncertain or committed reset keeps its custody while stale usage forbids a
/// further POST. A ready account must not steal its request before restoration.
#[tokio::test]
async fn reset_custody_with_elapsed_usage_neither_spends_nor_yields() {
    for (name, outcome) in [
        ("pending", Outcome::Pending),
        ("awaiting ack", Outcome::AwaitingAck),
        ("restored pending", Outcome::RestoredPending),
        ("consumed", Outcome::Consumed),
    ] {
        let fixture = Fixture::new().await;
        let reset = chrono::Utc::now().timestamp() - 1;
        let reads = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&reads);
        Mock::given(method("GET"))
            .and(path(USAGE))
            .respond_with(move |_: &wiremock::Request| {
                usage(true, observed.fetch_add(1, Ordering::SeqCst) != 0, reset)
            })
            .expect(2)
            .mount(&fixture.server)
            .await;
        forbid_reset_spending(&fixture).await;
        let account = digest(&format!("{}\0account\0user", fixture.server.uri()));
        let dir = fixture
            .config
            .home
            .join(format!("weekly-reset-{account}.d"));
        private_dir(&dir).unwrap();
        let mut ledger = Ledger::default();
        let operation = begin(&dir, &account, &mut ledger, hour_ahead(), "A").unwrap();
        ledger.current.as_mut().unwrap().outcome = outcome;
        save(&dir.join("state.json"), &ledger).unwrap();
        if outcome == Outcome::Consumed {
            let mut credit = credit_at(&dir, "A").unwrap();
            credit.consumed = true;
            save(&credit_path(&dir, "A"), &credit).unwrap();
        }
        let (handoff, asked) = host(true);
        assert!(
            tokio::time::timeout(
                Duration::from_secs(60),
                fixture.wait(Duration::ZERO, Some(hour_ahead()), Some(&handoff)),
            )
            .await
            .expect("reset custody stopped observing restored allowance"),
            "the held request must observe restoration, custody={name}",
        );
        assert_eq!(reads.load(Ordering::SeqCst), 2, "{name}");
        assert_eq!(asked.count(), 0, "custody={name}");
        assert_eq!(fixture.posts().await, 0, "custody={name}");
        let retained = load::<Ledger>(&dir.join("state.json"))
            .unwrap()
            .unwrap()
            .current
            .unwrap();
        assert_eq!(retained.key, operation.key, "custody={name}");
        let credit = credit_at(&dir, "A").unwrap();
        assert_eq!(credit.consumed, outcome == Outcome::Consumed, "{name}");
    }
}

/// Offer an available credit so a missing eligibility gate cannot pass merely
/// because the backend had no credit to spend. Neither endpoint may be called.
async fn forbid_reset_spending(fixture: &Fixture) {
    Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(credits(&["available"]))
        .expect(0)
        .mount(&fixture.server)
        .await;
    Mock::given(method("POST"))
        .and(path(CONSUME))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&fixture.server)
        .await;
}

#[tokio::test]
async fn an_available_credit_is_redeemed_before_any_handoff_is_considered() {
    let fixture = Fixture::new().await;
    let reset = distant_week_reset();
    let redeemed = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&redeemed);
    Mock::given(method("GET"))
        .and(path(USAGE))
        .respond_with(move |_: &wiremock::Request| {
            usage(true, observed.load(Ordering::SeqCst), reset)
        })
        .expect(3)
        .mount(&fixture.server)
        .await;
    Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(credits(&["A"]))
        .expect(1)
        .mount(&fixture.server)
        .await;
    Mock::given(method("POST"))
        .and(path(CONSUME))
        .respond_with(move |_: &wiremock::Request| {
            redeemed.store(true, Ordering::SeqCst);
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"code":"reset","windows_reset":2}))
        })
        .expect(1)
        .mount(&fixture.server)
        .await;
    let (handoff, asked) = host(true);
    assert!(
        tokio::time::timeout(
            Duration::from_secs(5),
            fixture.wait(Duration::from_secs(3600), Some(reset), Some(&handoff)),
        )
        .await
        .expect("redemption did not finish"),
        "the earned reset resumes the original request"
    );
    assert_eq!(fixture.posts().await, 1);
    assert_eq!(
        asked.count(),
        0,
        "another account is considered only after the earned reset"
    );
}

/// A spent five-hour window on a week that is not spent is the host's to
/// judge: it is told the window and the moment it resets, as the usage read
/// names it, and its yes ends the request, whatever reset the 429 named: none,
/// the window's own, one a few seconds either side, or one far off either way.
/// The ended turn's wall is the read's (the Autopilot keeps the wall its yes
/// was given for), so nothing that follows the turn runs on the 429's.
#[tokio::test]
async fn a_creditless_five_hour_wall_is_offered_with_the_reads_reset_whatever_the_429_named() {
    for named_after in [
        Some(0),
        Some(50),
        Some(-50),
        Some(61),
        Some(-61),
        Some(120),
        Some(23 * 3600),
        Some(-1800),
        None,
    ] {
        let reset = hour_ahead();
        let fixture = creditless_five_hours(reset).await;
        let (handoff, asked) = host(true);
        let recovered = tokio::time::timeout(
            Duration::from_secs(5),
            fixture.wait(
                Duration::from_secs(3600),
                named_after.map(|after| reset + after),
                Some(&handoff),
            ),
        )
        .await
        .unwrap_or_else(|_| {
            panic!(
                "a creditless five-hour wall whose 429 named {named_after:?} kept the request \
                 from a host that would leave it"
            )
        });
        assert!(
            !recovered,
            "the turn must end on its original usage error, not retry the request ({named_after:?})"
        );
        assert_eq!(
            asked.walls(),
            [SpentWall {
                window: SpentWindow::FiveHour,
                resets_at: reset,
            }],
            "the host is asked once, about the usage read's reset, not the 429's ({named_after:?})"
        );
        assert_eq!(fixture.posts().await, 0, "{named_after:?}");
    }
}

/// A usage reply that carries no weekly window at all, only the spent
/// five-hour one, names the five-hour wall too: no weekly window is in the
/// way. No real account was seen to report that shape; it is what the rule
/// does with it.
#[tokio::test]
async fn a_five_hour_wall_on_a_reply_with_no_weekly_window_is_named_too() {
    let reset = hour_ahead();
    let fixture = creditless_five_hours_reading(usage(false, false, reset)).await;
    let (handoff, asked) = host(true);
    let recovered = tokio::time::timeout(
        Duration::from_secs(5),
        fixture.wait(Duration::from_secs(3600), Some(reset), Some(&handoff)),
    )
    .await
    .expect("a five-hour wall with no weekly window kept the request");
    assert!(!recovered);
    assert_eq!(
        asked.walls(),
        [SpentWall {
            window: SpentWindow::FiveHour,
            resets_at: reset,
        }]
    );
}

/// The host's no (too near its reset, no capability, nowhere to go) leaves
/// the request polling for a late credit, as before.
#[tokio::test]
async fn a_five_hour_wall_the_host_declines_keeps_polling() {
    let reset = hour_ahead();
    let fixture = creditless_five_hours(reset).await;
    let (handoff, asked) = host(false);
    assert!(
        tokio::time::timeout(
            Duration::from_secs(2),
            fixture.wait(Duration::from_secs(3600), Some(reset), Some(&handoff)),
        )
        .await
        .is_err(),
        "a host that would not leave a five-hour wall ended its wait"
    );
    assert_eq!(asked.count(), 1);
}

/// A spent week is offered whatever the 429 named, none included, as the
/// five-hour window is above.
#[tokio::test]
async fn a_spent_week_whose_429_names_no_reset_is_offered_with_the_reads_reset() {
    let reset = distant_week_reset();
    let fixture = creditless_week(credits(&[]), reset).await;
    let (handoff, asked) = host(true);
    let recovered = tokio::time::timeout(
        Duration::from_secs(5),
        fixture.wait(Duration::from_secs(3600), None, Some(&handoff)),
    )
    .await
    .expect("a spent week with no named reset was kept from a ready handoff");
    assert!(!recovered);
    assert_eq!(
        asked.walls(),
        [SpentWall {
            window: SpentWindow::Week,
            resets_at: reset,
        }]
    );
}

/// The host's no leaves the request polling whatever the 429 named: with no
/// reset named, and with one far from the window's own, the request is asked
/// about again at each poll and ends only on a yes, a restored allowance or
/// the 429's own reset.
#[tokio::test]
async fn a_declined_five_hour_wall_keeps_polling_whatever_the_429_named() {
    for named_after in [None, Some(23 * 3600), Some(-1800)] {
        let reset = hour_ahead();
        let fixture = creditless_five_hours(reset).await;
        let (handoff, asked) = host(false);
        assert!(
            tokio::time::timeout(
                Duration::from_secs(2),
                fixture.wait(
                    Duration::from_secs(3600),
                    named_after.map(|after| reset + after),
                    Some(&handoff)
                ),
            )
            .await
            .is_err(),
            "a host that would not leave a five-hour wall ended its wait ({named_after:?})"
        );
        assert_eq!(asked.count(), 1, "{named_after:?}");
    }
}

/// Both windows spent: the week is the wall, as the host judges it, and the
/// five-hour reset an hour away is not what the host is told.
#[tokio::test]
async fn a_spent_week_is_named_before_a_spent_five_hour_window() {
    let fixture = Fixture::new().await;
    let week = distant_week_reset();
    Mock::given(method("GET"))
        .and(path(USAGE))
        .respond_with(usage_of_both(hour_ahead(), week))
        .expect(1)
        .mount(&fixture.server)
        .await;
    Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(credits(&[]))
        .expect(1)
        .mount(&fixture.server)
        .await;
    let (handoff, asked) = host(true);
    let recovered = tokio::time::timeout(
        Duration::from_secs(5),
        fixture.wait(Duration::from_secs(3600), Some(week), Some(&handoff)),
    )
    .await
    .expect("a spent week beside a spent five-hour window kept the request");
    assert!(!recovered);
    let walls = asked.walls();
    assert_eq!(walls.len(), 1);
    assert_eq!(
        walls[0],
        SpentWall {
            window: SpentWindow::Week,
            resets_at: week,
        }
    );
}

#[tokio::test]
async fn an_unready_handoff_keeps_the_sampler_for_a_late_credit() {
    let fixture = Fixture::new().await;
    let reset = distant_week_reset();
    let redeemed = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&redeemed);
    Mock::given(method("GET"))
        .and(path(USAGE))
        .respond_with(move |_: &wiremock::Request| {
            usage(true, observed.load(Ordering::SeqCst), reset)
        })
        .expect(4)
        .mount(&fixture.server)
        .await;
    let lists = Arc::new(AtomicUsize::new(0));
    let listed = Arc::clone(&lists);
    Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(move |_: &wiremock::Request| {
            if listed.fetch_add(1, Ordering::SeqCst) == 0 {
                credits(&[])
            } else {
                credits(&["A"])
            }
        })
        .expect(2)
        .mount(&fixture.server)
        .await;
    Mock::given(method("POST"))
        .and(path(CONSUME))
        .respond_with(move |_: &wiremock::Request| {
            redeemed.store(true, Ordering::SeqCst);
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"code":"reset","windows_reset":2}))
        })
        .expect(1)
        .mount(&fixture.server)
        .await;
    let (handoff, asked) = host(false);
    assert!(
        tokio::time::timeout(
            Duration::from_secs(5),
            fixture.wait(Duration::from_millis(10), Some(reset), Some(&handoff)),
        )
        .await
        .expect("the late credit was never redeemed"),
        "a host that cannot rotate leaves the request to redeem a late credit"
    );
    assert_eq!(asked.count(), 1);
    assert_eq!(fixture.posts().await, 1);
}

/// One failed list read may hide an earned credit, and the earned reset comes
/// before another account: the list is read once more, one poll later. A
/// second failure in a row yields, so an outage cannot hold a spent week.
#[tokio::test]
async fn a_credit_list_outage_yields_a_weekly_wall_only_on_its_second_failed_read() {
    let fixture = Fixture::new().await;
    let reset = distant_week_reset();
    Mock::given(method("GET"))
        .and(path(USAGE))
        .respond_with(usage(true, false, reset))
        .expect(2)
        .mount(&fixture.server)
        .await;
    Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(ResponseTemplate::new(503))
        .expect(2)
        .mount(&fixture.server)
        .await;
    Mock::given(method("POST"))
        .and(path(CONSUME))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&fixture.server)
        .await;
    let (handoff, asked) = host(true);
    let recovered = tokio::time::timeout(
        Duration::from_secs(5),
        fixture.wait(Duration::from_millis(10), Some(reset), Some(&handoff)),
    )
    .await
    .expect("a credit-list outage held a spent week away from another account");
    assert!(!recovered);
    assert_eq!(
        asked.count(),
        1,
        "the host is asked after the second failed read, not the first"
    );

    // NEGATIVE CONTROL: within one poll of the first failed read nothing is asked.
    let fixture = creditless_week(ResponseTemplate::new(503), reset).await;
    let (handoff, asked) = host(true);
    assert!(
        tokio::time::timeout(
            Duration::from_secs(2),
            fixture.wait(Duration::from_secs(3600), Some(reset), Some(&handoff)),
        )
        .await
        .is_err(),
        "one failed credit read ended the request"
    );
    assert_eq!(asked.count(), 0);
}

/// The credit exists; only its first list read failed. Rotation must not
/// precede an earned reset that one more poll finds.
#[tokio::test]
async fn a_credit_behind_one_failed_list_read_is_redeemed_before_any_handoff() {
    let fixture = Fixture::new().await;
    let reset = distant_week_reset();
    let redeemed = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&redeemed);
    Mock::given(method("GET"))
        .and(path(USAGE))
        .respond_with(move |_: &wiremock::Request| {
            usage(true, observed.load(Ordering::SeqCst), reset)
        })
        .expect(4)
        .mount(&fixture.server)
        .await;
    let lists = Arc::new(AtomicUsize::new(0));
    let listed = Arc::clone(&lists);
    Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(move |_: &wiremock::Request| {
            if listed.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(503)
            } else {
                credits(&["A"])
            }
        })
        .expect(2)
        .mount(&fixture.server)
        .await;
    Mock::given(method("POST"))
        .and(path(CONSUME))
        .respond_with(move |_: &wiremock::Request| {
            redeemed.store(true, Ordering::SeqCst);
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"code":"reset","windows_reset":2}))
        })
        .expect(1)
        .mount(&fixture.server)
        .await;
    let (handoff, asked) = host(true);
    assert!(
        tokio::time::timeout(
            Duration::from_secs(5),
            fixture.wait(Duration::from_millis(10), Some(reset), Some(&handoff)),
        )
        .await
        .expect("the credit behind a failed list read was never redeemed"),
        "the earned reset resumes the original request"
    );
    assert_eq!(fixture.posts().await, 1);
    assert_eq!(
        asked.count(),
        0,
        "a ready host was asked while an earned credit existed"
    );
}

/// The backend lists a credit and then refuses it. That list offered a
/// credit, so it does not show there is none: the next poll lists again.
#[tokio::test]
async fn a_refused_credit_does_not_yield_within_the_attempt_that_tried_it() {
    let fixture = Fixture::new().await;
    let reset = distant_week_reset();
    Mock::given(method("GET"))
        .and(path(USAGE))
        .respond_with(usage(true, false, reset))
        .mount(&fixture.server)
        .await;
    Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(credits(&["A"]))
        .mount(&fixture.server)
        .await;
    Mock::given(method("POST"))
        .and(path(CONSUME))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"code":"no_credit"})),
        )
        .expect(1)
        .mount(&fixture.server)
        .await;
    let (handoff, asked) = host(true);
    assert!(
        tokio::time::timeout(
            Duration::from_secs(2),
            fixture.wait(Duration::from_secs(3600), Some(reset), Some(&handoff)),
        )
        .await
        .is_err(),
        "a credit refused in this attempt ended the request at once"
    );
    assert_eq!(asked.count(), 0);
}

/// The credit list goes empty once its only credit is consumed. If custody did
/// not come first, the propagation window would look like a creditless wall.
#[tokio::test]
async fn consumed_credit_awaiting_propagation_never_yields() {
    let fixture = Fixture::new().await;
    let reset = distant_week_reset();
    let redeemed = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&redeemed);
    let reads = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&reads);
    Mock::given(method("GET"))
        .and(path(USAGE))
        .respond_with(move |_: &wiremock::Request| {
            let count = counted.fetch_add(1, Ordering::SeqCst);
            // The applied reset moves the cycle before its usage propagates.
            let applied = observed.load(Ordering::SeqCst);
            usage(
                true,
                applied && count >= 4,
                if applied { reset + 3600 } else { reset },
            )
        })
        .expect(5)
        .mount(&fixture.server)
        .await;
    let listed = Arc::clone(&redeemed);
    Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(move |_: &wiremock::Request| {
            if listed.load(Ordering::SeqCst) {
                credits(&[])
            } else {
                credits(&["A"])
            }
        })
        .expect(1)
        .mount(&fixture.server)
        .await;
    Mock::given(method("POST"))
        .and(path(CONSUME))
        .respond_with(move |_: &wiremock::Request| {
            redeemed.store(true, Ordering::SeqCst);
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"code":"reset","windows_reset":2}))
        })
        .expect(1)
        .mount(&fixture.server)
        .await;
    let (handoff, asked) = host(true);
    assert!(
        tokio::time::timeout(
            Duration::from_secs(5),
            fixture.wait(Duration::from_millis(10), Some(reset), Some(&handoff)),
        )
        .await
        .expect("the consumed credit never restored the request"),
        "a consumed credit must resume the request it was spent for"
    );
    assert_eq!(fixture.posts().await, 1);
    assert_eq!(
        asked.count(),
        0,
        "a credit in custody must never be abandoned to another account"
    );
}

/// One window of the ordinary bucket, in the secondary slot. JSON carries no
/// NaN, so the figure is set on the decoded window.
fn reading(used: f64, minutes: i64, reset: Option<i64>) -> RateLimitsWithResetCredits {
    let mut read = RateLimitsWithResetCredits {
        rate_limits: vec![
            serde_json::from_value(serde_json::json!({
                "limit_id": "codex", "primary": null,
                "secondary": {"used_percent": 0.0, "window_minutes": minutes, "resets_at": reset}
            }))
            .unwrap(),
        ],
        ordinary_usage_allowed: Some(false),
        rate_limit_reset_credits: None,
        account_id: Some("account".into()),
        user_id: Some("user".into()),
        rate_limit_upsell: None,
    };
    read.rate_limits[0].secondary.as_mut().unwrap().used_percent = used;
    read
}

/// [`reading`]'s window as the week, with this five-hour window beside it.
fn reading_of_both(
    five_hour: (f64, Option<i64>),
    week: (f64, Option<i64>),
) -> RateLimitsWithResetCredits {
    let mut read = reading(week.0, 10080, week.1);
    read.rate_limits[0].primary = serde_json::from_value(serde_json::json!({
        "used_percent": 0.0, "window_minutes": 300, "resets_at": five_hour.1
    }))
    .unwrap();
    read.rate_limits[0].primary.as_mut().unwrap().used_percent = five_hour.0;
    read
}

#[test]
fn only_a_spent_window_with_a_future_reset_is_a_wall_to_hand_off() {
    let owner = ("account".to_owned(), "user".to_owned(), 1);
    let wall = |window, resets_at| Some(SpentWall { window, resets_at });
    assert_eq!(
        spent_wall(&reading(100.0, 10080, Some(1001)), &owner, 1000),
        wall(SpentWindow::Week, 1001)
    );
    assert_eq!(
        spent_wall(&reading(100.0, 300, Some(1901)), &owner, 1000),
        wall(SpentWindow::FiveHour, 1901)
    );
    assert_eq!(
        spent_wall(&reading(250.0, 300, Some(19_000)), &owner, 1000),
        wall(SpentWindow::FiveHour, 19_000)
    );
    for minutes in [10080, 300] {
        for (used, reset) in [
            (99.9, Some(1001)),
            (100.0, Some(1000)),
            (100.0, Some(999)),
            (100.0, None),
            (f64::NAN, Some(1001)),
            (f64::INFINITY, Some(1001)),
        ] {
            assert_eq!(
                spent_wall(&reading(used, minutes, reset), &owner, 1000),
                None,
                "{used} {minutes} {reset:?}"
            );
        }
    }
    // A window of another length is no wall this recovery knows.
    for minutes in [60, 299, 301, 1440, 10079] {
        assert_eq!(
            spent_wall(&reading(100.0, minutes, Some(1901)), &owner, 1000),
            None,
            "{minutes}"
        );
    }
    let walls: [fn() -> RateLimitsWithResetCredits; 2] = [
        || reading(100.0, 10080, Some(1001)),
        || reading(100.0, 300, Some(1901)),
    ];
    for spent in walls {
        assert!(spent_wall(&spent(), &owner, 1000).is_some());
        for permission in [None, Some(true)] {
            let mut read = spent();
            read.ordinary_usage_allowed = permission;
            assert_eq!(spent_wall(&read, &owner, 1000), None);
        }
        let mut read = spent();
        read.account_id = Some("other".into());
        assert_eq!(spent_wall(&read, &owner, 1000), None);
        let mut read = spent();
        read.user_id = Some("other".into());
        assert_eq!(spent_wall(&read, &owner, 1000), None);
        let mut read = spent();
        read.rate_limits[0].limit_id = Some("premium".into());
        assert_eq!(spent_wall(&read, &owner, 1000), None);
        let mut read = spent();
        read.rate_limits[0].spend_control_reached = Some(true);
        assert_eq!(spent_wall(&read, &owner, 1000), None);
        let mut read = spent();
        read.rate_limits[0].rate_limit_reached_type =
            Some(codex_protocol::protocol::RateLimitReachedType::WorkspaceOwnerCreditsDepleted);
        assert_eq!(spent_wall(&read, &owner, 1000), None);
    }
}

/// The week is judged first, and a five-hour wall is named only beside a week
/// that reads as usable.
#[test]
fn a_five_hour_wall_is_named_only_on_a_usable_week() {
    let owner = ("account".to_owned(), "user".to_owned(), 1);
    let five_hour = (100.0, Some(4600));
    let named = |week| spent_wall(&reading_of_both(five_hour, week), &owner, 1000);
    assert_eq!(
        named((100.0, Some(90_000))),
        Some(SpentWall {
            window: SpentWindow::Week,
            resets_at: 90_000,
        }),
        "both spent: the week"
    );
    let five_hours = Some(SpentWall {
        window: SpentWindow::FiveHour,
        resets_at: 4600,
    });
    assert_eq!(named((0.0, Some(90_000))), five_hours);
    assert_eq!(named((99.9, Some(90_000))), five_hours);
    assert_eq!(named((99.9, None)), five_hours);
    // A weekly window that is spent with no reset ahead, or whose figure is
    // not a finite number, is in the way: the five-hour reset would not end
    // that wall.
    for week in [
        (100.0, Some(1000)),
        (100.0, None),
        (f64::NAN, Some(90_000)),
        (f64::INFINITY, Some(90_000)),
    ] {
        assert_eq!(named(week), None, "{week:?}");
    }
    // A spent week beside a five-hour window that is not spent is the week's.
    assert_eq!(
        spent_wall(
            &reading_of_both((40.0, Some(4600)), (100.0, Some(90_000))),
            &owner,
            1000
        )
        .map(|wall| wall.window),
        Some(SpentWindow::Week)
    );
}

/// Among several spent windows of one length the wall stands until the last
/// of them resets, whichever slot carries it. A window of that length that is
/// not spent does not move it.
#[test]
fn the_latest_reset_among_spent_windows_of_one_length_is_the_walls() {
    let owner = ("account".to_owned(), "user".to_owned(), 1);
    // Two windows of `minutes`: the primary slot's, then the secondary's.
    let pair = |minutes: i64, primary: (f64, i64), secondary: (f64, i64)| {
        let mut read = reading(secondary.0, minutes, Some(secondary.1));
        read.rate_limits[0].primary = serde_json::from_value(serde_json::json!({
            "used_percent": 0.0, "window_minutes": minutes, "resets_at": primary.1
        }))
        .unwrap();
        read.rate_limits[0].primary.as_mut().unwrap().used_percent = primary.0;
        spent_wall(&read, &owner, 1000)
    };
    for (minutes, window) in [(300, SpentWindow::FiveHour), (10080, SpentWindow::Week)] {
        let wall = |resets_at| Some(SpentWall { window, resets_at });
        assert_eq!(
            pair(minutes, (100.0, 2500), (100.0, 1901)),
            wall(2500),
            "{minutes}: the later reset, in the primary slot"
        );
        assert_eq!(
            pair(minutes, (100.0, 1901), (100.0, 2500)),
            wall(2500),
            "{minutes}: the later reset, in the secondary slot"
        );
        assert_eq!(pair(minutes, (100.0, 1901), (100.0, 1901)), wall(1901));
        // One of the two already reset: the other is the wall.
        assert_eq!(pair(minutes, (100.0, 900), (100.0, 1901)), wall(1901));
        assert_eq!(pair(minutes, (100.0, 2500), (100.0, 1000)), wall(2500));
    }
    // A five-hour window that is not spent has no say, however late it resets.
    assert_eq!(
        pair(300, (99.9, 2500), (100.0, 1901)),
        Some(SpentWall {
            window: SpentWindow::FiveHour,
            resets_at: 1901,
        })
    );
    // A weekly window that is not spent beside a spent one: the spent one is the wall.
    assert_eq!(
        pair(10080, (99.9, 9000), (100.0, 5000)),
        Some(SpentWall {
            window: SpentWindow::Week,
            resets_at: 5000,
        })
    );
}

/// A spent week `reset` away whose usage is read each look, with a credit list
/// that offers `A` once `offered` is set and a redemption that restores the
/// allowance. The usage reads so far are counted.
struct Waited {
    fixture: Fixture,
    reads: Arc<AtomicUsize>,
    offered: Arc<AtomicBool>,
}

async fn waited(reset: i64) -> Waited {
    let fixture = Fixture::new().await;
    let reads = Arc::new(AtomicUsize::new(0));
    let offered = Arc::new(AtomicBool::new(false));
    let redeemed = Arc::new(AtomicBool::new(false));
    let (counted, restored) = (Arc::clone(&reads), Arc::clone(&redeemed));
    Mock::given(method("GET"))
        .and(path(USAGE))
        .respond_with(move |_: &wiremock::Request| {
            counted.fetch_add(1, Ordering::SeqCst);
            usage(true, restored.load(Ordering::SeqCst), reset)
        })
        .mount(&fixture.server)
        .await;
    let listed = Arc::clone(&offered);
    Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(move |_: &wiremock::Request| {
            if listed.load(Ordering::SeqCst) {
                credits(&["A"])
            } else {
                credits(&[])
            }
        })
        .mount(&fixture.server)
        .await;
    Mock::given(method("POST"))
        .and(path(CONSUME))
        .respond_with(move |_: &wiremock::Request| {
            redeemed.store(true, Ordering::SeqCst);
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"code":"reset","windows_reset":2}))
        })
        .mount(&fixture.server)
        .await;
    Waited {
        fixture,
        reads,
        offered,
    }
}

/// A host that is not ready at its first question and ready from the second,
/// with the signal it sends when its answer may have changed, and how often it
/// was asked.
fn host_that_signals() -> (
    WallHandoff,
    tokio::sync::watch::Sender<u64>,
    Arc<AtomicUsize>,
) {
    let (wake, signals) = tokio::sync::watch::channel(0_u64);
    let asked = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&asked);
    let handoff = WallHandoff::new(move |_| {
        let ready = counted.fetch_add(1, Ordering::SeqCst) >= 1;
        async move { ready }
    })
    .waking(signals);
    (handoff, wake, asked)
}

/// Longer than any test here waits: a look that comes is never the poll's.
const NEVER: Duration = Duration::from_secs(3600);

/// A HOST'S SIGNAL HAS IT ASKED AGAIN AT ONCE, NOT AT THE NEXT POLL. The request
/// holds its turn, and so the session's record, busy while it waits on a host
/// that was not ready; a host that moves accounts only under an idle record
/// would wait out the poll interval for the very turn it needs ended.
#[tokio::test]
async fn a_host_that_signals_is_asked_again_before_the_poll() {
    let reset = distant_week_reset();
    let waited = waited(reset).await;
    let (handoff, wake, asked) = host_that_signals();
    let waiting = waited.fixture.wait(NEVER, Some(reset), Some(&handoff));
    tokio::pin!(waiting);
    // CONTROL: a host that is not ready, and silent, leaves the request waiting.
    assert!(
        tokio::time::timeout(Duration::from_millis(1500), &mut waiting)
            .await
            .is_err(),
        "a host that was not ready ended the wait"
    );
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(waited.reads.load(Ordering::SeqCst), 1);

    wake.send_modify(|signals| *signals += 1);
    let recovered = tokio::time::timeout(Duration::from_secs(10), &mut waiting)
        .await
        .expect("the host's signal did not have it asked again");
    assert!(
        !recovered,
        "the turn ends on its original usage error for the host that is now ready"
    );
    assert_eq!(asked.load(Ordering::SeqCst), 2);
    assert_eq!(
        waited.reads.load(Ordering::SeqCst),
        2,
        "the signal's look reads the usage again before the host is asked"
    );
    assert_eq!(waited.fixture.posts().await, 0);
}

/// THE EARNED RESET STILL COMES FIRST AT A SIGNAL. The signal only ends the wait
/// early: a credit that became available meanwhile is redeemed and the request
/// resumes, and the host, which would now say yes, is not asked.
#[tokio::test]
async fn a_credit_found_at_the_signal_is_redeemed_before_the_host_is_asked() {
    let reset = distant_week_reset();
    let waited = waited(reset).await;
    let (handoff, wake, asked) = host_that_signals();
    let waiting = waited.fixture.wait(NEVER, Some(reset), Some(&handoff));
    tokio::pin!(waiting);
    assert!(
        tokio::time::timeout(Duration::from_millis(1500), &mut waiting)
            .await
            .is_err(),
        "a host that was not ready ended the wait"
    );
    assert_eq!(asked.load(Ordering::SeqCst), 1);

    waited.offered.store(true, Ordering::SeqCst);
    wake.send_modify(|signals| *signals += 1);
    let recovered = tokio::time::timeout(Duration::from_secs(10), &mut waiting)
        .await
        .expect("the host's signal did not bring another look");
    assert!(recovered, "the earned reset resumes the original request");
    assert_eq!(waited.fixture.posts().await, 1);
    assert_eq!(
        asked.load(Ordering::SeqCst),
        1,
        "the host was asked before the credit the signal's look found was redeemed"
    );
}

/// A SIGNAL SENT WHILE THE HOST IS BEING ASKED IS NOT LOST. The host's answer
/// can turn to yes between its no and this request's wait, and the signal for
/// that comes before anything waits on it.
#[tokio::test]
async fn a_signal_sent_during_the_question_is_not_lost() {
    let reset = distant_week_reset();
    let waited = waited(reset).await;
    let (wake, signals) = tokio::sync::watch::channel(0_u64);
    let asked = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&asked);
    let handoff = WallHandoff::new(move |_| {
        let first = counted.fetch_add(1, Ordering::SeqCst) == 0;
        if first {
            // Not ready, and ready a moment later: signalled before the answer lands.
            wake.send_modify(|signals| *signals += 1);
        }
        async move { !first }
    })
    .waking(signals);
    let recovered = tokio::time::timeout(
        Duration::from_secs(10),
        waited.fixture.wait(NEVER, Some(reset), Some(&handoff)),
    )
    .await
    .expect("a signal sent while the host was asked was lost");
    assert!(!recovered);
    assert_eq!(asked.load(Ordering::SeqCst), 2);
}

/// A HOST THAT IS GONE LEAVES THE POLL, AND NOTHING FASTER. Its signal's sender
/// is dropped; a wait that read that as a signal would look, ask and wait
/// again without pause, a usage read each time.
#[tokio::test]
async fn a_host_whose_signal_is_gone_is_asked_only_at_the_poll() {
    const POLL: Duration = Duration::from_secs(2);
    let reset = distant_week_reset();
    let waited = waited(reset).await;
    let (wake, signals) = tokio::sync::watch::channel(0_u64);
    let handoff = WallHandoff::new(|_| async { false }).waking(signals);
    drop(wake);
    let started = tokio::time::Instant::now();
    let waiting = waited.fixture.wait(POLL, Some(reset), Some(&handoff));
    tokio::pin!(waiting);
    assert!(
        tokio::time::timeout(Duration::from_secs(1), &mut waiting)
            .await
            .is_err()
    );
    assert_eq!(
        waited.reads.load(Ordering::SeqCst),
        1,
        "a host that is gone was read as one that signals"
    );
    // The poll still comes.
    let deadline = started + Duration::from_secs(30);
    while waited.reads.load(Ordering::SeqCst) < 2 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the poll never came"
        );
        let _ = tokio::time::timeout(Duration::from_millis(50), &mut waiting).await;
    }
    assert!(
        started.elapsed() >= POLL,
        "the second look came before the poll: {:?}",
        started.elapsed()
    );
}

/// SIGNALS THAT KEEP COMING BUY ONE LOOK A SECOND. Each look is a usage read
/// and a credit list read, and the signal is the host's to send as often as its
/// state moves.
#[tokio::test]
async fn signals_that_keep_coming_buy_one_look_a_second() {
    let reset = distant_week_reset();
    let waited = waited(reset).await;
    let (wake, signals) = tokio::sync::watch::channel(0_u64);
    let handoff = WallHandoff::new(|_| async { false }).waking(signals);
    let waiting = waited.fixture.wait(NEVER, Some(reset), Some(&handoff));
    tokio::pin!(waiting);
    let started = tokio::time::Instant::now();
    while started.elapsed() < Duration::from_millis(2500) {
        wake.send_modify(|signals| *signals += 1);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut waiting)
                .await
                .is_err()
        );
    }
    let reads = waited.reads.load(Ordering::SeqCst);
    // The first look, then one a second: the floor is between the starts of two looks.
    assert!(
        (2..=4).contains(&reads),
        "{reads} looks in 2.5 s of signals"
    );
}

/// A spent week `reset` away behind a credit list outage: the list answers 503
/// to its first `failures` reads (every read, with `None`) and offers `A` after
/// them, whose redemption restores the allowance. The list reads so far are
/// counted.
async fn waited_behind_an_outage(
    reset: i64,
    failures: Option<usize>,
) -> (Fixture, Arc<AtomicUsize>) {
    let fixture = Fixture::new().await;
    let redeemed = Arc::new(AtomicBool::new(false));
    let restored = Arc::clone(&redeemed);
    Mock::given(method("GET"))
        .and(path(USAGE))
        .respond_with(move |_: &wiremock::Request| {
            usage(true, restored.load(Ordering::SeqCst), reset)
        })
        .mount(&fixture.server)
        .await;
    let lists = Arc::new(AtomicUsize::new(0));
    let listed = Arc::clone(&lists);
    Mock::given(method("GET"))
        .and(path(CREDITS))
        .respond_with(move |_: &wiremock::Request| {
            let read = listed.fetch_add(1, Ordering::SeqCst);
            if failures.is_none_or(|failures| read < failures) {
                ResponseTemplate::new(503)
            } else {
                credits(&["A"])
            }
        })
        .mount(&fixture.server)
        .await;
    Mock::given(method("POST"))
        .and(path(CONSUME))
        .respond_with(move |_: &wiremock::Request| {
            redeemed.store(true, Ordering::SeqCst);
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"code":"reset","windows_reset":2}))
        })
        .mount(&fixture.server)
        .await;
    (fixture, lists)
}

/// A host that would end the request whenever it is asked, with the signal it
/// sends, and how often it was asked.
fn ready_host_that_signals() -> (
    WallHandoff,
    tokio::sync::watch::Sender<u64>,
    Arc<AtomicUsize>,
) {
    let (wake, signals) = tokio::sync::watch::channel(0_u64);
    let asked = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&asked);
    let handoff = WallHandoff::new(move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
        async { true }
    })
    .waking(signals);
    (handoff, wake, asked)
}

/// A SIGNAL'S LOOK DOES NOT STAND IN FOR THE POLL'S AFTER AN UNREAD CREDIT LIST.
/// One unread list may hide an earned credit, so the request is not ended for
/// the host until the list has stayed unread for a whole poll. A host's signal
/// brings a look within a second or so, and a host whose move begins sends two
/// in a row; if those looks counted as the poll's, an outage of two seconds
/// would end a request that had a credit to redeem. Here the list fails twice
/// and then offers the credit: the signal's look reads the second failure, the
/// host, which would say yes, is not asked, and the next look redeems.
#[tokio::test]
async fn unread_credit_lists_a_signal_apart_do_not_end_the_request() {
    let reset = distant_week_reset();
    let (fixture, lists) = waited_behind_an_outage(reset, Some(2)).await;
    let (handoff, wake, asked) = ready_host_that_signals();
    let waiting = fixture.wait(NEVER, Some(reset), Some(&handoff));
    tokio::pin!(waiting);
    assert!(
        tokio::time::timeout(Duration::from_millis(1500), &mut waiting)
            .await
            .is_err(),
        "one unread credit list ended the request"
    );
    assert_eq!(lists.load(Ordering::SeqCst), 1);
    assert_eq!(asked.load(Ordering::SeqCst), 0);

    // The signal's look: the list is unread again, about a second after the first.
    wake.send_modify(|signals| *signals += 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(1500), &mut waiting)
            .await
            .is_err(),
        "two unread credit lists a signal apart ended the request"
    );
    assert_eq!(
        lists.load(Ordering::SeqCst),
        2,
        "the signal brought no look"
    );
    assert_eq!(
        asked.load(Ordering::SeqCst),
        0,
        "the host was asked within one poll of the first unread list"
    );

    // The credit the outage hid is found at the next look and redeemed.
    wake.send_modify(|signals| *signals += 1);
    let recovered = tokio::time::timeout(Duration::from_secs(10), &mut waiting)
        .await
        .expect("the credit behind the outage was never redeemed");
    assert!(recovered, "the earned reset resumes the original request");
    assert_eq!(fixture.posts().await, 1);
    assert_eq!(asked.load(Ordering::SeqCst), 0);
}

/// THE HOST HEARS EVERY LOOK BEGIN, BEFORE IT READS ANYTHING, THE LOOKS THAT
/// NEVER ASK IT TOO. A host that signals only once a look began since its last
/// signal must hear of a look whose credit list is unread: that look ends
/// before the question, and its request waits on the host's next signal as much
/// as one the host told no. Here the list fails twice and then offers the
/// credit: the host hears three looks, each before its list read, and is asked
/// nothing.
#[tokio::test]
async fn the_host_hears_every_look_begin_whether_or_not_it_is_asked() {
    let reset = distant_week_reset();
    let (fixture, lists) = waited_behind_an_outage(reset, Some(2)).await;
    let (wake, signals) = tokio::sync::watch::channel(0_u64);
    let asked = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&asked);
    // The list reads made before each look the host heard of.
    let heard = Arc::new(Mutex::new(Vec::new()));
    let (told, read) = (Arc::clone(&heard), Arc::clone(&lists));
    let handoff = WallHandoff::new(move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
        async { true }
    })
    .waking(signals)
    .hearing_looks(move || {
        told.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(read.load(Ordering::SeqCst));
    });
    let heard = || {
        heard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    };
    let waiting = fixture.wait(NEVER, Some(reset), Some(&handoff));
    tokio::pin!(waiting);
    assert!(
        tokio::time::timeout(Duration::from_millis(1500), &mut waiting)
            .await
            .is_err(),
        "one unread credit list ended the request"
    );
    assert_eq!(heard(), [0], "the first look, before its list read");

    wake.send_modify(|signals| *signals += 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(1500), &mut waiting)
            .await
            .is_err()
    );
    assert_eq!(
        lists.load(Ordering::SeqCst),
        2,
        "the signal brought no look"
    );
    assert_eq!(
        heard(),
        [0, 1],
        "a look whose list was unread was not heard before it read"
    );

    wake.send_modify(|signals| *signals += 1);
    let recovered = tokio::time::timeout(Duration::from_secs(10), &mut waiting)
        .await
        .expect("the credit behind the outage was never redeemed");
    assert!(recovered);
    assert_eq!(heard(), [0, 1, 2]);
    assert_eq!(
        asked.load(Ordering::SeqCst),
        0,
        "a look that ended before its question asked the host"
    );
}

/// A CREDIT LIST OUTAGE STILL YIELDS AFTER ONE WHOLE POLL, HOWEVER OFTEN THE
/// HOST SIGNALS. Each signal starts the wait for the next poll again, so a rule
/// that counted only the poll's own looks would hold a spent week away from
/// another account for as long as signals kept coming. The rule is the span:
/// the list unread at a look a poll interval or more after it first was.
#[tokio::test]
async fn a_credit_list_outage_yields_after_a_whole_poll_whatever_the_host_signals() {
    const POLL: Duration = Duration::from_secs(3);
    let reset = distant_week_reset();
    let (fixture, lists) = waited_behind_an_outage(reset, None).await;
    let (handoff, wake, asked) = ready_host_that_signals();
    let started = tokio::time::Instant::now();
    let waiting = fixture.wait(POLL, Some(reset), Some(&handoff));
    tokio::pin!(waiting);
    let recovered = loop {
        wake.send_modify(|signals| *signals += 1);
        if let Ok(recovered) = tokio::time::timeout(Duration::from_millis(50), &mut waiting).await {
            break recovered;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "an outage under a signalling host never yielded: {} list reads",
            lists.load(Ordering::SeqCst)
        );
    };
    assert!(!recovered, "the request ends for the host that is ready");
    assert!(
        started.elapsed() >= POLL,
        "the host was asked {:?} after the first unread list, within one poll",
        started.elapsed()
    );
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert!(
        lists.load(Ordering::SeqCst) >= 3,
        "the signals brought no looks of their own: {} list reads",
        lists.load(Ordering::SeqCst)
    );
    assert_eq!(fixture.posts().await, 0);
}
