// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual Core/writer/journal history controls at the production capture helper.
//! No Native origin, public frontier, receipt, UI ACK or Core-current grant is
//! fabricated. Full paired listener/UI qualification remains a separate route.
use super::*;
use crate::request_processors::alab_retained_projection_history as projection_history;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_protocol::items::ReasoningItem;
use codex_protocol::items::TurnItem as CoreItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_rollout::RolloutItem;
use wiremock::MockServer;

fn source<T>(value: std::result::Result<T, JSONRPCErrorError>) -> Result<T> {
    value.map_err(|error| anyhow::anyhow!("{} ({})", error.message, error.code))
}
fn canonical_records(id: ThreadId) -> Vec<RolloutItem> {
    vec![
        RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "actual-deferred-canonical-turn".into(),
            turn_attribution: None,
            root_turn_id: None,
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        })),
        RolloutItem::EventMsg(EventMsg::ItemCompleted(ItemCompletedEvent {
            content_token_count: None,
            thread_id: id,
            turn_id: "actual-deferred-canonical-turn".into(),
            item: CoreItem::Reasoning(ReasoningItem {
                id: "actual-deferred-reasoning".into(),
                summary_text: vec!["Actual deferred memory content".into()],
                raw_content: Vec::new(),
            }),
            started_at_ms: Some(1),
            completed_at_ms: 2,
        })),
    ]
}

