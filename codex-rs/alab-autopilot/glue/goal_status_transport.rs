// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Bounded, passive latest-state transport. Native enqueue acknowledgement owns
//! selected bytes until sent, cancelled or permanently closed. No goal authority.

use crate::outgoing_message::OutgoingMessageSender;
use codex_protocol::protocol::AlabGoalRuntimeDelivery;
use codex_protocol::protocol::AlabGoalRuntimeSnapshot;
use codex_protocol::protocol::AlabGoalRuntimeUpdatedNotification;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use tokio::sync::watch;

const MAX_PENDING: usize = crate::transport::CHANNEL_CAPACITY;
pub(crate) struct Transport(Arc<Inner>);
enum Work {
    Reset(AlabGoalRuntimeSnapshot),
    Selected(Selected),
}
struct Inner {
    identity: String,
    queue: Mutex<Queue>,
    changed: watch::Sender<u64>,
}
#[derive(Default)]
struct Queue {
    sequence: u64,
    generation: u64,
    closed: bool,
    resetting: bool,
    reset_pending: Option<AlabGoalRuntimeUpdatedNotification>,
    reset_in_flight: bool,
    slots: HashMap<String, Arc<Slot>>,
}
struct Slot {
    thread: String,
    owner: Weak<Inner>,
    state: Mutex<Pending>,
}
#[derive(Default)]
struct Pending {
    update: Option<AlabGoalRuntimeUpdatedNotification>,
    latest_sequence: u64,
    in_flight: bool,
    inline_custodies: usize,
}

/// The sole passive worker relinquishes its selected slot on every exit.
struct Selected(Arc<Slot>);
impl Drop for Selected {
    fn drop(&mut self) {
        self.0.finish();
    }
}
impl Slot {
    fn finish(self: &Arc<Self>) {
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let mut queue = owner
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !queue
            .slots
            .get(&self.thread)
            .is_some_and(|slot| Arc::ptr_eq(slot, self))
        {
            return;
        }
        let mut pending = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        pending.in_flight = false;
        let again = pending.update.is_some();
        let retire = !again && pending.inline_custodies == 0;
        drop(pending);
        if retire {
            queue.slots.remove(&self.thread);
        }
        drop(queue);
        if again {
            owner.wake();
        }
    }

    fn select(self: &Arc<Self>) -> Option<AlabGoalRuntimeSnapshot> {
        let owner = self.owner.upgrade()?;
        let queue = owner
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if queue.closed
            || !queue
                .slots
                .get(&self.thread)
                .is_some_and(|slot| Arc::ptr_eq(slot, self))
        {
            return None;
        }
        let update = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .update
            .take()?;
        Some(custody(Arc::clone(self), update, false))
    }
}
impl Inner {
    fn wake(&self) {
        self.changed
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }
    fn close(&self) {
        let mut queue = self
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        queue.closed = true;
        queue.reset_pending = None;
        queue.slots.clear();
        drop(queue);
        self.wake();
    }
    fn stamp(
        &self,
        queue: &mut Queue,
        mut update: AlabGoalRuntimeUpdatedNotification,
    ) -> AlabGoalRuntimeUpdatedNotification {
        queue.sequence = queue.sequence.saturating_add(1);
        update.transport.clone_from(&self.identity);
        update.generation = queue.generation;
        update.sequence = queue.sequence;
        update
    }
    fn insert(
        self: &Arc<Self>,
        queue: &mut Queue,
        update: AlabGoalRuntimeUpdatedNotification,
    ) -> Arc<Slot> {
        let slot = queue
            .slots
            .entry(update.thread_id.clone())
            .or_insert_with(|| {
                Arc::new(Slot {
                    thread: update.thread_id.clone(),
                    owner: Arc::downgrade(self),
                    state: Mutex::default(),
                })
            });
        let mut pending = slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        pending.latest_sequence = update.sequence;
        pending.update = Some(update);
        Arc::clone(slot)
    }
    fn overflow(&self, queue: &mut Queue) {
        queue.resetting = true;
        for slot in queue.slots.values() {
            slot.state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .update = None;
        }
        queue.slots.clear();
        let reset = AlabGoalRuntimeUpdatedNotification {
            thread_id: String::new(),
            incarnation: String::new(),
            transport: String::new(),
            generation: 0,
            sequence: 0,
            revision: 0,
            persistence: None,
            writes_pending: false,
            reset: true,
        };
        queue.reset_pending = Some(self.stamp(queue, reset));
    }
}

