// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Join several independent original shutdown decisions at one manager commit.
use super::LocalAgentRuntime;
use super::TreeDisposition;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::PoisonError;

#[derive(Default, Debug)]
pub(super) struct Disposition {
    value: OnceLock<TreeDisposition>,
    selecting: Mutex<()>,
}

impl Disposition {
    pub(super) fn get(&self) -> Option<&TreeDisposition> {
        self.value.get()
    }

    pub(super) fn get_or_init(
        &self,
        initialize: impl FnOnce() -> TreeDisposition,
    ) -> &TreeDisposition {
        if let Some(decided) = self.get() {
            return decided;
        }
        let _selection = self
            .selecting
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.value.get_or_init(initialize)
    }
}

impl LocalAgentRuntime {
    // No mutation until the caller actually commits. Dropping a forest's
    // selections reopens every original disposition, including earlier roots.
    pub(crate) fn prepare_exec_result_shutdown(&self) -> Option<impl FnOnce() + '_> {
        let disposition = &self.shutdown_state.disposition;
        let selection = disposition
            .selecting
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if disposition.get().is_some() {
            return None;
        }
        Some(move || {
            disposition.value.get_or_init(|| TreeDisposition::Shutdown);
            drop(selection);
        })
    }

    pub(crate) fn reserve_exec_result_shutdown(&self) -> bool {
        self.prepare_exec_result_shutdown().is_some_and(|commit| {
            commit();
            true
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::control::LocalAgentControl;

    #[test]
    fn abandoned_shutdown_selection_reopens_before_the_single_commit() {
        let runtime = LocalAgentControl::default().runtime;
        let selection = runtime.prepare_exec_result_shutdown().unwrap();
        assert!(runtime.shutdown_state.disposition.get().is_none());
        drop(selection);
        assert!(runtime.shutdown_state.disposition.get().is_none());
        assert!(runtime.reserve_exec_result_shutdown());
        assert!(matches!(
            runtime.shutdown_state.disposition.get(),
            Some(TreeDisposition::Shutdown)
        ));
        assert!(runtime.prepare_exec_result_shutdown().is_none());
    }
}
