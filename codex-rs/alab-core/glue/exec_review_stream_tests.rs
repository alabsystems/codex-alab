// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use futures::poll;
use serde_json::json;
use std::time::Duration;
use tokio::sync::Notify;

fn event(value: serde_json::Value) -> Event {
    Event {
        id: "delegate-turn".into(),
        msg: serde_json::from_value(value).unwrap(),
    }
}

fn started() -> Event {
    event(json!({"type":"task_started", "turn_id":"delegate-turn", "model_context_window":null}))
}

fn message(text: &str) -> Event {
    event(json!({"type":"agent_message", "message":text}))
}

fn warning(text: &str) -> Event {
    event(json!({"type":"warning", "message":text}))
}

fn complete() -> Event {
    event(json!({"type":"task_complete", "turn_id":"delegate-turn",
        "last_agent_message":"{\"findings\":[],\"overall_correctness\":\"correct\",\"overall_explanation\":\"review done\",\"overall_confidence_score\":1.0}"}))
}

fn state() -> Arc<State> {
    let state = State::new(
        ThreadId::new(),
        "parent-turn".into(),
        CancellationToken::new(),
    );
    state.data.lock().unwrap().delegate = Some(ThreadId::new());
    state
}

async fn ready(
    state: &Arc<State>,
) -> (
    async_channel::Sender<Event>,
    async_channel::Receiver<EventMsg>,
    tokio::task::JoinHandle<Option<ReviewOutputEvent>>,
) {
    let (send, receive) = async_channel::bounded(16);
    let (delivered, output) = async_channel::unbounded();
    let owner = Arc::clone(state);
    let task = tokio::spawn(async move {
        let _guard = RunGuard(Arc::clone(&owner));
        owner
            .run(receive, move |event| {
                let delivered = delivered.clone();
                async move {
                    delivered.send(event).await.unwrap();
                }
            })
            .await
    });
    send.send(started()).await.unwrap();
    assert!(matches!(
        output.recv().await.unwrap(),
        EventMsg::TurnStarted(_)
    ));
    (send, output, task)
}

#[tokio::test]
async fn freeze_waits_for_in_flight_forwarding_and_preserves_the_next_message() {
    let state = state();
    let (send, receive) = async_channel::bounded(16);
    let (delivered, output) = async_channel::unbounded();
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let owner = Arc::clone(&state);
    let task_entered = Arc::clone(&entered);
    let task_release = Arc::clone(&release);
    let task = tokio::spawn(async move {
        let _guard = RunGuard(Arc::clone(&owner));
        owner
            .run(receive, move |event| {
                let delivered = delivered.clone();
                let entered = Arc::clone(&task_entered);
                let release = Arc::clone(&task_release);
                async move {
                    if matches!(event, EventMsg::AgentMessage(_)) {
                        entered.notify_one();
                        release.notified().await;
                    }
                    delivered.send(event).await.unwrap();
                }
            })
            .await
    });
    send.send(started()).await.unwrap();
    output.recv().await.unwrap();
    send.send(message("first")).await.unwrap();
    send.send(message("second")).await.unwrap();
    entered.notified().await;
    let mut pending = Box::pin(state.freeze());
    assert!(poll!(pending.as_mut()).is_pending());
    release.notify_one();
    let frozen = tokio::time::timeout(Duration::from_secs(5), pending)
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(output.recv().await.unwrap(), EventMsg::AgentMessage(ref message) if message.message == "first")
    );
    send.send(warning("queued after freeze")).await.unwrap();
    drop(send);
    let snapshot = frozen.drain_snapshot().await.unwrap();
    assert!(
        matches!(snapshot.buffered.as_ref().unwrap().msg, EventMsg::AgentMessage(ref message) if message.message == "second")
    );
    assert_eq!(snapshot.pending.len(), 1);
    let encoded = serde_json::to_vec(&snapshot).unwrap();
    let restored: ExecReviewSnapshot = serde_json::from_slice(&encoded).unwrap();
    restored.validate(state.parent, &state.turn).unwrap();
    drop(frozen);
    assert!(
        matches!(output.recv().await.unwrap(), EventMsg::Warning(ref warning) if warning.message == "queued after freeze")
    );
    assert!(task.await.unwrap().is_none());
}

#[tokio::test]
async fn frozen_consumer_drains_bounded_producers_without_public_delivery() {
    let state = state();
    let (send, output, task) = ready(&state).await;
    let frozen = state.freeze().await.unwrap();
    let producer = tokio::spawn(async move {
        for index in 0..64 {
            send.send(warning(&format!("queued {index}")))
                .await
                .unwrap();
        }
    });
    tokio::time::timeout(Duration::from_secs(5), producer)
        .await
        .expect("frozen review must not block delegate shutdown on a full event queue")
        .unwrap();
    assert!(output.is_empty());
    let snapshot = frozen.drain_snapshot().await.unwrap();
    assert_eq!(snapshot.pending.len(), 64);
    for (index, event) in snapshot.pending.iter().enumerate() {
        assert!(
            matches!(&event.msg, EventMsg::Warning(value) if value.message == format!("queued {index}"))
        );
    }
    drop(frozen);
    for _ in 0..64 {
        output.recv().await.unwrap();
    }
    assert!(task.await.unwrap().is_none());
}

