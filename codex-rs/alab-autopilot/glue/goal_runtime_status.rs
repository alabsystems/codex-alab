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
use tokio::sync::watch;

type Registry = Mutex<HashMap<String, Weak<GoalRuntimeHandle>>>;

pub(super) struct Owner {
    incarnation: String,
    registration: Mutex<Option<Registration>>,
}

impl Default for Owner {
    fn default() -> Self {
        Self {
            incarnation: codex_alab_core::goal_persistence::observation_incarnation(),
            registration: Mutex::default(),
        }
    }
}

struct Registration {
    registry: Weak<Registry>,
    cancel: watch::Sender<bool>,
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.cancel.send_replace(true);
    }
}

impl GoalRuntimeHandle {
    pub(crate) fn alab_watch_runtime_status(self: &Arc<Self>, registry: Weak<Registry>) {
        if !codex_alab_core::active() {
            return;
        }
        let mut registration = self
            .inner
            .alab_status
            .registration
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if registration
            .as_ref()
            .is_some_and(|current| current.registry.ptr_eq(&registry))
        {
            return;
        }
        let (cancel, mut canceled) = watch::channel(false);
        *registration = Some(Registration { registry, cancel });
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

    /// Called only by matching native unregister, while its registry lock stands.
    pub(crate) fn alab_retire_runtime_status(&self) {
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
