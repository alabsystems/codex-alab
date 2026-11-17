// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use wiremock::MockServer;

#[tokio::test]
async fn actual_fresh_core_delegates_same_writer_then_observes_real_name_materialization()
-> Result<()> {
    let model = MockServer::start().await;
    let mut h = Harness::new_persistent(&model.uri()).await?;
    let result: Result<()> = async {
        let observed = h.parent.retained_deferred_history().await?.context("actual fresh Core deferred writer")?;
        ensure!(observed.thread_id == h.parent_id && observed.session_meta.id == h.parent_id);
        ensure!(observed.session_meta.history_base.is_none(), "fresh root has no inherited lineage");
        let path = h.parent.rollout_path().context("actual Core rollout path")?;
        ensure!(!tokio::fs::try_exists(&path).await?);
        let c1 = Arc::clone(&h.c1);
        let _: codex_app_server_protocol::ThreadSetNameResponse = h.request(ConnectionId(41), &c1,
            json!({"id":7801,"method":"thread/name/set","params":{"threadId":h.parent_id.to_string(),"name":"Deferred snapshot materialized"}})).await?;
        ensure!(h.parent.retained_deferred_history().await?.is_none());
        ensure!(tokio::fs::try_exists(path).await?);
        Ok(())
    }.await;
    let cleanup = h.shutdown().await;
    result?;
    cleanup?;
    Ok(())
}

#[tokio::test]
async fn old_actual_core_cannot_observe_typed_cold_resumes_replacement_writer() -> Result<()> {
    let model = MockServer::start().await;
    let mut h = Harness::new_persistent(&model.uri()).await?;
    let result: Result<()> = async {
        let original = Arc::clone(&h.parent);
        let id = h.parent_id;
        ensure!(original.retained_deferred_history().await?.is_some());
        let c1 = Arc::clone(&h.c1);
        let _: codex_app_server_protocol::ThreadSetNameResponse = h.request(ConnectionId(41), &c1,
            json!({"id":7811,"method":"thread/name/set","params":{"threadId":id.to_string(),"name":"Actual writer incarnation control"}})).await?;
        // Real Core shutdown and exact manager removal retain the original Arc
        // for observation. The following resume is the ORIGINAL typed policy.
        original.shutdown_and_wait().await?;
        h.processor.clear_all_thread_listeners().await;
        let removed = h.host.control.manager.remove_thread_if_matches(&id, &original).await
            .context("original actual Core was not registered")?;
        ensure!(Arc::ptr_eq(&removed, &original));
        let resumed: codex_app_server_protocol::ThreadResumeResponse = h.request(ConnectionId(41), &c1,
            json!({"id":7812,"method":"thread/resume","params":{"threadId":id.to_string(),"excludeTurns":true}})).await?;
        ensure!(resumed.thread.id == id.to_string());
        let current = h.host.control.manager.get_thread(id).await?;
        ensure!(!Arc::ptr_eq(&current, &original), "control did not create a distinct actual Core");
        ensure!(matches!(original.retained_deferred_history().await,
            Err(codex_thread_store::ThreadStoreError::Conflict { .. })), "old Core followed current logical-id writer");
        ensure!(current.retained_deferred_history().await?.is_none(), "real durable resume fabricated deferred data");
        Ok(())
    }.await;
    let cleanup = h.shutdown().await;
    result?;
    cleanup?;
    Ok(())
}

