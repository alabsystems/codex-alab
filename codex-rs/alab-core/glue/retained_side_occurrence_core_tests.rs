// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual Core/queue/canonical-record controls. The ordinary Harness supplies
//! real original Core work; no Native origin, reservation or UI grant exists.
use super::*;
use wiremock::MockServer;

#[tokio::test]
async fn actual_direct_core_boundary_records_in_ephemeral_legacy_and_paginated_modes() -> Result<()>
{
    for (persistent, mode) in [
        (false, codex_app_server_protocol::ThreadHistoryMode::Legacy),
        (true, codex_app_server_protocol::ThreadHistoryMode::Legacy),
        (
            true,
            codex_app_server_protocol::ThreadHistoryMode::Paginated,
        ),
    ] {
        let model = MockServer::start().await;
        let mut h = Harness::new_with_history_mode(&model.uri(), persistent, Some(mode)).await?;
        let result: Result<()> = async {
            let original = h.parent.prepare_retained_side_boundary()?;
            let checkpoint = original.clone();
            bounded(original.inject(&h.parent, original_injected_items()?)).await??;
            let recorded = bounded(checkpoint.recorded(&h.parent)).await??;
            recorded.check_owner(&h.parent)?;
            let local = recorded.canonical_local_items(&h.parent, 1 << 20).await?;
            ensure!(local.iter().filter(|item| matches!(item,
                codex_rollout::RolloutItem::ResponseItem(item)
                if item.metadata.as_ref().and_then(|metadata| metadata.alab_side_boundary.as_ref()) == Some(recorded.marker_data())
            )).count() == 1, "same actual writer/journal bridge lost the canonical occurrence");
            ensure!(recorded.canonical_local_items(&h.parent, 1).await.is_err(), "bounded observation invented partial/empty history");
            ensure!(!recorded.was_queued_data(), "direct original route was mislabeled queued");
            ensure!(recorded.marker_data().same_local(recorded.marker_data()));
            if persistent {
                // A test oracle on the original Core's known writer output,
                // never a production read fallback or a Core/Native witness.
                let path = h.parent.rollout_path().context("original persistent writer path missing")?;
                let bytes = tokio::fs::read(path).await?;
                let mut count = 0;
                for line in bytes.split(|byte| *byte == b'\n').filter(|line| !line.is_empty()) {
                    let line = codex_rollout::parse_rollout_line_bytes(line)?;
                    if let codex_rollout::RolloutItem::ResponseItem(item) = line.item
                        && let Some(marker) = item.metadata.and_then(|metadata| metadata.alab_side_boundary)
                        && marker == *recorded.marker_data() {
                        ensure!(!marker.same_local(recorded.marker_data()), "file DATA recreated a local receipt");
                        count += 1;
                    }
                }
                ensure!(count == 1, "original writer did not persist the one actual occurrence");
            } else {
                let history = h.parent.exec_recovery_history().await?;
                let count = history.items.iter().filter(|item| matches!(item,
                    codex_rollout::RolloutItem::ResponseItem(item)
                    if item.metadata.as_ref().and_then(|metadata| metadata.alab_side_boundary.as_ref()) == Some(recorded.marker_data())
                )).count();
                ensure!(count == 1, "actual ephemeral canonical journal lost or duplicated the occurrence");
            }
            ensure!(model.received_requests().await.context("model request observation absent")?.is_empty());
            Ok(())
        }.await;
        let cleanup = h.shutdown().await;
        result?;
        cleanup?;
    }
    Ok(())
}

