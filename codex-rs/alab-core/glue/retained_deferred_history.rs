// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionMeta;
use codex_rollout::RolloutItem;

/// A live writer observation, not a durable history record or an empty-history fallback.
#[derive(Clone, Debug)]
pub struct RetainedDeferredHistory {
    /// The actual LiveThread's logical id, also checked against SessionMeta.id.
    pub thread_id: ThreadId,
    /// Actual immutable rollout id from the same live recorder registration.
    pub rollout_id: ThreadId,
    pub session_meta: SessionMeta,
    /// Entire local pending canonical sequence in writer order. SessionMeta.history_base
    /// can refer to inherited history outside this buffer; callers must resolve that
    /// actual lineage or refuse it, never treat the buffer as a complete fork history.
    pub items: Vec<RolloutItem>,
}

/// Entire LOCAL sequence at an actual writer-ordered cutoff, including a
/// materialized writer. This is not a deferred/empty/inherited-history claim.
#[derive(Clone, Debug)]
pub struct RetainedLocalHistory {
    pub thread_id: ThreadId,
    pub rollout_id: ThreadId,
    pub session_meta: SessionMeta,
    pub items: Vec<RolloutItem>,
}

/// Process-local identity of ONE actual live recorder installation. It confers
/// no current Core, SDK, read permission, durability or settlement authority.
/// Its allocation owns no recorder, channel, file, writer lock or cleanup task.
#[derive(Clone)]
pub struct RetainedWriterIdentity(std::sync::Arc<()>);
impl RetainedWriterIdentity {
    pub(crate) fn new() -> Self {
        Self(std::sync::Arc::new(()))
    }
    pub(crate) fn same(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}
impl std::fmt::Debug for RetainedWriterIdentity {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("RetainedWriterIdentity(<process-local>)")
    }
}
pub(crate) type AcquiredHistory = (std::sync::Arc<Vec<RolloutItem>>, RetainedWriterIdentity);

#[path = "retained_writer_acquisition.rs"]
pub(crate) mod acquisition;

/// A separate complete display prefix: original durable inherited records plus
/// the SAME deferred writer's local cutoff. The local API above stays local.
/// This is neither a persistence claim nor a model-context reconstruction.
#[derive(Clone, Debug)]
pub struct RetainedDeferredDisplayHistory {
    pub local: RetainedDeferredHistory,
    pub inherited_items: Vec<RolloutItem>,
}
impl RetainedDeferredDisplayHistory {
    pub fn into_items(self) -> Vec<RolloutItem> {
        let mut items = self.inherited_items;
        items.extend(self.local.items);
        items
    }
}
