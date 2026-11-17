// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! SAME actual parser through the original editor. No Native/Go authority.
use super::*;
use codex_alab_aterm::frontend_control::retained_service::original_editor::BeforeStart;
use codex_alab_aterm::frontend_control::retained_service::original_editor::CancelledBeforeStart;
use codex_alab_aterm::frontend_control::retained_service::original_editor::ExecFence;
use codex_alab_aterm::frontend_control::retained_service::original_editor::Finished;
use codex_alab_aterm::frontend_control::retained_service::original_editor::Operation;
use std::os::fd::OwnedFd;

pub struct OriginalEditorInput {
    actual: codex_alab_input::EditorFence,
    operation: Arc<Operation>,
}
impl OriginalEditorInput {
    /// After original Tui pause, synchronously stop/join even surviving actual
    /// query subscriptions BEFORE child start. The SAME caller-held start gate
    /// must survive mode installation and actual Job publication; no await.
    pub fn prepare_before_original(
        operation: &Arc<Operation>,
        before: &BeforeStart<'_>,
    ) -> io::Result<Self> {
        if !owned() || !before.is_for(operation) {
            return Err(io::Error::other(
                "original editor input requires SAME actual source operation",
            ));
        }
        Ok(Self {
            actual: broker().prepare_editor()?,
            operation: operation.clone(),
        })
    }
    pub fn is_for(&self, operation: &Operation) -> bool {
        self.operation.same(operation)
    }
    /// Definite local no-child rollback only, from original Tui before_resume
    /// AFTER its actual post-policy. Refusal keeps this exact parser owner.
    pub fn finish_after_cancelled_handoff(
        &mut self,
        cancelled: &CancelledBeforeStart,
    ) -> io::Result<()> {
        if !cancelled.is_for(&self.operation) {
            return Err(io::Error::other(
                "cancelled parser belongs to another editor",
            ));
        }
        self.actual.finish_with(|_same_original_input| Ok(()))
    }
    pub fn snapshot_for_exec(&self, fence: &ExecFence) -> io::Result<&[u8]> {
        if !self.operation.is_for_fence(fence) {
            return Err(io::Error::other("original parser reap fence changed"));
        }
        let _same_terminal = self.actual.terminal_identity()?;
        Ok(self.actual.snapshot())
    }
    pub fn terminal_description(&self) -> io::Result<OwnedFd> {
        let _custody = codex_alab_core::native_fd_custody::receiving()?;
        self.actual.terminal_description()
    }
    /// Readonly reference only. Mode correlation never consumes/unfreezes it.
    pub fn fence(&self) -> &codex_alab_input::EditorFence {
        &self.actual
    }
    /// Invoke from original Tui handoff's before_resume closure, AFTER original
    /// parent stdio/modes/output/FD0 TCIFLUSH. Never clear saved parser bytes or
    /// perform a second flush against another terminal description.
    pub fn finish_after_original_handoff(&mut self, finished: &Finished) -> io::Result<()> {
        if !finished.is_for(&self.operation) {
            return Err(io::Error::other(
                "original parser finish belongs to another editor",
            ));
        }
        self.actual.finish_with(|_same_original_input| Ok(()))
    }
}

/// Every accepted pre-child wrapper remains owned on partial refusal. The
/// caller MUST publish this exact bundle (including a refused partial bundle)
/// into its actual Job before releasing the borrowed BeforeStart gate.
pub struct OriginalEditorResources {
    input: Option<OriginalEditorInput>,
    modes: Option<crate::terminal_modes::EditorModes>,
}
pub struct OriginalEditorResourcesRefused {
    pub actual: OriginalEditorResources,
    pub error: io::Error,
}
impl OriginalEditorResources {
    pub fn prepare(
        operation: &Arc<Operation>,
        before: &BeforeStart<'_>,
        source: &Arc<std::sync::Mutex<crate::terminal_modes::Prepared>>,
    ) -> Result<Self, Box<OriginalEditorResourcesRefused>> {
        let mut actual = Self {
            input: None,
            modes: None,
        };
        let result: io::Result<()> = (|| {
            actual.input = Some(OriginalEditorInput::prepare_before_original(
                operation, before,
            )?);
            let input = actual
                .input
                .as_ref()
                .ok_or_else(|| io::Error::other("original parser owner absent"))?;
            actual.modes = Some(crate::terminal_modes::hold_original_editor(
                operation, before, input, source,
            )?);
            Ok(())
        })();
        match result {
            Ok(()) => Ok(actual),
            Err(error) => Err(Box::new(OriginalEditorResourcesRefused { actual, error })),
        }
    }
    pub fn input(&self) -> Option<&OriginalEditorInput> {
        self.input.as_ref()
    }
    pub fn input_mut(&mut self) -> Option<&mut OriginalEditorInput> {
        self.input.as_mut()
    }
    pub fn modes(&self) -> Option<&crate::terminal_modes::EditorModes> {
        self.modes.as_ref()
    }
}

// C0 original-FD staging is separate from same-process Finished handback.
include!("retained_original_editor_stage.rs");
