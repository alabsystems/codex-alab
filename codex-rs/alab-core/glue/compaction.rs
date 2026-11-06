// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compiled inside core's session module. Native item identities give the live
//! extension the same compaction proof that the rollout carries across a restart.

use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_alab_core::compaction::Activity;
use codex_analytics::CompactionPhase;
use codex_protocol::error::CodexErr;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::EventMsg;

pub(crate) fn item(turn: &TurnContext, item: &TurnItem, started: bool) {
    if !codex_alab_core::active() {
        return;
    }
    if let TurnItem::ContextCompaction(item) = item {
        let activity = turn.extension_data.get_or_init(Activity::default);
        if started {
            activity.started(item.id.clone());
        } else {
            activity.completed(&item.id);
        }
    }
}

/// Upstream preserves an already completed answer on post-turn compaction failure.
/// Keep that answer, but do not let Cruise replay a policy-denied context reset.
pub(crate) async fn result(
    sess: &Session,
    turn: &TurnContext,
    phase: CompactionPhase,
    error: Option<&CodexErr>,
) {
    if codex_alab_core::active()
        && matches!(phase, CompactionPhase::PostTurn)
        && let Some(error) = error
        && error.to_codex_protocol_error() == CodexErrorInfo::CyberPolicy
    {
        sess.send_event(
            turn,
            EventMsg::Error(
                error.to_error_event(Some("Error running post-turn compact task".into())),
            ),
        )
        .await;
        sess.emit_turn_error_lifecycle(turn, CodexErrorInfo::CyberPolicy, error.details())
            .await;
    }
}
