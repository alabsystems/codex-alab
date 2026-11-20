// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::App;
use super::ThreadBufferedEvent;
use super::ThreadEventSnapshot;
use codex_app_server_protocol::CodexErrorInfo;
use codex_app_server_protocol::ErrorNotification;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnError;
use codex_app_server_protocol::TurnStatus;

/// A replay may restore the hold, but only live delivery supplies review details.
#[derive(Debug, Default)]
pub(super) struct TerminalHold {
    ids: Option<(String, String)>,
}

fn refused(error: &TurnError) -> bool {
    error.codex_error_info == Some(CodexErrorInfo::MisalignmentPolicyViolation)
}

fn failed_turn(turn: &Turn) -> bool {
    turn.status == TurnStatus::Failed && turn.error.as_ref().is_some_and(refused)
}

fn refusal_ids(notification: &ServerNotification) -> Option<(&str, &str)> {
    match notification {
        ServerNotification::Error(n) if !n.will_retry && refused(&n.error) => {
            Some((&n.thread_id, &n.turn_id))
        }
        ServerNotification::TurnCompleted(n) if failed_turn(&n.turn) => {
            Some((&n.thread_id, &n.turn.id))
        }
        _ => None,
    }
}

impl App {
    /// A plain composer operation can outlive a thread switch in the app event FIFO.
    pub(super) async fn alab_recover_switched_steer(
        &mut self,
        event: &crate::app_event::AppEvent,
        mode: crate::app_server_session::ThreadParamsMode,
    ) -> bool {
        let crate::app_event::AppEvent::CodexOp(op) = event else {
            return false;
        };
        let Some((source, _)) = op.alab_observed_steer(mode) else {
            return false;
        };
        if self.active_thread_id == Some(source) && self.chat_widget.thread_id() == Some(source) {
            return false;
        }
        let crate::app_command::AppCommand::UserTurn {
            client_user_message_id,
            ..
        } = op
        else {
            return false;
        };
        if self.chat_widget.thread_id() == Some(source) {
            self.chat_widget
                .alab_recover_undispatched_steer(client_user_message_id);
        }
        if let Some(widget) = self
            .background_voice
            .as_mut()
            .filter(|widget| widget.thread_id() == Some(source))
        {
            widget.alab_recover_undispatched_steer(client_user_message_id);
        }
        if let Some(channel) = self.thread_event_channels.get(&source)
            && let Some(input) = channel.store.lock().await.input_state.as_mut()
        {
            input.alab_recover_undispatched_steer(client_user_message_id);
        }
        if let Some(input) = self.agents_overview.input_states.get_mut(&source) {
            input.alab_recover_undispatched_steer(client_user_message_id);
        }
        true
    }

    pub(super) async fn accept_live_misalignment_turn(
        &mut self,
        event: &ThreadBufferedEvent,
    ) -> bool {
        let ThreadBufferedEvent::Notification(notification) = event else {
            return true;
        };
        let Some((thread, turn)) = refusal_ids(notification) else {
            return true;
        };
        let Some(id) = self.active_thread_id else {
            return false;
        };
        if self.chat_widget.thread_id() != Some(id) || id.to_string() != thread {
            return false;
        }
        let Some(channel) = self.thread_event_channels.get(&id) else {
            return false;
        };
        let store = channel.store.lock().await;
        store.latest_turn_id.as_deref() == Some(turn)
            && store.active_turn_id().is_none_or(|active| active == turn)
            && self.chat_widget.bind_live_misalignment_turn(turn)
    }
}

impl TerminalHold {
    pub(super) fn advance(&mut self, latest: &str) {
        if self.ids.as_ref().is_some_and(|(_, turn)| turn != latest) {
            self.ids = None;
        }
    }

    pub(super) fn observe(&mut self, notification: &ServerNotification, latest: Option<&str>) {
        if let ServerNotification::TurnCompleted(n) = notification
            && Some(n.turn.id.as_str()) == latest
        {
            self.advance(&n.turn.id);
        }
        if let Some((thread, turn)) = refusal_ids(notification)
            && Some(turn) == latest
        {
            self.ids = Some((thread.to_owned(), turn.to_owned()));
        }
    }

    pub(super) fn restore(&self, snapshot: &mut ThreadEventSnapshot, latest: Option<&str>) {
        let Some((thread, turn)) = &self.ids else {
            return;
        };
        if Some(turn.as_str()) != latest
            || snapshot
                .turns
                .iter()
                .any(|t| &t.id == turn && failed_turn(t))
            || snapshot.events.iter().any(|e| {
                matches!(e, ThreadBufferedEvent::Notification(n)
                if refusal_ids(n) == Some((thread.as_str(), turn.as_str())))
            })
        {
            return;
        }
        snapshot
            .events
            .push(ThreadBufferedEvent::Notification(Box::new(
                ServerNotification::Error(ErrorNotification {
                    thread_id: thread.clone(),
                    turn_id: turn.clone(),
                    error: TurnError {
                        message: String::new(),
                        codex_error_info: Some(CodexErrorInfo::MisalignmentPolicyViolation),
                        additional_details: None,
                        misalignment: None,
                    },
                    will_retry: false,
                }),
            )));
    }
}
