// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual task/observer lifetime controls. Producers return only errors; no
//! Controller, Native admission, SDK permission or positive F1 is fabricated.
use super::*;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[tokio::test]
async fn cancelled_open_observation_retains_the_same_partially_polled_producer() -> io::Result<()> {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let actual_entered = entered.clone();
    let actual_release = release.clone();
    let actual_calls = calls.clone();
    let mut opening = Opening::owned(async move {
        actual_calls.fetch_add(1, Ordering::SeqCst);
        actual_entered.notify_one();
        actual_release.notified().await;
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "closed control",
        ))
    })?;
    let observed = async {
        tokio::time::timeout(Duration::from_secs(5), entered.notified())
            .await
            .map_err(|_| unavailable())?;
        if tokio::time::timeout(Duration::from_millis(1), opening.observe())
            .await
            .is_ok()
        {
            return Err(unavailable());
        }
        if calls.load(Ordering::SeqCst) != 1 {
            return Err(unavailable());
        }
        Ok(())
    }
    .await;
    // Always release the actual producer, including a failed control assertion.
    release.notify_one();
    let completed = tokio::time::timeout(Duration::from_secs(5), opening.observe())
        .await
        .map_err(|_| unavailable())?;
    observed?;
    if completed.err().map(|error| error.kind()) != Some(io::ErrorKind::PermissionDenied) {
        return Err(unavailable());
    }
    if opening.observe().await.err().map(|error| error.kind())
        != Some(io::ErrorKind::PermissionDenied)
        || calls.load(Ordering::SeqCst) != 1
    {
        return Err(unavailable());
    }
    Ok(())
}

#[tokio::test]
async fn destruction_of_unpolled_open_worker_is_terminal_unknown_without_resubmit() -> io::Result<()>
{
    let calls = Arc::new(AtomicUsize::new(0));
    let actual_calls = calls.clone();
    let mut opening = Opening::owned(async move {
        actual_calls.fetch_add(1, Ordering::SeqCst);
        std::future::pending::<io::Result<Controller>>().await
    })?;
    // No yield before abort: the current-thread runtime has not polled it.
    opening.task.as_ref().ok_or_else(unavailable)?.abort();
    let completed = tokio::time::timeout(Duration::from_secs(5), opening.observe())
        .await
        .map_err(|_| unavailable())?;
    if completed.err().map(|error| error.kind()) != Some(io::ErrorKind::Other)
        || calls.load(Ordering::SeqCst) != 0
    {
        return Err(unavailable());
    }
    if opening.observe().await.err().map(|error| error.kind()) != Some(io::ErrorKind::Other)
        || calls.load(Ordering::SeqCst) != 0
    {
        return Err(unavailable());
    }
    Ok(())
}
