// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One accepted Source transition on the SAME physical backend/Goal Owner.
//! Native registration precedes Source acceptance. The accepted whole worker
//! owns begin/drain; C1/HTTP loss cannot cancel it. S1 proves Source scheduling
//! custody only, and cannot substitute for Go, S2 UI/currentness or effect closure.
use super::*;
use crate::frontend_control::retained_origin::AcceptedOrigin;
use codex_alab_core::retained_goal_gate::Inhibitor;
use serde::Deserialize;

#[path = "frontend_source_selection_frontier.rs"]
mod frontier;
pub use frontier::SelectionAck;
pub use frontier::SelectionCommit;
pub use frontier::SelectionFrontier;
pub use frontier::SelectionObservation;
pub use frontier::SelectionReleased;
const MAX_SELECTIONS: usize = 4096;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Prepared,
    Registering,
    Registered,
    Beginning,
    Held,
    Entered,
    Retiring,
    Retired,
    Rejected,
    Selected,
    Completing,
    Complete,
    Unknown,
}
struct Progress {
    phase: Phase,
    service: Option<String>,
    selection: Option<String>,
    inhibitor: Option<Arc<Inhibitor>>,
    challenge: Option<([u8; 32], [u8; 32])>,
    entered: bool,
}
pub(super) struct Record {
    source: String,
    origin: AcceptedOrigin,
    progress: Mutex<Progress>,
    changed: watch::Sender<u64>,
    frontiers: Mutex<BTreeMap<String, Arc<frontier::Record>>>,
}
/// Private construction captures the original physical binding and actual A.
/// The source operation/selection strings are correlation DATA, never grants.
pub struct SourceTransition {
    owner: Arc<GoalOwner>,
    record: Arc<Record>,
}
pub struct RegisteredSourceTransition {
    actual: SourceTransition,
}
pub struct HeldSourceTransition {
    actual: SourceTransition,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    schema: u32,
    request: String,
    operation: String,
    member: String,
    session: crate::accounts::Session,
    service: String,
    source_operation: String,
    selection: String,
    state: String,
}
fn operation() -> String {
    format!(
        "{:016x}{:016x}",
        codex_alab_core::native_source::connection_request_prefix(),
        codex_alab_core::native_source::connection_request_prefix()
    )
}
fn changed(record: &Record) {
    record
        .changed
        .send_modify(|value| *value = value.saturating_add(1));
}
fn decode(
    owner: &GoalOwner,
    record: &Record,
    operation: &str,
    body: &str,
    service: &str,
    selection: Option<&str>,
    states: &[&str],
) -> io::Result<Reply> {
    let reply: Reply = serde_json::from_str(body).map_err(|_| unavailable())?;
    if reply.schema != 1
        || !valid_hex(&reply.request, 32)
        || reply.operation != operation
        || reply.member != owner.controller.member
        || reply.session != owner.controller.session
        || reply.service != service
        || reply.source_operation != record.source
        || !valid_hex(&reply.selection, 32)
        || selection.is_some_and(|actual| actual != reply.selection)
        || !states.contains(&reply.state.as_str())
    {
        return Err(unavailable());
    }
    Ok(reply)
}
fn fields(owner: &GoalOwner, record: &Record, service: &str) -> String {
    format!(
        " member={} controller_operation={} service={} source_operation={}",
        owner.controller.member, owner.controller.operation, service, record.source
    )
}
impl GoalOwner {
    pub(in crate::frontend_control) fn prepare_source_transition(
        self: &Arc<Self>,
        origin: &AcceptedOrigin,
    ) -> io::Result<SourceTransition> {
        self.live()?;
        if !origin.controlled_by(&self.controller) {
            return Err(unavailable());
        }
        let source = operation();
        let (changed, _) = watch::channel(0);
        let record = Arc::new(Record {
            source: source.clone(),
            origin: origin.clone(),
            changed,
            frontiers: Mutex::new(BTreeMap::new()),
            progress: Mutex::new(Progress {
                phase: Phase::Prepared,
                service: None,
                selection: None,
                inhibitor: None,
                challenge: None,
                entered: false,
            }),
        });
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        self.admitting_source()?;
        let original = state
            .origins
            .get(&origin.attachment())
            .and_then(crate::frontend_control::retained_origin::WeakOrigin::upgrade)
            .ok_or_else(unavailable)?;
        if !original.same(origin)
            || state.source_selections.len() >= MAX_SELECTIONS
            || state.source_selections.contains_key(&source)
        {
            return Err(unavailable());
        }
        state.source_selections.insert(source, record.clone());
        Ok(SourceTransition {
            owner: self.clone(),
            record,
        })
    }
    pub(in crate::frontend_control) fn accept_source_inhibition(
        self: &Arc<Self>,
        packet: super::super::control_frame::Source,
    ) -> io::Result<()> {
        self.live()?;
        if tokio::time::Instant::now() > packet.deadline {
            return Err(unavailable());
        }
        let source = std::str::from_utf8(&packet.parent).map_err(|_| unavailable())?;
        let mut tasks = self.tasks.lock().map_err(|_| unavailable())?;
        self.live()?;
        let record = self
            .state
            .lock()
            .map_err(|_| unavailable())?
            .source_selections
            .get(source)
            .cloned()
            .ok_or_else(unavailable)?;
        {
            let mut progress = record.progress.lock().map_err(|_| unavailable())?;
            if !matches!(
                progress.phase,
                Phase::Beginning | Phase::Held | Phase::Entered
            ) || progress.selection.as_deref()
                != Some(std::str::from_utf8(&packet.operation).map_err(|_| unavailable())?)
                || record.origin.attachment() != packet.attachment
                || !progress
                    .inhibitor
                    .as_ref()
                    .is_some_and(|ticket| ticket.same_gate(&self.gate) && ticket.is_held())
            {
                return Err(unavailable());
            }
            if let Some((nonce, attachment)) = progress.challenge {
                return if nonce == packet.nonce && attachment == packet.attachment {
                    Ok(())
                } else {
                    Err(unavailable())
                };
            }
            // Same record and ticket are stored BEFORE ACK/spawn. No original
            // frontend-current check is allowed after actual Source acceptance.
            progress.challenge = Some((packet.nonce, packet.attachment));
        }
        let worker = self.clone();
        let accepted = record.clone();
        let guard = Guard {
            owner: worker.clone(),
            record,
            completed: false,
        };
        tasks.push(tokio::spawn(async move {
            let mut guard = guard;
            let result = tokio::select! {
                biased;
                _ = worker.withdrawn() => Err(unavailable()),
                result = tokio::time::timeout_at(packet.deadline, async {
                    HeldSourceTransition { actual: SourceTransition { owner: worker.clone(), record: accepted } }.recheck_ticket()?;
                    control(&worker.controller, "controller-retained-frontier-ack",
                        std::str::from_utf8(&packet.operation).map_err(|_| unavailable())?,
                        Some(std::str::from_utf8(&packet.nonce).map_err(|_| unavailable())?)).await?;
                    worker.live()
                }) => result.map_err(|_| io::Error::from(io::ErrorKind::TimedOut)).and_then(std::convert::identity),
            };
            if result.is_ok() { guard.completed = true; }
        }));
        Ok(())
    }
}
impl SourceTransition {
    /// Called BEFORE Source advertises actual acceptance. This observes Native
    /// registration, never a Source worker/model/SDK permission. Cancellation
    /// only drops this observation; the original owned registration continues.
    pub async fn register(&self) -> Result<RegisteredSourceTransition, RuntimeObservationError> {
        {
            let mut tasks = self.owner.tasks.lock().map_err(|_| unavailable())?;
            self.owner.live()?;
            let mut progress = self.record.progress.lock().map_err(|_| unavailable())?;
            if progress.phase == Phase::Prepared {
                self.owner.admitting_source()?;
                progress.phase = Phase::Registering;
                drop(progress); // Preguard Drop must never reacquire this held mutex.
                let actual = Self {
                    owner: self.owner.clone(),
                    record: self.record.clone(),
                };
                let guard = Guard {
                    owner: actual.owner.clone(),
                    record: actual.record.clone(),
                    completed: false,
                };
                tasks.push(tokio::spawn(async move {
                    let mut guard = guard;
                    if actual.owner.reap_finished_source_tasks().await.is_err() {
                        return;
                    }
                    let current = match actual.record.origin.observe_retained_current().await {
                        Ok(current) => current,
                        Err(crate::source_member::ObservationError::Refused(_)) => {
                            if let Ok(mut progress) = actual.record.progress.lock() {
                                progress.phase = Phase::Rejected;
                            }
                            changed(&actual.record);
                            guard.completed = true;
                            return;
                        }
                        Err(crate::source_member::ObservationError::Uncertain(_)) => return,
                    };
                    let service = current.service_data();
                    let op = operation();
                    let attachment = actual.record.origin.attachment();
                    let Ok(attachment) = std::str::from_utf8(&attachment) else {
                        return;
                    };
                    let fields = format!(
                        "{} attachment={}",
                        fields(&actual.owner, &actual.record, service),
                        attachment
                    );
                    let body = match actual
                        .owner
                        .controller
                        .transport
                        .observe_retained(
                            crate::source_member::RetainedQuery::SourceSelectionRegister,
                            &op,
                            &fields,
                            &actual.owner.controller.member,
                        )
                        .await
                    {
                        Ok(body) => body,
                        Err(crate::source_member::ObservationError::Refused(_)) => {
                            // A register handler may have inserted Reserved
                            // before its final backend check refused. Preserve
                            // the original channel, but never infer no record.
                            // Resolve exact unaccepted custody on this channel.
                            if actual
                                .cancel_registration_attempt(service, attachment)
                                .await
                                .is_err()
                            {
                                return;
                            }
                            if let Ok(mut progress) = actual.record.progress.lock() {
                                progress.phase = Phase::Rejected;
                            }
                            changed(&actual.record);
                            guard.completed = true;
                            return;
                        }
                        Err(crate::source_member::ObservationError::Uncertain(_)) => return,
                    };
                    let reply = match decode(
                        &actual.owner,
                        &actual.record,
                        &op,
                        &body,
                        service,
                        None,
                        &["registered"],
                    ) {
                        Ok(reply) => reply,
                        Err(_) => return,
                    };
                    if actual.owner.live().is_err() {
                        return;
                    }
                    let Ok(mut progress) = actual.record.progress.lock() else {
                        return;
                    };
                    if progress.phase != Phase::Registering {
                        return;
                    }
                    progress.service = Some(reply.service);
                    progress.selection = Some(reply.selection);
                    progress.phase = Phase::Registered;
                    drop(progress);
                    changed(&actual.record);
                    guard.completed = true;
                }));
            }
        }
        let mut updates = self.record.changed.subscribe();
        loop {
            self.owner.live()?;
            match self
                .record
                .progress
                .lock()
                .map_err(|_| unavailable())?
                .phase
            {
                Phase::Registered => {
                    return Ok(RegisteredSourceTransition {
                        actual: Self {
                            owner: self.owner.clone(),
                            record: self.record.clone(),
                        },
                    });
                }
                Phase::Prepared | Phase::Registering => {}
                Phase::Rejected => return Err(RuntimeObservationError::Refused(unavailable())),
                _ => return Err(RuntimeObservationError::Uncertain(unavailable())),
            }
            tokio::select! { biased; _ = self.owner.withdrawn() => return Err(RuntimeObservationError::Uncertain(unavailable())),
            result = updates.changed() => result.map_err(|_| unavailable())?, }
        }
    }
}
impl RegisteredSourceTransition {
    pub fn source_operation_data(&self) -> &str {
        &self.actual.record.source
    }
    /// Exact observed Native reservation selector DATA for the preflight offer;
    /// no Native role/currentness follows from its presence or serialization.
    pub fn selection_data(&self) -> io::Result<String> {
        self.actual.owner.live()?;
        let progress = self
            .actual
            .record
            .progress
            .lock()
            .map_err(|_| unavailable())?;
        if !matches!(
            progress.phase,
            Phase::Registered | Phase::Beginning | Phase::Held | Phase::Entered
        ) {
            return Err(unavailable());
        }
        progress.selection.clone().ok_or_else(unavailable)
    }
    /// Only the accepted whole Source worker calls this. The SAME actual Owner
    /// drains fresh automatic windows and installs a separate ticket even while
    /// initial/H1 holds remain. C1 loss after registration cannot cancel it.
    pub async fn begin(&self) -> io::Result<HeldSourceTransition> {
        {
            let mut tasks = self.actual.owner.tasks.lock().map_err(|_| unavailable())?;
            self.actual.owner.live()?;
            let mut progress = self
                .actual
                .record
                .progress
                .lock()
                .map_err(|_| unavailable())?;
            if progress.phase == Phase::Registered {
                self.actual.owner.admitting_source()?;
                progress.phase = Phase::Beginning;
                drop(progress);
                let actual = SourceTransition {
                    owner: self.actual.owner.clone(),
                    record: self.actual.record.clone(),
                };
                let guard = Guard {
                    owner: actual.owner.clone(),
                    record: actual.record.clone(),
                    completed: false,
                };
                tasks.push(tokio::spawn(async move {
                    let mut guard = guard;
                    let ticket = match actual.owner.owner.inhibit().await {
                        Ok(ticket) => Arc::new(ticket),
                        Err(_) => return,
                    };
                    {
                        let Ok(mut progress) = actual.record.progress.lock() else {
                            return;
                        };
                        if progress.phase != Phase::Beginning {
                            return;
                        }
                        progress.inhibitor = Some(ticket);
                    }
                    let (service, selection) = match selectors(&actual.record) {
                        Ok(value) => value,
                        Err(_) => return,
                    };
                    loop {
                        if actual.owner.live().is_err() {
                            return;
                        }
                        let op = operation();
                        let fields = format!(
                            "{} selection={selection}",
                            fields(&actual.owner, &actual.record, &service)
                        );
                        let body = match actual
                            .owner
                            .controller
                            .transport
                            .call(
                                "controller-source-selection-inhibit",
                                &op,
                                &fields,
                                Some(&actual.owner.controller.member),
                            )
                            .await
                        {
                            Ok(body) => body,
                            Err(_) => return,
                        };
                        let reply = match decode(
                            &actual.owner,
                            &actual.record,
                            &op,
                            &body,
                            &service,
                            Some(&selection),
                            &["waiting", "inhibited"],
                        ) {
                            Ok(reply) => reply,
                            Err(_) => return,
                        };
                        if reply.state == "inhibited" {
                            let Ok(mut progress) = actual.record.progress.lock() else {
                                return;
                            };
                            if progress.phase != Phase::Beginning
                                || progress.challenge.is_none()
                                || !progress
                                    .inhibitor
                                    .as_ref()
                                    .is_some_and(|ticket| ticket.is_held())
                            {
                                return;
                            }
                            progress.phase = Phase::Held;
                            drop(progress);
                            changed(&actual.record);
                            guard.completed = true;
                            return;
                        }
                        tokio::select! { biased; _ = actual.owner.withdrawn() => return,
                        _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {} }
                    }
                }));
            }
        }
        let mut updates = self.actual.record.changed.subscribe();
        loop {
            self.actual.owner.live()?;
            match self
                .actual
                .record
                .progress
                .lock()
                .map_err(|_| unavailable())?
                .phase
            {
                Phase::Held | Phase::Entered => {
                    return Ok(HeldSourceTransition {
                        actual: SourceTransition {
                            owner: self.actual.owner.clone(),
                            record: self.actual.record.clone(),
                        },
                    });
                }
                Phase::Beginning => {}
                _ => return Err(unavailable()),
            }
            tokio::select! { biased; _ = self.actual.owner.withdrawn() => return Err(unavailable()),
            result = updates.changed() => result.map_err(|_| unavailable())?, }
        }
    }
}
fn selectors(record: &Record) -> io::Result<(String, String)> {
    let progress = record.progress.lock().map_err(|_| unavailable())?;
    Ok((
        progress.service.clone().ok_or_else(unavailable)?,
        progress.selection.clone().ok_or_else(unavailable)?,
    ))
}
impl HeldSourceTransition {
    pub fn source_operation_data(&self) -> &str {
        &self.actual.record.source
    }
    /// Actual stored Native reservation selector DATA only. No ID is derived
    /// from a candidate/Core/label, and its presence supplies no currentness.
    pub fn selection_data(&self) -> io::Result<String> {
        self.recheck_held()?;
        selectors(&self.actual.record).map(|(_, selection)| selection)
    }
    pub fn goal_gate(&self) -> Gate {
        self.actual.owner.gate()
    }
    fn recheck_ticket(&self) -> io::Result<()> {
        self.actual.owner.live()?;
        let progress = self
            .actual
            .record
            .progress
            .lock()
            .map_err(|_| unavailable())?;
        if !matches!(
            progress.phase,
            Phase::Beginning | Phase::Held | Phase::Entered
        ) || !progress
            .inhibitor
            .as_ref()
            .is_some_and(|ticket| ticket.same_gate(&self.actual.owner.gate) && ticket.is_held())
        {
            return Err(unavailable());
        }
        Ok(())
    }
    pub fn recheck_held(&self) -> io::Result<()> {
        self.recheck_ticket()?;
        let progress = self
            .actual
            .record
            .progress
            .lock()
            .map_err(|_| unavailable())?;
        if !matches!(progress.phase, Phase::Held | Phase::Entered) {
            return Err(unavailable());
        }
        Ok(())
    }
    /// Invoke synchronously at actual Core entry, without holding a Source
    /// Scope progress lock. Retirement serializes this exact SDK phase first;
    /// the Source original worker still owns its own actual entry/capture facts.
    pub fn mark_core_entry(&self) -> io::Result<()> {
        // SAME spawn/admission lock as begin_source_shutdown. An already
        // entered original worker keeps its real capture/cleanup obligations.
        let _tasks = self.actual.owner.tasks.lock().map_err(|_| unavailable())?;
        self.actual.owner.admitting_source()?;
        self.actual.owner.live()?;
        let mut progress = self
            .actual
            .record
            .progress
            .lock()
            .map_err(|_| unavailable())?;
        if !matches!(progress.phase, Phase::Held | Phase::Entered)
            || !progress
                .inhibitor
                .as_ref()
                .is_some_and(|ticket| ticket.is_held())
        {
            return Err(unavailable());
        }
        progress.entered = true;
        progress.phase = Phase::Entered;
        Ok(())
    }
}
struct Guard {
    owner: Arc<GoalOwner>,
    record: Arc<Record>,
    completed: bool,
}
impl Drop for Guard {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        self.record
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .phase = Phase::Unknown;
        changed(&self.record);
        self.owner
            .failed
            .store(true, std::sync::atomic::Ordering::Release);
        self.owner.withdraw();
        self.owner.controller.transport.release();
    }
}

