// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One owned PRELOAD continuation. Observers never own its admission or cancel I/O.
use super::*;
use crate::native_preload::Attempt;
use crate::native_preload::Body;
use crate::native_preload::Predecessor;
use crate::native_preload::Resample;
use crate::native_source::Member;
use crate::native_source_use::ParentUseLease;
use crate::native_source_use::UseReceipt;
use std::sync::Weak;

pub(super) struct Flight {
    pub(super) attempt: Arc<Attempt>,
    member: Arc<dyn Member>,
    context: Option<Arc<UseReceipt>>,
    rebound: bool,
    seal: Arc<()>,
    // Operation admission, not a borrow of packet/report state.
    serial: tokio::sync::Semaphore,
    packet: Mutex<Option<Preload>>,
    original: Mutex<Option<Preload>>,
    steps: Mutex<Vec<(Predecessor, Resample)>>,
    report: Mutex<Option<(String, Loaded)>>,
    native_ack: Mutex<Option<(String, Loaded, u64)>>,
    terminal: AtomicBool,
    refused: AtomicBool,
}
impl Flight {
    pub(super) fn body(&self) -> &Body {
        self.attempt.body()
    }
    pub(super) fn terminal(&self) -> bool {
        self.terminal.load(Ordering::Acquire)
    }
    async fn fetch(
        self: &Arc<Self>,
        lifetime: Weak<Inner>,
        previous: Option<Predecessor>,
    ) -> io::Result<Resample> {
        let flight = self.clone();
        let (send, receive) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let Ok(_serial) = flight.serial.acquire().await else {
                let _ = send.send(Err(unavailable()));
                return;
            };
            let result = async {
                if flight.terminal() {
                    return Err(unavailable());
                }
                let existing = flight.packet.lock().map_err(|_| unavailable())?.clone();
                match (&previous, &existing) {
                    (None, Some(_)) => {
                        return flight
                            .original
                            .lock()
                            .map_err(|_| unavailable())?
                            .clone()
                            .map(Resample::Preload)
                            .ok_or_else(unavailable);
                    }
                    (Some(previous), Some(packet)) => {
                        previous.validate()?;
                        if let Some((_, result)) = flight
                            .steps
                            .lock()
                            .map_err(|_| unavailable())?
                            .iter()
                            .find(|(old, _)| old == previous)
                        {
                            return Ok(result.clone());
                        }
                        if previous.registration != packet.registration
                            || previous.revision != packet.source.revision
                        {
                            return Err(unavailable());
                        }
                        if flight.steps.lock().map_err(|_| unavailable())?.len() >= 128 {
                            return Err(unavailable());
                        }
                    }
                    (Some(_), None) => return Err(unavailable()),
                    _ => {}
                }
                flight.member.reconcile_work().await?;
                let packet = if flight.rebound {
                    if previous.is_some() {
                        return Err(unavailable());
                    }
                    Resample::Preload(flight.member.rebound_preload_owned(flight.body()).await?)
                } else {
                    match (previous.as_ref(), flight.context.as_ref()) {
                        (None, None) => Resample::Preload(
                            flight.member.preload_owned(flight.body(), None).await?,
                        ),
                        (None, Some(context)) => Resample::Preload(
                            flight
                                .member
                                .preload_owned_use(flight.body(), context)
                                .await?,
                        ),
                        (Some(previous), None) => {
                            flight
                                .member
                                .resample_owned(flight.body(), previous, None)
                                .await?
                        }
                        (Some(previous), Some(context)) => {
                            flight
                                .member
                                .resample_owned_use(flight.body(), previous, context)
                                .await?
                        }
                    }
                };
                let packet = match packet {
                    Resample::Preload(packet) => packet,
                    Resample::ReadyCommitted(committed) => {
                        let previous = previous.as_ref().ok_or_else(unavailable)?;
                        committed.validate(flight.body(), previous)?;
                        if !flight
                            .report
                            .lock()
                            .map_err(|_| unavailable())?
                            .as_ref()
                            .is_some_and(|(registration, _)| registration == &previous.registration)
                        {
                            return Err(unavailable());
                        }
                        let outcome = Resample::ReadyCommitted(committed);
                        flight
                            .steps
                            .lock()
                            .map_err(|_| unavailable())?
                            .push((previous.clone(), outcome.clone()));
                        return Ok(outcome);
                    }
                };
                packet.validate(flight.body())?;
                let inner = lifetime.upgrade().ok_or_else(unavailable)?;
                let mut state = inner.state.lock().map_err(|_| unavailable())?;
                if inner.retiring.load(Ordering::Acquire)
                    || !state
                        .flight
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, &flight))
                {
                    return Err(unavailable());
                }
                let kind = Kind::Managed(Scope::from(&packet.source));
                if state.kind.as_ref().is_some_and(|old| old != &kind) {
                    return Err(unavailable());
                }
                if state.preloaded.as_ref().is_some_and(|old| {
                    packet.source.revision.generation < old.generation
                        || (packet.source.revision.generation == old.generation
                            && packet.source.revision.digest != old.digest)
                }) {
                    return Err(unavailable());
                }
                state.snapshot = None;
                state.preloaded = Some(packet.source.revision.clone());
                state.kind = Some(kind);
                state.registration = Some(packet.registration.clone());
                state.preload_operation = Some(flight.body().operation.clone());
                if existing
                    .as_ref()
                    .is_none_or(|old| old.registration != packet.registration)
                {
                    *flight.report.lock().map_err(|_| unavailable())? = None;
                }
                match previous {
                    Some(previous) => flight
                        .steps
                        .lock()
                        .map_err(|_| unavailable())?
                        .push((previous, Resample::Preload(packet.clone()))),
                    None => {
                        *flight.original.lock().map_err(|_| unavailable())? = Some(packet.clone())
                    }
                }
                *flight.packet.lock().map_err(|_| unavailable())? = Some(packet.clone());
                Ok(Resample::Preload(packet))
            }
            .await;
            let _ = send.send(result);
        });
        receive.await.map_err(|_| unavailable())?
    }
    async fn ready_rpc(
        &self,
        lifetime: &Weak<Inner>,
        registration: &str,
        loaded: &Loaded,
        rebound: bool,
    ) -> io::Result<Option<(crate::native_preload::Ready, u64)>> {
        loop {
            if lifetime
                .upgrade()
                .is_none_or(|inner| inner.retiring.load(Ordering::Acquire))
            {
                return Err(unavailable());
            }
            let result = async {
                self.member.reconcile_work().await?;
                let work = self.member.work_incarnation();
                let receipt = if rebound {
                    match self
                        .member
                        .rebound_ready_owned(self.body(), registration, loaded)
                        .await?
                    {
                        crate::native_source::ReboundReadyOutcome::Ready(receipt) => receipt,
                        crate::native_source::ReboundReadyOutcome::SourceChanged => {
                            return Ok(None);
                        }
                    }
                } else {
                    match self.context.as_ref() {
                        Some(context) => {
                            self.member
                                .ready_owned_use(self.body(), registration, loaded, context)
                                .await?
                        }
                        None => {
                            self.member
                                .ready_owned(self.body(), registration, loaded)
                                .await?
                        }
                    }
                };
                receipt.validate(self.body(), registration)?;
                if work != self.member.work_incarnation() {
                    return Err(unavailable());
                }
                Ok(Some((receipt, work)))
            }
            .await;
            match result {
                Ok(actual) => return Ok(actual),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::Interrupted
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::BrokenPipe
                            | io::ErrorKind::ConnectionReset
                            | io::ErrorKind::ConnectionAborted
                            | io::ErrorKind::ConnectionRefused
                            | io::ErrorKind::UnexpectedEof
                            | io::ErrorKind::WouldBlock
                    ) =>
                {
                    // A lost ACK retains the original actual loaded report;
                    // it cannot trigger another cache write or new attempt.
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                Err(error) => {
                    if matches!(
                        error.kind(),
                        io::ErrorKind::PermissionDenied
                            | io::ErrorKind::Unsupported
                            | io::ErrorKind::InvalidInput
                            | io::ErrorKind::InvalidData
                    ) {
                        self.refused.store(true, Ordering::Release);
                    }
                    return Err(error);
                }
            }
        }
    }

    pub(super) async fn complete(
        self: &Arc<Self>,
        lifetime: Weak<Inner>,
        registration: String,
        loaded: Loaded,
    ) -> io::Result<()> {
        let flight = self.clone();
        let (send, receive) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let Ok(_serial) = flight.serial.acquire().await else {
                let _ = send.send(Err(unavailable()));
                return;
            };
            let result = async {
                if flight.refused.load(Ordering::Acquire) {
                    return Err(unavailable());
                }
                let packet = flight
                    .packet
                    .lock()
                    .map_err(|_| unavailable())?
                    .clone()
                    .ok_or_else(unavailable)?;
                if packet.registration != registration {
                    return Err(unavailable());
                }
                if let Loaded::Source { source, .. } | Loaded::ReaderSource { source } = &loaded
                    && (source.operation != packet.source.operation
                        || source.revision != packet.source.revision
                        || Scope::from(source.as_ref()) != Scope::from(&packet.source))
                {
                    return Err(unavailable());
                }
                if matches!(loaded, Loaded::NoNativeReader(_)) {
                    return Err(unavailable());
                }
                {
                    let mut report = flight.report.lock().map_err(|_| unavailable())?;
                    match report.as_ref() {
                        Some((old_registration, old_loaded))
                            if old_registration != &registration || old_loaded != &loaded =>
                        {
                            return Err(unavailable());
                        }
                        Some(_) => {}
                        None => *report = Some((registration.clone(), loaded.clone())),
                    }
                }
                let (receipt, work) = flight
                    .ready_rpc(&lifetime, &registration, &loaded, false)
                    .await?
                    .ok_or_else(unavailable)?;
                let inner = lifetime.upgrade().ok_or_else(unavailable)?;
                let mut state = inner.state.lock().map_err(|_| unavailable())?;
                if inner.retiring.load(Ordering::Acquire)
                    || !state
                        .flight
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, &flight))
                    || state.registration.as_deref() != Some(&registration)
                {
                    return Err(unavailable());
                }
                inner.ready_work.store(work, Ordering::Release);
                match loaded {
                    Loaded::Source {
                        source,
                        generations,
                    } => {
                        state.snapshot = Some((source.revision, generations));
                        inner.active.store(true, Ordering::Release);
                        inner.ready.store(true, Ordering::Release);
                    }
                    Loaded::ReaderSource { .. } => {
                        state.reader = true;
                        inner.active.store(true, Ordering::Release);
                        inner.ready.store(true, Ordering::Release);
                    }
                    Loaded::PolicyUnavailable => {
                        state.snapshot = None;
                        inner.active.store(false, Ordering::Release);
                        inner.ready.store(false, Ordering::Release);
                    }
                    Loaded::NoNativeReader(_) => return Err(unavailable()),
                }
                flight.attempt.ready(&receipt, &registration)?;
                flight.terminal.store(true, Ordering::Release);
                inner.changed.notify_waiters();
                Ok(())
            }
            .await;
            let _ = send.send(result);
        });
        receive.await.map_err(|_| unavailable())?
    }
}

