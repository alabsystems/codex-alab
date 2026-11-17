// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The real CLI checkout binding resource, independent of a frontend's memory.
//! An actual InitialSessionOperation must select its accepted START/FORK request.
//! This resource deliberately does not infer that selection from any first
//! thread, cwd, wire ID, metadata, nonce, or SDK role.
use codex_core::CodexThread;
use codex_core::ThreadManager;
use codex_core::ThreadRegistration;
use codex_protocol::ThreadId;
use codex_worktree::ManagedWorktree;
use codex_worktree::WorktreeManager;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

#[derive(Clone)]
pub struct StartupBinding(Arc<Inner>);
struct Inner {
    manager: WorktreeManager,
    checkout: ManagedWorktree,
    recovered: Arc<AtomicBool>,
    state: Mutex<State>,
}
enum State {
    Pending,
    Captured {
        id: ThreadId,
        actual: Arc<CodexThread>,
    },
    Completed {
        id: ThreadId,
        actual: Arc<CodexThread>,
        result: Result<(), String>,
    },
}
impl StartupBinding {
    /// Only the actual allocator hands these existing objects and the SAME
    /// recovery cell to the backend owner. No default/lookup/deserializer exists.
    pub fn from_allocated(
        manager: WorktreeManager,
        checkout: ManagedWorktree,
        recovered: Arc<AtomicBool>,
    ) -> Self {
        Self(Arc::new(Inner {
            manager,
            checkout,
            recovered,
            state: Mutex::new(State::Pending),
        }))
    }
    /// Capture the exact actual Core birth/resolution before any await or error.
    /// A repeated observer can see this same resource but cannot substitute a
    /// different incarnation, even under a reused logical thread ID.
    pub fn capture(&self, id: ThreadId, actual: Arc<CodexThread>) -> Result<(), String> {
        let mut state = self.0.state.lock().unwrap_or_else(PoisonError::into_inner);
        match &*state {
            State::Pending => {
                *state = State::Captured { id, actual };
                Ok(())
            }
            State::Captured {
                id: previous,
                actual: previous_actual,
            }
            | State::Completed {
                id: previous,
                actual: previous_actual,
                ..
            } if *previous == id && Arc::ptr_eq(previous_actual, &actual) => Ok(()),
            _ => Err("initial worktree already captured another actual runtime".into()),
        }
    }
    pub fn captured(&self) -> Option<(ThreadId, Arc<CodexThread>)> {
        match &*self.0.state.lock().unwrap_or_else(PoisonError::into_inner) {
            State::Pending => None,
            State::Captured { id, actual } | State::Completed { id, actual, .. } => {
                Some((*id, actual.clone()))
            }
        }
    }
    /// The accepted whole startup worker must own this future through outcome;
    /// dropping a UI observer does not own it. The caller retains its existing
    /// actual lifecycle serialization through this call and final publication.
    /// This checks Source registration only; it supplies no Native admission.
    pub async fn bind_captured(&self, registry: &ThreadManager) -> Result<(), String> {
        let (id, actual) = {
            let state = self.0.state.lock().unwrap_or_else(PoisonError::into_inner);
            match &*state {
                State::Pending => return Err("initial worktree has no actual Core result".into()),
                State::Completed { result, .. } => return result.clone(),
                State::Captured { id, actual } => (*id, actual.clone()),
            }
        };
        if actual.has_shutdown_status()
            || !matches!(
                registry.thread_registration(id, &actual).await,
                ThreadRegistration::Current
            )
            || actual.has_shutdown_status()
        {
            return Err("initial worktree runtime withdrew before binding".into());
        }
        let mut state = self.0.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let State::Completed { result, .. } = &*state {
            return result.clone();
        }
        // Exact capture remains in state during this original synchronous
        // metadata operation. A failure is sticky; no observation resubmits it.
        let result = self
            .0
            .manager
            .bind_thread(&self.0.checkout.root, &id.to_string())
            .map_err(|error| error.to_string());
        if result.is_ok() {
            self.0.recovered.store(true, Ordering::Relaxed);
        }
        *state = State::Completed {
            id,
            actual,
            result: result.clone(),
        };
        result
    }
}
