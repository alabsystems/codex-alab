// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual configured Source/manager/config policy controls. No AcceptedOrigin,
//! Native Go/currentness/SDK receipt or successful UI ACK is fabricated.
use super::*;
use crate::in_process::InitialSessionKind;
use crate::in_process::InitialSessionPlan;
use crate::message_processor::alab_task_mcp::tests::Harness;
use anyhow::Result;
use anyhow::ensure;
use codex_app_server_transport::ConnectionId;
use serde_json::json;
use wiremock::MockServer;

fn source<T>(value: Result<T, JSONRPCErrorError>) -> anyhow::Result<T> {
    value.map_err(|error| anyhow::anyhow!("actual configured policy error, code {}", error.code))
}

#[tokio::test]
async fn configured_empty_read_preserves_original_policy_and_an_existing_actual_core()
-> anyhow::Result<()> {
    let server = MockServer::start().await;
    let h = Harness::new(&server.uri()).await?;
    let (operation, installed) = h.processor.alab_make_initial_session(
        InitialSessionPlan::new(InitialSessionKind::Selection, None),
        h.input.clone(),
    );
    let result: anyhow::Result<()> = async {
        installed?;
        let (processor, _) = h.processor.alab_selection_publication_parts_for_tests();
        let original = processor.thread_manager.get_thread(h.parent_id).await?;
        let ids = processor.thread_manager.list_thread_ids().await;
        let empty = source(configured_empty(&processor, &h.input))?;
        let config = h.processor.alab_configured_processor_for_tests();
        let params = ConfigReadParams {
            include_layers: true,
            cwd: Some(processor.config.cwd.display().to_string()),
        };
        let expected = source(config.read(params.clone()).await)?;
        let actual = source(read_original_config(&config, params).await)?;
        ensure!(
            actual == expected,
            "configured read changed original typed config/layer policy"
        );
        let observed = source(configured_empty(&processor, &h.input))?;
        ensure!(empty.same_version(&observed) && empty.belongs_to(&h.input));
        ensure!(
            ids == processor.thread_manager.list_thread_ids().await,
            "NoPrimary constructed or removed a Core"
        );
        let still_actual = processor.thread_manager.get_thread(h.parent_id).await?;
        ensure!(
            Arc::ptr_eq(&original, &still_actual),
            "empty selection was treated as absence of Core"
        );
        let guard = source(empty.lock_unchanged())?;
        drop(guard); // Exact slot guard; no await or scheduling release.
        Ok(())
    }
    .await;
    let stopped = operation.drain().await;
    let cleanup = h.shutdown().await;
    result?;
    stopped.map_err(anyhow::Error::msg)?;
    cleanup
}

#[tokio::test]
async fn configured_empty_rejects_another_actual_input_and_closed_owner() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    let first = Harness::new(&server.uri()).await?;
    let second = match Harness::new(&server.uri()).await {
        Ok(second) => second,
        Err(error) => {
            let _ = first.shutdown().await;
            return Err(error);
        }
    };
    let (operation, installed) = first.processor.alab_make_initial_session(
        InitialSessionPlan::new(InitialSessionKind::Selection, None),
        first.input.clone(),
    );
    let result: anyhow::Result<()> = async {
        installed?;
        let (processor, _) = first.processor.alab_selection_publication_parts_for_tests();
        let actual = source(configured_empty(&processor, &first.input))?;
        ensure!(
            configured_empty(&processor, &second.input).is_err(),
            "foreign actual Incoming became the same allocation"
        );
        processor.alab_stop_initial_session();
        ensure!(
            actual.recheck_owned().is_err() && configured_empty(&processor, &first.input).is_err(),
            "closed configured owner admitted its old empty slot"
        );
        Ok(())
    }
    .await;
    let stopped = operation.drain().await;
    let first_cleanup = first.shutdown().await;
    let second_cleanup = second.shutdown().await;
    result?;
    stopped.map_err(anyhow::Error::msg)?;
    first_cleanup?;
    second_cleanup
}

#[tokio::test]
async fn public_configured_labels_do_not_fall_back_to_ordinary_config_read() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    let mut h = Harness::new(&server.uri()).await?;
    let result: anyhow::Result<()> = async {
        let session = h.c1.clone();
        let denied = h.request::<serde_json::Value>(ConnectionId(41), &session, json!({
            "id":"configured-without-real-owner", "method":"config/read", "params":{
                "includeLayers":true,"_meta":{"unowned":{"retained":true},
                "alab.retainedConfigured":{"phase":"project","frontier":"a".repeat(32)}}}
        })).await;
        let error = match denied { Ok(_) => anyhow::bail!("public labels bypassed actual bound frontier owner"), Err(error) => error };
        ensure!(error.to_string() == format!("actual RPC rejected request, code {}", invalid_request("unused").code), "unexpected denial: {error:#}");
        let ordinary = h.request::<ConfigReadResponse>(ConnectionId(41), &session, json!({
            "id":"ordinary-config-after-denial", "method":"config/read", "params":{"includeLayers":true}
        })).await?;
        ensure!(ordinary.layers.is_some(), "original ordinary configuration policy changed after protected refusal");
        Ok(())
    }.await;
    let cleanup = h.shutdown().await;
    result?;
    cleanup
}

#[tokio::test]
async fn an_unpolled_configured_guard_publishes_unknown_and_withdraws_the_same_input()
-> anyhow::Result<()> {
    use super::super::super::alab_task_start::tests::Harness as CoreHarness;
    let mut h = CoreHarness::new(16).await?;
    let result: anyhow::Result<()> = async {
        let completion = ConfiguredCompletion::new();
        let observed = completion.result.subscribe();
        let tracker = TaskTracker::new();
        let actual = h.parent.codex.clone();
        let guard = ConfiguredGuard {
            input: h.input.clone(), completion: completion.clone(), _resource: actual.clone(),
            _token: tracker.token(), finished: false,
        };
        // SAME production guard, existing actual Core/Input, and actual task
        // tracker. Current-thread spawn is aborted before the first yield.
        let worker = tokio::spawn(async move {
            let _owned = guard;
            std::future::pending::<()>().await;
        });
        worker.abort();
        let joined = worker.await;
        tracker.close();
        tokio::time::timeout(std::time::Duration::from_secs(60), tracker.wait()).await?;
        ensure!(joined.is_err_and(|error| error.is_cancelled()));
        ensure!(completion.uncertain.load(Ordering::Acquire));
        let outcome = observed.borrow().clone();
        ensure!(matches!(outcome, Some(Err(error)) if error.message == "configured accepted outcome remains unobserved"));
        ensure!(h.input.failure().is_some_and(|error| error.to_string() == "configured accepted outcome remains unobserved"));
        let current = h.processor.thread_manager.get_thread(actual.exec_thread_id()).await?;
        ensure!(Arc::ptr_eq(&current, &actual), "unknown outcome was mistaken for absent Core custody");
        Ok(())
    }.await;
    let cleanup = h.shutdown().await;
    result?;
    cleanup
}
