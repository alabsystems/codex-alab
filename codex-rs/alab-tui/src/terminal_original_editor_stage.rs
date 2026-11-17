// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

// SAME original mode Files staged for C0 transfer. No FD/path/PID DATA can
// construct this resource. The original global owner pins every staged File;
// observation/drop/transport loss is not acceptance, NoIO or Native Go.
use codex_alab_aterm::frontend_control::retained_service::original_editor::C0Capture;
use codex_alab_aterm::frontend_control::retained_service::original_editor::C0SpawnCompleted;
use std::os::fd::BorrowedFd;
use std::sync::Weak;
const C0_STAGED: u8 = 4;
const C0_OFFERED: u8 = 5;
struct C0ModeFiles {
    original: Original,
    terminal: File,
}
struct C0ModesStaged {
    owner: Weak<EditorModeState>,
    files: Mutex<Option<C0ModeFiles>>,
    snapshot: Snapshot,
}
pub struct EditorModesStage {
    owner: Arc<EditorModeState>,
    actual: Arc<C0ModesStaged>,
}
pub struct EditorModesStageRefused {
    pub original: EditorModes,
    pub error: io::Error,
}
pub struct EditorModesStageRollbackRefused {
    pub staged: EditorModesStage,
    pub error: io::Error,
}

impl EditorModes {
    /// Synchronous original-resource move after SAME actual spawn completed.
    /// It does not mutate modes or retire descriptors: these originals remain
    /// open and globally pinned until actual authenticated custody is accepted.
    pub fn stage_original_modes(
        self,
        capture: &C0Capture,
        spawn: &C0SpawnCompleted,
        input: &crate::input::OriginalEditorInputStage,
    ) -> Result<EditorModesStage, Box<EditorModesStageRefused>> {
        let result = (|| {
            if !capture.is_for(&self.actual.operation)
                || !spawn.is_for(capture)
                || !spawn.is_for_operation(&self.actual.operation)
                || !input.is_for(&self.actual.operation)
            {
                return Err(invalid(
                    "C0 mode stage changed original operation or completed spawn",
                ));
            }
            let input_identity = input.terminal_identity()?;
            let source = self
                .actual
                .source
                .lock()
                .map_err(|_| invalid("original mode preparation poisoned"))?;
            let mut state = ORIGINAL
                .lock()
                .map_err(|_| invalid("original mode owner poisoned"))?;
            let mut terminal = self
                .actual
                .terminal
                .lock()
                .map_err(|_| invalid("original editor terminal poisoned"))?;
            let mut pinned = self
                .actual
                .staged
                .lock()
                .map_err(|_| invalid("original mode stage poisoned"))?;
            let same_terminal = terminal
                .as_ref()
                .ok_or_else(|| invalid("original editor terminal absent"))?;
            self.check(&state, &source, same_terminal)?;
            let expected = &source.snapshot.terminal;
            if input_identity != (expected.device, expected.inode, expected.rdevice)
                || pinned.is_some()
                || !source.transfer.belongs_to(admission())
                || source.transfer.committed()
                || admission().committed()
            {
                return Err(invalid(
                    "C0 mode stage changed parser, original owner or transfer",
                ));
            }
            let snapshot = source.snapshot.clone();
            // Every refusal/lock precedes removal of either original File.
            let original = state.original.take();
            let held = terminal.take();
            let (original, held) = match (original, held) {
                (Some(original), Some(held)) => (original, held),
                (original, held) => {
                    state.original = original;
                    *terminal = held;
                    return Err(invalid("original mode Files disappeared before staging"));
                }
            };
            let actual = Arc::new(C0ModesStaged {
                owner: Arc::downgrade(&self.actual),
                files: Mutex::new(Some(C0ModeFiles {
                    original,
                    terminal: held,
                })),
                snapshot,
            });
            *pinned = Some(actual.clone());
            self.actual.phase.store(C0_STAGED, Ordering::Release);
            Ok(EditorModesStage {
                owner: self.actual.clone(),
                actual,
            })
        })();
        match result {
            Ok(actual) => Ok(actual),
            Err(error) => Err(Box::new(EditorModesStageRefused {
                original: self,
                error,
            })),
        }
    }
}
impl EditorModesStage {
    fn check_global(&self, state: &State) -> io::Result<()> {
        if !state
            .editor
            .as_ref()
            .is_some_and(|actual| Arc::ptr_eq(actual, &self.owner))
            || state.original.is_some()
            || !self
                .actual
                .owner
                .upgrade()
                .is_some_and(|actual| Arc::ptr_eq(&actual, &self.owner))
        {
            return Err(invalid("staged mode original owner changed"));
        }
        let pinned = self
            .owner
            .staged
            .lock()
            .map_err(|_| invalid("original mode stage poisoned"))?;
        if !pinned
            .as_ref()
            .is_some_and(|actual| Arc::ptr_eq(actual, &self.actual))
        {
            return Err(invalid("original mode stage publication changed"));
        }
        Ok(())
    }
    pub fn is_for(&self, operation: &Operation) -> bool {
        self.owner.operation.same(operation)
    }
    pub fn snapshot(&self) -> &Snapshot {
        &self.actual.snapshot
    }
    /// Mark ALL combined-offer members before any original FD is exposed.
    /// This restriction never says the Native peer accepted even one right.
    pub fn mark_offer_started(&self) -> io::Result<()> {
        let state = ORIGINAL
            .lock()
            .map_err(|_| invalid("original mode owner poisoned"))?;
        self.check_global(&state)?;
        match self.owner.phase.load(Ordering::Acquire) {
            C0_STAGED => self.owner.phase.store(C0_OFFERED, Ordering::Release),
            C0_OFFERED => {}
            _ => return Err(invalid("original mode offer uncertain or retired")),
        }
        Ok(())
    }
    /// Actual immutable role order: original canonical-mode TTY, editor-mode
    /// TTY. Borrow through a synchronous sendmsg only; never hold over await.
    pub fn with_offered_descriptions<R>(
        &self,
        action: impl FnOnce([BorrowedFd<'_>; 2]) -> io::Result<R>,
    ) -> io::Result<R> {
        self.mark_offer_started()?;
        let files = self
            .actual
            .files
            .lock()
            .map_err(|_| invalid("staged mode Files poisoned"))?;
        let files = files
            .as_ref()
            .ok_or_else(|| invalid("staged mode Files absent"))?;
        action([files.original.terminal.as_fd(), files.terminal.as_fd()])
    }
    /// This restores File ownership only and performs no terminal mutation or
    /// input resume. Native-started/unknown offers must retain their originals.
    pub fn rollback_before_offer(
        self,
    ) -> Result<EditorModes, Box<EditorModesStageRollbackRefused>> {
        let result = (|| {
            let _source = self
                .owner
                .source
                .lock()
                .map_err(|_| invalid("original mode preparation poisoned"))?;
            let mut state = ORIGINAL
                .lock()
                .map_err(|_| invalid("original mode owner poisoned"))?;
            self.check_global(&state)?;
            let mut terminal = self
                .owner
                .terminal
                .lock()
                .map_err(|_| invalid("original editor terminal poisoned"))?;
            let mut pinned = self
                .owner
                .staged
                .lock()
                .map_err(|_| invalid("original mode stage poisoned"))?;
            let mut files = self
                .actual
                .files
                .lock()
                .map_err(|_| invalid("staged mode Files poisoned"))?;
            if self.owner.phase.load(Ordering::Acquire) != C0_STAGED
                || terminal.is_some()
                || files.is_none()
            {
                return Err(invalid("original mode offer already began or changed"));
            }
            let actual = files
                .take()
                .ok_or_else(|| invalid("staged mode Files disappeared"))?;
            state.original = Some(actual.original);
            *terminal = Some(actual.terminal);
            let retired = pinned.take();
            self.owner.phase.store(HELD, Ordering::Release);
            drop(files);
            drop(pinned);
            drop(terminal);
            drop(state);
            drop(retired);
            Ok(EditorModes {
                actual: self.owner.clone(),
            })
        })();
        match result {
            Ok(original) => Ok(original),
            Err(error) => Err(Box::new(EditorModesStageRollbackRefused {
                staged: self,
                error,
            })),
        }
    }
}
