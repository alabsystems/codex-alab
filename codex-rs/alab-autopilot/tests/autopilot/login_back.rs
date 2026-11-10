// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE LOST LOGIN COMES BACK end to end (gap 13 stage 1, aterm's `RULE_LOGIN_BACK`): a turn the
//! model refuses for its login (401) stands at the login wall with today's ask; once the same
//! login is renewed in the login file (by codex's own login code, as `codex login` elsewhere
//! writes it) codex reloads it and retries the original native request once on
//! the renewed credential. The login is a scratch `CODEX_HOME`'s, in File mode (one test's is in
//! ephemeral storage), with fake keys only ([`fixtures`], the unit tests' own copy): never a
//! real credential, never the keychain.
//!
//! The looks run every [`LOOK`] here (aterm's are 15 s apart), and two tests set them an
//! [`HOUR`] apart so that a wake of the watch alone triggers recovery; a test that says nothing
//! came of a login waits for looks to have passed, or for codex's reload to have been read,
//! never for time.
//!
//! THE CODEX AUTH ENVIRONMENT: each test refuses to run where `CODEX_AUTH_PATH` is set, its
//! manager reads no key from the environment (`enable_codex_api_key_env` off), and no test in
//! this binary sets a codex auth variable, so they need no serial lane; one that sets one runs
//! serially with these.

use std::path::PathBuf;

use codex_alab_autopilot::words;
use codex_login::AuthCredentialsStoreMode;
use codex_login::AuthManager;

use super::*;

use super::login_fixtures as fixtures;

/// How far apart the watch's looks are here.
const LOOK: Duration = Duration::from_millis(25);

/// Looks so far apart that none runs in a test: the watch's wakes alone deliver.
const HOUR: Duration = Duration::from_secs(3600);

/// The wall's own key and the renewed one (fake keys).
const OLD_KEY: &str = "sk-test-old";
const NEW_KEY: &str = "sk-test-new";

/// The model's refusal of the login: an expired or revoked key.
fn refused() -> wiremock::ResponseTemplate {
    wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({
        "error": {
            "message": "Incorrect API key provided.",
            "type": "invalid_request_error",
            "code": "invalid_api_key"
        }
    }))
}

/// The scratch codex home and the manager over its login.
struct Scratch {
    home: Arc<tempfile::TempDir>,
    manager: Arc<AuthManager>,
}

/// The scratch `home` logged in with [`OLD_KEY`] in `mode`'s store, and codex's manager over
/// it.
async fn scratch(home: tempfile::TempDir, mode: AuthCredentialsStoreMode) -> Scratch {
    let home = Arc::new(home);
    fixtures::api_key_in(home.path(), OLD_KEY, mode);
    let manager = fixtures::manager(home.path(), mode).await;
    Scratch { home, manager }
}

/// A FOLDER WHOSE CHANGES THE OS REPORTS: the temporary folder, outside any `.ubd` folder. On
/// macOS fseventsd drops the events of a path holding `/.ubd/`, and aterm's test runs keep
/// their scratch in one (`$TMPDIR/.ubd`, aterm's `docs/measured/fseventsd-ubd-2026-10-26.md`).
/// A real login's folder is in none.
fn reported_root() -> PathBuf {
    let mut root = std::env::temp_dir();
    while root.components().any(|part| part.as_os_str() == ".ubd") {
        root.pop();
    }
    root
}

/// [`walled_on`] a File-mode scratch home, its looks [`LOOK`] apart.
async fn walled(replies: Vec<wiremock::ResponseTemplate>) -> (Harness, Scratch) {
    let scratch = scratch(fixtures::home(), AuthCredentialsStoreMode::File).await;
    walled_on(scratch, replies, LOOK).await
}

