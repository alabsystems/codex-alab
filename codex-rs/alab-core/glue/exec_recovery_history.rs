// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Canonical latest-turn evidence, independent of model-context compaction and
//! the rebuildable display index. Compaction must not erase recovery identity.

use super::LocalThreadStore;
use super::history_revision;
use super::rollout_lineage::RolloutLineage;
use super::thread_rollout_resolver;
use crate::LoadThreadHistoryParams;
use crate::StoredThreadHistory;
use crate::ThreadStoreError;
use crate::ThreadStoreResult;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_rollout::ReverseJsonlScanner;
use codex_rollout::RolloutItem;
use codex_rollout::ScanOutcome;
use std::io;

#[path = "exec_review_history.rs"]
mod review;

pub(super) async fn load(
    store: &LocalThreadStore,
    params: LoadThreadHistoryParams,
) -> ThreadStoreResult<StoredThreadHistory> {
    let resolved = if params.include_archived {
        thread_rollout_resolver::resolve_current_including_archived(store, params.thread_id).await?
    } else {
        thread_rollout_resolver::resolve_current(store, params.thread_id).await?
    };
    let path = resolved
        .ok_or(ThreadStoreError::ThreadNotFound {
            thread_id: params.thread_id,
        })?
        .path;
    let metadata = codex_rollout::read_session_meta_line(&path)
        .await
        .map_err(|_| invalid("recovery metadata is unreadable"))?;
    if metadata.meta.id != params.thread_id {
        return Err(invalid("recovery metadata names another thread"));
    }
    if metadata.meta.history_mode == ThreadHistoryMode::Legacy {
        return store.load_history(params).await;
    }
    let before = history_revision::read(&path)
        .await
        .ok_or_else(|| invalid("recovery history has no stable revision"))?;
    let lineage = store
        .resolve_rollout_lineage(params.thread_id, Some(path.clone()))
        .await?;
    // Dropping the caller also stops compressed decoding and the blocking scan.
    let (stop, keep_running) = tokio::sync::oneshot::channel::<()>();
    let items = tokio::task::spawn_blocking(move || scan(&lineage, || stop.is_closed()))
        .await
        .map_err(|_| invalid("recovery history scan did not finish"))?
        .map_err(|_| invalid("recovery history is incomplete or malformed"))?;
    drop(keep_running);
    if history_revision::read(&path).await.as_ref() != Some(&before) {
        return Err(invalid("recovery history changed during its read"));
    }
    Ok(StoredThreadHistory {
        thread_id: params.thread_id,
        items,
        revision: Some(before),
    })
}

fn invalid(message: &str) -> ThreadStoreError {
    ThreadStoreError::Internal {
        message: message.to_owned(),
    }
}

fn scan(lineage: &RolloutLineage, is_cancelled: impl Fn() -> bool) -> io::Result<Vec<RolloutItem>> {
    let mut reversed = Vec::new();
    let mut latest_turn = None;
    let mut start_boundary = None;
    let mut review_owner: Option<String> = None;
    let mut review_entry_seen = false;
    'segments: for segment in lineage.segments().iter().rev() {
        let reader =
            codex_rollout::open_rollout_seekable_reader(&segment.rollout_path, &is_cancelled)?;
        let mut scanner = match segment.end {
            Some(end) => ReverseJsonlScanner::new_at(reader, end.end_byte_offset)?,
            None => ReverseJsonlScanner::new(reader)?,
        };
        while let Some(outcome) = scanner.scan_next_rollout_line()? {
            if is_cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "recovery read cancelled",
                ));
            }
            let ScanOutcome::Parsed(line) = outcome else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid recovery record",
                ));
            };
            if matches!(line.item, RolloutItem::SessionMeta(_)) {
                break;
            }
            let ordinal = line.ordinal.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "recovery record lacks its ordinal",
                )
            })?;
            if ordinal < segment.start_ordinal() {
                break;
            }
            if segment.end_ordinal().is_some_and(|end| ordinal >= end) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "recovery record exceeds its lineage",
                ));
            }
            match line.item {
                RolloutItem::EventMsg(event) => {
                    let terminal = match &event {
                        EventMsg::TurnComplete(event) => Some(event.turn_id.as_str()),
                        EventMsg::TurnAborted(event) => event.turn_id.as_deref(),
                        _ => None,
                    };
                    if start_boundary.is_some()
                        && terminal.is_some_and(|turn| latest_turn.as_deref() != Some(turn))
                    {
                        break 'segments;
                    }
                    if let Some(turn) = review::exited(&event) {
                        if start_boundary.is_some() && latest_turn.as_deref() != Some(turn) {
                            break 'segments;
                        }
                        review_owner = Some(turn.to_owned());
                        latest_turn = Some(turn.to_owned());
                    }
                    if let Some(turn) = review::entered(&event) {
                        review_entry_seen = true;
                        review_owner = Some(turn.to_owned());
                        latest_turn = Some(turn.to_owned());
                        reversed.push(RolloutItem::EventMsg(event));
                        start_boundary = Some(reversed.len());
                        continue;
                    }
                    let started = matches!(event, EventMsg::TurnStarted(_));
                    if let EventMsg::TurnStarted(start) = &event {
                        if review_owner
                            .as_ref()
                            .is_some_and(|turn| turn != &start.turn_id)
                        {
                            if review_entry_seen {
                                break 'segments;
                            }
                            reversed.push(RolloutItem::EventMsg(event));
                            continue;
                        }
                        if latest_turn
                            .as_ref()
                            .is_some_and(|turn| turn != &start.turn_id)
                        {
                            if review_owner.is_some() {
                                break 'segments;
                            }
                            // A recovered review can have a parent restart after
                            // its original forwarded delegate start. Its entry
                            // item below establishes which start owns the turn.
                            // Without that entry, the existing boundary still
                            // excludes this earlier, different turn.
                            reversed.push(RolloutItem::EventMsg(event));
                            continue;
                        }
                        latest_turn.get_or_insert_with(|| start.turn_id.clone());
                    }
                    reversed.push(RolloutItem::EventMsg(event));
                    if started {
                        // Recovery starts another execution of the same logical turn
                        // without replaying its user input. Retain its earlier starts
                        // and input, including any intervening terminal that revokes it.
                        start_boundary = Some(reversed.len());
                    }
                }
                RolloutItem::ResponseItem(item)
                    if matches!(
                        item.item,
                        codex_protocol::models::ResponseItem::AgentMessage { .. }
                    ) =>
                {
                    // Agent-started regular turns have no UserMessage event.
                    // Preserve their exact accepted input and turn attribution.
                    reversed.push(RolloutItem::ResponseItem(item));
                }
                _ => {}
            }
        }
    }
    if let Some(boundary) = start_boundary {
        reversed.truncate(boundary);
    }
    reversed.reverse();
    Ok(reversed)
}

#[cfg(test)]
#[path = "exec_recovery_history_tests.rs"]
mod tests;