#[tokio::test]
async fn cancelling_the_freeze_waiter_releases_the_consumer() {
    let state = state();
    let (send, output, task) = ready(&state).await;
    let mut pending = Box::pin(state.freeze());
    assert!(poll!(pending.as_mut()).is_pending());
    drop(pending);
    assert!(*state.control.borrow() == Control::Running);
    send.send(complete()).await.unwrap();
    drop(send);
    assert_eq!(
        task.await.unwrap().unwrap().overall_explanation,
        "review done"
    );
    assert!(output.is_empty());
}

#[tokio::test]
async fn interrupted_snapshot_keeps_each_dequeued_event_for_the_next_waiter() {
    let state = state();
    let (send, output, task) = ready(&state).await;
    let mut frozen = state.freeze().await.unwrap();
    frozen.commit().unwrap();
    // Isolate cancellation of the snapshot reader from the live frozen drainer.
    state.cancel.cancel();
    assert!(task.await.unwrap().is_none());
    send.send(warning("one")).await.unwrap();
    let mut pending = Box::pin(frozen.drain_snapshot());
    assert!(poll!(pending.as_mut()).is_pending());
    drop(pending);
    assert_eq!(state.data.lock().unwrap().pending.len(), 1);
    send.send(warning("two")).await.unwrap();
    send.send(event(json!({"type":"shutdown_complete"})))
        .await
        .unwrap();
    drop(send);
    let snapshot = frozen.drain_snapshot().await.unwrap();
    assert_eq!(snapshot.pending.len(), 2);
    assert!(output.is_empty());
    drop(frozen);
}

#[tokio::test]
async fn concurrent_snapshot_waiters_receive_the_same_complete_queue() {
    let state = state();
    let (send, output, task) = ready(&state).await;
    let frozen = state.freeze().await.unwrap();
    send.send(warning("one")).await.unwrap();
    let mut first = Box::pin(frozen.drain_snapshot());
    assert!(poll!(first.as_mut()).is_pending());
    let mut second = Box::pin(frozen.drain_snapshot());
    assert!(poll!(second.as_mut()).is_pending());
    send.send(warning("two")).await.unwrap();
    drop(send);
    let (first, second) = futures::join!(first, second);
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(first.pending.len(), 2);
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(second).unwrap()
    );
    drop(frozen);
    output.recv().await.unwrap();
    output.recv().await.unwrap();
    assert!(task.await.unwrap().is_none());
}

#[tokio::test]
async fn cancelling_an_uncommitted_freeze_finishes_the_consumer() {
    let state = state();
    let (_send, _output, task) = ready(&state).await;
    let (bridge, watcher) = state.bridge_cancel();
    let mut frozen = state.freeze().await.unwrap();
    state.cancel.cancel();
    assert!(task.await.unwrap().is_none());
    watcher.await.unwrap();
    assert!(bridge.is_cancelled());
    assert!(frozen.commit().is_err());
    assert!(matches!(*state.control.borrow(), Control::Finished));
    drop(frozen);
    assert!(matches!(*state.control.borrow(), Control::Finished));
}

#[tokio::test]
async fn committed_transfer_drains_the_live_bridge_after_parent_cancellation() {
    let state = state();
    let (bridge, watcher) = state.bridge_cancel();
    let (send, receive) = async_channel::bounded(1);
    let (ops, requests) = async_channel::bounded(1);
    let (_status, agent_status) =
        watch::channel(codex_protocol::protocol::AgentStatus::PendingInit);
    let io = crate::codex_delegate::forward_session_io(
        Arc::new(SessionIo {
            tx_sub: ops,
            rx_event: receive,
            agent_status,
            session_loop_termination: crate::session::completed_session_loop_termination(),
        }),
        bridge.clone(),
    );
    let receiver = io.rx_event.clone();
    drop(io);
    let (delivered, output) = async_channel::unbounded();
    let owner = Arc::clone(&state);
    let task = tokio::spawn(async move {
        let _guard = RunGuard(Arc::clone(&owner));
        owner
            .run(receiver, move |event| {
                let delivered = delivered.clone();
                async move {
                    delivered.send(event).await.unwrap();
                }
            })
            .await
    });
    send.send(started()).await.unwrap();
    assert!(matches!(
        output.recv().await.unwrap(),
        EventMsg::TurnStarted(_)
    ));
    let mut frozen = state.freeze().await.unwrap();
    frozen.commit().unwrap();
    state.cancel.cancel();
    assert!(task.await.unwrap().is_none());
    watcher.await.unwrap();
    assert!(!bridge.is_cancelled());
    assert!(output.is_empty());
    let producer = tokio::spawn(async move {
        for index in 0..3 {
            send.send(warning(&format!("after cancellation {index}")))
                .await
                .unwrap();
        }
        send.send(complete()).await.unwrap();
        send.send(event(json!({"type":"shutdown_complete"})))
            .await
            .unwrap();
    });
    let snapshot = tokio::time::timeout(Duration::from_secs(5), frozen.drain_snapshot())
        .await
        .unwrap()
        .unwrap();
    producer.await.unwrap();
    assert_eq!(snapshot.pending.len(), 4);
    for (index, event) in snapshot.pending.iter().take(3).enumerate() {
        assert!(matches!(&event.msg, EventMsg::Warning(value)
            if value.message == format!("after cancellation {index}")));
    }
    assert!(matches!(snapshot.pending[3].msg, EventMsg::TurnComplete(_)));
    assert!(
        requests.try_recv().is_err(),
        "handoff must not interrupt the delegate bridge"
    );
}

