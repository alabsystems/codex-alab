// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real Core idle/lifetime control at the PRIVATE Source selection seam.
//! Actual RPC contexts/results/Arcs/leases are used; install_local/finish_install
//! deliberately stand alone. No AcceptedOrigin, NativeCommit or UI grant is made.
use super::*;
use crate::message_processor::ConnectionSessionState;
use crate::message_processor::alab_task_mcp::tests::Harness;
use crate::outgoing_message::ConnectionId;
use crate::request_processors::thread_processor::alab_initial_session::publication_tests;
use crate::transport::ConnectionOrigin;
use codex_app_server_protocol::InitializeResponse;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadSetNameResponse;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::ThreadUnsubscribeResponse;
use codex_app_server_protocol::ThreadUnsubscribeStatus;
use serde_json::json;
use std::time::Duration;

async fn bounded<T>(future: impl std::future::Future<Output = T>) -> Result<T> {
    tokio::time::timeout(Duration::from_secs(60), future)
        .await
        .context("selected lease control hang detector")
}
async fn idle_thread(h: &mut Harness, serial: i64) -> Result<(ThreadId, Arc<CodexThread>)> {
    let c1 = h.c1.clone();
    let started: ThreadStartResponse = h
        .request(
            ConnectionId(41),
            &c1,
            json!({"id":serial,"method":"thread/start","params":{
                "ephemeral":false,"historyMode":"legacy","config":{"thread_unload_delay_secs":1}
            }}),
        )
        .await
        .context("start actual Core for selected lease control")?;
    let id = ThreadId::from_string(&started.thread.id)?;
    let (processor, _) = h.processor.alab_selection_publication_parts_for_tests();
    let actual = processor.thread_manager.get_thread(id).await?;
    ensure!(actual.config().await.thread_unload_delay == Duration::from_secs(1));
    let _: ThreadSetNameResponse = h
        .request(
            ConnectionId(41),
            &c1,
            json!({"id":serial+1,"method":"thread/name/set","params":{
                "threadId":id.to_string(),"name":"Actual selected lease idle control"
            }}),
        )
        .await
        .context("name actual Core for selected lease control")?;
    Ok((id, actual))
}
async fn resumed(
    h: &mut Harness,
    connection: ConnectionId,
    session: &Arc<ConnectionSessionState>,
    id: ThreadId,
    serial: i64,
) -> Result<ThreadResumeResponse> {
    // No override: keep the existing loaded Core/config, never rebuild it.
    h.request(
        connection,
        session,
        json!({"id":serial,"method":"thread/resume","params":{
            "threadId":id.to_string(),"excludeTurns":true
        }}),
    )
    .await
    .with_context(|| {
        format!("resume actual selected lease Core on {connection:?}, request {serial}")
    })
}
async fn removed(
    processor: &ThreadRequestProcessor,
    id: ThreadId,
    actual: &Arc<CodexThread>,
) -> Result<()> {
    bounded(async {
        loop {
            if processor.thread_manager.get_thread(id).await.is_err() {
                ensure!(
                    actual.has_shutdown_status(),
                    "actual Core removal preceded shutdown"
                );
                return Ok::<(), anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;
    Ok(())
}

#[tokio::test]
async fn old_prepared_and_receipt_views_survive_two_local_handoffs_without_pinning_old_core()
-> Result<()> {
    let model = wiremock::MockServer::start().await;
    let mut h = Harness::new_persistent(&model.uri()).await?;
    let mut owned_operation: Option<Arc<InitialSessionOperation>> = None;
    let mut c2_cleanup = None;
    let result: Result<()> = async {
        let (old_id, old) = idle_thread(&mut h, 9700).await?;
        let (new_id, new) = idle_thread(&mut h, 9710).await?;
        let c1 = h.c1.clone();
        let old_response = resumed(&mut h, ConnectionId(41), &c1, old_id, 9720).await?;
        let new_response = resumed(&mut h, ConnectionId(41), &c1, new_id, 9721).await?;
        let (old_context, _, processor) = publication_tests::dispatched_context_for(
            &h, ConnectionId(41), &c1, 9730, old_id).await?;
        let (operation, old_scope) = publication_tests::publication_fixture(
            &h, processor.clone(), old_context, &c1)?;
        owned_operation = Some(operation.clone());
        let old_lease = source(processor.alab_acquire_primary(old_id, old.clone(), &h.input).await)?;
        {
            let mut progress = old_scope.progress.lock().unwrap_or_else(PoisonError::into_inner);
            progress.resolved = Some((old_id, old.clone()));
            progress.primary = Some(old_lease);
            progress.worker_finished = true;
            progress.complete = Some(Reply::Resume(Arc::new(old_response), InitialSessionPlan::fresh_claim()));
        }
        let prior_empty = source(operation.snapshot_selection())?;
        let old_candidate = source(operation.primary_candidate(&old_scope.candidate).await)?;
        let old_prepared = source(old_candidate.prepare_transition())?;
        source(install_local(&old_candidate, &old_prepared))?;
        let old_receipt = source(finish_install(&old_candidate, &old_prepared))?;
        let prior_old = source(operation.snapshot_selection())?;
        let prior_old_again = source(operation.snapshot_selection())?;
        ensure!(old_receipt.lease().is_some());

        let (new_context, _, _) = publication_tests::dispatched_context_for(
            &h, ConnectionId(41), &c1, 9731, new_id).await?;
        let (scratch, mut new_scope) = publication_tests::publication_fixture(
            &h, processor.clone(), new_context, &c1)?;
        // Reuse only the existing PRIVATE publication fixture. Move its unique
        // test Scope into this actual Source owner; no accept()/Native path runs.
        let scratch_owner = scratch.state.lock().unwrap_or_else(PoisonError::into_inner).accepted.take();
        drop(scratch_owner); // Any resource destructor runs outside the registry lock.
        {
            let scope = Arc::get_mut(&mut new_scope).context("fresh private fixture has another owner")?;
            scope.operation = Arc::downgrade(&operation);
            scope.prior_selection = source(operation.snapshot_selection())?;
        }
        operation.state.lock().unwrap_or_else(PoisonError::into_inner).clients
            .insert(new_scope.candidate.clone(), new_scope.clone());
        let new_lease = source(processor.alab_acquire_primary(new_id, new.clone(), &h.input).await)?;
        {
            let mut progress = new_scope.progress.lock().unwrap_or_else(PoisonError::into_inner);
            progress.resolved = Some((new_id, new.clone()));
            progress.primary = Some(new_lease);
            progress.worker_finished = true;
            progress.complete = Some(Reply::Resume(Arc::new(new_response), InitialSessionPlan::fresh_claim()));
        }
        let new_candidate = source(operation.primary_candidate(&new_scope.candidate).await)?;
        let new_prepared = source(new_candidate.prepare_transition())?;
        source(install_local(&new_candidate, &new_prepared))?;
        let new_receipt = source(finish_install(&new_candidate, &new_prepared))?;
        ensure!(!prior_empty.unchanged(&operation) && !prior_old.unchanged(&operation)
            && !prior_old_again.unchanged(&operation));
        ensure!(old_receipt.lease().is_none() && old_candidate.lease().is_none(),
            "historical selected/candidate view retained the old actual lease");
        ensure!(old_prepared.candidate_lease.lock().unwrap_or_else(PoisonError::into_inner).is_none());
        ensure!(new_prepared.prior.lock().unwrap_or_else(PoisonError::into_inner).is_none(),
            "completed second handoff still owns prior selection");
        ensure!(new_receipt.lease().is_some(), "current selection lost its real owner");

        let c2 = Arc::new(ConnectionSessionState::new(ConnectionOrigin::WebSocket));
        c2_cleanup = Some(c2.clone());
        let _: InitializeResponse = h.request(ConnectionId(42), &c2,
            json!({"id":9740,"method":"initialize","params":{
                "clientInfo":{"name":"retained-http-c1","version":"1"},
                "capabilities":{"experimentalApi":true,"requiredHumanApproval":true}
            }})).await?;
        for (index, id, actual) in [(0_i64, old_id, &old), (1, new_id, &new)] {
            let response = resumed(&mut h, ConnectionId(42), &c2, id, 9750+index).await?;
            ensure!(response.thread.id == id.to_string());
            let still = processor.thread_manager.get_thread(id).await?;
            ensure!(Arc::ptr_eq(&still, actual), "C2 rebuilt a loaded Core");
            let unsubscribed: ThreadUnsubscribeResponse = h.request(ConnectionId(42), &c2,
                json!({"id":9760+index,"method":"thread/unsubscribe","params":{"threadId":id.to_string()}})).await?;
            ensure!(unsubscribed.status == ThreadUnsubscribeStatus::Unsubscribed);
        }
        bounded(h.processor.connection_closed(ConnectionId(41), &c1)).await?;
        bounded(h.processor.connection_closed(ConnectionId(42), &c2)).await?;
        removed(&processor, old_id, &old).await?;
        let still_current = processor.thread_manager.get_thread(new_id).await?;
        ensure!(Arc::ptr_eq(&still_current, &new) && !new.has_shutdown_status(),
            "actual current primary did not outlive the same original idle deadline");
        // Keep ALL old views/prepared receipts alive through the runtime proof.
        // Source state/currentness now rejects old handles without retaining a lease.
        ensure!(old_receipt.recheck_owned().is_err());
        source(new_receipt.recheck().await)?;
        drop((old_prepared, old_candidate, old_receipt, prior_empty, prior_old,
            prior_old_again, new_prepared, new_candidate, new_receipt));
        Ok(())
    }.await;
    if let Some(operation) = owned_operation.take() {
        operation.stop();
        drop(operation);
    }
    let c2_closed = match c2_cleanup {
        Some(c2) => bounded(h.processor.connection_closed(ConnectionId(42), &c2))
            .await
            .map(|_| ()),
        None => Ok(()),
    };
    let cleanup = h.shutdown().await;
    result.and(c2_closed).and(cleanup)
}
