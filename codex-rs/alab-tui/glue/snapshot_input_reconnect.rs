// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::backend_id::BackendId;

pub(super) fn validate_returned_thread(
    requested: Option<ThreadId>,
    connected: Reconnected,
) -> Result<Reconnected> {
    if let Some(started) = &connected.thread {
        color_eyre::eyre::ensure!(
            Some(started.session.thread_id) == requested,
            "reconnect resumed a different conversation"
        );
    }
    Ok(connected)
}

pub(super) fn belongs_to_reconnect(
    thread: ThreadId,
    channel_owner: Option<BackendId>,
    registry: &backend_registry::BackendRegistry,
    backend: BackendId,
) -> bool {
    channel_owner
        .or_else(|| registry.owner(thread))
        .is_none_or(|owner| owner == backend)
}

pub(super) fn writable_result(
    session: &AppServerSession,
    returned: Option<&AppServerStartedThread>,
) -> Option<(BackendId, ThreadId)> {
    returned
        .filter(|started| !started.blocks_direct_input)
        .map(|started| (session.backend_id(), started.session.thread_id))
}

impl App {
    pub(super) fn alab_check_reconnect_input_owner(
        &self,
        outgoing: BackendId,
        connected: &Reconnected,
    ) -> Result<Option<ThreadId>> {
        let incoming = connected.session.backend_id();
        if let Some(returned) = &connected.thread {
            let thread = returned.session.thread_id;
            color_eyre::eyre::ensure!(
                belongs_to_reconnect(
                    thread,
                    self.thread_event_channels
                        .get(&thread)
                        .and_then(|c| c.backend_id),
                    &self.backend_registry,
                    incoming
                ),
                "reconnect would replace another backend's conversation"
            );
        }
        let widget = self.chat_widget.thread_id();
        let matches_backend = |owner| owner == outgoing && owner == incoming;
        color_eyre::eyre::ensure!(
            self.app_server_backend_id.is_none_or(matches_backend)
                && self
                    .displayed_conversation_backend_id()
                    .is_none_or(matches_backend)
                && widget.is_none_or(|thread| incoming == outgoing
                    && self.current_displayed_thread_id() == Some(thread)),
            "reconnect changed the configured input owner"
        );
        Ok(widget)
    }

    pub(super) fn alab_mark_reconnected_parked_inputs(&mut self, backend: BackendId) {
        for (&thread, input) in &mut self.agents_overview.input_states {
            let owner = self
                .thread_event_channels
                .get(&thread)
                .and_then(|c| c.backend_id);
            if belongs_to_reconnect(thread, owner, &self.backend_registry, backend) {
                input.reconnect_pending = true;
            }
        }
    }

    pub(super) async fn alab_reconnected_store_input(
        &mut self,
        captured: Option<ThreadId>,
        returned: ThreadId,
        input: Option<&ThreadInputState>,
    ) -> Option<ThreadInputState> {
        if captured == Some(returned) {
            self.agents_overview.input_states.remove(&returned);
            return input.cloned();
        }
        let channel = self.thread_event_channels.get(&returned)?;
        channel.store.lock().await.input_state.clone()
    }

    pub(in crate::app) fn alab_release_reconnected_input(
        &mut self,
        displayed: Option<ThreadId>,
        resumed: Option<(BackendId, ThreadId)>,
    ) {
        let Some((backend, thread)) = resumed else {
            return;
        };
        if displayed == Some(thread)
            && self.active_thread_id == Some(thread)
            && self.app_server_backend_id == Some(backend)
            && self.chat_widget.thread_id() == Some(thread)
            && self.displayed_conversation_backend_id() == Some(backend)
            && self
                .thread_event_channels
                .get(&thread)
                .is_some_and(|channel| channel.attachment() == ThreadEventAttachment::Live)
        {
            self.chat_widget.alab_resume_snapshot_input();
        }
    }
}
