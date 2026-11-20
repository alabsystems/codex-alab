// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Same original native clipboard thread, FIFO probe and real join custody.
//! A no-lease observation never certifies thread completion or whole-App handoff.
use super::*;
use std::sync::Condvar;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
#[path = "retained_clipboard_scopes.rs"]
mod scopes;
pub(crate) use scopes::ScopeCapture;
pub(crate) use scopes::ScopeData;
pub(crate) use scopes::ScopeRef;
pub(crate) use scopes::ScopeRestored;
#[path = "retained_clipboard_checkpoint.rs"]
mod checkpoint;
pub(crate) use checkpoint::BoundClipboard;
pub(crate) use checkpoint::ClipboardData;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    ProbeQueued,
    PausedNoLease,
    Resident,
    ResumeRequested,
    Cancelled,
    JoinRequested,
    ExitingNoLease,
    JoinedNoLease,
    NoResidentWorker,
    Unknown,
}
struct Preparation {
    phase: Mutex<Phase>,
    changed: Condvar,
    failed: AtomicBool,
    abandoned: AtomicBool,
    frames: FrameRequester,
    next_id: u64,
}
impl Preparation {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Phase>, String> {
        self.phase.lock().map_err(|_| {
            self.failed.store(true, Ordering::Release);
            "clipboard handoff custody poisoned".to_owned()
        })
    }
    fn wake(&self) {
        self.changed.notify_all();
        self.frames.schedule_frame();
    }
}
/// Observation ownership may disappear; the original ClipboardWorker retains this record.
#[derive(Clone)]
pub(crate) struct JoinPreparation {
    actual: Arc<Preparation>,
}
/// Issued only after the SAME retained JoinHandle was actually joined, or the
/// actual original worker was never started. Not a clipboard lease/restore permit.
#[derive(Clone)]
pub(crate) struct JoinedWorker {
    actual: Arc<Preparation>,
}
impl JoinedWorker {
    pub(crate) fn same_preparation(&self, prepared: &JoinPreparation) -> bool {
        Arc::ptr_eq(&self.actual, &prepared.actual)
    }
    pub(crate) fn is_for(&self, worker: &ClipboardWorker) -> bool {
        worker
            .alab_retained
            .current
            .as_ref()
            .is_some_and(|record| Arc::ptr_eq(record, &self.actual))
            && worker.next_id == self.actual.next_id
            && worker
                .alab_retained
                .frames
                .as_ref()
                .is_none_or(|frames| frames.alab_same_scheduler(&self.actual.frames))
            && self
                .actual
                .lock()
                .is_ok_and(|phase| *phase == Phase::JoinedNoLease)
    }
}
pub(crate) enum JoinObservation {
    WaitingProbe,
    ResidentLease,
    AwaitingOriginalUi,
    Joining,
    Cancelled,
    Joined(JoinedWorker),
}
#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum ThreadLife {
    #[default]
    NeverStarted,
    Running,
    Joined,
}
#[derive(Default)]
pub(crate) struct Custody {
    domain: Arc<checkpoint::WorkerDomain>,
    worker: Option<JoinHandle<()>>,
    frames: Option<FrameRequester>,
    life: ThreadLife,
    current: Option<Arc<Preparation>>,
    failed_join: Option<Box<dyn std::any::Any + Send + 'static>>,
}
impl Custody {
    pub(super) fn before_start(&self) -> Result<(), String> {
        self.accepting()?;
        if self.life == ThreadLife::Running || self.worker.is_some() {
            return Err("original clipboard worker already has thread custody".into());
        }
        Ok(())
    }
    pub(super) fn started(&mut self, worker: JoinHandle<()>, frames: FrameRequester) {
        self.life = ThreadLife::Running;
        self.worker = Some(worker);
        let _ = self.domain.frame.set(frames.clone());
        self.frames = Some(frames);
    }
    pub(super) fn install_held(&self) -> bool {
        self.domain.install_held()
    }
    pub(super) fn fenced(&self) -> bool {
        self.install_held()
            || self.current.as_ref().is_some_and(|record| {
                record
                    .lock()
                    .map_or(true, |phase| *phase != Phase::Cancelled)
            })
    }
    pub(super) fn accepting(&self) -> Result<(), String> {
        if self.fenced() {
            Err("clipboard handoff owns original admission".into())
        } else {
            Ok(())
        }
    }
    pub(super) fn advance(&self, next: u64) {
        self.domain.next_id.store(next, Ordering::Release);
    }
    pub(super) fn owner_dropped(&self) {
        self.domain.closed.store(true, Ordering::Release);
        if let Some(record) = &self.current {
            record.abandoned.store(true, Ordering::Release);
            record.wake();
        }
    }
}
/// Private request variant; only the actual original worker supplies its live
/// lease slots. No DTO/label/observer bool can manufacture a no-lease result.
pub(super) struct Control {
    actual: Arc<Preparation>,
}
pub(super) enum Directive {
    Continue,
    ExitNoLease,
}
impl Control {
    pub(super) fn at_worker(
        self,
        lease: &Option<ClipboardLease>,
        primary: &Option<ClipboardLease>,
    ) -> Directive {
        let record = &self.actual;
        let Ok(mut phase) = record.lock() else {
            return Directive::Continue;
        };
        if record.abandoned.load(Ordering::Acquire) || *phase == Phase::ResumeRequested {
            *phase = Phase::Cancelled;
            record.wake();
            return Directive::Continue;
        }
        if *phase != Phase::ProbeQueued {
            record.failed.store(true, Ordering::Release);
            *phase = Phase::Unknown;
            record.wake();
            return Directive::Continue;
        }
        if lease.is_some() || primary.is_some() {
            *phase = Phase::Resident;
            record.wake();
            return Directive::Continue;
        }
        *phase = Phase::PausedNoLease;
        record.wake();
        loop {
            if record.abandoned.load(Ordering::Acquire) {
                *phase = Phase::Cancelled;
                record.wake();
                return Directive::Continue;
            }
            match *phase {
                Phase::ResumeRequested => {
                    *phase = Phase::Cancelled;
                    record.wake();
                    return Directive::Continue;
                }
                Phase::JoinRequested => {
                    *phase = Phase::ExitingNoLease;
                    record.wake();
                    return Directive::ExitNoLease;
                }
                Phase::PausedNoLease => {}
                _ => {
                    record.failed.store(true, Ordering::Release);
                    *phase = Phase::Unknown;
                    record.wake();
                    return Directive::Continue;
                }
            }
            phase = match record.changed.wait(phase) {
                Ok(phase) => phase,
                Err(_) => {
                    record.failed.store(true, Ordering::Release);
                    record.wake();
                    return Directive::Continue;
                }
            };
        }
    }
}
/// The actual original Draw operation owns this guard. Observation/drop cannot
/// manufacture completion; the worker incarnation retains Unknown and event DATA.
pub(crate) struct OriginalDraw {
    actual: Arc<checkpoint::UiProgress>,
    complete: bool,
}
impl OriginalDraw {
    pub(crate) fn complete(mut self) -> std::io::Result<()> {
        let mut state = self
            .actual
            .state
            .lock()
            .map_err(|_| std::io::Error::other("clipboard original event custody poisoned"))?;
        if !state.active || state.unknown {
            return Err(std::io::Error::other(
                "original clipboard event lost custody",
            ));
        }
        let Some(completed) = state.completed.checked_add(1) else {
            state.unknown = true;
            return Err(std::io::Error::other(
                "original clipboard event domain overflow",
            ));
        };
        state.active = false;
        state.completed = completed;
        state.last_event = None;
        self.complete = true;
        Ok(())
    }
}
impl Drop for OriginalDraw {
    fn drop(&mut self) {
        if !self.complete
            && let Ok(mut state) = self.actual.state.lock()
        {
            state.active = false;
            state.unknown = true;
        }
    }
}
impl ClipboardWorker {
    pub(crate) fn alab_begin_original_draw(
        &self,
        prepared: &JoinPreparation,
        event: &crate::tui::TuiEvent,
    ) -> std::io::Result<OriginalDraw> {
        self.alab_same_preparation(prepared)
            .map_err(std::io::Error::other)?;
        if self.alab_retained.install_held() {
            return Err(std::io::Error::other(
                "actual clipboard graph is not installed",
            ));
        }
        if !matches!(event, crate::tui::TuiEvent::Draw) {
            return Err(std::io::Error::other(
                "clipboard drain requires an actual original Draw event",
            ));
        }
        let actual = self.alab_retained.domain.ui.clone();
        {
            let mut state = actual
                .state
                .lock()
                .map_err(|_| std::io::Error::other("clipboard original event custody poisoned"))?;
            if state.active || state.unknown {
                return Err(std::io::Error::other(
                    "clipboard original event remains unobserved",
                ));
            }
            state.active = true;
            state.last_event = Some(event.clone());
        }
        Ok(OriginalDraw {
            actual,
            complete: false,
        })
    }
    /// Install record+fence before the FIFO send; dropping the observer does not
    /// lose accepted work, close its inlet, or destroy a Linux clipboard lease.
    pub(crate) fn alab_prepare_join(
        &mut self,
        frames: FrameRequester,
    ) -> Result<JoinPreparation, String> {
        if let Some(actual) = &self.alab_retained.current
            && *actual.lock()? != Phase::Cancelled
        {
            return Ok(JoinPreparation {
                actual: actual.clone(),
            });
        }
        if self
            .alab_retained
            .frames
            .as_ref()
            .is_some_and(|actual| !actual.alab_same_scheduler(&frames))
        {
            return Err("clipboard preparation uses another original frame scheduler".into());
        }
        let initial = if self.alab_retained.life == ThreadLife::Running {
            Phase::ProbeQueued
        } else {
            Phase::NoResidentWorker
        };
        let actual = Arc::new(Preparation {
            phase: Mutex::new(initial),
            changed: Condvar::new(),
            failed: AtomicBool::new(false),
            abandoned: AtomicBool::new(false),
            frames,
            next_id: self.next_id,
        });
        self.alab_retained.current = Some(actual.clone());
        let prepared = JoinPreparation {
            actual: actual.clone(),
        };
        if initial == Phase::ProbeQueued {
            let sent = self.requests.as_ref().is_some_and(|sender| {
                sender
                    .send(Request::AlabRetained(Control {
                        actual: actual.clone(),
                    }))
                    .is_ok()
            });
            if !sent {
                *actual.lock()? = Phase::Unknown;
                actual.wake();
            }
        } else if self.requests.is_some()
            || (self.alab_retained.life == ThreadLife::NeverStarted && self.responses.is_some())
            || self.alab_retained.worker.is_some()
        {
            *actual.lock()? = Phase::Unknown;
            actual.wake();
        }
        Ok(prepared)
    }
    fn alab_same_preparation(&self, prepared: &JoinPreparation) -> Result<(), String> {
        if !self
            .alab_retained
            .current
            .as_ref()
            .is_some_and(|actual| Arc::ptr_eq(actual, &prepared.actual))
        {
            return Err("clipboard preparation belongs to another original worker".into());
        }
        if prepared.actual.failed.load(Ordering::Acquire) {
            return Err("clipboard worker custody is unknown".into());
        }
        Ok(())
    }
    /// A genuine FIFO pause is still not UI delivery. Native copy/read results
    /// and terminal sequences must be consumed by the original Tui before this
    /// closes its inlet. Deferred UI-only fields remain owned and unchanged.
    pub(crate) fn alab_close_join(&mut self, prepared: &JoinPreparation) -> Result<bool, String> {
        self.alab_same_preparation(prepared)?;
        let mut phase = prepared.actual.lock()?;
        if self.pending.is_some()
            || self
                .pending_read
                .as_ref()
                .is_some_and(|read| read.response.is_some())
            || self.read_result.is_some()
        {
            return Ok(false);
        }
        match *phase {
            Phase::PausedNoLease => {
                *phase = Phase::JoinRequested;
                self.requests.take();
                prepared.actual.wake();
                Ok(true)
            }
            Phase::NoResidentWorker => {
                *phase = Phase::JoinedNoLease;
                prepared.actual.wake();
                Ok(true)
            }
            Phase::JoinRequested | Phase::ExitingNoLease | Phase::JoinedNoLease => Ok(true),
            Phase::ProbeQueued | Phase::Resident | Phase::ResumeRequested | Phase::Cancelled => {
                Ok(false)
            }
            Phase::Unknown => Err("clipboard worker custody is unknown".into()),
        }
    }
    /// Caller keeps this worker alive and polls under its original terminal
    /// owner. Receiver disconnection is deliberately never a completion oracle.
    pub(crate) fn alab_observe_join(
        &mut self,
        prepared: &JoinPreparation,
    ) -> Result<JoinObservation, String> {
        self.alab_same_preparation(prepared)?;
        let phase = *prepared.actual.lock()?;
        if phase == Phase::ExitingNoLease {
            let finished = self
                .alab_retained
                .worker
                .as_ref()
                .is_some_and(JoinHandle::is_finished);
            if !finished {
                prepared
                    .actual
                    .frames
                    .schedule_frame_in(Duration::from_millis(1));
                return Ok(JoinObservation::Joining);
            }
            let Some(worker) = self.alab_retained.worker.take() else {
                *prepared.actual.lock()? = Phase::Unknown;
                return Err("original clipboard JoinHandle missing".into());
            };
            match worker.join() {
                Ok(()) => {
                    self.alab_retained.life = ThreadLife::Joined;
                    *prepared.actual.lock()? = Phase::JoinedNoLease;
                }
                Err(payload) => {
                    self.alab_retained.failed_join = Some(payload);
                    *prepared.actual.lock()? = Phase::Unknown;
                    return Err("original clipboard worker did not join normally".into());
                }
            }
        }
        Ok(match *prepared.actual.lock()? {
            Phase::ProbeQueued => JoinObservation::WaitingProbe,
            Phase::Resident => JoinObservation::ResidentLease,
            Phase::PausedNoLease => JoinObservation::AwaitingOriginalUi,
            Phase::JoinRequested | Phase::ExitingNoLease => JoinObservation::Joining,
            Phase::ResumeRequested => JoinObservation::WaitingProbe,
            Phase::Cancelled => JoinObservation::Cancelled,
            Phase::NoResidentWorker => JoinObservation::AwaitingOriginalUi,
            Phase::JoinedNoLease => JoinObservation::Joined(JoinedWorker {
                actual: prepared.actual.clone(),
            }),
            Phase::Unknown => return Err("clipboard worker custody is unknown".into()),
        })
    }
    /// Local failed-upgrade rollback only after the exact original thread is
    /// genuinely joined. Original UI state/IDs/results/scopes are retained;
    /// the original idle service returns without submitting any request.
    /// This is neither C2 restoration nor a clipboard/Native handoff grant.
    pub(crate) fn alab_resume_after_join(&mut self, joined: &JoinedWorker) -> Result<(), String> {
        self.alab_resume_after_join_with(joined, ClipboardWorker::ensure_started)
    }
    fn alab_resume_after_join_with(
        &mut self,
        joined: &JoinedWorker,
        restart: impl FnOnce(&mut ClipboardWorker, FrameRequester) -> Result<(), String>,
    ) -> Result<(), String> {
        if !joined.is_for(self)
            || self.alab_retained.worker.is_some()
            || self.alab_retained.life == ThreadLife::Running
        {
            return Err(
                "original clipboard worker has no matching joined rollback resource".into(),
            );
        }
        *joined.actual.lock()? = Phase::Cancelled;
        // Native deferred start_read assumes its original service already
        // exists. Restore only that idle service, never submit an old request.
        if let Err(error) = restart(self, joined.actual.frames.clone()) {
            *joined.actual.lock()? = if self.alab_retained.worker.is_none()
                && self.alab_retained.life != ThreadLife::Running
            {
                Phase::JoinedNoLease
            } else {
                Phase::Unknown
            };
            joined.actual.wake();
            return Err(error);
        }
        joined.actual.wake();
        Ok(())
    }
    pub(crate) fn alab_cancel_join(&mut self, prepared: &JoinPreparation) -> Result<(), String> {
        self.alab_same_preparation(prepared)?;
        let mut phase = prepared.actual.lock()?;
        match *phase {
            Phase::ProbeQueued | Phase::PausedNoLease => {
                *phase = Phase::ResumeRequested;
                prepared.actual.wake();
                Ok(())
            }
            Phase::Resident | Phase::NoResidentWorker => {
                *phase = Phase::Cancelled;
                prepared.actual.wake();
                Ok(())
            }
            Phase::ResumeRequested | Phase::Cancelled => Ok(()),
            Phase::JoinRequested | Phase::ExitingNoLease | Phase::JoinedNoLease => {
                Err("original clipboard inlet already committed to join".into())
            }
            Phase::Unknown => Err("clipboard worker custody is unknown".into()),
        }
    }
}
#[cfg(test)]
#[path = "retained_clipboard_worker_tests.rs"]
mod tests;
