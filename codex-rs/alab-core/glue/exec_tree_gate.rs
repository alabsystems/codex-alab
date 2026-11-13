// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Fence short start/input operations without waiting for whole session lifetimes.
#[path = "exec_child_restore.rs"]
mod child_restore;
use super::LocalAgentRuntime;
use crate::CodexThread;
pub use child_restore::ExecIdleRestore;
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

    pub(crate) fn admit_upgrade_input(
        &self,
    ) -> Result<codex_alab_core::upgrade_barrier::Admission> {
        self.alab_upgrade
            .admit()
            .map_err(|error| CodexErr::InvalidRequest(error.to_string()))
    }

    pub(crate) fn admit_exec_tree_start(&self) -> Result<ExecStartAdmission> {
        let manager = self
            .alab_upgrade
            .admit()
            .map_err(|error| CodexErr::InvalidRequest(error.to_string()))?;
        let tree = self.exec_start_gate.admit()?;
        Ok(ExecStartAdmission {
            _tree: tree,
            _manager: manager,
        })
    }

    /// Mutation ownership follows native shutdown membership into detached
    /// registration or cancellation cleanup. Passive completion watchers keep
    /// ordinary membership without indefinitely preventing an idle transfer.
    pub(crate) fn admit_exec_producer(&self) -> Result<super::AgentTreeMembership> {
        let admission = self.admit_exec_tree_start()?;
        let mut membership = self.admit_start()?;
        membership.alab_exec_start = Some(Arc::new(admission));
        Ok(membership)
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

/// Retain both short tree admission and manager-wide admission through startup.
pub(crate) struct ExecStartAdmission {
    _tree: OwnedRwLockReadGuard<()>,
    // Release the physical tree reader before advertising manager quiescence.
    _manager: codex_alab_core::upgrade_barrier::Admission,
}

pub(crate) fn manager_upgrade(
    manager: &std::sync::Weak<crate::thread_manager::ThreadManagerState>,
) -> Arc<codex_alab_core::upgrade_barrier::State> {
    manager.upgrade().map_or_else(
        || Arc::new(codex_alab_core::upgrade_barrier::State::default()),
        |manager| Arc::clone(&manager.alab_upgrade),
    )
}

/// Manager-wide start/input barrier and exact loaded-thread inventory. This
/// does not assert that its members are idle, persisted or restorable. The
/// caller must prove those facts before combining commit with native teardown.
pub struct UpgradeManagerBarrier {
    transfer: codex_alab_core::upgrade_barrier::Transfer,
    threads:
        OwnedRwLockReadGuard<std::collections::HashMap<codex_protocol::ThreadId, Arc<CodexThread>>>,
}

impl UpgradeManagerBarrier {
    pub fn threads(&self) -> Vec<Arc<CodexThread>> {
        let mut threads = self.threads.values().cloned().collect::<Vec<_>>();
        threads.sort_by_key(|thread| thread.session.thread_id.to_string());
        threads
    }

    pub(crate) async fn invalidated(&self) {
        self.transfer.invalidated().await;
    }

    pub fn valid(&self) -> bool {
        self.transfer.valid()
    }

    pub(crate) fn protects_member(&self, thread: &CodexThread, committed: bool) -> bool {
        let runtime = &thread.session.services.local_agent_runtime;
        self.transfer.belongs_to(&runtime.alab_upgrade)
            && self
                .threads
                .get(&thread.session.thread_id)
                .is_some_and(|kept| std::ptr::eq(kept.as_ref(), thread))
            && if committed {
                runtime.alab_upgrade.committed()
            } else {
                self.transfer.valid()
            }
    }

    /// Retain the exact members after commitment while releasing the registry
    /// lock so their native shutdown and eviction can finish.
    pub fn into_committed(self) -> Result<CommittedUpgradeInventory> {
        if !self
            .threads
            .values()
            .any(|thread| self.protects_member(thread, true))
        {
            return Err(CodexErr::InvalidRequest(
                "upgrade inventory was not committed".into(),
            ));
        }
        let threads = self.threads();
        let transfer = self.transfer;
        drop(self.threads);
        Ok(CommittedUpgradeInventory { transfer, threads })
    }

    /// Linearize the final host decision with every root's native startup and
    /// accepted input. A new admission revokes this attempt and proceeds; it is
    /// never discarded merely because the update is being prepared.
    pub fn commit_if(&mut self, decide: impl FnOnce() -> bool) -> bool {
        self.transfer.commit_if(decide)
    }
}

/// Data custody for the source's exact committed members, not execution authority.
/// Registry retirement may now proceed; these handles keep the original queues alive.
pub struct CommittedUpgradeInventory {
    transfer: codex_alab_core::upgrade_barrier::Transfer,
    threads: Vec<Arc<CodexThread>>,
}

impl CommittedUpgradeInventory {
    pub fn threads(&self) -> &[Arc<CodexThread>] {
        &self.threads
    }

    pub(crate) fn protects_member(&self, thread: &CodexThread) -> bool {
        let runtime = &thread.session.services.local_agent_runtime;
        self.transfer.belongs_to(&runtime.alab_upgrade)
            && runtime.alab_upgrade.committed()
            && self
                .threads
                .iter()
                .any(|kept| std::ptr::eq(kept.as_ref(), thread))
    }
}

impl CodexThread {
    /// Capture every loaded root and private member under one manager admission.
    /// Keep the registry read guard through the final decision so eviction and
    /// replacement cannot silently change the captured inventory.
    pub async fn prepare_upgrade_manager(&self) -> Result<UpgradeManagerBarrier> {
        let runtime = &self.session.services.local_agent_runtime;
        let manager = runtime.manager.upgrade().ok_or_else(|| {
            CodexErr::InvalidRequest("upgrade thread manager is unavailable".into())
        })?;
        if !Arc::ptr_eq(&manager.alab_upgrade, &runtime.alab_upgrade) {
            return Err(CodexErr::InvalidRequest(
                "upgrade manager identity changed".into(),
            ));
        }
        let transfer = manager
            .alab_upgrade
            .prepare()
            .map_err(|error| CodexErr::InvalidRequest(error.to_string()))?;
        let threads = tokio::select! {
            biased;
            () = transfer.invalidated() => return Err(CodexErr::InvalidRequest(
                "upgrade inventory was superseded".into(),
            )),
            threads = Arc::clone(&manager.threads).read_owned() => threads,
        };
        if !transfer.valid()
            || !threads
                .values()
                .any(|thread| std::ptr::eq(thread.as_ref(), self))
        {
            return Err(CodexErr::InvalidRequest(
                "upgrade inventory was superseded".into(),
            ));
        }
        Ok(UpgradeManagerBarrier { transfer, threads })
    }
}

impl crate::session::session::Session {
    /// A post-turn probe can run after the idle observer queued a handoff.
    /// Inspect without extracting input; only actual pending work needs admission.
    pub(crate) async fn admit_exec_pending_work(&self) -> Option<ExecStartAdmission> {
        if !self.input_queue.has_pending_mailbox_items().await {
            return None;
        }
        self.services
            .local_agent_runtime
            .admit_exec_tree_start()
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn upgrade_preparation_yields_to_a_blocked_native_reload() {
        let home = tempfile::tempdir().unwrap();
        let config = crate::config::ConfigBuilder::without_managed_config_for_tests()
            .codex_home(home.path().to_owned())
            .build()
            .await
            .unwrap();
        let manager = crate::ThreadManager::with_models_provider_and_home_for_tests(
            codex_login::CodexAuth::from_api_key("fixture"),
            config.model_provider.clone(),
            home.path().to_owned(),
            Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        );
        let root = manager
            .start_thread(crate::StartThreadOptions::new(config.clone()))
            .await
            .unwrap()
            .thread;
        let runtime = &root.session.services.local_agent_runtime;
        let control = runtime.control(root.session.session_id());
        let native_manager = runtime.manager.upgrade().unwrap();
        let held = Arc::clone(&native_manager.threads).write_owned().await;
        let mut transfer = runtime.alab_upgrade.prepare().unwrap();
        let mut reload = Box::pin(control.ensure_v2_agent_loaded(
            config,
            codex_protocol::ThreadId::new(),
            Some(Arc::clone(&root)),
        ));
        assert!(poll!(reload.as_mut()).is_pending());
        assert!(!transfer.commit_if(|| panic!("native reload owns priority")));
        drop(transfer);
        assert!(runtime.alab_upgrade.prepare().is_err());
        drop(reload);
        assert!(root.is_running());
        drop(held);
        drop(root.prepare_upgrade_manager().await.unwrap());
        root.shutdown_exec_tree().await.unwrap();
    }

    #[tokio::test]
    async fn upgrade_producer_ownership_survives_cancellation_and_teardown_clones() {
        let runtime = super::super::LocalAgentControl::default().runtime;
        let watcher = runtime.admit_start().unwrap();
        let mut prepared = runtime.alab_upgrade.prepare().unwrap();
        let producer = runtime.admit_exec_producer().unwrap();
        assert!(!prepared.commit_if(|| panic!("new native work must revoke transfer")));
        drop(prepared);
        let cleanup = producer
            .clone()
            .into_teardown_guard("upgrade_fixture", None);
        let nested = cleanup.clone_for_teardown("upgrade_fixture_nested", None);
        drop(producer);
        assert!(matches!(
            runtime.alab_upgrade.prepare(),
            Err(codex_alab_core::upgrade_barrier::Refusal::PendingAdmission)
        ));
        cleanup.complete();
        assert!(runtime.alab_upgrade.prepare().is_err());
        let mut sealing = Box::pin(runtime.exec_start_gate.seal());
        assert!(poll!(sealing.as_mut()).is_pending());
        nested.complete();
        let seal = sealing.await.unwrap();
        drop(seal);
        // The passive watcher still owns real shutdown membership, but neither
        // a completed mutation nor its completed cleanup can strand admission.
        drop(runtime.admit_exec_producer().unwrap());
        let mut ready = runtime.alab_upgrade.prepare().unwrap();
        assert!(ready.commit());
        assert!(runtime.admit_exec_producer().is_err());
        drop(watcher);
    }

    #[tokio::test]
    async fn upgrade_forest_revocation_cancels_a_blocked_registry_read() {
        let home = tempfile::tempdir().unwrap();
        let config = crate::config::ConfigBuilder::without_managed_config_for_tests()
            .codex_home(home.path().to_owned())
            .build()
            .await
            .unwrap();
        let manager = crate::ThreadManager::with_models_provider_and_home_for_tests(
            codex_login::CodexAuth::from_api_key("fixture"),
            config.model_provider.clone(),
            home.path().to_owned(),
            Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        );
        let root = manager
            .start_thread(crate::StartThreadOptions::new(config.clone()))
            .await
            .unwrap()
            .thread;
        let runtime = &root.session.services.local_agent_runtime;
        let native_manager = runtime.manager.upgrade().unwrap();
        let held = Arc::clone(&native_manager.threads).write_owned().await;
        let invocation =
            crate::config::ExecConfiguration::capture(&config, &Default::default()).unwrap();
        let mut preparing = Box::pin(root.prepare_upgrade_forest(&invocation));
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(std::future::Future::poll(
                preparing.as_mut(),
                cx
            ),))
            .await
            .is_pending()
        );
        let input = runtime.alab_upgrade.admit().unwrap();
        assert!(input.interrupted_transfer());
        drop(input);
        let error = tokio::time::timeout(std::time::Duration::from_secs(10), preparing)
            .await
            .unwrap()
            .err()
            .expect("revoked inventory must return while write lock stays held");
        assert!(error.contains("superseded"));
        assert!(root.is_running());
        drop(held);
        drop(root.prepare_upgrade_forest(&invocation).await.unwrap());
        root.shutdown_exec_tree().await.unwrap();
    }

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

    #[tokio::test]
    async fn an_empty_post_turn_probe_does_not_revoke_a_prepared_upgrade() {
        let (session, _) = crate::session::tests::make_session_and_context().await;
        let session = Arc::new(session);
        let mut transfer = session
            .services
            .local_agent_runtime
            .alab_upgrade
            .prepare()
            .unwrap();
        session.maybe_start_turn_for_pending_work().await;
        assert!(transfer.valid());
        assert!(transfer.commit());
        assert!(session.active_turn.lock().await.is_none());
    }

    #[tokio::test]
    async fn pending_mail_revokes_preparation_without_extracting_the_accepted_message() {
        let (session, _) = crate::session::tests::make_session_and_context().await;
        let runtime = &session.services.local_agent_runtime;
        let mut transfer = runtime.alab_upgrade.prepare().unwrap();
        session
            .input_queue
            .enqueue_mailbox_communication(
                crate::PendingAgentMessage::from(
                    codex_protocol::protocol::InterAgentCommunication::new(
                        codex_protocol::AgentPath::root(),
                        codex_protocol::AgentPath::root(),
                        Vec::new(),
                        "accepted native pending work".into(),
                        true,
                    ),
                ),
                Default::default(),
            )
            .await;
        let admission = session
            .admit_exec_pending_work()
            .await
            .expect("pending work retains priority");
        assert!(!transfer.commit_if(|| panic!("pending work revoked preparation")));
        assert!(session.input_queue.has_pending_mailbox_items().await);
        drop(admission);
        drop(transfer);
        let mut committed = runtime.alab_upgrade.prepare().unwrap();
        assert!(committed.commit());
        assert!(session.admit_exec_pending_work().await.is_none());
        assert!(
            session.input_queue.has_pending_mailbox_items().await,
            "a committed source cannot extract the queued message"
        );
    }
}
