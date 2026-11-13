// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exercise the real status processor with unrelated harness and MCP credential owners.
use super::*;
use codex_login::AuthCredentialsStoreMode;
use codex_login::AuthKeyringBackendKind;
use core_test_support::test_codex::test_codex;
use serde_json::json;

#[tokio::test]
async fn status_uses_selected_mcp_owner_when_harness_storage_is_unavailable() -> anyhow::Result<()> {
    let home = Arc::new(tempfile::tempdir()?);
    let id_token = concat!(
        "eyJhbGciOiJFUzI1NiJ9.",
        "eyJpc3MiOiJodHRwczovL2lkcC5leGFtcGxlIiwic3ViIjoidXNlciIsImF1ZCI6ImFjY291bnQt",
        "Y2xpZW50IiwiZXhwIjoxLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF91",
        "c2VyX2lkIjoidXNlciIsImNoYXRncHRfYWNjb3VudF9pZCI6IndvcmtzcGFjZSJ9fQ",
        ".signature",
    );
    std::fs::write(home.path().join("auth.json"), serde_json::to_vec(&json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "id_token": id_token, "access_token": "selected-access",
            "refresh_token": "selected-refresh", "account_id": "workspace",
            "login_owner_id": "completed-login",
            "oauth_provenance": {"issuer": "https://idp.example", "client_id": "account-client"}
        }, "last_refresh": "2099-01-01T00:00:00Z"
    }))?)?;
    let selected = AuthManager::shared(
        home.path().to_path_buf(), false, AuthCredentialsStoreMode::File,
        None, None, AuthKeyringBackendKind::default(),
        codex_login::test_support::transport_default_auth_route_config(),
    ).await;
    assert!(selected.account_oauth_grant().await?.is_some());
    let server = core_test_support::responses::start_mock_server().await;
    let test = test_codex().with_home(home.clone()).with_auth_manager(selected.clone())
        .build(&server).await?;
    // Add the inspection-only server after startup. Its closed loopback destination
    // cannot contact an external service; a transport failure must preserve auth status.
    std::fs::write(home.path().join("config.toml"), r#"
mcp_oauth_credentials_store = "file"
[features]
use_xaa = true
[mcp_enterprise_managed_auth.idp]
issuer = "https://idp.example"
client_id = "enterprise-client"
[mcp_servers.owner]
url = "https://127.0.0.1:9/mcp"
auth = "ema_auth"
startup_timeout_sec = 0.05
"#)?;
    let harness_home = tempfile::tempdir()?;
    std::fs::copy(home.path().join("auth.json"), harness_home.path().join("auth.json"))?;
    let harness = AuthManager::shared(
        harness_home.path().to_path_buf(), false, AuthCredentialsStoreMode::File,
        None, None, AuthKeyringBackendKind::default(),
        codex_login::test_support::transport_default_auth_route_config(),
    ).await;
    std::fs::write(harness_home.path().join("auth.json"), "invalid fixture storage")?;
    assert!(harness.account_oauth_grant().await.is_err());
    assert!(!Arc::ptr_eq(&selected, &harness));
    let config_manager = ConfigManager::without_managed_config_for_tests(home.path().to_path_buf());
    let config = Arc::new(test.config.clone());
    let model_catalog = Arc::new(crate::model_catalog::ModelCatalog::new(
        config_manager.clone(), None, config.clone(), test.thread_manager.clone(),
    ));
    let (outgoing_tx, _outgoing_rx) = tokio::sync::mpsc::channel(16);
    let outgoing = Arc::new(OutgoingMessageSender::new(
        outgoing_tx, codex_analytics::AnalyticsEventsClient::disabled(),
    ));
    let account = AccountRequestProcessor::new(
        harness.clone(), test.thread_manager.clone(), outgoing.clone(), config,
        config_manager.clone(), model_catalog,
    );
    let processor = McpRequestProcessor::new(
        harness, test.thread_manager.clone(), ThreadStateManager::new(), outgoing,
        config_manager, account,
    );
    for thread in [None, Some(test.codex.clone())] {
        let response = tokio::time::timeout(std::time::Duration::from_secs(10),
            processor.list_mcp_server_status_response(ListMcpServerStatusParams {
                cursor: None, limit: None, detail: Some(McpServerStatusDetail::ToolsAndAuthOnly),
                thread_id: None, server_name: None,
            }, thread),
        ).await?.map_err(|error| anyhow::anyhow!("status request failed: {error:?}"))?;
        let owner = response.data.iter().find(|entry| entry.name == "owner")
            .ok_or_else(|| anyhow::anyhow!("missing owner server: {response:?}"))?;
        assert_eq!(owner.auth_status, codex_app_server_protocol::McpAuthStatus::BearerToken);
    }
    test.codex.shutdown_and_wait().await?;
    Ok(())
}
