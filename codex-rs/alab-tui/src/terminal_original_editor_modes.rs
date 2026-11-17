// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
// SAME original mode owner and SAME pre-child Prepared. No Native admission.
use codex_alab_aterm::frontend_control::retained_service::original_editor::BeforeStart;
use codex_alab_aterm::frontend_control::retained_service::original_editor::CancelledBeforeStart;
use codex_alab_aterm::frontend_control::retained_service::original_editor::ExecFence;
use codex_alab_aterm::frontend_control::retained_service::original_editor::Finished;
use codex_alab_aterm::frontend_control::retained_service::original_editor::Operation;
use std::os::fd::AsFd;
use std::os::fd::OwnedFd;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;
const HELD: u8 = 0;
const RESTORING: u8 = 1;
const RESTORED: u8 = 2;
const UNKNOWN: u8 = 3;
pub(super) struct EditorModeState {
    operation: Arc<Operation>,
    source: Arc<Mutex<Prepared>>,
    terminal: Mutex<Option<File>>,
    staged: Mutex<Option<Arc<C0ModesStaged>>>,
    phase: AtomicU8,
}
/// Restrict-only real TTY owner. Global state independently retains it on Drop.
/// Actual Native escrow/currentness/Go and the parser resource stay separate.
pub struct EditorModes {
    actual: Arc<EditorModeState>,
}
impl EditorModes {
    pub fn is_for(&self, operation: &Operation) -> bool {
        self.actual.operation.same(operation)
    }
    fn check(&self, state: &State, source: &Prepared, terminal: &File) -> io::Result<()> {
        if !state
            .editor
            .as_ref()
            .is_some_and(|actual| Arc::ptr_eq(actual, &self.actual))
            || self.actual.phase.load(Ordering::Acquire) != HELD
        {
            return Err(invalid(
                "original editor mode handback is absent or uncertain",
            ));
        }
        let original = state
            .original
            .as_ref()
            .ok_or_else(|| invalid("actual original mode owner absent"))?;
        if Identity::read(terminal)? != source.snapshot.terminal
            || Identity::read(&original.terminal)? != source.snapshot.terminal
            || Mode::capture(&original.mode) != source.snapshot.original
        {
            return Err(invalid("original editor mode source changed"));
        }
        Ok(())
    }
    pub fn terminal_description(&self) -> io::Result<OwnedFd> {
        let _custody = codex_alab_core::native_fd_custody::receiving()?;
        let source = self
            .actual
            .source
            .lock()
            .map_err(|_| invalid("original mode preparation poisoned"))?;
        let state = ORIGINAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let terminal = self
            .actual
            .terminal
            .lock()
            .map_err(|_| invalid("original editor terminal poisoned"))?;
        let terminal = terminal
            .as_ref()
            .ok_or_else(|| invalid("original editor terminal staged or retired"))?;
        self.check(&state, &source, terminal)?;
        terminal.as_fd().try_clone_to_owned()
    }
    /// The editor's current modes may differ from the old parent raw modes.
    /// This returns old parent DATA only under its actual root/read fence.
    pub fn snapshot_for_exec(
        &self,
        fence: &ExecFence,
        input: &codex_alab_input::EditorFence,
    ) -> io::Result<Snapshot> {
        if !self.actual.operation.is_for_fence(fence) {
            return Err(invalid("original editor reap fence changed"));
        }
        let input_identity = input.terminal_identity()?;
        let source = self
            .actual
            .source
            .lock()
            .map_err(|_| invalid("original mode preparation poisoned"))?;
        let state = ORIGINAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let terminal = self
            .actual
            .terminal
            .lock()
            .map_err(|_| invalid("original editor terminal poisoned"))?;
        let terminal = terminal
            .as_ref()
            .ok_or_else(|| invalid("original editor terminal staged or retired"))?;
        self.check(&state, &source, terminal)?;
        let terminal = &source.snapshot.terminal;
        // The actual editor owner is the boundary after pre-child admission.
        // A refused generic setter can revoke the earlier idle preparation
        // without changing these held resources. Never accept an irreversible
        // transfer, even though idle freshness no longer describes this phase.
        if source.transfer.committed()
            || admission().committed()
            || input_identity != (terminal.device, terminal.inode, terminal.rdevice)
        {
            return Err(invalid(
                "original parser/mode resource changed before editor capture",
            ));
        }
        Ok(source.snapshot.clone())
    }
    /// Borrow the exact owner on refusal. Only a real result of this SAME
    /// original root wait/document read can restore parent modes. The caller
    /// must first restore actual parent stdio and pair Native handback if it was
    /// escrowed; generic mode setters never release this restriction.
    pub fn release_for_original_handback(&self, finished: &Finished) -> io::Result<()> {
        if !finished.is_for(&self.actual.operation) {
            return Err(invalid(
                "editor finish belongs to another original operation",
            ));
        }
        self.release_parent_running()
    }
    /// Only exact definite pre-start cancellation permits local rollback
    /// without a Finished or a fabricated child/root outcome.
    pub fn release_for_cancelled_handback(
        &self,
        cancelled: &CancelledBeforeStart,
    ) -> io::Result<()> {
        if !cancelled.is_for(&self.actual.operation) {
            return Err(invalid(
                "cancelled modes belong to another original operation",
            ));
        }
        self.release_parent_running()
    }
    fn release_parent_running(&self) -> io::Result<()> {
        let source = self
            .actual
            .source
            .lock()
            .map_err(|_| invalid("original mode preparation poisoned"))?;
        let mut state = ORIGINAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let terminal = self
            .actual
            .terminal
            .lock()
            .map_err(|_| invalid("original editor terminal poisoned"))?;
        let terminal = terminal
            .as_ref()
            .ok_or_else(|| invalid("original editor terminal staged or retired"))?;
        self.check(&state, &source, terminal)?;
        // Validate everything before marking mutation possible. The source raw
        // cache survived its child's changes; re-establish its exact saved
        // parent-running modes before the original Tui post-editor setters.
        let running = source.snapshot.running.decode(read(terminal)?)?;
        self.actual.phase.store(RESTORING, Ordering::Release);
        let mut attempt = Handback {
            actual: self.actual.clone(),
            complete: false,
        };
        // SAFETY: SAME held original TTY and fully validated initialized modes;
        // actual original root completion or typed no-child cancellation
        // precedes this borrowed handback.
        if unsafe { libc::tcsetattr(terminal.as_raw_fd(), libc::TCSANOW, &running) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let retired = state.editor.take();
        self.actual.phase.store(RESTORED, Ordering::Release);
        attempt.complete = true;
        drop(state);
        drop(retired);
        Ok(())
    }
}
/// Under the SAME pre-child start gate and actual stopped parser, install the
/// mode restriction before any Child can receive the TTY. Perform no terminal
/// mutation. The preparation is the SAME actual source captured before start.
/// Any refusal keeps that exact source Prepared and its actual operation owned.
pub fn hold_original_editor(
    operation: &Arc<Operation>,
    before: &BeforeStart<'_>,
    input: &crate::input::OriginalEditorInput,
    source: &Arc<Mutex<Prepared>>,
) -> io::Result<EditorModes> {
    if !crate::input::owned() || !before.is_for(operation) || !input.is_for(operation) {
        return Err(invalid(
            "original editor mode custody requires its actual source operation",
        ));
    }
    let _custody = codex_alab_core::native_fd_custody::receiving()?;
    // Read the sticky parser description before taking either mode lock.
    let input_identity = input.fence().terminal_identity()?;
    let prepared = source
        .lock()
        .map_err(|_| invalid("original mode preparation poisoned"))?;
    let mut state = ORIGINAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.editor.is_some()
        || !prepared.transfer.belongs_to(admission())
        || !prepared.transfer.valid()
        || !matches(&state, &prepared.snapshot)?
    {
        return Err(invalid(
            "original mode source is already transferring or replaced",
        ));
    }
    let original = state
        .original
        .as_ref()
        .ok_or_else(|| invalid("actual original terminal mode absent"))?;
    let terminal = &prepared.snapshot.terminal;
    if input_identity != (terminal.device, terminal.inode, terminal.rdevice)
        || Identity::read(&original.terminal)? != prepared.snapshot.terminal
        || Mode::capture(&original.mode) != prepared.snapshot.original
    {
        return Err(invalid(
            "pre-child mode preparation changed its original owner",
        ));
    }
    let actual = Arc::new(EditorModeState {
        operation: operation.clone(),
        source: source.clone(),
        terminal: Mutex::new(Some(original.terminal.try_clone()?)),
        staged: Mutex::new(None),
        phase: AtomicU8::new(HELD),
    });
    state.editor = Some(actual.clone());
    Ok(EditorModes { actual })
}
/// Partial pre-child admission may have stopped the actual parser before mode
/// admission refused stale running modes. Restore the SAME genuine parent's
/// saved running modes only after exact local no-child cancellation, before
/// original Tui post-policy and that parser's actual before_resume handback.
/// This borrows every owner on all refusals; it grants no Native/Go authority.
pub fn restore_cancelled_original_editor(
    operation: &Operation,
    cancelled: &CancelledBeforeStart,
    input: &crate::input::OriginalEditorInput,
    source: &Arc<Mutex<Prepared>>,
) -> io::Result<()> {
    if !crate::input::owned() || !cancelled.is_for(operation) || !input.is_for(operation) {
        return Err(invalid(
            "partial editor mode rollback changed its original operation",
        ));
    }
    let input_identity = input.fence().terminal_identity()?;
    let prepared = source
        .lock()
        .map_err(|_| invalid("original mode preparation poisoned"))?;
    let state = ORIGINAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Rollback is not execution admission: an interrupted uncommitted token
    // remains the SAME native source owner. A committed transfer or another
    // actual editor must never be reopened by local cancellation.
    if state.editor.is_some()
        || !prepared.transfer.belongs_to(admission())
        || prepared.transfer.committed()
        || admission().committed()
    {
        return Err(invalid(
            "partial editor modes already transferred or replaced",
        ));
    }
    let original = state
        .original
        .as_ref()
        .ok_or_else(|| invalid("actual original terminal mode absent"))?;
    let terminal = &prepared.snapshot.terminal;
    if input_identity != (terminal.device, terminal.inode, terminal.rdevice)
        || Identity::read(&original.terminal)? != *terminal
        || Mode::capture(&original.mode) != prepared.snapshot.original
    {
        return Err(invalid(
            "partial editor mode rollback changed its actual TTY or original owner",
        ));
    }
    let running = prepared
        .snapshot
        .running
        .decode(read(&original.terminal)?)?;
    // SAFETY: SAME original TTY and initialized captured parent-running modes;
    // exact cancelled receipt proves THIS operation has never started a child.
    if unsafe { libc::tcsetattr(original.terminal.as_raw_fd(), libc::TCSANOW, &running) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // The parent parser remains sticky on any post-mutation uncertainty.
    if Mode::capture(&read(&original.terminal)?) != prepared.snapshot.running {
        return Err(invalid(
            "actual parent running modes changed during cancelled handback",
        ));
    }
    Ok(())
}
struct Handback {
    actual: Arc<EditorModeState>,
    complete: bool,
}
impl Drop for Handback {
    fn drop(&mut self) {
        if !self.complete {
            self.actual.phase.store(UNKNOWN, Ordering::Release);
        }
    }
}

// C0 staging moves the originals; unlike terminal_description it never duplicates.
include!("terminal_original_editor_stage.rs");
