// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! An ordered observation of the actual rollout writer, without creating its file.

use super::IoError;
use super::RolloutCmd;
use super::RolloutItem;
use super::RolloutRecorder;
use super::RolloutWriterState;
use super::SessionMeta;
use tokio::sync::oneshot;

/// Actual creation metadata and the complete LOCAL pending sequence of a deferred writer.
/// SessionMeta.history_base can reference inherited history outside this sequence.
/// This snapshot does not claim that the local buffer is complete inherited display history.
/// Git information is not invented: the original writer only collects it at materialization.
#[derive(Clone, Debug)]
pub struct DeferredRolloutHistory {
    pub session_meta: SessionMeta,
    pub items: Vec<RolloutItem>,
}

impl RolloutRecorder {
    /// Observe all commands ordered before this one. This neither flushes nor persists.
    /// A stopped or failed writer is an error, never an empty-history observation.
    pub async fn retained_deferred_history(
        &self,
    ) -> std::io::Result<Option<DeferredRolloutHistory>> {
        let (ack, done) = oneshot::channel();
        self.tx
            .send(RolloutCmd::ReadRetainedDeferredHistory { ack })
            .await
            .map_err(|error| {
                self.writer_task.terminal_failure().unwrap_or_else(|| {
                    IoError::other(format!(
                        "retained deferred history writer unavailable: {error}"
                    ))
                })
            })?;
        done.await.map_err(|error| {
            self.writer_task.terminal_failure().unwrap_or_else(|| {
                IoError::other(format!(
                    "retained deferred history writer unavailable: {error}"
                ))
            })
        })?
    }
}

pub(super) fn snapshot(
    state: &RolloutWriterState,
) -> std::io::Result<Option<DeferredRolloutHistory>> {
    if let Some(error) = &state.last_logged_error {
        return Err(IoError::other(format!(
            "rollout writer has an unresolved failure: {error}"
        )));
    }
    if !state.is_deferred() {
        return Ok(None);
    }
    let session_meta = state
        .meta
        .clone()
        .ok_or_else(|| IoError::other("deferred rollout writer has no creation metadata"))?;
    Ok(Some(DeferredRolloutHistory {
        session_meta,
        items: state.pending_items.clone(),
    }))
}

#[cfg(test)]
#[path = "retained_deferred_rollout_tests.rs"]
mod tests;
