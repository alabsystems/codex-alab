// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One passive watcher per native runtime; neither the watcher nor the transport
//! grants goal authority or retains the runtime/server through a wait.

use super::GoalRuntimeHandle;
use codex_alab_core::goal_persistence::Admission;
use codex_protocol::protocol::AlabGoalPersistencePhase;
use codex_protocol::protocol::AlabGoalRuntimeUpdatedNotification;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio::sync::watch;

type Registry = Mutex<HashMap<String, Weak<GoalRuntimeHandle>>>;
type PublishUpgradeRetirements = dyn Fn(&[Arc<GoalRuntimeHandle>], &mut dyn FnMut() -> Result<(), String>) -> Result<(), String>
    + Send
    + Sync;

/// Exact registry owner for one inventory-wide committed-upgrade publication.
/// The callback holds the service registry and persistence-gap locks while it
/// invokes the non-awaiting publication closure.
#[doc(hidden)]
pub struct UpgradeRetirementCoordinator {
    identity: Arc<()>,
    publish: Arc<PublishUpgradeRetirements>,
}

impl std::fmt::Debug for UpgradeRetirementCoordinator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UpgradeRetirementCoordinator")
            .finish_non_exhaustive()
    }
}

impl UpgradeRetirementCoordinator {
    pub(crate) fn new(
        publish: impl Fn(
            &[Arc<GoalRuntimeHandle>],
            &mut dyn FnMut() -> Result<(), String>,
        ) -> Result<(), String>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            identity: Arc::new(()),
            publish: Arc::new(publish),
        }
    }

    pub fn same_owner(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity)
    }

    pub fn publish(
        &self,
        runtimes: &[Arc<GoalRuntimeHandle>],
        commit: &mut dyn FnMut() -> Result<(), String>,
    ) -> Result<(), String> {
        (self.publish)(runtimes, commit)
    }
}

pub(super) struct Owner {
    incarnation: String,
    registration: Mutex<Option<Registration>>,
    upgrade_retirement: Mutex<Option<Arc<UpgradeRetirementCoordinator>>>,
    registered_once: AtomicBool,
    retired: AtomicBool,
}

impl Default for Owner {
    fn default() -> Self {
        Self {
            incarnation: codex_alab_core::goal_persistence::observation_incarnation(),
            registration: Mutex::default(),
            upgrade_retirement: Mutex::default(),
            registered_once: AtomicBool::new(false),
            retired: AtomicBool::new(false),
        }
    }
}

struct Registration {
    registry: Weak<Registry>,
    cancel: watch::Sender<bool>,
    reconcile: Arc<dyn Fn() + Send + Sync>,
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.cancel.send_replace(true);
    }
}

impl GoalRuntimeHandle {
    #[doc(hidden)]
    pub fn alab_upgrade_retirement_coordinator(&self) -> Option<Arc<UpgradeRetirementCoordinator>> {
        self.inner
            .alab_status
            .upgrade_retirement
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn alab_runtime_status_retired(&self) -> bool {
        self.inner.alab_status.retired.load(Ordering::SeqCst)
    }

    /// Close runtime authority before teardown waits behind work already
    /// admitted on this owner. Persistence remains live only for the orderly
    /// final accounting drain and is retired before registry custody is released.
    pub(crate) fn alab_begin_runtime_retirement(&self) -> bool {
        self.inner
            .alab_status
            .retired
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub(crate) fn alab_finalize_runtime_retirement(&self) {
        self.inner.accounting_state.alab_persistence.retire();
    }

    /// A handle refused at registration can never become authoritative after
    /// the current owner retires. It was never published, so rejection is
    /// intentionally silent.
    pub(crate) fn alab_reject_runtime_status(&self) {
        self.inner
            .alab_status
            .registered_once
            .store(true, Ordering::SeqCst);
        let _ = self.alab_begin_runtime_retirement();
        self.alab_finalize_runtime_retirement();
        self.inner
            .alab_status
            .registration
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }

    pub(crate) fn alab_watch_runtime_status(
        self: &Arc<Self>,
        registry: Weak<Registry>,
        reconcile: Arc<dyn Fn() + Send + Sync>,
        upgrade_retirement: Arc<UpgradeRetirementCoordinator>,
    ) {
        if !codex_alab_core::is_alab_binary() || self.alab_runtime_status_retired() {
            return;
        }
        self.inner
            .alab_status
            .registered_once
            .store(true, Ordering::SeqCst);
        let mut registration = self
            .inner
            .alab_status
            .registration
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.alab_runtime_status_retired() {
            return;
        }
        if registration
            .as_ref()
            .is_some_and(|current| current.registry.ptr_eq(&registry))
        {
            return;
        }
        *self
            .inner
            .alab_status
            .upgrade_retirement
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(upgrade_retirement);
        let (cancel, mut canceled) = watch::channel(false);
        *registration = Some(Registration {
            registry,
            cancel,
            reconcile,
        });
        drop(registration);
        let runtime = Arc::downgrade(self);
        let mut changes = self.inner.accounting_state.alab_persistence.changes();
        tokio::spawn(async move {
            loop {
                if *canceled.borrow() {
                    return;
                }
                let Some(current) = runtime.upgrade() else {
                    return;
                };
                current.alab_emit_runtime_status();
                drop(current);
                tokio::select! {
                    result = changes.changed() => if result.is_err() { return; },
                    _ = canceled.changed() => return,
                }
            }
        });
    }

    pub(crate) fn alab_reconcile_unowned_goal_mutation(&self) {
        let reconcile = self
            .inner
            .alab_status
            .registration
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|registration| Arc::clone(&registration.reconcile));
        if let Some(reconcile) = reconcile {
            reconcile();
        }
    }

    /// A runtime used directly by upstream tests has no registry. Once the
    /// extension publishes an owner, however, retirement or replacement is a
    /// permanent loss of goal-mutation authority for that handle.
    pub(crate) fn alab_registered_current(&self) -> bool {
        if !codex_alab_core::is_alab_binary() {
            return true;
        }
        if self.alab_runtime_status_retired() {
            return false;
        }
        if !self
            .inner
            .alab_status
            .registered_once
            .load(Ordering::SeqCst)
        {
            return true;
        }
        let registry = self
            .inner
            .alab_status
            .registration
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .and_then(|registration| registration.registry.upgrade());
        let Some(registry) = registry else {
            return false;
        };
        registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&self.inner.thread_id.to_string())
            .and_then(Weak::upgrade)
            .is_some_and(|current| Arc::ptr_eq(&current.inner, &self.inner))
    }

