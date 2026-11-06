// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The lost login's read and reload (gap 13 stage 1) against codex's own `AuthManager` over a
//! scratch `CODEX_HOME` in File mode, with ALab's switch on as the codex binary has it, its
//! logins written by codex's own login code ([`super::fixtures`]): fake `sk-test-*` keys and
//! fake JWTs only, never a real credential, never the keychain.

use codex_login::AuthCredentialsStoreMode;
use pretty_assertions::assert_eq;

use super::Baseline;
use super::Read;
use super::fixtures::api_key;
use super::fixtures::chatgpt;
use super::fixtures::home;
use super::stat;

/// Codex's manager over the scratch home's login file.
async fn manager(home: &std::path::Path) -> std::sync::Arc<codex_login::AuthManager> {
    super::fixtures::manager(home, AuthCredentialsStoreMode::File).await
}

/// A RENEWED KEY READS BACK, ONCE CODEX RELOADED IT: the look's stat moved with the save, and
/// only codex's reload makes the cache say so. NEGATIVE CONTROLS: the wall's own login reads as
/// standing, a rewrite of the same key moves the stat but reads as standing after its reload,
/// and the renewed key unreloaded reads as standing (the read is codex's cache, never the file).
#[tokio::test]
async fn a_renewed_key_reads_back_once_codex_reloaded_it() {
    let home = home();
    let file = home.path().join("auth.json");
    api_key(home.path(), "sk-test-old");
    let manager = manager(home.path()).await;
    let baseline = Baseline::of(&manager);
    assert_eq!(baseline.file(), file.as_path());
    assert!(baseline.stat().is_some());
    assert_eq!(baseline.read(&manager), Read::Stands);

    api_key(home.path(), "sk-test-old");
    let inode = |stat: Option<super::Stat>| stat.map(|stat| stat.ino);
    assert_ne!(
        inode(stat(&file)),
        inode(baseline.stat()),
        "a save renames a new file over"
    );
    manager.reload().await;
    assert_eq!(baseline.read(&manager), Read::Stands, "the same key again");

    api_key(home.path(), "sk-test-new");
    assert_eq!(baseline.read(&manager), Read::Stands, "not reloaded yet");
    manager.reload().await;
    assert_eq!(baseline.read(&manager), Read::Back);
}

/// ANOTHER ACCOUNT'S LOGIN IS NOT BACK: a ChatGPT login of another user and account, renewed in
/// the walled login's place, reads as another identity's; so does a login of another auth mode.
/// NEGATIVE CONTROL: the walled account's own login renewed (new tokens, the same user and
/// account) reads back.
#[tokio::test]
async fn another_accounts_login_reads_other_and_the_same_accounts_reads_back() {
    let home = home();
    chatgpt(home.path(), "user-a", "account-a", "refresh-1", None);
    let manager = manager(home.path()).await;
    let baseline = Baseline::of(&manager);
    chatgpt(home.path(), "user-b", "account-b", "refresh-2", None);
    manager.reload().await;
    assert_eq!(baseline.read(&manager), Read::Other);
    api_key(home.path(), "sk-test-other");
    manager.reload().await;
    assert_eq!(baseline.read(&manager), Read::Other, "another auth mode");
    chatgpt(home.path(), "user-a", "account-a", "refresh-3", None);
    manager.reload().await;
    assert_eq!(baseline.read(&manager), Read::Back);
}

