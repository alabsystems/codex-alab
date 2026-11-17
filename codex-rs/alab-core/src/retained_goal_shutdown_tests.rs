// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real Source gate windows only; these resources confer no Native permission.
use super::*;
use std::future::Future;
use std::future::poll_fn;
use std::task::Poll;

#[tokio::test]
async fn shutdown_closes_new_admission_before_draining_the_original_automatic_permit()
-> Result<(), String> {
    let (owner, gate, hold) = Owner::new_held();
    owner.release_if(&hold, || Ok(())).await?;
    let original =
        AutomaticPermit::retained_only(gate.admit().await.ok_or("original admission denied")?);
    let shutdown = owner.begin_shutdown();
    assert!(gate.admit().await.is_none());
    let mut drained = Box::pin(shutdown.drain());
    assert!(
        poll_fn(|cx| Poll::Ready(drained.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(gate.admit().await.is_none());
    drop(original);
    drained.await;
    assert!(gate.admit().await.is_none());
    drop(shutdown);
    assert!(gate.admit().await.is_none());
    Ok(())
}

#[tokio::test]
async fn shutdown_keeps_exact_hold_and_source_retirement_usable_without_reopening()
-> Result<(), String> {
    let (owner, gate, hold) = Owner::new_held();
    let inhibitor = owner.inhibit().await?;
    let shutdown = owner.begin_shutdown();
    shutdown.drain().await;
    let mut checked = false;
    owner
        .release_selection_if(&inhibitor, Some(&hold), || {
            checked = true;
            Ok(())
        })
        .await?;
    assert!(checked && !inhibitor.is_held());
    assert!(gate.admit().await.is_none());
    assert!(
        owner
            .release_selection_if(&inhibitor, Some(&hold), || Err(
                "stale resource entered callback".into()
            ))
            .await
            .is_err()
    );
    assert!(gate.admit().await.is_none());
    Ok(())
}

#[tokio::test]
async fn canceled_shutdown_drain_observation_keeps_same_monotonic_closure_and_cleanup_bookkeeping()
-> Result<(), String> {
    let (owner, gate, hold) = Owner::new_held();
    owner.release_if(&hold, || Ok(())).await?;
    let original = gate.admit().await.ok_or("original admission denied")?;
    let shutdown = owner.begin_shutdown();
    let same = shutdown.clone();
    let mut observation = Box::pin(shutdown.drain());
    assert!(
        poll_fn(|cx| Poll::Ready(observation.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    drop(observation);
    assert!(gate.admit().await.is_none());
    drop(original);
    same.drain().await;
    // Existing cleanup resources may still be reconciled after closure. This
    // does not manufacture SDK admission; new actual SDK entry is denied there.
    let exact = owner.inhibit().await?;
    owner.release_inhibitor_if(&exact, || Ok(())).await?;
    assert!(gate.admit().await.is_none());
    owner.withdraw();
    assert!(owner.inhibit().await.is_err());
    Ok(())
}

#[tokio::test]
async fn shutdown_writer_drops_before_cleanup_already_queued_on_the_same_gate() -> Result<(), String>
{
    let (owner, gate, hold) = Owner::new_held();
    owner.release_if(&hold, || Ok(())).await?;
    let original = gate.admit().await.ok_or("original admission denied")?;
    let shutdown = owner.begin_shutdown();
    let mut drained = Box::pin(shutdown.drain());
    assert!(
        poll_fn(|cx| Poll::Ready(drained.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    // This genuine writer-needing cleanup is queued while the original reader
    // still owns the gate and BEFORE the shutdown drain obtains its writer.
    let mut cleanup = Box::pin(owner.inhibit());
    assert!(
        poll_fn(|cx| Poll::Ready(cleanup.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(gate.admit().await.is_none());
    drop(original);
    drained.await;
    let exact = cleanup.await?;
    assert!(exact.is_held());
    owner.release_inhibitor_if(&exact, || Ok(())).await?;
    assert!(!exact.is_held());
    assert!(gate.admit().await.is_none());
    Ok(())
}
