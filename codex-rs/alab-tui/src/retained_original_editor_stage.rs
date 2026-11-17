// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

// Move SAME local editor resources, never DATA, into an actual C0 offer.
// All partial failures retain their exact owners. No accepted-custody/final
// close/NoIO/Native/Go constructor exists in this staging-only component.
use codex_alab_aterm::frontend_control::retained_service::original_editor::C0Capture;
use codex_alab_aterm::frontend_control::retained_service::original_editor::C0SpawnCompleted;
use std::os::fd::BorrowedFd;
pub struct OriginalEditorInputStage {
    actual: codex_alab_input::EditorInputStage,
    operation: Arc<Operation>,
}
pub struct OriginalEditorInputStageRefused {
    pub original: OriginalEditorInput,
    pub error: io::Error,
}
pub struct OriginalEditorInputStageRollbackRefused {
    pub staged: OriginalEditorInputStage,
    pub error: io::Error,
}
impl OriginalEditorInput {
    pub fn stage_c0(
        self,
        capture: &C0Capture,
        spawn: &C0SpawnCompleted,
    ) -> Result<OriginalEditorInputStage, Box<OriginalEditorInputStageRefused>> {
        if !capture.is_for(&self.operation)
            || !spawn.is_for(capture)
            || !spawn.is_for_operation(&self.operation)
        {
            return Err(Box::new(OriginalEditorInputStageRefused {
                original: self,
                error: io::Error::other(
                    "C0 input stage changed original operation or completed spawn",
                ),
            }));
        }
        let OriginalEditorInput { actual, operation } = self;
        match actual.stage_original_input() {
            Ok(actual) => Ok(OriginalEditorInputStage { actual, operation }),
            Err(refused) => Err(Box::new(OriginalEditorInputStageRefused {
                original: OriginalEditorInput {
                    actual: refused.original,
                    operation,
                },
                error: refused.error,
            })),
        }
    }
}
impl OriginalEditorInputStage {
    pub fn is_for(&self, operation: &Operation) -> bool {
        self.operation.same(operation)
    }
    pub fn terminal_identity(&self) -> io::Result<(u64, u64, u64)> {
        self.actual.terminal_identity()
    }
    pub fn snapshot(&self) -> &[u8] {
        self.actual.snapshot()
    }
    pub fn mark_offer_started(&self) -> io::Result<()> {
        self.actual.mark_offer_started()
    }
    pub fn with_offered_description<R>(
        &self,
        action: impl FnOnce(BorrowedFd<'_>) -> io::Result<R>,
    ) -> io::Result<R> {
        self.actual.with_offered_description(action)
    }
    pub fn rollback_before_offer(
        self,
    ) -> Result<OriginalEditorInput, Box<OriginalEditorInputStageRollbackRefused>> {
        let OriginalEditorInputStage { actual, operation } = self;
        match actual.rollback_before_offer() {
            Ok(actual) => Ok(OriginalEditorInput { actual, operation }),
            Err(refused) => Err(Box::new(OriginalEditorInputStageRollbackRefused {
                staged: OriginalEditorInputStage {
                    actual: refused.staged,
                    operation,
                },
                error: refused.error,
            })),
        }
    }
}
/// An actual failed transaction is a resource scene, never Empty/Closed.
pub struct C0EditorResources {
    actual: C0EditorResourceState,
}
enum C0EditorResourceState {
    Held(OriginalEditorResources),
    InputStaged {
        input: OriginalEditorInputStage,
        modes: crate::terminal_modes::EditorModes,
    },
    Staged(C0EditorStage),
}
pub struct C0EditorStage {
    input: OriginalEditorInputStage,
    modes: crate::terminal_modes::EditorModesStage,
}
pub struct C0EditorStageRefused {
    pub actual: C0EditorResources,
    pub error: io::Error,
}
impl OriginalEditorResources {
    /// Caller stores every returned stage in the SAME actual global Job/frame
    /// before ANY await. The lower owners also independently pin original FDs.
    pub fn stage_c0(
        self,
        capture: &C0Capture,
        spawn: &C0SpawnCompleted,
    ) -> Result<C0EditorStage, Box<C0EditorStageRefused>> {
        if self.input.is_none() || self.modes.is_none() {
            return Err(Box::new(C0EditorStageRefused {
                actual: C0EditorResources::held(self),
                error: io::Error::other("C0 stage requires the actual complete pre-child bundle"),
            }));
        }
        let OriginalEditorResources { input, modes } = self;
        let (input, modes) = match (input, modes) {
            (Some(input), Some(modes)) => (input, modes),
            (input, modes) => {
                return Err(Box::new(C0EditorStageRefused {
                    actual: C0EditorResources::held(OriginalEditorResources { input, modes }),
                    error: io::Error::other("actual pre-child bundle changed before staging"),
                }));
            }
        };
        let input = match input.stage_c0(capture, spawn) {
            Ok(input) => input,
            Err(refused) => {
                return Err(Box::new(C0EditorStageRefused {
                    actual: C0EditorResources::held(OriginalEditorResources {
                        input: Some(refused.original),
                        modes: Some(modes),
                    }),
                    error: refused.error,
                }));
            }
        };
        match modes.stage_original_modes(capture, spawn, &input) {
            Ok(modes) => Ok(C0EditorStage { input, modes }),
            Err(refused) => Err(Box::new(C0EditorStageRefused {
                actual: C0EditorResources::input_staged(input, refused.original),
                error: refused.error,
            })),
        }
    }
}
impl C0EditorStage {
    pub fn is_for(&self, operation: &Operation) -> bool {
        self.input.is_for(operation) && self.modes.is_for(operation)
    }
    pub fn parser_snapshot(&self) -> &[u8] {
        self.input.snapshot()
    }
    pub fn mode_snapshot(&self) -> &crate::terminal_modes::Snapshot {
        self.modes.snapshot()
    }
    pub fn mark_offer_started(&self) -> io::Result<()> {
        // Mark every original domain before exposing ANY member descriptor.
        // A partial error leaves both actual owners retained and restricted.
        self.input.mark_offer_started()?;
        self.modes.mark_offer_started()
    }
    /// Immutable physical role order: original parser input, canonical-mode
    /// TTY, editor-mode TTY. These are ORIGINAL Files, not descriptor clones.
    pub fn with_offered_descriptions<R>(
        &self,
        action: impl FnOnce([BorrowedFd<'_>; 3]) -> io::Result<R>,
    ) -> io::Result<R> {
        self.mark_offer_started()?;
        self.input.with_offered_description(|input| {
            self.modes
                .with_offered_descriptions(|modes| action([input, modes[0], modes[1]]))
        })
    }
    pub fn rollback_before_offer(
        self,
    ) -> Result<OriginalEditorResources, Box<C0EditorStageRefused>> {
        let C0EditorStage { input, modes } = self;
        let modes = match modes.rollback_before_offer() {
            Ok(modes) => modes,
            Err(refused) => {
                return Err(Box::new(C0EditorStageRefused {
                    actual: C0EditorResources::staged(C0EditorStage {
                        input,
                        modes: refused.staged,
                    }),
                    error: refused.error,
                }));
            }
        };
        restore_input(input, modes)
    }
}
fn restore_input(
    input: OriginalEditorInputStage,
    modes: crate::terminal_modes::EditorModes,
) -> Result<OriginalEditorResources, Box<C0EditorStageRefused>> {
    match input.rollback_before_offer() {
        Ok(input) => Ok(OriginalEditorResources {
            input: Some(input),
            modes: Some(modes),
        }),
        Err(refused) => Err(Box::new(C0EditorStageRefused {
            actual: C0EditorResources::input_staged(refused.staged, modes),
            error: refused.error,
        })),
    }
}
impl C0EditorResources {
    fn held(actual: OriginalEditorResources) -> Self {
        Self {
            actual: C0EditorResourceState::Held(actual),
        }
    }
    fn input_staged(
        input: OriginalEditorInputStage,
        modes: crate::terminal_modes::EditorModes,
    ) -> Self {
        Self {
            actual: C0EditorResourceState::InputStaged { input, modes },
        }
    }
    fn staged(actual: C0EditorStage) -> Self {
        Self {
            actual: C0EditorResourceState::Staged(actual),
        }
    }
    pub fn is_for(&self, operation: &Operation) -> bool {
        match &self.actual {
            C0EditorResourceState::Held(actual) => {
                actual.input().is_some_and(|input| input.is_for(operation))
            }
            C0EditorResourceState::InputStaged { input, modes } => {
                input.is_for(operation) && modes.is_for(operation)
            }
            C0EditorResourceState::Staged(actual) => actual.is_for(operation),
        }
    }
    /// Rebuild only original local ownership before an offer began. This never
    /// opens/resumes input, changes modes, or creates a Native handback receipt.
    pub fn rollback_before_offer(
        self,
    ) -> Result<OriginalEditorResources, Box<C0EditorStageRefused>> {
        match self.actual {
            C0EditorResourceState::Held(actual) => Ok(actual),
            C0EditorResourceState::InputStaged { input, modes } => restore_input(input, modes),
            C0EditorResourceState::Staged(actual) => actual.rollback_before_offer(),
        }
    }
}
