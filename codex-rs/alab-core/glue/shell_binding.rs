// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A resumed shell number carries no authority to interact with a new process.

use super::ProcessStore;
use super::UnifiedExecProcessManager;
use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolEnvironments;
use crate::tools::context::ToolInvocation;
use codex_history::InitialHistory;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[derive(PartialEq)]
enum Issuance {
    First,
    Reused,
}

#[derive(Default)]
pub(super) struct ShellBindings {
    restored: bool,
    numbers: HashMap<i32, Issuance>,
}

impl ShellBindings {
    pub(super) fn issue(&mut self, number: i32) {
        // A number first used in vanilla can reappear after a live mode change.
        if codex_alab_core::is_alab_binary() && !self.restored {
            self.record_issue(number);
        }
    }

    fn record_issue(&mut self, number: i32) {
        self.numbers
            .entry(number)
            .and_modify(|issuance| *issuance = Issuance::Reused)
            .or_insert(Issuance::First);
    }

    fn import_history(&mut self) {
        self.restored = true;
        self.numbers.clear();
    }

    fn accepts(&self, number: i32, supplied: Option<Uuid>, live: Option<Uuid>) -> bool {
        match (supplied, live) {
            (Some(supplied), Some(live)) => supplied == live,
            (None, Some(_)) => {
                !self.restored && self.numbers.get(&number) == Some(&Issuance::First)
            }
            (_, None) => false,
        }
    }
}

impl ProcessStore {
    /// A number remains exclusive from allocation through the owner's last release,
    /// including when pruning or bulk cleanup has removed its writable entry.
    pub(super) fn reserve_launch_number(&mut self, number: i32) -> Option<Arc<()>> {
        if self.processes.contains_key(&number)
            || self.in_flight_launches.contains_key(&number)
            || self.reserved_process_ids.contains(&number)
        {
            return None;
        }
        if super::process_manager::should_use_deterministic_process_ids() {
            self.reserved_process_ids.insert(number);
        }
        let owner = Arc::new(());
        self.in_flight_launches
            .insert(number, Arc::downgrade(&owner));
        Some(owner)
    }
}

impl UnifiedExecProcessManager {
    /// A yielded process keeps its handle while the model samples its recorded output.
    /// Only an active launch, collection or terminal interaction still owns an operation.
    pub(crate) async fn alab_terminal_operation_pending(&self) -> bool {
        let store = self.process_store.lock().await;
        store
            .in_flight_launches
            .values()
            .any(|lease| lease.strong_count() != 0)
            || store.processes.values().any(|entry| {
                entry
                    .initial_exec_command_active
                    .load(std::sync::atomic::Ordering::Acquire)
                    || entry.process.interaction_lock().try_lock_owned().is_err()
            })
    }

    #[cfg(test)]
    pub(crate) async fn allocate_process_id(&self) -> i32 {
        // Upstream fixtures reserve deterministic numbers outside a launch request.
        self.reserve_process_id().await.0
    }

    pub(crate) fn with_shell_history(mut self, history: &InitialHistory) -> Self {
        if matches!(
            history,
            InitialHistory::Resumed(_) | InitialHistory::Forked(_)
        ) {
            self.process_store.get_mut().shell_bindings.import_history();
        }
        self
    }

    pub(crate) async fn import_shell_history(&self) {
        self.process_store
            .lock()
            .await
            .shell_bindings
            .import_history();
    }
}

pub(crate) async fn terminal(
    invocation: &ToolInvocation,
    session_id: i32,
    supplied: Option<serde_json::Value>,
    max_tokens: Option<usize>,
) -> Result<ToolEnvironments, FunctionCallError> {
    let supplied: Option<Uuid> =
        supplied
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| {
                FunctionCallError::RespondToModel("invocation_id must be a UUID".to_owned())
            })?;
    let manager = &invocation.session.services.unified_exec_manager;
    if let Ok(terminal) = manager.resolve_terminal(session_id).await {
        let live = terminal.process.invocation_id;
        // Capture the terminal first. A number that is reissued during the
        // lookup becomes ambiguous. A reissue after admission is rejected by
        // write_stdin's existing check against the same Arc under its store lock.
        if manager
            .process_store
            .lock()
            .await
            .shell_bindings
            .accepts(session_id, supplied, live)
        {
            return Ok(ToolEnvironments::Terminal(terminal));
        }
    }
    Err(super::shell_observation::stale(invocation, supplied, session_id, max_tokens).await)
}

#[cfg(test)]
#[path = "shell_binding_tests.rs"]
mod tests;
