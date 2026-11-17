// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual task/join custody only. No Controller, Native member, SDK admission,
//! Source transition, or successful Native reply is fabricated here.
use super::TaskLease;
use super::reap_ready_source_tasks;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::sync::oneshot;

struct Tasks {
    handles: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    joins: Semaphore,
}

#[tokio::test]
async fn a_registered_worker_completes_while_shutdown_owns_its_join_lease() -> io::Result<()> {
    let actual = Arc::new(Tasks {
        handles: Mutex::new(Vec::new()),
        joins: Semaphore::new(1),
    });
    // Shutdown obtains this SAME lease before it pops/awaits the registered
    // worker. Holding it across the real join reproduces the old wait cycle.
    let shutdown = actual.joins.acquire().await.map_err(io::Error::other)?;
    let (begin, beginning) = oneshot::channel();
    let (finished, completion) = oneshot::channel();
    let owner = actual.clone();
    let worker = tokio::spawn(async move {
        let _ = beginning.await;
        let result = reap_ready_source_tasks(&owner.handles, &owner.joins).await;
        let _ = finished.send(result);
    });
    actual
        .handles
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(worker);
    let task = actual
        .handles
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .pop();
    let mut owned = TaskLease {
        tasks: &actual.handles,
        task,
    };
    let _ = begin.send(());
    let outcome = tokio::time::timeout(Duration::from_secs(2), async {
        owned
            .task
            .as_mut()
            .ok_or_else(|| io::Error::other("actual worker missing"))?
            .await
            .map_err(io::Error::other)
    })
    .await;
    if outcome.is_err() {
        // Borrow the SAME handle for timeout, then abort and actually join it.
        // No failed control leaves a detached worker waiting on this lease.
        if let Some(task) = owned.task.as_mut() {
            task.abort();
            let _ = task.await;
        }
    }
    owned.task.take();
    drop(shutdown);
    outcome.map_err(io::Error::other)??;
    completion.await.map_err(io::Error::other)??;
    assert!(
        actual
            .handles
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn housekeeping_actually_joins_a_completed_task_without_consuming_live_work() -> io::Result<()>
{
    let actual = Arc::new(Tasks {
        handles: Mutex::new(Vec::new()),
        joins: Semaphore::new(1),
    });
    let (complete, completed) = oneshot::channel();
    let finished = tokio::spawn(async move {
        let _ = complete.send(());
    });
    completed.await.map_err(io::Error::other)?;
    // Observe the real scheduler's terminal state, not merely the channel send.
    while !finished.is_finished() {
        tokio::task::yield_now().await;
    }
    let (stop, stopping) = oneshot::channel::<()>();
    let active = tokio::spawn(async move {
        let _ = stopping.await;
    });
    actual
        .handles
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .extend([finished, active]);
    let outcome = reap_ready_source_tasks(&actual.handles, &actual.joins).await;
    let remaining = actual
        .handles
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .len();
    let _ = stop.send(());
    // Cleanup precedes propagating either the reaper result or the assertion.
    let tasks = std::mem::take(
        &mut *actual
            .handles
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
    );
    let mut cleanup_failed = false;
    for mut task in tasks {
        match tokio::time::timeout(Duration::from_secs(2), &mut task).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => cleanup_failed = true,
            Err(_) => {
                task.abort();
                let _ = task.await;
                cleanup_failed = true;
            }
        }
    }
    outcome?;
    assert!(
        !cleanup_failed,
        "original live worker did not finish cleanup"
    );
    assert_eq!(remaining, 1, "only the actual completed handle was retired");
    Ok(())
}
