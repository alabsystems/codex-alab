// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnItemsView;
use codex_protocol::models::MessagePhase;

#[derive(Debug, Default)]
pub(super) struct FinalItems {
    // Identities outlive eviction. Only recovered summaries suppress a subsequent standalone item.
    turns: HashMap<String, HashMap<String, FinalSource>>,
}

#[derive(Debug, PartialEq)]
enum FinalSource {
    Delivered,
    Recovered,
}

fn final_id(item: &ThreadItemPayload) -> Option<&str> {
    match item {
        ThreadItemPayload::AgentMessage {
            id,
            text,
            phase: Some(MessagePhase::FinalAnswer) | None,
            ..
        } if !text.trim().is_empty() => Some(id),
        _ => None,
    }
}

impl FinalItems {
    fn source(&self, turn: &str, item: &str) -> Option<&FinalSource> {
        self.turns.get(turn)?.get(item)
    }

    fn remember(&mut self, turn: &str, item: &str, source: FinalSource) {
        self.turns
            .entry(turn.to_owned())
            .or_default()
            .entry(item.to_owned())
            .or_insert(source);
    }
}

/// Only turn lifecycle events and terminal policy findings can change the review state.
fn lifecycle(notification: &ServerNotification) -> Option<(&str, bool)> {
    match notification {
        ServerNotification::TurnStarted(n) => Some((&n.turn.id, true)),
        ServerNotification::TurnCompleted(n) => Some((&n.turn.id, false)),
        ServerNotification::Error(n)
            if !n.will_retry
                && n.error.codex_error_info
                    == Some(AppServerCodexErrorInfo::MisalignmentPolicyViolation) =>
        {
            Some((&n.turn_id, false))
        }
        _ => None,
    }
}

impl ThreadEventStore {
    fn retired_turn(&self, id: &str, starting: bool) -> bool {
        if self.active_turn_id() == Some(id)
            || (!starting && self.latest_turn_id.as_deref() == Some(id))
        {
            return false;
        }
        self.alab_retired_turns.contains(id)
    }

    fn recovered_item(&self, notification: &ServerNotification) -> bool {
        matches!(notification, ServerNotification::ItemCompleted(n) if final_id(&n.item.payload)
            .is_some_and(|id| self.alab_final_items.source(&n.turn_id, id) == Some(&FinalSource::Recovered)))
    }

    fn recover_retired_final(&mut self, turn: &Turn) -> Option<ThreadItem> {
        if turn.status != TurnStatus::Completed {
            return None;
        }
        let mut items = turn.items.clone();
        super::thread_events::hide_private_items_after_voice_handoff(
            &mut items,
            self.is_realtime_delegated_turn(&turn.id),
        );
        let item = items
            .into_iter()
            .rev()
            .find(|item| final_id(&item.payload).is_some())?;
        let id = item.id();
        if self.alab_final_items.source(&turn.id, id).is_some() {
            return None;
        }
        if let Some(history) = self.turns.iter_mut().find(|known| known.id == turn.id) {
            if history.items.iter().any(|known| known.id() == id) {
                return None;
            }
            history.items.push(item.clone());
            if history.items_view == TurnItemsView::NotLoaded {
                history.items_view = TurnItemsView::Summary;
            }
        } else {
            // An evicted live start may have no saved row. Keep only the final item and put
            // the historical turn before any newer known time or current saved lifecycle.
            let index = self
                .turns
                .iter()
                .position(|known| {
                    Some(known.id.as_str()) == self.latest_turn_id.as_deref()
                        || known
                            .started_at
                            .zip(turn.started_at)
                            .is_some_and(|(known, old)| known > old)
                })
                .unwrap_or(self.turns.len());
            self.turns.insert(
                index,
                Turn {
                    id: turn.id.clone(),
                    root_turn_id: turn.root_turn_id.clone(),
                    items: vec![item.clone()],
                    items_view: TurnItemsView::Summary,
                    status: TurnStatus::Completed,
                    error: None,
                    started_at: turn.started_at,
                    completed_at: turn.completed_at,
                    duration_ms: turn.duration_ms,
                },
            );
        }
        self.alab_final_items
            .remember(&turn.id, id, FinalSource::Recovered);
        self.replay_may_have_output = true;
        Some(item)
    }