#[tokio::test]
async fn current_display_preserves_real_deferred_core_then_uses_original_append_materialization()
-> Result<()> {
    let model = MockServer::start().await;
    let mut h = Harness::new_with_history_mode(
        &model.uri(), true, Some(codex_app_server_protocol::ThreadHistoryMode::Paginated),
    ).await?;
    let result: Result<()> = async {
        let id = h.parent_id;
        let path = h.parent.rollout_path().context("actual deferred path missing")?;
        let before = h.parent.retained_deferred_history().await?.context("fresh actual Core deferred writer missing")?;
        ensure!(before.thread_id == id && before.session_meta.id == id);
        ensure!(!tokio::fs::try_exists(&path).await?);
        let (config, _) = h.parent.resume_config_snapshot().await;
        ensure!(config.history_mode == codex_protocol::protocol::ThreadHistoryMode::Paginated);
        let original = source(projection_history::capture(id, &h.parent, &config, true, false, false, false).await)?;
        ensure!(!original.include_turns && original.history_items.is_empty());
        let display = source(projection_history::capture(id, &h.parent, &config, true, false, false, true).await)?;
        ensure!(!display.durable_pages && display.include_turns && display.memory_mode == Some(config.history_mode));
        ensure!(serde_json::to_value(&display.history_items)? == serde_json::to_value(&before.items)?);
        let after = h.parent.retained_deferred_history().await?.context("capture materialized fresh actual writer")?;
        ensure!(serde_json::to_value(&after.items)? == serde_json::to_value(&before.items)?);
        ensure!(!tokio::fs::try_exists(&path).await?, "UI capture forced persistence");

        // Original Core append ALWAYS flushes under the same live-writer
        // lifecycle lock. Nonempty deferred queues are directly covered by the
        // recorder controls; do not fabricate that state in a returned Core append.
        h.parent.append_rollout_items(&canonical_records(id)).await?;
        ensure!(tokio::fs::try_exists(&path).await?, "original append did not materialize its actual writer");
        ensure!(h.parent.retained_deferred_history().await?.is_none(), "materialized writer fabricated a deferred cut");
        ensure!(Arc::ptr_eq(&h.parent, &h.host.control.manager.get_thread(id).await?), "capture replaced actual Core");
        let durable = source(projection_history::capture(id, &h.parent, &config, true, false, false, true).await)?;
        ensure!(durable.durable_pages && !durable.include_turns && durable.memory_mode.is_none() && durable.history_items.is_empty());
        let c1 = h.c1.clone();
        let history: codex_app_server_protocol::ThreadTurnsListResponse = h.request(ConnectionId(41), &c1,
            json!({"id":7971,"method":"thread/turns/list","params":{"threadId":id.to_string(),"limit":10,"itemsView":"full"}})).await?;
        ensure!(serde_json::to_string(&history.data)?.contains("Actual deferred memory content"), "original appended canonical history was lost");
        ensure!(h.parent.retained_deferred_history().await?.is_none());
        Ok(())
    }.await;
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn unrelated_original_metadata_read_preserves_fresh_deferred_and_hides_materialized_history()
-> Result<()> {
    let model = MockServer::start().await;
    let mut h = Harness::new_with_history_mode(
        &model.uri(), true, Some(codex_app_server_protocol::ThreadHistoryMode::Paginated),
    ).await?;
    let result: Result<()> = async {
        let id = h.parent_id;
        let path = h.parent.rollout_path().context("actual deferred path missing")?;
        let before = h.parent.retained_deferred_history().await?.context("actual fresh writer missing")?;
        ensure!(!tokio::fs::try_exists(&path).await?);
        let c1 = h.c1.clone();
        let read: codex_app_server_protocol::ThreadReadResponse = h.request(ConnectionId(41), &c1,
            json!({"id":7981,"method":"thread/read","params":{"threadId":id.to_string(),"includeTurns":false}})).await?;
        ensure!(read.thread.turns.is_empty(), "unrelated metadata-only RPC exposed UI snapshot");
        let after = h.parent.retained_deferred_history().await?.context("metadata read materialized fresh writer")?;
        ensure!(serde_json::to_value(&before.items)? == serde_json::to_value(&after.items)?);
        ensure!(!tokio::fs::try_exists(&path).await?);

        // Cross the REAL original persistence boundary, then retain the same
        // metadata-only oracle with actual nonempty stored canonical history.
        h.parent.append_rollout_items(&canonical_records(id)).await?;
        ensure!(tokio::fs::try_exists(&path).await? && h.parent.retained_deferred_history().await?.is_none());
        let history: codex_app_server_protocol::ThreadTurnsListResponse = h.request(ConnectionId(41), &c1,
            json!({"id":7983,"method":"thread/turns/list","params":{"threadId":id.to_string(),"limit":10,"itemsView":"full"}})).await?;
        ensure!(serde_json::to_string(&history.data)?.contains("Actual deferred memory content"));
        let read: codex_app_server_protocol::ThreadReadResponse = h.request(ConnectionId(41), &c1,
            json!({"id":7982,"method":"thread/read","params":{"threadId":id.to_string(),"includeTurns":false}})).await?;
        ensure!(read.thread.turns.is_empty(), "unrelated metadata-only RPC exposed stored display history");
        ensure!(h.parent.retained_deferred_history().await?.is_none());
        ensure!(Arc::ptr_eq(&h.parent, &h.host.control.manager.get_thread(id).await?));
        Ok(())
    }.await;
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn current_display_reads_actual_ephemeral_model_history_with_no_durable_cursor_route()
-> Result<()> {
    let (release, gate) = oneshot::channel();
    let (model, _) = start_streaming_sse_server(vec![vec![StreamingSseChunk {
        gate: Some(gate),
        body: responses::sse(vec![
            responses::ev_response_created("memory-ui-model"),
            responses::ev_assistant_message(
                "memory-ui-model",
                "Actual ephemeral current display reply",
            ),
            responses::ev_completed("memory-ui-model"),
        ]),
    }]])
    .await;
    let mut h = match Harness::new_persistent(model.uri()).await {
        Ok(h) => h,
        Err(error) => {
            let _sent = release.send(());
            model.shutdown().await;
            return Err(error);
        }
    };
    let mut release = Some(release);
    let result: Result<()> = async {
        let c1 = h.c1.clone();
        let started: codex_app_server_protocol::ThreadStartResponse = h.request(ConnectionId(41), &c1,
            json!({"id":7991,"method":"thread/start","params":{"ephemeral":true,"historyMode":"paginated"}})).await?;
        let id = ThreadId::from_string(&started.thread.id)?;
        let actual = h.host.control.manager.get_thread(id).await?;
        ensure!(actual.rollout_path().is_none(), "actual ephemeral Core has a persistent rollout");
        let _: codex_app_server_protocol::TurnStartResponse = h.request(ConnectionId(41), &c1,
            json!({"id":7992,"method":"turn/start","params":{"threadId":id.to_string(),
                "input":[{"type":"text","text":"Produce actual ephemeral history"}]}})).await?;
        bounded(model.wait_for_request_count(1)).await?;
        release.take().context("actual model release missing")?.send(())
            .map_err(|_| anyhow::anyhow!("actual model response gate ended"))?;
        bounded(async { while actual.has_active_turn().await { tokio::time::sleep(Duration::from_millis(10)).await; } }).await?;
        let (config, _) = actual.resume_config_snapshot().await;
        ensure!(config.ephemeral && config.history_mode == codex_protocol::protocol::ThreadHistoryMode::Paginated);
        let original = source(projection_history::capture(id, &actual, &config, true, false, false, false).await)?;
        ensure!(!original.include_turns && original.history_items.is_empty());
        let display = source(projection_history::capture(id, &actual, &config, true, false, false, true).await)?;
        ensure!(display.include_turns && !display.durable_pages && display.memory_mode == Some(config.history_mode));
        ensure!(serde_json::to_string(&display.history_items)?.contains("Actual ephemeral current display reply"));
        ensure!(actual.rollout_path().is_none() && actual.retained_deferred_history().await?.is_none());
        ensure!(model.requests().await.len() == 1, "history capture submitted another model/Core turn");
        Ok(())
    }.await;
    // Release actual IO before joining/draining on EVERY fallible body path.
    if let Some(release) = release.take() {
        let _sent = release.send(());
    }
    let cleanup = h.shutdown().await;
    model.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn current_display_of_fresh_legacy_and_paginated_cores_keeps_a_complete_empty_memory_cut()
-> Result<()> {
    let model = MockServer::start().await;
    let mut h = Harness::new_persistent(&model.uri()).await?;
    let result: Result<()> = async {
        for (request, mode) in [(7997, "legacy"), (7998, "paginated")] {
            let c1 = h.c1.clone();
            let response: codex_app_server_protocol::ThreadStartResponse = h
                .request(
                    ConnectionId(41),
                    &c1,
                    json!({"id":request,"method":"thread/start","params":{"historyMode":mode}}),
                )
                .await?;
            let id = ThreadId::from_string(&response.thread.id)?;
            let actual = h.host.control.manager.get_thread(id).await?;
            let before = actual
                .retained_deferred_history()
                .await?
                .context("fresh actual Core was already materialized")?;
            let path = actual
                .rollout_path()
                .context("fresh actual Core path missing")?;
            ensure!(!tokio::fs::try_exists(&path).await?);
            let (config, _) = actual.resume_config_snapshot().await;
            let paginated =
                config.history_mode == codex_protocol::protocol::ThreadHistoryMode::Paginated;
            ensure!(paginated == (mode == "paginated"));
            let captured = source(
                projection_history::capture(id, &actual, &config, paginated, false, false, true)
                    .await,
            )?;
            ensure!(
                captured.include_turns
                    && !captured.durable_pages
                    && captured.memory_mode == Some(config.history_mode)
            );
            ensure!(
                serde_json::to_value(&captured.history_items)?
                    == serde_json::to_value(&before.items)?
            );
            ensure!(!actual.has_active_turn().await && response.thread.turns.is_empty());
            ensure!(
                !tokio::fs::try_exists(path).await?,
                "empty memory observation materialized writer"
            );
        }
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}
