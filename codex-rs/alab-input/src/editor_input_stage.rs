// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Linear staging of the actual stopped editor input description. A staged
//! File and whole Broker/parser are independently pinned before publication,
//! even when a generic Broker's last external observer is dropped.
//! This moves no reader/parser to another process and grants no Native/NoIO/Go.
use super::*;
use std::collections::BTreeMap;
use std::os::fd::AsFd;
use std::os::fd::BorrowedFd;
use std::sync::OnceLock;
use std::sync::Weak;
use std::sync::atomic::AtomicU64;
const MAX_STAGES: usize = 4096;
struct Retained {
    key: u64,
    broker: Arc<Broker>,
    actual: Arc<Staged>,
}
type Stages = BTreeMap<u64, Vec<Arc<Retained>>>;
fn stages() -> &'static Mutex<Stages> {
    static STAGES: OnceLock<Mutex<Stages>> = OnceLock::new();
    STAGES.get_or_init(|| Mutex::new(BTreeMap::new()))
}
struct Reservation {
    key: u64,
    installed: bool,
}
impl Reservation {
    fn reserve() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let mut entries = stages()
            .lock()
            .map_err(|_| invalid("original input stage registry poisoned"))?;
        if entries.len() >= MAX_STAGES {
            return Err(invalid("original input stage capacity"));
        }
        let key = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
                old.checked_add(1)
            })
            .map_err(|_| invalid("original input stage identity exhausted"))?;
        if entries.contains_key(&key) {
            return Err(invalid("original input stage identity conflict"));
        }
        entries.insert(key, Vec::new());
        Ok(Self {
            key,
            installed: false,
        })
    }
    fn install(mut self, actual: Arc<Retained>) {
        let mut entries = stages()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // No outside observer owns this reserved key. Publication is infallible
        // after actual File transfer, including poison recovery.
        entries.entry(self.key).or_default().push(actual);
        self.installed = true;
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if !self.installed {
            let mut entries = stages()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if entries.get(&self.key).is_some_and(Vec::is_empty) {
                entries.remove(&self.key);
            }
        }
    }
}
const STAGED: u8 = 4;
const OFFERED: u8 = 5;

pub(super) struct Staged {
    token: Weak<Token>,
    input: Mutex<Option<File>>,
    snapshot: Vec<u8>,
}
/// No Clone or constructor from DATA. A bounded original-resource registry
/// independently pins the whole Broker/parser and SAME actual File until real
/// rollback or a future authenticated acceptance path; no observer Drop retires it.
pub struct EditorInputStage {
    retained: Arc<Retained>,
}
pub struct EditorInputStageRefused {
    pub original: EditorFence,
    pub error: io::Error,
}
pub struct EditorInputStageRollbackRefused {
    pub staged: EditorInputStage,
    pub error: io::Error,
}

