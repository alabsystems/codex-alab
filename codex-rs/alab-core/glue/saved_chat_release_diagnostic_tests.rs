// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::DEFAULT_READ_TIMEOUT;
use anyhow::Result;
use app_test_support::TestAppServer;
use codex_alab_core::diagnostic_id::SAVED_CHAT_RELEASE_PREFIX;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::JSONRPCRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadUnsubscribeResponse;
use codex_app_server_protocol::ThreadUnsubscribeStatus;
use codex_state::LogQuery;
use codex_state::StateRuntime;
use core_test_support::load_default_config_for_test;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;

async fn send_unsubscribe(mcp: &mut TestAppServer, wire: &str, thread_id: &str) -> Result<()> {
    mcp.send_jsonrpc_message(JSONRPCMessage::Request(JSONRPCRequest {
        id: RequestId::String(wire.to_owned()),
        method: "thread/unsubscribe".into(),
        params: Some(json!({ "threadId": thread_id })),
        trace: None,
    }))
    .await
}

#[tokio::test]
async fn original_saved_chat_unsubscribe_diagnostic_survives_client_exit() -> Result<()> {
    let home = TempDir::new()?;
    let mut mcp = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let missing_child = "0195d538-fcf1-7429-a9d0-18ef9cffbe56";
    let ack = format!("{SAVED_CHAT_RELEASE_PREFIX}7bd5743b-a0bc-4819-823d-24f9eb7e8235");
    let rejected = format!("{SAVED_CHAT_RELEASE_PREFIX}fd592512-c09f-4c3a-ac7d-3c7133e4cfaf");
    for wire in [
        format!("{SAVED_CHAT_RELEASE_PREFIX}7BD5743B-A0BC-4819-823D-24F9EB7E8235"),
        format!("{SAVED_CHAT_RELEASE_PREFIX}7bd5743ba0bc4819823d24f9eb7e8235"),
        ack.clone(),
    ] {
        send_unsubscribe(&mut mcp, &wire, missing_child).await?;
        let reply = timeout(
            DEFAULT_READ_TIMEOUT,
            mcp.read_stream_until_response_message(RequestId::String(wire)),
        )
        .await??;
        let response: ThreadUnsubscribeResponse = serde_json::from_value(reply.result)?;
        assert_eq!(response.status, ThreadUnsubscribeStatus::NotLoaded);
    }
    let invalid_child = "arbitrary-child-text";
    send_unsubscribe(&mut mcp, &rejected, invalid_child).await?;
    let error = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::String(rejected.clone())),
    )
    .await??;

    // LogDbLayer queues writes. Observe the last entry before closing stdio, then
    // read the complete set using a fresh database connection after server exit.
    let sqlite = load_default_config_for_test(&home).await.sqlite;
    let query = LogQuery {
        search: Some("saved chat unsubscribe handler completed".into()),
        ..Default::default()
    };
    let reader = StateRuntime::init(sqlite.clone(), "test-provider".into()).await?;
    timeout(Duration::from_secs(60), async {
        loop {
            if reader.query_logs(&query).await?.iter().any(|row| {
                row.message
                    .as_deref()
                    .is_some_and(|body| body.contains(&rejected))
            }) {
                return Ok::<_, anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await??;
    reader.close().await;
    assert!(
        timeout(Duration::from_secs(60), mcp.shutdown_gracefully())
            .await??
            .success()
    );
    drop(mcp);

    let reopened = StateRuntime::init(sqlite, "test-provider".into()).await?;
    let rows = reopened.query_logs(&query).await?;
    reopened.close().await;
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0].thread_id.as_deref(), Some(missing_child));
    assert!(rows[0].process_uuid.is_some());
    assert_eq!(rows[0].process_uuid, rows[1].process_uuid);
    let ack_body = rows[0].message.as_deref().unwrap();
    assert!(ack_body.contains(&format!("wire_request={ack}")));
    assert!(ack_body.contains("outcome=notLoaded"));
    assert!(!ack_body.contains("server_code"));
    let connection = ack_body
        .split_whitespace()
        .find(|field| field.starts_with("connection_id="))
        .unwrap();
    connection
        .strip_prefix("connection_id=")
        .unwrap()
        .parse::<u64>()?;
    let rejected_body = rows[1].message.as_deref().unwrap();
    assert!(rows[1].thread_id.is_none());
    assert!(rejected_body.contains(connection));
    assert!(rejected_body.contains(&format!("wire_request={rejected}")));
    assert!(rejected_body.contains("outcome=rejected"));
    assert!(rejected_body.contains(&format!("server_code={}", error.error.code)));
    assert!(!rejected_body.contains(invalid_child));
    Ok(())
}