fn acknowledged_snapshot(state: &State, flight: &Flight) -> io::Result<bool> {
    let acknowledgment = flight.native_ack.lock().map_err(|_| unavailable())?;
    Ok(acknowledgment
        .as_ref()
        .is_some_and(|(registration, loaded, _work)| {
            matches!(loaded, Loaded::Source {source, ..}
            if state.registration.as_ref() == Some(registration)
                && state.preloaded.as_ref() == Some(&source.revision)
                && state.kind.as_ref() == Some(&Kind::Managed(Scope::from(source.as_ref())))
                && Arc::ptr_eq(&state.seal_occurrence, &flight.seal))
        }))
}
pub(super) async fn observe(
    lifetime: &Lifetime,
    preferred: &str,
    borrow: Option<&Borrow>,
) -> io::Result<Preload> {
    observe_kind(lifetime, preferred, borrow, false).await
}
pub(super) async fn observe_rebound(lifetime: &Lifetime, preferred: &str) -> io::Result<Preload> {
    observe_kind(lifetime, preferred, None, true).await
}
pub(super) async fn observe_kind(
    lifetime: &Lifetime,
    preferred: &str,
    borrow: Option<&Borrow>,
    rebound: bool,
) -> io::Result<Preload> {
    if !crate::managed_auth::valid_hex(preferred, 32) || borrow.is_some() {
        return Err(unavailable());
    }
    let activity = crate::assignment::current_activity();
    let inner = lifetime.0.clone();
    let weak = Arc::downgrade(&inner);
    let operation = preferred.to_owned();
    let (send, receive) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = async {
            let creation = inner
                .preload_creation
                .acquire()
                .await
                .map_err(|_| unavailable())?;
            if inner.retiring.load(Ordering::Acquire) {
                return Err(unavailable());
            }
            inner.allocation.member.reconcile_work().await?;
            if !inner.allocation.member.live() {
                return Err(unavailable());
            }
            let prior = inner
                .state
                .lock()
                .map_err(|_| unavailable())?
                .flight
                .clone();
            // A historical PRELOAD reply is correlation only once completion
            // released its read custody. It cannot authorize a fresh local read.
            if prior
                .as_ref()
                .is_some_and(|flight| flight.terminal() && flight.body().operation == operation)
            {
                return Err(unavailable());
            }
            let existing = prior.filter(|flight| !flight.terminal());
            let (flight, first_use): (Arc<Flight>, Option<ParentUseLease>) = match existing {
                Some(flight) => {
                    if flight.rebound != rebound {
                        return Err(unavailable());
                    }
                    (flight, None)
                }
                None => {
                    let selected = Lifetime(inner.clone());
                    // A B parent only admitted this constructor; target custody, not
                    // A/B parent use, selects its initial credential source.
                    let previously_loaded = inner
                        .state
                        .lock()
                        .map_err(|_| unavailable())?
                        .preloaded
                        .is_some();
                    let target_initial = inner.allocation.target.is_some() && !previously_loaded;
                    let seal = {
                        let state = inner.state.lock().map_err(|_| unavailable())?;
                        if rebound
                            && (state.reader
                                || !inner.active.load(Ordering::Acquire)
                                || (!inner.ready.load(Ordering::Acquire)
                                    && !state
                                        .flight
                                        .as_ref()
                                        .map(|flight| acknowledged_snapshot(&state, flight))
                                        .transpose()?
                                        .unwrap_or(false))
                                || !matches!(state.kind, Some(Kind::Managed(_))))
                        {
                            return Err(unavailable());
                        }
                        state.seal_occurrence.clone()
                    };
                    let parent = if rebound || target_initial {
                        None
                    } else if selected.ready() || previously_loaded {
                        match activity.as_ref() {
                            Some(activity) => activity.native_use_for(&selected)?,
                            None => return Err(unavailable()),
                        }
                    } else {
                        inner
                            .allocation
                            .parent_use
                            .as_ref()
                            .map(|parent| parent.upgrade().ok_or_else(unavailable))
                            .transpose()?
                    };
                    let context = match parent.as_ref() {
                        Some(parent) => Some(parent.ready().await?),
                        None => None,
                    };
                    let attempt = inner.preload_order.begin(operation).await?;
                    let flight = Arc::new(Flight {
                        attempt,
                        member: inner.allocation.member.clone(),
                        context,
                        rebound,
                        seal,
                        serial: tokio::sync::Semaphore::new(1),
                        packet: Mutex::new(None),
                        original: Mutex::new(None),
                        steps: Mutex::new(Vec::new()),
                        report: Mutex::new(None),
                        native_ack: Mutex::new(None),
                        terminal: AtomicBool::new(false),
                        refused: AtomicBool::new(false),
                    });
                    let mut state = inner.state.lock().map_err(|_| unavailable())?;
                    if inner.retiring.load(Ordering::Acquire) {
                        return Err(unavailable());
                    }
                    state.attempted_preload = Some(flight.body().clone());
                    state.flight = Some(flight.clone());
                    inner.ready.store(false, Ordering::Release);
                    (flight, parent)
                }
            };
            drop(creation);
            drop(inner);
            // This real work lease survives observer cancellation through the first
            // accepted read. Later completion uses native captured-preload custody.
            let admitted_use = first_use;
            loop {
                let result = flight.fetch(weak.clone(), None).await;
                match result {
                    Ok(Resample::Preload(packet)) => return Ok(packet),
                    Ok(Resample::ReadyCommitted(_)) => return Err(unavailable()),
                    Err(error)
                        if admitted_use.is_none()
                            || matches!(
                                error.kind(),
                                io::ErrorKind::PermissionDenied
                                    | io::ErrorKind::Unsupported
                                    | io::ErrorKind::InvalidInput
                            )
                            || weak
                                .upgrade()
                                .is_none_or(|inner| inner.retiring.load(Ordering::Acquire)) =>
                    {
                        return Err(error);
                    }
                    Err(_) => {
                        // Unknown first admission retains the real owed use. The
                        // member adapter recovers only this existing work identity;
                        // the immutable attempt never becomes a new OPEN or use.
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    }
                }
            }
        }
        .await;
        let _ = send.send(result);
    });
    receive.await.map_err(|_| unavailable())?
}

