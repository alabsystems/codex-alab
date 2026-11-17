// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::LocalThreadStore;
use crate::CreateThreadParams;
use crate::ResumeThreadParams;
use crate::RetainedDeferredHistory;
use crate::RetainedWriterIdentity;
use crate::ThreadStore;
use crate::ThreadStoreError;
use crate::ThreadStoreResult;
use crate::ThreadStoreResumePreparation;
use codex_protocol::ThreadId;
use codex_rollout::RolloutItem;
use std::sync::Arc;

pub(super) async fn create(
    store: &LocalThreadStore,
    params: CreateThreadParams,
) -> ThreadStoreResult<Option<RetainedWriterIdentity>> {
    super::live_writer::create_thread(store, params)
        .await
        .map(Some)
}
pub(super) async fn resume(
    store: &LocalThreadStore,
    params: ResumeThreadParams,
    preparation: Option<ThreadStoreResumePreparation>,
) -> ThreadStoreResult<(Arc<Vec<RolloutItem>>, Option<RetainedWriterIdentity>)> {
    if let Some(preparation) = preparation {
        // Preserve the original preparation contract. Local does not currently
        // produce one; its original unsupported/error result remains exact.
        return store
            .resume_thread_with_preparation(params, preparation)
            .await
            .map(|history| (history, None));
    }
    let (history, identity) = super::live_writer::resume_thread(store, params).await?;
    Ok((history, Some(identity)))
}
pub(super) async fn load(
    store: &LocalThreadStore,
    thread_id: ThreadId,
    writer: RetainedWriterIdentity,
) -> ThreadStoreResult<Option<RetainedDeferredHistory>> {
    let _lifecycle = store.live_writer_locks.lock(thread_id).await;
    let (recorder, rollout_id, history_mode) = {
        let registrations = store.live_recorders.lock().await;
        let entry = registrations
            .get(&thread_id)
            .ok_or(ThreadStoreError::ThreadNotFound { thread_id })?;
        if !entry.retained_writer.same(&writer) {
            return Err(ThreadStoreError::Conflict {
                message: "original live rollout writer was replaced".to_string(),
            });
        }
        (entry.recorder.clone(), entry.rollout_id, entry.history_mode)
    };
    let Some(snapshot) = recorder
        .retained_deferred_history()
        .await
        .map_err(|error| ThreadStoreError::Internal {
            message: error.to_string(),
        })?
    else {
        return Ok(None);
    };
    if snapshot.session_meta.id != thread_id || snapshot.session_meta.history_mode != history_mode {
        return Err(ThreadStoreError::Conflict {
            message: "deferred rollout snapshot differs from its actual live recorder registration"
                .to_string(),
        });
    }
    Ok(Some(RetainedDeferredHistory {
        thread_id,
        rollout_id,
        session_meta: snapshot.session_meta,
        items: snapshot.items,
    }))
}

/// The original writer cutoff remains unchanged. Reacquire its original
/// lifecycle lock and recheck the SAME acquisition token before any lineage
/// reads, so discard/recreate ABA cannot combine incarnations.
pub(super) async fn display(
    store: &LocalThreadStore,
    thread_id: ThreadId,
    writer: RetainedWriterIdentity,
) -> ThreadStoreResult<Option<crate::RetainedDeferredDisplayHistory>> {
    let Some(local) = load(store, thread_id, writer.clone()).await? else {
        return Ok(None);
    };
    let _lifecycle = store.live_writer_locks.lock(thread_id).await;
    {
        let registrations = store.live_recorders.lock().await;
        let entry = registrations
            .get(&thread_id)
            .ok_or(ThreadStoreError::ThreadNotFound { thread_id })?;
        if !entry.retained_writer.same(&writer)
            || entry.rollout_id != local.rollout_id
            || entry.history_mode != local.session_meta.history_mode
        {
            return Err(ThreadStoreError::Conflict {
                message: "deferred display writer incarnation changed".into(),
            });
        }
    }
    let inherited_items =
        match local.session_meta.history_base {
            None => Vec::new(),
            Some(base) => {
                if local.session_meta.history_mode
                    != codex_protocol::protocol::ThreadHistoryMode::Paginated
                {
                    return Err(ThreadStoreError::Conflict {
                        message: "deferred lineage has nonpaginated history".into(),
                    });
                }
                // Original attached creation persists its header immediately, so
                // it cannot produce this deferred snapshot. Do not guess a hidden
                // ancestor policy if that original invariant changes.
                if local.session_meta.attached_thread {
                    return Err(ThreadStoreError::Unsupported {
                        operation: "deferred_attached_display_history",
                    });
                }
                let parent = local.session_meta.forked_from_id.ok_or_else(|| {
                    ThreadStoreError::Conflict {
                        message: "deferred inherited history has no actual fork source".into(),
                    }
                })?;
                if parent == thread_id {
                    return Err(ThreadStoreError::Conflict {
                        message: "deferred fork source cycles to itself".into(),
                    });
                }
                store.retained_inherited_display(thread_id, base).await?
            }
        };
    Ok(Some(crate::RetainedDeferredDisplayHistory {
        local,
        inherited_items,
    }))
}
