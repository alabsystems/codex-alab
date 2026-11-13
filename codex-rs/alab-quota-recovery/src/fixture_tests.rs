// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The replies the rotation smokes serve, decoded by the real client.
//!
//! `alab/smoke/provider_fixture.py` builds the usage, earned-reset list and
//! redemption replies of the loopback provider the rotation smokes run
//! against, and the refusal of a model request at a wall, and writes them to
//! `alab/smoke/fixtures/provider`. A reply this client cannot decode makes a
//! walled turn poll until the smoke's hang detector, and a refusal it does
//! not read as a usage limit is retried as an ordinary 429, so the files are
//! decoded here, where the gate runs.

use super::*;
use codex_alab_core::quota::WallHandoff;
use std::sync::Arc;
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

/// One golden reply, as the smoke's fixture wrote it.
fn golden(name: &str) -> serde_json::Value {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../alab/smoke/fixtures/provider")
        .join(name);
    let text = std::fs::read_to_string(&file)
        .unwrap_or_else(|error| panic!("{}: {error}", file.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// A golden usage reply as the fixture serves it now: every window's reset
/// lies as far ahead of this moment as its `reset_after_seconds` says, or
/// `primary` seconds ahead for the five-hour window when given.
fn usage_now(name: &str, primary: Option<i64>) -> serde_json::Value {
    let mut body = golden(name);
    let now = chrono::Utc::now().timestamp();
    for (window, ahead) in [("primary_window", primary), ("secondary_window", None)] {
        let window = &mut body["rate_limit"][window];
        let stated = window["reset_after_seconds"]
            .as_i64()
            .expect("the golden window states its reset distance");
        let ahead = ahead.unwrap_or(stated);
        window["reset_after_seconds"] = ahead.into();
        window["reset_at"] = (now + ahead).into();
    }
    body
}

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let group = chunk
            .iter()
            .fold(0_u32, |group, byte| (group << 8) | u32::from(*byte))
            << (8 * (3 - chunk.len()));
        for index in 0..=chunk.len() {
            let sextet = (group >> (18 - 6 * index)) & 63;
            text.push(char::from(ALPHABET[sextet as usize]));
        }
    }
    text
}

/// A login whose token claims name the account and user a golden usage reply
/// is for, as the smoke's fixture logins do.
struct Login {
    server: MockServer,
    _home: tempfile::TempDir,
    manager: Arc<AuthManager>,
    owner: (String, String, u64),
    config: Settings,
}

impl Login {
    async fn of(usage: &serde_json::Value) -> Self {
        codex_alab_core::activate();
        let account = usage["account_id"].as_str().unwrap();
        let claims = serde_json::json!({"https://api.openai.com/auth": {
            "chatgpt_account_id": account,
            "chatgpt_user_id": usage["user_id"].as_str().unwrap(),
            "chatgpt_plan_type": usage["plan_type"].as_str().unwrap(),
        }});
        let token = format!("e30.{}.fixture", base64url(claims.to_string().as_bytes()));
        let auth = codex_login::CodexAuth::from_external_chatgpt_tokens(
            &token,
            account,
            usage["plan_type"].as_str(),
        )
        .unwrap();
        let server = MockServer::start().await;
        let home = tempfile::tempdir().unwrap();
        let manager = AuthManager::from_auth_for_testing_with_home(auth, home.path().to_owned());
        let owner = State::bind(&manager)
            .await
            .owner
            .expect("the fixture login names an account and a user");
        let config = Settings {
            home: home.path().to_owned(),
            base: server.uri(),
            factory: HttpClientFactory::new(codex_http_client::OutboundProxyPolicy::ReqwestDefault),
        };
        Self {
            server,
            _home: home,
            manager,
            owner,
            config,
        }
    }

    async fn serve(&self, verb: &str, route: &str, body: serde_json::Value) {
        Mock::given(method(verb))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&self.server)
            .await;
    }

    async fn client(&self) -> Client {
        let auth = self.manager.auth().await.unwrap();
        Client::from_auth(self.config.base.clone(), &auth, self.config.factory.clone())
    }

    async fn received(&self, verb: &str, route: &str) -> usize {
        self.server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.method.as_str() == verb && request.url.path() == route)
            .count()
    }
}