pub(super) async fn resample(lifetime: &Lifetime, previous: Predecessor) -> io::Result<Resample> {
    let flight = lifetime
        .0
        .state
        .lock()
        .map_err(|_| unavailable())?
        .flight
        .clone()
        .ok_or_else(unavailable)?;
    flight
        .fetch(Arc::downgrade(&lifetime.0), Some(previous))
        .await
}

pub(super) async fn resume_completion(lifetime: &Lifetime) -> io::Result<()> {
    let flight = lifetime
        .0
        .state
        .lock()
        .map_err(|_| unavailable())?
        .flight
        .clone()
        .ok_or_else(unavailable)?;
    let (registration, report) = flight
        .report
        .lock()
        .map_err(|_| unavailable())?
        .clone()
        .ok_or_else(unavailable)?;
    flight
        .complete(Arc::downgrade(&lifetime.0), registration, report)
        .await
}
pub(super) fn retained_completion(lifetime: &Lifetime) -> io::Result<Option<Loaded>> {
    let flight = lifetime
        .0
        .state
        .lock()
        .map_err(|_| unavailable())?
        .flight
        .clone();
    let Some(flight) = flight else {
        return Ok(None);
    };
    let report = flight
        .report
        .lock()
        .map_err(|_| unavailable())?
        .as_ref()
        .map(|(_, report)| report.clone());
    Ok(report)
}
pub(super) async fn resume_source_completion(
    lifetime: &Lifetime,
    source: &Source,
    generations: &Generations,
) -> io::Result<()> {
    let flight = lifetime
        .0
        .state
        .lock()
        .map_err(|_| unavailable())?
        .flight
        .clone()
        .ok_or_else(unavailable)?;
    let (registration, report) = flight
        .report
        .lock()
        .map_err(|_| unavailable())?
        .clone()
        .ok_or_else(unavailable)?;
    if report
        != (Loaded::Source {
            source: Box::new(source.clone()),
            generations: generations.clone(),
        })
    {
        return Err(unavailable());
    }
    flight
        .complete(Arc::downgrade(&lifetime.0), registration, report)
        .await
}