fn custody(
    slot: Arc<Slot>,
    update: AlabGoalRuntimeUpdatedNotification,
    inline: bool,
) -> AlabGoalRuntimeSnapshot {
    let selected = update.clone();
    AlabGoalRuntimeSnapshot::new(update, move |outcome| {
        let Some(owner) = slot.owner.upgrade() else {
            if inline {
                slot.state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .inline_custodies -= 1;
            }
            return;
        };
        let mut queue = owner
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut pending = slot
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inline {
            pending.inline_custodies -= 1;
        }
        if outcome == AlabGoalRuntimeDelivery::Closed {
            drop(pending);
            drop(queue);
            owner.close();
            return;
        }
        if queue.closed
            || !queue
                .slots
                .get(&slot.thread)
                .is_some_and(|current| Arc::ptr_eq(current, &slot))
        {
            return;
        }
        match outcome {
            AlabGoalRuntimeDelivery::Sent => {
                if pending
                    .update
                    .as_ref()
                    .is_some_and(|update| update.sequence == selected.sequence)
                {
                    pending.update = None;
                }
            }
            AlabGoalRuntimeDelivery::Cancelled => {
                // A newer inline snapshot may already have been acknowledged and
                // left no pending value. Its high-water mark still fences this one.
                if pending.latest_sequence == selected.sequence && pending.update.is_none() {
                    pending.update = Some(selected);
                }
            }
            AlabGoalRuntimeDelivery::Closed => {}
        }
        let again = pending.update.is_some();
        let in_flight = pending.in_flight;
        let retire = !again && !in_flight && pending.inline_custodies == 0;
        drop(pending);
        if retire {
            queue.slots.remove(&slot.thread);
        }
        drop(queue);
        if again && !in_flight {
            owner.wake();
        }
    })
}

fn reset_custody(
    owner: &Arc<Inner>,
    update: AlabGoalRuntimeUpdatedNotification,
) -> AlabGoalRuntimeSnapshot {
    let sequence = update.sequence;
    let owner = Arc::downgrade(owner);
    AlabGoalRuntimeSnapshot::new(update, move |outcome| {
        let Some(owner) = owner.upgrade() else {
            return;
        };
        if outcome == AlabGoalRuntimeDelivery::Closed {
            owner.close();
            return;
        }
        let mut queue = owner
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if queue.closed
            || !queue
                .reset_pending
                .as_ref()
                .is_some_and(|reset| reset.sequence == sequence)
        {
            return;
        }
        queue.reset_in_flight = false;
        if outcome == AlabGoalRuntimeDelivery::Sent {
            queue.reset_pending = None;
            queue.resetting = false;
            queue.generation = queue.generation.wrapping_add(1);
        }
        drop(queue);
        owner.wake();
    })
}

impl Transport {
    pub(crate) fn new(outgoing: &Arc<OutgoingMessageSender>) -> Self {
        let (changed, mut receiver) = watch::channel(0_u64);
        let inner = Arc::new(Inner {
            identity: codex_alab_core::goal_persistence::observation_incarnation(),
            queue: Mutex::default(),
            changed,
        });
        let owner = Arc::downgrade(&inner);
        let outgoing = Arc::downgrade(outgoing);
        tokio::spawn(async move {
            loop {
                receiver.borrow_and_update();
                let Some(inner) = owner.upgrade() else {
                    return;
                };
                let work = {
                    let mut queue = inner
                        .queue
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if queue.closed {
                        return;
                    }
                    if !queue.reset_in_flight
                        && let Some(update) = queue.reset_pending.clone()
                    {
                        queue.reset_in_flight = true;
                        Some(Work::Reset(reset_custody(&inner, update)))
                    } else {
                        queue.slots.values().find_map(|slot| {
                            let mut pending = slot
                                .state
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if pending.in_flight || pending.update.is_none() {
                                return None;
                            }
                            pending.in_flight = true;
                            Some(Work::Selected(Selected(Arc::clone(slot))))
                        })
                    }
                };
                drop(inner);
                if let Some(work) = work {
                    let Some(outgoing) = outgoing.upgrade() else {
                        if let Some(inner) = owner.upgrade() {
                            inner.close();
                        }
                        return;
                    };
                    match work {
                        Work::Reset(snapshot) => send(&outgoing, snapshot).await,
                        Work::Selected(selected) => send_selected(&outgoing, selected).await,
                    }
                    continue;
                }
                if receiver.changed().await.is_err() {
                    return;
                }
            }
        });
        Self(inner)
    }
    pub(crate) fn publish(&self, update: AlabGoalRuntimeUpdatedNotification) {
        let mut queue = self
            .0
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if queue.closed || queue.resetting {
            return;
        }
        if queue.slots.len() >= MAX_PENDING && !queue.slots.contains_key(&update.thread_id) {
            self.0.overflow(&mut queue);
        } else {
            let update = self.0.stamp(&mut queue, update);
            self.0.insert(&mut queue, update);
        }
        drop(queue);
        self.0.wake();
    }
    /// Synchronous capture+stamp under native current-owner publication custody.
    pub(crate) fn inline(
        &self,
        update: AlabGoalRuntimeUpdatedNotification,
    ) -> Option<AlabGoalRuntimeSnapshot> {
        let mut queue = self
            .0
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if queue.closed || queue.resetting {
            return None;
        }
        if queue.slots.len() >= MAX_PENDING && !queue.slots.contains_key(&update.thread_id) {
            self.0.overflow(&mut queue);
            drop(queue);
            self.0.wake();
            return None;
        }
        let update = self.0.stamp(&mut queue, update);
        let slot = self.0.insert(&mut queue, update.clone());
        {
            let mut pending = slot
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            pending.inline_custodies += 1;
            pending.update = None;
        }
        Some(custody(slot, update, true))
    }
}

