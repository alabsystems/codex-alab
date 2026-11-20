// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::Openings;
use crate::app_event::AppEvent;
use crate::app_event::ExitMode;
use crate::app_server_session::AppServerSession;
use codex_alab_core::session::UpgradeRequest;
use tokio::sync::mpsc::UnboundedReceiver;

impl Openings {
    /// Keep waiting for the original reply after its physical backend is replaced.
    /// If it subscribed, only its reserved unsubscribe can release that connection.
    pub(in crate::app) fn alab_retire_connection(&mut self, outgoing: &AppServerSession) {
        self.alab_retire_saved_history(outgoing);
        for flight in self.active.values_mut() {
            if let Some(incoming) = &mut flight.incoming
                && incoming.release.retire_if_owned_by(outgoing)
            {
                flight.retired = true;
            }
        }
    }

    /// The startup actor receives from both inboxes. An unfinished worker has
    /// no queued result until it publishes to the completions inbox.
    pub(in crate::app) fn alab_events_pending(
        &self,
        backend_events: &UnboundedReceiver<AppEvent>,
    ) -> bool {
        !self.completions.is_empty() || !backend_events.is_empty()
    }

    /// This actor receives completions across backend reconnects. Process lifecycle callbacks
    /// registered once at startup need its original sender for the same reason.
    pub(in crate::app) fn alab_shutdown_callback(&self) -> impl Fn() + Send + Sync + use<> {
        let sender = self.tx.clone();
        move || sender.send(AppEvent::Exit(ExitMode::ShutdownFirst))
    }

    pub(in crate::app) fn alab_upgrade_callback(
        &self,
    ) -> impl Fn(UpgradeRequest) + Send + Sync + use<> {
        let sender = self.tx.clone();
        move |request| sender.send(AppEvent::AlabUpgrade(request))
    }

    /// Both inboxes are polled independently. A conditional leave selected ahead of observed
    /// work must reject this attempt and leave the original work on its owning queue.
    pub(in crate::app) fn alab_admit_actor_event(
        &self,
        event: AppEvent,
        backend_events: &UnboundedReceiver<AppEvent>,
        pending_actor_work: bool,
    ) -> Option<AppEvent> {
        if matches!(&event, AppEvent::AlabUpgrade(_))
            && (pending_actor_work
                || !self.active.is_empty()
                || self.alab_events_pending(backend_events))
        {
            // Dropping this exact UpgradeRequest resolves its decision as refused.
            return None;
        }
        Some(event)
    }
}

impl Openings {
    /// Conditional admission checks original event queues and current jobs. Busy,
    /// failed or changed cuts refuse this exact request; original jobs/FIFOs
    /// remain owned by their actual actor, and the Native scan is never rebased.
    pub(in crate::app) fn alab_admit_event(
        &mut self,
        event: AppEvent,
        backend_events: &UnboundedReceiver<AppEvent>,
        pending_actor_work: bool,
        backends: &crate::app::backend_sessions::BackendSessions,
    ) -> Option<AppEvent> {
        let event = self.alab_admit_actor_event(event, backend_events, pending_actor_work)?;
        let AppEvent::AlabUpgrade(request) = event else {
            return Some(event);
        };
        if self.alab_history_requires_observation() {
            if request.refuse_if_pending() {
                self.alab_retry_saved_history_for_upgrade(backends);
            }
            return None;
        }
        // Event queues and Book lineage are separate original-owner policies.
        if !matches!(backends.alab_upgrade_events_pending(), Ok(false)) {
            return None;
        }
        let ready = match backends.alab_try_upgrade_jobs() {
            Ok(Some(ready)) => ready,
            Ok(None) | Err(_) => return None,
        };
        if ready.current(backends).is_err() {
            return None;
        }
        // LAST: actual joins may have published original App/actor completions.
        // Never dequeue them, or equate an empty inbox with settled Book work.
        if !matches!(backends.alab_upgrade_events_pending(), Ok(false))
            || self.alab_events_pending(backend_events)
        {
            return None;
        }
        Some(AppEvent::AlabUpgrade(request))
    }
}

#[cfg(test)]
#[path = "lifecycle_actor_events_tests.rs"]
mod tests;
