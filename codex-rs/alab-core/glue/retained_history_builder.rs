// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Mode-aware entry to the SAME original public history reducer. Non-events
//! retain its normal index handling; events retain exact canonical mode.
use super::ThreadHistoryBuilder;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_rollout::RolloutItem;
impl ThreadHistoryBuilder {
    /// SAME canonical mode-aware reducer and original incremental change set.
    /// Side visibility filtering must not reset its IDs/ongoing-turn indexes.
    pub fn handle_retained_history_item_with_changes(
        &mut self,
        item: &RolloutItem,
        mode: ThreadHistoryMode,
    ) -> super::ThreadHistoryChangeSet {
        self.collect_changes(|builder| builder.handle_retained_history_item(item, mode))
    }

    pub fn handle_retained_history_item(&mut self, item: &RolloutItem, mode: ThreadHistoryMode) {
        if let RolloutItem::EventMsg(event) = item {
            self.current_rollout_index = self.next_rollout_index;
            self.next_rollout_index += 1;
            self.handle_event_for_history_mode(event, mode);
        } else {
            self.handle_rollout_item(item);
        }
    }
}