async fn send_selected(outgoing: &Arc<OutgoingMessageSender>, selected: Selected) {
    if let Some(snapshot) = selected.0.select() {
        send(outgoing, snapshot).await;
    }
}
pub(crate) async fn send(outgoing: &Arc<OutgoingMessageSender>, snapshot: AlabGoalRuntimeSnapshot) {
    let _ = outgoing.alab_queue_goal_runtime(snapshot).await;
}
/// Snapshot custody is released before awaiting the independent database read.
pub(crate) async fn before_goal_read<F: Future>(
    outgoing: &Arc<OutgoingMessageSender>,
    update: Option<AlabGoalRuntimeSnapshot>,
    read: F,
) -> F::Output {
    if let Some(update) = update {
        send(outgoing, update).await;
    }
    read.await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outgoing_message::OutgoingEnvelope;
    use crate::outgoing_message::OutgoingMessage;
    use crate::thread_state::ThreadStateManager;
    use codex_analytics::AnalyticsEventsClient;
    use codex_app_server_protocol::AlabGoalPersistencePhase;
    use codex_app_server_protocol::ServerNotification;
    use codex_protocol::ThreadId;
    use tokio::sync::mpsc;
    use tokio::time::Duration;
    use tokio::time::timeout;

    fn observation(
        thread: ThreadId,
        revision: u64,
        phase: Option<AlabGoalPersistencePhase>,
    ) -> AlabGoalRuntimeUpdatedNotification {
        AlabGoalRuntimeUpdatedNotification {
            thread_id: thread.to_string(),
            incarnation: "registered-native-owner".into(),
            transport: String::new(),
            generation: 0,
            sequence: 0,
            revision,
            persistence: phase,
            writes_pending: false,
            reset: false,
        }
    }
    fn selected(transport: &Transport, thread: ThreadId) -> Selected {
        let slot = transport.0.queue.lock().unwrap().slots[&thread.to_string()].clone();
        slot.state.lock().unwrap().in_flight = true;
        Selected(slot)
    }
    async fn wait_until(ready: impl Fn() -> bool) {
        timeout(Duration::from_secs(5), async {
            while !ready() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    async fn prefill(outgoing: &Arc<OutgoingMessageSender>) {
        outgoing
            .send_server_notification(ServerNotification::ThreadGoalCleared(
                codex_app_server_protocol::ThreadGoalClearedNotification {
                    thread_id: ThreadId::new().to_string(),
                },
            ))
            .await;
    }
    fn wire_status(envelope: OutgoingEnvelope) -> AlabGoalRuntimeUpdatedNotification {
        let OutgoingEnvelope::Broadcast {
            message: OutgoingMessage::AppServerNotification(envelope),
        } = envelope
        else {
            panic!("native notification envelope");
        };
        let packet = serde_json::to_value(envelope.notification).unwrap();
        let ServerNotification::AlabGoalRuntimeUpdated(update) =
            serde_json::from_value(packet).unwrap()
        else {
            panic!("typed runtime notification");
        };
        update
    }
    async fn receive(
        rx: &mut mpsc::Receiver<OutgoingEnvelope>,
    ) -> AlabGoalRuntimeUpdatedNotification {
        wire_status(
            timeout(Duration::from_secs(5), rx.recv())
                .await
                .unwrap()
                .unwrap(),
        )
    }

    #[tokio::test]
    async fn goal_runtime_status_native_sink_reaches_wire_without_db_listener_or_host_retention() {
        let (tx, mut rx) = mpsc::channel(8);
        let outgoing = Arc::new(OutgoingMessageSender::new(
            tx,
            AnalyticsEventsClient::disabled(),
        ));
        let manager = ThreadStateManager::new();
        let thread = ThreadId::new();
        let (listener, mut commands) = mpsc::unbounded_channel();
        manager.register_listener_command_tx(thread, listener);
        let sink =
            crate::extensions::app_server_extension_event_sink(outgoing.clone(), manager.clone());
        sink.alab_goal_runtime_updated(observation(
            thread,
            11,
            Some(AlabGoalPersistencePhase::Held),
        ));
        let held = receive(&mut rx).await;
        sink.alab_goal_runtime_updated(observation(
            thread,
            18,
            Some(AlabGoalPersistencePhase::Ready),
        ));
        let ready = receive(&mut rx).await;
        assert_eq!((held.revision, ready.revision), (11, 18));
        assert_eq!(held.transport, ready.transport);
        assert!(held.sequence < ready.sequence);
        assert!(
            commands.try_recv().is_err(),
            "passive status never enters the DB-owning listener"
        );
        let weak = Arc::downgrade(&outgoing);
        drop(sink);
        drop(manager);
        drop(outgoing);
        assert!(
            weak.upgrade().is_none(),
            "idle observation keeps no native host alive"
        );
    }

    #[tokio::test]
    async fn goal_runtime_status_inline_snapshot_reaches_wire_while_goal_read_remains_gated() {
        let (tx, mut rx) = mpsc::channel(1);
        let outgoing = Arc::new(OutgoingMessageSender::new(
            tx,
            AnalyticsEventsClient::disabled(),
        ));
        let transport = Transport::new(&outgoing);
        let snapshot = transport
            .inline(observation(
                ThreadId::new(),
                23,
                Some(AlabGoalPersistencePhase::Held),
            ))
            .unwrap();
        let (read_started, started) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let job = tokio::spawn({
            let outgoing = Arc::clone(&outgoing);
            async move {
                before_goal_read(&outgoing, Some(snapshot), async {
                    read_started.send(()).unwrap();
                    released.await.unwrap();
                    Err::<(), _>("injected goal snapshot read failure")
                })
                .await
            }
        });
        timeout(Duration::from_secs(5), started)
            .await
            .unwrap()
            .unwrap();
        let update = receive(&mut rx).await;
        assert!(
            !job.is_finished(),
            "the actual goal-read future remains gated"
        );
        assert_eq!(update.persistence, Some(AlabGoalPersistencePhase::Held));
        assert!(!update.writes_pending && update.sequence > 0 && !update.transport.is_empty());
        release.send(()).unwrap();
        assert_eq!(
            job.await.unwrap(),
            Err("injected goal snapshot read failure")
        );
        assert!(
            rx.try_recv().is_err(),
            "a failed read does not fabricate a goal clear"
        );
    }

    #[tokio::test]
    async fn goal_runtime_status_cancelled_blocked_selection_and_inline_preserve_each_final_phase()
    {
        for inline in [false, true] {
            for phase in [
                Some(AlabGoalPersistencePhase::Ready),
                Some(AlabGoalPersistencePhase::Held),
                None,
            ] {
                let (tx, mut rx) = mpsc::channel(1);
                let outgoing = Arc::new(OutgoingMessageSender::new(
                    tx,
                    AnalyticsEventsClient::disabled(),
                ));
                prefill(&outgoing).await;
                let transport = Transport::new(&outgoing);
                tokio::task::yield_now().await;
                let thread = ThreadId::new();
                let (started, began) = tokio::sync::oneshot::channel();
                let job = if inline {
                    transport.publish(observation(
                        thread,
                        20,
                        Some(AlabGoalPersistencePhase::Held),
                    ));
                    let stale = selected(&transport, thread);
                    let older = transport
                        .inline(observation(
                            thread,
                            21,
                            Some(AlabGoalPersistencePhase::Held),
                        ))
                        .unwrap();
                    let snapshot = transport.inline(observation(thread, 22, phase)).unwrap();
                    drop(older);
                    assert!(
                        stale.0.select().is_none(),
                        "a selected worker cannot consume inline-owned bytes"
                    );
                    drop(stale);
                    assert_eq!(transport.0.queue.lock().unwrap().slots.len(), 1);
                    let outgoing = Arc::clone(&outgoing);
                    tokio::spawn(async move {
                        started.send(()).unwrap();
                        send(&outgoing, snapshot).await;
                    })
                } else {
                    transport.publish(observation(thread, 22, phase));
                    // No await between publish and selecting this worker slot.
                    let selected = selected(&transport, thread);
                    let outgoing = Arc::clone(&outgoing);
                    tokio::spawn(async move {
                        started.send(()).unwrap();
                        send_selected(&outgoing, selected).await;
                    })
                };
                began.await.unwrap();
                assert!(!job.is_finished(), "the actual bounded output is full");
                job.abort();
                assert!(job.await.unwrap_err().is_cancelled());
                rx.recv().await.unwrap();
                let update = receive(&mut rx).await;
                assert_eq!((update.revision, update.persistence), (22, phase));
                wait_until(|| transport.0.queue.lock().unwrap().slots.is_empty()).await;
                assert!(rx.try_recv().is_err(), "the final snapshot is queued once");
            }
        }
    }

    #[tokio::test]
    async fn goal_runtime_status_cancelled_old_selection_cannot_restore_over_acknowledged_inline() {
        let (tx, mut rx) = mpsc::channel(1);
        let outgoing = Arc::new(OutgoingMessageSender::new(
            tx,
            AnalyticsEventsClient::disabled(),
        ));
        let transport = Transport::new(&outgoing);
        let thread = ThreadId::new();
        transport.publish(observation(
            thread,
            15,
            Some(AlabGoalPersistencePhase::Held),
        ));
        let selected = selected(&transport, thread);
        let old = selected.0.select().unwrap();
        let (started, began) = tokio::sync::oneshot::channel();
        let blocked = tokio::spawn(async move {
            let _flight = selected;
            let _old = old; // drops before flight, so an incorrect restoration would be retried
            started.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        began.await.unwrap();
        send(
            &outgoing,
            transport
                .inline(observation(
                    thread,
                    16,
                    Some(AlabGoalPersistencePhase::Ready),
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(receive(&mut rx).await.revision, 16);
        blocked.abort();
        assert!(blocked.await.unwrap_err().is_cancelled());
        assert!(transport.0.queue.lock().unwrap().slots.is_empty());
        assert!(
            rx.try_recv().is_err(),
            "old selection cannot resurrect after newer acknowledgement"
        );
    }

    #[tokio::test]
    async fn goal_runtime_status_coalesces_blocked_worker_and_retires_each_final_state() {
        for phase in [
            Some(AlabGoalPersistencePhase::Ready),
            Some(AlabGoalPersistencePhase::Held),
            None,
        ] {
            let (tx, mut rx) = mpsc::channel(1);
            let outgoing = Arc::new(OutgoingMessageSender::new(
                tx,
                AnalyticsEventsClient::disabled(),
            ));
            prefill(&outgoing).await;
            let transport = Transport::new(&outgoing);
            let thread = ThreadId::new();
            transport.publish(observation(thread, 1, Some(AlabGoalPersistencePhase::Held)));
            wait_until(|| {
                transport.0.queue.lock().unwrap().slots[&thread.to_string()]
                    .state
                    .lock()
                    .unwrap()
                    .update
                    .is_none()
            })
            .await;
            for revision in 2..=512 {
                transport.publish(observation(thread, revision, phase));
            }
            assert_eq!(transport.0.queue.lock().unwrap().slots.len(), 1);
            rx.recv().await.unwrap();
            assert_eq!(receive(&mut rx).await.revision, 1);
            let last = receive(&mut rx).await;
            assert_eq!((last.revision, last.persistence), (512, phase));
            wait_until(|| transport.0.queue.lock().unwrap().slots.is_empty()).await;
        }
    }

    #[tokio::test]
    async fn goal_runtime_status_repeated_churn_reset_bypasses_stalled_listener_and_closes_terminally()
     {
        let (tx, mut rx) = mpsc::channel(1);
        let outgoing = Arc::new(OutgoingMessageSender::new(
            tx,
            AnalyticsEventsClient::disabled(),
        ));
        let manager = ThreadStateManager::new();
        let thread = ThreadId::new();
        let (listener, mut commands) = mpsc::unbounded_channel();
        manager.register_listener_command_tx(thread, listener);
        let transport = Transport::new(&outgoing);
        let (release, released) = tokio::sync::oneshot::channel();
        let read = tokio::spawn({
            let outgoing = Arc::clone(&outgoing);
            async move {
                before_goal_read(&outgoing, None, async {
                    released.await.unwrap();
                })
                .await
            }
        });
        let mut previous = None;
        for _ in 0..4 {
            prefill(&outgoing).await;
            for revision in 0..(MAX_PENDING * 8) {
                transport.publish(observation(ThreadId::new(), revision as u64, None));
            }
            wait_until(|| transport.0.queue.lock().unwrap().reset_in_flight).await;
            for revision in 0..(MAX_PENDING * 8) {
                transport.publish(observation(ThreadId::new(), revision as u64, None));
            }
            {
                let queue = transport.0.queue.lock().unwrap();
                assert!(queue.resetting && queue.slots.is_empty() && queue.reset_pending.is_some());
            }
            rx.recv().await.unwrap();
            let reset = receive(&mut rx).await;
            assert!(reset.reset && reset.persistence.is_none());
            assert!(!transport.0.queue.lock().unwrap().resetting);
            send(
                &outgoing,
                transport
                    .inline(observation(
                        thread,
                        23,
                        Some(AlabGoalPersistencePhase::Held),
                    ))
                    .unwrap(),
            )
            .await;
            let fresh = receive(&mut rx).await;
            assert_eq!(fresh.persistence, Some(AlabGoalPersistencePhase::Held));
            assert_eq!(fresh.transport, reset.transport);
            assert!(fresh.generation > reset.generation);
            assert_ne!(Some(&fresh.generation), previous.as_ref());
            previous = Some(fresh.generation);
            assert!(
                !read.is_finished(),
                "resync does not wait behind the earlier gated DB read"
            );
            assert!(
                commands.try_recv().is_err(),
                "no passive tokens accumulate across reset generations"
            );
            wait_until(|| transport.0.queue.lock().unwrap().slots.is_empty()).await;
        }
        release.send(()).unwrap();
        read.await.unwrap();
        drop(rx);
        transport.publish(observation(
            thread,
            24,
            Some(AlabGoalPersistencePhase::Ready),
        ));
        wait_until(|| {
            transport.0.queue.lock().unwrap().closed && transport.0.changed.receiver_count() == 0
        })
        .await;
        for revision in 0..(MAX_PENDING * 8) {
            transport.publish(observation(ThreadId::new(), revision as u64, None));
        }
        let queue = transport.0.queue.lock().unwrap();
        assert!(queue.closed && queue.slots.is_empty() && queue.reset_pending.is_none());
    }

    #[tokio::test]
    async fn goal_runtime_status_cancelled_blocked_reset_keeps_one_control_and_rearms() {
        let (tx, mut rx) = mpsc::channel(1);
        let outgoing = Arc::new(OutgoingMessageSender::new(
            tx,
            AnalyticsEventsClient::disabled(),
        ));
        prefill(&outgoing).await;
        let transport = Transport::new(&outgoing);
        for revision in 0..=MAX_PENDING {
            transport.publish(observation(ThreadId::new(), revision as u64, None));
        }
        let reset = {
            let mut queue = transport.0.queue.lock().unwrap();
            queue.reset_in_flight = true;
            reset_custody(&transport.0, queue.reset_pending.clone().unwrap())
        };
        let (started, began) = tokio::sync::oneshot::channel();
        let job = tokio::spawn({
            let outgoing = Arc::clone(&outgoing);
            async move {
                started.send(()).unwrap();
                send(&outgoing, reset).await;
            }
        });
        began.await.unwrap();
        assert!(!job.is_finished());
        job.abort();
        assert!(job.await.unwrap_err().is_cancelled());
        rx.recv().await.unwrap();
        assert!(receive(&mut rx).await.reset);
        let queue = transport.0.queue.lock().unwrap();
        assert!(!queue.resetting && queue.reset_pending.is_none() && queue.slots.is_empty());
    }
}