    /// Snapshot publication is independent of SQLite reads, including reattach.
    pub fn alab_emit_runtime_status(&self) {
        let registry = self
            .inner
            .alab_status
            .registration
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .and_then(|registration| registration.registry.upgrade());
        let Some(registry) = registry else {
            return;
        };
        let runtimes = registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Native registration serializes publication with retirement/replacement.
        // An old watch cannot announce itself as the new owner of a reused key.
        if runtimes
            .get(&self.inner.thread_id.to_string())
            .and_then(Weak::upgrade)
            .is_some_and(|current| Arc::ptr_eq(&current.inner, &self.inner))
        {
            self.alab_publish_runtime_status(false);
        }
    }

    /// Capture and stamp atomically with native current-owner validation; the
    /// caller releases this custody before its outgoing await or database read.
    pub fn alab_runtime_status_snapshot(
        &self,
    ) -> Option<codex_protocol::protocol::AlabGoalRuntimeSnapshot> {
        let registry = self
            .inner
            .alab_status
            .registration
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .and_then(|registration| registration.registry.upgrade())?;
        let runtimes = registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !runtimes
            .get(&self.inner.thread_id.to_string())
            .and_then(Weak::upgrade)
            .is_some_and(|current| Arc::ptr_eq(&current.inner, &self.inner))
        {
            return None;
        }
        self.inner
            .event_emitter
            .alab_runtime_inline_snapshot(self.alab_runtime_status_update(false))
    }

    /// Called while matching native unregister still holds registry custody.
    pub(crate) fn alab_retire_runtime_status(&self) {
        self.inner
            .alab_status
            .registered_once
            .store(true, Ordering::SeqCst);
        self.inner.alab_status.retired.store(true, Ordering::SeqCst);
        self.inner
            .alab_status
            .registration
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        self.alab_publish_runtime_status(true);
    }

    fn alab_publish_runtime_status(&self, retired: bool) {
        self.inner
            .event_emitter
            .alab_runtime_updated(self.alab_runtime_status_update(retired));
    }

    fn alab_runtime_status_update(&self, retired: bool) -> AlabGoalRuntimeUpdatedNotification {
        let snapshot = self.inner.accounting_state.alab_persistence.snapshot();
        let persistence = (!retired).then_some(match snapshot.admission {
            Admission::Ready => AlabGoalPersistencePhase::Ready,
            Admission::Writing => AlabGoalPersistencePhase::Writing,
            Admission::Held => AlabGoalPersistencePhase::Held,
        });
        AlabGoalRuntimeUpdatedNotification {
            thread_id: self.inner.thread_id.to_string(),
            incarnation: self.inner.alab_status.incarnation.clone(),
            generation: 0,            // Assigned by the native host sink.
            transport: String::new(), // Assigned by the host sink, never inferred from a thread key.
            sequence: 0,              // The authoritative host sink assigns transport order.
            revision: snapshot.revision,
            persistence,
            writes_pending: snapshot.writes_pending,
            reset: false,
        }
    }
}
