// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Capture-aware request lifetime, before the TUI/runtime can be torn down.
use codex_alab_core::session::Shared;
use codex_alab_core::session::UpgradeRequest;
use codex_alab_core::upgrade_completion::UpgradeCompletion;
use std::time::Duration;

/// Queue a final synchronous source decision using the actual TUI owner. A
/// deadline withdraws only an undispatched request. Once committed, capture must
/// settle even if it outlives that deadline; returning refusal would let the
/// caller restore suspended work into a permanently fenced source.
///
/// The completion owns source capture and installation of its admitted execution
/// plan. Errors after commitment are fatal, never permission to resume the source.
pub async fn request<T: std::any::Any + Send>(
    shared: &Shared,
    deadline: Duration,
    commit: impl FnOnce(T) -> Option<UpgradeCompletion> + Send + 'static,
) -> Result<bool, String> {
    let (answer, mut accepted) = tokio::sync::oneshot::channel();
    let (request, cancellation) = UpgradeRequest::with_context(move |context: T| {
        let completion = commit(context);
        let _ = answer.send(completion.clone());
        completion
    });
    if !shared.queue_upgrade(request) {
        return Ok(false);
    }
    let response = match tokio::time::timeout(deadline, &mut accepted).await {
        Ok(response) => response,
        Err(_) => {
            // Cancellation synchronizes with dispatch's decision ownership. A
            // dispatched callback still owns the sender and must report its
            // result; an undispatched callback drops it without source effects.
            drop(cancellation);
            return match accepted.await {
                Ok(Some(completion)) => completion.wait().await.map(|()| true),
                _ => Ok(false),
            };
        }
    };
    drop(cancellation);
    match response {
        Ok(Some(completion)) => completion.wait().await.map(|()| true),
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_alab_core::session::UpgradeDispatch;

    #[tokio::test(start_paused = true)]
    async fn committed_capture_outlives_ack_deadline() {
        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        let (queue, mut events) = tokio::sync::mpsc::unbounded_channel();
        shared.set_upgrade_leave(Box::new(move |request| {
            let _ = queue.send(request);
        }));
        let (release, released) = tokio::sync::oneshot::channel();
        let waiter = tokio::spawn(request(shared, Duration::from_secs(1), move |_: u64| {
            Some(UpgradeCompletion::new(async move {
                released.await.map_err(|error| error.to_string())
            }))
        }));
        let event = events.recv().await.unwrap();
        let UpgradeDispatch::Capturing(completion) = event.decide_with_context(true, 7u64) else {
            panic!("source did not commit");
        };
        tokio::time::advance(Duration::from_secs(2)).await;
        assert!(!waiter.is_finished());
        release.send(()).unwrap();
        assert_eq!(waiter.await.unwrap(), Ok(true));
        completion.wait().await.unwrap();
    }

    #[tokio::test]
    async fn committed_failure_is_an_error_and_caller_cancellation_keeps_capture_alive() {
        for cancel in [false, true] {
            let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
            let (queue, mut events) = tokio::sync::mpsc::unbounded_channel();
            shared.set_upgrade_leave(Box::new(move |request| {
                let _ = queue.send(request);
            }));
            let (release, released) = tokio::sync::oneshot::channel();
            let waiter = tokio::spawn(request(shared, Duration::from_secs(1), move |_: u64| {
                Some(UpgradeCompletion::new(async move {
                    released.await.map_err(|error| error.to_string())?;
                    Err("source capture failed after commitment".to_owned())
                }))
            }));
            let event = events.recv().await.unwrap();
            let UpgradeDispatch::Capturing(completion) = event.decide_with_context(true, 7u64)
            else {
                panic!("source did not commit");
            };
            if cancel {
                waiter.abort();
                assert!(waiter.await.unwrap_err().is_cancelled());
                release.send(()).unwrap();
            } else {
                release.send(()).unwrap();
                assert_eq!(
                    waiter.await.unwrap(),
                    Err("source capture failed after commitment".to_owned())
                );
            }
            assert_eq!(
                completion.wait().await,
                Err("source capture failed after commitment".to_owned())
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn expired_request_cannot_commit_from_a_late_tui_event() {
        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        let (queue, mut events) = tokio::sync::mpsc::unbounded_channel();
        shared.set_upgrade_leave(Box::new(move |request| {
            let _ = queue.send(request);
        }));
        let waiter = tokio::spawn(request(shared, Duration::from_secs(1), |_: u64| {
            panic!("expired request committed");
        }));
        let event = events.recv().await.unwrap();
        tokio::time::advance(Duration::from_secs(2)).await;
        assert_eq!(waiter.await.unwrap(), Ok(false));
        assert!(matches!(
            event.decide_with_context(true, 7u64),
            UpgradeDispatch::Refused
        ));
    }
}
