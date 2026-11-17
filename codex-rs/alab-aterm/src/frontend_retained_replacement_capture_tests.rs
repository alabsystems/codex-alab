// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual private task/slot scheduling controls only. These never construct a
//! Controller, FrontendCaptureWindow, Native peer, H1 receipt or Go grant.
use super::*;
use std::future::Future as _;
use std::task::Context as PollContext;
use std::task::Poll;
use std::task::Waker;

fn original_data() -> Arc<Data> {
    Arc::new(Data {
        settings: StartupSettings::new(&[OsString::from("original prompt")], &[])
            .expect("original DATA"),
        input: Arc::from(&b"whole original DATA"[..]),
    })
}

#[tokio::test]
async fn cancelled_private_data_wait_preserves_the_same_actual_producer_result() {
    let state = Arc::new(State::default());
    {
        let mut first = Box::pin(state.data());
        assert!(matches!(
            first
                .as_mut()
                .poll(&mut PollContext::from_waker(Waker::noop())),
            Poll::Pending
        ));
    } // Drop the actual future, not only its Pin<&mut> observer.
    let actual = original_data();
    let original = actual.clone();
    let worker_state = state.clone();
    let producer = tokio::spawn(async move {
        worker_state.progress.lock().expect("slot").data = Some(actual);
        worker_state.changed.notify_waiters();
    });
    producer.await.expect("same producer");
    let observed = state.data().await.expect("same actual DATA");
    assert!(Arc::ptr_eq(&original, &observed));
}

#[tokio::test]
async fn unpolled_capture_guard_loss_wakes_the_same_data_wait_with_uncertainty() {
    let state = Arc::new(State::default());
    let guard = CaptureGuard {
        state: state.clone(),
        complete: false,
    };
    let producer = tokio::spawn(async move {
        let _guard = guard;
        std::future::pending::<()>().await;
    });
    producer.abort();
    let _joined = producer.await;
    let observed = tokio::time::timeout(Duration::from_secs(1), state.data())
        .await
        .expect("guard wake");
    assert!(observed.is_err());
    assert!(matches!(
        state.progress.lock().expect("slot").result,
        Some(ReplacementObservation::Unresolved(_))
    ));
    assert!(state.progress.lock().expect("slot").data.is_none());
}

#[tokio::test]
async fn completed_uncertainty_before_first_poll_cannot_be_lost_as_a_notification() {
    let state = State::default();
    state.completed(ReplacementObservation::Unresolved(io::ErrorKind::TimedOut));
    let observed = tokio::time::timeout(Duration::from_secs(1), state.data())
        .await
        .expect("stored outcome");
    assert!(observed.is_err());
    assert!(!state.progress.lock().expect("slot").connected);
}
