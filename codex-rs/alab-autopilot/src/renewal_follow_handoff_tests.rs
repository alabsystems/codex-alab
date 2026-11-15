// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE WALL HANDOFF A STARTED TURN CARRIES, against alab-aterm's in-process FAKE aterm (never
//! a real one) and codex's own `AuthManager` over a scratch home's fake ChatGPT login: quota
//! recovery ends a turn at a creditless usage wall only while account rotation would follow
//! it, so the handoff's answer must be the gates `follow_renewal` applies once the turn has
//! ended, and the host's account pool must show somewhere to go: a host whose policy allows
//! rotation over a pool with no other account answers `waiting` to the ended turn, which no
//! longer polls for an earned reset. The answer is for the wall it is asked about: a spent
//! week under the rotation policy alone, a spent five-hour window only under the bound the
//! host advertises (`codex_rotation_five_hour`) and `limit_wait`. Each refusal is read while
//! codex-alab acts under a policy before and after the question, so it is the handoff's own
//! and not a claim in motion.

use super::*;
use crate::login::fixtures;
use codex_alab_aterm::account_recovery::FIVE_HOUR_HANDOFF;
use codex_alab_core::account_handoff::Purpose;
use codex_alab_core::quota::SpentWall;
use codex_alab_core::quota::SpentWindow;
use codex_alab_core::quota::WallHandoff;
use codex_alab_core::session::AccountHandoff;
use codex_alab_core::session::NativePhase;
use codex_alab_core::session::OwedAct;
use codex_login::AuthCredentialsStoreMode;

/// The host policy under which a weekly wall rotates accounts (`relaunch` and `resume_limits`
/// are the fake's defaults).
const ROTATION: &[(&str, &str)] = &[("codex_rotation", "1"), ("accounts", "wall")];

/// The moment `seconds` from now, as a usage read names a reset (seconds since the epoch).
fn resets_in(seconds: u64) -> i64 {
    i64::try_from(now_ms() / 1000 + seconds).unwrap_or(i64::MAX)
}

/// A spent week, three days from its reset.
fn week() -> SpentWall {
    SpentWall {
        window: SpentWindow::Week,
        resets_at: resets_in(3 * 24 * 3600),
    }
}

/// A spent five-hour window `seconds` from its reset now. The handoff measures the time
/// left when it is asked, so the wall is nearer its reset at each later question.
fn five_hours(seconds: u64) -> SpentWall {
    SpentWall {
        window: SpentWindow::FiveHour,
        resets_at: resets_in(seconds),
    }
}

/// One row of aterm's `codex-accounts` snapshot, as the wire carries it.
fn row(id: &str, condition: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id, "label": id, "masked_email": null, "condition": condition,
        "enrolled": condition != "not-enrolled", "held": false, "eligible": false,
        "quota": {"stale": true, "observed_at": null, "five_hour_used_percent": null,
            "five_hour_reset_at": null, "weekly_used_percent": null, "weekly_reset_at": null},
    })
}

/// This process's kernel birth, as aterm binds an account snapshot to its asker.
fn birth() -> String {
    codex_alab_tui::identity::current()
        .ok()
        .and_then(|identity| codex_alab_tui::identity::proc_start_utc(&identity))
        .unwrap_or_else(|| panic!("this process has no readable birth"))
}

/// The fake aterm shows this process a pool of `rows`, naming `own` as its session's row in
/// a schema 2 snapshot (schema 1 names none).
fn show_pool(h: &Harness, own: Option<&str>, rows: &[serde_json::Value]) {
    h.fake
        .set_account_pool(Some((&birth(), own, serde_json::json!(rows))));
}

/// A configuration whose model requests use the stored OpenAI login.
async fn openai_config(home: &tempfile::TempDir) -> Arc<codex_core::config::Config> {
    let mut config = core_test_support::load_default_config_for_test(home).await;
    config.model_provider.name = "OpenAI".into();
    config.model_provider.requires_openai_auth = true;
    config.model_provider.env_key = None;
    config.model_provider.experimental_bearer_token = None;
    config.model_provider.auth = None;
    config.model_provider.aws = None;
    Arc::new(config)
}

/// Core accepts `turn` on the thread `store` holds; the handoff its store then carries.
async fn begun(autopilot: &Autopilot, store: &ExtensionData, turn: &str) -> Arc<WallHandoff> {
    begin(autopilot, store, turn).await.1
}

/// [`begun`], with the turn's own store, which the turn's error and its stop are read from.
async fn begin(
    autopilot: &Autopilot,
    store: &ExtensionData,
    turn: &str,
) -> (ExtensionData, Arc<WallHandoff>) {
    use codex_extension_api::TurnLifecycleContributor;
    use codex_extension_api::TurnStartInput;
    use codex_protocol::config_types::CollaborationMode;
    use codex_protocol::config_types::ModeKind;
    use codex_protocol::config_types::Settings;
    let mode = CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "gpt-5".to_string(),
            reasoning_effort: None,
            developer_instructions: None,
        },
    };
    let turn = ExtensionData::new(turn);
    assert!(
        turn.get::<WallHandoff>().is_none(),
        "only a started turn carries the handoff"
    );
    autopilot
        .on_turn_start(TurnStartInput {
            turn_id: turn.level_id(),
            collaboration_mode: &mode,
            token_usage_at_turn_start: None,
            session_store: &ExtensionData::new("session"),
            thread_store: store,
            turn_store: &turn,
        })
        .await;
    let handoff = turn
        .get::<WallHandoff>()
        .unwrap_or_else(|| panic!("the started turn carries no wall handoff"));
    (turn, handoff)
}

/// One look of a walled turn's recovery that comes to its handoff question, as quota
/// recovery makes it: the host hears the look begin ([`WallHandoff::look_begins`]), and is
/// then asked.
async fn looks_and_asks(handoff: &WallHandoff, wall: SpentWall) -> bool {
    handoff.look_begins();
    handoff.ready(wall).await
}

/// The handoff's answer for a spent week while codex-alab acts under a policy before and
/// after it.
async fn settled(h: &Harness, handoff: &WallHandoff) -> bool {
    settled_at(h, handoff, week()).await
}

