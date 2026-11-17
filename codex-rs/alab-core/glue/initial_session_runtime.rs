// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real processor/operation association. Runtime owns the strong operation;
//! the processor stores a weak slot, avoiding a self-cycle or frontend owner.
use super::*;
use crate::in_process::InitialSessionPlan;
use crate::request_processors::InitialSessionOperation;
use codex_alab_core::native_incoming::Incoming;

impl MessageProcessor {
    pub(crate) fn alab_make_initial_session(
        self: &Arc<Self>,
        plan: InitialSessionPlan,
        input: Incoming,
    ) -> (Arc<InitialSessionOperation>, std::io::Result<()>) {
        let actual =
            InitialSessionOperation::new(plan, input, Arc::new(self.thread_processor.clone()));
        let installed = self.thread_processor.alab_install_initial_session(&actual);
        (actual, installed)
    }
    pub(crate) fn alab_make_initial_session_bound(
        self: &Arc<Self>,
        plan: InitialSessionPlan,
        bound: codex_alab_aterm::frontend_control::retained_service::BoundBackend,
    ) -> (Arc<InitialSessionOperation>, std::io::Result<()>) {
        let gate = self
            .alab_bind_retained_goal_gate(bound.goal_gate())
            .map_err(std::io::Error::other);
        let actual = InitialSessionOperation::new_bound(
            plan,
            bound,
            Arc::new(self.thread_processor.clone()),
        );
        let installed =
            gate.and_then(|()| self.thread_processor.alab_install_initial_session(&actual));
        (actual, installed)
    }
    pub(crate) fn alab_stop_initial_session(&self) {
        self.thread_processor.alab_stop_initial_session();
    }
}