impl RegisteredSourceTransition {
    /// Exact Native reservation retirement for the Source registry's rejected
    /// UNACCEPTED handoff. SDK phase serialization prevents begin/Core entry.
    /// A lost/unknown reply retains custody and withdraws, never reopens.
    pub async fn retire_unaccepted(&self) -> io::Result<()> {
        self.actual.retire(Retirement::Unaccepted, || Ok(())).await
    }
}
impl HeldSourceTransition {
    /// Actual Source caller must own a normally joined original terminal worker
    /// with !entered && created=None && resolved=None. The repeated callback
    /// checks those real immutable facts; no error text, caller metadata or
    /// Native reply can establish them. Loaded/resolved failures require UI
    /// reconciliation. Entered/no-capture and worker loss remain Unknown.
    pub async fn retire_before_core<C>(&self, check: C) -> io::Result<()>
    where
        C: Fn() -> Result<(), String> + Send + Sync + 'static,
    {
        self.actual.retire(Retirement::BeforeCore, check).await
    }
}
impl HeldSourceTransition {
    /// Only the original Source cleanup owner may use this after its normally
    /// joined failed NEW Core has genuinely terminated. The callback must retain
    /// and compare the exact captured Arc, observed terminal status and unchanged
    /// prior UI-primary owner. Existing resolved Cores and unknown births must
    /// not use this route. This proves no Native effects/reader settlement.
    pub async fn retire_after_new_core_terminated<C>(&self, check: C) -> io::Result<()>
    where
        C: Fn() -> Result<(), String> + Send + Sync + 'static,
    {
        self.actual.retire(Retirement::TerminatedNew, check).await
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Retirement {
    Unaccepted,
    BeforeCore,
    TerminatedNew,
}
impl SourceTransition {
    async fn retire<C>(&self, mode: Retirement, check: C) -> io::Result<()>
    where
        C: Fn() -> Result<(), String> + Send + Sync + 'static,
    {
        {
            let mut tasks = self.owner.tasks.lock().map_err(|_| unavailable())?;
            self.owner.live()?;
            let mut progress = self.record.progress.lock().map_err(|_| unavailable())?;
            if progress.phase == Phase::Retired {
                return Ok(());
            }
            if progress.phase != Phase::Retiring {
                let expected_entry = mode == Retirement::TerminatedNew;
                let allowed = match mode {
                    Retirement::Unaccepted => progress.phase == Phase::Registered,
                    Retirement::BeforeCore => progress.phase == Phase::Held,
                    Retirement::TerminatedNew => progress.phase == Phase::Entered,
                };
                if !allowed || progress.entered != expected_entry || check().is_err() {
                    return Err(unavailable());
                }
                // SAME phase lock as mark_core_entry. Never hold a Source
                // Scope mutex while calling mark_core_entry/retirement.
                progress.phase = Phase::Retiring;
                drop(progress);
                let actual = Self {
                    owner: self.owner.clone(),
                    record: self.record.clone(),
                };
                let guard = Guard {
                    owner: actual.owner.clone(),
                    record: actual.record.clone(),
                    completed: false,
                };
                tasks.push(tokio::spawn(async move {
                    let mut guard = guard;
                    let (service, selection) = match selectors(&actual.record) {
                        Ok(value) => value,
                        Err(_) => return,
                    };
                    let op = operation();
                    let fields = format!(
                        "{} selection={selection}",
                        fields(&actual.owner, &actual.record, &service)
                    );
                    let body = match actual
                        .owner
                        .controller
                        .transport
                        .call(
                            "controller-source-selection-retire",
                            &op,
                            &fields,
                            Some(&actual.owner.controller.member),
                        )
                        .await
                    {
                        Ok(body) => body,
                        Err(_) => return,
                    };
                    if decode(
                        &actual.owner,
                        &actual.record,
                        &op,
                        &body,
                        &service,
                        Some(&selection),
                        &["retired"],
                    )
                    .is_err()
                    {
                        return;
                    }
                    let ticket = match actual.record.progress.lock() {
                        Ok(progress)
                            if progress.phase == Phase::Retiring
                                && progress.entered == expected_entry =>
                        {
                            progress.inhibitor.clone()
                        }
                        _ => return,
                    };
                    if let Some(ticket) = ticket {
                        if actual
                            .owner
                            .owner
                            .release_inhibitor_if(&ticket, || {
                                actual.owner.live().map_err(|error| error.to_string())?;
                                let progress = actual
                                    .record
                                    .progress
                                    .lock()
                                    .map_err(|_| "selection custody poisoned".to_owned())?;
                                if progress.phase != Phase::Retiring
                                    || progress.entered != expected_entry
                                {
                                    return Err("selection entry custody changed".into());
                                }
                                check()
                            })
                            .await
                            .is_err()
                        {
                            return;
                        }
                    } else if mode != Retirement::Unaccepted || check().is_err() {
                        return;
                    }
                    // The only caller-owned destructor in this SDK worker is
                    // the terminal Source callback. Observe its destruction
                    // BEFORE publishing a normal retirement. A destructor panic
                    // keeps Unknown via the original pre-spawn guard.
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(check)))
                        .is_err()
                    {
                        return;
                    }
                    let Ok(mut progress) = actual.record.progress.lock() else {
                        return;
                    };
                    if progress.phase != Phase::Retiring || progress.entered != expected_entry {
                        return;
                    }
                    progress.inhibitor.take();
                    progress.phase = Phase::Retired;
                    drop(progress);
                    changed(&actual.record);
                    guard.completed = true;
                }));
            }
        }
        let mut updates = self.record.changed.subscribe();
        loop {
            self.owner.live()?;
            match self
                .record
                .progress
                .lock()
                .map_err(|_| unavailable())?
                .phase
            {
                Phase::Retired => return Ok(()),
                Phase::Retiring => {}
                _ => return Err(unavailable()),
            }
            tokio::select! { biased; _ = self.owner.withdrawn() => return Err(unavailable()),
            result = updates.changed() => result.map_err(|_| unavailable())?, }
        }
    }
}

