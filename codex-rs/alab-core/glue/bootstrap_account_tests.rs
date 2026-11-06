// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use pretty_assertions::assert_eq;
use test_case::test_case;

#[test_case(401; "rejected_refresh_token")]
#[test_case(503; "unavailable_refresh_service")]
#[tokio::test]
async fn rejected_saved_login_can_reach_sign_in_but_cannot_send_a_turn(
    refresh_status: u16,
) -> Result<()> {
    let backend = MockServer::start().await;
    Mock::given(path("/backend-api/wham/config/bundle"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&backend)
        .await;
    Mock::given(path("/backend-api/wham/accounts/check"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&backend)
        .await;
    Mock::given(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(refresh_status).set_body_json(json!({
            "error": {"code": "refresh_token_reused"}
        })))
        .mount(&backend)
        .await;
    let home = TempDir::new()?;
    config(&home, &backend).await?;
    write_models_cache(home.path()).await?;
    write_chatgpt_auth(
        home.path(),
        ChatGptAuthFixture::new("stale-token")
            .account_id("selected")
            .plan_type("pro"),
        AuthCredentialsStoreMode::File,
    )?;
    let refresh_url = format!("{}/oauth/token", backend.uri());
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .with_env_overrides(&[
            ("OPENAI_API_KEY", None),
            (REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR, Some(&refresh_url)),
        ])
        .build_initialized_with_timeout(READ_TIMEOUT)
        .await?;
    let account = read(&mut server).await?;
    assert_eq!(account["account"], Value::Null);
    assert_eq!(account["requiresOpenaiAuth"], true);
    assert_eq!(account["workspaceRouting"], Value::Null);
    let thread = server
        .start_thread(ThreadStartParams::default())
        .await?
        .thread;
    let completed = timeout(
        READ_TIMEOUT,
        server.start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.id,
            input: vec![UserInput::Text {
                text: "hi".into(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        }),
    )
    .await??;
    assert_eq!(completed.turn.status, TurnStatus::Failed);
    assert_eq!(
        completed
            .turn
            .error
            .expect("authentication failure")
            .codex_error_info,
        Some(CodexErrorInfo::Unauthorized)
    );
    assert!(home.path().join("auth.json").exists());
    // A new login must clear the startup wall and discover its actual routing.
    Mock::given(path("/backend-api/wham/accounts/check"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "accounts": [{"id": "selected", "workspace_backend_origin": "https://chatgpt.com",
                "account_routing_override": "NO_CONSTRAINT"}]
        })))
        .with_priority(1)
        .mount(&backend)
        .await;
    login(&mut server, "selected").await?;
    let account = read(&mut server).await?;
    assert_eq!(account["account"]["type"], "chatgpt");
    assert_eq!(
        account["workspaceRouting"]["backendOrigin"],
        "https://chatgpt.com"
    );
    Ok(())
}
