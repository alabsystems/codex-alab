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
use codex_alab_core::quota::SpentWall;
use codex_alab_core::quota::SpentWindow;
use codex_alab_core::quota::WallHandoff;
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

/// The fake aterm shows this process a pool of `rows`, naming `own` as its session's row
/// (a real aterm names none yet).
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
    turn.get::<WallHandoff>()
        .unwrap_or_else(|| panic!("the started turn carries no wall handoff"))
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

/// Wait (a hang detector) until the handoff says rotation would follow `wall`.
async fn ready_at(h: &Harness, handoff: &WallHandoff, wall: SpentWall, what: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while !settled_at(h, handoff, wall).await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "never ready: {what}: {:#?}",
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
