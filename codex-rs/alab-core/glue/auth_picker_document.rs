// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Read the selected file and bind the very document that supplies authentication.

use super::*;
use codex_alab_core::bound_picker;
use serde_json::Value;
use std::io;

/// Keep all readers and refresh commits on the same effective credential store.
/// Named auth paths already select a file. A confirmed default-slot launch also
/// constrains Auto to the file, while respecting an explicit keyring-only policy.
pub(in crate::auth) fn storage_mode(mode: AuthCredentialsStoreMode) -> AuthCredentialsStoreMode {
    use AuthCredentialsStoreMode::Auto;
    use AuthCredentialsStoreMode::File;
    use AuthCredentialsStoreMode::Keyring;
    if (matches!(mode, Auto | Keyring) && codex_alab_core::home::has_admitted_auth_path())
        || (mode == Auto && bound_picker::expected().is_some())
    {
        File
    } else {
        mode
    }
}

fn token_auth(token: &str) -> Option<Value> {
    let claims: Value = crate::token_data::decode_jwt_payload(token).ok()?;
    claims.get("https://api.openai.com/auth").cloned()
}

fn user(claims: &Value) -> Option<&str> {
    ["chatgpt_user_id", "user_id"].iter().find_map(|key| {
        claims
            .get(key)?
            .as_str()
            .map(str::trim)
            .filter(|user| !user.is_empty())
    })
}

impl CodexAuth {
    /// Check a file login or the exact token snapshot supplying a selected request.
    pub fn alab_picker_tokens_match(expected: &str, tokens: &TokenData) -> bool {
        let Some(workspace) = tokens
            .account_id
            .as_deref()
            .filter(|id| !id.is_empty() && *id == id.trim())
        else {
            return false;
        };
        if tokens.access_token.trim().is_empty() {
            return false;
        }
        let id = token_auth(&tokens.id_token.raw_jwt);
        let access = token_auth(&tokens.access_token);
        if access
            .as_ref()
            .and_then(|claims| claims.get("chatgpt_account_id"))
            .is_some_and(|value| value.as_str() != Some(workspace))
        {
            return false;
        }
        let user = match (id.as_ref().and_then(user), access.as_ref().and_then(user)) {
            (Some(id), Some(access)) if id != access => return false,
            (Some(user), _) | (None, Some(user)) => user,
            (None, None) => return false,
        };
        bound_picker::principal(workspace, user) == expected
    }
}