    pub(super) fn alab_accept_lifecycle(&mut self, notification: &ServerNotification) -> bool {
        if let ServerNotification::ItemCompleted(n) = notification
            && let Some(id) = final_id(&n.item.payload)
        {
            if self.recovered_item(notification) {
                return false;
            }
            self.alab_final_items
                .remember(&n.turn_id, id, FinalSource::Delivered);
        }
        let Some((id, starting)) = lifecycle(notification) else {
            return true;
        };
        if self.retired_turn(id, starting) {
            if let ServerNotification::TurnCompleted(n) = notification {
                self.recover_retired_final(&n.turn);
            }
            return false;
        }
        if let ServerNotification::TurnCompleted(n) = notification
            && n.turn.status == TurnStatus::Completed
            && let Some((item, _)) = crate::chatwidget::final_agent_message(&n.turn, false)
            && let Some(item_id) = final_id(item)
        {
            self.alab_final_items
                .remember(id, item_id, FinalSource::Delivered);
        }
        if !starting {
            self.alab_retired_turns.insert(id.to_owned());
        }
        true
    }

    pub(super) fn alab_note_latest_turn(&mut self, id: &str) {
        self.terminal_hold.advance(id);
        if self.latest_turn_id.as_deref() != Some(id)
            && let Some(previous) = self.latest_turn_id.replace(id.to_owned())
        {
            self.alab_retired_turns.insert(previous);
        }
    }

    pub(super) fn alab_refresh_turn_history(&mut self, turns: &[Turn]) {
        // A later history page may omit an old turn. Keep only its identity for late delivery.
        for turn in turns {
            for item in turn.item_payloads().filter_map(final_id) {
                if self.alab_final_items.source(&turn.id, item).is_none() {
                    self.alab_final_items
                        .remember(&turn.id, item, FinalSource::Delivered);
                }
            }
            if turn.status != TurnStatus::InProgress && !self.alab_retired_turns.contains(&turn.id)
            {
                self.alab_retired_turns.insert(turn.id.clone());
            }
        }
    }
}

impl App {
    pub(super) async fn alab_stale_lifecycle(
        &mut self,
        thread: ThreadId,
        notification: &ServerNotification,
        source: Option<crate::backend_id::BackendId>,
    ) -> bool {
        let lifecycle = lifecycle(notification);
        if lifecycle.is_none()
            && !matches!(notification, ServerNotification::ItemCompleted(n)
            if final_id(&n.item.payload).is_some())
        {
            return false;
        }
        let Some(channel) = self.thread_event_channels.get(&thread) else {
            return false;
        };
        let mut store = channel.store.lock().await;
        let Some((id, starting)) = lifecycle else {
            return store.recovered_item(notification);
        };
        if !store.retired_turn(id, starting) {
            return false;
        }
        if self.abandoned_side_threads.contains(&thread)
            || source
                .zip(channel.backend_id)
                .is_some_and(|(source, owner)| source != owner)
        {
            return true;
        }
        let ServerNotification::TurnCompleted(n) = notification else {
            return true;
        };
        let Some(item) = store.recover_retired_final(&n.turn) else {
            return true;
        };
        let cwd = store
            .session
            .as_ref()
            .map_or(&self.config.cwd, |s| &s.cwd)
            .clone();
        drop(store);
        let displayed = self.current_displayed_thread_id() == Some(thread);
        if displayed || self.side_companions.contains_key(&thread) {
            let cells = crate::thread_transcript::thread_items_to_transcript_cells(
                Some(thread),
                &cwd,
                [item.payload.clone()],
                crate::thread_transcript::RawReasoningVisibility::Hidden,
                Some(&self.config),
            );
            #[cfg(unix)]
            self.alab_record_side_history_group(
                thread,
                &[crate::app_server_session::history_page::HistoryPageItem {
                    turn_id: id.to_owned(),
                    payload: item.payload,
                }],
                &cells,
            );
            if displayed {
                for cell in cells {
                    if let Some(Overlay::Transcript(overlay)) = &mut self.overlay {
                        overlay.insert_cell(Arc::clone(&cell));
                    }
                    self.native_history.defer(&cell);
                    self.transcript_cells.push(cell);
                }
            } else if let Some(pane) = self.side_companions.get_mut(&thread) {
                let end = pane.cells.len();
                pane.splice_older_cells(end..end, cells);
            }
            self.chat_widget.request_redraw();
        }
        true
    }
}
