// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Conditional event-backlog policy, separate from the dual-Book lineage guard.
use super::BackendSessions;
use crate::app::App;
use crate::app::thread_events::ThreadBufferedEvent;
use crate::app_event::AppEvent;
use crate::bottom_pane::ComposerDraftSnapshot;
use crate::chatwidget::ThreadInputState;
use std::io;
use tokio::sync::mpsc::UnboundedReceiver;

impl App {
    pub(in crate::app) fn alab_admit_upgrade_event(
        &mut self,
        event: AppEvent,
        inbox: &UnboundedReceiver<AppEvent>,
        pending_actor_work: bool,
        pending_startup_draft: Option<&ComposerDraftSnapshot>,
        backends: &BackendSessions,
    ) -> Option<AppEvent> {
        if let AppEvent::AlabAccountsCheckInput { thread_id, reply } = event {
            let clear = thread_id.is_some()
                && self.chat_widget.thread_id() == thread_id
                && self.chat_widget.composer_is_empty()
                && !self.chat_widget.alab_has_upgrade_input()
                && !self.alab_has_protected_restart_input(pending_startup_draft, backends)
                && !self.saved_openings.alab_owns_saved_opening_work()
                // The last flight may have freed its slot and queued the next opening.
                && !self.waiting_saved_chat().is_some_and(|target| {
                    target.backend_id == backends.displayed_owner() && self.wants_saved_chat(target)
                })
                // The check has been dequeued. A later app event can own a submission
                // that hasn't reached the widget or its parked drafts yet.
                && inbox.is_empty();
            let _ = reply.send(clear);
            return None;
        }
        let event =
            self.saved_openings
                .alab_admit_event(event, inbox, pending_actor_work, backends)?;
        if matches!(&event, AppEvent::AlabUpgrade(_))
            && self.alab_has_protected_restart_input(pending_startup_draft, backends)
        {
            return None;
        }
        Some(event)
    }

    fn alab_has_protected_restart_input(
        &self,
        pending_startup_draft: Option<&ComposerDraftSnapshot>,
        backends: &BackendSessions,
    ) -> bool {
        if pending_startup_draft.is_some_and(startup_draft_has_input)
            || self.has_queued_startup_protected_request()
            || self.sidebars.values().any(|pane| {
                pane.creation
                    .as_ref()
                    .is_some_and(|creation| creation.message.is_some())
            })
            || self
                .side_threads
                .values()
                .any(|side| side.opening_message.is_some())
            || self
                .pending_primary_events
                .iter()
                .any(|event| matches!(event, ThreadBufferedEvent::Request(_)))
            || self
                .agents_overview
                .input_states
                .values()
                .any(ThreadInputState::alab_has_upgrade_input)
            || backends.entries.iter().any(|entry| {
                entry
                    .parked_inputs
                    .values()
                    .any(ThreadInputState::alab_has_upgrade_input)
            })
        {
            return true;
        }

        let Some(displayed) = backends.entries.get(backends.displayed.0) else {
            return true;
        };
        let displayed_owner = Some(displayed.session.backend_id());
        let widget_owns_receiver = self.active_thread_rx.is_some()
            && self.app_server_backend_id == displayed_owner
            && self.displayed_conversation_backend_id() == displayed_owner;
        self.thread_event_channels.iter().any(|(thread, channel)| {
            let Ok(store) = channel.store.try_lock() else {
                return true;
            };
            // Activation clones a snapshot into the widget. A consumed draft may
            // still be in that store until the widget parks its live receiver.
            let widget_owns_input = widget_owns_receiver
                && self.active_thread_id == Some(*thread)
                && self.chat_widget.thread_id() == Some(*thread)
                && channel.receiver.is_none()
                && store.active;
            store.side_parent_pending_status().is_some()
                || !widget_owns_input
                    && store
                        .input_state
                        .as_ref()
                        .is_some_and(ThreadInputState::alab_has_upgrade_input)
        })
    }
}

/// The loop can still own this draft when it dequeues an upgrade as its last
/// pending App event. Startup history holds submitted entries only for recall.
fn startup_draft_has_input(draft: &ComposerDraftSnapshot) -> bool {
    !draft.text.is_empty()
        || !draft.text_elements.is_empty()
        || !draft.local_images.is_empty()
        || !draft.remote_image_urls.is_empty()
        || !draft.mention_bindings.is_empty()
        || !draft.pending_pastes.is_empty()
}

impl BackendSessions {
    pub(in crate::app) fn alab_upgrade_events_pending(&self) -> io::Result<bool> {
        if self.entries.is_empty() || self.displayed.0 >= self.entries.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "conditional upgrade event observation has no displayed backend",
            ));
        }
        let mut pending = false;
        for entry in &self.entries {
            // Include every entry, even hidden, paused, or not listening. Do not
            // short-circuit away an unavailable later physical owner.
            pending |= !entry.pending_events.is_empty() || !entry.side_startup.is_empty();
            pending |= entry.session.alab_upgrade_events_pending()?;
        }
        Ok(pending)
    }
}

#[cfg(test)]
#[path = "backend_upgrade_events_tests.rs"]
mod tests;
