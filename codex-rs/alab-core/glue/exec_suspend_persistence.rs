// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Preserve Core's writer barrier while allowing a marked ephemeral exec to
//! retain its canonical evidence in memory. Missing or failed durable storage
//! is never converted into an ephemeral handoff.

use crate::session::session::Session;
use codex_history::RolloutItem;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::protocol::EventMsg;
use codex_thread_store::LiveThread;

#[path = "exec_review_history.rs"]
mod review;

pub(crate) enum SuspensionPersistence<'a> {
    Durable(&'a LiveThread),
    Ephemeral(&'a Session),
}

pub(crate) async fn prepare(session: &Session) -> CodexResult<SuspensionPersistence<'_>> {
    if let Some(writer) = session.live_thread() {
        writer.flush().await.map_err(|error| {
            CodexErr::Fatal(format!("flush before root turn suspension failed: {error}"))
        })?;
        return Ok(SuspensionPersistence::Durable(writer));
    }
    let history = super::history(session).map_err(|error| CodexErr::Fatal(error.to_string()))?;
    if open_turn(&history.items).is_none() {
        return Err(CodexErr::Fatal(
            "ephemeral suspension has no unfinished original turn".into(),
        ));
    }
    Ok(SuspensionPersistence::Ephemeral(session))
}

impl SuspensionPersistence<'_> {
    pub(crate) async fn finish(self, turn: &str) -> CodexResult<()> {
        match self {
            Self::Durable(writer) => {
                writer.flush().await.map_err(|error| {
                    CodexErr::Fatal(format!("flush after root turn suspension failed: {error}"))
                })?;
                writer.shutdown().await.map_err(|error| {
                    CodexErr::Fatal(format!("close suspended root turn writer failed: {error}"))
                })
            }
            Self::Ephemeral(session) => {
                let history =
                    super::history(session).map_err(|error| CodexErr::Fatal(error.to_string()))?;
                if open_turn(&history.items) != Some(turn) {
                    return Err(CodexErr::Fatal(
                        "ephemeral suspension no longer names the unfinished original turn".into(),
                    ));
                }
                Ok(())
            }
        }
    }
}

pub(crate) fn open_turn(items: &[RolloutItem]) -> Option<&str> {
    let mut current = None;
    let mut review_owner = None;
    for item in items {
        if let RolloutItem::EventMsg(event) = item {
            if let Some(turn) = review::entered(event) {
                current = Some(turn);
                review_owner = Some(turn);
                continue;
            }
            if review::exited(event).is_some_and(|turn| current == Some(turn)) {
                current = None;
                review_owner = None;
                continue;
            }
        }
        match item {
            RolloutItem::EventMsg(EventMsg::TurnStarted(event)) if review_owner.is_none() => {
                current = Some(event.turn_id.as_str());
            }
            RolloutItem::EventMsg(EventMsg::TurnComplete(event))
                if current == Some(event.turn_id.as_str()) =>
            {
                current = None;
                review_owner = None;
            }
            RolloutItem::EventMsg(EventMsg::TurnAborted(event))
                if event.turn_id.is_none() || current == event.turn_id.as_deref() =>
            {
                current = None;
                review_owner = None;
            }
            _ => {}
        }
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::ThreadId;
    use serde_json::json;

    fn event(value: serde_json::Value) -> RolloutItem {
        RolloutItem::EventMsg(serde_json::from_value(value).unwrap())
    }
    fn entered() -> RolloutItem {
        event(
            json!({"type":"item_completed","thread_id":ThreadId::new(),"turn_id":"parent", "item":{"type":"EnteredReviewMode","id":"entry","target":{"type":"custom","instructions":"review"},"user_facing_hint":"Review"}}),
        )
    }
    fn started(turn: &str) -> RolloutItem {
        event(json!({"type":"task_started","turn_id":turn,"model_context_window":null}))
    }
    #[test]
    fn review_parent_owns_forwarded_delegate_events_until_its_terminal() {
        let mut items = vec![entered(), started("delegate")];
        assert_eq!(open_turn(&items), Some("parent"));
        items.push(event(
            json!({"type":"task_complete","turn_id":"delegate","last_agent_message":null}),
        ));
        assert_eq!(open_turn(&items), Some("parent"));
        items.push(event(
            json!({"type":"task_complete","turn_id":"parent","last_agent_message":null}),
        ));
        assert_eq!(open_turn(&items), None);
        items.push(started("later"));
        assert_eq!(open_turn(&items), Some("later"));
    }
    #[test]
    fn review_exit_and_parent_abort_revoke_recovery() {
        for terminal in [
            event(
                json!({"type":"item_completed","thread_id":ThreadId::new(),"turn_id":"parent","item":{"type":"ExitedReviewMode","id":"exit","review_output":null}}),
            ),
            event(json!({"type":"turn_aborted","turn_id":"parent","reason":"interrupted"})),
        ] {
            assert_eq!(open_turn(&[entered(), started("delegate"), terminal]), None);
        }
    }
}
