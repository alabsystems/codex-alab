// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real bounded outgoing-queue tests, with only the exact-turn loader supplied by
//! fixtures. They do not activate ALab globally or claim an actual core cancellation.

use super::*;
use crate::outgoing_message::OutgoingMessage;
use codex_alab_core::capacity::CAPACITY_RETRY_NOTICE;
use codex_analytics::AnalyticsEventsClient;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::WarningNotification;
use std::pin::Pin;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Poll;
use std::time::Duration;
use tokio::sync::mpsc;

fn channel() -> (OutgoingMessageSender, mpsc::Receiver<OutgoingEnvelope>) {
    let (sender, receiver) = mpsc::channel(1);
    (
        OutgoingMessageSender::new(sender, AnalyticsEventsClient::disabled()),
        receiver,
    )
}

async fn fill(sender: &OutgoingMessageSender) {
    sender
        .sender
        .send(OutgoingEnvelope::Broadcast {
            message: timestamped_server_notification(ServerNotification::Warning(
                WarningNotification {
                    thread_id: None,
                    message: "fixture occupies the single queue permit".into(),
                },
            )),
        })
        .await
        .unwrap();
}

async fn assert_pending<F: Future>(mut future: Pin<&mut F>) {
    std::future::poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
}

fn no_message(receiver: &mut mpsc::Receiver<OutgoingEnvelope>) {
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
}

fn live() -> (
    Arc<LiveCapacityWait>,
    codex_alab_core::capacity::LiveCapacityWaitGuard,
) {
    let state = Arc::new(LiveCapacityWait::default());
    let guard = state.begin(tokio::time::Instant::now().into_std() + Duration::from_secs(600));
    (state, guard)
}

#[tokio::test]
async fn live_notice_uses_one_reserved_permit_and_only_the_rejoining_connection() {
    let (sender, mut receiver) = channel();
    let (state, _guard) = live();
    assert!(
        sender
            .alab_send_capacity_wait_checked(
                ConnectionId(47),
                "thread".into(),
                "turn".into(),
                || async { Some(state) },
            )
            .await
    );
    let OutgoingEnvelope::ToConnection {
        connection_id,
        message,
        write_complete_tx,
    } = receiver.recv().await.unwrap()
    else {
        panic!("must not broadcast")
    };
    assert_eq!(connection_id, ConnectionId(47));
    assert!(write_complete_tx.is_none());
    let OutgoingMessage::AppServerNotification(envelope) = message else {
        panic!("notification")
    };
    let ServerNotification::Error(error) = envelope.notification else {
        panic!("retry notice")
    };
    assert!(error.will_retry);
    assert_eq!(error.thread_id, "thread");
    assert_eq!(error.turn_id, "turn");
    assert_eq!(error.error.message, CAPACITY_RETRY_NOTICE);
    assert_eq!(
        error.error.codex_error_info,
        Some(CodexErrorInfo::ServerOverloaded)
    );
    no_message(&mut receiver);
}

#[tokio::test]
async fn dropping_the_wait_before_blocked_reserve_releases_emits_nothing() {
    let (sender, mut receiver) = channel();
    fill(&sender).await;
    let (state, guard) = live();
    let loaded = AtomicUsize::new(0);
    let pending = sender.alab_send_capacity_wait_checked(
        ConnectionId(47),
        "thread".into(),
        "turn".into(),
        || async {
            loaded.fetch_add(1, Ordering::SeqCst);
            Some(state)
        },
    );
    tokio::pin!(pending);
    assert_pending(pending.as_mut()).await;
    assert_eq!(
        loaded.load(Ordering::SeqCst),
        0,
        "reserve is actually blocked before lookup"
    );
    drop(guard);
    receiver.recv().await.unwrap();
    assert!(!pending.await);
    assert_eq!(loaded.load(Ordering::SeqCst), 1);
    no_message(&mut receiver);
}

#[tokio::test(start_paused = true)]
async fn expiry_during_blocked_reserve_is_checked_after_capacity_arrives() {
    let (sender, mut receiver) = channel();
    fill(&sender).await;
    let (state, _guard) = live();
    let pending = sender.alab_send_capacity_wait_checked(
        ConnectionId(47),
        "thread".into(),
        "turn".into(),
        || async { Some(state) },
    );
    tokio::pin!(pending);
    assert_pending(pending.as_mut()).await;
    tokio::time::advance(Duration::from_secs(600)).await;
    receiver.recv().await.unwrap();
    assert!(!pending.await);
    no_message(&mut receiver);
}

