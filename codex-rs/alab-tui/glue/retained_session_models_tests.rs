// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Production original task/join/result owner with actual held Tokio workers.
//! These controls construct no Native/source currentness or model permission.
use super::*;
use std::future::Future;
use std::future::poll_fn;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
const WAIT: std::time::Duration = std::time::Duration::from_secs(60);
#[test]
fn prefetched_native_model_state_survives_wire_including_skipped_multi_agent_choice()
-> io::Result<()> {
    let payload = serde_json::json!({"id":"original-prefetched-model","model":"original-prefetched-model","displayName":"Original Prefetched Model","description":"Actual native model data","hidden":false,"supportedReasoningEfforts":[{"reasoningEffort":"medium","description":"Medium"}],"defaultReasoningEffort":"medium","isDefault":true});
    let mut original =
        model_preset_from_api_model(serde_json::from_value(payload).map_err(invalid)?);
    original.multi_agent_version = Some(codex_protocol::protocol::MultiAgentVersion::V2);
    let captured = Data::capture(&Ok(vec![original.clone()]), &ReadCapture::prepare())?;
    let raw = serde_json::to_vec(&captured).map_err(invalid)?;
    let restored: Data = serde_json::from_slice(&raw).map_err(invalid)?;
    let prepared = restored.prepare()?;
    assert_eq!(prepared.native.map_err(invalid)?, vec![original]);
    let wire: serde_json::Value = serde_json::from_slice(&raw).map_err(invalid)?;
    for field in ["multi_agent", "native"] {
        let mut bad = wire.clone();
        bad["models"][0].as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_slice::<Data>(&serde_json::to_vec(&bad).map_err(invalid)?).is_err(),
            "{field}"
        );
    }
    for field in [
        "supported_in_api",
        "input_modalities",
        "supports_personality",
    ] {
        let mut bad = wire.clone();
        bad["models"][0]["native"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(
            serde_json::from_slice::<Data>(&serde_json::to_vec(&bad).map_err(invalid)?).is_err(),
            "{field}"
        );
    }
    let mut bad = wire;
    bad["models"][0]["native"]["unknown_future_field"] = serde_json::json!(true);
    assert!(serde_json::from_slice::<Data>(&serde_json::to_vec(&bad).map_err(invalid)?).is_err());
    Ok(())
}
async fn finish(job: &Arc<Job>) -> io::Result<()> {
    match tokio::time::timeout(WAIT, job.join()).await {
        Ok(result) => result,
        Err(_) => {
            if let Some(handle) = job
                .lifecycle
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pending
                .as_ref()
            {
                handle.abort();
            }
            let _joined = job.join().await;
            Err(invalid("same original model worker timed out"))
        }
    }
}
async fn cleanup(job: &Arc<Job>) {
    if let Some(handle) = job
        .lifecycle
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .pending
        .as_ref()
    {
        handle.abort();
    }
    let _joined = finish(job).await;
}
#[tokio::test]
async fn cancelled_ordinary_observer_preserves_same_original_worker_and_late_native_result()
-> io::Result<()> {
    let book = Book::default();
    let job = Job::prepare(Arc::downgrade(&book.0));
    assert!(book.register(&job));
    let calls = Arc::new(AtomicUsize::new(0));
    let worker_calls = calls.clone();
    let (release, released) = tokio::sync::oneshot::channel();
    job.start(async move {
        worker_calls.fetch_add(1, Ordering::SeqCst);
        released
            .await
            .map_err(|_| color_eyre::eyre::eyre!("test release lost"))?;
        Ok(Vec::new())
    });
    let body = async {
        if job.progress.lock().map_err(invalid)?.lost {
            return Err(invalid(
                "accepted unpolled model worker lost its actual guard",
            ));
        }
        {
            let mut observer = std::pin::pin!(Observer(job.clone()));
            poll_fn(|cx| match observer.as_mut().poll(cx) {
                Poll::Pending => Poll::Ready(Ok(())),
                Poll::Ready(_) => Poll::Ready(Err(invalid(
                    "ordinary result arrived before actual release",
                ))),
            })
            .await?;
        }
        if job.consumed() {
            return Err(invalid("canceled observer consumed original result"));
        }
        release
            .send(())
            .map_err(|_| invalid("actual worker release failed"))?;
        finish(&job).await?;
        let native = Observer(job.clone()).await.map_err(invalid)?;
        if !native.is_empty()
            || calls.load(Ordering::SeqCst) != 1
            || !job.consumed()
            || !matches!(job.snapshot()?, Data::Ready { .. })
        {
            return Err(invalid("same original prefetch/result was not preserved"));
        }
        Ok(())
    }
    .await;
    cleanup(&job).await;
    body
}
#[tokio::test]
async fn canceled_join_restores_same_handle_until_original_worker_really_finishes() -> io::Result<()>
{
    let book = Book::default();
    let job = Job::prepare(Arc::downgrade(&book.0));
    assert!(book.register(&job));
    let (release, released) = tokio::sync::oneshot::channel();
    job.start(async move {
        released
            .await
            .map_err(|_| color_eyre::eyre::eyre!("test release lost"))?;
        Ok(Vec::new())
    });
    let body = async {
        {
            let mut observer = std::pin::pin!(job.join());
            poll_fn(|cx| match observer.as_mut().poll(cx) {
                Poll::Pending => Poll::Ready(Ok(())),
                Poll::Ready(_) => {
                    Poll::Ready(Err(invalid("join finished before original worker release")))
                }
            })
            .await?;
        }
        if job.lifecycle.lock().map_err(invalid)?.pending.is_none()
            || job.lifecycle.lock().map_err(invalid)?.joined
        {
            return Err(invalid("canceled observer lost original unjoined handle"));
        }
        release
            .send(())
            .map_err(|_| invalid("actual worker release failed"))?;
        finish(&job).await?;
        if !job.lifecycle.lock().map_err(invalid)?.joined {
            return Err(invalid("original worker did not actually join"));
        }
        Ok(())
    }
    .await;
    cleanup(&job).await;
    body
}
#[tokio::test]
async fn actual_worker_loss_stays_refused_on_repeated_join_and_capture() -> io::Result<()> {
    let book = Book::default();
    let job = Job::prepare(Arc::downgrade(&book.0));
    assert!(book.register(&job));
    job.start(std::future::pending());
    let body = async {
        if let Some(handle) = job.lifecycle.lock().map_err(invalid)?.pending.as_ref() {
            handle.abort();
        }
        let first = finish(&job).await;
        let second = finish(&job).await;
        if first.is_ok()
            || second.is_ok()
            || job.snapshot().is_ok()
            || book.capture().observe().await.is_ok()
        {
            return Err(invalid(
                "actual original worker loss was reclassified as complete",
            ));
        }
        Ok(())
    }
    .await;
    cleanup(&job).await;
    body
}
#[test]
fn prepared_historical_cache_is_not_an_invented_joined_task() -> io::Result<()> {
    let book = Book::default();
    let cached = Data::Ready { models: Vec::new() }
        .prepare()?
        .into_prefetch(&book);
    assert!(book.0.progress.lock().map_err(invalid)?.jobs.is_empty());
    let lifecycle = cached.0.lifecycle.lock().map_err(invalid)?;
    assert!(lifecycle.cached);
    assert!(!lifecycle.started && !lifecycle.joined && lifecycle.pending.is_none());
    drop(lifecycle);
    assert!(matches!(cached.0.snapshot()?, Data::Ready { .. }));
    Ok(())
}
fn actual_session() -> AppServerSession {
    let (client, commands, events) = codex_app_server_client::CloudClient::channel(2);
    drop(commands);
    drop(events);
    AppServerSession::new(
        crate::backend_id::BackendId::new(),
        codex_app_server_client::AppServerClient::Cloud(client),
        ThreadParamsMode::Cloud,
    )
}
#[tokio::test]
async fn actual_shutdown_admission_refuses_before_spawning_any_new_model_job() -> io::Result<()> {
    let mut session = actual_session();
    session.alab_model_jobs.stop();
    session.prefetch_model_list();
    assert!(
        session
            .alab_model_jobs
            .0
            .progress
            .lock()
            .map_err(invalid)?
            .jobs
            .is_empty()
    );
    let owner = session
        .prefetched_model_list
        .as_ref()
        .ok_or_else(|| invalid("local refused operation not retained"))?;
    assert!(!owner.0.lifecycle.lock().map_err(invalid)?.started);
    assert!(owner.0.lifecycle.lock().map_err(invalid)?.refused);
    let observer =
        observer(&mut session).ok_or_else(|| invalid("original local refusal observer missing"))?;
    assert!(observer.await.is_err());
    assert!(session.alab_model_jobs.capture().observe().await.is_err());
    Ok(())
}
#[tokio::test]
async fn actual_new_prefetch_invalidates_old_cut_and_request_view_cannot_supply_owning_metadata()
-> io::Result<()> {
    let mut session = actual_session();
    let old = session.alab_capture_original_model_jobs();
    session.prefetch_model_list();
    let body = async {
        if old
            .observe()
            .await
            .err()
            .ok_or_else(|| invalid("old empty cut ignored new actual job"))?
            .kind()
            != io::ErrorKind::WouldBlock
        {
            return Err(invalid(
                "changed original cut has different error classification",
            ));
        }
        let current = session.alab_capture_original_model_jobs();
        let ready = tokio::time::timeout(WAIT, current.observe())
            .await
            .map_err(|_| invalid("actual model cut join timed out"))??;
        if !matches!(ready.current(&session)?, Some(Data::Ready { .. })) {
            return Err(invalid("actual current prefetch result was omitted"));
        }
        let view = session.request_view();
        if ready.current(&view).is_ok() {
            return Err(invalid(
                "request-only view supplied owning-session metadata",
            ));
        }
        Ok(())
    }
    .await;
    if let Some(owner) = &session.prefetched_model_list {
        cleanup(&owner.0).await;
    }
    body
}
