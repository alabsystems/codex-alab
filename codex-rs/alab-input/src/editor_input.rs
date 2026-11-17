// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The original parser/description stays fenced through an external editor.
//! This resource is process-local input custody, never Native editor authority.
use super::*;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;

const HELD: u8 = 0;
const FINISHING: u8 = 1;
const FINISHED: u8 = 2;
const UNKNOWN: u8 = 3;

#[path = "editor_input_stage.rs"]
mod staged;
pub use staged::EditorInputStage;
pub use staged::EditorInputStageRefused;
pub use staged::EditorInputStageRollbackRefused;

pub(super) struct Token {
    phase: AtomicU8,
    staged: Mutex<Option<Arc<staged::Staged>>>,
}
impl Token {
    fn held() -> Self {
        Self {
            phase: AtomicU8::new(HELD),
            staged: Mutex::new(None),
        }
    }
}

/// Dropping an observer or this token never restarts the reader. The broker
/// retains the exact parser, input description, and sticky restriction. The
/// actual editor job must retain this owner before its first spawn/claim await.
pub struct EditorFence {
    broker: Arc<Broker>,
    token: Arc<Token>,
    snapshot: Vec<u8>,
}

/// Failed admission returns the exact supplied description and error. A
/// caller may retry with this SAME File; no path or descriptor reconstruction
/// is needed and no refused restoration can close its only actual owner.
#[derive(Debug)]
pub struct EditorRestoreRefused {
    pub input: File,
    pub error: io::Error,
}

impl Broker {
    /// Stop/join the same producer and preserve every queued/partial sequence.
    /// Call after the original Tui pause and before handing the TTY to a child.
    pub fn prepare_editor(self: &Arc<Self>) -> io::Result<EditorFence> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.frozen || state.committed || state.editor.is_some() {
            return Err(invalid("input already has a retained owner"));
        }
        Self::stop(&mut state)?;
        if let Some(failure) = &state.failure {
            return Err(io::Error::other(failure.clone()));
        }
        if state.input.is_none() {
            state.input = Some((self.open)()?);
        }
        let snapshot = state.parser.checkpoint()?;
        let token = Arc::new(Token::held());
        state.pristine = false;
        state.frozen = true;
        state.editor = Some(token.clone());
        Ok(EditorFence {
            broker: self.clone(),
            token,
            snapshot,
        })
    }

    /// C2's actual claimed editor supplies this same terminal description.
    /// Install the complete parser and its restriction under ONE lock before
    /// any subscription, replay, quarantine, or new input description exists.
    /// The validator may correlate actual resources; DATA alone grants nothing.
    /// Every returned refusal retains the SAME input File and original error.
    pub fn restore_editor_with(
        self: &Arc<Self>,
        bytes: &[u8],
        input: File,
        validate: impl FnOnce(&File) -> io::Result<()>,
    ) -> Result<EditorFence, EditorRestoreRefused> {
        let prepared = (|| {
            let parser = Parser::restore(bytes)?;
            let snapshot = parser.checkpoint()?;
            Ok::<_, io::Error>((parser, snapshot))
        })();
        let (parser, snapshot) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return Err(EditorRestoreRefused { input, error }),
        };
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.pristine
            || state.users != 0
            || state.input.is_some()
            || state.driver.is_some()
            || state.frozen
            || state.committed
            || state.editor.is_some()
        {
            return Err(EditorRestoreRefused {
                input,
                error: invalid("editor input restoration is too late"),
            });
        }
        if let Err(error) = validate(&input) {
            return Err(EditorRestoreRefused { input, error });
        }
        let token = Arc::new(Token::held());
        state.parser = parser;
        state.input = Some(input);
        state.pristine = false;
        state.frozen = true;
        state.editor = Some(token.clone());
        Ok(EditorFence {
            broker: self.clone(),
            token,
            snapshot,
        })
    }
}

impl EditorFence {
    pub fn snapshot(&self) -> &[u8] {
        &self.snapshot
    }

    fn check(&self, state: &State) -> io::Result<()> {
        if !state.frozen
            || state.committed
            || state.driver.is_some()
            || !state
                .editor
                .as_ref()
                .is_some_and(|actual| Arc::ptr_eq(actual, &self.token))
            || self.token.phase.load(Ordering::Acquire) != HELD
        {
            return Err(invalid(
                "editor input owner changed or handback is uncertain",
            ));
        }
        Ok(())
    }

    pub fn terminal_identity(&self) -> io::Result<(u64, u64, u64)> {
        let state = self
            .broker
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.check(&state)?;
        let metadata = state
            .input
            .as_ref()
            .ok_or_else(|| invalid("editor input description absent"))?
            .metadata()?;
        Ok((metadata.dev(), metadata.ino(), metadata.rdev()))
    }

    /// Duplicate the SAME checked actual description; no path or descriptor
    /// number is accepted as a restoration or editor permission constructor.
    pub fn terminal_description(&self) -> io::Result<std::os::fd::OwnedFd> {
        use std::os::fd::AsFd;
        let state = self
            .broker
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.check(&state)?;
        state
            .input
            .as_ref()
            .ok_or_else(|| invalid("editor input description absent"))?
            .as_fd()
            .try_clone_to_owned()
    }

    /// The real caller first retains the SAME SDK EditorFinished and performs
    /// its original terminal handback. This synchronous closure must validate
    /// that resource/TTY and apply the original kernel-only suffix flush. It
    /// cannot replace or clear the saved parser. No input reader can start
    /// between that work and reopening this original owner.
    ///
    /// Any error/unwind makes handback sticky Unknown, retaining parser+TTY;
    /// neither retrying a closure nor dropping this token reopens input.
    pub fn finish_with(
        &mut self,
        handback: impl FnOnce(&File) -> io::Result<()>,
    ) -> io::Result<()> {
        let mut state = self
            .broker
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.check(&state)?;
        let input = state
            .input
            .as_ref()
            .ok_or_else(|| invalid("editor input description absent"))?;
        self.token.phase.store(FINISHING, Ordering::Release);
        let mut attempt = Handback {
            token: self.token.clone(),
            completed: false,
        };
        handback(input)?;
        state.editor = None;
        state.frozen = false;
        self.token.phase.store(FINISHED, Ordering::Release);
        attempt.completed = true;
        drop(state);
        self.broker.wake.notify();
        Ok(())
    }
}

struct Handback {
    token: Arc<Token>,
    completed: bool,
}
impl Drop for Handback {
    fn drop(&mut self) {
        if !self.completed {
            self.token.phase.store(UNKNOWN, Ordering::Release);
        }
    }
}
