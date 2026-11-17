// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real Core/lease + PRIVATE Source installation observation. No actual SDK
//! Hold, AcceptedOrigin, NativeCommit or UI grant is fabricated. An early local
//! notification models the SDK-before-Source scheduling edge only.
use super::*;
use crate::message_processor::alab_task_mcp::tests::Harness;
use crate::outgoing_message::ConnectionId;
use crate::request_processors::thread_processor::alab_initial_session::publication_tests;
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadSetNameResponse;
use serde_json::json;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

async fn bounded<T>(future: impl std::future::Future<Output = T>) -> Result<T> {
    tokio::time::timeout(Duration::from_secs(60), future)
        .await
        .context("actual Source handoff control hang detector")
}
fn source<T>(result: std::result::Result<T, JSONRPCErrorError>) -> Result<T> {
    result.map_err(|error| anyhow::anyhow!("Source handoff error, code {}", error.code))
}
async fn fixture(
    h: &mut Harness,
    serial: i64,
) -> Result<(
    Arc<InitialSessionOperation>,
    PrimaryCandidate,
    Arc<Transition>,
    EmptyPrimary,
)> {
    let c1 = h.c1.clone();
    let id = h.parent_id;
    let _: ThreadSetNameResponse = h
        .request(
            ConnectionId(41),
            &c1,
            json!({"id":serial,"method":"thread/name/set","params":{
            "threadId":id.to_string(),"name":"Actual Source handoff control"}}),
        )
        .await?;
    let response: ThreadResumeResponse = h
        .request(
            ConnectionId(41),
            &c1,
            json!({"id":serial+1,"method":"thread/resume","params":{
            "threadId":id.to_string(),"excludeTurns":true}}),
        )
        .await?;
    let (context, _, processor) =
        publication_tests::dispatched_context_for(h, ConnectionId(41), &c1, serial + 2, id).await?;
    let (operation, scope) =
        publication_tests::publication_fixture(h, processor.clone(), context, &c1)?;
    let actual = processor.thread_manager.get_thread(id).await?;
    let lease = source(
        processor
            .alab_acquire_primary(id, actual.clone(), &h.input)
            .await,
    )?;
    {
        let mut progress = scope
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        progress.resolved = Some((id, actual));
        progress.primary = Some(lease);
        progress.worker_finished = true;
        progress.complete = Some(Reply::Resume(
            Arc::new(response),
            InitialSessionPlan::fresh_claim(),
        ));
    }
    let CurrentPrimary::Empty(empty) = source(operation.current_primary())? else {
        anyhow::bail!("Core construction selected a UI primary");
    };
    let candidate = source(operation.primary_candidate(&scope.candidate).await)?;
    let transition = source(candidate.prepare_transition())?;
    source(install_local(&candidate, &transition))?;
    Ok((operation, candidate, transition, empty))
}
async fn finish_worker(mut worker: JoinHandle<Result<()>>) -> Result<()> {
    match tokio::time::timeout(Duration::from_secs(60), &mut worker).await {
        Ok(joined) => joined.context("actual handoff worker failed")?,
        Err(_) => {
            worker.abort();
            let _joined = worker.await; // SAME original handle, no detached timeout.
            anyhow::bail!("actual handoff worker did not finish");
        }
    }
}

async fn abort_worker(mut worker: JoinHandle<()>) -> Result<()> {
    worker.abort();
    match tokio::time::timeout(Duration::from_secs(60), &mut worker).await {
        Ok(Err(error)) if error.is_cancelled() => Ok(()),
        Ok(_) => anyhow::bail!("actual canceled worker did not retain cancellation outcome"),
        Err(_) => {
            worker.abort();
            let _joined = worker.await; // Always join even the hang-detector path.
            anyhow::bail!("actual canceled worker did not finish");
        }
    }
}

