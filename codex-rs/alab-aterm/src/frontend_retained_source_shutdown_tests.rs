// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual shared Source gate/task-ledger controls. No Controller, Native member,
//! SourceShutdown, origin, ACK, Go, UI or public SDK receipt is fabricated.
use super::*;
use std::future::Future;
use std::future::poll_fn;
use std::task::Poll;

fn ledger() -> State {
    State {
        next: 0,
        initial: None,
        initial_transferred: false,
        replacements: BTreeMap::new(),
        origins: BTreeMap::new(),
        runtimes: BTreeMap::new(),
        source_selections: BTreeMap::new(),
    }
}
fn error(message: &str) -> io::Error {
    io::Error::other(message.to_owned())
}

#[tokio::test]
async fn close_releases_original_ledger_locks_before_draining_a_real_automatic_window()
-> io::Result<()> {
    let (owner, gate, hold) = Owner::new_held();
    owner
        .release_if(&hold, || Ok(()))
        .await
        .map_err(io::Error::other)?;
    let active = gate
        .admit()
        .await
        .ok_or_else(|| error("real automatic window unavailable"))?;
    let tasks = Mutex::new(Vec::new());
    let state = Mutex::new(ledger());
    let closing = AtomicBool::new(false);
    let failed = AtomicBool::new(false);
    let (changed, _) = watch::channel(0u64);
    let barrier = close_source_records(&owner, &closing, &tasks, &state, &failed, &changed);
    let locks_released = tasks.try_lock().is_ok() && state.try_lock().is_ok();
    let mut draining = Box::pin(barrier.drain());
    let waited = poll_fn(|context| Poll::Ready(draining.as_mut().poll(context).is_pending())).await;
    let denied = gate.admit().await.is_none();
    drop(active);
    tokio::time::timeout(std::time::Duration::from_secs(60), draining)
        .await
        .map_err(|_| error("original automatic window did not drain"))?;
    // The SAME writer is free again for exact existing Source cleanup. No
    // scheduling admission can reopen through this bookkeeping or Drop.
    let ticket = owner.inhibit().await.map_err(io::Error::other)?;
    owner
        .release_inhibitor_if(&ticket, || Ok(()))
        .await
        .map_err(io::Error::other)?;
    assert!(locks_released && waited && denied && closing.load(Ordering::Acquire));
    assert!(!failed.load(Ordering::Acquire) && gate.admit().await.is_none());
    Ok(())
}

#[tokio::test]
async fn original_task_needing_the_same_writer_finishes_after_the_shutdown_barrier()
-> io::Result<()> {
    let (owner, gate, hold) = Owner::new_held();
    let owner = Arc::new(owner);
    owner
        .release_if(&hold, || Ok(()))
        .await
        .map_err(io::Error::other)?;
    let active = gate
        .admit()
        .await
        .ok_or_else(|| error("real automatic window unavailable"))?;
    let (start, accepted) = tokio::sync::oneshot::channel();
    let (completed, completion) = tokio::sync::oneshot::channel();
    let worker_owner = owner.clone();
    let worker = tokio::spawn(async move {
        let result = async {
            accepted
                .await
                .map_err(|_| error("original cleanup signal lost"))?;
            let ticket = worker_owner.inhibit().await.map_err(io::Error::other)?;
            worker_owner
                .release_inhibitor_if(&ticket, || Ok(()))
                .await
                .map_err(io::Error::other)
        }
        .await;
        let _observed = completed.send(result);
    });
    let tasks = Mutex::new(vec![worker]);
    let state = Mutex::new(ledger());
    let closing = AtomicBool::new(false);
    let failed = AtomicBool::new(false);
    let (changed, _) = watch::channel(0u64);
    let barrier = close_source_records(&owner, &closing, &tasks, &state, &failed, &changed);
    let mut draining = Box::pin(barrier.drain());
    let waited = poll_fn(|context| Poll::Ready(draining.as_mut().poll(context).is_pending())).await;
    let sent = start.send(()).is_ok();
    drop(active);
    let drained = tokio::time::timeout(std::time::Duration::from_secs(60), draining).await;
    // Borrow the original production TaskLease, including abort+join on any
    // timeout. Dropping a join handle is never called joined cleanup.
    let task = tasks
        .lock()
        .map_err(|_| error("task queue poisoned"))?
        .pop();
    let mut task = TaskLease {
        tasks: &tasks,
        task,
    };
    let handle = task
        .task
        .as_mut()
        .ok_or_else(|| error("original task handle lost"))?;
    let joined = tokio::time::timeout(std::time::Duration::from_secs(60), &mut *handle).await;
    let joined = match joined {
        Ok(joined) => joined.map_err(|_| error("original task panicked")),
        Err(_) => {
            handle.abort();
            let _joined = handle.await;
            Err(error(
                "original task was blocked by a retained shutdown writer",
            ))
        }
    };
    task.task.take();
    joined?;
    completion
        .await
        .map_err(|_| error("original cleanup result lost"))??;
    drained.map_err(|_| error("shutdown barrier did not drain"))?;
    assert!(waited && sent && closing.load(Ordering::Acquire));
    assert!(
        tasks
            .lock()
            .map_err(|_| error("task queue poisoned"))?
            .is_empty()
    );
    assert!(!failed.load(Ordering::Acquire) && gate.admit().await.is_none());
    Ok(())
}

#[tokio::test]
async fn no_h1_source_observer_stops_on_the_same_close_without_native_withdrawal() -> io::Result<()>
{
    let (owner, gate, hold) = Owner::new_held();
    let hold = Arc::new(hold);
    let tasks = Mutex::new(Vec::new());
    let mut original = ledger();
    original.initial = Some(hold.clone());
    let state = Mutex::new(original);
    let closing = AtomicBool::new(false);
    let failed = AtomicBool::new(false);
    let (changed, mut observed) = watch::channel(0u64);
    // This is the EXACT production observer branch, with no H1 record/phase
    // manufactured. It really waits before the real close helper signals it.
    let mut waiting = Box::pin(wait_source_change(&closing, &mut observed));
    let pending = poll_fn(|context| Poll::Ready(waiting.as_mut().poll(context).is_pending())).await;
    let barrier = close_source_records(&owner, &closing, &tasks, &state, &failed, &changed);
    let stopped = tokio::time::timeout(std::time::Duration::from_secs(60), waiting)
        .await
        .map_err(|_| error("no-H1 Source observer kept waiting until final Native withdrawal"))?;
    barrier.drain().await;
    // Original actual bookkeeping remains usable, proving this observer stop
    // did NOT call Owner.withdraw or release/recreate initial closed custody.
    let ticket = owner.inhibit().await.map_err(io::Error::other)?;
    owner
        .release_inhibitor_if(&ticket, || Ok(()))
        .await
        .map_err(io::Error::other)?;
    let kept = {
        let original = state
            .lock()
            .map_err(|_| error("original ledger poisoned"))?;
        original
            .initial
            .as_ref()
            .is_some_and(|actual| Arc::ptr_eq(actual, &hold))
            && !original.initial_transferred
            && original.replacements.is_empty()
    };
    assert!(pending && stopped.is_err() && kept && !failed.load(Ordering::Acquire));
    assert!(gate.admit().await.is_none());
    Ok(())
}
