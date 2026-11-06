// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE LOST LOGIN'S FIXTURES (gap 13 stage 1), one copy for the unit tests (`login_tests.rs`)
//! and the end-to-end ones (`tests/autopilot/login_back.rs`, by path): logins written into a
//! scratch `CODEX_HOME` by codex's own login code, fake `sk-test-*` keys and fake JWTs only,
//! never a real credential and never the keychain, and codex's manager over them.

use std::path::Path;
use std::sync::Arc;

use base64::Engine as _;
use codex_login::AuthCredentialsStoreMode;
use codex_login::AuthDotJson;
use codex_login::AuthKeyringBackendKind;
use codex_login::AuthManager;
use codex_login::TokenData;
use codex_protocol::auth::AuthMode;

/// [`home_in`] the system's temporary folder.
pub fn home() -> tempfile::TempDir {
    home_in(&std::env::temp_dir())
}

/// A SCRATCH CODEX HOME in `root`, with ALab's switch on as the codex binary has it: a save
/// then renames a new file over the login's (`codex_alab_core::home::save_auth_bytes`), as in
/// production. The switch honors `CODEX_AUTH_PATH`, so a test refuses to run where it is set:
/// that would name another file than the scratch home's.
pub fn home_in(root: &Path) -> tempfile::TempDir {
    assert!(
        std::env::var_os(codex_alab_core::home::AUTH_PATH_ENV).is_none(),
        "CODEX_AUTH_PATH is set: these tests write only a scratch home's login"
    );
    codex_alab_core::activate();
    tempfile::tempdir_in(root).unwrap_or_else(|err| panic!("scratch home: {err}"))
}

/// A login of `key` in `mode`'s store, written by codex's own API-key login.
pub fn api_key_in(home: &Path, key: &str, mode: AuthCredentialsStoreMode) {
    codex_login::login_with_api_key(home, key, mode, AuthKeyringBackendKind::default())
        .unwrap_or_else(|err| panic!("codex's own API-key login: {err}"));
}

/// [`api_key_in`] the login's file (File mode).
pub fn api_key(home: &Path, key: &str) {
    api_key_in(home, key, AuthCredentialsStoreMode::File);
}

/// An unsigned JWT of `claims`, base64url without padding, as the refresh lock's tests build
/// theirs (`alab-core/glue/auth_refresh_lock.rs`).
fn jwt(claims: &serde_json::Value) -> String {
    let b64 = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    format!(
        "{}.{}.sig",
        b64(br#"{"alg":"none","typ":"JWT"}"#),
        b64(claims.to_string().as_bytes())
    )
}

/// A ChatGPT login of `user` in `account`, holding `refresh` (fake tokens, an unsigned JWT),
/// written to the login's file by codex's own `save_auth`; `issuer` is the authority codex
/// refreshes it at (a mock), where one is named.
pub fn chatgpt(home: &Path, user: &str, account: &str, refresh: &str, issuer: Option<&str>) {
    let claims = serde_json::json!({
        "sub": user,
        "email": format!("{user}@example.invalid"),
        "https://api.openai.com/auth": {
            "chatgpt_user_id": user,
            "chatgpt_account_id": account,
            "chatgpt_plan_type": "pro",
        },
    });
    let tokens = TokenData {
        id_token: codex_login::token_data::parse_chatgpt_jwt_claims(&jwt(&claims))
            .unwrap_or_else(|err| panic!("fake JWT: {err}")),
        access_token: format!("access-for-{refresh}"),
        refresh_token: refresh.to_owned(),
        oauth_provenance: issuer.map(|issuer| codex_login::token_data::AccountOAuthProvenance {
            issuer: issuer.to_owned(),
            client_id: "client-test".to_owned(),
        }),
        login_owner_id: Some(format!("login-{refresh}")),
        account_id: Some(account.to_owned()),
    };
    codex_login::save_auth(
        home,
        &AuthDotJson {
            auth_mode: Some(AuthMode::Chatgpt),
            openai_api_key: None,
            tokens: Some(tokens),
            last_refresh: Some(chrono::Utc::now()),
            agent_identity: None,
            personal_access_token: None,
            bedrock_api_key: None,
            bedrock_access_keys: None,
        },
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )
    .unwrap_or_else(|err| panic!("codex's own save: {err}"));
}

/// Codex's manager over `home`'s login in `mode`'s store; it reads no key from the environment.
pub async fn manager(home: &Path, mode: AuthCredentialsStoreMode) -> Arc<AuthManager> {
    AuthManager::shared(
        home.to_path_buf(),
        /*enable_codex_api_key_env*/ false,
        mode,
        /*forced_chatgpt_workspace_id*/ None,
        /*chatgpt_base_url*/ None,
        AuthKeyringBackendKind::default(),
        codex_login::test_support::transport_default_auth_route_config(),
    )
    .await
}
