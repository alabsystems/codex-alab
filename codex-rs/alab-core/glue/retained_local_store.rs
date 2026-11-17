// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! SAME actual create/resume acquisition token, held under original lifecycle
//! serialization through the actual writer command/read. No path/ID fallback.
use super::LocalThreadStore;
use crate::RetainedLocalHistory;
use crate::RetainedWriterIdentity;
use crate::ThreadStoreError;
use crate::ThreadStoreResult;
use codex_protocol::ThreadId;

pub(super) async fn load(
    store: &LocalThreadStore,
    thread: ThreadId,
    writer: RetainedWriterIdentity,
    max_bytes: usize,
) -> ThreadStoreResult<RetainedLocalHistory> {
    let _lifecycle = store.live_writer_locks.lock(thread).await;
    let (recorder, rollout, mode) = {
        let registrations = store.live_recorders.lock().await;
        let actual = registrations
            .get(&thread)
            .ok_or(ThreadStoreError::ThreadNotFound { thread_id: thread })?;
        if !actual.retained_writer.same(&writer) {
            return Err(ThreadStoreError::Conflict {
                message: "actual local writer incarnation was replaced".into(),
            });
        }
        (
            actual.recorder.clone(),
            actual.rollout_id,
            actual.history_mode,
        )
    };
    let snapshot = recorder
        .retained_local_rollout(max_bytes)
        .await
        .map_err(|error| ThreadStoreError::Internal {
            message: error.to_string(),
        })?;
    // SessionMeta.id is the logical thread and session_id names its root.
    // The immutable rollout id belongs to this SAME recorder registration;
    // the matched acquisition token and held lifecycle lock bind its handle
    // through the writer command. Metadata has no physical-rollout field.
    if snapshot.session_meta.id != thread || snapshot.session_meta.history_mode != mode {
        return Err(ThreadStoreError::Conflict {
            message: "actual local writer cutoff differs from original acquisition".into(),
        });
    }
    Ok(RetainedLocalHistory {
        thread_id: thread,
        rollout_id: rollout,
        session_meta: snapshot.session_meta,
        items: snapshot.items,
    })
}