#[tokio::test]
async fn active_core_injection_ack_does_not_become_recorded_until_real_queue_consumption()
-> Result<()> {
    let (release, gate) = oneshot::channel();
    let (model, _) = start_streaming_sse_server(vec![
        vec![StreamingSseChunk {
            gate: Some(gate),
            body: responses::sse(vec![
                responses::ev_response_created("side-occurrence-before-queue"),
                responses::ev_assistant_message(
                    "side-occurrence-before-queue",
                    "Original active response",
                ),
                responses::ev_completed("side-occurrence-before-queue"),
            ]),
        }],
        vec![StreamingSseChunk {
            gate: None,
            body: responses::sse(vec![
                responses::ev_response_created("side-occurrence-after-queue"),
                responses::ev_assistant_message(
                    "side-occurrence-after-queue",
                    "Original continued response",
                ),
                responses::ev_completed("side-occurrence-after-queue"),
            ]),
        }],
    ])
    .await;
    let mut h = match Harness::new(model.uri()).await {
        Ok(h) => h,
        Err(error) => {
            let _ = release.send(());
            model.shutdown().await;
            return Err(error);
        }
    };
    let mut release = Some(release);
    let result: Result<()> = async {
        let c1 = Arc::clone(&h.c1);
        let _: codex_app_server_protocol::TurnStartResponse = h.request(ConnectionId(41), &c1,
            json!({"id":7991,"method":"turn/start","params":{
                "threadId":h.parent_id.to_string(),"input":[{"type":"text","text":"Hold the actual original turn"}]
            }})).await?;
        bounded(model.wait_for_request_count(1)).await?;
        ensure!(h.parent.has_active_turn().await);
        let boundary = h.parent.prepare_retained_side_boundary()?;
        bounded(boundary.inject(&h.parent, original_injected_items()?)).await??;
        // The real original injection returned after queuing. No fake
        // recording flag/empty snapshot can discharge this pending receipt.
        let mut recorded = Box::pin(boundary.recorded(&h.parent));
        ensure!(futures::poll!(recorded.as_mut()).is_pending());
        release.take().context("original model gate missing")?.send(()).map_err(|_| anyhow::anyhow!("original model gate ended"))?;
        let actual = bounded(recorded).await??;
        ensure!(actual.was_queued_data());
        actual.check_owner(&h.parent)?;
        Ok(())
    }.await;
    // Release original I/O before actual Core/model cleanup on EVERY path.
    if let Some(release) = release {
        let _ = release.send(());
    }
    let cleanup = h.shutdown().await;
    model.shutdown().await;
    result?;
    cleanup?;
    Ok(())
}

#[tokio::test]
async fn wrong_actual_core_is_denied_and_unpolled_original_injection_stays_unknown_without_history_mutation()
-> Result<()> {
    let model = MockServer::start().await;
    let mut h = Harness::new(&model.uri()).await?;
    let result: Result<()> = async {
        let c1 = Arc::clone(&h.c1);
        let other: codex_app_server_protocol::ThreadStartResponse = h
            .request(
                ConnectionId(41),
                &c1,
                json!({"id":7992,"method":"thread/start","params":{}}),
            )
            .await?;
        let other_id = ThreadId::from_string(&other.thread.id)?;
        let other = h.host.control.manager.get_thread(other_id).await?;
        let prepared = h.parent.prepare_retained_side_boundary()?;
        ensure!(
            prepared
                .inject(&other, original_injected_items()?)
                .await
                .is_err()
        );
        let before = h
            .parent
            .conversation_history_snapshot()
            .await
            .history_version();
        let original = prepared.inject(&h.parent, original_injected_items()?);
        drop(original);
        ensure!(prepared.recorded(&h.parent).await.is_err());
        ensure!(
            prepared
                .inject(&h.parent, original_injected_items()?)
                .await
                .is_err(),
            "unknown injection was resubmitted"
        );
        ensure!(
            h.parent
                .conversation_history_snapshot()
                .await
                .history_version()
                == before
        );
        ensure!(
            model
                .received_requests()
                .await
                .context("model request observation absent")?
                .is_empty()
        );
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result?;
    cleanup?;
    Ok(())
}

/// Ordinary typed input exercises the actual Core queue/canonical recorder.
/// Full frontend Side policy projection remains an independent caller control.
fn original_injected_items() -> Result<Vec<codex_protocol::models::ResponseItem>> {
    Ok(serde_json::from_value(
        json!([{"type":"message","role":"user","content":[{"type":"input_text","text":"Original Core canonical occurrence control"}]}]),
    )?)
}
