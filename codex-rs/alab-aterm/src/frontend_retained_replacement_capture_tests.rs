// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual private task/slot scheduling controls only. These never construct a
//! Controller, FrontendCaptureWindow, Native peer, H1 receipt or Go grant.
use super::*;
use futures::poll;

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
        assert!(poll!(&mut first).is_pending());
    } // Drop the actual future, not only its Pin<&mut> observer.
    let actual = original_data();
    let original = actual.clone();
    let worker_state = state.clone();
    let producer = tokio::spawn(async move {
        worker_state
            .settings_connected()
            .expect("private slot ready");
        worker_state
            .claim_window()
            .await
            .expect("one lease")
            .provide(actual)
            .expect("private data");
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
    assert!(matches!(
        state.progress.lock().expect("slot").window,
        WindowSlot::Waiting
    ));
}

#[tokio::test]
async fn completed_uncertainty_before_first_poll_cannot_be_lost_as_a_notification() {
    let state = State::default();
    state.completed(ReplacementObservation::Unresolved(io::ErrorKind::TimedOut));
    let observed = tokio::time::timeout(Duration::from_secs(1), state.data())
        .await
        .expect("stored outcome");
    assert!(observed.is_err());
    assert!(matches!(
        state.progress.lock().expect("slot").window,
        WindowSlot::Waiting
    ));
}

async fn window_error(state: &Arc<State>) -> io::ErrorKind {
    state
        .claim_window()
        .await
        .err()
        .expect("window must be unavailable")
        .kind()
}

#[tokio::test]
async fn pending_and_held_window_cancellation_reuses_one_unprovided_slot() {
    let state = Arc::new(State::default());
    {
        let mut cancelled = Box::pin(state.claim_window());
        assert!(poll!(&mut cancelled).is_pending());
    }
    let mut first = Box::pin(state.claim_window());
    let mut second = Box::pin(state.claim_window());
    assert!(poll!(&mut first).is_pending());
    assert!(poll!(&mut second).is_pending());
    state.settings_connected().expect("private slot ready");
    let lease = first.await.expect("one window");
    assert_eq!(
        second.await.err().expect("second holder").kind(),
        io::ErrorKind::WouldBlock
    );
    drop(lease);

    let lease = state.claim_window().await.expect("reacquire the same slot");
    let actual = original_data();
    lease.provide(actual.clone()).expect("first submission");
    assert_eq!(window_error(&state).await, io::ErrorKind::AlreadyExists);
    assert!(Arc::ptr_eq(&state.data().await.expect("original"), &actual));
    state.completed(ReplacementObservation::Granted);
    assert_eq!(window_error(&state).await, io::ErrorKind::AlreadyExists);
}

#[tokio::test]
async fn terminal_failure_closes_a_waiting_or_outstanding_window() {
    for held in [false, true] {
        let state = Arc::new(State::default());
        if held {
            state.settings_connected().expect("private slot ready");
            let lease = state.claim_window().await.expect("one lease");
            state.completed(ReplacementObservation::Unresolved(io::ErrorKind::TimedOut));
            assert!(lease.provide(original_data()).is_err());
        } else {
            let mut waiting = Box::pin(state.claim_window());
            assert!(poll!(&mut waiting).is_pending());
            state.completed(ReplacementObservation::Unresolved(io::ErrorKind::TimedOut));
            assert_eq!(
                waiting.await.err().expect("terminal wake").kind(),
                io::ErrorKind::TimedOut
            );
        }
        assert_eq!(window_error(&state).await, io::ErrorKind::TimedOut);
        assert!(state.data().await.is_err());
    }
}
