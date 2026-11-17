// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Observe the SAME original editor inside a surviving C0 actor runtime.
//! Unlike the same-PID execution fence, this does not wait for progression or
//! a blocking document open/read. These Source resources grant no Native role,
//! root waitability to C2, foreground, NoFrontendIO, or replacement permission.
use super::*;
use std::fs::File;
use std::io::Read;
use std::os::fd::OwnedFd;

pub(super) struct OriginalRead {
    state: Mutex<ReadState>,
}
enum ReadState {
    Opening,
    Reading(Arc<File>),
    Settled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginalReadPhase {
    Opening,
    Reading,
    Settled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum C0Phase {
    Starting,
    RootLive,
    RootReapedReadPending,
    FinishedPending,
}

/// This can only pin an already-started actual registered Operation. It does
/// not create another command, Child, read task, document open or callback.
pub struct C0Capture {
    original: Arc<Original>,
}
/// The SAME actual blocking spawn worker was observed and joined. Scheduling
/// or a phase label cannot construct this resource; stdio retirement must
/// retain it so inherited descriptors were resolved before they can change.
pub struct C0SpawnCompleted {
    original: Arc<Original>,
}
pub struct C0Observation {
    original: Arc<Original>,
    phase: C0Phase,
    root_pid_data: Option<u32>,
    read: Option<OriginalReadObservation>,
}
/// The exact actual open/read slot, including a genuinely pending File::open.
/// A missing description is not a closed root/read or Native IO permission.
pub struct OriginalReadObservation {
    actual: Arc<OriginalRead>,
}

impl Operation {
    pub fn capture_c0(&self) -> io::Result<C0Capture> {
        let lifecycle = self
            .0
            .lifecycle
            .lock()
            .map_err(|_| unavailable("original editor lifecycle poisoned"))?;
        if !lifecycle.starting || lifecycle.cancelled {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "original editor has no accepted start",
            ));
        }
        check_pending(&self.0)?;
        Ok(C0Capture {
            original: self.0.clone(),
        })
    }
}
fn check_pending(original: &Arc<Original>) -> io::Result<()> {
    if original.callback.load(Ordering::Acquire) != CALLBACK_PENDING {
        return Err(unavailable(
            "original editor callback is applied or uncertain",
        ));
    }
    let records = registry()
        .lock()
        .map_err(|_| unavailable("original editor registry poisoned"))?;
    if !records
        .get(&original.key)
        .is_some_and(|actual| Arc::ptr_eq(actual, original))
    {
        return Err(unavailable("original editor registry resource changed"));
    }
    Ok(())
}
impl C0Capture {
    pub fn is_for(&self, actual: &Operation) -> bool {
        Arc::ptr_eq(&self.original, &actual.0)
    }
    /// Arm before observing so actual root/read/result publication cannot be
    /// missed. A wake alone is never a phase, closure or permission receipt.
    pub fn changes(&self) -> watch::Receiver<()> {
        self.original.changed.subscribe()
    }
    pub async fn spawn_completed(&self) -> io::Result<C0SpawnCompleted> {
        let mut changes = self.changes();
        loop {
            check_pending(&self.original)?;
            let observed = {
                let progress = self
                    .original
                    .progress
                    .lock()
                    .map_err(|_| unavailable("original editor state poisoned"))?;
                if progress.unknown {
                    return Err(unavailable("original editor spawn custody unknown"));
                }
                progress.started || progress.finished.is_some()
            };
            if observed {
                break;
            }
            changes
                .changed()
                .await
                .map_err(|_| unavailable("original spawn observation retired"))?;
        }
        self.original.join_spawn().await?;
        check_pending(&self.original)?;
        Ok(C0SpawnCompleted {
            original: self.original.clone(),
        })
    }
    pub fn observe(&self) -> io::Result<C0Observation> {
        check_pending(&self.original)?;
        let progress = self
            .original
            .progress
            .lock()
            .map_err(|_| unavailable("original editor state poisoned"))?;
        if progress.unknown {
            return Err(unavailable("original editor outcome remains unobserved"));
        }
        let phase = if progress.finished.is_some() {
            C0Phase::FinishedPending
        } else if progress.reaped.is_some() {
            C0Phase::RootReapedReadPending
        } else if progress.started {
            C0Phase::RootLive
        } else {
            C0Phase::Starting
        };
        Ok(C0Observation {
            original: self.original.clone(),
            phase,
            root_pid_data: progress.child.as_ref().and_then(Child::id),
            read: progress
                .c0_read
                .as_ref()
                .map(|actual| OriginalReadObservation {
                    actual: actual.clone(),
                }),
        })
    }
    /// Reuse the actual original wait/join and its retained result. C2's
    /// authenticated host channel will observe this; it never gets a Child or
    /// a constructor that could run another read or submit a guessed result.
    pub async fn finished(&self) -> io::Result<Arc<Finished>> {
        check_pending(&self.original)?;
        Operation(self.original.clone()).wait().await
    }
}
impl C0SpawnCompleted {
    pub fn is_for(&self, actual: &C0Capture) -> bool {
        Arc::ptr_eq(&self.original, &actual.original)
    }
    pub fn is_for_operation(&self, actual: &Operation) -> bool {
        Arc::ptr_eq(&self.original, &actual.0)
    }
}
impl C0Observation {
    pub fn is_for(&self, actual: &C0Capture) -> bool {
        Arc::ptr_eq(&self.original, &actual.original)
    }
    pub fn phase(&self) -> C0Phase {
        self.phase
    }
    /// Kernel correlation DATA only; never a PID-to-Child or parent-wait grant.
    pub fn root_pid_data(&self) -> Option<u32> {
        self.root_pid_data
    }
    pub fn original_read(&self) -> Option<&OriginalReadObservation> {
        self.read.as_ref()
    }
}
impl OriginalReadObservation {
    pub fn phase(&self) -> io::Result<OriginalReadPhase> {
        let state = self
            .actual
            .state
            .lock()
            .map_err(|_| unavailable("original editor read slot poisoned"))?;
        Ok(match &*state {
            ReadState::Opening => OriginalReadPhase::Opening,
            ReadState::Reading(_) => OriginalReadPhase::Reading,
            ReadState::Settled => OriginalReadPhase::Settled,
        })
    }
    /// Duplicate only the already-open SAME File. No path is accepted or
    /// opened here. Native must bind any allowed editor-read IO separately
    /// from retirement of parent UI writers and independently authenticate C0.
    pub fn description(&self) -> io::Result<Option<OwnedFd>> {
        let _custody = codex_alab_core::native_fd_custody::receiving()?;
        let state = self
            .actual
            .state
            .lock()
            .map_err(|_| unavailable("original editor read slot poisoned"))?;
        let file = match &*state {
            ReadState::Opening => None,
            ReadState::Reading(actual) => Some(actual),
            ReadState::Settled => None,
        };
        file.map(|actual| actual.try_clone().map(OwnedFd::from))
            .transpose()
    }
}

