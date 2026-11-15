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
//!
//! The files state every wall kind the provider names, and the tests below
//! say what this crate makes of each today. Gap 4 of
//! `alab/PARALLEL-LOGINS.md` is that no wall outside the ordinary `codex`
//! bucket's spent window ever moves to another account. Most of that gap lies
//! outside this crate, and a test says where it lies and which step of the
//! gap's plan closes it. A reading here that a step keeps is asserted as
//! right, not as the gap. Step S3 classifies each wall's bucket
//! (`wall_kind`, alab-core's one classifier) and defines
//! `ordinary_weekly_wall` from it with its meaning kept: false for a
//! workspace or a model's limit, and true for Reserve, which S3 classifies
//! as ordinary.

use super::*;
use crate::WallKind;
use codex_alab_core::quota::WallHandoff;
use codex_protocol::protocol::CreditsSnapshot;
use codex_protocol::protocol::RateLimitReachedType;
use codex_protocol::protocol::SpendControlLimitSnapshot;
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

/// A golden usage reply as the fixture serves it now: every reset it states
/// lies as far ahead of this moment as its `reset_after_seconds` says, or
/// `primary` seconds ahead for the ordinary bucket's five-hour window when
/// given. That covers the windows of each other limit the reply names and a
/// spend cap's reset. A window the reply leaves out stays out: a credit-only
/// plan's reply has none.
fn usage_now(name: &str, primary: Option<i64>) -> serde_json::Value {
    let mut body = golden(name);
    let now = chrono::Utc::now().timestamp();
    let limits = body
        .get("additional_rate_limits")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    let mut resets = vec![
        ("/rate_limit/primary_window".to_string(), primary),
        ("/rate_limit/secondary_window".to_string(), None),
        ("/spend_control/individual_limit".to_string(), None),
    ];
    for index in 0..limits {
        for window in ["primary_window", "secondary_window"] {
            resets.push((
                format!("/additional_rate_limits/{index}/rate_limit/{window}"),
                None,
            ));
        }
    }
    for (pointer, ahead) in resets {
        // Never through `IndexMut`, which would add the absent object as null.
        let Some(reset) = body.pointer_mut(&pointer).filter(|reset| !reset.is_null()) else {
            continue;
        };
        let stated = reset["reset_after_seconds"]
            .as_i64()
            .expect("the golden window states its reset distance");
        let ahead = ahead.unwrap_or(stated);
        reset["reset_after_seconds"] = ahead.into();
        reset["reset_at"] = (now + ahead).into();
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
    // The login is made before the wall is stated. Making it can take
    // seconds: it builds an HTTP client, and on macOS the first one a process
    // builds looks up the system proxy by reading the whole directory that
    // holds the test executable, hundreds of thousands of files in a shared
    // build directory. A reset stated before the login could pass before the
    // wall was read or offered.
    let login = Login::of(&golden("usage-five-hour-wall.json")).await;
    login
        .serve("GET", CREDITS, golden("credits-empty.json"))
        .await;
    // The golden wall resets in ten minutes; served here to reset `ahead`
    // seconds after `stated`, the moment the reply states, so only the read
    // below and the wait's first attempt must come before the reset.
    let ahead = 2;
    let usage = usage_now("usage-five-hour-wall.json", Some(ahead));
    let reset = usage["rate_limit"]["primary_window"]["reset_at"]
        .as_i64()
        .unwrap();
    let stated = reset - ahead;
    login.serve("GET", USAGE, usage).await;
    let read = login
        .client()
        .await
        .get_rate_limits_with_reset_credits()
        .await
        .expect("the fixture's five-hour wall does not decode");
    assert_eq!(read.ordinary_usage_allowed, Some(false));
    assert_eq!(ordinary_used(&read), Some(100.0));
    // What the reading names is judged at the moment it states, which no
    // delay in reaching this line moves.
    assert_eq!(
        spent_wall(&read, &login.owner, stated),
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
        "the host is asked whether it leaves the five-hour wall; it does not, \
         and the wall is waited out"
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

/// A golden usage reply as the fixture serves it now, what the client reads
/// of it, and the login it is for.
async fn read_now(name: &str) -> (Login, serde_json::Value, RateLimitsWithResetCredits) {
    let usage = usage_now(name, None);
    let login = Login::of(&usage).await;
    login.serve("GET", USAGE, usage.clone()).await;
    let read = login
        .client()
        .await
        .get_rate_limits_with_reset_credits()
        .await
        .unwrap_or_else(|error| panic!("the fixture's {name} does not decode: {error}"));
    assert!(
        matches(&read, &login.owner),
        "{name} names the login's own account and user"
    );
    (login, usage, read)
}

/// The reset a golden usage reply states at `pointer`, as served.
fn reset_at(usage: &serde_json::Value, pointer: &str) -> i64 {
    usage
        .pointer(pointer)
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_else(|| panic!("the golden states a reset at {pointer}"))
}

/// An ordinary five-hour wall five minutes from its reset, on a reading that
/// also names the ordinary reached type. `rate_limit_reached` is the ordinary
/// wall's own word, not a workspace's: the wall is the five-hour window's,
/// with its own reset, as it is on a reading without the word. A host that
/// took the word for a refusal no window explains would leave a wall that
/// resets in five minutes (gap 4's step S2 in aterm must not).
#[tokio::test]
async fn the_ordinary_reached_type_names_the_ordinary_wall() {
    let (login, usage, read) = read_now("usage-five-hour-reached.json").await;
    let reset = reset_at(&usage, "/rate_limit/primary_window/reset_at");
    assert_eq!(
        usage["rate_limit"]["primary_window"]["reset_after_seconds"],
        300
    );
    let [codex] = read.rate_limits.as_slice() else {
        panic!("one bucket: {:?}", read.rate_limits);
    };
    assert_eq!(
        codex.rate_limit_reached_type,
        Some(RateLimitReachedType::RateLimitReached)
    );
    assert_eq!(codex.spend_control_reached, None);
    assert!(!explicit_denial(&read), "the ordinary word denies nothing");
    let now = chrono::Utc::now().timestamp();
    let wall = SpentWall {
        window: SpentWindow::FiveHour,
        resets_at: reset,
    };
    assert_eq!(spent_wall(&read, &login.owner, now), Some(wall));

    // The same reading without the word names the same wall.
    let mut plain = usage;
    plain
        .as_object_mut()
        .unwrap()
        .remove("rate_limit_reached_type")
        .expect("the golden names the reached type");
    let login = Login::of(&plain).await;
    login.serve("GET", USAGE, plain).await;
    let read = login
        .client()
        .await
        .get_rate_limits_with_reset_credits()
        .await
        .unwrap();
    assert_eq!(read.rate_limits[0].rate_limit_reached_type, None);
    assert_eq!(spent_wall(&read, &login.owner, now), Some(wall));
}

/// A workspace's walls as a usage read states them: a member's spend cap
/// (`spend_control`, with the cap's own reset) and a workspace out of
/// credits (its reached type). Neither is a window's: each reading refuses
/// ordinary use with every window under 100%. Both are explicit denials here,
/// so no earned reset is redeemed and the in-turn recovery names no wall to a
/// host; their 429 never enters it anyway
/// (`the_fixtures_workspace_refusals_are_not_ordinary_walls`). The move is the
/// host's, to an account outside the workspace, and today aterm answers
/// `not-walled` because no window is at 100% (gap 4, step S2, in aterm).
#[tokio::test]
async fn the_fixtures_workspace_walls_are_explicit_denials_with_no_spent_window() {
    use codex_protocol::account::PlanType;
    let credits = Some(RateLimitReachedType::WorkspaceMemberCreditsDepleted);
    for (name, spend, reached) in [
        ("usage-spend-control.json", Some(true), None),
        ("usage-workspace-credits.json", None, credits),
    ] {
        let (login, usage, read) = read_now(name).await;
        assert_eq!(read.ordinary_usage_allowed, Some(false), "{name}");
        let [codex] = read.rate_limits.as_slice() else {
            panic!("{name}: one bucket: {:?}", read.rate_limits);
        };
        assert_eq!(codex.limit_id.as_deref(), Some("codex"), "{name}");
        assert_eq!(codex.plan_type, Some(PlanType::Team), "{name}");
        assert_eq!(codex.spend_control_reached, spend, "{name}");
        assert_eq!(codex.rate_limit_reached_type, reached, "{name}");
        let cap = spend.map(|_| SpendControlLimitSnapshot {
            limit: "25000".to_string(),
            used: "25000".to_string(),
            remaining_percent: 0,
            resets_at: reset_at(&usage, "/spend_control/individual_limit/reset_at"),
        });
        assert_eq!(codex.individual_limit, cap, "{name}");
        assert_eq!(
            ordinary_used(&read),
            Some(20.0),
            "{name}: every window has room"
        );
        assert!(explicit_denial(&read), "{name}");
        let now = chrono::Utc::now().timestamp();
        assert_eq!(spent_wall(&read, &login.owner, now), None, "{name}");
        assert_eq!(redemption_cycle(&read, &login.owner, now), None, "{name}");
        assert!(!stale_exhaustion(&read, &login.owner, now), "{name}");
        assert!(!usage_below(&read, &login.owner, 100.0), "{name}");
    }
}

/// A credit-only plan out of credits: its `codex` bucket refuses ordinary use
/// and has no window (`usage_now` serves it as it is), and its credits say
/// none are left. Every reader here judges windows, so none names a wall: the
/// in-turn recovery stops at its first read and asks no host, which is right,
/// since no reset of this account ends the wall. The move is the host's, asked
/// once the turn has ended; today aterm answers `not-walled`, because no window
/// is at 100% (gap 4, step S2, in aterm).
#[tokio::test]
async fn the_fixtures_credit_only_plan_states_credits_and_no_window() {
    let (login, usage, read) = read_now("usage-credit-only.json").await;
    assert_eq!(usage["rate_limit"].get("primary_window"), None);
    assert_eq!(read.ordinary_usage_allowed, Some(false));
    let [codex] = read.rate_limits.as_slice() else {
        panic!("one bucket: {:?}", read.rate_limits);
    };
    assert_eq!(codex.limit_id.as_deref(), Some("codex"));
    assert_eq!((&codex.primary, &codex.secondary), (&None, &None));
    assert_eq!(
        codex.credits,
        Some(CreditsSnapshot {
            has_credits: false,
            unlimited: false,
            balance: Some("0".to_string()),
        })
    );
    assert_eq!(
        (codex.spend_control_reached, codex.rate_limit_reached_type),
        (None, None)
    );
    let now = chrono::Utc::now().timestamp();
    assert!(!explicit_denial(&read));
    assert_eq!(ordinary_used(&read), None);
    assert!(!usage_below(&read, &login.owner, 100.0));
    assert_eq!(redemption_cycle(&read, &login.owner, now), None);
    assert_eq!(spent_wall(&read, &login.owner, now), None);

    let (handoff, asked) = ready_host();
    let read_before = login.received("GET", USAGE).await;
    let recovered = tokio::time::timeout(
        Duration::from_secs(10),
        wait_for_reset(
            &login.manager,
            &login.config,
            &login.owner,
            "turn",
            Duration::from_secs(3600),
            /*reset_at*/ None,
            Some(&handoff),
        ),
    )
    .await
    .expect("a credit-only wall kept the walled turn polling");
    assert!(!recovered, "the turn ends on its usage error");
    assert_eq!(
        (
            asked.week.load(Ordering::SeqCst),
            asked.five_hour.load(Ordering::SeqCst)
        ),
        (0, 0),
        "no window names a wall to offer"
    );
    assert_eq!(
        login.received("GET", USAGE).await,
        read_before + 1,
        "the recovery stops at its first read"
    );
    assert_eq!(
        login.received("GET", CREDITS).await,
        0,
        "no window is near a reset an earned credit could bring forward"
    );
    assert_eq!(login.received("POST", CONSUME).await, 0);
}

/// A model's own limit, spent beside an ordinary bucket with room. The client
/// reads it as a second snapshot whose limit id is the reply's
/// `metered_feature` and keeps its windows, not its verdict
/// (`allowed: false`). Every reader here judges the `codex` bucket alone, so
/// the account reads as permitting a turn and no wall is named. That is right
/// for the ordinary bucket. What gap 4 lacks is a judgement per limit: aterm
/// reads `additional_rate_limits` in step S4, and codex-alab names the wall's
/// limit to it in S5.
#[tokio::test]
async fn the_fixtures_model_limit_is_a_second_bucket_the_ordinary_readers_pass_over() {
    let (login, usage, read) = read_now("usage-model-limit.json").await;
    let other = "/additional_rate_limits/0/rate_limit";
    assert_eq!(read.ordinary_usage_allowed, Some(true));
    let [codex, model] = read.rate_limits.as_slice() else {
        panic!("two buckets: {:?}", read.rate_limits);
    };
    assert_eq!(
        (codex.limit_id.as_deref(), codex.limit_name.as_deref()),
        (Some("codex"), None)
    );
    let session = codex
        .primary
        .as_ref()
        .expect("the ordinary five-hour window");
    assert_eq!(
        (session.used_percent, session.window_minutes),
        (20.0, Some(300))
    );
    assert_eq!(
        model.limit_id.as_deref(),
        usage
            .pointer("/additional_rate_limits/0/metered_feature")
            .and_then(serde_json::Value::as_str),
        "the limit id is the reply's metered_feature"
    );
    assert_eq!(model.limit_id.as_deref(), Some("codex_other"));
    assert_eq!(model.limit_name.as_deref(), Some("codex_other"));
    assert_eq!(model.normal_model_slug, None);
    assert_eq!(
        model.primary,
        Some(window(
            100.0,
            300,
            reset_at(&usage, &format!("{other}/primary_window/reset_at"))
        ))
    );
    assert_eq!(
        model.secondary,
        Some(window(
            40.0,
            10080,
            reset_at(&usage, &format!("{other}/secondary_window/reset_at"))
        ))
    );
    assert_eq!((&model.credits, &model.individual_limit), (&None, &None));
    assert_eq!(
        (model.spend_control_reached, model.rate_limit_reached_type),
        (None, None)
    );
    assert_eq!(model.plan_type, codex.plan_type);
    let now = chrono::Utc::now().timestamp();
    assert!(!explicit_denial(&read));
    assert_eq!(ordinary_used(&read), Some(20.0));
    assert!(
        usage_below(&read, &login.owner, 95.0),
        "the ordinary bucket permits a turn, through S5 too; gap 4's per-limit read is aterm's S4"
    );
    assert_eq!(spent_wall(&read, &login.owner, now), None);
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

/// The refusals of a usage limit among the golden files.
const LIMIT_REFUSALS: [&str; 7] = [
    "refusal-weekly-wall.json",
    "refusal-five-hour-wall.json",
    "refusal-workspace-credits.json",
    "refusal-spend-cap.json",
    "refusal-credit-only.json",
    "refusal-model-limit.json",
    "refusal-reserve.json",
];

/// Each golden refusal's bucket, as alab-core's one classifier reads it
/// through codex-api's decoding (gap 4, step S3), and whether an earned reset
/// may answer it (`ordinary_weekly_wall`, defined from the kind). The weekly,
/// five-hour and credit-only walls are the ordinary bucket's, and so is
/// Reserve, by its name. A workspace's reached type is the workspace's wall
/// although the 429 names the `codex` bucket. The model's limit is its own
/// bucket, by the limit id the 429's active-limit header names.
#[test]
fn the_fixtures_refusals_each_name_their_walls_kind() {
    use codex_alab_core::wall_kind::Workspace;
    for (name, kind) in [
        ("refusal-weekly-wall.json", WallKind::Ordinary),
        ("refusal-five-hour-wall.json", WallKind::Ordinary),
        (
            "refusal-workspace-credits.json",
            WallKind::Workspace(Workspace::MemberCreditsDepleted),
        ),
        (
            "refusal-spend-cap.json",
            WallKind::Workspace(Workspace::MemberUsageLimitReached),
        ),
        ("refusal-credit-only.json", WallKind::Ordinary),
        (
            "refusal-model-limit.json",
            WallKind::Limit("codex_other".into()),
        ),
        ("refusal-reserve.json", WallKind::Ordinary),
    ] {
        assert!(LIMIT_REFUSALS.contains(&name), "{name}");
        let error = refused(&golden(name));
        let details = usage_limit(&error).unwrap_or_else(|| panic!("{name} is a usage limit"));
        assert_eq!(crate::wall_kind(details), kind, "{name}");
        assert_eq!(
            ordinary_weekly_wall(details),
            kind == WallKind::Ordinary,
            "{name}"
        );
        assert_eq!(kind.asked(), name != "refusal-model-limit.json", "{name}");
        assert_eq!(
            kind.host_kept(),
            name.contains("workspace") || name.contains("spend")
        );
    }
    // The table names every golden refusal of a usage limit.
    assert_eq!(LIMIT_REFUSALS.len(), 7);
    // Each workspace word on the 429 is its own wall, owner's or member's.
    for (word, workspace) in [
        (
            "workspace_owner_credits_depleted",
            Workspace::OwnerCreditsDepleted,
        ),
        (
            "workspace_member_credits_depleted",
            Workspace::MemberCreditsDepleted,
        ),
        (
            "workspace_owner_usage_limit_reached",
            Workspace::OwnerUsageLimitReached,
        ),
        (
            "workspace_member_usage_limit_reached",
            Workspace::MemberUsageLimitReached,
        ),
    ] {
        let mut reply = golden("refusal-workspace-credits.json");
        reply["headers"]["x-codex-rate-limit-reached-type"] = word.into();
        let error = refused(&reply);
        let details = usage_limit(&error).expect("a usage limit");
        assert_eq!(
            crate::wall_kind(details),
            WallKind::Workspace(workspace),
            "{word}"
        );
    }
}

/// No 429 states spend control: codex-api's header parse leaves it unread,
/// whatever the wall. A spend cap reaches this client's refused turn as a
/// workspace reached type alone; its usage read is where `spend_control`
/// shows (`the_fixtures_workspace_walls_are_explicit_denials_with_no_spent_window`).
#[test]
fn no_golden_refusal_states_spend_control() {
    for name in LIMIT_REFUSALS {
        let error = refused(&golden(name));
        let details = usage_limit(&error).unwrap_or_else(|| panic!("{name} is a usage limit"));
        let snapshot = details
            .rate_limits
            .as_deref()
            .unwrap_or_else(|| panic!("{name} states a snapshot"));
        assert_eq!(
            (snapshot.spend_control_reached, &snapshot.individual_limit),
            (None, &None),
            "{name}"
        );
    }
}

/// A workspace's walls as their 429 states them: out of credits (no reset)
/// and a member's spend cap (the cap's own far reset). The client keeps the
/// reached type, on the error and on its snapshot of the `codex` family,
/// whose windows have room. None is an ordinary wall, which is right for an
/// earned reset: none can answer it, and step S3 keeps that reading. Each is
/// a workspace's wall (`the_fixtures_refusals_each_name_their_walls_kind`),
/// which exec keeps since S3 only while a host may move it
/// (`alab-core/src/exec_wall.rs`; the glue's `reset_delay` table in
/// `alab-core/glue/responses_quota_retry.rs`), publishing its observation by
/// the wall's kind. Interactively the turn ends at once
/// (`responses_retry.rs`) and the host is asked, which needs no change here.
#[test]
fn the_fixtures_workspace_refusals_are_not_ordinary_walls() {
    use RateLimitReachedType::WorkspaceMemberCreditsDepleted;
    use RateLimitReachedType::WorkspaceMemberUsageLimitReached;
    let cap = reset_at(
        &golden("usage-spend-control.json"),
        "/spend_control/individual_limit/reset_at",
    );
    for (name, reached, resets_at, says) in [
        (
            "refusal-workspace-credits.json",
            WorkspaceMemberCreditsDepleted,
            None,
            "Your workspace is out of credits.",
        ),
        (
            "refusal-spend-cap.json",
            WorkspaceMemberUsageLimitReached,
            Some(cap),
            "You hit your spend cap set by the owner of your workspace.",
        ),
    ] {
        let error = refused(&golden(name));
        let details = usage_limit(&error).unwrap_or_else(|| panic!("{name} is a usage limit"));
        assert_eq!(details.rate_limit_reached_type, Some(reached), "{name}");
        assert_eq!(
            details.resets_at.map(|at| at.timestamp()),
            resets_at,
            "{name}"
        );
        assert!(details.to_string().starts_with(says), "{name}: {details}");
        let snapshot = details.rate_limits.as_deref().expect("a snapshot");
        assert_eq!(snapshot.limit_id.as_deref(), Some("codex"), "{name}");
        assert_eq!(snapshot.rate_limit_reached_type, Some(reached), "{name}");
        assert_eq!(
            response_used(snapshot),
            Some(20.0),
            "{name}: no window explains the wall"
        );
        assert!(
            !ordinary_weekly_wall(details),
            "{name}: not ordinary; exec keeps it only while a host may move it (S3)"
        );
    }
    // Each workspace word reads the same way, owner's or member's.
    for (word, reached) in [
        (
            "workspace_owner_credits_depleted",
            RateLimitReachedType::WorkspaceOwnerCreditsDepleted,
        ),
        (
            "workspace_member_credits_depleted",
            WorkspaceMemberCreditsDepleted,
        ),
        (
            "workspace_owner_usage_limit_reached",
            RateLimitReachedType::WorkspaceOwnerUsageLimitReached,
        ),
        (
            "workspace_member_usage_limit_reached",
            WorkspaceMemberUsageLimitReached,
        ),
    ] {
        let mut reply = golden("refusal-workspace-credits.json");
        reply["headers"]["x-codex-rate-limit-reached-type"] = word.into();
        let error = refused(&reply);
        let details = usage_limit(&error).expect("a usage limit");
        assert_eq!(details.rate_limit_reached_type, Some(reached), "{word}");
        assert!(!ordinary_weekly_wall(details), "{word}");
    }
}

/// A credit-only plan's 429: the ordinary reached type on the `codex` family,
/// which states the balance and no window, and no reset. It is an ordinary
/// wall: the turn's recovery reads the account and stops
/// (`the_fixtures_credit_only_plan_states_credits_and_no_window`), and exec
/// waits with its observation published. Only the host's answer is missing
/// (gap 4, step S2, in aterm); nothing here changes for it.
#[test]
fn the_fixtures_credit_only_refusal_is_an_ordinary_wall_with_no_window() {
    let error = refused(&golden("refusal-credit-only.json"));
    let details = usage_limit(&error).expect("the credit-only refusal is a usage limit");
    assert_eq!(details.resets_at, None);
    assert_eq!(
        details.rate_limit_reached_type,
        Some(RateLimitReachedType::RateLimitReached)
    );
    let snapshot = details.rate_limits.as_deref().expect("a snapshot");
    assert_eq!(snapshot.limit_id.as_deref(), Some("codex"));
    assert_eq!((&snapshot.primary, &snapshot.secondary), (&None, &None));
    assert_eq!(
        snapshot.credits,
        Some(CreditsSnapshot {
            has_credits: false,
            unlimited: false,
            balance: Some("0".to_string()),
        })
    );
    assert_eq!(response_used(snapshot), None);
    assert!(ordinary_weekly_wall(details));
}

/// A model's own limit, as its 429 states it: `x-codex-active-limit` names
/// `codex_other`, and the client reads that family's windows alone. This is
/// the case where they are sent, beside the one above where they are not.
/// The `codex` family the same 429 states, with room, is not read, and the
/// reset is the named limit's spent window's. Upstream's copy tells the person
/// to switch model; ALab keeps the model and moves the account, or waits.
/// It is not an ordinary wall, which is right for an earned reset, and step
/// S3 keeps that reading. Since S3, exec publishes its observation by the
/// wall's kind, a model's own limit included, and its runtime asks no host
/// about one (`WallKind::asked`): a host asked today judges the ordinary
/// bucket. The run waits out the reset, as before, until S5 names the limit
/// to a host that judges it.
#[test]
fn the_fixtures_model_limit_refusal_states_the_named_limits_windows() {
    let reply = golden("refusal-model-limit.json");
    let (error, resets_at) = golden_refusal("refusal-model-limit.json");
    let details = usage_limit(&error).expect("the model-limit refusal is a usage limit");
    assert_eq!(details.resets_at.map(|at| at.timestamp()), Some(resets_at));
    assert_eq!(
        details.rate_limit_reached_type,
        Some(RateLimitReachedType::RateLimitReached)
    );
    assert!(
        details
            .to_string()
            .contains("usage limit for codex_other. Switch to another model"),
        "{details}"
    );
    let snapshot = details.rate_limits.as_deref().expect("a snapshot");
    assert_eq!(snapshot.limit_id.as_deref(), Some("codex_other"));
    assert_eq!(snapshot.limit_name.as_deref(), Some("codex_other"));
    assert_eq!(
        snapshot.rate_limit_reached_type,
        Some(RateLimitReachedType::RateLimitReached)
    );
    assert_eq!(snapshot.primary, Some(window(100.0, 300, resets_at)));
    let week = snapshot
        .secondary
        .as_ref()
        .expect("the limit's weekly window");
    assert_eq!(
        (week.used_percent, week.window_minutes),
        (40.0, Some(10080))
    );
    assert_eq!(reply["headers"]["x-codex-primary-used-percent"], "20");
    assert_eq!(response_used(snapshot), None, "not the ordinary bucket's");
    assert!(
        !ordinary_weekly_wall(details),
        "not ordinary: a model's own limit, which S5 names to a host"
    );
}

/// Reserve's 429: the ordinary bucket's week is spent and so is Reserve's own
/// limit (upstream's `base_model_inference`, named `gpt-reserve`), which the
/// 429 names as reached. Upstream treats Reserve as ordinary usage's
/// fallback, not as a model of its own: its copy is the ordinary one. Before
/// step S3, `ordinary_weekly_wall` judged by the limit id alone and read
/// Reserve as not ordinary, so exec's publish gate withheld the observation
/// for it too (gap 4). S3's classifier takes Reserve as ordinary, by its name
/// as upstream's copy does, and defines `ordinary_weekly_wall` from that
/// classifier: an earned reset may answer it, and exec asks its host about it
/// as about any ordinary wall. S5 never names Reserve to a host as a limit of
/// its own.
#[test]
fn the_fixtures_reserve_refusal_is_ordinary_usages_fallback() {
    let (error, resets_at) = golden_refusal("refusal-reserve.json");
    let details = usage_limit(&error).expect("the Reserve refusal is a usage limit");
    assert_eq!(details.resets_at.map(|at| at.timestamp()), Some(resets_at));
    let snapshot = details.rate_limits.as_deref().expect("a snapshot");
    assert_eq!(snapshot.limit_id.as_deref(), Some("base_model_inference"));
    assert_eq!(snapshot.limit_name.as_deref(), Some("gpt-reserve"));
    assert_eq!(snapshot.primary, Some(window(100.0, 10080, resets_at)));
    assert_eq!(snapshot.secondary, None);
    let says = details.to_string();
    assert!(
        says.starts_with("You’ve hit your usage limit.")
            && !says.contains("Switch to another model"),
        "{says}"
    );
    assert_eq!(response_used(snapshot), None);
    assert_eq!(crate::wall_kind(details), WallKind::Ordinary);
    assert!(
        ordinary_weekly_wall(details),
        "Reserve is ordinary usage's fallback (S3), not a limit of its own"
    );
}

/// A plan that does not include the usage. codex-api makes
/// `CodexErr::UsageNotIncluded` of it, which is no usage limit to anything
/// here; it reaches the Autopilot as `UsageLimitExceeded` with that detail,
/// the pair alab-autopilot's `classify` makes `Wall::Entitlement`
/// (`wake_boundary_tests::subscription_errors_retain_the_route_for_their_recovery`).
/// The Autopilot never asks a host to move an Entitlement wall. Whether a
/// provider ever states credit or workspace depletion this way, or as
/// `insufficient_quota`, is not known; gap 4 records the class as out of scope.
#[test]
fn the_fixtures_usage_not_included_refusal_is_an_entitlement_wall() {
    use codex_protocol::error::CodexErrorDetails;
    use codex_protocol::protocol::CodexErrorInfo;
    let error = refused(&golden("refusal-usage-not-included.json"));
    assert!(
        matches!(error.details(), CodexErrorDetails::UsageNotIncluded),
        "{error:?}"
    );
    assert!(usage_limit(&error).is_none());
    assert_eq!(
        error.to_codex_protocol_error(),
        CodexErrorInfo::UsageLimitExceeded
    );
}