#[tokio::test]
async fn early_sdk_style_notify_waits_for_actual_source_finish_without_failing_input() -> Result<()>
{
    let model = wiremock::MockServer::start().await;
    let mut h = Harness::new_persistent(&model.uri()).await?;
    let result: Result<()> = async {
        let (operation, candidate, transition, empty) = fixture(&mut h, 9890).await?;
        let (release, allowed) = oneshot::channel();
        let mut release = Some(release);
        let observation = InstallObservation::new(&candidate, &transition);
        let worker = tokio::spawn(async move {
            let observation = observation;
            allowed
                .await
                .context("actual Source finish control closed")?;
            source(finish_install(&candidate, &transition))?;
            observation.finish();
            Ok(())
        });
        let mut observing = Box::pin(empty.wait_selected_handoff());
        let checked: Result<()> = async {
            ensure!(futures::poll!(observing.as_mut()).is_pending());
            operation.selection_changed.notify_waiters(); // NOT a Native ACK.
            ensure!(
                futures::poll!(observing.as_mut()).is_pending(),
                "SDK-style wake became Source completion"
            );
            ensure!(
                h.input.failure().is_none(),
                "in-progress installation failed actual Input"
            );
            release
                .take()
                .context("actual release missing")?
                .send(())
                .map_err(|_| anyhow::anyhow!("actual Source worker ended"))?;
            let completed = source(bounded(observing.as_mut()).await?)?;
            let SelectionHandoff::Selected(selected) = completed else {
                anyhow::bail!("handoff was only stopped");
            };
            ensure!(selected.belongs_to(&h.input));
            ensure!(h.input.failure().is_none());
            Ok(())
        }
        .await;
        drop(observing);
        // Every failed assertion releases and joins the SAME original task.
        if let Some(release) = release.take() {
            let _sent = release.send(());
        }
        let joined = finish_worker(worker).await;
        operation.stop();
        drop((empty, operation));
        checked.and(joined)
    }
    .await;
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn lost_actual_install_publishes_unknown_and_retains_both_source_owners() -> Result<()> {
    let model = wiremock::MockServer::start().await;
    let mut h = Harness::new_persistent(&model.uri()).await?;
    let result: Result<()> = async {
        let (operation, candidate, transition, empty) = fixture(&mut h, 9910).await?;
        let observation = InstallObservation::new(&candidate, &transition);
        let worker = tokio::spawn(async move {
            let _observation = observation;
            std::future::pending::<()>().await;
        });
        let mut observing = Box::pin(empty.wait_selected_handoff());
        let pending = futures::poll!(observing.as_mut()).is_pending();
        let joined = abort_worker(worker).await;
        let observed = bounded(observing.as_mut()).await;
        drop(observing);
        let checked: Result<()> = async {
            ensure!(pending, "actual installation observer did not wait");
            joined?;
            ensure!(
                observed?.is_err(),
                "Unknown installation became a selected primary"
            );
            let slot = operation
                .selected_primary
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            ensure!(
                slot.installing
                    .as_ref()
                    .is_some_and(|actual| Arc::ptr_eq(actual, &transition))
            );
            ensure!(
                transition
                    .uncertain
                    .load(std::sync::atomic::Ordering::Acquire)
            );
            ensure!(
                transition
                    .candidate_lease
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .is_some()
            );
            Ok(())
        }
        .await;
        operation.stop();
        drop((empty, candidate, transition, operation));
        checked
    }
    .await;
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}

#[tokio::test]
async fn initial_stop_before_frontier_stop_wakes_only_the_same_source_observer() -> Result<()> {
    let model = wiremock::MockServer::start().await;
    let mut h = Harness::new_persistent(&model.uri()).await?;
    let result: Result<()> = async {
        let (operation, candidate, transition, empty) = fixture(&mut h, 9930).await?;
        let mut observing = Box::pin(empty.wait_selected_handoff());
        ensure!(futures::poll!(observing.as_mut()).is_pending());
        operation.stop(); // Actual dispatcher closes Initial BEFORE Binding.
        let observed = source(bounded(observing.as_mut()).await?)?;
        ensure!(matches!(observed, SelectionHandoff::Stopped));
        ensure!(
            h.input.failure().is_none(),
            "normal Source stop failed actual Input"
        );
        ensure!(
            transition
                .candidate_lease
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_some()
        );
        drop(observing);
        drop((empty, candidate, transition, operation));
        Ok(())
    }
    .await;
    let cleanup = h.shutdown().await;
    result.and(cleanup)
}
