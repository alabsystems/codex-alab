// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::future::poll_fn;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::oneshot;

async fn pending<F: Future>(mut work: Pin<&mut F>) {
    let result = poll_fn(|context| Poll::Ready(work.as_mut().poll(context))).await;
    assert!(result.is_pending(), "the barrier or work must remain live");
}

async fn settle<F: Future>(work: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(5), work)
        .await
        .expect("retained completion hang detector")
}

#[tokio::test]
async fn normal_completion_returns_its_output_and_disarms_abandonment() {
    let owner = Owner::default();
    let abandoned = Abandonments::default();
    let cancelled = Arc::new(AtomicUsize::new(0));
    let cancel = Arc::clone(&cancelled);
    let work = retain(
        async {
            tokio::task::yield_now().await;
            37
        },
        owner.register().unwrap(),
        abandoned.clone(),
        move || {
            cancel.fetch_add(1, Ordering::SeqCst);
        },
    );
    assert_eq!(settle(work).await, 37);
    settle(owner.close_and_wait()).await;
    settle(abandoned.wait()).await;
    assert_eq!(cancelled.load(Ordering::SeqCst), 0);
    assert_eq!(owner.register().unwrap_err(), Closed);
}

#[tokio::test]
async fn polling_a_completed_waiter_is_inert_and_cannot_reacquire_ownership() {
    let owner = Owner::default();
    let abandoned = Abandonments::default();
    let cancelled = Arc::new(AtomicUsize::new(0));
    let cancel = Arc::clone(&cancelled);
    let mut work = Box::pin(retain(
        async { 37 },
        owner.register().unwrap(),
        abandoned.clone(),
        move || {
            cancel.fetch_add(1, Ordering::SeqCst);
        },
    ));
    assert_eq!(settle(work.as_mut()).await, 37);
    pending(work.as_mut()).await;
    settle(owner.close_and_wait()).await;
    drop(work);
    settle(abandoned.wait()).await;
    assert_eq!(cancelled.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn never_polled_work_transfers_and_both_barriers_wait_for_its_completion() {
    let owner = Owner::default();
    let abandoned = Abandonments::default();
    let (entered, started) = oneshot::channel();
    let (finish, released) = oneshot::channel();
    let cancelled = Arc::new(AtomicUsize::new(0));
    let cancel = Arc::clone(&cancelled);
    let work = retain(
        async move {
            entered.send(()).unwrap();
            released.await.unwrap();
        },
        owner.register().unwrap(),
        abandoned.clone(),
        move || {
            cancel.fetch_add(1, Ordering::SeqCst);
        },
    );
    drop(work);
    assert_eq!(cancelled.load(Ordering::SeqCst), 1);
    settle(started).await.unwrap();
    owner.close();
    let mut owner_wait = Box::pin(owner.wait());
    let mut abandoned_wait = Box::pin(abandoned.wait());
    pending(owner_wait.as_mut()).await;
    pending(abandoned_wait.as_mut()).await;
    finish.send(()).unwrap();
    settle(owner_wait).await;
    settle(abandoned_wait).await;
}

#[tokio::test]
async fn a_partly_polled_future_resumes_its_exact_state_without_replaying_work() {
    let owner = Owner::default();
    let abandoned = Abandonments::default();
    let steps = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&steps);
    let (finish, released) = oneshot::channel();
    let mut work = Box::pin(retain(
        async move {
            assert_eq!(observed.fetch_add(1, Ordering::SeqCst), 0);
            released.await.unwrap();
            assert_eq!(observed.fetch_add(1, Ordering::SeqCst), 1);
        },
        owner.register().unwrap(),
        abandoned.clone(),
        || {},
    ));
    pending(work.as_mut()).await;
    assert_eq!(steps.load(Ordering::SeqCst), 1);
    drop(work);
    finish.send(()).unwrap();
    settle(abandoned.wait()).await;
    settle(owner.close_and_wait()).await;
    assert_eq!(steps.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn admission_is_closed_before_any_future_is_constructed_or_started() {
    let owner = Owner::default();
    let registration = owner.register().unwrap();
    let another = owner.clone();
    owner.close();
    assert_eq!(another.register().unwrap_err(), Closed);
    let mut waiting = Box::pin(owner.wait());
    pending(waiting.as_mut()).await;
    // Admission ownership exists even before a retained wrapper is installed.
    drop(registration);
    settle(waiting).await;
    assert_eq!(owner.register().unwrap_err(), Closed);
}

#[tokio::test]
async fn normal_pending_waiters_do_not_enter_the_turn_abandonment_barrier() {
    let owner = Owner::default();
    let abandoned = Abandonments::default();
    let (finish, released) = oneshot::channel();
    let mut work = Box::pin(retain(
        async move { released.await.unwrap() },
        owner.register().unwrap(),
        abandoned.clone(),
        || {},
    ));
    pending(work.as_mut()).await;
    settle(abandoned.wait()).await;
    let mut owner_wait = Box::pin(owner.wait());
    pending(owner_wait.as_mut()).await;
    finish.send(()).unwrap();
    settle(work).await;
    settle(owner_wait).await;
    settle(abandoned.wait()).await;
}

#[tokio::test]
async fn abandonment_uses_the_captured_runtime_from_an_unentered_thread() {
    let owner = Owner::default();
    let abandoned = Abandonments::default();
    let (finished, completed) = oneshot::channel();
    let work = retain(
        async move { finished.send(()).unwrap() },
        owner.register().unwrap(),
        abandoned.clone(),
        || {},
    );
    std::thread::spawn(move || drop(work)).join().unwrap();
    settle(completed).await.unwrap();
    settle(abandoned.wait()).await;
    settle(owner.close_and_wait()).await;
}

#[tokio::test]
async fn closing_a_thread_waits_for_normal_and_abandoned_work_together() {
    let owner = Owner::default();
    let abandoned = Abandonments::default();
    let (normal_finish, normal_released) = oneshot::channel();
    let (retained_finish, retained_released) = oneshot::channel();
    let normal = retain(
        async move { normal_released.await.unwrap() },
        owner.register().unwrap(),
        abandoned.clone(),
        || {},
    );
    drop(retain(
        async move { retained_released.await.unwrap() },
        owner.register().unwrap(),
        abandoned.clone(),
        || {},
    ));
    owner.close();
    let mut waiting = Box::pin(owner.wait());
    pending(waiting.as_mut()).await;
    normal_finish.send(()).unwrap();
    settle(normal).await;
    pending(waiting.as_mut()).await;
    retained_finish.send(()).unwrap();
    settle(waiting).await;
    settle(abandoned.wait()).await;
}

#[tokio::test]
async fn cancelling_a_close_wait_neither_reopens_admission_nor_loses_ownership() {
    let owner = Owner::default();
    let abandoned = Abandonments::default();
    let (finish, released) = oneshot::channel();
    let work = retain(
        async move { released.await.unwrap() },
        owner.register().unwrap(),
        abandoned.clone(),
        || {},
    );
    let mut waiting = Box::pin(owner.close_and_wait());
    pending(waiting.as_mut()).await;
    drop(waiting);
    assert_eq!(owner.register().unwrap_err(), Closed);
    drop(work);
    let mut waiting = Box::pin(owner.wait());
    pending(waiting.as_mut()).await;
    finish.send(()).unwrap();
    settle(waiting).await;
    settle(abandoned.wait()).await;
}

#[tokio::test]
async fn abandoned_completion_destroys_its_output_before_releasing_either_barrier() {
    struct Output {
        owner: Owner,
        abandoned: Abandonments,
        dropped: Arc<AtomicUsize>,
    }

    impl Drop for Output {
        fn drop(&mut self) {
            assert_eq!(Arc::strong_count(&self.owner.0.registrations), 2);
            assert_eq!(Arc::strong_count(&self.abandoned.0.registrations), 2);
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }

    let owner = Owner::default();
    let abandoned = Abandonments::default();
    let dropped = Arc::new(AtomicUsize::new(0));
    let output = Output {
        owner: owner.clone(),
        abandoned: abandoned.clone(),
        dropped: Arc::clone(&dropped),
    };
    drop(retain(
        async move { output },
        owner.register().unwrap(),
        abandoned.clone(),
        || {},
    ));
    settle(owner.close_and_wait()).await;
    settle(abandoned.wait()).await;
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}
