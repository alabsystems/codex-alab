// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compiled inside app-server outgoing messages. A running-thread resume is already
//! serialized with its core event listener; this only restores current capacity display.

use super::ConnectionId;
use super::OutgoingEnvelope;
use super::OutgoingMessageSender;
use super::timestamped_server_notification;
use codex_alab_core::capacity::CapacityReason;
use codex_alab_core::capacity::LiveCapacityWait;
use codex_app_server_protocol::CodexErrorInfo;
use codex_app_server_protocol::ErrorNotification;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnError;
use codex_app_server_protocol::TurnStatus;
use codex_core::CodexThread;
use codex_protocol::ThreadId;
use std::future::Future;
use std::sync::Arc;

impl OutgoingMessageSender {
    /// Reconcile returned history with the listener's exact live-turn metadata.
    /// Metadata-only resumes intentionally return no turns; a pacing record is never used.
    pub(crate) fn alab_capacity_resume_turn(
        response: &ThreadResumeResponse,
        active_turn: Option<(&str, &TurnStatus)>,
    ) -> Option<String> {
        if !codex_alab_core::active() {
            return None;
        }
        reconcile_running_turn(
            active_turn,
            response.thread.turns.iter().chain(
                response
                    .initial_turns_page
                    .iter()
                    .flat_map(|page| page.data.iter()),
            ),
        )
    }

    /// Must run inline after the resume response in the same thread listener command.
    pub(crate) async fn alab_replay_capacity_wait(
        &self,
        connection: ConnectionId,
        thread: ThreadId,
        conversation: &CodexThread,
        expected_turn: Option<&str>,
    ) {
        if !codex_alab_core::active() {
            return;
        }
        let Some(turn) = expected_turn else { return };
        self.alab_send_capacity_wait_checked(
            connection,
            thread.to_string(),
            turn.to_owned(),
            || {
                // This existing accessor locks the actual core task, matches its exact turn ID,
                // and refuses a cancelled task. A later cancellation is ordered by the listener.
                conversation.current_turn_extension_data::<LiveCapacityWait>(turn)
            },
        )
        .await;
    }

    /// The production enqueue path is shared with the bounded backpressure tests.
    /// The loader is called only after capacity is reserved, and holds no state lock.
    async fn alab_send_capacity_wait_checked<Load, Loaded>(
        &self,
        connection: ConnectionId,
        thread: String,
        turn: String,
        load: Load,
    ) -> bool
    where
        Load: FnOnce() -> Loaded,
        Loaded: Future<Output = Option<Arc<LiveCapacityWait>>>,
    {
        let Ok(permit) = self.sender.reserve().await else {
            return false;
        };
        let Some(live) = load().await else {
            return false;
        };
        live.publish_reason_if_current(
            || tokio::time::Instant::now().into_std(),
            |reason| {
                let info = match reason {
                    CapacityReason::ServerOverloaded => CodexErrorInfo::ServerOverloaded,
                    CapacityReason::PolicyCapacityUnavailable => {
                        CodexErrorInfo::PolicyCapacityUnavailable
                    }
                };
                let message =
                    timestamped_server_notification(ServerNotification::Error(ErrorNotification {
                        error: TurnError {
                            message: reason.notice().into(),
                            codex_error_info: Some(info),
                            additional_details: None,
                            misalignment: None,
                        },
                        will_retry: true,
                        thread_id: thread,
                        turn_id: turn,
                    }));
                let envelope = OutgoingEnvelope::ToConnection {
                    connection_id: connection,
                    message,
                    write_complete_tx: None,
                };
                // Only synchronous handoff follows the under-lock clock/deadline check.
                permit.send(envelope);
            },
        )
        .is_some()
    }
}

fn reconcile_running_turn<'a>(
    active_turn: Option<(&str, &TurnStatus)>,
    turns: impl Iterator<Item = &'a Turn>,
) -> Option<String> {
    let (active_id, status) = active_turn?;
    if active_id.is_empty() || *status != TurnStatus::InProgress {
        return None;
    }
    for turn in turns {
        // Other completed history does not contradict metadata-only/paged resumes.
        // A different running ID or a terminal copy of the exact ID does.
        if (turn.status == TurnStatus::InProgress && turn.id != active_id)
            || (turn.id == active_id && turn.status != TurnStatus::InProgress)
        {
            return None;
        }
    }
    Some(active_id.to_owned())
}

#[cfg(test)]
#[path = "capacity_status_replay_tests.rs"]
mod tests;
