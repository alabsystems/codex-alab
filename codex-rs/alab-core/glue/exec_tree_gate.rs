// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Fence short start/input operations without waiting for whole session lifetimes.
#[path = "exec_child_restore.rs"]
mod child_restore;
use super::LocalAgentRuntime;
use crate::CodexThread;
pub use child_restore::ExecPreparedChildRestore;
pub use child_restore::ExecRestoredChild;
pub use child_restore::ExecRestoredMember;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio::sync::OwnedRwLockReadGuard;
use tokio::sync::OwnedRwLockWriteGuard;
use tokio::sync::RwLock;

#[derive(Default)]
pub(crate) struct ExecTreeGate {
    sealed: AtomicBool,
    starts: Arc<RwLock<()>>,
    quota_events: Arc<crate::session::alab_exec_recovery_options::quota_observation::Events>,
}

/// A drained start barrier for one local exec tree, not an execution grant.
pub struct ExecTreeSeal {
    gate: Arc<ExecTreeGate>,
    _guard: Option<OwnedRwLockWriteGuard<()>>,
    committed: bool,
}

impl ExecTreeSeal {
    pub(crate) fn protects(&self, runtime: &LocalAgentRuntime, committed: bool) -> bool {
        Arc::ptr_eq(&self.gate, &runtime.exec_start_gate)
            && self._guard.is_some()
            && (!committed || self.committed)
    }

    pub(crate) async fn loaded_threads(&self, root: &CodexThread) -> Result<Vec<Arc<CodexThread>>> {
        if !self.protects(&root.session.services.local_agent_runtime, false) {
            return Err(CodexErr::InvalidRequest(
                "tree inventory requires its own drained barrier".into(),
            ));
        }
        let manager = root
            .session
            .services
            .local_agent_runtime
            .manager
            .upgrade()
            .ok_or_else(|| CodexErr::InvalidRequest("tree inventory manager is gone".into()))?;
        let threads = manager.threads.read().await;
        let mut result = threads
            .values()
            .filter(|thread| {
                Arc::ptr_eq(
                    &self.gate,
                    &thread.session.services.local_agent_runtime.exec_start_gate,
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        if !result
            .iter()
            .any(|thread| std::ptr::eq(thread.as_ref(), root))
        {
            return Err(CodexErr::InvalidRequest(
                "tree inventory root was replaced".into(),
            ));
        }
        result.sort_by_key(|thread| thread.session.thread_id.to_string());
        Ok(result)
    }

    /// Keep the source sealed once its owner commits to stopping the runtime.
    pub fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for ExecTreeSeal {
    fn drop(&mut self) {
        if !self.committed {
            self.gate.sealed.store(false, Ordering::Release);
        }
    }
}

impl ExecTreeGate {
    fn admit(&self) -> Result<OwnedRwLockReadGuard<()>> {
        let refused = || CodexErr::InvalidRequest("agent tree is being transferred".into());
        if self.sealed.load(Ordering::Acquire) {
            return Err(refused());
        }
        // Nested startup operations must refuse, not wait behind a writer that
        // is waiting for their outer operation. This avoids a reentrant deadlock.
        let guard = Arc::clone(&self.starts)
            .try_read_owned()
            .map_err(|_| refused())?;
        if self.sealed.load(Ordering::Acquire) {
            return Err(refused());
        }
        Ok(guard)
    }

    async fn seal(self: &Arc<Self>) -> Result<ExecTreeSeal> {
        self.sealed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                CodexErr::InvalidRequest("agent tree transfer is already in progress".into())
            })?;
        // Construct rollback ownership before awaiting. Cancelling an uncommitted
        // seal cannot strand a live source behind a permanently closed gate.
        let mut seal = ExecTreeSeal {
            gate: Arc::clone(self),
            _guard: None,
            committed: false,
        };
        seal._guard = Some(Arc::clone(&self.starts).write_owned().await);
        Ok(seal)
    }
}

impl LocalAgentRuntime {
    pub(crate) async fn prepare_exec_review(
        &self,
        parent: Arc<crate::session::session::Session>,
        context: Arc<crate::session::turn_context::TurnContext>,
    ) -> Result<crate::thread_manager::alab_exec_review::Registration> {
        let start = self.admit_exec_tree_start()?;
        let manager = self.manager.upgrade().ok_or_else(|| {
            CodexErr::InvalidRequest("review's thread manager is unavailable".into())
        })?;
        crate::thread_manager::alab_exec_review::Registration::prepare(
            manager, parent, context, start,
        )
        .await
    }

    pub(crate) fn shares_exec_runtime(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.registry, &other.registry)
            && Arc::ptr_eq(&self.exec_start_gate, &other.exec_start_gate)
    }

    pub(crate) fn exec_quota_events(
        &self,
    ) -> Arc<crate::session::alab_exec_recovery_options::quota_observation::Events> {
        Arc::clone(&self.exec_start_gate.quota_events)
    }

    pub(crate) fn admit_exec_tree_start(&self) -> Result<OwnedRwLockReadGuard<()>> {
        self.exec_start_gate.admit()
    }

    pub(crate) async fn seal_exec_tree_starts(&self) -> Result<ExecTreeSeal> {
        self.exec_start_gate.seal().await
    }
}

impl CodexThread {
    pub(crate) fn shares_exec_tree(&self, other: &Self) -> bool {
        Arc::ptr_eq(
            &self.session.services.local_agent_runtime.registry,
            &other.session.services.local_agent_runtime.registry,
        )
    }