/// The handoff's answer for `wall` while codex-alab acts under a policy before and after it.
async fn settled_at(h: &Harness, handoff: &WallHandoff, wall: SpentWall) -> bool {
    let acting = || h.sup.acting() && h.sup.policy().is_some();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        if acting() {
            let ready = handoff.ready(wall).await;
            if acting() {
                return ready;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "never acting under a policy: {:#?}",
            h.sup.journal()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Wait (a hang detector) until the handoff says rotation would follow a spent week.
async fn ready(h: &Harness, handoff: &WallHandoff, what: &str) {
    ready_at(h, handoff, week(), what).await;
}

/// Wait (a hang detector) until the handoff says rotation would follow `wall`. A hang says
/// how many answers it read and the slowest, so a slow host is told from a refusing one.
async fn ready_at(h: &Harness, handoff: &WallHandoff, wall: SpentWall, what: &str) {
    let started = tokio::time::Instant::now();
    let deadline = started + Duration::from_secs(60);
    let (mut answers, mut slowest) = (0_u32, Duration::ZERO);
    loop {
        let asked = tokio::time::Instant::now();
        let ready = settled_at(h, handoff, wall).await;
        answers += 1;
        slowest = slowest.max(asked.elapsed());
        if ready {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "never ready: {what}: {answers} answers in {:?}, the slowest {slowest:?}: {:#?}",
            started.elapsed(),
            h.sup.journal()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_started_turn_carries_a_wall_handoff_that_follows_rotation_policy() {
    let home = fixtures::home();
    fixtures::chatgpt(home.path(), "user", "account", "refresh", None);
    let manager = fixtures::manager(home.path(), AuthCredentialsStoreMode::File).await;
    let config = openai_config(&home).await;
    let h = started_timed_with_config(
        ROTATION,
        |_, _| {},
        true,
        ClientOptions::default(),
        Timing::default(),
        Some(Arc::clone(&config)),
    )
    .await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    let handoff = begun(&h.autopilot, &h._store, "walled-turn").await;

    // No ChatGPT identity is known: rotation has no source account to prepare from.
    assert!(!settled(&h, &handoff).await, "no login was read yet");
    h.autopilot.set_auth_manager(Arc::clone(&manager));

    // Policy alone never hands off: a host that shows no account pool has nowhere to move
    // the conversation that this process can see.
    assert!(
        !settled(&h, &handoff).await,
        "a host that shows no account pool hands off"
    );
    // The host names no row as this session's: two able rows are another account,
    // whichever is the session's own.
    let two = [row("a", "usable"), row("b", "unseen")];
    show_pool(&h, None, &two);
    ready(
        &h,
        &handoff,
        "an acting primary thread under rotation policy with another account",
    )
    .await;

    // Every pool below would leave the ended turn asking a host that has no account to
    // move it to, with nothing polling for an earned reset. Only the pool changed since the
    // handoff was ready, so each refusal is the pool's.
    show_pool(&h, None, &[]);
    assert!(!settled(&h, &handoff).await, "an empty pool hands off");
    show_pool(&h, None, &[row("only", "unseen")]);
    assert!(!settled(&h, &handoff).await, "a one-account pool hands off");
    for spent in ["limited", "low", "signed-out", "not-enrolled"] {
        show_pool(&h, None, &[row("a", "usable"), row("b", spent)]);
        assert!(
            !settled(&h, &handoff).await,
            "a pool whose other account is {spent} hands off"
        );
    }
    h.fake
        .set_account_pool(Some(("another process", None, serde_json::json!(two))));
    assert!(
        !settled(&h, &handoff).await,
        "a pool read for another process hands off"
    );
    // The host names this session's row: its own never counts, one other is enough.
    show_pool(&h, Some("a"), &[row("a", "usable"), row("b", "limited")]);
    assert!(
        !settled(&h, &handoff).await,
        "a pool whose only able account is the session's own hands off"
    );
    show_pool(&h, Some("a"), &[row("a", "limited"), row("b", "unseen")]);
    ready(&h, &handoff, "one able account beside the session's own").await;
    // An aterm before schema 2 names no row: the one spare may be the session's own, so the
    // same pool hands off nothing there, and asking schema 2 again restores it.
    h.fake.answer_account_schema_two(false);
    assert!(
        !settled(&h, &handoff).await,
        "one spare account on an aterm that names no row hands off"
    );
    show_pool(&h, None, &two);
    ready(&h, &handoff, "two able rows on an aterm before schema 2").await;
    h.fake.answer_account_schema_two(true);
    show_pool(&h, Some("a"), &[row("a", "limited"), row("b", "unseen")]);
    ready(&h, &handoff, "one spare account named again by schema 2").await;
    h.fake.set_account_pool(None);
    assert!(
        !settled(&h, &handoff).await,
        "a host that stopped showing its pool hands off"
    );
    show_pool(&h, None, &two);
    ready(&h, &handoff, "the two-account pool again").await;

    // Each key the host's rotation needs; the handoff reads the policy fresh.
    for (key, off, on) in [
        ("codex_rotation", "0", "1"),
        ("accounts", "off", "wall"),
        ("relaunch", "false", "true"),
        ("resume_limits", "false", "true"),
    ] {
        h.fake.set_policy_key(key, off);
        assert!(!settled(&h, &handoff).await, "{key}={off} still hands off");
        h.fake.set_policy_key(key, on);
        ready(&h, &handoff, key).await;
    }
    h.fake.set_policy_key("accounts", "early");
    ready(&h, &handoff, "accounts=early").await;

    // The upgrade's exec is under way: no act follows a turn's end.
    h.autopilot.set_upgrade_requested(true);
    assert!(!settled(&h, &handoff).await, "an upgrade is replacing us");
    h.autopilot.set_upgrade_requested(false);
    ready(&h, &handoff, "the upgrade request withdrawn").await;

    // On standby under a kept claim nothing would follow the turn's end. The policy reads on
    // again with no event, so only the standby itself refuses here; the passive recheck may
    // end it at any time, and an answer after that proves nothing.
    h.fake.set_effective(false, "suspended");
    wait_until("passive", || !h.sup.acting()).await;
    assert!(!handoff.ready(week()).await, "effective=off hands off");
    h.fake.set_effective_quietly(true, "enabled");
    let answer = handoff.ready(week()).await;
    if !h.sup.acting() {
        assert!(!answer, "a standby hands off");
    }
    act_again(&h).await;
    ready(&h, &handoff, "acting again").await;

    // Another root thread the TUI shows is not the one aterm's rotation restarts.
    let other = ThreadId::new();
    let other_key = other.to_string();
    let other_store = ExtensionData::new(other_key.clone());
    other_store.insert(ThreadRt::new(
        other,
        Some(Arc::clone(&config)),
        false,
        Duration::ZERO,
    ));
    let other_rt = other_store
        .get::<ThreadRt>()
        .unwrap_or_else(|| panic!("inserted"));
    h.autopilot.keep_thread(&other_rt);
    h.shared.observe(
        &Frame {
            primary_thread: Some(h.rt.key()),
            displayed_thread: Some(&other_key),
            rollout_path: None,
            cwd: std::path::Path::new("/work"),
            model: "gpt-5",
            codex_home: home.path(),
            embedded: true,
            state: ProgramState::Idle,
            draft_empty: true,
            queued_input: false,
            popup_open: false,
            editor_open: false,
            pending_user_turn: false,
        },
        now_ms(),
    );
    h.shared.note_root_thread(&other_key);
    assert!(h.shared.is_registered(&other_key));
    let other_handoff = begun(&h.autopilot, &other_store, "another-root's-turn").await;
    assert!(
        !settled(&h, &other_handoff).await,
        "a non-primary thread hands off"
    );
    ready(&h, &handoff, "the primary thread beside another root").await;

    // The store's handoff keeps neither the thread nor the autopilot alive, and a thread that
    // is gone has nothing to hand off.
    drop(other_rt);
    drop(other_store);
    assert!(
        !other_handoff.ready(week()).await,
        "a dropped thread hands off"
    );

    Autopilot::cancel_timer(&h.rt).await;
}

/// A FIVE-HOUR WALL IS HANDED OFF ONLY TO A HOST THAT SAYS IT LEAVES ONE. aterm prints the
/// bound it applies (`codex_rotation_five_hour=<seconds>`) while its rotation is on; a host
/// without the word answers `not-needed` at every five-hour wall, and ending a turn for that
/// gives up its poll for an earned reset. Each refusal below stands beside a spent week that
/// the same host, pool and policy would leave, so it is the five-hour rule's own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_five_hour_wall_hands_off_only_under_the_hosts_advertised_bound() {
    let home = fixtures::home();
    fixtures::chatgpt(home.path(), "user", "account", "refresh", None);
    let manager = fixtures::manager(home.path(), AuthCredentialsStoreMode::File).await;
    let config = openai_config(&home).await;
    let h = started_timed_with_config(
        ROTATION,
        |_, _| {},
        true,
        ClientOptions::default(),
        Timing::default(),
        Some(Arc::clone(&config)),
    )
    .await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    let handoff = begun(&h.autopilot, &h._store, "walled-turn").await;
    h.autopilot.set_auth_manager(Arc::clone(&manager));
    let two = [row("a", "usable"), row("b", "unseen")];
    show_pool(&h, None, &two);
    let two_hours = five_hours(2 * 3600);

    // The fake's policy prints neither the word nor `limit_wait`: an aterm before the word.
    ready(&h, &handoff, "a spent week on a host without the word").await;
    assert!(
        !settled_at(&h, &handoff, two_hours).await,
        "a host that prints no five-hour word hands off a five-hour wall"
    );
    h.fake.set_policy_key("limit_wait", "true");
    ready(&h, &handoff, "a spent week with limit_wait on").await;
    assert!(
        !settled_at(&h, &handoff, two_hours).await,
        "limit_wait alone hands off a five-hour wall"
    );

    // The word: the wall must be further from its reset than the bound and a minute.
    h.fake.set_policy_key(FIVE_HOUR_HANDOFF, "900");
    ready_at(
        &h,
        &handoff,
        two_hours,
        "the word, two hours from the reset",
    )
    .await;
    assert!(
        !settled_at(&h, &handoff, five_hours(600)).await,
        "ten minutes from its reset hands off"
    );
    assert!(
        !settled_at(&h, &handoff, five_hours(960)).await,
        "the bound and exactly its skew hands off"
    );
    // The exact second is `leaves_wall`'s own table (alab-aterm); against the real clock
    // this wall stays clear of the bound and its skew for as long as the wait may take.
    ready_at(
        &h,
        &handoff,
        five_hours(960 + 300),
        "five minutes past the bound and its skew",
    )
    .await;
    // The host's number is followed.
    h.fake.set_policy_key(FIVE_HOUR_HANDOFF, "7200");
    ready(&h, &handoff, "a spent week under a two-hour bound").await;
    assert!(
        !settled_at(&h, &handoff, two_hours).await,
        "a wall inside the host's own bound hands off"
    );
    h.fake.set_policy_key(FIVE_HOUR_HANDOFF, "900");
    ready_at(&h, &handoff, two_hours, "the 900 s bound again").await;

    // Without `limit_wait` a declined five-hour turn would raise attention at once.
    h.fake.set_policy_key("limit_wait", "false");
    ready(&h, &handoff, "a spent week with limit_wait off").await;
    assert!(
        !settled_at(&h, &handoff, two_hours).await,
        "limit_wait=false hands off a five-hour wall"
    );
    h.fake.set_policy_key("limit_wait", "true");
    ready_at(&h, &handoff, two_hours, "limit_wait on again").await;

    // The pool and the rotation policy gate a five-hour wall as they gate a spent week.
    show_pool(&h, None, &[row("only", "unseen")]);
    assert!(
        !settled_at(&h, &handoff, two_hours).await,
        "a one-account pool hands off a five-hour wall"
    );
    show_pool(&h, None, &two);
    ready_at(&h, &handoff, two_hours, "the two-account pool again").await;
    h.fake.set_policy_key("codex_rotation", "0");
    assert!(
        !settled_at(&h, &handoff, two_hours).await,
        "codex_rotation=0 hands off a five-hour wall"
    );
    h.fake.set_policy_key("codex_rotation", "1");
    ready_at(&h, &handoff, two_hours, "rotation on again").await;

    // The word withdrawn (a blank value is no bound): only the week is left again.
    h.fake.set_policy_key(FIVE_HOUR_HANDOFF, "");
    ready(&h, &handoff, "a spent week once the word is gone").await;
    assert!(
        !settled_at(&h, &handoff, two_hours).await,
        "a blank five-hour word hands off"
    );

    Autopilot::cancel_timer(&h.rt).await;
}

/// Every `harness policy` read the fake host was sent.
fn policy_reads(h: &Harness) -> usize {
    h.fake
        .requests()
        .iter()
        .filter(|request| request.ends_with(" harness policy"))
        .count()
}

/// A HOST THAT PRINTS NO FIVE-HOUR WORD IS NOT CONTACTED ABOUT A FIVE-HOUR WALL (second
/// review, 2026-11-12). A creditless wall asks its handoff every 30 seconds for as long as it
/// stands, and each question read the host's policy afresh before the rule refused the wall:
/// one request per poll, for up to five hours, to every host there is today. The claim's
/// connection remembers the last policy reply it read; where that reply prints no bound the
/// question is refused with nothing sent. A spent week, and a five-hour wall on a host whose
/// remembered reply prints the bound, are still read afresh.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_five_hour_wall_is_refused_without_a_request_to_a_host_that_prints_no_word() {
    let home = fixtures::home();
    fixtures::chatgpt(home.path(), "user", "account", "refresh", None);
    let manager = fixtures::manager(home.path(), AuthCredentialsStoreMode::File).await;
    let config = openai_config(&home).await;
    let h = started_timed_with_config(
        ROTATION,
        |_, _| {},
        true,
        ClientOptions::default(),
        Timing::default(),
        Some(Arc::clone(&config)),
    )
    .await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    let handoff = begun(&h.autopilot, &h._store, "walled-turn").await;
    h.autopilot.set_auth_manager(Arc::clone(&manager));
    show_pool(&h, None, &[row("a", "usable"), row("b", "unseen")]);
    h.fake.set_policy_key("limit_wait", "true");
    let two_hours = five_hours(2 * 3600);
    let acting = || h.sup.acting() && h.sup.policy().is_some();

    // A spent week is handed off, so the claim, the login, the pool and the rotation
    // policy all stand, and the claim's connection remembers a reply with no word.
    ready(&h, &handoff, "a spent week on a host without the word").await;
    // POSITIVE CONTROL for the count: the week's question is a request to the host.
    let reads = policy_reads(&h);
    assert!(handoff.ready(week()).await && acting());
    assert!(
        policy_reads(&h) > reads,
        "a spent week is read afresh: {:#?}",
        h.fake.requests()
    );

    // The five-hour wall: refused, and the host hears nothing of it, poll after poll:
    // neither a policy read nor a look at its account pool.
    let asked = || {
        h.fake
            .requests()
            .into_iter()
            .filter(|request| {
                request.ends_with(" harness policy") || request.contains(" codex-accounts ")
            })
            .collect::<Vec<_>>()
    };
    let before = asked();
    for poll in 0..3 {
        assert!(
            !handoff.ready(two_hours).await,
            "a host that prints no five-hour word hands off a five-hour wall"
        );
        assert!(acting(), "the refusal was the claim's, not the rule's");
        assert_eq!(
            asked(),
            before,
            "poll {poll}: the refused five-hour wall sent the host a request"
        );
    }

    // The host prints the word (announced, so the supervisor reads it): from here the
    // remembered reply carries the bound and each question reads the host afresh.
    h.fake.set_policy_key(FIVE_HOUR_HANDOFF, "900");
    ready_at(
        &h,
        &handoff,
        two_hours,
        "the word, two hours from the reset",
    )
    .await;
    let reads = policy_reads(&h);
    assert!(handoff.ready(two_hours).await && acting());
    assert!(
        policy_reads(&h) > reads,
        "a host that prints the word is read afresh: {:#?}",
        h.fake.requests()
    );
    // And the fresh reply decides, not the remembered one: the host stops answering its
    // policy without announcing it, and the wall the remembered reply would leave is refused.
    h.fake
        .answer_policy_with(Some("ERR harness policy unavailable"));
    assert!(
        !handoff.ready(two_hours).await,
        "the remembered reply answered for a host that no longer gives one"
    );
    h.fake.answer_policy_with(None);
    ready_at(&h, &handoff, two_hours, "the host answers its policy again").await;

    Autopilot::cancel_timer(&h.rt).await;
}

/// The 429 a turn at a usage wall ends on: its body names `resets_at`, or no reset at all.
fn usage_error(resets_at: Option<i64>) -> codex_protocol::error::CodexErrorDetails {
    codex_protocol::error::CodexErrorDetails::UsageLimitReached(
        codex_protocol::error::UsageLimitReachedError {
            plan_type: None,
            resets_at: resets_at.map(|at| {
                chrono::DateTime::from_timestamp(at, 0)
                    .unwrap_or_else(|| panic!("a reset no clock names: {at}"))
            }),
            limit_window_minutes: None,
            rate_limits: None,
            promo_message: None,
            rate_limit_reached_type: None,
        },
    )
}

/// Core ends `turn` on the 429 `details`: its error, then its stop.
async fn ended_at_usage_wall(
    h: &Harness,
    turn: &ExtensionData,
    details: &codex_protocol::error::CodexErrorDetails,
) {
    use codex_extension_api::TurnErrorInput;
    use codex_extension_api::TurnLifecycleContributor;
    use codex_extension_api::TurnStopInput;
    let session = ExtensionData::new("session");
    h.autopilot
        .on_turn_error(TurnErrorInput {
            turn_id: turn.level_id(),
            error: codex_protocol::protocol::CodexErrorInfo::UsageLimitExceeded,
            error_details: details,
            session_store: &session,
            thread_store: &h._store,
            turn_store: turn,
        })
        .await;
    h.autopilot
        .on_turn_stop(TurnStopInput {
            session_store: &session,
            thread_store: &h._store,
            turn_store: turn,
        })
        .await;
}

/// The usage wall the point stands at: its reset on this clock, and the words it is told in.
fn point_usage_wall(h: &Harness) -> (Option<Instant>, Option<String>) {
    match h.rt.state().point.and_then(|point| point.wall) {
        Some(Wall::Usage {
            reset_at,
            reset_label,
        }) => (reset_at, reset_label),
        wall => panic!("the ended turn stands at no usage wall: {wall:?}"),
    }
}

/// A TURN THE HOST'S YES ENDED AT THE WALL `read` STANDS AT IT, its 429 naming `named_after`
/// seconds from the read's reset, or no reset: on a fresh harness (rotation, a two-row pool,
/// `limit_wait`, `codex_rotation_five_hour=900`) the turn's handoff answers yes for the wall
/// `read` builds, and the turn ends on that 429. The point's reset and its label are the
/// read's, and the real evaluator decides the wall's own resume (no move to ask for here) at
/// that reset plus the grace, with no hold. NEGATIVE CONTROL: the next turn, whose handoff was
/// never answered, stands at its own 429's word alone.
async fn a_yes_ended_turn_stands_at_the_reads_wall(
    config: &Arc<codex_core::config::Config>,
    manager: &Arc<codex_login::AuthManager>,
    read: fn() -> SpentWall,
    named_after: Option<i64>,
) {
    let h = started_timed_with_config(
        &[
            ("codex_rotation", "1"),
            ("accounts", "wall"),
            ("limit_wait", "true"),
            (FIVE_HOUR_HANDOFF, "900"),
        ],
        |_, _| {},
        true,
        ClientOptions::default(),
        Timing::default(),
        Some(Arc::clone(config)),
    )
    .await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    h.autopilot.set_auth_manager(Arc::clone(manager));
    show_pool(&h, None, &[row("a", "usable"), row("b", "unseen")]);
    h.rt.with_state(crate::decide::ThreadState::observe_person_turn);
    let read = read();
    let window = read.window;
    let (turn, handoff) = begin(&h.autopilot, &h._store, "walled-turn").await;
    ready_at(
        &h,
        &handoff,
        read,
        &format!("a {window:?} wall the host leaves"),
    )
    .await;

    let named = named_after.map(|after| read.resets_at + after);
    let expected = crate::supervisor::clock()
        + read.resets_in(i64::try_from(now_ms() / 1000).unwrap_or(i64::MAX));
    ended_at_usage_wall(&h, &turn, &usage_error(named)).await;
    let (reset_at, reset_label) = point_usage_wall(&h);
    let reset_at = reset_at.unwrap_or_else(|| {
        panic!(
            "{window:?}: the 429 named {named:?}: the wall the host was asked about names no \
             reset"
        )
    });
    let off = if reset_at > expected {
        reset_at - expected
    } else {
        expected - reset_at
    };
    assert!(
        off <= Duration::from_secs(2),
        "{window:?}: the 429 named {named_after:?} from the window's reset: the wall resets \
         {off:?} from the usage read's"
    );
    let label = chrono::DateTime::from_timestamp(read.resets_at, 0)
        .unwrap_or_else(|| panic!("the read's reset"))
        .with_timezone(&chrono::Local)
        .format("%H:%M")
        .to_string();
    assert_eq!(
        reset_label.as_deref(),
        Some(label.as_str()),
        "{window:?}: {named_after:?}"
    );

    // The real evaluator: no move to ask for here, so the wall's own resume is decided.
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert!(
        h.rt.timer_pending(),
        "the evaluator scheduled the wall's resume"
    );
    let policy = h.sup.policy().unwrap_or_else(|| panic!("a proven policy"));
    let timing = Timing::default();
    let decision = crate::decide::decide(
        &h.rt.state(),
        &policy,
        &timing,
        crate::supervisor::clock(),
        Duration::ZERO,
    );
    assert!(
        matches!(decision.act, crate::decide::Act::ResumeLimit { .. }),
        "{window:?}: {named_after:?}: {decision:?}"
    );
    assert_eq!(
        decision.not_before,
        Some(reset_at + timing.reset_grace),
        "{window:?}: {named_after:?}: the session resumes at the read's reset and the grace"
    );
    assert!(
        decision.held.is_none(),
        "{window:?}: {named_after:?}: {decision:?}"
    );
    Autopilot::cancel_timer(&h.rt).await;

    // NEGATIVE CONTROL: the next turn's handoff was never answered, so its 429's word is all
    // its wall has; the earlier turn's yes is not carried into it.
    let (next, _) = begin(&h.autopilot, &h._store, "next-turn").await;
    ended_at_usage_wall(&h, &next, &usage_error(None)).await;
    assert_eq!(
        point_usage_wall(&h),
        (None, None),
        "{window:?}: a turn no yes ended took another turn's wall"
    );
    Autopilot::cancel_timer(&h.rt).await;
}

/// A FIVE-HOUR TURN THE HOST'S YES ENDED STANDS AT THE WALL THE HOST WAS ASKED ABOUT, WHATEVER
/// THE 429 NAMED (gap 6 of the audit of 2026-11-14). Quota recovery offers a five-hour wall on
/// the usage read alone, so the 429 may name no reset, or one far from the window's own in
/// either direction. The handoff keeps the wall its yes was given for, and the turn's error
/// builds its usage wall from that read's reset: the bound on asking the host
/// (`rotation_ask_lapsed`, which reads this same wall) and the wall's own resume, which is all
/// a declined or lapsed ask leaves, run on it. Here nothing moves the session (this harness has
/// no thread to ask a move for), so the point is the wall's own: the usage resume, at the
/// read's reset plus the grace, with no hold. Without the kept wall a 429 with no reset would
/// leave the ask with no bound and the resume blind (the longest back-off, into the same
/// wall), and one far off would resume the session at the 429's moment.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_five_hour_turn_a_yes_ended_resumes_at_the_reads_reset_whatever_the_429_named() {
    let home = fixtures::home();
    fixtures::chatgpt(home.path(), "user", "account", "refresh", None);
    let manager = fixtures::manager(home.path(), AuthCredentialsStoreMode::File).await;
    let config = openai_config(&home).await;
    for named_after in [None, Some(23 * 3600), Some(-1800), Some(30)] {
        a_yes_ended_turn_stands_at_the_reads_wall(
            &config,
            &manager,
            || five_hours(2 * 3600),
            named_after,
        )
        .await;
    }
}

/// A WEEK TURN THE HOST'S YES ENDED STANDS AT THE WEEK'S OWN RESET, WHATEVER THE 429 NAMED (gap
/// 6 of the audit of 2026-11-14, the departure from the narrowest fix: the read's wall is kept
/// for a spent week too). A spent week was always offered whatever the 429 named, and a turn it
/// ended stood at the 429's reset: one that named a five-hour window's earlier reset lapsed the
/// ask then and resumed into a week still spent, and one that named none was asked about
/// without limit and resumed blind. The point's reset and label are the read's week reset, and
/// the wall's own resume is decided at that reset plus the grace.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_week_turn_a_yes_ended_resumes_at_the_weeks_reset_whatever_the_429_named() {
    let home = fixtures::home();
    fixtures::chatgpt(home.path(), "user", "account", "refresh", None);
    let manager = fixtures::manager(home.path(), AuthCredentialsStoreMode::File).await;
    let config = openai_config(&home).await;
    // A five-hour window's reset two hours from now, short of the week's three days by
    // seventy hours, and no reset at all.
    for named_after in [Some(2 * 3600 - 3 * 24 * 3600), None] {
        a_yes_ended_turn_stands_at_the_reads_wall(&config, &manager, week, named_after).await;
    }
}

/// The usage wall's move of the session's primary thread away from the login `account`,
/// `user`, as `follow_renewal` publishes it before it asks the host; `purpose` and the
/// account are the test's to vary.
fn move_intent(h: &Harness, purpose: Purpose, account: &str, user: &str) -> OwedAct {
    let identity = codex_alab_aterm::renewal::Identity::from_native(account, user)
        .unwrap_or_else(|| panic!("fixture identity"));
    OwedAct {
        thread: h.rt.key().to_owned(),
        native_origin: Some(codex_alab_core::session::NativeOrigin {
            turn_id: "the-primary's-walled-turn".into(),
            phase: NativePhase::Complete,
        }),
        renewal: Some(AccountHandoff {
            purpose,
            destination: None,
            openai_scope: true,
            local: false,
            account: identity.account,
            user: identity.user,
            receipt: None,
            turn: None,
            accepted: false,
            starts: 1,
            goal_id: None,
        }),
        rule: Some(purpose.rule().into()),
        text: Some("Continue the original task.".into()),
        ..Default::default()
    }
}

/// ANOTHER ROOT THREAD'S WALLED TURN ENDS ONLY WHILE THE SESSION IS LEAVING THE ACCOUNT. A
/// side chat's turn at a creditless usage wall starts no move, so it waits the wall out;
/// but its turn keeps the record busy, and aterm moves no account under a busy record, so
/// one such turn would hold the primary thread's handoff until the wall's reset. Its
/// handoff therefore says ready while the primary's move is live: the usage wall's intent
/// from this login on record, codex-alab acting, no upgrade under way, and the primary's
/// last follow pass having kept its point and either sent the ask or kept the receipt the
/// host granted. Each of those is necessary. An intent that only lingers on record (the
/// pass let its point go, Autopilot went off) ends nothing, and neither does an unbound
/// point the pass kept with no word to the host: that turn would give up its look for an
/// earned reset with no move to follow. The answer is a memory read, with no request to
/// the host.
///
/// A waiting turn is woken by the next pass that leaves the usage wall's move live after one
/// of its looks began, whether that look was told no or ended before it asked (its credit
/// list unread), and by no other pass: one with no look since the last wake, one for
/// another handoff, or one while codex-alab does not act wakes nothing, since each wake
/// costs every waiting turn a usage read and a credit list read. The primary thread's own
/// looks leave no word.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_root_threads_walled_turn_ends_only_while_the_session_is_leaving() {
    let home = fixtures::home();
    fixtures::chatgpt(home.path(), "user", "account", "refresh", None);
    let manager = fixtures::manager(home.path(), AuthCredentialsStoreMode::File).await;
    let config = openai_config(&home).await;
    let h = started_timed_with_config(
        ROTATION,
        |_, _| {},
        true,
        ClientOptions::default(),
        Timing::default(),
        Some(Arc::clone(&config)),
    )
    .await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    h.autopilot.set_auth_manager(Arc::clone(&manager));
    show_pool(&h, None, &[row("a", "usable"), row("b", "unseen")]);

    // The other root thread the TUI shows (a side chat), with a turn running.
    let other = ThreadId::new();
    let other_key = other.to_string();
    let other_store = ExtensionData::new(other_key.clone());
    other_store.insert(ThreadRt::new(
        other,
        Some(Arc::clone(&config)),
        false,
        Duration::ZERO,
    ));
    let other_rt = other_store
        .get::<ThreadRt>()
        .unwrap_or_else(|| panic!("inserted"));
    h.autopilot.keep_thread(&other_rt);
    h.shared.note_root_thread(&other_key);
    let side = begun(&h.autopilot, &other_store, "the-side-chat's-turn").await;
    let acting = || h.sup.acting() && h.sup.policy().is_some();
    // What the host hears of a handoff question: its policy, its pool, the handoff verb.
    let asked = || {
        h.fake
            .requests()
            .into_iter()
            .filter(|request| {
                request.ends_with(" harness policy")
                    || request.contains(" codex-accounts ")
                    || request.contains(" codex-rotation ")
            })
            .count()
    };
    let told = || {
        h.sup
            .journal()
            .iter()
            .filter(|line| line.contains("its walled turn ends; the session is leaving"))
            .count()
    };
    let usage = move_intent(&h, Purpose::WeeklyQuota, "account", "user");
    let publish = |owed: &OwedAct| {
        assert!(
            h.shared
                .publish_renewal(h.shared.admission_generation(), owed.clone()),
            "the intent was not published"
        );
    };

    // Nothing on record: the side turn waits its wall out, as it always has. A follow pass
    // that left a handoff live with no usage move on record (a login's renewal has such
    // passes) says nothing about a move either.
    assert!(acting());
    assert!(!looks_and_asks(&side, week()).await, "no move is on record");
    h.autopilot.note_move_followed(&h.rt, true);
    assert!(
        !looks_and_asks(&side, week()).await,
        "a live pass with no move"
    );
    h.autopilot.note_move_followed(&h.rt, false);

    // AN INTENT THAT ONLY LINGERS: on record, and the primary's last pass let its point go.
    publish(&usage);
    assert!(acting());
    assert!(
        !looks_and_asks(&side, week()).await,
        "an intent nobody is asking about ended a side turn"
    );
    assert_eq!(told(), 0);

    // A KEPT POINT IS NOT A MOVE BEING ASKED FOR. This pass keeps its point because it
    // could not read the thread's custody, and sends the host nothing; a pass held by a
    // person's hold, a draft or an unreadable `status` keeps it the same way
    // (`side_wall.rs` runs the hold). Nothing is asking the host, so the side turn keeps
    // its look for an earned reset and is not woken.
    let mut wake = h.shared.account_move_watch();
    let heard = asked();
    assert!(
        h.autopilot.follow_renewal_for_test(&h.rt).await,
        "the pass keeps a point whose custody it could not read"
    );
    assert_eq!(asked(), heard, "{:#?}", h.fake.requests());
    assert!(
        !h.rt.move_live(),
        "a kept point was taken for a move put to the host"
    );
    assert!(acting());
    assert!(
        !looks_and_asks(&side, week()).await,
        "a pass that asked the host nothing ended a side turn"
    );
    assert_eq!(
        wake.has_changed().ok(),
        Some(false),
        "a pass that asked the host nothing woke the turns that wait the wall out"
    );
    assert_eq!(told(), 0);

    // THE LIVE MOVE: the pass sent the ask (this harness's pass cannot reach the ask, as it
    // has no core thread, so its record is made here). The turn that was told no is woken,
    // and its next question is answered yes; the host hears nothing of it.
    h.autopilot.note_move_followed(&h.rt, true);
    assert!(h.rt.move_live());
    assert_eq!(
        wake.has_changed().ok(),
        Some(true),
        "the pass that asks wakes the turn that was told to wait"
    );
    wake.mark_unchanged();
    // The next live pass, with no look since the wake, wakes nobody.
    h.autopilot.note_move_followed(&h.rt, true);
    assert_eq!(
        wake.has_changed().ok(),
        Some(false),
        "a pass woke with no turn waiting"
    );
    // A look that ends before its question (its credit list unread within a poll of the
    // first unread one) asks nothing, and waits on the next live pass all the same.
    side.look_begins();
    h.autopilot.note_move_followed(&h.rt, true);
    assert_eq!(
        wake.has_changed().ok(),
        Some(true),
        "a live pass did not wake a turn whose look ended before it asked"
    );
    wake.mark_unchanged();
    assert!(acting());
    assert!(
        looks_and_asks(&side, week()).await,
        "the session is leaving"
    );
    assert!(
        looks_and_asks(&side, five_hours(2 * 3600)).await,
        "the wall is the account's, whichever window the side turn's read names"
    );
    assert_eq!(
        asked(),
        heard,
        "the side turn's question is a memory read: {:#?}",
        h.fake.requests()
    );
    assert_eq!(told(), 1, "told once a turn: {:#?}", h.sup.journal());

    // The upgrade's exec is under way: nothing follows any turn's end.
    h.autopilot.set_upgrade_requested(true);
    assert!(
        !looks_and_asks(&side, week()).await,
        "an upgrade is replacing us"
    );
    h.autopilot.set_upgrade_requested(false);
    assert!(looks_and_asks(&side, week()).await);

    // The move is not this process's to wait for: another login's, a login's own renewal,
    // a lost login's move (not claimed here), a same-file recovery. The side turn is told
    // no, and the pass that leaves such a handoff live does not wake it: it would be told
    // no again, at the cost of a usage read and a credit list read, at every pass.
    let ours = |owed: &OwedAct| {
        assert!(
            h.shared
                .discard_renewal(&h.shared.owed_act().unwrap_or_default())
        );
        publish(owed);
        h.autopilot.note_move_followed(&h.rt, true);
        assert!(acting());
    };
    let mut local = usage.clone();
    if let Some(renewal) = local.renewal.as_mut() {
        renewal.local = true;
    }
    for (owed, what) in [
        (
            move_intent(&h, Purpose::WeeklyQuota, "another-account", "user"),
            "a move from another login",
        ),
        (
            move_intent(&h, Purpose::Login, "account", "user"),
            "a login's own renewal",
        ),
        (
            move_intent(&h, Purpose::LoginRotation, "account", "user"),
            "a lost login's move",
        ),
        (local, "a same-file recovery"),
    ] {
        ours(&owed);
        assert!(
            !looks_and_asks(&side, week()).await,
            "{what} ended a side turn"
        );
        // The turn waits, told no. The intent's own arrival on the record is not the
        // pass's wake, so the watch is read from here.
        wake.mark_unchanged();
        h.autopilot.note_move_followed(&h.rt, true);
        assert_eq!(
            wake.has_changed().ok(),
            Some(false),
            "{what}: its pass woke the turns that wait a usage wall out"
        );
    }
    ours(&usage);
    assert!(
        looks_and_asks(&side, week()).await,
        "the usage wall's move again"
    );

    // A PASS THAT LETS THE POINT GO clears the mark, and the intent it leaves on record
    // ends nothing: here the intent is released first, so the pass has nothing to keep.
    assert!(h.shared.discard_renewal(&usage));
    assert!(
        !looks_and_asks(&side, week()).await,
        "the intent was released"
    );
    assert!(h.rt.move_live());
    assert!(
        !h.autopilot.follow_renewal_for_test(&h.rt).await,
        "a point with no wall and no intent is kept"
    );
    assert!(!h.rt.move_live());
    publish(&usage);
    assert!(acting());
    assert!(
        !looks_and_asks(&side, week()).await,
        "the pass let the point go"
    );
    wake.mark_unchanged();
    h.autopilot.note_move_followed(&h.rt, true);
    assert_eq!(
        wake.has_changed().ok(),
        Some(true),
        "the pass that asks again did not wake the turn told to wait"
    );
    wake.mark_unchanged();
    assert!(acting());
    assert!(looks_and_asks(&side, week()).await);

    // A RECEIPT THE HOST GRANTED IS A LIVE MOVE, whether or not the pass reached the host:
    // its `commit` may have gone, and the host restarts the session only under a record at
    // rest. The pass that kept the unbound point with no word to the host, and told the
    // side turn no, keeps the bound one the same way, and that is the live move.
    assert!(h.autopilot.follow_renewal_for_test(&h.rt).await);
    assert!(!h.rt.move_live());
    assert!(
        !looks_and_asks(&side, week()).await,
        "an unbound point kept with no ask ended a side turn"
    );
    let mut granted = usage.clone();
    assert!(
        granted.renewal.as_mut().is_some_and(|renewal| renewal.bind(
            &"1".repeat(32),
            &"c".repeat(64),
            &"d".repeat(64),
            "the-primary's-walled-turn",
        )),
        "the receipt did not bind"
    );
    publish(&granted);
    assert_eq!(
        wake.has_changed().ok(),
        Some(false),
        "a receipt bound to the move on record woke the turns as a new move"
    );
    let heard = asked();
    assert!(
        h.autopilot.follow_renewal_for_test(&h.rt).await,
        "the pass keeps a bound point whose custody it could not read"
    );
    assert_eq!(asked(), heard, "{:#?}", h.fake.requests());
    assert!(
        h.rt.move_live(),
        "a kept receipt the host granted was not taken for a live move"
    );
    assert_eq!(
        wake.has_changed().ok(),
        Some(true),
        "the pass that keeps the receipt did not wake the turn told to wait"
    );
    wake.mark_unchanged();
    assert!(
        looks_and_asks(&side, week()).await,
        "the receipt the host granted stands"
    );

    // AUTOPILOT OFF with the receipt on record and the mark set: nothing follows the move
    // while codex-alab stands by, so the side turn keeps its look for an earned reset, and
    // a pass that keeps the receipt then wakes nobody, since its turn would be told no.
    h.fake.set_effective(false, "suspended");
    wait_until("passive", || !h.sup.acting()).await;
    assert_eq!(
        h.shared.owed_act().as_ref(),
        Some(&granted),
        "the receipt stays on record through an Off"
    );
    assert!(
        !looks_and_asks(&side, week()).await,
        "an Off ended a side turn"
    );
    h.autopilot.note_move_followed(&h.rt, true);
    assert_eq!(
        wake.has_changed().ok(),
        Some(false),
        "a live pass under an Off woke the turns that wait the wall out"
    );
    // ACTING AGAIN. Every pass kept the receipt, so the mark was set before the Off and is
    // set still: it never turns from no to yes. The turn told no under the Off is woken
    // all the same, by the next pass that keeps the receipt, because its look left word
    // that it waits; and its next question is answered yes.
    act_again(&h).await;
    wait_until(
        "a pass after the Off wakes the turn told no under it",
        || wake.has_changed().unwrap_or(false),
    )
    .await;
    assert!(h.rt.move_live());
    assert!(
        settled(&h, &side).await,
        "acting again, with the receipt the host granted kept"
    );

    // A TURN WHOSE LOOK NEVER ASKED, THROUGH ANOTHER OFF. That wake spent the word, and
    // with no look since, a live pass wakes nobody. Off again: the receipt is kept and the
    // mark stays set, so acting again turns nothing from no to yes; and the side turn's
    // look under this Off ends before its question (its credit list unread), so it asks
    // nothing either. The look left the word, and the next pass after acting again wakes
    // the turn.
    wake.mark_unchanged();
    h.autopilot.note_move_followed(&h.rt, true);
    assert_eq!(
        wake.has_changed().ok(),
        Some(false),
        "a pass woke with no look since the last wake"
    );
    h.fake.set_effective(false, "suspended");
    wait_until("passive again", || !h.sup.acting()).await;
    side.look_begins();
    h.autopilot.note_move_followed(&h.rt, true);
    assert_eq!(
        wake.has_changed().ok(),
        Some(false),
        "a live pass under the second Off woke the turns that wait the wall out"
    );
    act_again(&h).await;
    wait_until(
        "a pass after the Off wakes the turn whose look never asked",
        || wake.has_changed().unwrap_or(false),
    )
    .await;
    assert!(h.rt.move_live());
    assert!(
        settled(&h, &side).await,
        "acting again after the second Off"
    );

    // A look of the primary thread's own walled turn leaves no word that a turn waits on
    // its move: its question is its own. A side turn's look does, on the same live move.
    wake.mark_unchanged();
    h.autopilot.walled_look_begins(&h.rt);
    h.autopilot.note_move_followed(&h.rt, true);
    assert_eq!(
        wake.has_changed().ok(),
        Some(false),
        "the primary's own look woke the turns that wait the wall out"
    );
    side.look_begins();
    h.autopilot.note_move_followed(&h.rt, true);
    assert_eq!(
        wake.has_changed().ok(),
        Some(true),
        "a side turn's look left no word"
    );

    // The primary thread's own question is not this rule's: it reads the host afresh.
    let primary = begun(&h.autopilot, &h._store, "the-primary's-next-turn").await;
    let reads = policy_reads(&h);
    ready(&h, &primary, "the primary thread, by its own gates").await;
    assert!(policy_reads(&h) > reads);

    Autopilot::cancel_timer(&h.rt).await;
}

/// The person's line for a move a background terminal holds, word for word.
const MOVE_WAITS_FOR_TERMINALS: &str = "The usage limit is reached, and another signed-in \
                                        account has room. Background terminals run in this \
                                        session, and the restart would end them, so the move \
                                        waits until they finish or you stop them.";

/// The warnings the host is handed for the person, as the app server's sink takes them to the
/// TUI's history (`ExtensionEventSink::emit_warning`).
#[derive(Default)]
struct Warnings(std::sync::Mutex<Vec<codex_extension_api::ExtensionWarning>>);

impl Warnings {
    fn said(&self) -> Vec<codex_extension_api::ExtensionWarning> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl codex_extension_api::ExtensionEventSink for Warnings {
    fn emit(&self, _event: codex_protocol::protocol::Event) {}

    fn emit_warning(&self, warning: codex_extension_api::ExtensionWarning) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(warning);
    }
}

/// The journal's line for a move the session's background terminals hold.
const TERMINALS_HOLD: &str = "waits until the session's background terminals stop";

/// A RUNNING BACKGROUND TERMINAL HOLDS THE HANDOFF (gap 1 of the audit of 2026-11-14, as its
/// review amended it). The move's restart ends every unified-exec terminal (a dev server, a
/// watch) of the session, and aterm's rest fence sees only the shown thread's. A walled turn
/// ended for such a move would give up its look for an earned reset, and its move would either
/// be refused until the wall's reset or end the terminal. So the handoff is not ready while the
/// session's count (core's, over every loaded thread; this harness has no thread manager, so
/// the test says it) is above zero, whatever the TUI's footer counts for the thread it shows:
/// the turn keeps polling and asks again on its own cadence, and the first question after the
/// terminals end is ready. The person is told once a wall, in a warning the host shows, why
/// the move waits; only where the move would otherwise follow, and with nothing more asked of
/// the host while the terminals still run.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn background_terminals_hold_the_handoff_and_the_person_is_told_once() {
    let home = fixtures::home();
    fixtures::chatgpt(home.path(), "user", "account", "refresh", None);
    let manager = fixtures::manager(home.path(), AuthCredentialsStoreMode::File).await;
    let config = openai_config(&home).await;
    let h = started_timed_with_config(
        ROTATION,
        |_, _| {},
        true,
        ClientOptions::default(),
        Timing::default(),
        Some(Arc::clone(&config)),
    )
    .await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    // The host's event sink, as the app server hands every extension one at registration.
    let warnings = Arc::new(Warnings::default());
    h.autopilot
        .register(&mut codex_extension_api::ExtensionRegistryBuilder::<
            codex_core::config::Config,
        >::with_event_sink(Arc::clone(&warnings) as _));
    h.autopilot.set_auth_manager(Arc::clone(&manager));
    let handoff = begun(&h.autopilot, &h._store, "walled-turn").await;
    // The TUI shows another thread, whose footer counts none: not what holds the move.
    h.shared.set_background_terminals(0, Instant::now());
    let terminals = |count| h.autopilot.set_session_terminals(Some(count));
    let told = || {
        h.sup
            .journal()
            .iter()
            .filter(|line| line.contains(TERMINALS_HOLD))
            .count()
    };
    // What the host hears of a handoff question: its policy and its pool.
    let asked = || {
        h.fake
            .requests()
            .into_iter()
            .filter(|request| {
                request.ends_with(" harness policy") || request.contains(" codex-accounts ")
            })
            .count()
    };

    // A terminal runs, and the host has nowhere to move the session: not ready, and nothing
    // is said of terminals, since no move would follow them.
    terminals(1);
    assert!(
        !settled(&h, &handoff).await,
        "a pool with nowhere to go hands off"
    );
    assert_eq!(told(), 0, "{:#?}", h.sup.journal());
    assert!(warnings.said().is_empty(), "{:?}", warnings.said());

    // Another account has room: the move would follow, but the terminal holds it.
    show_pool(&h, None, &[row("a", "usable"), row("b", "unseen")]);
    assert!(
        !settled(&h, &handoff).await,
        "a running background terminal hands off"
    );
    assert_eq!(told(), 1, "{:#?}", h.sup.journal());
    let said = warnings.said();
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0].thread_id, h.rt.key());
    assert_eq!(said[0].turn_id, None);
    assert_eq!(said[0].message, MOVE_WAITS_FOR_TERMINALS);
    assert!(
        said[0].message.len() <= 256,
        "the app server cuts a warning at 256 bytes"
    );
    // Asked again while it runs: not ready, not told again, and the host hears nothing.
    let heard = asked();
    for poll in 0..3 {
        assert!(
            !settled(&h, &handoff).await,
            "poll {poll}: a running background terminal hands off"
        );
    }
    assert_eq!(asked(), heard, "{:#?}", h.fake.requests());
    assert_eq!(told(), 1);
    assert_eq!(warnings.said().len(), 1);

    // The terminals end: the next question is ready, and reads the host afresh.
    terminals(0);
    ready(&h, &handoff, "the terminals ended").await;
    assert!(asked() > heard);
    // A terminal again within the same turn: held again, told no more.
    terminals(2);
    assert!(!settled(&h, &handoff).await, "two terminals hand off");
    assert_eq!(told(), 1);
    assert_eq!(warnings.said().len(), 1);

    // The next walled turn is told once again.
    let next = begun(&h.autopilot, &h._store, "next-walled-turn").await;
    assert!(
        !settled(&h, &next).await,
        "the next turn's terminals hand off"
    );
    assert_eq!(told(), 2, "{:#?}", h.sup.journal());
    assert_eq!(warnings.said().len(), 2);
    terminals(0);
    ready(&h, &next, "the next turn's terminals ended").await;

    Autopilot::cancel_timer(&h.rt).await;
}

