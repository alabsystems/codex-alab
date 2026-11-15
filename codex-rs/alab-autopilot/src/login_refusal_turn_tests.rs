// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE TURN ERROR'S REFRESH REFUSAL (the lost-login move), against alab-aterm's in-process FAKE
//! aterm (never a real one) and codex's own `AuthManager` over a scratch home's fake ChatGPT
//! login, refused by a mock token authority: the reason is remembered for the thread until the
//! next turn error, and a reason that is not `Expired` is kept as a veto that outlives it and
//! the process.

use super::*;
use crate::login;
use crate::login::fixtures;
use codex_extension_api::TurnErrorInput;
use codex_extension_api::TurnLifecycleContributor;
use codex_login::AuthCredentialsStoreMode;
use codex_protocol::auth::RefreshTokenFailedError;
use codex_protocol::auth::RefreshTokenFailedReason;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::protocol::CodexErrorInfo;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// `error` with `details` ends a turn of the harness's thread.
async fn turn_error(h: &Harness, turn: &str, error: CodexErrorInfo, details: &CodexErrorDetails) {
    let session = ExtensionData::new("session");
    let turn = ExtensionData::new(turn);
    h.autopilot
        .on_turn_error(TurnErrorInput {
            turn_id: turn.level_id(),
            error,
            error_details: details,
            session_store: &session,
            thread_store: &h._store,
            turn_store: &turn,
        })
        .await;
}

/// A REFRESH REFUSAL IS REMEMBERED UNTIL THE NEXT ERROR, ITS VETO FOR GOOD: codex's refresh of
/// the login was refused (`refresh_token_invalidated`), and the turn error that says so leaves
/// its reason on the thread and keeps a veto for the refused token. A later error that is no
/// refresh refusal takes the reason off the thread, but the veto stands, and a fresh manager
/// over the same home (a restarted process, which holds no refusal of its own) still reads it.
/// NEGATIVE CONTROL: before the error, nothing is known and nothing vetoes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refresh_refusal_is_remembered_until_the_next_error_and_its_veto_outlives_it() {
    let authority = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/oauth/token"))
        .respond_with(
            wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": {"code": "refresh_token_invalidated"}
            })),
        )
        .mount(&authority)
        .await;
    let home = fixtures::home();
    fixtures::chatgpt(
        home.path(),
        "user-a",
        "account-a",
        "refresh-refused",
        Some(&authority.uri()),
    );
    let manager = fixtures::manager(home.path(), AuthCredentialsStoreMode::File).await;
    let h = harness(&[]).await;
    h.autopilot.set_auth_manager(Arc::clone(&manager));
    assert_eq!(h.rt.login_refusal(), None);
    assert!(!h.autopilot.login_move_vetoed(&h.rt), "nothing known yet");

    assert!(manager.refresh_token_from_authority().await.is_err());
    let revoked = RefreshTokenFailedReason::Revoked;
    turn_error(
        &h,
        "refused-turn",
        CodexErrorInfo::Unauthorized,
        &CodexErrorDetails::RefreshTokenFailed(RefreshTokenFailedError::new(
            revoked,
            "fixture refusal",
        )),
    )
    .await;
    assert_eq!(h.rt.login_refusal(), Some(revoked));
    assert_eq!(login::refusal_kept(&manager), Some(revoked));
    assert!(h.autopilot.login_move_vetoed(&h.rt));

    turn_error(
        &h,
        "server-turn",
        CodexErrorInfo::InternalServerError,
        &CodexErrorDetails::InternalServerError,
    )
    .await;
    assert_eq!(
        h.rt.login_refusal(),
        None,
        "the next error is no refresh refusal"
    );
    assert_eq!(
        login::refusal_kept(&manager),
        Some(revoked),
        "the veto stands"
    );
    assert!(h.autopilot.login_move_vetoed(&h.rt));

    let restarted = fixtures::manager(home.path(), AuthCredentialsStoreMode::File).await;
    h.autopilot.set_auth_manager(restarted);
    assert!(
        h.autopilot.login_move_vetoed(&h.rt),
        "a process after a restart reads the kept veto"
    );
}

