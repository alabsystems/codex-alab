// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use codex_login::AuthCredentialsStoreMode;
use codex_login::AuthKeyringBackendKind;
use http::HeaderValue;
use http::Method;
use serde_json::json;
use tempfile::TempDir;

fn jwt(workspace: &str, user: &str) -> String {
    let payload = json!({"https://api.openai.com/auth":{"chatgpt_account_id":workspace,"chatgpt_user_id":user}});
    format!(
        "test.{}.fixture",
        URL_SAFE_NO_PAD.encode(payload.to_string())
    )
}

async fn selected(workspace: &str, user: &str, bearer: &str) -> CodexAuth {
    let home = TempDir::new().unwrap();
    let data = json!({"auth_mode":"chatgpt","tokens":{"id_token":jwt(workspace,user),"access_token":bearer,"refresh_token":"synthetic-refresh","account_id":workspace},"last_refresh":"2099-01-01T00:00:00Z"});
    std::fs::write(
        home.path().join("auth.json"),
        serde_json::to_vec(&data).unwrap(),
    )
    .unwrap();
    CodexAuth::from_auth_storage(
        home.path(),
        AuthCredentialsStoreMode::File,
        None,
        AuthKeyringBackendKind::default(),
        &codex_login::test_support::transport_default_auth_route_config(),
    )
    .await
    .unwrap()
    .unwrap()
}

fn headers(workspace: &str, bearer: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {bearer}")).unwrap(),
    );
    headers.insert(
        CHATGPT_ACCOUNT_ID_HEADER,
        HeaderValue::from_str(workspace).unwrap(),
    );
    headers
}

// All three trait methods can return different credentials. Header previews cannot establish
// which bearer a websocket or a complete HTTP request actually sends.
struct SelectedHeaders {
    preview: HeaderMap,
    websocket: HeaderMap,
    http: HeaderMap,
}
impl AuthProvider for SelectedHeaders {
    fn add_auth_headers(&self, headers: &mut HeaderMap) -> Result<(), AuthError> {
        headers.extend(self.preview.clone());
        Ok(())
    }
    fn apply_auth_headers(&self, _headers: HeaderMap) -> AuthHeadersFuture<'_> {
        Box::pin(async { Ok(self.websocket.clone()) })
    }
    fn apply_auth(&self, mut request: Request) -> AuthProviderFuture<'_> {
        Box::pin(async move {
            request.headers = self.http.clone();
            Ok(request)
        })
    }
}

#[tokio::test]
async fn checks_the_transmitted_bearer_and_workspace_after_each_signer() {
    let workspace = "fixture-workspace";
    let bearer = "opaque-grant-a";
    let principal = bound_picker::principal(workspace, "fixture-user-a");
    let auth = selected(workspace, "fixture-user-a", bearer).await;
    let valid = headers(workspace, bearer);
    let incorrect_bearer = headers(workspace, &jwt(workspace, "fixture-user-a"));
    let incorrect_workspace = headers("other-workspace", bearer);
    let mut duplicate = valid.clone();
    duplicate.append(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer opaque-grant-b"),
    );
    let mut duplicate_workspace = valid.clone();
    duplicate_workspace.append(
        CHATGPT_ACCOUNT_ID_HEADER,
        HeaderValue::from_static("other-workspace"),
    );
    let mut cookie = valid.clone();
    cookie.insert("cookie", HeaderValue::from_static("session=other-grant"));
    let mut assertion = valid.clone();
    assertion.insert(
        "x-openai-agent-assertion",
        HeaderValue::from_static("another-runtime"),
    );
    for (preview, ws, http, preview_ok, send_ok) in [
        (&valid, &valid, &valid, true, true),
        (&valid, &incorrect_bearer, &incorrect_workspace, true, false),
        (&duplicate, &duplicate_workspace, &duplicate, false, false),
        (&valid, &cookie, &assertion, true, false),
    ] {
        let mut resolved = crate::ResolvedProviderAuth::new(Arc::new(SelectedHeaders {
            preview: preview.clone(),
            websocket: ws.clone(),
            http: http.clone(),
        }));
        resolved.credential = Some(auth.clone());
        bind_selected(&mut resolved, &principal).unwrap();
        let mut presented = HeaderMap::new();
        assert_eq!(
            resolved.auth.add_auth_headers(&mut presented).is_ok(),
            preview_ok
        );
        if !preview_ok {
            assert!(presented.is_empty());
        }
        assert_eq!(
            resolved
                .auth
                .apply_auth_headers(HeaderMap::new())
                .await
                .is_ok(),
            send_ok
        );
        assert_eq!(
            resolved
                .auth
                .apply_auth(Request::new(Method::POST, "http://localhost/unused".into()))
                .await
                .is_ok(),
            send_ok
        );
    }
}

