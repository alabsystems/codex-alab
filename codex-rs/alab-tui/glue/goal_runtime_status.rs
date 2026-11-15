// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Typed host/generation ordering is scoped to the authenticated backend. A
//! saved goal remains Active; passive metadata grants no execution authority.

use super::ChatWidget;
use crate::backend_id::BackendId;
use codex_app_server_protocol::AlabGoalPersistencePhase;
use codex_app_server_protocol::AlabGoalRuntimeUpdatedNotification;
use std::collections::HashSet;
use std::sync::Mutex;

#[derive(Clone)]
pub(crate) struct StreamFrontier {
    host: Option<String>,
    generation: u64,
    retired_hosts: HashSet<String>,
    replacement_allowed: bool,
}
impl Default for StreamFrontier {
    fn default() -> Self {
        Self {
            host: None,
            generation: 0,
            retired_hosts: HashSet::new(),
            replacement_allowed: true,
        }
    }
}
impl StreamFrontier {
    pub(crate) fn admit(&mut self, update: &AlabGoalRuntimeUpdatedNotification) -> bool {
        if update.transport.is_empty() || update.sequence == 0 {
            return false;
        }
        if self.host.as_deref() == Some(update.transport.as_str()) {
            if update.generation < self.generation {
                return false;
            }
            self.generation = update.generation;
            self.replacement_allowed = false;
            return true;
        }
        if self.retired_hosts.contains(&update.transport)
            || (!self.replacement_allowed && self.host.is_some())
        {
            return false;
        }
        if let Some(previous) = self.host.replace(update.transport.clone()) {
            self.retired_hosts.insert(previous);
        }
        self.generation = update.generation;
        self.replacement_allowed = false;
        true
    }
    pub(crate) fn reset(&mut self, update: &AlabGoalRuntimeUpdatedNotification) {
        if self.host.as_deref() == Some(update.transport.as_str()) {
            self.generation = self.generation.max(update.generation.saturating_add(1));
        } else if self.host.is_none() || self.replacement_allowed {
            if self.retired_hosts.contains(&update.transport) {
                return;
            }
            if let Some(previous) = self.host.replace(update.transport.clone()) {
                self.retired_hosts.insert(previous);
            }
            self.generation = update.generation.saturating_add(1);
            self.replacement_allowed = false;
        } else {
            self.retired_hosts.insert(update.transport.clone());
        }
    }
    pub(crate) fn permits(&self, update: &AlabGoalRuntimeUpdatedNotification) -> bool {
        self.host.as_deref() == Some(update.transport.as_str())
            && update.generation >= self.generation
    }
    pub(crate) fn host_matches(&self, host: &str) -> bool {
        self.host.as_deref() == Some(host)
    }
    pub(crate) fn disconnected(&mut self) {
        self.replacement_allowed = true;
    }
}

#[derive(Default)]
pub(crate) struct State(Mutex<Inner>);
#[derive(Default)]
struct Inner {
    backend: Option<BackendId>,
    thread: Option<String>,
    sequence: u64,
    stream: StreamFrontier,
    observation: Option<AlabGoalRuntimeUpdatedNotification>,
}
impl State {
    pub(crate) fn bind(&self, backend: BackendId, thread: Option<&str>) {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inner.backend != Some(backend) || inner.thread.as_deref() != thread {
            *inner = Inner {
                backend: Some(backend),
                thread: thread.map(str::to_owned),
                ..Inner::default()
            };
        }
    }
    pub(crate) fn forget(&self, backend: BackendId) -> bool {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.backend == Some(backend) && inner.observation.take().is_some()
    }
    pub(crate) fn disconnected(&self, backend: BackendId) {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inner.backend == Some(backend) {
            inner.observation = None;
            inner.stream.disconnected();
        }
    }
    pub(crate) fn constrain(&self, frontier: &StreamFrontier) -> bool {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inner
            .observation
            .as_ref()
            .is_some_and(|update| !frontier.permits(update))
        {
            return inner.observation.take().is_some();
        }
        false
    }
    pub(crate) fn reset(
        &self,
        backend: BackendId,
        update: &AlabGoalRuntimeUpdatedNotification,
    ) -> bool {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inner.backend != Some(backend) {
            return false;
        }
        inner.stream.reset(update);
        if inner
            .observation
            .as_ref()
            .is_some_and(|previous| !inner.stream.permits(previous))
        {
            return inner.observation.take().is_some();
        }
        false
    }
    pub(crate) fn current(&self) -> Option<AlabGoalRuntimeUpdatedNotification> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .observation
            .clone()
    }
    pub(crate) fn observe(
        &self,
        backend: BackendId,
        update: &AlabGoalRuntimeUpdatedNotification,
    ) -> bool {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inner.backend != Some(backend)
            || inner.thread.as_deref() != Some(update.thread_id.as_str())
        {
            return false;
        }
        let previous_host = inner.stream.host.clone();
        let previous_generation = inner.stream.generation;
        if !inner.stream.admit(update) {
            return false;
        }
        if previous_host.as_deref() != Some(update.transport.as_str())
            || previous_generation != update.generation
        {
            inner.sequence = 0;
            inner.observation = None;
        }
        if update.sequence <= inner.sequence {
            return false;
        }
        inner.sequence = update.sequence;
        if inner.observation.as_ref().is_some_and(|previous| {
            previous.incarnation == update.incarnation && previous.revision > update.revision
        }) {
            return inner.observation.take().is_some();
        }
        inner.observation = Some(update.clone());
        true
    }
    fn settled_hold(&self, thread: &str) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .observation
            .as_ref()
            .is_some_and(|update| {
                update.thread_id == thread
                    && update.persistence == Some(AlabGoalPersistencePhase::Held)
                    && !update.writes_pending
            })
    }
}
impl ChatWidget {
    pub(crate) fn alab_bind_goal_runtime_backend(&self, backend: BackendId) {
        let thread = self.thread_id.map(|thread| thread.to_string());
        self.alab_goal_runtime.bind(backend, thread.as_deref());
    }
    pub(crate) fn alab_forget_goal_runtime_backend(&self, backend: BackendId) {
        if self.alab_goal_runtime.forget(backend) {
            self.request_redraw();
        }
    }
    pub(crate) fn alab_goal_runtime_transport_lost(&self, backend: BackendId) {
        self.alab_goal_runtime.disconnected(backend);
        self.request_redraw();
    }
    pub(crate) fn alab_observe_goal_runtime(&self, update: &AlabGoalRuntimeUpdatedNotification) {
        let backend = self
            .alab_goal_runtime
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .backend;
        let Some(backend) = backend else {
            return;
        };
        if update.reset {
            if self.alab_goal_runtime.reset(backend, update) {
                self.request_redraw();
            }
            return;
        }
        if self
            .thread_id
            .is_some_and(|thread| thread.to_string() == update.thread_id)
            && self.alab_goal_runtime.observe(backend, update)
        {
            self.request_redraw();
        }
    }
    pub(crate) fn alab_goal_counts_as_work(&self) -> bool {
        self.current_goal_status
            .as_ref()
            .is_some_and(super::GoalStatusState::is_active)
            && (!codex_alab_core::active()
                || !self
                    .thread_id
                    .is_some_and(|thread| self.alab_goal_runtime.settled_hold(&thread.to_string()))
                || !self.unified_exec_processes.is_empty())
    }
}