/// Native ACK for the exact previously published snapshot. It deliberately
/// carries no local Ready until the manager verifies its unchanged live cache.
pub enum ReboundPreparation {
    Ready(Box<ReboundReady>),
    SourceChanged,
}
pub struct ReboundReady {
    inner: Weak<Inner>,
    flight: Arc<Flight>,
    seal: Arc<()>,
    source: Source,
    generations: Generations,
    registration: String,
    receipt: crate::native_preload::Ready,
    work: u64,
    fresh: bool,
}
impl ReboundReady {
    pub fn commit(self) -> io::Result<()> {
        let inner = self.inner.upgrade().ok_or_else(unavailable)?;
        // Correlation can settle before the final local publication; it grants
        // no credential access on its own.
        self.flight
            .attempt
            .ready(&self.receipt, &self.registration)?;
        let mut state = inner.state.lock().map_err(|_| unavailable())?;
        if inner.retiring.load(Ordering::Acquire)
            || !Arc::ptr_eq(&state.seal_occurrence, &self.seal)
            || (!self.fresh
                && (!inner.active.load(Ordering::Acquire) || !inner.ready.load(Ordering::Acquire)))
            || !inner.allocation.member.live()
            || inner.allocation.member.work_incarnation() != self.work
            || !state
                .flight
                .as_ref()
                .is_some_and(|flight| Arc::ptr_eq(flight, &self.flight))
            || state.registration.as_deref() != Some(self.registration.as_str())
            || (if self.fresh {
                !self.flight.rebound
                    || state.kind.as_ref() != Some(&Kind::Managed(Scope::from(&self.source)))
                    || state.preloaded.as_ref() != Some(&self.source.revision)
            } else {
                state.snapshot.as_ref()
                    != Some(&(self.source.revision.clone(), self.generations.clone()))
            })
        {
            return Err(unavailable());
        }
        // A retained report only acknowledges this work incarnation. A fresh
        // report publishes its actually loaded snapshot after caller validation.
        if self.fresh {
            state.snapshot = Some((self.source.revision.clone(), self.generations.clone()));
            inner.active.store(true, Ordering::Release);
            inner.ready.store(true, Ordering::Release);
            self.flight.terminal.store(true, Ordering::Release);
        }
        inner.ready_work.store(self.work, Ordering::Release);
        drop(state);
        inner.changed.notify_waiters();
        Ok(())
    }
}