/// AN EXPIRED TOKEN IS REMEMBERED BUT KEPT AS NOTHING: the turn's reason is `Expired`, no veto
/// is written, and nothing vetoes the move (aterm's record decides it).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_expired_refusal_is_remembered_and_vetoes_nothing() {
    let authority = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/oauth/token"))
        .respond_with(
            wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": {"code": "refresh_token_expired"}
            })),
        )
        .mount(&authority)
        .await;
    let home = fixtures::home();
    fixtures::chatgpt(
        home.path(),
        "user-a",
        "account-a",
        "refresh-expired",
        Some(&authority.uri()),
    );
    let manager = fixtures::manager(home.path(), AuthCredentialsStoreMode::File).await;
    assert!(manager.refresh_token_from_authority().await.is_err());
    let h = harness(&[]).await;
    h.autopilot.set_auth_manager(Arc::clone(&manager));
    let expired = RefreshTokenFailedReason::Expired;
    turn_error(
        &h,
        "expired-turn",
        CodexErrorInfo::Unauthorized,
        &CodexErrorDetails::RefreshTokenFailed(RefreshTokenFailedError::new(
            expired,
            "fixture refusal",
        )),
    )
    .await;
    assert_eq!(h.rt.login_refusal(), Some(expired));
    assert_eq!(login::refusal_kept(&manager), None);
    assert!(!home.path().join("alab").join("login-refusals").exists());
    assert!(!h.autopilot.login_move_vetoed(&h.rt));
}

/// THE KEPT VETO IS WRITTEN OFF THE RUNTIME'S THREAD (review of the lost-login move): keeping
/// it makes the veto folder, prunes it and syncs the file and the folder, all blocking work, so
/// the turn error hands it to tokio's blocking pool. On a runtime with one thread, a task the
/// held write spawns onto that runtime runs while the write still waits for it, and the veto is
/// kept once the write goes on. Written on the runtime's own thread instead, the held write
/// would keep its task from running until it gave up, and the task would never be seen.
#[tokio::test(flavor = "current_thread")]
async fn the_kept_veto_is_written_off_the_runtime_thread() {
    let authority = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/oauth/token"))
        .respond_with(
            wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": {"code": "refresh_token_invalidated"}
            })),
        )
        .mount(&authority)
        .await;
    let home = fixtures::home();
    fixtures::chatgpt(
        home.path(),
        "user-a",
        "account-a",
        "refresh-refused",
        Some(&authority.uri()),
    );
    let manager = fixtures::manager(home.path(), AuthCredentialsStoreMode::File).await;
    assert!(manager.refresh_token_from_authority().await.is_err());
    let h = harness(&[]).await;
    h.autopilot.set_auth_manager(Arc::clone(&manager));
    let runtime = tokio::runtime::Handle::current();
    let seen = Arc::new(AtomicBool::new(false));
    let saw = Arc::clone(&seen);
    h.autopilot.before_refusal_keep(move || {
        let ran = Arc::new(AtomicBool::new(false));
        let marks = Arc::clone(&ran);
        runtime.spawn(async move { marks.store(true, Ordering::SeqCst) });
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ran.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        saw.store(ran.load(Ordering::SeqCst), Ordering::SeqCst);
    });
    let revoked = RefreshTokenFailedReason::Revoked;
    turn_error(
        &h,
        "refused-turn",
        CodexErrorInfo::Unauthorized,
        &CodexErrorDetails::RefreshTokenFailed(RefreshTokenFailedError::new(
            revoked,
            "fixture refusal",
        )),
    )
    .await;
    assert!(
        seen.load(Ordering::SeqCst),
        "the runtime's one thread ran a task while the veto was being written"
    );
    assert_eq!(h.rt.login_refusal(), Some(revoked));
    assert_eq!(login::refusal_kept(&manager), Some(revoked));
    assert!(h.autopilot.login_move_vetoed(&h.rt));
}
