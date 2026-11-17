// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Forward reset decisions while recovery is pending and before it returns.

use std::future::Future;
use tokio::sync::mpsc::UnboundedReceiver;

pub(super) async fn forward<T, F, Emit, Emitted>(
    mut notices: UnboundedReceiver<String>,
    operation: F,
    mut emit: Emit,
) -> T
where
    F: Future<Output = T>,
    Emit: FnMut(String) -> Emitted,
    Emitted: Future<Output = ()>,
{
    tokio::pin!(operation);
    loop {
        tokio::select! {
            outcome = &mut operation => {
                // A ready operation can have queued its final result in the same poll.
                while let Ok(message) = notices.try_recv() {
                    emit(message).await;
                }
                return outcome;
            }
            Some(message) = notices.recv() => emit(message).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn final_decision_is_delivered_before_recovery_returns() {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let delivered = std::sync::Mutex::new(Vec::new());
        let outcome = forward(
            receiver,
            async {
                sender
                    .send("Reset used; allowance restored.".into())
                    .unwrap();
                true
            },
            |message| {
                let delivered = &delivered;
                async move {
                    delivered.lock().unwrap().push(message);
                }
            },
        )
        .await;
        assert!(outcome);
        assert_eq!(
            *delivered.lock().unwrap(),
            vec!["Reset used; allowance restored."]
        );
    }

    #[tokio::test]
    async fn decision_is_visible_while_recovery_waits() {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let (seen, visible) = tokio::sync::oneshot::channel();
        let mut seen = Some(seen);
        let pending = forward(
            receiver,
            async {
                sender
                    .send("1 reset available; considering redemption.".into())
                    .unwrap();
                visible.await.unwrap();
                42
            },
            |message| {
                let seen = seen.take();
                async move {
                    assert_eq!(message, "1 reset available; considering redemption.");
                    seen.unwrap().send(()).unwrap();
                }
            },
        );
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), pending)
                .await
                .expect("notice must not wait for recovery to finish"),
            42
        );
    }
}