#[tokio::test]
async fn an_old_guard_cannot_erase_the_new_wait_while_enqueue_is_blocked() {
    let (sender, mut receiver) = channel();
    fill(&sender).await;
    let (state, old) = live();
    let pending = sender.alab_send_capacity_wait_checked(
        ConnectionId(47),
        "thread".into(),
        "turn".into(),
        || async { Some(Arc::clone(&state)) },
    );
    tokio::pin!(pending);
    assert_pending(pending.as_mut()).await;
    let new = state.begin_with_reason(
        tokio::time::Instant::now().into_std() + Duration::from_secs(1200),
        CapacityReason::PolicyCapacityUnavailable,
    );
    drop(old);
    receiver.recv().await.unwrap();
    assert!(pending.await);
    assert!(
        state
            .publish_if_current(|| tokio::time::Instant::now().into_std(), || ())
            .is_some()
    );
    let Some(OutgoingEnvelope::ToConnection { message, .. }) = receiver.recv().await else {
        panic!("the exact replacement wait must reach the rejoining connection");
    };
    let OutgoingMessage::AppServerNotification(envelope) = message else {
        panic!("retry notification");
    };
    let ServerNotification::Error(error) = envelope.notification else {
        panic!("capacity error");
    };
    assert_eq!(
        error.error.codex_error_info,
        Some(CodexErrorInfo::PolicyCapacityUnavailable)
    );
    assert_eq!(
        error.error.message,
        CapacityReason::PolicyCapacityUnavailable.notice()
    );
    no_message(&mut receiver);
    drop(new);
}

#[tokio::test]
async fn a_rejected_exact_turn_or_cancelled_task_loader_emits_nothing() {
    for reason in ["wrong turn", "cancelled task"] {
        let (sender, mut receiver) = channel();
        fill(&sender).await;
        let called = AtomicUsize::new(0);
        let pending = sender.alab_send_capacity_wait_checked(
            ConnectionId(47),
            "thread".into(),
            reason.into(),
            || async {
                called.fetch_add(1, Ordering::SeqCst);
                None
            },
        );
        tokio::pin!(pending);
        assert_pending(pending.as_mut()).await;
        assert_eq!(called.load(Ordering::SeqCst), 0);
        receiver.recv().await.unwrap();
        assert!(!pending.await);
        assert_eq!(called.load(Ordering::SeqCst), 1);
        no_message(&mut receiver);
    }
}

fn turn(id: &str, status: TurnStatus) -> Turn {
    Turn {
        id: id.into(),
        root_turn_id: None,
        items_view: TurnItemsView::Full,
        items: Vec::new(),
        status,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }
}

#[test]
fn metadata_only_resume_uses_only_exact_in_progress_listener_metadata() {
    let status = TurnStatus::InProgress;
    assert_eq!(
        reconcile_running_turn(Some(("live", &status)), std::iter::empty()),
        Some("live".into())
    );
    assert_eq!(reconcile_running_turn(None, std::iter::empty()), None);
    assert_eq!(
        reconcile_running_turn(Some(("", &status)), std::iter::empty()),
        None
    );
    for status in [
        TurnStatus::Completed,
        TurnStatus::Interrupted,
        TurnStatus::Failed,
    ] {
        assert_eq!(
            reconcile_running_turn(Some(("live", &status)), std::iter::empty()),
            None
        );
    }
}

#[test]
fn returned_history_must_agree_with_exact_listener_identity() {
    let status = TurnStatus::InProgress;
    let completed = turn("done", TurnStatus::Completed);
    let live = turn("live", TurnStatus::InProgress);
    assert_eq!(
        reconcile_running_turn(Some(("live", &status)), [&completed].into_iter()),
        Some("live".into())
    );
    assert_eq!(
        reconcile_running_turn(Some(("live", &status)), [&completed, &live].into_iter()),
        Some("live".into())
    );
    assert_eq!(
        reconcile_running_turn(Some(("live", &status)), [&live, &live].into_iter()),
        Some("live".into())
    );
    assert_eq!(reconcile_running_turn(None, [&live].into_iter()), None);
    assert_eq!(
        reconcile_running_turn(Some(("live", &TurnStatus::Completed)), [&live].into_iter()),
        None
    );
}

#[test]
fn contradictory_returned_running_or_terminal_id_is_ineligible() {
    let status = TurnStatus::InProgress;
    let live = turn("live", TurnStatus::InProgress);
    let other = turn("other", TurnStatus::InProgress);
    assert_eq!(
        reconcile_running_turn(Some(("live", &status)), [&other].into_iter()),
        None
    );
    assert_eq!(
        reconcile_running_turn(Some(("live", &status)), [&live, &other].into_iter()),
        None
    );
    for status_of_returned in [
        TurnStatus::Completed,
        TurnStatus::Interrupted,
        TurnStatus::Failed,
    ] {
        let terminal = turn("live", status_of_returned);
        assert_eq!(
            reconcile_running_turn(Some(("live", &status)), [&terminal].into_iter()),
            None
        );
        assert_eq!(
            reconcile_running_turn(Some(("live", &status)), [&live, &terminal].into_iter()),
            None
        );
    }
}
