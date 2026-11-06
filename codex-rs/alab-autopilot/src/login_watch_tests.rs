// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE LOST LOGIN'S WATCH at its edges (gap 13 stage 1; review of the stage, findings 1, 3 and
//! 4), against alab-aterm's in-process FAKE aterm (never a real one) and codex's own
//! `AuthManager` over a scratch `CODEX_HOME` in File mode, with fake `sk-test-*` keys written
//! by codex's own login code ([`crate::login::fixtures`]); no thread manager, so nothing is
//! typed here, and a decision is read from the thread's state:
//!
//! These component tests explicitly bind a proven request's provider and auth scope. A saved
//! API-key wall alone supplies no such proof; a separate negative control checks that boundary.
//!
//! - a proven login wall without a retained baseline takes the login as it stands: its first
//!   decision reads nothing back and leaves the ask (no blind line), and only a later renewal
//!   reads back;
//! - a turn's start ends the watch, none is armed while the turn runs, and the next decision
//!   after it arms one again;
//! - a watch armed after a passive spell compares with the stat the last one read, so a file
//!   that moved once is reloaded once.
//!
//! The looks run every [`LOOK`] here (aterm's are 15 s apart); a test that says nothing came of
//! them waits for looks to have passed, never for time.

use super::*;
use crate::decide;
use crate::decide::Act;
use crate::login::fixtures;
use crate::words;
use codex_login::AuthCredentialsStoreMode;

/// How far apart the watch's looks are here.
const LOOK: Duration = Duration::from_millis(25);

/// The wall's own key and the renewed one (fake keys).
const OLD_KEY: &str = "sk-test-old";
const NEW_KEY: &str = "sk-test-new";

/// A thread under a proven claim whose codex holds [`OLD_KEY`] from a scratch home's login
/// file, its looks [`LOOK`] apart.
async fn on_a_login() -> (Harness, tempfile::TempDir, Arc<codex_login::AuthManager>) {
    login_fixture(true).await
}

async fn login_fixture(
    proven: bool,
) -> (Harness, tempfile::TempDir, Arc<codex_login::AuthManager>) {
    let home = fixtures::home();
    fixtures::api_key(home.path(), OLD_KEY);
    let manager = fixtures::manager(home.path(), AuthCredentialsStoreMode::File).await;
    let mut config = core_test_support::load_default_config_for_test(&home).await;
    config.model_provider.name = "OpenAI".into();
    config.model_provider.requires_openai_auth = true;
    config.model_provider.env_key = None;
    config.model_provider.experimental_bearer_token = None;
    config.model_provider.auth = None;
    config.model_provider.aws = None;
    let h = started_timed_with_config(
        &[],
        |_, _| {},
        true,
        ClientOptions::default(),
        Timing {
            login_look: LOOK,
            ..Timing::default()
        },
        Some(Arc::new(config)),
    )
    .await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    h.autopilot.set_auth_manager(Arc::clone(&manager));
    if proven {
        let auth = manager.auth_cached().expect("fixture auth");
        let revision = manager
            .auth_change_state_for(&auth)
            .expect("fixture revision");
        let owner = Arc::clone(&manager);
        let scope = codex_alab_core::renewal::AuthRequestScope::new(
            "fixture-request".into(),
            Some(codex_alab_core::renewal::RequestAuth::OpenAiApiKey),
            revision.owner_generation,
            revision.generation,
            move || {
                owner
                    .auth_cached()
                    .and_then(|auth| owner.auth_change_state_for(&auth))
                    .is_some_and(|now| now == revision)
            },
        );
        h.rt.set_login_scope_for_test(Arc::new(scope));
    }
    (h, home, manager)
}

/// Restore only the component's wall state, without its ephemeral baseline. The
/// positive fixture supplies request provenance separately; this is not evidence
/// that a restarted process may infer API-key provenance from a saved wall.
fn restored_wall(h: &Harness) {
    point(
        h,
        TurnEnd {
            restored: true,
            ..ended(None, Some(Wall::Login))
        },
    );
}

/// How many journal lines say `needle`.
fn notes(h: &Harness, needle: &str) -> usize {
    h.sup
        .journal()
        .iter()
        .filter(|line| line.contains(needle))
        .count()
}

/// The key codex holds now.
fn key_held(manager: &codex_login::AuthManager) -> Option<String> {
    manager
        .auth_cached()
        .and_then(|auth| auth.api_key().map(str::to_owned))
}

/// Wait for `n` more of the watches' looks.
async fn looks_pass(h: &Harness, n: u64) {
    let from = h.autopilot.login_looks();
    wait_until("the watch's looks", || {
        h.autopilot.login_looks() >= from + n
    })
    .await;
}

/// The act the thread's state decides now.
fn decided(h: &Harness) -> Act {
    let policy = h
        .sup
        .policy()
        .unwrap_or_else(|| panic!("a proven claim's policy"));
    decide::decide(
        &h.rt.state(),
        &policy,
        &Timing::default(),
        Instant::now(),
        Duration::ZERO,
    )
    .act
}

