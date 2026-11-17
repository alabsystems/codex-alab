// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original ConfigManager/AuthConfig policy and protected dispatch negatives.
//! No AcceptedOrigin, Native/SDK grant or successful reader retirement is made.
use super::*;
use crate::message_processor::alab_task_mcp::tests::Harness;
use anyhow::Result;
use anyhow::ensure;
use codex_app_server_transport::ConnectionId;
use serde_json::json;
use wiremock::MockServer;
fn source<T>(value: Result<T, JSONRPCErrorError>) -> anyhow::Result<T> {
    value.map_err(|error| anyhow::anyhow!("actual reader policy error, code {}", error.code))
}
#[tokio::test]
async fn actual_managed_read_preserves_original_config_loader_and_current_core()
-> anyhow::Result<()> {
    let server = MockServer::start().await;
    let h = Harness::new(&server.uri()).await?;
    let result: anyhow::Result<()> = async {
        let config = h.processor.alab_configured_processor_for_tests();
        let params = ConfigReadParams {
            include_layers: true,
            cwd: None,
        };
        let expected = source(config.read(params.clone()).await)?;
        let actual_loader = config
            .config_manager
            .alab_retained_managed_snapshot()
            .await?;
        let (response, cloud, account) = source(
            config
                .alab_retained_managed_read(params, ReadKind::Managed)
                .await,
        )?;
        ensure!(
            response == expected,
            "reader changed original typed configuration policy"
        );
        let observed = cloud.ok_or_else(|| anyhow::anyhow!("managed observation absent"))?;
        let expected_data = ManagedCloudData::from_actual(&actual_loader);
        ensure!(
            serde_json::to_value(&observed)? == serde_json::to_value(&expected_data)?,
            "reader selected another loader"
        );
        let observed = serde_json::to_value(account)?;
        ensure!(
            observed
                == json!({"authenticated":false,"chatgptAuth":false,"managedChatgpt":false,
            "fedramp":false,"account":null,"user":null}),
            "isolated no-auth fixture acquired credentials"
        );
        let (processor, _) = h.processor.alab_selection_publication_parts_for_tests();
        let actual = processor.thread_manager.get_thread(h.parent_id).await?;
        ensure!(
            std::sync::Arc::ptr_eq(&actual, &h.parent),
            "observation replaced the actual Core"
        );
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result?;
    cleanup
}
#[tokio::test]
async fn account_storage_error_is_not_an_observed_unauthenticated_account() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    let h = Harness::new(&server.uri()).await?;
    let result: anyhow::Result<()> = async {
        let config = h.processor.alab_configured_processor_for_tests();
        let path = config.config_manager.codex_home().join("auth.json");
        tokio::fs::write(path, b"invalid original auth JSON").await?;
        let params = ConfigReadParams {
            include_layers: false,
            cwd: None,
        };
        ensure!(
            config
                .alab_retained_managed_read(params, ReadKind::Account)
                .await
                .is_err(),
            "original auth error became a local account preference"
        );
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result?;
    cleanup
}
#[tokio::test]
async fn reader_labels_without_actual_bound_origin_deny_and_preserve_ordinary_config_read()
-> anyhow::Result<()> {
    let server = MockServer::start().await;
    let mut h = Harness::new(&server.uri()).await?;
    let result: anyhow::Result<()> = async {
        let session = h.c1.clone();
        let denied = h.request::<serde_json::Value>(ConnectionId(41), &session, json!({
            "id":"reader-without-real-owner", "method":"config/read", "params":{
                "includeLayers":true,"_meta":{"alab.retainedReader":{
                    "service":"a".repeat(32),"operation":"b".repeat(32),"kind":"account"}}}
        })).await;
        ensure!(denied.is_err(), "public DATA bypassed actual received A/owner checks");
        let response = h.request::<ConfigReadResponse>(ConnectionId(41), &session, json!({
            "id":"ordinary-after-reader-refusal","method":"config/read","params":{"includeLayers":true}
        })).await?;
        ensure!(response.layers.is_some(), "protected refusal changed original config read");
        Ok(())
    }.await;
    let cleanup = h.shutdown().await;
    result?;
    cleanup
}
