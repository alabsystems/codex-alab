// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Original recall/cache/search traversal DATA, not a new history lookup owner.
use super::*;

crate::alab_editor_upgrade::native_state!(
    HistorySearchDirection,
    DirectionWire,
    #[serde(remote = "HistorySearchDirection", deny_unknown_fields)]
    enum DirectionWire {
        Older,
        Newer,
    }
);
crate::alab_editor_upgrade::native_state!(
    PendingHistorySearch,
    PendingWire,
    #[serde(remote = "PendingHistorySearch", deny_unknown_fields)]
    enum PendingWire {
        Entry {
            offset: usize,
            direction: HistorySearchDirection,
            boundary_if_exhausted: bool,
        },
        Batch {
            cursor: HistoryBatchCursor,
            boundary_if_exhausted: bool,
            read_failures: u8,
        },
    }
);
crate::alab_editor_upgrade::native_state!(
    UniqueHistoryMatch,
    MatchWire,
    #[serde(remote = "UniqueHistoryMatch", deny_unknown_fields)]
    struct MatchWire {
        offset: usize,
        entry: HistoryEntry,
    }
);
crate::alab_editor_upgrade::native_state!(
    HistorySearchState,
    SearchWire,
    #[serde(remote = "HistorySearchState", deny_unknown_fields)]
    struct SearchWire {
        query: String,
        query_lower: String,
        selected_offset: Option<usize>,
        unique_matches: Vec<UniqueHistoryMatch>,
        selected_match_index: Option<usize>,
        seen_texts: HashSet<String>,
        awaiting: Option<PendingHistorySearch>,
        next_older_cursor: Option<HistoryBatchCursor>,
        exhausted_older: bool,
        exhausted_newer: bool,
    }
);
crate::alab_editor_upgrade::native_state!(
    ChatComposerHistory,
    HistoryWire,
    #[serde(remote = "ChatComposerHistory", deny_unknown_fields)]
    struct HistoryWire {
        thread_id: Option<ThreadId>,
        persistent_log_id: Option<u64>,
        persistent_entry_count: usize,
        local_history: Vec<HistoryEntry>,
        replay_seeded_history: Vec<HistoryEntry>,
        fetched_history: HashMap<usize, Option<HistoryEntry>>,
        history_cursor: Option<isize>,
        pending_navigation_direction: Option<HistorySearchDirection>,
        last_history_text: Option<String>,
        search: Option<HistorySearchState>,
        at_mention_restore_enabled: bool,
    }
);
impl ChatComposerHistory {
    pub(crate) fn alab_validate_retained(&self) -> std::io::Result<()> {
        let invalid = || {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "retained history cursor differs from its original graph",
            )
        };
        let total = self
            .persistent_entry_count
            .checked_add(self.local_history.len())
            .ok_or_else(invalid)?;
        if isize::try_from(total).is_err()
            || self
                .fetched_history
                .keys()
                .any(|offset| *offset >= self.persistent_entry_count)
            || self
                .history_cursor
                .is_some_and(|offset| offset < 0 || offset as usize > total)
        {
            return Err(invalid());
        }
        if let Some(search) = &self.search
            && (search.query_lower != search.query.to_lowercase()
                || search
                    .selected_match_index
                    .is_some_and(|index| index >= search.unique_matches.len())
                || search.selected_offset.is_some_and(|offset| offset >= total)
                || search
                    .unique_matches
                    .iter()
                    .any(|item| item.offset >= total)
                || search
                    .next_older_cursor
                    .is_some_and(|cursor| cursor.end_offset() > self.persistent_entry_count)
                || search.awaiting.is_some_and(|pending| match pending {
                    PendingHistorySearch::Entry { offset, .. } => {
                        offset >= self.persistent_entry_count
                    }
                    PendingHistorySearch::Batch {
                        cursor,
                        read_failures,
                        ..
                    } => {
                        cursor.end_offset() > self.persistent_entry_count
                            || read_failures > MAX_BATCH_READ_RETRIES
                    }
                }))
        {
            return Err(invalid());
        }
        Ok(())
    }
}
