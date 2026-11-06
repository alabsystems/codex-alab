// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Standalone compaction reports its own UI error and returns a clean task result.
//! Its terminal failure still has to reach extensions, so they cannot continue as
//! though the context reset succeeded. Upstream already notifies usage-limit errors.

use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_protocol::error::CodexErr;
use codex_protocol::protocol::CodexErrorInfo;

pub(super) async fn notify(sess: &Session, turn_context: &TurnContext, err: &CodexErr) {
    if !codex_alab_core::active() {
        return;
    }
    crate::responses_retry::alab_capacity_retry::remember(err, turn_context);
    let error = err.to_codex_protocol_error();
    if !matches!(error, CodexErrorInfo::UsageLimitExceeded) {
        sess.emit_turn_error_lifecycle(turn_context, error, err.details())
            .await;
    }
}