/// CODEX TAKES ONLY THE WALLED IDENTITY'S LOGIN ([`Baseline::reload`]): a ChatGPT login of
/// another account, of another user in the walled account, or of another auth mode, saved in the
/// walled login's place, is left in the store, and the manager keeps the walled login, as
/// codex's own 401 recovery keeps it; so does a login signed out. NEGATIVE CONTROL: the walled
/// account's own login renewed is taken, and reads back.
#[tokio::test]
async fn the_reload_takes_only_the_walled_identitys_login() {
    let home = home();
    let file = home.path().join("auth.json");
    chatgpt(home.path(), "user-a", "account-a", "refresh-1", None);
    let manager = manager(home.path()).await;
    let baseline = Baseline::of(&manager);
    let held = || {
        manager
            .auth_cached()
            .map(|auth| (auth.get_chatgpt_user_id(), auth.get_account_id()))
    };
    let walled = Some((Some("user-a".to_owned()), Some("account-a".to_owned())));
    chatgpt(home.path(), "user-b", "account-b", "refresh-2", None);
    assert!(!baseline.reload(&manager).await, "another account's");
    assert_eq!(held(), walled);
    chatgpt(home.path(), "user-c", "account-a", "refresh-3", None);
    assert!(!baseline.reload(&manager).await, "another user's");
    assert_eq!(held(), walled);
    api_key(home.path(), "sk-test-other");
    assert!(!baseline.reload(&manager).await, "another auth mode's");
    assert_eq!(held(), walled);
    std::fs::remove_file(&file).unwrap_or_else(|err| panic!("{err}"));
    assert!(!baseline.reload(&manager).await, "none");
    assert_eq!(held(), walled);
    assert_eq!(baseline.read(&manager), Read::Stands);
    chatgpt(home.path(), "user-a", "account-a", "refresh-4", None);
    assert!(baseline.reload(&manager).await, "the walled account's own");
    assert_eq!(held(), walled);
    assert_eq!(baseline.read(&manager), Read::Back);
}

/// A LOGIN SIGNED OUT IS NOT BACK: the store emptied after the wall (a logout elsewhere)
/// advances the generation, but nothing is cached to come back.
#[tokio::test]
async fn a_login_signed_out_since_the_wall_stands() {
    let home = home();
    let file = home.path().join("auth.json");
    api_key(home.path(), "sk-test-old");
    let manager = manager(home.path()).await;
    let baseline = Baseline::of(&manager);
    std::fs::remove_file(&file).unwrap_or_else(|err| panic!("{err}"));
    manager.reload().await;
    assert!(manager.auth_cached().is_none());
    assert_eq!(baseline.read(&manager), Read::Stands);
}

/// A RENEWED LOGIN WHOSE REFRESH CODEX REFUSED IS NOT BACK: the walled account's login renewed
/// reads back until codex's own refresh of it is refused for good (the token authority, a mock
/// here, answers `refresh_token_expired`), which codex records for that credential; then it
/// stands, as the wall's own refused login did. NEGATIVE CONTROL: the read before the refusal.
#[tokio::test]
async fn a_renewed_login_whose_refresh_codex_refused_stands() {
    assert!(
        std::env::var_os(codex_login::REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR).is_none(),
        "CODEX_REFRESH_TOKEN_URL_OVERRIDE is set: the refresh would go past the mock authority"
    );
    let authority = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/oauth/token"))
        .respond_with(
            wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": {"code": "refresh_token_expired"}
            })),
        )
        .expect(1)
        .mount(&authority)
        .await;
    let home = home();
    chatgpt(home.path(), "user-a", "account-a", "refresh-1", None);
    let manager = manager(home.path()).await;
    let baseline = Baseline::of(&manager);
    chatgpt(
        home.path(),
        "user-a",
        "account-a",
        "refresh-2",
        Some(&authority.uri()),
    );
    manager.reload().await;
    assert_eq!(baseline.read(&manager), Read::Back, "nothing refused yet");
    assert!(
        manager.refresh_token_from_authority().await.is_err(),
        "the authority refuses the refresh"
    );
    let renewed = manager
        .auth_cached()
        .unwrap_or_else(|| panic!("the renewed login is cached"));
    assert!(manager.refresh_failure_for_auth(&renewed).is_some());
    assert_eq!(baseline.read(&manager), Read::Stands);
}