pub(super) async fn prepare_rebound_ready(
    lifetime: &Lifetime,
    source: &Source,
    generations: &Generations,
) -> io::Result<ReboundPreparation> {
    let (flight, seal, registration, fresh) = {
        let state = lifetime.0.state.lock().map_err(|_| unavailable())?;
        let flight = state.flight.clone().ok_or_else(unavailable)?;
        let acknowledged = acknowledged_snapshot(&state, &flight)?;
        let retained = lifetime.0.ready.load(Ordering::Acquire)
            && state.snapshot.as_ref() == Some(&(source.revision.clone(), generations.clone()));
        let report = Loaded::Source {
            source: Box::new(source.clone()),
            generations: generations.clone(),
        };
        let exact_ack = flight
            .native_ack
            .lock()
            .map_err(|_| unavailable())?
            .as_ref()
            .is_some_and(|(registration, old, _work)| {
                state.registration.as_ref() == Some(registration) && old == &report
            });
        if lifetime.0.retiring.load(Ordering::Acquire)
            || !lifetime.0.active.load(Ordering::Acquire)
            || (!retained && !(acknowledged && exact_ack))
            || state.reader
            || state.kind.as_ref() != Some(&Kind::Managed(Scope::from(source)))
        {
            return Err(unavailable());
        }
        (
            flight,
            state.seal_occurrence.clone(),
            state.registration.clone().ok_or_else(unavailable)?,
            !retained,
        )
    };
    let _serial = flight.serial.acquire().await.map_err(|_| unavailable())?;
    let report = Loaded::Source {
        source: Box::new(source.clone()),
        generations: generations.clone(),
    };
    if flight.refused.load(Ordering::Acquire)
        || flight.report.lock().map_err(|_| unavailable())?.as_ref()
            != Some(&(registration.clone(), report.clone()))
    {
        return Err(unavailable());
    }
    let inner = Arc::downgrade(&lifetime.0);
    let Some((receipt, work)) = flight
        .ready_rpc(&inner, &registration, &report, true)
        .await?
    else {
        return Ok(ReboundPreparation::SourceChanged);
    };
    drop(_serial);
    Ok(ReboundPreparation::Ready(Box::new(ReboundReady {
        inner,
        flight,
        seal,
        source: source.clone(),
        generations: generations.clone(),
        registration,
        receipt,
        work,
        fresh,
    })))
}

