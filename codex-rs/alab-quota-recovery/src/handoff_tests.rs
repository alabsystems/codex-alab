// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A spent allowance with no earned credit ends the original request for a
//! host that would leave that wall for another account: a spent week, or a
//! spent five-hour window whose reset the 429 itself names. The host is told
//! which. Every other wall keeps polling.

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

fn hour_ahead() -> i64 {
    chrono::Utc::now().timestamp() + 3600
}

fn day_ahead() -> i64 {
    chrono::Utc::now().timestamp() + 24 * 3600
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
    let reset = day_ahead();
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

#[tokio::test]
async fn an_available_credit_is_redeemed_before_any_handoff_is_considered() {
    let fixture = Fixture::new().await;
    let reset = day_ahead();
    let redeemed = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&redeemed);
    Mock::given(method("GET"))
        .and(path(USAGE))
        .respond_with(move |_: &wiremock::Request| {
            usage(true, observed.load(Ordering::SeqCst), reset)
        })
        .expect(2)
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
/// names it, and its yes ends the request. The 429 and the usage read name
/// one reset, give or take what two reports of it may differ by.
#[tokio::test]
async fn a_creditless_five_hour_wall_asks_the_host_about_that_window() {
    for named_after in [0, 50, -50] {
        let reset = hour_ahead();
        let fixture = creditless_five_hours(reset).await;
        let (handoff, asked) = host(true);
        let recovered = tokio::time::timeout(
            Duration::from_secs(5),
            fixture.wait(
                Duration::from_secs(3600),
                Some(reset + named_after),
                Some(&handoff),
            ),
        )
        .await
        .expect("a creditless five-hour wall kept the request from a host that would leave it");
        assert!(
            !recovered,
            "the turn must end on its original usage error, not retry the request"
        );
        let walls = asked.walls();
        assert_eq!(walls.len(), 1, "{named_after}");
        assert_eq!(
            walls[0],
            SpentWall {
                window: SpentWindow::FiveHour,
                resets_at: reset,
            },
            "the usage read's reset, not the 429's ({named_after})"
        );
        assert_eq!(fixture.posts().await, 0);
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

/// What follows an ended turn runs on the 429's reset. With none named, an
/// ended five-hour turn would be asked about without bound, so it is not
/// offered. (A spent week is: `weekly_wall_without_credit_...` and below.)
#[tokio::test]
async fn a_five_hour_wall_whose_429_names_no_reset_keeps_polling() {
    let fixture = creditless_five_hours(hour_ahead()).await;
    let (handoff, asked) = host(true);
    assert!(
        tokio::time::timeout(
            Duration::from_secs(2),
            fixture.wait(Duration::from_secs(3600), None, Some(&handoff)),
        )
        .await
        .is_err(),
        "a five-hour wall with no named reset was handed to the host"
    );
    assert_eq!(asked.count(), 0);

    // POSITIVE CONTROL: a spent week whose 429 names no reset is still offered.
    let fixture = creditless_week(credits(&[]), day_ahead()).await;
    let (handoff, asked) = host(true);
    let recovered = tokio::time::timeout(
        Duration::from_secs(5),
        fixture.wait(Duration::from_secs(3600), None, Some(&handoff)),
    )
    .await
    .expect("a spent week with no named reset was kept from a ready handoff");
    assert!(!recovered);
    assert_eq!(asked.count(), 1);
}

/// A 429 that names a later reset than the five-hour window's would hold the
/// ended session, and the bound on asking the host, to that later moment.
#[tokio::test]
async fn a_five_hour_wall_whose_429_names_a_later_reset_keeps_polling() {
    for later in [23 * 3600, 120] {
        let reset = hour_ahead();
        let fixture = creditless_five_hours(reset).await;
        let (handoff, asked) = host(true);
        assert!(
            tokio::time::timeout(
                Duration::from_secs(2),
                fixture.wait(
                    Duration::from_secs(3600),
                    Some(reset + later),
                    Some(&handoff)
                ),
            )
            .await
            .is_err(),
            "a 429 naming a reset {later} s after the window's own was handed to the host"
        );
        assert_eq!(asked.count(), 0, "{later}");
    }
}

/// A 429 that names an earlier reset than the five-hour window's would have
/// the ended session resumed into the same wall.
#[tokio::test]
async fn a_five_hour_wall_whose_429_names_an_earlier_reset_keeps_polling() {
    let reset = hour_ahead();
    let fixture = creditless_five_hours(reset).await;
    let (handoff, asked) = host(true);
    assert!(
        tokio::time::timeout(
            Duration::from_secs(2),
            fixture.wait(
                Duration::from_secs(3600),
                Some(reset - 1800),
                Some(&handoff)
            ),
        )
        .await
        .is_err(),
        "a 429 naming a reset half an hour before the window's own was handed to the host"
    );
    assert_eq!(asked.count(), 0);
}

/// Both windows spent: the week is the wall, as the host judges it, and the
/// five-hour reset an hour away is not what the host is told.
#[tokio::test]
async fn a_spent_week_is_named_before_a_spent_five_hour_window() {
    let fixture = Fixture::new().await;
    let week = day_ahead();
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
    let reset = day_ahead();
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
    let reset = day_ahead();
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
    let reset = day_ahead();
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
    let reset = day_ahead();
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
    let reset = day_ahead();
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

/// A spent week is offered whatever the 429 named; a spent five-hour window
/// only when the 429's reset is the window's own, within a minute. Both are
/// moments, so the rule reads no clock and the minute is exact.
#[test]
fn only_a_five_hour_wall_the_429_agrees_with_is_offered() {
    const NOW: i64 = 1_800_000_000;
    let wall = |window, seconds: i64| SpentWall {
        window,
        resets_at: NOW + seconds,
    };
    for reset_at in [None, Some(NOW + 30), Some(NOW + 3600), Some(NOW + 900_000)] {
        assert!(
            offerable(wall(SpentWindow::Week, 86_400), reset_at),
            "{reset_at:?}"
        );
    }
    let five_hours = wall(SpentWindow::FiveHour, 3600);
    for named in [3600, 3600 + 60, 3600 - 60] {
        assert!(offerable(five_hours, Some(NOW + named)), "{named}");
    }
    for named in [3600 + 61, 3600 - 61, 86_400, 30, 0, -5] {
        assert!(!offerable(five_hours, Some(NOW + named)), "{named}");
    }
    assert!(!offerable(five_hours, None));
    // Resets no clock could name do not wrap into agreement.
    for (window_reset, named) in [(i64::MAX, i64::MIN), (i64::MIN, i64::MAX), (i64::MAX, 0)] {
        assert!(
            !offerable(
                SpentWall {
                    window: SpentWindow::FiveHour,
                    resets_at: window_reset,
                },
                Some(named)
            ),
            "{window_reset} {named}"
        );
    }
}