    /// Fence starts before inspecting a native exec handoff's original turn.
    /// Dropping an uncommitted barrier restores normal admission.
    pub async fn seal_exec_tree_starts(&self) -> Result<ExecTreeSeal> {
        if self
            .thread_extension_data()
            .get::<codex_alab_core::quota::HeadlessExec>()
            .is_none()
        {
            return Err(CodexErr::InvalidRequest(
                "tree sealing requires an embedded exec runtime".into(),
            ));
        }
        self.session
            .services
            .local_agent_runtime
            .seal_exec_tree_starts()
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::poll;

    #[tokio::test]
    async fn seal_drains_admitted_starts_and_permanently_refuses_late_ones() {
        let gate = Arc::new(ExecTreeGate::default());
        let admitted = gate.admit().unwrap();
        let mut pending = Box::pin(gate.seal());
        assert!(poll!(pending.as_mut()).is_pending());
        assert!(gate.admit().is_err());
        drop(admitted);
        let mut seal = pending.await.unwrap();
        seal.commit();
        drop(seal);
        assert!(gate.admit().is_err());
        assert!(gate.seal().await.is_err());
    }

    #[tokio::test]
    async fn cancelled_and_uncommitted_seals_reopen_the_source() {
        let gate = Arc::new(ExecTreeGate::default());
        let admitted = gate.admit().unwrap();
        let mut pending = Box::pin(gate.seal());
        assert!(poll!(pending.as_mut()).is_pending());
        drop(pending);
        drop(gate.admit().unwrap());
        drop(admitted);
        let seal = gate.seal().await.unwrap();
        assert!(gate.admit().is_err());
        drop(seal);
        assert!(gate.admit().is_ok());
    }

    #[tokio::test]
    async fn the_barrier_is_shared_only_with_the_same_runtime_tree() {
        let runtime = super::super::LocalAgentControl::default().runtime;
        let child = runtime.clone();
        let other_tree = super::super::LocalAgentControl::default().runtime;
        assert!(Arc::ptr_eq(
            &runtime.exec_quota_events(),
            &child.exec_quota_events()
        ));
        assert!(!Arc::ptr_eq(
            &runtime.exec_quota_events(),
            &other_tree.exec_quota_events()
        ));
        let mut seal = runtime.exec_start_gate.seal().await.unwrap();
        seal.commit();
        assert!(child.admit_exec_tree_start().is_err());
        assert!(other_tree.admit_exec_tree_start().is_ok());
    }
}