/// ANOTHER ROOT THREAD'S WALLED TURN WAITS WHILE THE SESSION'S BACKGROUND TERMINALS RUN (the
/// review of gap 1, its major finding). The primary walled with no terminal running, and its
/// move is live (its intent on record, its last pass asked), so a side chat's walled turn
/// would end for it. But a terminal now runs in the session (the side chat's own dev server),
/// which the move's restart would end, so the move waits, and ending the side turn would give
/// up its look for an earned reset for nothing. Its handoff says no while the terminal runs.
/// While the host's pool shows no other account the person is told nothing (the review of
/// `af5f4fcc54`, its first finding: the move is live on the intent alone, which no pool read
/// made, and a person told it waits for their terminals would stop a dev server for a move
/// that never comes); the hold stands all the same. Once another account has room the person is
/// told once a wall, in the history of every root thread the autopilot keeps (whichever root
/// thread the person views shows it, the side chat it shows here among them); questions after
/// that tell no more and ask the host nothing; and the first question after the terminal ends
/// says yes. The wall is the primary's: a later terminal under it is told no more, and the
/// primary's next walled turn is told again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_root_threads_walled_turn_waits_while_the_sessions_terminals_run() {
    let home = fixtures::home();
    fixtures::chatgpt(home.path(), "user", "account", "refresh", None);
    let manager = fixtures::manager(home.path(), AuthCredentialsStoreMode::File).await;
    let config = openai_config(&home).await;
    let h = started_timed_with_config(
        ROTATION,
        |_, _| {},
        true,
        ClientOptions::default(),
        Timing::default(),
        Some(Arc::clone(&config)),
    )
    .await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    let warnings = Arc::new(Warnings::default());
    h.autopilot
        .register(&mut codex_extension_api::ExtensionRegistryBuilder::<
            codex_core::config::Config,
        >::with_event_sink(Arc::clone(&warnings) as _));
    h.autopilot.set_auth_manager(Arc::clone(&manager));
    // The host's pool shows only this session's account able to run: nowhere to move to.
    show_pool(&h, None, &[row("a", "usable"), row("b", "limited")]);

    // The side chat, a root thread the TUI shows, with a turn running.
    let other = ThreadId::new();
    let other_key = other.to_string();
    let other_store = ExtensionData::new(other_key.clone());
    other_store.insert(ThreadRt::new(
        other,
        Some(Arc::clone(&config)),
        false,
        Duration::ZERO,
    ));
    let other_rt = other_store
        .get::<ThreadRt>()
        .unwrap_or_else(|| panic!("inserted"));
    h.autopilot.keep_thread(&other_rt);
    h.shared.observe(
        &Frame {
            primary_thread: Some(h.rt.key()),
            displayed_thread: Some(&other_key),
            rollout_path: None,
            cwd: std::path::Path::new("/work"),
            model: "gpt-5",
            codex_home: home.path(),
            embedded: true,
            state: ProgramState::Idle,
            draft_empty: true,
            queued_input: false,
            popup_open: false,
            editor_open: false,
            pending_user_turn: false,
        },
        now_ms(),
    );
    h.shared.note_root_thread(&other_key);
    assert!(h.shared.is_root_thread(&other_key));
    // Its footer counts none for a moment: not what holds the move.
    h.shared.set_background_terminals(0, Instant::now());
    let side = begun(&h.autopilot, &other_store, "the-side-chat's-turn").await;
    let acting = || h.sup.acting() && h.sup.policy().is_some();
    let asked = || {
        h.fake
            .requests()
            .into_iter()
            .filter(|request| {
                request.ends_with(" harness policy")
                    || request.contains(" codex-accounts ")
                    || request.contains(" codex-rotation ")
            })
            .count()
    };
    let told = || {
        h.sup
            .journal()
            .iter()
            .filter(|line| line.contains(TERMINALS_HOLD))
            .count()
    };
    let leaving = || {
        h.sup
            .journal()
            .iter()
            .filter(|line| line.contains("its walled turn ends; the session is leaving"))
            .count()
    };
    let terminals = |count| h.autopilot.set_session_terminals(Some(count));
    let threads = |said: &[codex_extension_api::ExtensionWarning]| {
        let mut threads: Vec<String> = said.iter().map(|w| w.thread_id.clone()).collect();
        threads.sort();
        threads
    };
    let mut both = vec![h.rt.key().to_owned(), other_key.clone()];
    both.sort();

    // THE PRIMARY'S MOVE IS LIVE: its intent from this login on record, its last pass asked.
    let usage = move_intent(&h, Purpose::WeeklyQuota, "account", "user");
    assert!(
        h.shared
            .publish_renewal(h.shared.admission_generation(), usage.clone()),
        "the intent was not published"
    );
    h.autopilot.note_move_followed(&h.rt, true);

    // The side chat's dev server runs, and no other account has room: held, and the person
    // is told nothing, question after question.
    terminals(1);
    for poll in 0..3 {
        assert!(acting());
        assert!(
            !looks_and_asks(&side, week()).await,
            "poll {poll}: a side turn ended while the session's terminal ran"
        );
    }
    assert_eq!(told(), 0, "{:#?}", h.sup.journal());
    assert!(warnings.said().is_empty(), "{:?}", warnings.said());
    assert_eq!(leaving(), 0);

    // Another account has room: still not ready, and the person is told, in each root
    // thread's history.
    show_pool(&h, None, &[row("a", "usable"), row("b", "unseen")]);
    assert!(acting());
    assert!(
        !looks_and_asks(&side, week()).await,
        "a side turn ended while the session's terminal ran"
    );
    assert_eq!(told(), 1, "{:#?}", h.sup.journal());
    let said = warnings.said();
    assert_eq!(threads(said.as_slice()), both, "{said:?}");
    assert!(
        said.iter()
            .all(|warning| warning.message == MOVE_WAITS_FOR_TERMINALS && warning.turn_id.is_none()),
        "{said:?}"
    );
    assert_eq!(leaving(), 0);
    // Asked again while it runs: not ready, not told again, and the host hears nothing.
    let heard = asked();
    for poll in 0..3 {
        assert!(acting());
        assert!(
            !looks_and_asks(&side, week()).await,
            "poll {poll}: a side turn ended while the session's terminal ran"
        );
    }
    assert_eq!(told(), 1);
    assert_eq!(warnings.said().len(), 2);
    assert_eq!(asked(), heard, "{:#?}", h.fake.requests());

    // The terminal ends: the first question after it says the session is leaving.
    terminals(0);
    assert!(acting());
    assert!(
        looks_and_asks(&side, week()).await,
        "the terminal ended and the move is live"
    );
    assert_eq!(leaving(), 1, "{:#?}", h.sup.journal());
    assert_eq!(asked(), heard, "{:#?}", h.fake.requests());

    // A terminal again under the same wall: held again, told no more.
    terminals(1);
    assert!(acting());
    assert!(!looks_and_asks(&side, week()).await);
    assert_eq!(told(), 1);
    assert_eq!(warnings.said().len(), 2);

    // The primary's next walled turn is another wall: its own question is told again.
    let next = begun(&h.autopilot, &h._store, "the-primary's-next-turn").await;
    assert!(
        !settled(&h, &next).await,
        "the session's terminal hands the primary's turn off"
    );
    assert_eq!(told(), 2, "{:#?}", h.sup.journal());
    assert_eq!(threads(&warnings.said()[2..]), both);
    terminals(0);
    ready(&h, &next, "the session's terminal ended").await;

    Autopilot::cancel_timer(&h.rt).await;
}