/// ACK for a newly loaded rebound source. The caller must still validate its
/// actual published cache before committing any local readiness.
pub(super) async fn prepare_loaded_ready(
    lifetime: &Lifetime,
    source: &Source,
    generations: &Generations,
) -> io::Result<ReboundPreparation> {
    let (flight, registration) = {
        let state = lifetime.0.state.lock().map_err(|_| unavailable())?;
        if lifetime.0.retiring.load(Ordering::Acquire)
            || state.reader
            || state.preloaded.as_ref() != Some(&source.revision)
            || state.kind.as_ref() != Some(&Kind::Managed(Scope::from(source)))
        {
            return Err(unavailable());
        }
        (
            state.flight.clone().ok_or_else(unavailable)?,
            state.registration.clone().ok_or_else(unavailable)?,
        )
    };
    let serial = flight.serial.acquire().await.map_err(|_| unavailable())?;
    let packet = flight
        .packet
        .lock()
        .map_err(|_| unavailable())?
        .clone()
        .ok_or_else(unavailable)?;
    if !flight.rebound
        || flight.refused.load(Ordering::Acquire)
        || packet.source != *source
        || packet.registration != registration
    {
        return Err(unavailable());
    }
    let report = Loaded::Source {
        source: Box::new(source.clone()),
        generations: generations.clone(),
    };
    {
        let mut recorded = flight.report.lock().map_err(|_| unavailable())?;
        match recorded.as_ref() {
            Some(old) if old != &(registration.clone(), report.clone()) => {
                return Err(unavailable());
            }
            Some(_) => {}
            None => *recorded = Some((registration.clone(), report.clone())),
        }
    }
    let inner = Arc::downgrade(&lifetime.0);
    let (receipt, work) = flight
        .ready_rpc(&inner, &registration, &report, false)
        .await?
        .ok_or_else(unavailable)?;
    // Actual native ACK settles this read occurrence independently from local
    // credential publication. Its immutable history can own a later read-only
    // rebound if the physical owner advances before the caller's final check.
    flight.attempt.ready(&receipt, &registration)?;
    *flight.native_ack.lock().map_err(|_| unavailable())? =
        Some((registration.clone(), report, work));
    flight.terminal.store(true, Ordering::Release);
    drop(serial);
    // Ordinary ACK replay may be historical after work EOF. Require the
    // explicit physical rebound completion before any local publication.
    prepare_rebound_ready(lifetime, source, generations).await
}