/// A host that would move a session off a spent week and leaves no five-hour
/// wall (it prints no bound for one), and how often it was asked about each.
fn ready_host() -> (WallHandoff, Arc<Asked>) {
    let calls = Arc::new(Asked::default());
    let asked = Arc::clone(&calls);
    let handoff = WallHandoff::new(move |wall: SpentWall| {
        let asked = Arc::clone(&asked);
        async move {
            match wall.window {
                SpentWindow::Week => asked.week.fetch_add(1, Ordering::SeqCst),
                SpentWindow::FiveHour => asked.five_hour.fetch_add(1, Ordering::SeqCst),
            };
            wall.window == SpentWindow::Week
        }
    });
    (handoff, calls)
}

/// How often the host was asked whether it would leave each kind of wall.
#[derive(Default)]
struct Asked {
    week: AtomicUsize,
    five_hour: AtomicUsize,
}

#[tokio::test]
async fn the_fixtures_weekly_wall_ends_the_turn_for_a_ready_host() {
    let usage = usage_now("usage-weekly-wall.json", None);
    let reset = usage["rate_limit"]["secondary_window"]["reset_at"]
        .as_i64()
        .unwrap();
    let login = Login::of(&usage).await;
    login.serve("GET", USAGE, usage).await;
    login
        .serve("GET", CREDITS, golden("credits-empty.json"))
        .await;
    let (handoff, asked) = ready_host();
    let recovered = tokio::time::timeout(
        Duration::from_secs(10),
        wait_for_reset(
            &login.manager,
            &login.config,
            &login.owner,
            "turn",
            Duration::from_secs(3600),
            Some(reset),
            Some(&handoff),
        ),
    )
    .await
    .expect("the fixture's weekly wall kept the walled turn polling");
    assert!(
        !recovered,
        "the turn ends on its usage error; it is not retried"
    );
    assert_eq!(
        asked.week.load(Ordering::SeqCst),
        1,
        "one clean, empty credit list is conclusive: the host is asked at once"
    );
    assert_eq!(asked.five_hour.load(Ordering::SeqCst), 0);
    assert_eq!(login.received("GET", USAGE).await, 1);
    assert_eq!(login.received("GET", CREDITS).await, 1);
    assert_eq!(
        login.received("POST", CONSUME).await,
        0,
        "nothing is redeemed"
    );
}

#[tokio::test]
async fn the_fixtures_healthy_usage_permits_a_turn() {
    let usage = usage_now("usage-healthy.json", None);
    let login = Login::of(&usage).await;
    login.serve("GET", USAGE, usage).await;
    let read = login
        .client()
        .await
        .get_rate_limits_with_reset_credits()
        .await
        .expect("the fixture's healthy usage reply does not decode");
    assert!(matches(&read, &login.owner));
    assert_eq!(read.ordinary_usage_allowed, Some(true));
    assert_eq!(ordinary_used(&read), Some(0.0));
    assert!(usage_below(&read, &login.owner, 95.0));
    assert_eq!(
        spent_wall(&read, &login.owner, chrono::Utc::now().timestamp()),
        None
    );
}

#[tokio::test]
async fn the_fixtures_five_hour_wall_is_waited_out_on_its_own_account() {
    // The golden wall resets in ten minutes; served here two seconds away.
    let usage = usage_now("usage-five-hour-wall.json", Some(2));
    let reset = usage["rate_limit"]["primary_window"]["reset_at"]
        .as_i64()
        .unwrap();
    let login = Login::of(&usage).await;
    login.serve("GET", USAGE, usage).await;
    login
        .serve("GET", CREDITS, golden("credits-empty.json"))
        .await;
    let read = login
        .client()
        .await
        .get_rate_limits_with_reset_credits()
        .await
        .expect("the fixture's five-hour wall does not decode");
    let now = chrono::Utc::now().timestamp();
    assert_eq!(read.ordinary_usage_allowed, Some(false));
    assert_eq!(ordinary_used(&read), Some(100.0));
    assert_eq!(
        spent_wall(&read, &login.owner, now),
        Some(SpentWall {
            window: SpentWindow::FiveHour,
            resets_at: reset,
        }),
        "the week is usable: the wall is the five-hour window's"
    );
    let (handoff, asked) = ready_host();
    let started = std::time::Instant::now();
    let recovered = tokio::time::timeout(
        Duration::from_secs(20),
        wait_for_reset(
            &login.manager,
            &login.config,
            &login.owner,
            "turn",
            Duration::from_secs(3600),
            Some(reset),
            Some(&handoff),
        ),
    )
    .await
    .expect("the wait outlived the wall's own reset");
    assert!(
        !recovered,
        "the fixture never restores usage, so the reset ends the wait"
    );
    assert!(
        chrono::Utc::now().timestamp() >= reset,
        "the wait ended {:?} in, before the wall's reset",
        started.elapsed()
    );
    assert_eq!(
        asked.week.load(Ordering::SeqCst),
        0,
        "the host was asked about a week that is not spent"
    );
    assert!(
        asked.five_hour.load(Ordering::SeqCst) >= 1,
        "the 429 named the five-hour window's own reset, so the host is asked \
         whether it leaves that wall; it does not, and the wall is waited out"
    );
    assert_eq!(login.received("POST", CONSUME).await, 0);
}