#[tokio::test]
async fn completed_consumer_releases_its_bridge_watcher() {
    let state = state();
    let weak = Arc::downgrade(&state);
    let (bridge, watcher) = state.bridge_cancel();
    state.complete();
    tokio::time::timeout(Duration::from_secs(5), watcher)
        .await
        .unwrap()
        .unwrap();
    assert!(bridge.is_cancelled());
    drop(state);
    assert!(weak.upgrade().is_none());
}

#[tokio::test]
async fn dropped_consumer_cancels_its_bridge_without_waiting_for_the_parent() {
    let state = state();
    let parent = state.cancel.clone();
    let (bridge, watcher) = state.bridge_cancel();
    drop(state);
    tokio::time::timeout(Duration::from_secs(5), watcher)
        .await
        .unwrap()
        .unwrap();
    assert!(!parent.is_cancelled());
    assert!(bridge.is_cancelled());
}

#[tokio::test]
async fn suspended_consumer_does_not_finish_or_consume_queued_output() {
    let state = state();
    let (send, output, task) = ready(&state).await;
    let mut frozen = state.freeze().await.unwrap();
    send.send(warning("retained")).await.unwrap();
    frozen.commit().unwrap();
    state.cancel.cancel();
    assert!(task.await.unwrap().is_none());
    assert!(output.is_empty());
    drop(send);
    assert_eq!(frozen.drain_snapshot().await.unwrap().pending.len(), 1);
    drop(frozen);
    assert!(matches!(*state.control.borrow(), Control::Frozen(_)));
}

#[tokio::test]
async fn completed_review_refuses_a_late_freeze() {
    let state = state();
    let (send, _, task) = ready(&state).await;
    send.send(complete()).await.unwrap();
    assert!(task.await.unwrap().is_some());
    assert!(state.freeze().await.is_err());
}

#[tokio::test]
async fn consumer_preserves_review_message_suppression() {
    let state = state();
    let (send, output, task) = ready(&state).await;
    send.send(message("intermediate")).await.unwrap();
    let delegate = state.data.lock().unwrap().delegate.unwrap();
    send.send(event(json!({"type":"item_completed", "thread_id":delegate,
        "turn_id":"delegate-turn", "completed_at_ms":1,
        "item":{"type":"AgentMessage", "id":"raw-message", "content":[{"type":"Text", "text":"raw final"}]}})))
        .await.unwrap();
    send.send(message("raw final")).await.unwrap();
    send.send(complete()).await.unwrap();
    drop(send);
    assert_eq!(
        task.await.unwrap().unwrap().overall_explanation,
        "review done"
    );
    assert!(
        matches!(output.recv().await.unwrap(), EventMsg::AgentMessage(ref message) if message.message == "intermediate")
    );
    assert!(output.is_empty());
}

#[tokio::test]
async fn another_delegate_turn_cannot_complete_the_review() {
    let state = state();
    let (send, _, task) = ready(&state).await;
    let mut wrong = complete();
    let EventMsg::TurnComplete(completion) = &mut wrong.msg else {
        unreachable!();
    };
    completion.turn_id = "another-turn".into();
    send.send(wrong).await.unwrap();
    assert!(task.await.unwrap().is_none());
}

#[tokio::test]
async fn snapshot_rejects_changed_identity_and_aborted_delegate() {
    let state = state();
    let (send, _, task) = ready(&state).await;
    let frozen = state.freeze().await.unwrap();
    drop(send);
    let mut snapshot = frozen.drain_snapshot().await.unwrap();
    assert!(snapshot.validate(ThreadId::new(), &state.turn).is_err());
    assert!(snapshot.validate(state.parent, "new-turn").is_err());
    snapshot.pending.push(event(
        json!({"type":"turn_aborted","reason":"interrupted","turn_id":"delegate-turn"}),
    ));
    assert!(snapshot.validate(state.parent, &state.turn).is_err());
    drop(frozen);
    assert!(task.await.unwrap().is_none());
}