impl EditorFence {
    /// Take the SAME actual File out of the frozen broker, without duplicating
    /// it or changing any parser bytes, subscription, mode or reader state.
    /// All returned failures keep the exact original EditorFence.
    pub fn stage_original_input(self) -> Result<EditorInputStage, Box<EditorInputStageRefused>> {
        let result = (|| {
            // Release registry lock before acquiring Broker: all publication
            // and rollback lock pairs use Broker -> registry, never inverse.
            let reservation = Reservation::reserve()?;
            let mut state = self
                .broker
                .state
                .lock()
                .map_err(|_| invalid("editor broker poisoned"))?;
            self.check(&state)?;
            let mut pinned = self
                .token
                .staged
                .lock()
                .map_err(|_| invalid("editor input stage poisoned"))?;
            if pinned.is_some() || state.input.is_none() {
                return Err(invalid("editor input already staged or absent"));
            }
            // Every fallible check/lock precedes taking the original File.
            let input = state
                .input
                .take()
                .ok_or_else(|| invalid("editor input disappeared"))?;
            let actual = Arc::new(Staged {
                token: Arc::downgrade(&self.token),
                input: Mutex::new(Some(input)),
                snapshot: self.snapshot.clone(),
            });
            *pinned = Some(actual.clone());
            self.token.phase.store(STAGED, Ordering::Release);
            let retained = Arc::new(Retained {
                key: reservation.key,
                broker: self.broker.clone(),
                actual,
            });
            reservation.install(retained.clone());
            Ok(EditorInputStage { retained })
        })();
        match result {
            Ok(actual) => Ok(actual),
            Err(error) => Err(Box::new(EditorInputStageRefused {
                original: self,
                error,
            })),
        }
    }
}
impl EditorInputStage {
    fn check_pinned(&self, state: &State, token: &Arc<Token>) -> io::Result<()> {
        if !state.frozen
            || state.committed
            || state.driver.is_some()
            || state.input.is_some()
            || !state
                .editor
                .as_ref()
                .is_some_and(|actual| Arc::ptr_eq(actual, token))
        {
            return Err(invalid("staged original broker ownership changed"));
        }
        let pinned = token
            .staged
            .lock()
            .map_err(|_| invalid("editor input stage poisoned"))?;
        if !pinned
            .as_ref()
            .is_some_and(|actual| Arc::ptr_eq(actual, &self.retained.actual))
        {
            return Err(invalid("original input stage publication changed"));
        }
        drop(pinned);
        let entries = stages()
            .lock()
            .map_err(|_| invalid("original input stage registry poisoned"))?;
        if !entries
            .get(&self.retained.key)
            .is_some_and(|actual| actual.len() == 1 && Arc::ptr_eq(&actual[0], &self.retained))
        {
            return Err(invalid("original input stage keeper changed"));
        }
        Ok(())
    }
    fn token(&self) -> io::Result<Arc<Token>> {
        self.retained
            .actual
            .token
            .upgrade()
            .ok_or_else(|| invalid("staged input token retired"))
    }
    /// Actual unchanged parser checkpoint DATA, never a new broker constructor.
    pub fn snapshot(&self) -> &[u8] {
        &self.retained.actual.snapshot
    }
    pub fn terminal_identity(&self) -> io::Result<(u64, u64, u64)> {
        let token = self.token()?;
        let state = self
            .retained
            .broker
            .state
            .lock()
            .map_err(|_| invalid("editor broker poisoned"))?;
        self.check_pinned(&state, &token)?;
        if !matches!(token.phase.load(Ordering::Acquire), STAGED | OFFERED) {
            return Err(invalid("staged input phase changed"));
        }
        let input = self
            .retained
            .actual
            .input
            .lock()
            .map_err(|_| invalid("staged input File poisoned"))?;
        let metadata = input
            .as_ref()
            .ok_or_else(|| invalid("staged input File absent"))?
            .metadata()?;
        Ok((metadata.dev(), metadata.ino(), metadata.rdev()))
    }
    /// Restrict-only: call before exposing ANY member of the combined offer.
    /// This is not acceptance, final retirement, a Native receipt or permission.
    pub fn mark_offer_started(&self) -> io::Result<()> {
        let token = self.token()?;
        let state = self
            .retained
            .broker
            .state
            .lock()
            .map_err(|_| invalid("editor broker poisoned"))?;
        self.check_pinned(&state, &token)?;
        match token.phase.load(Ordering::Acquire) {
            STAGED => token.phase.store(OFFERED, Ordering::Release),
            OFFERED => {}
            _ => return Err(invalid("staged input offer is uncertain or retired")),
        }
        Ok(())
    }
    /// Borrow the original, never a dup. Mark the offer first and retain its
    /// File mutex through only the synchronous sendmsg/inspection closure.
    /// Errors never close it or reset OFFERED; no lock may cross an await.
    pub fn with_offered_description<R>(
        &self,
        action: impl FnOnce(BorrowedFd<'_>) -> io::Result<R>,
    ) -> io::Result<R> {
        self.mark_offer_started()?;
        let input = self
            .retained
            .actual
            .input
            .lock()
            .map_err(|_| invalid("staged input File poisoned"))?;
        action(
            input
                .as_ref()
                .ok_or_else(|| invalid("staged input File absent"))?
                .as_fd(),
        )
    }
    /// Only an offer that has NEVER begun may restore its same frozen owner.
    /// This does not resume a reader; actual Finished/Native handback is still
    /// required by the higher original-editor wrapper. Refusal keeps this stage.
    pub fn rollback_before_offer(
        self,
    ) -> Result<EditorFence, Box<EditorInputStageRollbackRefused>> {
        let result = (|| {
            let token = self.token()?;
            let snapshot = self.retained.actual.snapshot.clone();
            let mut state = self
                .retained
                .broker
                .state
                .lock()
                .map_err(|_| invalid("editor broker poisoned"))?;
            self.check_pinned(&state, &token)?;
            let mut pinned = token
                .staged
                .lock()
                .map_err(|_| invalid("editor input stage poisoned"))?;
            let mut input = self
                .retained
                .actual
                .input
                .lock()
                .map_err(|_| invalid("staged input File poisoned"))?;
            if token.phase.load(Ordering::Acquire) != STAGED || input.is_none() {
                return Err(invalid("actual input offer already began or changed"));
            }
            let mut entries = stages()
                .lock()
                .map_err(|_| invalid("original input stage registry poisoned"))?;
            if !entries
                .get(&self.retained.key)
                .is_some_and(|actual| actual.len() == 1 && Arc::ptr_eq(&actual[0], &self.retained))
            {
                return Err(invalid("original input stage keeper changed"));
            }
            // All refusal checks precede moves. No path/open/dup/reader starts.
            state.input = input.take();
            let retired = pinned.take();
            token.phase.store(HELD, Ordering::Release);
            let retired_keeper = entries.remove(&self.retained.key);
            drop(entries);
            drop(input);
            drop(pinned);
            drop(state);
            drop(retired);
            drop(retired_keeper);
            Ok(EditorFence {
                broker: self.retained.broker.clone(),
                token,
                snapshot,
            })
        })();
        match result {
            Ok(original) => Ok(original),
            Err(error) => Err(Box::new(EditorInputStageRollbackRefused {
                staged: self,
                error,
            })),
        }
    }
}

#[cfg(test)]
#[path = "editor_input_stage_tests.rs"]
mod tests;