impl GoalOwner {
    fn forget_known_source(&self, actual: &Arc<Record>) {
        // Lock order is always owner state -> record progress, matching the
        // physical S2 dispatcher. Never retain the progress guard while waiting
        // for state. These phases have either no Native registration, or an
        // actually observed explicit operation retirement; they claim no
        // effects/reader settlement. Active and Unknown are never removed here.
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let progress = actual
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(
            progress.phase,
            Phase::Prepared | Phase::Rejected | Phase::Retired | Phase::Complete
        ) && state
            .source_selections
            .get(&actual.source)
            .is_some_and(|stored| Arc::ptr_eq(stored, actual))
        {
            state.source_selections.remove(&actual.source);
        }
    }
}
impl SourceTransition {
    /// Local unregistered-resource retirement only. Prepared has not started
    /// any Native registration; Rejected already observed the original exact
    /// refusal/cancellation. Registering/reserved/accepted/Unknown never use
    /// this route. This reports no Native effects/readers or role settlement.
    pub fn discard_unregistered(&self) -> io::Result<()> {
        {
            let mut state = self.owner.state.lock().map_err(|_| unavailable())?;
            let mut progress = self.record.progress.lock().map_err(|_| unavailable())?;
            if !discardable_unregistered(progress.phase) {
                return Err(unavailable());
            }
            match state.source_selections.get(&self.record.source) {
                Some(stored) if Arc::ptr_eq(stored, &self.record) => {}
                None if matches!(progress.phase, Phase::Rejected | Phase::Retired) => {}
                _ => return Err(unavailable()),
            }
            progress.phase = Phase::Retired;
            state.source_selections.remove(&self.record.source);
        }
        changed(&self.record);
        Ok(())
    }
}
fn discardable_unregistered(phase: Phase) -> bool {
    matches!(phase, Phase::Prepared | Phase::Rejected | Phase::Retired)
}
impl Drop for SourceTransition {
    fn drop(&mut self) {
        self.owner.forget_known_source(&self.record);
    }
}

