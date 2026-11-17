// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Operation-specific original parent handback. The shared native census owns
//! exact FD0/1/2 and saved-stderr duplicates; it grants no writer retirement.
use super::TerminalStderrGuard;
use codex_alab_tui::frontend_control::retained_service::original_editor::ExecFence;
use codex_alab_tui::frontend_control::retained_service::original_editor::Finished;
use codex_alab_tui::frontend_control::retained_service::original_editor::Operation;
use std::io;
use std::os::fd::AsRawFd;
use std::os::fd::BorrowedFd;
use std::sync::Arc;

pub(crate) struct Captured {
    operation: Arc<Operation>,
    actual: super::alab_retained_stdio::Captured,
}
pub(super) fn check_original_writer()
-> io::Result<codex_alab_tui::terminal_modes::ParentStdioWriter> {
    codex_alab_tui::terminal_modes::parent_stdio_writer()
}

impl Captured {
    pub(crate) fn capture(
        actual: &TerminalStderrGuard,
        operation: &Arc<Operation>,
        modes: &codex_alab_tui::terminal_modes::EditorModes,
        fence: &ExecFence,
        input: &codex_alab_tui::input::OriginalEditorInput,
    ) -> io::Result<Arc<Self>> {
        if !operation.is_for_fence(fence) || !codex_alab_tui::terminal_modes::editor_owns_terminal()
        {
            return Err(io::Error::other(
                "original stdio capture changed its real editor owner",
            ));
        }
        let _input = input.snapshot_for_exec(fence)?;
        modes.snapshot_for_exec(fence, input.fence())?;
        // The genuine shared factory clones under SAME global stderr lock and
        // FD exclusion, always above 0/1/2 so a closed standard is never filled
        // accidentally by an earlier clone. No duplicate census implementation.
        let actual = super::alab_retained_stdio::Captured::capture(actual)?;
        Ok(Arc::new(Self {
            operation: operation.clone(),
            actual,
        }))
    }
    pub(crate) fn parent_descriptors(&self) -> [BorrowedFd<'_>; 3] {
        self.actual.parent_descriptors()
    }
    pub(crate) fn saved_stderr(&self) -> Option<BorrowedFd<'_>> {
        self.actual.saved_stderr()
    }
    pub(crate) fn stderr_state_data(&self) -> (bool, bool) {
        self.actual.stderr_state_data()
    }
    /// Only SAME actual active App Job's callback calls this, before releasing
    /// its mode owner. Distinct-C2/C0 handback is a separate Native claim and
    /// cannot be inferred from this process-local restoration method.
    pub(crate) fn restore_original_parent(&self, actual: &Finished) -> io::Result<()> {
        self.actual.check_process()?;
        if !actual.is_for(&self.operation)
            || !codex_alab_tui::terminal_modes::editor_owns_terminal()
        {
            return Err(io::Error::other(
                "original parent stdio handback changed real owner",
            ));
        }
        #[cfg(target_os = "macos")]
        let state = super::lock_state()?;
        #[cfg(target_os = "macos")]
        if state.owner_active != self.actual.stderr_state_data().0
            || state.saved_stderr.is_some() != self.actual.saved_stderr().is_some()
        {
            return Err(io::Error::other(
                "original global stderr changed during editor custody",
            ));
        }
        for (source, target) in self.actual.parent_descriptors().into_iter().zip([
            libc::STDIN_FILENO,
            libc::STDOUT_FILENO,
            libc::STDERR_FILENO,
        ]) {
            // SAFETY: SAME owned census descriptions remain live; any partial
            // refusal retains this exact object in the globally registered Job.
            if unsafe { libc::dup2(source.as_raw_fd(), target) } < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
}