/// A turn on [`OLD_KEY`] the model refuses, standing at the login wall with today's ask and the
/// login's watch armed, its looks `look` apart; `replies` are the model's after the refusal.
async fn walled_on(
    scratch: Scratch,
    replies: Vec<wiremock::ResponseTemplate>,
    look: Duration,
) -> (Harness, Scratch) {
    let h = harness_login(
        Mode::Current,
        std::iter::once(refused()).chain(replies).collect(),
        &[("human_grace_s", "0"), ("continue", "false")],
        &[],
        Some(0),
        Some(TestLogin::Manager {
            manager: Arc::clone(&scratch.manager),
            home: Arc::clone(&scratch.home),
        }),
        Timing {
            min_work: Duration::ZERO,
            login_look: look,
            ..Timing::default()
        },
        |_| {},
        None,
    )
    .await;
    assert!(
        h.fake
            .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
            .await
    );
    h.test
        .submit_turn("do the task")
        .await
        .unwrap_or_else(|err| panic!("first turn: {err}"));
    assert!(h.fake.wait_for(asked).await, "the login's ask");
    wait_until("the login's watch", || {
        notes(&h, "the lost login's file is watched while the wall stands") == 1
    })
    .await;
    assert_eq!(bearers(&h), [Some(OLD_KEY.to_owned())]);
    if scratch.manager.alab_uses_file_auth() {
        // The first selected-file reconciliation closes the pre-watch arrival
        // window. Later assertions measure only reads caused after that event.
        wait_until("initial file reconciliation completed", || {
            notes(&h, "codex reloaded the login: no new login") >= 1
        })
        .await;
    }
    (h, scratch)
}

/// The key codex holds now.
fn key_held(manager: &AuthManager) -> Option<String> {
    manager
        .auth_cached()
        .and_then(|auth| auth.api_key().map(str::to_owned))
}

/// The original turn went through once on the renewed key: the ask is down,
/// exactly one retry carries the renewed key and its input is unchanged.
async fn delivered_once(h: &Harness) {
    wait_until("the line's turn took the ask down", || {
        !asked(&h.fake) && notes(h, "nothing (continue is off)") >= 1
    })
    .await;
    assert_eq!(
        bearers(h),
        [Some(OLD_KEY.to_owned()), Some(NEW_KEY.to_owned())]
    );
    let input = h
        .mock
        .requests()
        .last()
        .and_then(|request| request.message_input_texts("user").last().cloned())
        .unwrap_or_default();
    assert_eq!(input, "do the task", "the original request is retained");
    assert_eq!(
        h.mock.requests()[0].body_json()["input"],
        h.mock.requests()[1].body_json()["input"],
        "the failed native request is recovered without a replacement prompt"
    );
    assert_eq!(login_backs(h), 1);
    assert_eq!(h.autopilot.supervisor().standing_attention(), None);
}

/// Today's ask of the login wall stands.
fn asked(fake: &FakeAterm) -> bool {
    fake.attention()
        .values()
        .any(|text| text.contains("sign-in was refused or expired"))
}

/// How many journal lines say `needle`.
fn notes(h: &Harness, needle: &str) -> usize {
    h.autopilot
        .supervisor()
        .journal()
        .iter()
        .filter(|line| line.contains(needle))
        .count()
}

/// The key each request to the model carried.
fn bearers(h: &Harness) -> Vec<Option<String>> {
    h.mock
        .requests()
        .iter()
        .map(|request| {
            request
                .header("authorization")
                .and_then(|value| value.strip_prefix("Bearer ").map(str::to_owned))
        })
        .collect()
}

/// How many times the lost login's line was typed.
fn login_backs(h: &Harness) -> usize {
    h.fake
        .acts()
        .iter()
        .filter(|act| *act == words::RULE_LOGIN_BACK)
        .count()
}

/// Wait for `n` more of the watch's looks.
async fn looks_pass(h: &Harness, n: u64) {
    let from = h.autopilot.login_looks();
    wait_until("the watch's looks", || {
        h.autopilot.login_looks() >= from + n
    })
    .await;
}

/// THE RENEWED LOGIN'S LINE, ONCE (CONTRACT §5, the login wall row): the walled key renewed in
/// the login file, codex reloads it, and exactly one request goes out on the renewed key, whose
/// input is aterm's line; its turn goes through and takes the ask down, and nothing more is
/// typed. NEGATIVE CONTROLS: the four tests after it.
#[test]
fn an_expired_then_renewed_login_types_login_back_exactly_once() {
    big_stack(|| async {
        let (h, scratch) = walled(vec![responses::sse_response(reply(
            "r2",
            "Signed in again; carrying on.",
        ))])
        .await;
        fixtures::api_key(scratch.home.path(), NEW_KEY);
        delivered_once(&h).await;
        assert!(
            notes(
                &h,
                "codex reloaded the login: the same identity's login is back"
            ) >= 1
        );
    });
}