pub(in crate::auth) fn validate(expected: &str, document: &AuthDotJson) -> io::Result<()> {
    if document.resolved_mode() != AuthMode::Chatgpt
        || document.openai_api_key.is_some()
        || document.agent_identity.is_some()
        || document.personal_access_token.is_some()
        || document.bedrock_api_key.is_some()
        || document.bedrock_access_keys.is_some()
        || !document
            .tokens
            .as_ref()
            .is_some_and(|tokens| CodexAuth::alab_picker_tokens_match(expected, tokens))
    {
        return Err(bound_picker::unavailable());
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
pub(super) async fn load_selected_file(
    home: &Path,
    mode: AuthCredentialsStoreMode,
    methods: Option<&[ForcedLoginMethod]>,
    workspaces: Option<&[String]>,
    base_url: Option<&str>,
    keyring: AuthKeyringBackendKind,
    identity_url: Option<&str>,
    route: &AuthRouteConfig,
    modes: Option<&[AuthMode]>,
) -> io::Result<Option<CodexAuth>> {
    if storage_mode(mode) != AuthCredentialsStoreMode::File
        || !auth_mode_is_allowed(methods, AuthMode::Chatgpt)
        || modes.is_some_and(|allowed| !allowed.contains(&AuthMode::Chatgpt))
    {
        return Err(bound_picker::unavailable());
    }
    let store = create_auth_storage(home.to_owned(), AuthCredentialsStoreMode::File, keyring);
    let document = load_stored_auth(store)
        .await?
        .ok_or_else(bound_picker::unavailable)?;
    validate(
        bound_picker::expected().ok_or_else(bound_picker::unavailable)?,
        &document,
    )?;
    let auth = CodexAuth::from_auth_dot_json(
        home,
        document,
        AuthCredentialsStoreMode::File,
        base_url,
        keyring,
        identity_url,
        route,
        None,
    )
    .await?;
    validate_auth_restrictions(methods, workspaces, &auth)
        .map_err(|_| bound_picker::unavailable())?;
    Ok(Some(auth))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use codex_http_client::HttpClientFactory;
    use codex_http_client::OutboundProxyPolicy;
    use serde_json::json;

    const EXPECTED: &str = "92726e7270b1b75a1eb9a82bac81d19e9dd60489eafa7afff660fc6b3dc54677";

    fn jwt(user: &str) -> String {
        let claims = json!({"https://api.openai.com/auth":{"chatgpt_user_id":user}});
        format!(
            "e30.{}.fixture",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string())
        )
    }

    fn document(workspace: &str, id: &str, access: &str) -> AuthDotJson {
        serde_json::from_value(json!({"auth_mode":"chatgpt", "OPENAI_API_KEY":null, "last_refresh":"2026-01-01T00:00:00Z",
            "tokens":{"account_id":workspace,"access_token":jwt(access),"id_token":jwt(id),"refresh_token":"fixture"}})).unwrap()
    }

    #[test]
    fn picker_principal_requires_the_selected_workspace_and_consistent_native_user() {
        assert_eq!(bound_picker::principal("workspace-b", "user-b"), EXPECTED);
        assert!(validate(EXPECTED, &document("workspace-b", "user-b", "user-b")).is_ok());
        for doc in [
            document("workspace-a", "user-b", "user-b"),
            document("workspace-b", "user-a", "user-a"),
            document("workspace-b", "user-b", "user-other"),
            document("workspace-b", "user-other", "user-b"),
            document("workspace-b", "", ""),
            document(" workspace-b", "user-b", "user-b"),
        ] {
            assert_eq!(
                validate(EXPECTED, &doc).unwrap_err().kind(),
                io::ErrorKind::PermissionDenied
            );
        }
        let mut opaque = document("workspace-b", "user-b", "user-b");
        opaque.tokens.as_mut().unwrap().access_token = "opaque-fixture".into();
        assert!(validate(EXPECTED, &opaque).is_ok());
        let mut conflicting_workspace = opaque.clone();
        let claims = json!({"https://api.openai.com/auth":{"chatgpt_account_id":"other","chatgpt_user_id":"user-b"}});
        conflicting_workspace.tokens.as_mut().unwrap().access_token = format!(
            "e30.{}.fixture",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        assert!(validate(EXPECTED, &conflicting_workspace).is_err());
        let mut mixed = document("workspace-b", "user-b", "user-b");
        mixed.openai_api_key = Some("fixture".into());
        assert!(validate(EXPECTED, &mixed).is_err());
    }

    #[tokio::test]
    async fn confirmed_picker_file_replacement_refuses_reload_refresh_and_env_auth() {
        const CHILD: &str = "ALAB_TEST_CONFIRMED_PICKER_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let name = std::thread::current().name().unwrap().to_owned();
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &name, "--nocapture"])
                .env(CHILD, "1")
                .env_remove("CODEX_AUTH_PATH")
                .env("CODEX_API_KEY", "fixture-ignored")
                .env_remove("CODEX_AUTH_SLOT")
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "isolated picker auth test failed: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed"));
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        codex_alab_core::activate();
        bound_picker::install(EXPECTED).unwrap();
        let path = dir.path().join("auth.json");
        let original = document("workspace-b", "user-b", "user-b");
        let other = document("workspace-a", "user-a", "user-a");
        std::fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
        let route = AuthRouteConfig::from_http_client_factory(HttpClientFactory::new(
            OutboundProxyPolicy::ReqwestDefault,
        ));
        let read_mode = |mode| {
            load_auth(
                dir.path(),
                true,
                mode,
                None,
                None,
                None,
                AuthKeyringBackendKind::default(),
                None,
                &route,
                None,
                None,
            )
        };
        for excluded in [
            AuthCredentialsStoreMode::Keyring,
            AuthCredentialsStoreMode::Ephemeral,
        ] {
            assert!(read_mode(excluded).await.is_err());
        }
        let read = || read_mode(AuthCredentialsStoreMode::Auto);
        let auth = read().await.unwrap().unwrap();
        assert!(auth.is_chatgpt_auth());
        assert_eq!(auth.get_account_id().as_deref(), Some("workspace-b"));
        assert!(CodexAuth::alab_picker_tokens_match(
            EXPECTED,
            &auth.get_token_data().unwrap()
        ));
        let store = create_auth_storage(
            dir.path().to_owned(),
            AuthCredentialsStoreMode::Auto,
            AuthKeyringBackendKind::default(),
        );
        let mut rotated = original.clone();
        rotated.tokens.as_mut().unwrap().refresh_token = "fixture-rotated".into();
        store.compare_and_save(&original, &rotated).unwrap();
        assert!(store.load().unwrap() == Some(rotated.clone()));
        assert!(store.compare_and_save(&rotated, &other).is_err());
        std::fs::write(&path, serde_json::to_vec(&other).unwrap()).unwrap();
        assert!(read().await.is_err());
        assert!(store.load_account_grant().is_err());
        assert!(store.save(&original).is_err());
        assert!(store.delete().is_err());
        assert!(store.compare_and_save(&rotated, &original).is_err());
        assert!(std::fs::read(&path).unwrap() == serde_json::to_vec(&other).unwrap());
        std::fs::remove_file(&path).unwrap();
        assert!(read().await.is_err());
    }
}