// The original typed fork deliberately materializes persistent children. This
// control preserves that policy rather than inventing a deferred fork Core.
#[tokio::test]
async fn actual_typed_fork_preserves_materialized_history_and_deferred_none() -> Result<()> {
    let (release, gate) = oneshot::channel();
    let (model, _) = start_streaming_sse_server(vec![vec![StreamingSseChunk {
        gate: Some(gate),
        body: responses::sse(vec![
            responses::ev_response_created("deferred-core-fork-parent"),
            responses::ev_assistant_message(
                "deferred-core-fork-parent",
                "Actual inherited Core reply",
            ),
            responses::ev_completed("deferred-core-fork-parent"),
        ]),
    }]])
    .await;
    let mut h = match Harness::new_persistent(model.uri()).await {
        Ok(h) => h,
        Err(error) => {
            let _ = release.send(());
            model.shutdown().await;
            return Err(error);
        }
    };
    let mut release = Some(release);
    let result: Result<()> = async {
        // Before any persistence, the same actual Core bridge returns its real
        // local snapshot without creating its rollout.
        let before = h
            .parent
            .retained_deferred_display_history()
            .await?
            .context("fresh actual Core display snapshot missing")?;
        ensure!(before.local.thread_id == h.parent_id && before.inherited_items.is_empty());
        let parent_path = h
            .parent
            .rollout_path()
            .context("parent rollout path missing")?;
        ensure!(!tokio::fs::try_exists(&parent_path).await?);
        let c1 = Arc::clone(&h.c1);
        let turn: codex_app_server_protocol::TurnStartResponse = h
            .request(
                ConnectionId(41),
                &c1,
                json!({"id":7821,"method":"turn/start","params":{
                    "threadId":h.parent_id.to_string(),
                    "input":[{"type":"text","text":"Produce the inherited Core reply"}]
                }}),
            )
            .await?;
        bounded(model.wait_for_request_count(1)).await?;
        ensure!(
            h.parent.has_active_turn().await,
            "parent response gate did not hold an actual turn"
        );
        release
            .take()
            .context("parent model release missing")?
            .send(())
            .map_err(|_| anyhow::anyhow!("parent model response gate ended"))?;
        bounded(async {
            while h.parent.has_active_turn().await {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?;
        h.parent.flush_rollout().await?;
        ensure!(
            tokio::fs::try_exists(&parent_path).await?,
            "real parent turn did not materialize history"
        );
        ensure!(
            h.parent
                .retained_deferred_display_history()
                .await?
                .is_none()
        );
        // excludeTurns is the ORIGINAL cheap typed fork request. It avoids
        // response hydration, not Core's Standard persistence barrier.
        let forked: codex_app_server_protocol::ThreadForkResponse = h
            .request(
                ConnectionId(41),
                &c1,
                json!({"id":7822,"method":"thread/fork","params":{
                    "threadId":h.parent_id.to_string(),"excludeTurns":true
                }}),
            )
            .await?;
        let child_id = ThreadId::from_string(&forked.thread.id)?;
        let child = h.host.control.manager.get_thread(child_id).await?;
        ensure!(child_id != h.parent_id && !Arc::ptr_eq(&child, &h.parent));
        ensure!(forked.thread.forked_from_id == Some(h.parent_id.to_string()));
        let child_path = child
            .rollout_path()
            .context("typed persistent fork path missing")?;
        ensure!(
            tokio::fs::try_exists(&child_path).await?,
            "original typed fork skipped Standard materialization"
        );
        ensure!(child.retained_deferred_history().await?.is_none());
        ensure!(
            child.retained_deferred_display_history().await?.is_none(),
            "materialized typed fork fabricated a deferred display prefix"
        );
        let inherited: codex_app_server_protocol::ThreadTurnsListResponse = h
            .request(
                ConnectionId(41),
                &c1,
                json!({"id":7823,"method":"thread/turns/list","params":{
                    "threadId":child_id.to_string(),"limit":10,"itemsView":"full"
                }}),
            )
            .await?;
        let actual = inherited
            .data
            .iter()
            .find(|actual| actual.id == turn.turn.id)
            .context("original typed fork lost its inherited parent turn")?;
        let actual = serde_json::to_value(actual)?;
        ensure!(actual["status"] == "completed");
        ensure!(
            actual["items"].as_array().is_some_and(|items| items
                .iter()
                .any(|item| item["type"] == "agentMessage"
                    && item["text"] == "Actual inherited Core reply")),
            "original typed history lost the actual inherited model reply"
        );
        ensure!(
            model.requests().await.len() == 1,
            "history observation submitted another model request"
        );
        Ok(())
    }
    .await;
    // Release the actual response gate before draining on EVERY error path.
    if let Some(release) = release.take() {
        let _ = release.send(());
    }
    let cleanup = h.shutdown().await;
    model.shutdown().await;
    result.and(cleanup)
}
