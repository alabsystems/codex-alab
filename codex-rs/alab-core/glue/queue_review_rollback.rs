// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use crate::context_manager::is_user_turn_boundary;
use codex_history::RolloutItem;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::EventMsg;
use std::collections::HashSet;

struct Segment<'a> {
    id: Option<&'a str>,
    counts_as_user_turn: bool,
}

#[derive(Default)]
struct Segments<'a> {
    turns: Vec<Segment<'a>>,
    seen: HashSet<&'a str>,
    discarded: HashSet<&'a str>,
    active: Option<usize>,
}

impl<'a> Segments<'a> {
    fn push(&mut self, id: Option<&'a str>) -> usize {
        let index = self.turns.len();
        self.turns.push(Segment {
            id,
            counts_as_user_turn: false,
        });
        self.active = Some(index);
        index
    }

    fn start(&mut self, id: &'a str) {
        if self.seen.insert(id) {
            self.push(Some(id));
        }
    }

    // A targeted record can name a sparse segment whose start was never persisted.
    // A late record for an already known ID cannot displace a newer segment.
    fn targeted(&mut self, id: &'a str) -> Option<usize> {
        if let Some(index) = self.active {
            if let Some(active_id) = self.turns[index].id {
                return (active_id == id).then_some(index);
            }
            if self.seen.insert(id) {
                self.turns[index].id = Some(id);
                return Some(index);
            }
            return None;
        }
        self.seen.insert(id).then(|| self.push(Some(id)))
    }

    fn accept(&mut self, id: Option<&'a str>) {
        let index = match id {
            Some(id) => {
                let Some(index) = self.targeted(id) else {
                    return;
                };
                index
            }
            None => self.active.unwrap_or_else(|| self.push(None)),
        };
        self.turns[index].counts_as_user_turn = true;
    }

    fn finish(&mut self, id: Option<&'a str>) {
        if id.is_none_or(|id| self.targeted(id).is_some()) {
            self.active = None;
        }
    }

    fn rollback(&mut self, mut count: u32) {
        if count == 0 {
            return;
        }
        self.active = None;
        while count > 0 {
            let Some(segment) = self.turns.pop() else {
                break;
            };
            self.discarded.extend(segment.id);
            count -= u32::from(segment.counts_as_user_turn);
        }
    }
}

/// The same unit as cold-resume turn metadata: while a rollback is pending,
/// discard intervening context-only segments and charge one per instruction turn.
/// Consult all supplied items because a review decision can predate a compaction.
pub(super) fn discarded_turns(items: &[RolloutItem]) -> HashSet<&str> {
    let Some(last_rollback) = items.iter().rposition(|item| {
        matches!(item, RolloutItem::EventMsg(EventMsg::ThreadRolledBack(rollback)) if rollback.num_turns > 0)
    }) else {
        return HashSet::new();
    };
    let mut segments = Segments::default();
    for item in &items[..=last_rollback] {
        match item {
            RolloutItem::EventMsg(EventMsg::TurnStarted(start)) => segments.start(&start.turn_id),
            RolloutItem::EventMsg(EventMsg::TurnComplete(done)) => {
                segments.finish(Some(&done.turn_id))
            }
            RolloutItem::EventMsg(EventMsg::TurnAborted(abort)) => {
                segments.finish(abort.turn_id.as_deref())
            }
            RolloutItem::TurnContext(context) => {
                if let Some(id) = context.turn_id.as_deref() {
                    segments.targeted(id);
                }
            }
            RolloutItem::ResponseItem(response) if is_user_turn_boundary(&response.item) => {
                segments.accept(None);
            }
            RolloutItem::EventMsg(EventMsg::UserMessage(_))
            | RolloutItem::InterAgentCommunication(_) => {
                segments.accept(None);
            }
            // Paginated input can also arrive without its raw response in partial history.
            RolloutItem::EventMsg(EventMsg::ItemCompleted(done))
                if matches!(&done.item, TurnItem::UserMessage(_)) =>
            {
                segments.accept(Some(&done.turn_id));
            }
            RolloutItem::EventMsg(EventMsg::ThreadRolledBack(rollback)) => {
                segments.rollback(rollback.num_turns);
            }
            _ => {}
        }
    }
    segments.discarded
}
