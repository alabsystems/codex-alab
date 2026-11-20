// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::wait_until;
use std::cell::Cell;
use std::pin::pin;
use std::task::Context;
use std::task::Waker;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::Semaphore;

#[tokio::test(start_paused = true)]
async fn native_wait_allows_a_queued_permit_after_the_runtime_clock_advances() {
    tokio::time::advance(Duration::from_secs(121)).await;
    let permits = Semaphore::new(0);
    let deadline = Instant::now() + Duration::from_secs(30);
    let (acquired, ()) = tokio::join!(wait_until(deadline, permits.acquire()), async {
        tokio::time::sleep(Duration::from_secs(1)).await;
        permits.add_permits(1);
    });
    let permit = acquired.expect("remaining physical budget").unwrap();
    assert_eq!(permits.available_permits(), 0);
    drop(permit);
    assert_eq!(permits.available_permits(), 1);
}

#[tokio::test(start_paused = true)]
async fn native_wait_counts_queued_time_and_releases_the_permit_on_expiry() {
    let permits = Semaphore::new(0);
    let phase_started = Instant::now() - Duration::from_secs(90);
    let deadline = phase_started + Duration::from_secs(120);
    let mut queued = pin!(wait_until(deadline, permits.acquire()));
    assert!(
        queued
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    // The operation and timer are both ready when the waiter next gets a turn.
    tokio::time::advance(Duration::from_secs(31)).await;
    permits.add_permits(1);
    assert!(queued.await.is_none());
    assert_eq!(permits.available_permits(), 1);
}

#[tokio::test]
async fn native_wait_never_polls_an_operation_with_an_expired_physical_deadline() {
    let polled = Cell::new(false);
    let expired = Instant::now();
    assert!(
        wait_until(expired, async { polled.set(true) })
            .await
            .is_none()
    );
    assert!(!polled.get());
}