#[cfg(test)]
#[path = "frontend_source_unregistered_discard_tests.rs"]
mod unregistered_discard_tests;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UnacceptedAbsent {
    schema: u32,
    request: String,
    operation: String,
    member: String,
    session: crate::accounts::Session,
    service: String,
    source_operation: String,
    attachment: String,
    state: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UnacceptedRetired {
    schema: u32,
    request: String,
    operation: String,
    member: String,
    session: crate::accounts::Session,
    service: String,
    source_operation: String,
    attachment: String,
    state: String,
    selection: String,
}
impl SourceTransition {
    async fn cancel_registration_attempt(&self, service: &str, attachment: &str) -> io::Result<()> {
        let op = operation();
        let fields = format!(
            "{} attachment={attachment}",
            fields(&self.owner, &self.record, service)
        );
        let body = self
            .owner
            .controller
            .transport
            .call(
                "controller-source-selection-cancel-unaccepted",
                &op,
                &fields,
                Some(&self.owner.controller.member),
            )
            .await?;
        macro_rules! matches_reply {
            ($value:ident,$state:literal) => {
                $value.schema == 1
                    && valid_hex(&$value.request, 32)
                    && $value.operation == op
                    && $value.member == self.owner.controller.member
                    && $value.session == self.owner.controller.session
                    && $value.service == service
                    && $value.source_operation == self.record.source
                    && $value.attachment == attachment
                    && $value.state == $state
            };
        }
        let absent = serde_json::from_str::<UnacceptedAbsent>(&body)
            .is_ok_and(|value| matches_reply!(value, "unaccepted-absent"));
        let retired = serde_json::from_str::<UnacceptedRetired>(&body).is_ok_and(|value| {
            matches_reply!(value, "unaccepted-retired") && valid_hex(&value.selection, 32)
        });
        if !absent && !retired {
            self.owner.controller.transport.release();
            return Err(unavailable());
        }
        self.owner.live()
    }
}

impl GoalOwner {
    /// Completed-handle housekeeping cannot wait behind a shutdown observer
    /// which may already own and be awaiting THIS registered worker.
    async fn reap_finished_source_tasks(&self) -> io::Result<()> {
        if reap_ready_source_tasks(&self.tasks, &self.joins)
            .await
            .is_err()
        {
            self.failed
                .store(true, std::sync::atomic::Ordering::Release);
            self.withdraw();
            self.controller.transport.release();
            return Err(unavailable());
        }
        self.live()
    }
}

impl std::fmt::Debug for SourceTransition {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("SourceTransition(<same actual resource; no Native grant>)")
    }
}

impl std::fmt::Debug for RegisteredSourceTransition {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("RegisteredSourceTransition(<same actual resource; no Native grant>)")
    }
}

impl std::fmt::Debug for HeldSourceTransition {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("HeldSourceTransition(<same actual resource; no Native grant>)")
    }
}