#[tokio::test]
async fn the_fixtures_redemption_reply_says_no_credit() {
    let login = Login::of(&golden("usage-weekly-wall.json")).await;
    login
        .serve("POST", CONSUME, golden("consume-no-credit.json"))
        .await;
    let reply = login
        .client()
        .await
        .consume_rate_limit_reset_credit_by_id("request", "credit")
        .await
        .expect("the fixture's redemption reply does not decode");
    assert_eq!(reply.code, ConsumeRateLimitResetCreditCode::NoCredit);
    assert_eq!(reply.windows_reset, 0);
    assert_eq!(login.received("POST", CONSUME).await, 1);

    let listed = golden("credits-empty.json");
    login.serve("GET", CREDITS, listed).await;
    let credits = login
        .client()
        .await
        .list_rate_limit_reset_credits()
        .await
        .expect("the fixture's credit list does not decode");
    assert_eq!(credits.available_count, 0);
    assert!(credits.credits.is_empty());
}

/// The reply the smoke's fixture served before `provider_fixture.py`: a window
/// with no `reset_after_seconds`. The client cannot read it, which is why a
/// walled turn against that fixture never ended.
#[tokio::test]
async fn a_window_without_its_reset_distance_is_an_unreadable_usage_reply() {
    for window in ["primary_window", "secondary_window"] {
        let mut usage = usage_now("usage-weekly-wall.json", None);
        usage["rate_limit"][window]
            .as_object_mut()
            .unwrap()
            .remove("reset_after_seconds")
            .expect("the golden window states its reset distance");
        let login = Login::of(&usage).await;
        login.serve("GET", USAGE, usage).await;
        assert!(
            login
                .client()
                .await
                .get_rate_limits_with_reset_credits()
                .await
                .is_err(),
            "a {window} without reset_after_seconds decoded"
        );
    }
}

/// A golden refusal, and what codex-api's error mapping makes of it when the
/// transport hands over its status, headers and body text, as it does for a
/// refused model request.
fn refused(reply: &serde_json::Value) -> codex_protocol::error::CodexErr {
    let mut headers = http::HeaderMap::new();
    for (name, value) in reply["headers"]
        .as_object()
        .expect("the refusal states its headers")
    {
        headers.insert(
            http::HeaderName::from_bytes(name.as_bytes()).expect("a header name"),
            http::HeaderValue::from_str(value.as_str().expect("a header value is text"))
                .expect("a header value"),
        );
    }
    let status = reply["status"]
        .as_u64()
        .and_then(|status| u16::try_from(status).ok())
        .and_then(|status| http::StatusCode::from_u16(status).ok())
        .expect("the refusal states its status");
    codex_api::map_api_error(codex_api::ApiError::Transport(
        codex_api::TransportError::Http {
            status,
            url: None,
            headers: Some(headers),
            body: Some(reply["body"].to_string()),
            retry_after: None,
        },
    ))
}

/// The usage limit an error is, to the code that handles a refused turn.
fn usage_limit(
    error: &codex_protocol::error::CodexErr,
) -> Option<&codex_protocol::error::UsageLimitReachedError> {
    match error.details() {
        codex_protocol::error::CodexErrorDetails::UsageLimitReached(details) => Some(details),
        _ => None,
    }
}

/// A golden refusal's error, and the reset its body names.
fn golden_refusal(name: &str) -> (codex_protocol::error::CodexErr, i64) {
    let reply = golden(name);
    let resets_at = reply["body"]["error"]["resets_at"]
        .as_i64()
        .expect("the golden refusal names its reset");
    (refused(&reply), resets_at)
}

fn window(used: f64, minutes: i64, resets_at: i64) -> codex_protocol::protocol::RateLimitWindow {
    codex_protocol::protocol::RateLimitWindow {
        used_percent: used,
        window_minutes: Some(minutes),
        resets_at: Some(resets_at),
    }
}

