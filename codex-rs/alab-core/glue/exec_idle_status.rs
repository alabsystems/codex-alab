// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Restore an idle member's observed task state without replaying its completion.

use crate::agent::agent_status_from_event;
use crate::session::session::Session;
use codex_history::RolloutItem;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::EventMsg;

pub(crate) async fn restore(session: &Session, history: &[RolloutItem]) -> Result<(), String> {
    let active = session.active_turn.lock().await;
    if active.is_some() {
        return Err("idle status restoration found an active turn".into());
    }
    let status = history
        .iter()
        .rev()
        .find_map(|item| match item {
            // Source shutdown ends its execution, not the logical child's task.
            RolloutItem::EventMsg(EventMsg::ShutdownComplete) => None,
            RolloutItem::EventMsg(event) => agent_status_from_event(event),
            _ => None,
        })
        .unwrap_or(AgentStatus::PendingInit);
    if matches!(status, AgentStatus::Running | AgentStatus::Shutdown) {
        return Err("idle status restoration found an unfinished task".into());
    }
    session.agent_status.send_replace(status);
    drop(active);
    Ok(())
}
