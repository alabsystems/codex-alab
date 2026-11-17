// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual initialized MessageProcessor/Core/binding controls, with NO fabricated
//! Native origin. These deny before reservation or original mutation work.
use super::*;
use wiremock::MockServer;

#[tokio::test]
async fn actual_preparation_without_origin_cannot_reserve_or_poison_original_config_read()
-> Result<()> {
    let model = MockServer::start().await;
    let mut h = Harness::new(&model.uri()).await?;
    let plan = crate::in_process::InitialSessionPlan::new(
        crate::in_process::InitialSessionKind::Selection,
        None,
    );
    let (initial, installed) = h.processor.alab_make_initial_session(plan, h.input.clone());
    let result: Result<()> = async {
        installed?;
        let actual = h.host.control.manager.get_thread(h.parent_id).await?;
        let before = h.host.control.manager.list_thread_ids().await;
        let c1 = h.c1.clone();
        let denied = h.request::<serde_json::Value>(ConnectionId(41), &c1, json!({
            "id":8901,"method":"config/read","params":{"includeLayers":false,"_meta":{
                "alab.clientConstructionPrepare":{"phase":"prepare",
                    "preparation":"11111111111111111111111111111111","method":"thread/resume",
                    "parameters":{"threadId":h.parent_id.to_string(),"excludeTurns":true},
                    "selection":{"prospectiveCreate":{"ephemeral":true},"presentation":"primary"}}
            }}
        })).await;
        ensure!(
            denied.is_err(),
            "preparation DATA became actual Native registration"
        );
        ensure!(before == h.host.control.manager.list_thread_ids().await);
        ensure!(Arc::ptr_eq(
            &actual,
            &h.host.control.manager.get_thread(h.parent_id).await?
        ));
        let _: codex_app_server_protocol::ConfigReadResponse = h
            .request(
                ConnectionId(41),
                &c1,
                json!({"id":8902,"method":"config/read","params":{"includeLayers":false}}),
            )
            .await?;
        ensure!(
            !h.input.is_stopped(),
            "confirmed missing-origin denial poisoned actual backend"
        );
        Ok(())
    }
    .await;
    let drained = initial
        .drain()
        .await
        .map_err(|error| anyhow::anyhow!(error));
    let cleanup = h.shutdown().await;
    result.and(drained).and(cleanup)
}

#[tokio::test]
async fn actual_s2_data_without_origin_cannot_project_or_rebuild_loaded_core() -> Result<()> {
    let model = MockServer::start().await;
    let mut h = Harness::new(&model.uri()).await?;
    let result: Result<()> = async {
        let actual = h.host.control.manager.get_thread(h.parent_id).await?;
        let private = actual.config().await.mcp_servers.get().clone();
        let before = h.host.control.manager.list_thread_ids().await;
        let c1 = h.c1.clone();
        let denied = h.request::<serde_json::Value>(ConnectionId(41), &c1, json!({
            "id":8903,"method":"thread/resume","params":{"threadId":h.parent_id.to_string(),"_meta":{
                "alab.sourceSelectionFrontier":{"phase":"project","history":"currentDisplay","claim":"11111111111111111111111111111111",
                    "sourceOperation":"22222222222222222222222222222222","selection":"33333333333333333333333333333333",
                    "frontier":"44444444444444444444444444444444","threadId":h.parent_id.to_string(),"go":null}
            }}
        })).await;
        ensure!(denied.is_err(), "S2 correlation DATA became a Native/UI/current grant");
        ensure!(before == h.host.control.manager.list_thread_ids().await);
        ensure!(Arc::ptr_eq(&actual, &h.host.control.manager.get_thread(h.parent_id).await?));
        ensure!(private == *actual.config().await.mcp_servers.get());
        ensure!(!h.input.is_stopped());
        Ok(())
    }.await;
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn actual_later_start_prepare_and_mutation_require_real_origin_before_core_or_binding()
-> Result<()> {
    let model = MockServer::start().await;
    let mut h = Harness::new(&model.uri()).await?;
    let plan = crate::in_process::InitialSessionPlan::new(
        crate::in_process::InitialSessionKind::Selection,
        None,
    );
    let (initial, installed) = h.processor.alab_make_initial_session(plan, h.input.clone());
    let result: Result<()> = async {
        installed?;
        let before = h.host.control.manager.list_thread_ids().await;
        let bindings = h
            .host
            .control
            .registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .bindings
            .len();
        let params = codex_app_server_protocol::ThreadStartParams {
            ephemeral: Some(true),
            ..Default::default()
        };
        let c1 = h.c1.clone();
        let preparation = "11111111111111111111111111111111";
        let denied = h.request::<serde_json::Value>(ConnectionId(41), &c1, json!({
            "id":8961,"method":"config/read","params":{"includeLayers":false,"_meta":{
                "alab.clientConstructionPrepare":{"phase":"prepare","preparation":preparation,
                    "method":"thread/start","parameters":params,
                    "selection":{"prospectiveCreate":params,"presentation":"primary"}}
            }}
        })).await;
        ensure!(
            denied.is_err(),
            "Start preparation DATA became Native registration"
        );
        let mut original = serde_json::to_value(&params)?;
        original["_meta"] = json!({"foreign.original":{"kept":true},
            "alab.clientConstruction":{"prospectiveCreate":params,"presentation":"primary",
                "prepared":{"preparation":preparation,"claim":"22222222222222222222222222222222",
                    "sourceOperation":"33333333333333333333333333333333",
                    "selection":"44444444444444444444444444444444"}}});
        let denied = h
            .request::<serde_json::Value>(
                ConnectionId(41),
                &c1,
                json!({
                    "id":8962,"method":"thread/start","params":original
                }),
            )
            .await;
        ensure!(
            denied.is_err(),
            "Start selection DATA became actual acceptance"
        );
        ensure!(before == h.host.control.manager.list_thread_ids().await);
        ensure!(
            bindings
                == h.host
                    .control
                    .registry
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .bindings
                    .len()
        );
        ensure!(!h.input.is_stopped());
        // Ordinary original Start remains usable after confirmed denials.
        let started: ThreadStartResponse = h
            .request(
                ConnectionId(41),
                &c1,
                json!({"id":8963,"method":"thread/start","params":params}),
            )
            .await?;
        let id = ThreadId::from_string(&started.thread.id)?;
        ensure!(id != h.parent_id);
        let actual = h.host.control.manager.get_thread(id).await?;
        ensure!(!actual.has_shutdown_status());
        Ok(())
    }
    .await;
    let drained = initial
        .drain()
        .await
        .map_err(|error| anyhow::anyhow!(error));
    let cleanup = h.shutdown().await;
    result.and(drained).and(cleanup)
}