/// THE FOLDER'S WATCH ALONE DELIVERS THE RENEWAL (spec decision 2): with the looks an [`HOUR`]
/// apart, the save's change in the login file's folder (`login::FolderWatch`) wakes the watch,
/// codex reloads the login, and the line goes out once on the renewed key, no look taken. The
/// home is in a folder whose changes the OS reports ([`reported_root`]), as a real login's is.
#[test]
fn the_folder_watch_alone_delivers_the_renewed_login() {
    big_stack(|| async {
        let home = fixtures::home_in(&reported_root());
        let scratch = scratch(home, AuthCredentialsStoreMode::File).await;
        let (h, scratch) = walled_on(
            scratch,
            vec![responses::sse_response(reply(
                "r2",
                "Signed in again; carrying on.",
            ))],
            HOUR,
        )
        .await;
        fixtures::api_key(scratch.home.path(), NEW_KEY);
        delivered_once(&h).await;
        assert_eq!(h.autopilot.login_looks(), 0);
    });
}

/// CODEX'S OWN CHANGE ALONE DELIVERS THE RENEWAL: a login in ephemeral storage (no file, so no
/// folder event and no stat to move) signed in again in this process as codex's own sign-in
/// does it (its save, then its reload) wakes the watch through codex's change of the login
/// (`auth_change_state_receiver`), and the line goes out once on the renewed key, no look taken
/// and no reload of the watch's own.
#[test]
fn codexs_own_change_alone_delivers_the_renewed_login() {
    big_stack(|| async {
        let scratch = scratch(fixtures::home(), AuthCredentialsStoreMode::Ephemeral).await;
        let (h, scratch) = walled_on(
            scratch,
            vec![responses::sse_response(reply(
                "r2",
                "Signed in again; carrying on.",
            ))],
            HOUR,
        )
        .await;
        fixtures::api_key_in(
            scratch.home.path(),
            NEW_KEY,
            AuthCredentialsStoreMode::Ephemeral,
        );
        scratch.manager.reload().await;
        delivered_once(&h).await;
        assert_eq!(h.autopilot.login_looks(), 0);
        assert_eq!(notes(&h, "the lost login's file changed"), 0);
    });
}

/// After initial reconciliation, changes that move no stat reload nothing: the file's mode
/// changed in a folder whose
/// changes the OS reports names the file in an event but moves none of the stat's fields
/// (device, inode, size, modified time), so codex reloads nothing and nothing is typed; the
/// renewal after it is the one reload. A change reported late (the fixture's own save, arriving
/// once the watch stands) is the same case: it failed `an_unrenewed_login_types_nothing` once at
/// a machine's load of ~100. NEGATIVE CONTROL: reloading on an event that names the file (the
/// watch before this fix) reloads at the mode's change. Drive metadata changes until a real
/// wake arrives: filesystem events are hints and a single chmod can be coalesced away.
#[test]
fn a_change_that_moves_no_stat_reloads_nothing() {
    big_stack(|| async {
        use std::os::unix::fs::PermissionsExt;
        let home = fixtures::home_in(&reported_root());
        let scratch = scratch(home, AuthCredentialsStoreMode::File).await;
        let (h, scratch) = walled_on(
            scratch,
            vec![responses::sse_response(reply(
                "r2",
                "Signed in again; carrying on.",
            ))],
            HOUR,
        )
        .await;
        let initial_reads = notes(&h, "the lost login's file changed");
        let file = scratch.home.path().join("auth.json");
        let wakes = h.autopilot.login_wakes();
        let stat = codex_alab_autopilot::login::stat(&file);
        assert!(stat.is_some());
        let mode = std::cell::Cell::new(0o600);
        wait_until("the mode's change in the folder", || {
            if h.autopilot.login_wakes() > wakes {
                return true;
            }
            mode.set(mode.get() ^ 0o040);
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode.get()))
                .unwrap_or_else(|err| panic!("the login file's mode: {err}"));
            assert_eq!(codex_alab_autopilot::login::stat(&file), stat);
            false
        })
        .await;
        assert_eq!(notes(&h, "the lost login's file changed"), initial_reads);
        assert_eq!(bearers(&h).len(), 1);
        fixtures::api_key(scratch.home.path(), NEW_KEY);
        delivered_once(&h).await;
        assert_eq!(
            notes(&h, "the lost login's file changed"),
            initial_reads + 1
        );
        assert_eq!(h.autopilot.login_looks(), 0);
    });
}