pub(super) fn read_original_document(original: &Original) -> Result<String, Arc<io::Error>> {
    let actual = Arc::new(OriginalRead {
        state: Mutex::new(ReadState::Opening),
    });
    {
        let mut progress = original
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if progress.c0_read.is_some() {
            progress.unknown = true;
            return Err(Arc::new(unavailable(
                "original editor read owner already installed",
            )));
        }
        progress.c0_read = Some(actual.clone());
    }
    original.changed.send_replace(());
    let close = ReadClose {
        actual: actual.clone(),
        changed: &original.changed,
    };
    // SAME original open/content/error policy: follow the editor's actual path,
    // including FIFO/symlinks, with no new UID/type/nofollow check. The public
    // Read helper performs its own non-consuming metadata/position probe and
    // reserve strategy; this is not a claim of identical private std allocator
    // steps. Publish the File before read; hold no slot mutex through any IO.
    let file = match File::open(&original.document) {
        Ok(file) => Arc::new(file),
        Err(error) => {
            drop(close);
            return Err(Arc::new(error));
        }
    };
    *actual.state.lock().unwrap_or_else(PoisonError::into_inner) = ReadState::Reading(file.clone());
    original.changed.send_replace(());
    let mut text = String::new();
    let result = (&*file)
        .read_to_string(&mut text)
        .map(|_| text)
        .map_err(Arc::new);
    drop(close);
    // Close the original File at the same read-return boundary. Only an
    // explicitly taken description may outlive it under its real new owner.
    drop(file);
    result
}

/// Clears the published description BEFORE the reading thread's File drops,
/// including unwind. Observers duplicate under this SAME publication mutex;
/// no retired numeric descriptor can be borrowed or accidentally reused.
struct ReadClose<'a> {
    actual: Arc<OriginalRead>,
    changed: &'a watch::Sender<()>,
}
impl Drop for ReadClose<'_> {
    fn drop(&mut self) {
        *self
            .actual
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = ReadState::Settled;
        self.changed.send_replace(());
    }
}

#[cfg(test)]
#[path = "frontend_c0_editor_tests.rs"]
mod tests;
