// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Listener-owned unfinished display items, separate from persisted history.
//! Canonical events supply every identity and lifecycle time. No SDK permission
//! or provider continuation is reconstructed from this display data.
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadHistoryTurnMetadata;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadItemPayload;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::ThreadId;
use codex_protocol::items::TurnItem as CoreItem;
use codex_protocol::protocol::EventMsg;
use std::collections::BTreeMap;

// Match the existing joined-runtime data budget; overflow refuses a live full
// snapshot instead of silently omitting text or interrupting its producer.
const MAX_BYTES: usize = 256 * 1024 * 1024;
#[derive(Default)]
pub(crate) struct State {
    turn: Option<String>,
    next: u64,
    live: BTreeMap<String, Live>,
    bytes: usize,
    unavailable: bool,
}
struct Live {
    item: ThreadItem,
    bytes: usize,
    rank: u64,
}
fn escaped_bytes(text: &str) -> usize {
    text.bytes().fold(0usize, |sum, byte| {
        sum.saturating_add(match byte {
            b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 8 | 12 => 2,
            0..=31 => 6,
            _ => 1,
        })
    })
}
fn encoded_len(value: &impl serde::Serialize, limit: usize) -> Option<usize> {
    struct Counter {
        bytes: usize,
        limit: usize,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let next = self
                .bytes
                .checked_add(bytes.len())
                .filter(|next| *next <= self.limit)
                .ok_or_else(|| std::io::Error::other("live snapshot budget"))?;
            self.bytes = next;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter { bytes: 0, limit };
    serde_json::to_writer(&mut counter, value).ok()?;
    Some(counter.bytes)
}
impl State {
    fn insert(&mut self, id: String, item: ThreadItem, limit: usize) {
        if self.live.contains_key(&id) {
            return;
        }
        let Some(encoded) = encoded_len(&item, limit.saturating_sub(self.bytes)) else {
            self.live.clear();
            self.bytes = 0;
            self.unavailable = true;
            return;
        };
        let encoded = encoded.saturating_add(id.len()).saturating_add(64);
        let Some(next) = self.next.checked_add(1) else {
            self.unavailable = true;
            return;
        };
        if !self.charge(encoded, limit) {
            return;
        }
        self.live.insert(
            id,
            Live {
                item,
                bytes: encoded,
                rank: self.next,
            },
        );
        self.next = next;
    }
    fn retire(&mut self, id: &str) {
        if let Some(live) = self.live.remove(id) {
            self.bytes -= live.bytes;
        }
    }
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
    pub(crate) fn observe(&mut self, turn: &str, event: &EventMsg) {
        if codex_alab_core::active() {
            self.track(turn, event, MAX_BYTES);
        } else {
            self.clear();
        }
    }
    fn charge(&mut self, bytes: usize, limit: usize) -> bool {
        if self
            .bytes
            .checked_add(bytes)
            .is_none_or(|next| next > limit)
        {
            self.live.clear();
            self.bytes = 0;
            self.unavailable = true;
            return false;
        }
        self.bytes += bytes;
        true
    }
    fn track(&mut self, turn: &str, event: &EventMsg, limit: usize) {
        if matches!(event, EventMsg::TurnStarted(_)) {
            self.clear();
            self.turn = Some(turn.to_owned());
        }
        if self.turn.as_deref() != Some(turn) {
            return;
        }
        if matches!(event, EventMsg::TurnComplete(_) | EventMsg::TurnAborted(_)) {
            self.clear();
            return;
        }
        if self.unavailable {
            return;
        }
        match event {
            EventMsg::ItemStarted(started) if started.turn_id == turn => {
                let id = started.item.id();
                if matches!(started.item, CoreItem::AgentMessage(_)) && !self.live.contains_key(&id)
                {
                    let item = ThreadItem {
                        payload: ThreadItemPayload::from(started.item.clone()),
                        started_at_ms: Some(started.started_at_ms),
                        completed_at_ms: None,
                    };
                    self.insert(id, item, limit);
                }
            }
            EventMsg::ExecCommandBegin(begin) if begin.turn_id == turn => {
                self.insert(
                    begin.call_id.clone(),
                    ThreadItem {
                        payload: codex_app_server_protocol::build_command_execution_begin_item(
                            begin,
                        ),
                        started_at_ms: Some(begin.started_at_ms),
                        completed_at_ms: None,
                    },
                    limit,
                );
            }
            EventMsg::ExecCommandOutputDelta(delta) => {
                if !self.live.get(&delta.call_id).is_some_and(|item| {
                    matches!(
                        item.item.payload,
                        ThreadItemPayload::CommandExecution { .. }
                    )
                }) {
                    return;
                }
                let text = String::from_utf8_lossy(&delta.chunk);
                let bytes = escaped_bytes(&text);
                if !self.charge(bytes, limit) {
                    return;
                }
                if let Some(live) = self.live.get_mut(&delta.call_id) {
                    live.bytes += bytes;
                    if let ThreadItemPayload::CommandExecution {
                        aggregated_output, ..
                    } = &mut live.item.payload
                    {
                        aggregated_output
                            .get_or_insert_with(String::new)
                            .push_str(&text);
                    }
                }
            }
            EventMsg::ExecCommandEnd(end) if end.turn_id == turn => self.retire(&end.call_id),
            EventMsg::AgentMessageContentDelta(delta) if delta.turn_id == turn => {
                if !self.live.contains_key(&delta.item_id) {
                    return;
                }
                let bytes = escaped_bytes(&delta.delta);
                if !self.charge(bytes, limit) {
                    return;
                }
                if let Some(live) = self.live.get_mut(&delta.item_id) {
                    live.bytes += bytes;
                    if let ThreadItemPayload::AgentMessage { text, .. } = &mut live.item.payload {
                        text.push_str(&delta.delta);
                    }
                }
            }
            EventMsg::ItemCompleted(completed) if completed.turn_id == turn => {
                self.retire(&completed.item.id());
            }
            _ => {}
        }
    }
    fn snapshot(&self, turn: &str) -> Result<Vec<ThreadItem>, JSONRPCErrorError> {
        if self.turn.as_deref() != Some(turn) {
            return Ok(Vec::new());
        }
        if self.unavailable {
            return Err(JSONRPCErrorError {
                code: -32603,
                message: "Live assistant snapshot exceeds its supported data budget".into(),
                data: None,
            });
        }
        let mut items = self.live.values().collect::<Vec<_>>();
        items.sort_by_key(|item| item.rank);
        Ok(items.into_iter().map(|live| live.item.clone()).collect())
    }
    pub(crate) fn for_resume(
        &self,
        thread: ThreadId,
        active: Option<&ThreadHistoryTurnMetadata>,
        view: Option<&TurnItemsView>,
    ) -> Result<Vec<ItemStartedNotification>, JSONRPCErrorError> {
        let Some(active) = active.filter(|turn| turn.status == TurnStatus::InProgress) else {
            return Ok(Vec::new());
        };
        if view != Some(&TurnItemsView::Full) {
            return Ok(Vec::new());
        }
        self.snapshot(&active.turn_id)?
            .into_iter()
            .map(|item| {
                let Some(started_at_ms) = item.started_at_ms else {
                    return Err(JSONRPCErrorError {
                        code: -32603,
                        message: "Live item lost its canonical start time".into(),
                        data: None,
                    });
                };
                Ok(ItemStartedNotification {
                    item,
                    thread_id: thread.to_string(),
                    turn_id: active.turn_id.clone(),
                    started_at_ms,
                })
            })
            .collect()
    }
}

pub(crate) async fn replay(
    outgoing: &crate::outgoing_message::OutgoingMessageSender,
    connection: crate::outgoing_message::ConnectionId,
    items: Vec<ItemStartedNotification>,
) {
    for item in items {
        outgoing
            .send_server_notification_to_connections(
                &[connection],
                ServerNotification::ItemStarted(item),
            )
            .await;
    }
}

#[cfg(test)]
#[path = "live_agent_replay_tests.rs"]
mod tests;