#[tokio::test]
async fn another_workspace_user_or_bearer_cannot_reuse_a_confirmation() {
    let workspace = "fixture-workspace";
    let user = "fixture-user-a";
    let expected = bound_picker::principal(workspace, user);
    for (account, subject, bearer, ok) in [
        (workspace, user, jwt(workspace, user), true),
        (workspace, "fixture-user-b", "opaque-grant-b".into(), false),
        ("other-workspace", user, "opaque-grant-c".into(), false),
        (workspace, user, jwt(workspace, "fixture-user-b"), false),
        (workspace, user, jwt("other-workspace", user), false),
    ] {
        let document = selected(account, subject, &bearer).await;
        let mut resolved =
            crate::ResolvedProviderAuth::new(crate::auth::auth_provider_from_auth(&document));
        resolved.credential = Some(document);
        bind_selected(&mut resolved, &expected).unwrap();
        let result = resolved.auth.to_auth_headers();
        assert_eq!(result.is_ok(), ok);
        if let Err(error) = result {
            assert!(!error.to_string().contains("fixture-"));
        }
    }
}

#[test]
fn picker_requires_a_subscription_source_while_an_ordinary_override_is_untouched() {
    let mut info = ModelProviderInfo::create_openai_provider(None);
    assert!(permitted(&info));
    info.experimental_bearer_token = Some("provider-secret".into());
    assert!(!permitted(&info));
    assert!(!permitted(&ModelProviderInfo::create_gcp_provider()));
    assert!(!permitted(
        &ModelProviderInfo::create_amazon_bedrock_provider(None)
    ));
    let api_key = CodexAuth::from_api_key("synthetic-key");
    let mut resolved =
        crate::ResolvedProviderAuth::new(crate::auth::auth_provider_from_auth(&api_key));
    resolved.credential = Some(api_key);
    let ordinary = resolved.auth.clone();
    bind(&info, &mut resolved).unwrap();
    assert!(Arc::ptr_eq(&ordinary, &resolved.auth));
    assert!(bind_selected(&mut resolved, &bound_picker::principal("a", "b")).is_err());
}

// The picker restriction is process-scoped. Exercise the public startup state in a fresh
// copy of this test executable, leaving other tests' ordinary account selection intact.
#[tokio::test]
async fn pinned_request_resolver_rejects_an_override_and_protects_the_signed_request() {
    const CHILD: &str = "ALAB_PICKER_PROVIDER_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let name = concat!(
            module_path!(),
            "::pinned_request_resolver_rejects_an_override_and_protects_the_signed_request"
        );
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name.split_once("::").unwrap().1, "--nocapture"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            child.status.success() && String::from_utf8_lossy(&child.stdout).contains("1 passed"),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        return;
    }
    let workspace = "fixture-workspace";
    let user = "fixture-user";
    bound_picker::install(&bound_picker::principal(workspace, user)).unwrap();
    let mut info = ModelProviderInfo::create_openai_provider(None);
    info.experimental_bearer_token = Some("synthetic-provider-override".into());
    assert!(
        crate::request_setup::resolve_request_auth(info.clone(), None, None, None, None)
            .await
            .is_err()
    );
    assert_eq!(
        redirect_policy(ClientRedirectPolicy::Default),
        ClientRedirectPolicy::Reject
    );
    info.experimental_bearer_token = None;
    let credential = selected(workspace, user, "fixture-grant").await;
    let mut selected =
        crate::ResolvedProviderAuth::new(crate::auth::auth_provider_from_auth(&credential));
    selected.credential = Some(credential);
    let resolved = crate::combined_auth::compose_auth(&info, None, selected)
        .await
        .unwrap();
    let approved = resolved
        .auth
        .apply_auth_headers(HeaderMap::new())
        .await
        .unwrap();
    assert_eq!(approved[CHATGPT_ACCOUNT_ID_HEADER], workspace);
    let mut extra = HeaderMap::new();
    extra.insert("x-api-key", HeaderValue::from_static("different-grant"));
    assert!(resolved.auth.apply_auth_headers(extra).await.is_err());
}