/// NO CLAIM: an autopilot with no aterm to claim installs the handoff all the same, and it
/// refuses, so the sampler keeps polling for an earned reset as it does without a host.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wall_handoff_without_a_claim_keeps_the_sampler_polling() {
    let home = fixtures::home();
    fixtures::chatgpt(home.path(), "user", "account", "refresh", None);
    let manager = fixtures::manager(home.path(), AuthCredentialsStoreMode::File).await;
    let config = openai_config(&home).await;
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(|| None),
        client: ClientOptions::default(),
    });
    let autopilot = Autopilot::new(Arc::clone(&sup), None, None, Timing::default(), |_| true);
    autopilot.set_auth_manager(manager);
    let id = ThreadId::new();
    let key = id.to_string();
    let store = ExtensionData::new(key.clone());
    store.insert(ThreadRt::new(id, Some(config), false, Duration::ZERO));
    let rt = store
        .get::<ThreadRt>()
        .unwrap_or_else(|| panic!("inserted"));
    autopilot.keep_thread(&rt);
    shared.observe(
        &Frame {
            primary_thread: Some(&key),
            displayed_thread: Some(&key),
            rollout_path: None,
            cwd: std::path::Path::new("/work"),
            model: "gpt-5",
            codex_home: home.path(),
            embedded: true,
            state: ProgramState::Idle,
            draft_empty: true,
            queued_input: false,
            popup_open: false,
            editor_open: false,
            pending_user_turn: false,
        },
        now_ms(),
    );
    shared.note_root_thread(&key);
    let handoff = begun(&autopilot, &store, "walled-turn").await;
    assert!(!sup.acting());
    assert!(!handoff.ready(week()).await, "no proven claim hands off");
    assert!(
        !handoff.ready(five_hours(2 * 3600)).await,
        "no proven claim hands off a five-hour wall"
    );
}
