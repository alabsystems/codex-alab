// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A delegate may complete while its parent is frozen or waiting to attach.
//! Completion is evidence about that exact turn, never permission to run one.
use super::ExecReviewSnapshot;
use codex_history::RolloutItem;
use codex_protocol::ThreadId;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_thread_store::StoredThreadHistory;

fn completed<'a>(
    history: &'a StoredThreadHistory,
    thread: ThreadId,
    turn: &str,
) -> Option<&'a TurnCompleteEvent> {
    if history.thread_id != thread {
        return None;
    }
    let mut current = None;
    let mut result = None;
    let mut seen = false;
    for item in &history.items {
        match item {
            RolloutItem::EventMsg(EventMsg::TurnStarted(start)) => {
                if start.turn_id == turn {
                    if result.is_some() {
                        return None;
                    }
                    seen = true;
                }
                current = Some(start.turn_id.as_str());
            }
            RolloutItem::EventMsg(EventMsg::TurnComplete(end)) if end.turn_id == turn => {
                if current != Some(turn) || result.is_some() {
                    return None;
                }
                result = Some(end);
            }
            RolloutItem::EventMsg(EventMsg::TurnAborted(abort))
                if seen && (abort.turn_id.is_none() || abort.turn_id.as_deref() == Some(turn)) =>
            {
                return None;
            }
            _ => {}
        }
    }
    if current == Some(turn) { result } else { None }
}

impl ExecReviewSnapshot {
    pub(crate) fn delegate_completed(&self, history: &StoredThreadHistory) -> bool {
        completed(history, self.delegate, &self.delegate_turn).is_some()
    }

    pub(crate) fn retains_completed_delegate(&self, history: &StoredThreadHistory) -> bool {
        let Some(actual) = completed(history, self.delegate, &self.delegate_turn) else {
            return false;
        };
        let mut pending = self.pending.iter().filter_map(|event| match &event.msg {
            EventMsg::TurnComplete(end) => Some(end),
            _ => None,
        });
        pending.next().is_some_and(|end| {
            end.turn_id == actual.turn_id && end.last_agent_message == actual.last_agent_message
        }) && pending.next().is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(value: serde_json::Value) -> RolloutItem {
        RolloutItem::EventMsg(serde_json::from_value(value).unwrap())
    }
    fn start(turn: &str) -> RolloutItem {
        event(json!({"type":"task_started","turn_id":turn,"model_context_window":null}))
    }
    fn end() -> RolloutItem {
        event(
            json!({"type":"task_complete","turn_id":"delegate","last_agent_message":"original result"}),
        )
    }
    #[test]
    fn completion_requires_the_original_last_turn_and_no_abort_or_restart() {
        let thread = ThreadId::new();
        let history = |items| StoredThreadHistory {
            thread_id: thread,
            items,
            revision: None,
        };
        assert!(completed(&history(vec![start("delegate"), end()]), thread, "delegate").is_some());
        for items in [
            vec![end()],
            vec![start("other"), end()],
            vec![start("delegate"), end(), end()],
            vec![start("delegate"), end(), start("delegate")],
            vec![start("delegate"), end(), start("other")],
            vec![
                start("delegate"),
                event(json!({"type":"turn_aborted","turn_id":"delegate","reason":"interrupted"})),
                end(),
            ],
        ] {
            assert!(completed(&history(items), thread, "delegate").is_none());
        }
        assert!(
            completed(
                &history(vec![start("delegate"), end()]),
                ThreadId::new(),
                "delegate"
            )
            .is_none()
        );
    }

    #[test]
    fn retained_completion_must_match_the_canonical_result_exactly_once() {
        let thread = ThreadId::new();
        let history = StoredThreadHistory {
            thread_id: thread,
            items: vec![start("delegate"), end()],
            revision: None,
        };
        let RolloutItem::EventMsg(completion) = end() else {
            unreachable!()
        };
        let event = codex_protocol::protocol::Event {
            id: "delegate".into(),
            msg: completion,
        };
        let mut snapshot = ExecReviewSnapshot {
            version: 1,
            parent: ThreadId::new(),
            turn: "parent".into(),
            delegate: thread,
            delegate_turn: "delegate".into(),
            buffered: None,
            pending: vec![event.clone()],
        };
        assert!(snapshot.retains_completed_delegate(&history));
        snapshot.pending.push(event);
        assert!(!snapshot.retains_completed_delegate(&history));
        snapshot.pending.pop();
        let EventMsg::TurnComplete(completion) = &mut snapshot.pending[0].msg else {
            unreachable!()
        };
        completion.last_agent_message = Some("replacement result".into());
        assert!(!snapshot.retains_completed_delegate(&history));
        snapshot.pending.clear();
        assert!(!snapshot.retains_completed_delegate(&history));
        assert!(snapshot.delegate_completed(&history));
    }
}