#[test]
fn the_fixtures_weekly_refusal_is_an_ordinary_usage_limit_with_a_spent_week() {
    use codex_protocol::auth::KnownPlan;
    use codex_protocol::auth::PlanType;
    use codex_protocol::protocol::RateLimitReachedType;
    let (error, resets_at) = golden_refusal("refusal-weekly-wall.json");
    let details = usage_limit(&error).expect("the weekly refusal is a usage limit to this client");
    assert_eq!(details.plan_type, Some(PlanType::Known(KnownPlan::Pro)));
    assert_eq!(
        details.resets_at.map(|at| at.timestamp()),
        Some(resets_at),
        "the body's reset"
    );
    assert_eq!(
        details.rate_limit_reached_type,
        Some(RateLimitReachedType::RateLimitReached)
    );
    assert!(
        ordinary_weekly_wall(details),
        "an ordinary wall, which an earned reset or another account can answer"
    );
    let snapshot = details
        .rate_limits
        .as_deref()
        .expect("the refusal's headers state the account's windows");
    assert_eq!(snapshot.limit_id.as_deref(), Some("codex"));
    // The week is the spent window, and its reset is the one the body names.
    assert_eq!(snapshot.secondary, Some(window(100.0, 10080, resets_at)));
    let session = snapshot.primary.as_ref().expect("the five-hour window");
    assert_eq!(
        (session.used_percent, session.window_minutes),
        (0.0, Some(300))
    );
    assert_eq!(response_used(snapshot), Some(100.0));
}

#[test]
fn the_fixtures_five_hour_refusal_states_a_spent_session_on_a_usable_week() {
    use codex_protocol::protocol::RateLimitReachedType;
    let (error, resets_at) = golden_refusal("refusal-five-hour-wall.json");
    let details =
        usage_limit(&error).expect("the five-hour refusal is a usage limit to this client");
    assert_eq!(details.resets_at.map(|at| at.timestamp()), Some(resets_at));
    assert_eq!(
        details.rate_limit_reached_type,
        Some(RateLimitReachedType::RateLimitReached)
    );
    let snapshot = details
        .rate_limits
        .as_deref()
        .expect("the refusal's headers state the account's windows");
    assert_eq!(snapshot.limit_id.as_deref(), Some("codex"));
    // The session is the spent window: ten minutes from the fixture's clock.
    assert_eq!(snapshot.primary, Some(window(100.0, 300, resets_at)));
    let week = snapshot.secondary.as_ref().expect("the weekly window");
    assert_eq!(
        (week.used_percent, week.window_minutes),
        (10.0, Some(10080))
    );
    assert!(week.resets_at.is_some_and(|at| at > resets_at));
    assert_eq!(response_used(snapshot), Some(100.0));
}

/// What `provider_fixture.py`'s `refusal` check refuses, and why: each of
/// these is a 429 the client reads as something other than the wall meant.
#[test]
fn a_refusal_the_fixtures_check_refuses_is_not_the_wall_it_meant() {
    let golden = golden("refusal-weekly-wall.json");
    assert!(usage_limit(&refused(&golden)).is_some());

    // A reset that is not an integer: the body no longer decodes as a usage error.
    let mut textual = golden.clone();
    textual["body"]["error"]["resets_at"] = "1800086400".into();
    assert!(
        usage_limit(&refused(&textual)).is_none(),
        "a textual resets_at decoded as a usage limit"
    );

    // Another error type: not a usage limit.
    let mut other = golden.clone();
    other["body"]["error"]["type"] = "rate_limit_exceeded".into();
    assert!(usage_limit(&refused(&other)).is_none());

    // A reached type the client does not know is dropped, not kept as stated.
    let mut unknown = golden.clone();
    unknown["headers"]["x-codex-rate-limit-reached-type"] = "usage_limit_reached".into();
    let error = refused(&unknown);
    let details = usage_limit(&error).expect("still a usage limit");
    assert_eq!(details.rate_limit_reached_type, None);

    // A limit named as reached whose windows are not sent: no window is read,
    // and the wall is not the ordinary one an earned reset can answer.
    let mut family = golden;
    family["headers"]["x-codex-active-limit"] = "codex_other".into();
    let error = refused(&family);
    let details = usage_limit(&error).expect("still a usage limit");
    let snapshot = details.rate_limits.as_deref().expect("a snapshot");
    assert_eq!(snapshot.limit_id.as_deref(), Some("codex_other"));
    assert_eq!((&snapshot.primary, &snapshot.secondary), (&None, &None));
    assert!(!ordinary_weekly_wall(details));
}
