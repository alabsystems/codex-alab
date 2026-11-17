// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Existing store acquisitions remain the sole policy. Unsupported stores
//! supply no incarnation token and cannot authorize a deferred snapshot.
use crate::CreateThreadParams;
use crate::ResumeThreadParams;
use crate::RetainedWriterIdentity;
use crate::ThreadStore;
use crate::ThreadStoreResult;
use crate::ThreadStoreResumePreparation;
use codex_rollout::RolloutItem;
use std::sync::Arc;

pub(crate) async fn create<S: ThreadStore + ?Sized>(
    store: &S,
    params: CreateThreadParams,
) -> ThreadStoreResult<Option<RetainedWriterIdentity>> {
    store.create_thread(params).await?;
    Ok(None)
}
pub(crate) async fn resume<S: ThreadStore + ?Sized>(
    store: &S,
    params: ResumeThreadParams,
    preparation: Option<ThreadStoreResumePreparation>,
) -> ThreadStoreResult<(Arc<Vec<RolloutItem>>, Option<RetainedWriterIdentity>)> {
    let history = match preparation {
        Some(preparation) => {
            store
                .resume_thread_with_preparation(params, preparation)
                .await?
        }
        None => store.resume_thread(params).await?,
    };
    Ok((history, None))
}