/// After initial reconciliation, looks over the unchanged file cause no further
/// reloads or model requests, and the ask stands.
#[test]
fn an_unrenewed_login_types_nothing() {
    big_stack(|| async {
        let (h, _scratch) = walled(Vec::new()).await;
        let initial_reads = notes(&h, "codex reloaded the login");
        looks_pass(&h, 4).await;
        assert_eq!(notes(&h, "codex reloaded the login"), initial_reads);
        assert_eq!(bearers(&h).len(), 1);
        assert_eq!(login_backs(&h), 0);
        assert!(asked(&h.fake));
    });
}

/// THE SAME LOGIN WRITTEN AGAIN TYPES NOTHING: the walled key saved again moves the file, codex
/// reloads it and reads nothing new (its change generation stands), and the ask stands.
#[test]
fn a_rewrite_of_the_same_login_types_nothing() {
    big_stack(|| async {
        let (h, scratch) = walled(Vec::new()).await;
        let initial_reads = notes(&h, "codex reloaded the login: no new login");
        fixtures::api_key(scratch.home.path(), OLD_KEY);
        wait_until("codex reloaded the rewritten login", || {
            notes(&h, "codex reloaded the login: no new login") > initial_reads
        })
        .await;
        looks_pass(&h, 2).await;
        assert_eq!(notes(&h, "the lost login is back"), 0);
        assert_eq!(bearers(&h).len(), 1);
        assert_eq!(login_backs(&h), 0);
        assert!(asked(&h.fake));
    });
}

/// THE RENEWED LOGIN REFUSED AGAIN TYPES NOTHING MORE: the line's turn meets the login wall
/// again, and the ask stands; a second renewal reads back once more, the thread looks again,
/// and nothing more is typed for that wall (aterm's `back_typed`).
#[test]
fn a_renewed_login_refused_again_types_nothing_more() {
    big_stack(|| async {
        let (h, scratch) = walled(vec![refused()]).await;
        fixtures::api_key(scratch.home.path(), NEW_KEY);
        wait_until("the line's turn met the wall again", || {
            notes(&h, "the lost login's file is watched while the wall stands") == 2
        })
        .await;
        assert_eq!(
            bearers(&h),
            [Some(OLD_KEY.to_owned()), Some(NEW_KEY.to_owned())]
        );
        assert_eq!(login_backs(&h), 1);
        assert!(asked(&h.fake));
        fixtures::api_key(scratch.home.path(), "sk-test-newer");
        wait_until("the thread looked again at the second return", || {
            notes(&h, "looked again at the lost login's return") == 2
        })
        .await;
        looks_pass(&h, 2).await;
        assert_eq!(bearers(&h).len(), 2);
        assert_eq!(login_backs(&h), 1);
        assert!(asked(&h.fake));
    });
}

/// ANOTHER ACCOUNT'S LOGIN TYPES NOTHING, AND CODEX DOES NOT TAKE IT: a ChatGPT login of
/// another user and account saved in the walled key's place is left in the store, so codex goes
/// on holding the walled key, as its own 401 recovery would (`reload_if_account_id_matches`),
/// and the ask stands. NEGATIVE CONTROL: the renewed key's test above, whose reload is taken.
#[test]
fn another_accounts_login_types_nothing() {
    big_stack(|| async {
        let (h, scratch) = walled(Vec::new()).await;
        let initial_reads = notes(&h, "codex reloaded the login");
        fixtures::chatgpt(
            scratch.home.path(),
            "user-other",
            "account-other",
            "refresh-other",
            None,
        );
        wait_until("the watch read another account's login", || {
            notes(
                &h,
                "it holds another identity's login, or none, which codex does not take",
            ) >= 1
        })
        .await;
        looks_pass(&h, 2).await;
        assert_eq!(key_held(&scratch.manager).as_deref(), Some(OLD_KEY));
        assert_eq!(notes(&h, "codex reloaded the login"), initial_reads);
        assert_eq!(notes(&h, "the lost login is back"), 0);
        assert_eq!(bearers(&h).len(), 1);
        assert_eq!(login_backs(&h), 0);
        assert!(asked(&h.fake));
    });
}
