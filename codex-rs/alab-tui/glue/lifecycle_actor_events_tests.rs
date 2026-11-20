// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::super::Flight;
use super::Openings;
use crate::app_event::AppEvent;
use crate::app_event::ExitMode;
use crate::app_event_sender::AppEventSender;
use crate::backend_id::BackendId;
use codex_alab_core::session::Shared;
use codex_alab_core::session::UpgradeCancellation;
use codex_alab_core::session::UpgradeRequest;
use codex_protocol::ThreadId;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use uuid::Uuid;

fn upgrade() -> (UpgradeRequest, UpgradeCancellation, oneshot::Receiver<bool>) {
    let (answer, observed) = oneshot::channel();
    let (request, cancellation) = UpgradeRequest::new(move |ready| {
        let _ = answer.send(ready);
        ready
    });
    (request, cancellation, observed)
}

impl Openings {
    pub(in crate::app) fn alab_test_register_lifecycle(
        &self,
        backend: &AppEventSender,
    ) -> (Shared, AppEventSender) {
        let shared = Shared::new();
        assert!(shared.set_leave(Box::new(self.alab_shutdown_callback())));
        assert!(shared.set_upgrade_leave(Box::new(self.alab_upgrade_callback())));
        (shared, backend.clone())
    }

    pub(in crate::app) fn alab_test_reconnected_lifecycle(
        &mut self,
        (shared, old_backend): (Shared, AppEventSender),
    ) {
        assert!(old_backend.app_event_tx.is_closed());
        assert!(self.completions.is_empty());
        let (original, _cancellation, mut observed) = upgrade();
        assert!(shared.queue_upgrade(original));
        assert!(shared.leave());
        let Ok(AppEvent::AlabUpgrade(received)) = self.completions.try_recv() else {
            panic!("the registered upgrade failed to reach the reconnected actor");
        };
        assert!(received.decide(true));
        assert_eq!(observed.try_recv(), Ok(true));
        assert!(matches!(
            self.completions.try_recv(),
            Ok(AppEvent::Exit(ExitMode::ShutdownFirst))
        ));
        assert!(self.completions.is_empty());
    }
}

#[tokio::test]
async fn registered_lifecycle_sender_survives_two_backend_rotations() {
    let mut actor = Openings::default();
    let on_upgrade = actor.alab_upgrade_callback();
    let on_shutdown = actor.alab_shutdown_callback();
    let (original, first_receiver) = mpsc::unbounded_channel();
    let (request, _cancellation, observed) = upgrade();
    on_upgrade(request);
    drop(first_receiver);
    let (second, mut events) = mpsc::unbounded_channel();
    assert!(original.send(AppEvent::StartQueuedSavedChat).is_err());
    let Some(received) = actor.next_event(&mut events).await else {
        panic!("actor lost the original upgrade request");
    };
    let Some(AppEvent::AlabUpgrade(request)) =
        actor.alab_admit_actor_event(received, &events, false)
    else {
        panic!("idle actor refused the original request");
    };
    assert!(request.decide(true));
    assert_eq!(observed.await, Ok(true));

    on_shutdown();
    drop(events);
    let (_current, mut events) = mpsc::unbounded_channel();
    assert!(second.send(AppEvent::StartQueuedSavedChat).is_err());
    let Some(received) = actor.next_event(&mut events).await else {
        panic!("actor lost the signal requesting process shutdown");
    };
    assert!(matches!(
        actor.alab_admit_actor_event(received, &events, true),
        Some(AppEvent::Exit(ExitMode::ShutdownFirst))
    ));
    assert!(actor.completions.is_empty());
}

#[tokio::test]
async fn selected_upgrade_leaves_each_queued_action_for_its_original_receiver() {
    for actor_queue in [false, true] {
        let mut actor = Openings::default();
        let (backend, mut events) = mpsc::unbounded_channel();
        let (request, _cancellation, observed) = upgrade();
        let on_upgrade = actor.alab_upgrade_callback();
        on_upgrade(request);
        // Select the original request, then enqueue real work before its leave decision.
        let Some(received) = actor.next_event(&mut events).await else {
            panic!("actor did not select its request");
        };
        if actor_queue {
            actor.tx.send(AppEvent::StartQueuedSavedChat);
        } else {
            assert!(backend.send(AppEvent::StartQueuedSavedChat).is_ok());
        }
        assert!(
            actor
                .alab_admit_actor_event(received, &events, false)
                .is_none()
        );
        assert_eq!(observed.await, Ok(false));
        assert!(matches!(
            actor.next_event(&mut events).await,
            Some(AppEvent::StartQueuedSavedChat)
        ));
        assert!(actor.completions.is_empty() && events.is_empty());
    }
}

#[tokio::test]
async fn held_actor_work_and_retired_saved_openings_preserve_the_original_operation() {
    let mut actor = Openings::default();
    let (_backend, events) = mpsc::unbounded_channel();
    let (request, _cancellation, observed) = upgrade();
    assert!(
        actor
            .alab_admit_actor_event(AppEvent::AlabUpgrade(request), &events, true)
            .is_none()
    );
    assert_eq!(observed.await, Ok(false));

    let key = (BackendId::new(), ThreadId::new());
    let original = Uuid::new_v4();
    actor.active.insert(
        key,
        Flight {
            request: original,
            started: Instant::now(),
            retired: false,
            incoming: None,
        },
    );
    actor.abandon();
    let (request, _cancellation, observed) = upgrade();
    assert!(
        actor
            .alab_admit_actor_event(AppEvent::AlabUpgrade(request), &events, false)
            .is_none()
    );
    assert_eq!(observed.await, Ok(false));
    assert!(
        actor
            .active
            .get(&key)
            .is_some_and(|flight| flight.retired && flight.request == original)
    );
}

#[tokio::test]
async fn cancelled_lifecycle_request_keeps_its_original_decision_across_reconnect() {
    let mut actor = Openings::default();
    let on_upgrade = actor.alab_upgrade_callback();
    let (request, cancellation, observed) = upgrade();
    let (_previous, previous_events) = mpsc::unbounded_channel::<AppEvent>();
    on_upgrade(request);
    drop(cancellation);
    assert_eq!(observed.await, Ok(false));
    drop(previous_events);
    let (_current, mut events) = mpsc::unbounded_channel();
    let Some(AppEvent::AlabUpgrade(request)) = actor.next_event(&mut events).await else {
        panic!("original canceled request was replaced");
    };
    assert!(!request.decide(true));
    assert!(actor.completions.is_empty());
}
