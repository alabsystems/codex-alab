// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual physical command and Tokio task/join ownership. No Native grant is fabricated.
use super::*;

#[tokio::test]
async fn accepted_unpolled_shutdown_keeps_its_actual_guard_until_the_original_future_finishes()
-> io::Result<()> {
    let operation = Operation::prepare();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let original_calls = calls.clone();
    operation.start(
        tokio::runtime::Handle::current(),
        Book::default(),
        async move {
            original_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        },
    );
    let lost_before_poll = operation.progress.lock().map_err(invalid)?.lost;
    let result = observe(&operation).await;
    assert!(
        !lost_before_poll,
        "accepted unpolled shutdown lost its actual guard"
    );
    result?;
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(operation.lifecycle.lock().map_err(invalid)?.joined);
    Ok(())
}
use std::future::Future;
use std::future::poll_fn;
const WAIT: std::time::Duration = std::time::Duration::from_secs(60);
async fn observe(actual: &Arc<Operation>) -> io::Result<()> {
    match tokio::time::timeout(WAIT, actual.observe()).await {
        Ok(result) => result,
        Err(_) => {
            if let Some(handle) = actual
                .lifecycle
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pending
                .as_ref()
            {
                handle.abort();
            }
            let _joined = actual.observe().await;
            Err(invalid("same actual shutdown worker timed out"))
        }
    }
}
async fn clean_job(job: &Arc<Job>) {
    if let Some(handle) = job
        .lifecycle
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .pending
        .as_ref()
    {
        handle.abort();
    }
    if tokio::time::timeout(WAIT, job.join()).await.is_err() {
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
    }
}
#[tokio::test]
async fn canceled_consuming_shutdown_keeps_actual_physical_command_and_original_job_until_join()
-> io::Result<()> {
    let (client, mut commands, events) = codex_app_server_client::CloudClient::channel(2);
    let session = AppServerSession::new(
        crate::backend_id::BackendId::new(),
        codex_app_server_client::AppServerClient::Cloud(client),
        ThreadParamsMode::Cloud,
    );
    let book = session.alab_model_jobs.clone();
    let job = Job::prepare(Arc::downgrade(&book.0));
    if !book.register(&job) {
        return Err(invalid("actual original job registration refused"));
    }
    let (release, released) = tokio::sync::oneshot::channel();
    job.start(async move {
        released
            .await
            .map_err(|_| color_eyre::eyre::eyre!("original job release lost"))?;
        Ok(Vec::new())
    });
    let body = async {
        {
            let mut consuming = std::pin::pin!(shutdown_actual(session));
            poll_fn(|cx| match consuming.as_mut().poll(cx) {
                Poll::Pending => Poll::Ready(Ok(())),
                Poll::Ready(_) => Poll::Ready(Err(invalid(
                    "physical shutdown ended before original command response",
                ))),
            })
            .await?;
        }
        let command = tokio::time::timeout(WAIT, commands.recv())
            .await
            .map_err(|_| invalid("original physical shutdown command missing"))?
            .ok_or_else(|| invalid("physical command channel closed early"))?;
        if !matches!(
            command.operation,
            codex_app_server_client::CloudOperation::Shutdown
        ) {
            return Err(invalid(
                "a different physical command replaced original shutdown",
            ));
        }
        let actual = book
            .0
            .shutdown
            .lock()
            .map_err(invalid)?
            .as_ref()
            .cloned()
            .ok_or_else(|| invalid("canceled observer lost original shutdown operation"))?;
        command
            .response
            .send(Ok(Ok(serde_json::Value::Null)))
            .map_err(|_| invalid("actual shutdown response was not retained"))?;
        {
            let mut waiting = std::pin::pin!(actual.observe());
            poll_fn(|cx| match waiting.as_mut().poll(cx) {
                Poll::Pending => Poll::Ready(Ok(())),
                Poll::Ready(_) => Poll::Ready(Err(invalid(
                    "physical response skipped held original model worker join",
                ))),
            })
            .await?;
        }
        release
            .send(())
            .map_err(|_| invalid("same original model worker release failed"))?;
        observe(&actual).await?;
        if !job.lifecycle.lock().map_err(invalid)?.joined {
            return Err(invalid(
                "shutdown completion preceded same original model worker join",
            ));
        }
        Ok(())
    }
    .await;
    drop(commands);
    drop(events);
    clean_job(&job).await;
    let original = {
        book.0
            .shutdown
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .cloned()
    };
    if let Some(actual) = original {
        let _joined = observe(&actual).await;
    }
    body
}
#[derive(Debug)]
struct OriginalFailure;
impl std::fmt::Display for OriginalFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "same original shutdown payload")
    }
}
impl std::error::Error for OriginalFailure {}
#[tokio::test]
async fn canceled_shutdown_join_restores_same_handle_and_preserves_exact_first_native_error()
-> io::Result<()> {
    let book = Book::default();
    let actual = Operation::prepare();
    let (release, released) = tokio::sync::oneshot::channel();
    actual.start(tokio::runtime::Handle::current(), book, async move {
        released.await.map_err(invalid)?;
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            OriginalFailure,
        ))
    });
    let body = async {
        {
            let mut waiting = std::pin::pin!(actual.observe());
            poll_fn(|cx| match waiting.as_mut().poll(cx) {
                Poll::Pending => Poll::Ready(Ok(())),
                Poll::Ready(_) => Poll::Ready(Err(invalid(
                    "shutdown task completed before actual release",
                ))),
            })
            .await?;
        }
        if actual.lifecycle.lock().map_err(invalid)?.pending.is_none() {
            return Err(invalid("canceled shutdown join lost same handle"));
        }
        release
            .send(())
            .map_err(|_| invalid("actual shutdown release failed"))?;
        let error = observe(&actual)
            .await
            .err()
            .ok_or_else(|| invalid("original shutdown failure became success"))?;
        if error.kind() != io::ErrorKind::PermissionDenied
            || !error
                .get_ref()
                .is_some_and(
                    <dyn std::error::Error + Send + Sync + 'static>::is::<OriginalFailure>,
                )
        {
            return Err(invalid("first native shutdown error was flattened"));
        }
        if observe(&actual).await.is_ok() {
            return Err(invalid(
                "repeated observation reran or erased actual shutdown failure",
            ));
        }
        Ok(())
    }
    .await;
    if let Some(handle) = actual
        .lifecycle
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .pending
        .as_ref()
    {
        handle.abort();
    }
    let _joined = observe(&actual).await;
    body
}
#[tokio::test]
async fn actual_shutdown_worker_loss_remains_unknown_on_repeated_observation() -> io::Result<()> {
    let actual = Operation::prepare();
    actual.start(
        tokio::runtime::Handle::current(),
        Book::default(),
        std::future::pending(),
    );
    if let Some(handle) = actual
        .lifecycle
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .pending
        .as_ref()
    {
        handle.abort();
    }
    let first = observe(&actual).await;
    let second = observe(&actual).await;
    if first.is_ok() || second.is_ok() {
        return Err(invalid("actual aborted shutdown was reclassified complete"));
    }
    Ok(())
}