/// A proven wall whose ephemeral baseline is absent takes the current cache as
/// its baseline. This component check does not grant restart provenance: the
/// negative control below rejects an unproven restored API-key wall.
/// NEGATIVE CONTROL: the key renewed after it reads back once codex reloaded it, and the
/// decision then is the lost login's line.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_proven_wall_without_a_baseline_reads_back_only_after_a_later_renewal() {
    let (h, home, manager) = on_a_login().await;
    restored_wall(&h);
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert_eq!(notes(&h, "a login wall with no baseline"), 1);
    assert!(!h.rt.state().login.back, "{:?}", h.sup.journal());
    wait_until("the login's ask", || {
        shown(&h).is_some_and(|text| text.starts_with("login: "))
    })
    .await;
    assert!(
        matches!(decided(&h), Act::Attention { .. }),
        "{:?}",
        decided(&h)
    );

    fixtures::api_key(home.path(), NEW_KEY);
    wait_until("the thread looked again at the renewal", || {
        notes(&h, "looked again at the lost login's return") == 1
    })
    .await;
    assert!(
        notes(
            &h,
            "codex reloaded the login: the same identity's login is back"
        ) >= 1
    );
    assert_eq!(key_held(&manager).as_deref(), Some(NEW_KEY));
    assert!(h.rt.state().login.back);
    assert!(
        matches!(decided(&h), Act::Type { rule, .. } if rule == words::RULE_LOGIN_BACK),
        "{:?}",
        decided(&h)
    );
    assert_eq!(notes(&h, "a login wall with no baseline"), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unproven_restored_api_key_wall_does_not_arm_a_watch() {
    let (h, _home, manager) = login_fixture(false).await;
    restored_wall(&h);
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert!(!h.rt.state().login.back);
    assert!(h.rt.login_watch_for_test().is_none());
    assert_eq!(notes(&h, "the lost login's file is watched"), 0);
    assert_eq!(key_held(&manager).as_deref(), Some(OLD_KEY));
}

/// A TURN'S START ENDS THE WATCH (a person's, a goal's, codex-alab's own line), so no reload of
/// the login lands inside it, and no watch is armed while it runs. NEGATIVE CONTROL: once the
/// turn is over the next decision arms one again, and the key renewed meanwhile reads back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turns_start_ends_the_lost_logins_watch() {
    let (h, home, manager) = on_a_login().await;
    restored_wall(&h);
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert_eq!(
        notes(&h, "the lost login's file is watched while the wall stands"),
        1
    );
    assert!(h.shared.turn_started(h.rt.key(), "a-persons-turn"));
    wait_until("the watch ended", || {
        notes(&h, "a turn runs; the lost login's watch ends") == 1
    })
    .await;
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert_eq!(
        notes(&h, "the lost login's file is watched while the wall stands"),
        1,
        "no watch while the turn runs"
    );
    fixtures::api_key(home.path(), NEW_KEY);
    assert_eq!(key_held(&manager).as_deref(), Some(OLD_KEY));

    h.shared.turn_ended(h.rt.key(), "a-persons-turn");
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert_eq!(
        notes(&h, "the lost login's file is watched while the wall stands"),
        2
    );
    wait_until("the renewal read after the turn", || {
        notes(
            &h,
            "codex reloaded the login: the same identity's login is back",
        ) >= 1
    })
    .await;
    assert_eq!(key_held(&manager).as_deref(), Some(NEW_KEY));
}

/// A WATCH ARMED AFTER A PASSIVE SPELL RELOADS NO UNCHANGED FILE: the file moved once at the
/// wall (the same key saved again) and the watch reloaded it (once at the move, and again where
/// a folder event named the file); the watch armed when codex-alab acts again compares with the
/// stat that one read, not the wall's, so it reloads nothing more: a reload in keyring or auto
/// mode reads the macOS keychain. NEGATIVE CONTROL: the first watch's reload, at the move.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_watch_armed_after_a_passive_spell_reloads_no_unchanged_file() {
    let (h, home, _manager) = on_a_login().await;
    restored_wall(&h);
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    fixtures::api_key(home.path(), OLD_KEY);
    let rewritten = crate::login::stat(&codex_alab_core::home::auth_file(home.path()));
    assert!(rewritten.is_some());
    wait_until("codex observed the rewritten file", || {
        h.rt.login_watch_for_test() == Some((rewritten, true))
    })
    .await;

    h.fake.set_effective(false, "autopilot-off");
    wait_until("the watch ended at the Off", || {
        notes(&h, "passive; the lost login's watch ends") == 1
    })
    .await;
    // The first watch has ended: no reload of its own comes after this.
    let reloads = notes(&h, "codex reloaded the login");
    act_again(&h).await;
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert_eq!(
        notes(&h, "the lost login's file is watched while the wall stands"),
        2
    );
    looks_pass(&h, 3).await;
    assert_eq!(
        notes(&h, "codex reloaded the login"),
        reloads,
        "{:?}",
        h.sup.journal()
    );
}