/// A replacement can be on disk before the wall's baseline or even its first
/// file observation. The failed attempt, not the later cache observation, owns
/// the baseline. Identical reloading is the negative control.
#[tokio::test]
async fn request_baseline_recovers_a_file_already_present_before_the_wall() {
    use codex_alab_core::renewal::AuthRequestScope;
    use codex_alab_core::renewal::RequestAuth;
    let home = home();
    api_key(home.path(), "sk-test-old");
    let manager = manager(home.path()).await;
    assert!(
        manager.alab_uses_file_auth(),
        "real file mode supports initial reconciliation"
    );
    let ephemeral =
        super::fixtures::manager(home.path(), AuthCredentialsStoreMode::Ephemeral).await;
    assert!(
        !ephemeral.alab_uses_file_auth(),
        "an existing file cannot select an ephemeral manager's store"
    );
    let generation = manager.auth_change_state_receiver().borrow().generation;
    api_key(home.path(), "sk-test-new");
    let scope = AuthRequestScope::new(
        "failed".into(),
        Some(RequestAuth::OpenAiApiKey),
        0,
        generation,
        || true,
    );
    scope.observe_file(&home.path().join("auth.json"));
    let baseline = Baseline::from_request(&manager, &scope);
    assert_eq!(baseline.read(&manager), Read::Stands);
    assert!(baseline.reload(&manager).await);
    assert_eq!(baseline.read(&manager), Read::Back);
    let attempted_new = AuthRequestScope::new(
        "failed".into(),
        Some(RequestAuth::OpenAiApiKey),
        0,
        manager.auth_change_state_receiver().borrow().generation,
        || true,
    );
    attempted_new.observe_file(&home.path().join("auth.json"));
    let refused_new = Baseline::from_request(&manager, &attempted_new);
    assert!(refused_new.reload(&manager).await);
    assert_eq!(
        refused_new.read(&manager),
        Read::Stands,
        "already tried credential is not a recovery"
    );
}

/// The opaque bearer comparison must not lend another publication's freshness
/// to the identity snapshot that Baseline already read.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn request_baseline_rejects_a_cache_publication_inside_credential_comparison() {
    use codex_alab_core::renewal::AuthRequestScope;
    use codex_alab_core::renewal::RequestAuth;
    use codex_alab_core::renewal::record_auth_attempt;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    let home = std::sync::Arc::new(home());
    api_key(home.path(), "sk-test-attempted");
    let manager = manager(home.path()).await;
    let revision = *manager.auth_change_state_receiver().borrow();
    let scope = std::sync::Arc::new(AuthRequestScope::new(
        "coherent-baseline".into(),
        Some(RequestAuth::OpenAiApiKey),
        revision.owner_generation,
        revision.generation,
        || true,
    ));
    scope.register(std::sync::Arc::clone(&manager));
    let publish_manager = std::sync::Arc::clone(&manager);
    let publish_home = std::sync::Arc::clone(&home);
    let published = std::sync::Arc::new(AtomicBool::new(false));
    let once = std::sync::Arc::clone(&published);
    record_auth_attempt(
        std::sync::Arc::as_ptr(&manager) as usize,
        "coherent-baseline",
        Some(RequestAuth::OpenAiApiKey),
        revision.owner_generation,
        revision.generation,
        || true,
        move || {
            if !once.swap(true, Ordering::SeqCst) {
                super::fixtures::chatgpt(
                    publish_home.path(),
                    "other-user",
                    "other-account",
                    "other-refresh",
                    None,
                );
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(publish_manager.reload());
                });
            }
            true
        },
    );
    let baseline = Baseline::from_request(&manager, &scope);
    api_key(home.path(), "sk-test-fresh-same-mode");
    manager.reload().await;
    assert_eq!(
        baseline.read(&manager),
        Read::Stands,
        "mixed publications grant nothing"
    );
    assert!(published.load(Ordering::SeqCst));
    assert_eq!(
        baseline.read(&manager),
        Read::Other,
        "the next read observes the actual other identity"
    );
}

#[test]
fn stat_rejects_symlinks_and_directories() {
    let home = home();
    let dir = home.path().join("dir-auth.json");
    std::fs::create_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(stat(&dir), None);

    #[cfg(unix)]
    {
        let target = home.path().join("real-auth.json");
        std::fs::write(&target, "{}\n").unwrap_or_else(|err| panic!("{err}"));
        let link = home.path().join("link-auth.json");
        std::os::unix::fs::symlink(&target, &link).unwrap_or_else(|err| panic!("{err}"));
        assert!(stat(&target).is_some());
        assert_eq!(stat(&link), None);
    }
}
