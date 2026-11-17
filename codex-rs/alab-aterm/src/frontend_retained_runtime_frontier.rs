// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The actual backend control packet and same physical A receipt. Native owns
//! Go/UI admission; these types only correlate its real observations and the
//! Source inhibitor. The app-server consumer must use its captured Core/lease.
use super::*;
use crate::frontend_control::retained_origin::AcceptedOrigin;
use crate::frontend_control::retained_origin::CurrentOrigin;
use crate::source_member::ObservationError;
use crate::source_member::RetainedQuery;
use serde::Deserialize;

/// A correlated authenticated Native negative is a denial, never a grant.
/// Every other error retains uncertainty; no caller can construct a receipt.
#[derive(Debug)]
pub enum RuntimeObservationError {
    Refused(io::Error),
    Uncertain(io::Error),
}
impl RuntimeObservationError {
    pub fn into_io(self) -> io::Error {
        match self {
            Self::Refused(error) | Self::Uncertain(error) => error,
        }
    }
}
impl From<io::Error> for RuntimeObservationError {
    fn from(error: io::Error) -> Self {
        Self::Uncertain(error)
    }
}
impl From<ObservationError> for RuntimeObservationError {
    fn from(error: ObservationError) -> Self {
        match error {
            ObservationError::Refused(error) => Self::Refused(error),
            ObservationError::Uncertain(error) => Self::Uncertain(error),
        }
    }
}
use std::sync::Weak;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RuntimePhase {
    Received,
    Acknowledging,
    Acknowledged,
    Activating,
    Observed,
    Committing,
    Committed,
    Transferred,
    Releasing,
    Released,
    Unknown,
}
pub(super) struct Record {
    go: Option<String>,
    frontier: String,
    nonce: [u8; 32],
    attachment: [u8; 32],
    origin: crate::frontend_control::retained_origin::WeakOrigin,
    phase: Mutex<RuntimePhase>,
    activation: Mutex<Option<RuntimeActivation>>,
    commit: Mutex<Option<RuntimeCommit>>,
}
#[derive(Clone)]
pub struct RuntimeFrontier {
    owner: Arc<GoalOwner>,
    record: Arc<Record>,
    origin: AcceptedOrigin,
}
#[derive(Clone)]
pub struct RuntimeAck {
    frontier: RuntimeFrontier,
    current: CurrentOrigin,
}
/// A genuine Native observation, not a durable/current grant from its presence.
#[derive(Clone)]
pub struct RuntimeActivation {
    owner: Weak<GoalOwner>,
    record: Weak<Record>,
    service: String,
    go: Option<String>,
    frontier: String,
    human: u64,
    observation: u64,
}
/// Only a genuine final Native query can construct this. The earlier
/// RuntimeActivation stage cannot release Source scheduling admission.
#[derive(Clone)]
pub struct RuntimeCommit {
    activation: RuntimeActivation,
    app_owner: Option<String>,
}
impl RuntimeCommit {
    /// Exact receipt-resource correlation only. Presence or equality is not a
    /// current Native grant; the accepted consumer must query and recheck.
    pub fn same_frontier(&self, actual: &RuntimeFrontier) -> bool {
        self.activation.same_frontier(actual)
    }
}
impl std::fmt::Debug for RuntimeCommit {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("RuntimeCommit(<actual Native final current role; no lasting grant>)")
    }
}
impl RuntimeActivation {
    pub fn service_data(&self) -> &str {
        &self.service
    }
    pub fn go_data(&self) -> Option<&str> {
        self.go.as_deref()
    }
    pub fn frontier_data(&self) -> &str {
        &self.frontier
    }
    pub fn human_position_data(&self) -> u64 {
        self.human
    }
    pub fn same_frontier(&self, actual: &RuntimeFrontier) -> bool {
        self.owner.ptr_eq(&Arc::downgrade(&actual.owner))
            && self.record.ptr_eq(&Arc::downgrade(&actual.record))
    }
}
impl std::fmt::Debug for RuntimeFrontier {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(
            "RuntimeFrontier(<original physical control/A; requires runtime reconciliation>)",
        )
    }
}
impl std::fmt::Debug for RuntimeAck {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("RuntimeAck(<original physical runtime receipt>)")
    }
}
impl std::fmt::Debug for RuntimeActivation {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("RuntimeActivation(<Native observation; no lasting grant>)")
    }
}

impl GoalOwner {
    pub(in crate::frontend_control) fn register_origin(
        &self,
        origin: &AcceptedOrigin,
    ) -> io::Result<()> {
        self.live()?;
        if !origin.controlled_by(&self.controller) {
            return Err(unavailable());
        }
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        state.origins.retain(|_, origin| origin.upgrade().is_some());
        if state.origins.contains_key(&origin.attachment()) || state.origins.len() >= 4096 {
            return Err(unavailable());
        }
        state
            .origins
            .insert(origin.attachment(), origin.downgrade());
        Ok(())
    }
    pub(in crate::frontend_control) fn accept_runtime(
        &self,
        packet: super::super::control_frame::Runtime,
    ) -> io::Result<()> {
        self.live()?;
        if tokio::time::Instant::now() > packet.deadline {
            return Err(unavailable());
        }
        let key = text(&packet.frontier)?.to_owned();
        let go = packet
            .go
            .as_ref()
            .map(text)
            .transpose()?
            .map(ToOwned::to_owned);
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        if let Some(existing) = state.runtimes.get(&key) {
            return if existing.go == go
                && existing.frontier == key
                && existing.nonce == packet.nonce
                && existing.attachment == packet.attachment
            {
                Ok(())
            } else {
                Err(unavailable())
            };
        }
        if state.runtimes.len() >= 4096 {
            return Err(unavailable());
        }
        let origin = state
            .origins
            .get(&packet.attachment)
            .and_then(crate::frontend_control::retained_origin::WeakOrigin::upgrade)
            .ok_or_else(unavailable)?;
        state.runtimes.insert(
            key.clone(),
            Arc::new(Record {
                go,
                frontier: key,
                nonce: packet.nonce,
                attachment: packet.attachment,
                origin: origin.downgrade(),
                phase: Mutex::new(RuntimePhase::Received),
                activation: Mutex::new(None),
                commit: Mutex::new(None),
            }),
        );
        drop(state);
        self.notify();
        Ok(())
    }
    pub(in crate::frontend_control) async fn frontier(
        self: &Arc<Self>,
        frontier: &str,
        origin: &AcceptedOrigin,
    ) -> io::Result<RuntimeFrontier> {
        self.admitting_source()?;
        if !valid_hex(frontier, 32) || !origin.controlled_by(&self.controller) {
            return Err(unavailable());
        }
        origin.check_current().await?;
        let mut changed = self.changed.subscribe();
        tokio::time::timeout(IO_DEADLINE, async {
            loop {
                self.admitting_source()?;
                let found = self
                    .state
                    .lock()
                    .map_err(|_| unavailable())?
                    .runtimes
                    .get(frontier)
                    .cloned();
                if let Some(record) = found {
                    let original = record.origin.upgrade().ok_or_else(unavailable)?;
                    if !original.same(origin) {
                        return Err(unavailable());
                    }
                    return Ok(RuntimeFrontier {
                        owner: self.clone(),
                        record,
                        origin: origin.clone(),
                    });
                }
                tokio::select! {
                    biased;
                    _ = self.withdrawn() => return Err(unavailable()),
                    result = changed.changed() => result.map_err(|_| unavailable())?,
                }
            }
        })
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
    }
    pub(super) fn notify(&self) {
        self.changed.send_modify(|value| {
            *value = value.saturating_add(1);
        });
    }
    fn hold_for(&self, record: &Record) -> io::Result<Option<Arc<Hold>>> {
        self.selection_hold(record.go.as_deref())
    }
    async fn release(
        &self,
        record: &Record,
        check: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), ReleaseError> {
        let hold = self.hold_for(record).map_err(ReleaseError::Unchanged)?;
        let Some(hold) = hold else {
            self.live().map_err(ReleaseError::Unchanged)?;
            return check().map_err(|_| ReleaseError::Unchanged(unavailable()));
        };
        // Owner::release_if guarantees no change on any returned error. A
        // failed local check keeps this SAME Hold closed and retryable; it is
        // not transport/Native uncertainty and does not revoke the controller.
        self.owner
            .release_if(&hold, check)
            .await
            .map_err(|_| ReleaseError::Unchanged(unavailable()))?;
        // The owning backend operation cannot be canceled by its UI observer.
        // No await between real release, retiring the SAME stored Hold and
        // waking the next H1. Failure here withdraws future Source admission.
        let mut state = self
            .state
            .lock()
            .map_err(|_| ReleaseError::Uncertain(unavailable()))?;
        match &record.go {
            None if state
                .initial
                .as_ref()
                .is_some_and(|actual| Arc::ptr_eq(actual, &hold)) =>
            {
                state.initial.take();
            }
            Some(go) => {
                let key: &[u8; 32] = go
                    .as_bytes()
                    .try_into()
                    .map_err(|_| ReleaseError::Uncertain(unavailable()))?;
                let accepted = state
                    .replacements
                    .get(key)
                    .ok_or_else(|| ReleaseError::Uncertain(unavailable()))?;
                let mut actual = accepted
                    .hold
                    .lock()
                    .map_err(|_| ReleaseError::Uncertain(unavailable()))?;
                if !actual
                    .as_ref()
                    .is_some_and(|actual| Arc::ptr_eq(actual, &hold))
                {
                    return Err(ReleaseError::Uncertain(unavailable()));
                }
                actual.take();
            }
            _ => return Err(ReleaseError::Uncertain(unavailable())),
        }
        drop(state);
        self.notify();
        Ok(())
    }
}

impl RuntimeFrontier {
    pub fn frontier_data(&self) -> &str {
        &self.record.frontier
    }
    pub fn go_data(&self) -> Option<&str> {
        self.record.go.as_deref()
    }
    pub fn goal_gate(&self) -> Gate {
        self.owner.gate()
    }
    pub fn same_receipt(&self, actual: &AcceptedOrigin) -> bool {
        self.origin.same(actual)
    }

    /// Invoke from the original serialized listener AFTER actual same-Core
    /// history/settings/pending-request replay. It only ACKs this physical
    /// packet; Native UI/Go observation is still independently required.
    pub async fn acknowledge_reconciled(
        &self,
        actual: &AcceptedOrigin,
    ) -> Result<RuntimeAck, RuntimeObservationError> {
        self.acknowledge_projection(actual).await
    }

    // Same real R0/R1 packet, Native current A query and original owned Hold.
    // The two public consumers retain distinct Core/configured Source policy.
    async fn acknowledge_projection(
        &self,
        actual: &AcceptedOrigin,
    ) -> Result<RuntimeAck, RuntimeObservationError> {
        self.owner.live()?;
        if !self.same_receipt(actual) {
            return Err(unavailable().into());
        }
        let current = actual.observe_retained_current().await?;
        let _hold = self.owner.hold_for(&self.record)?;
        let phase = *self.record.phase.lock().map_err(|_| unavailable())?;
        if matches!(
            phase,
            RuntimePhase::Acknowledged
                | RuntimePhase::Observed
                | RuntimePhase::Committed
                | RuntimePhase::Released
        ) {
            return Ok(RuntimeAck {
                frontier: self.clone(),
                current,
            });
        }
        {
            let mut phase = self.record.phase.lock().map_err(|_| unavailable())?;
            if *phase != RuntimePhase::Received {
                return Err(unavailable().into());
            }
            *phase = RuntimePhase::Acknowledging;
        }
        let mut guard = RuntimeGuard {
            frontier: self.clone(),
            complete: false,
        };
        control(
            &self.owner.controller,
            "controller-retained-frontier-ack",
            self.frontier_data(),
            Some(text(&self.record.nonce)?),
        )
        .await?;
        self.owner.live()?;
        *self.record.phase.lock().map_err(|_| unavailable())? = RuntimePhase::Acknowledged;
        guard.complete = true;
        Ok(RuntimeAck {
            frontier: self.clone(),
            current,
        })
    }
}
impl RuntimeAck {
    pub fn same_frontier(&self, frontier: &RuntimeFrontier) -> bool {
        Arc::ptr_eq(&self.frontier.owner, &frontier.owner)
            && Arc::ptr_eq(&self.frontier.record, &frontier.record)
    }
    /// Call only after the actual frontend's Native UI ACK. A genuine Native
    /// observation comes first; the closure can only check/remove this Source
    /// scheduling inhibitor, never grant Native/model permission itself.
    pub async fn observe_activation(&self) -> Result<RuntimeActivation, RuntimeObservationError> {
        self.frontier.owner.live()?;
        if !self.current.same_receipt(&self.frontier.origin) {
            return Err(unavailable().into());
        }
        let previous_phase = {
            let mut phase = self
                .frontier
                .record
                .phase
                .lock()
                .map_err(|_| unavailable())?;
            if !matches!(
                *phase,
                RuntimePhase::Acknowledged
                    | RuntimePhase::Observed
                    | RuntimePhase::Committed
                    | RuntimePhase::Released
            ) {
                return Err(unavailable().into());
            }
            let previous = *phase;
            *phase = RuntimePhase::Activating;
            previous
        };
        let mut guard = RuntimeGuard {
            frontier: self.frontier.clone(),
            complete: false,
        };
        let observation = async {
            let current = self.frontier.origin.observe_retained_current().await?;
            if current.service_data() != self.current.service_data() {
                return Err(ObservationError::Uncertain(unavailable()));
            }
            observe_native(&self.frontier, current.service_data()).await
        }
        .await;
        let mut observed = match observation {
            Ok(observed) => observed,
            Err(ObservationError::Refused(error)) => {
                // Only a correlated authenticated Native negative permits
                // another observation on this SAME controller. It cannot ACK,
                // release, recreate or open the existing closed Source Hold.
                *self
                    .frontier
                    .record
                    .phase
                    .lock()
                    .map_err(|_| unavailable())? = previous_phase;
                guard.complete = true;
                return Err(RuntimeObservationError::Refused(error));
            }
            Err(ObservationError::Uncertain(error)) => {
                return Err(RuntimeObservationError::Uncertain(error));
            }
        };
        let previous = self
            .frontier
            .record
            .activation
            .lock()
            .map_err(|_| unavailable())?
            .as_ref()
            .map_or(0, |value| value.observation);
        observed.observation = previous.checked_add(1).ok_or_else(unavailable)?;
        *self
            .frontier
            .record
            .activation
            .lock()
            .map_err(|_| unavailable())? = Some(observed.clone());
        self.frontier
            .record
            .commit
            .lock()
            .map_err(|_| unavailable())?
            .take();
        *self
            .frontier
            .record
            .phase
            .lock()
            .map_err(|_| unavailable())? = RuntimePhase::Observed;
        guard.complete = true;
        Ok(observed)
    }
    /// Keep the SAME Hold closed while the actual UI promotes the SAME Go
    /// into Native App custody. This final query verifies that real App lease;
    /// caller labels and the earlier stage receipt cannot substitute.
    pub async fn observe_commit(
        &self,
        staged: &RuntimeActivation,
    ) -> Result<RuntimeCommit, RuntimeObservationError> {
        self.frontier.owner.live()?;
        if !staged.same_frontier(&self.frontier) {
            return Err(unavailable().into());
        }
        let previous_phase = {
            let mut phase = self
                .frontier
                .record
                .phase
                .lock()
                .map_err(|_| unavailable())?;
            let stage = self
                .frontier
                .record
                .activation
                .lock()
                .map_err(|_| unavailable())?
                .clone()
                .ok_or_else(unavailable)?;
            if !matches!(
                *phase,
                RuntimePhase::Observed | RuntimePhase::Committed | RuntimePhase::Released
            ) || stage.observation != staged.observation
            {
                return Err(unavailable().into());
            }
            let previous = *phase;
            *phase = RuntimePhase::Committing;
            previous
        };
        let mut guard = RuntimeGuard {
            frontier: self.frontier.clone(),
            complete: false,
        };
        let observed = observe_native_commit(&self.frontier, staged).await;
        let committed = match observed {
            Ok(commit) => commit,
            Err(ObservationError::Refused(error)) => {
                *self
                    .frontier
                    .record
                    .phase
                    .lock()
                    .map_err(|_| unavailable())? = previous_phase;
                guard.complete = true;
                return Err(RuntimeObservationError::Refused(error));
            }
            Err(ObservationError::Uncertain(error)) => {
                return Err(RuntimeObservationError::Uncertain(error));
            }
        };
        *self
            .frontier
            .record
            .commit
            .lock()
            .map_err(|_| unavailable())? = Some(committed.clone());
        *self
            .frontier
            .record
            .phase
            .lock()
            .map_err(|_| unavailable())? = RuntimePhase::Committed;
        guard.complete = true;
        Ok(committed)
    }
    /// The actual app-server performs its LAST async manager Current query
    /// after the final Native App/Go/frontier query under the original lifecycle
    /// lock, then repeats local ownership/hold facts in this closure.
    pub async fn release_committed(
        &self,
        observed: &RuntimeCommit,
        check: impl FnOnce() -> Result<(), String>,
    ) -> io::Result<()> {
        let _mutation = self
            .frontier
            .owner
            .mutation
            .acquire()
            .await
            .map_err(|_| unavailable())?;
        self.frontier.owner.live()?;
        if !observed.activation.same_frontier(&self.frontier) {
            return Err(unavailable());
        }
        {
            let mut phase = self
                .frontier
                .record
                .phase
                .lock()
                .map_err(|_| unavailable())?;
            let current = self
                .frontier
                .record
                .commit
                .lock()
                .map_err(|_| unavailable())?
                .clone()
                .ok_or_else(unavailable)?;
            if !matches!(*phase, RuntimePhase::Committed | RuntimePhase::Released)
                || current.activation.observation != observed.activation.observation
                || current.app_owner != observed.app_owner
            {
                return Err(unavailable());
            }
            *phase = RuntimePhase::Releasing;
        }
        let mut guard = RuntimeGuard {
            frontier: self.frontier.clone(),
            complete: false,
        };
        match self
            .frontier
            .owner
            .release(&self.frontier.record, check)
            .await
        {
            Ok(()) => {}
            Err(ReleaseError::Unchanged(error)) => {
                *self
                    .frontier
                    .record
                    .phase
                    .lock()
                    .map_err(|_| unavailable())? = RuntimePhase::Committed;
                guard.complete = true;
                return Err(error);
            }
            Err(ReleaseError::Uncertain(error)) => return Err(error),
        }
        *self
            .frontier
            .record
            .phase
            .lock()
            .map_err(|_| unavailable())? = RuntimePhase::Released;
        guard.complete = true;
        Ok(())
    }
}
enum ReleaseError {
    Unchanged(io::Error),
    Uncertain(io::Error),
}
struct RuntimeGuard {
    frontier: RuntimeFrontier,
    complete: bool,
}
impl Drop for RuntimeGuard {
    fn drop(&mut self) {
        if !self.complete {
            *self
                .frontier
                .record
                .phase
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = RuntimePhase::Unknown;
            self.frontier
                .owner
                .failed
                .store(true, std::sync::atomic::Ordering::Release);
            self.frontier.owner.withdraw();
        }
    }
}
fn text(bytes: &[u8; 32]) -> io::Result<&str> {
    std::str::from_utf8(bytes)
        .ok()
        .filter(|value| valid_hex(value, 32))
        .ok_or_else(unavailable)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InitialActivation {
    schema: u32,
    request: String,
    operation: String,
    member: String,
    session: crate::accounts::Session,
    service: String,
    state: String,
    frontier: String,
    human: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplacementActivation {
    schema: u32,
    request: String,
    operation: String,
    member: String,
    session: crate::accounts::Session,
    service: String,
    state: String,
    go: String,
    frontier: String,
    human: u64,
}
async fn observe_native(
    frontier: &RuntimeFrontier,
    service: &str,
) -> Result<RuntimeActivation, ObservationError> {
    let context = &frontier.owner.controller;
    let operation = format!(
        "{:016x}{:016x}",
        codex_alab_core::native_source::connection_request_prefix(),
        codex_alab_core::native_source::connection_request_prefix()
    );
    let go = frontier.go_data();
    let fields = format!(
        " member={} controller_operation={} service={service} frontier={}{}",
        context.member,
        context.operation,
        frontier.frontier_data(),
        go.map_or(String::new(), |go| format!(" go={go}"))
    );
    let action = if go.is_some() {
        RetainedQuery::Activation
    } else {
        RetainedQuery::InitialActivation
    };
    let body = context
        .transport
        .observe_retained(action, &operation, &fields, &context.member)
        .await?;
    let decoded = (|| -> io::Result<RuntimeActivation> {
        let (
            schema,
            request,
            actual_operation,
            member,
            session,
            actual_service,
            state,
            actual_go,
            actual_frontier,
            human,
        ) = if let Some(go) = go {
            let value: ReplacementActivation =
                serde_json::from_str(&body).map_err(|_| unavailable())?;
            if value.go != go {
                return Err(unavailable());
            }
            (
                value.schema,
                value.request,
                value.operation,
                value.member,
                value.session,
                value.service,
                value.state,
                Some(value.go),
                value.frontier,
                value.human,
            )
        } else {
            let value: InitialActivation =
                serde_json::from_str(&body).map_err(|_| unavailable())?;
            (
                value.schema,
                value.request,
                value.operation,
                value.member,
                value.session,
                value.service,
                value.state,
                None,
                value.frontier,
                value.human,
            )
        };
        let expected_state = if go.is_some() {
            "activated"
        } else {
            "initial-activated"
        };
        if schema != 1
            || !valid_hex(&request, 32)
            || actual_operation != operation
            || member != context.member
            || session != context.session
            || actual_service != service
            || actual_frontier != frontier.frontier_data()
            || state != expected_state
        {
            return Err(unavailable());
        }
        frontier.owner.live()?;
        Ok(RuntimeActivation {
            owner: Arc::downgrade(&frontier.owner),
            record: Arc::downgrade(&frontier.record),
            service: actual_service,
            go: actual_go,
            frontier: actual_frontier,
            human,
            observation: 0,
        })
    })();
    if decoded.is_err() {
        context.transport.release();
    }
    decoded.map_err(ObservationError::Uncertain)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplacementCommit {
    schema: u32,
    request: String,
    operation: String,
    member: String,
    session: crate::accounts::Session,
    service: String,
    state: String,
    go: String,
    frontier: String,
    human: u64,
    app_owner: String,
}
async fn observe_native_commit(
    frontier: &RuntimeFrontier,
    staged: &RuntimeActivation,
) -> Result<RuntimeCommit, ObservationError> {
    if frontier.go_data().is_none() {
        // The genuine original creator/App/birth route remains separate. It
        // rechecks all its actual current facts; it cannot commit replacement.
        let observed = observe_native(frontier, &staged.service).await?;
        if observed.human != staged.human {
            frontier.owner.controller.transport.release();
            return Err(ObservationError::Uncertain(unavailable()));
        }
        return Ok(RuntimeCommit {
            activation: staged.clone(),
            app_owner: None,
        });
    }
    let context = &frontier.owner.controller;
    let operation = format!(
        "{:016x}{:016x}",
        codex_alab_core::native_source::connection_request_prefix(),
        codex_alab_core::native_source::connection_request_prefix()
    );
    let go = frontier
        .go_data()
        .ok_or_else(|| ObservationError::Uncertain(unavailable()))?;
    let fields = format!(
        " member={} controller_operation={} service={} go={go} frontier={}",
        context.member,
        context.operation,
        staged.service,
        frontier.frontier_data()
    );
    let body = context
        .transport
        .observe_retained(
            RetainedQuery::ActivationCommit,
            &operation,
            &fields,
            &context.member,
        )
        .await?;
    let checked = (|| {
        let value: ReplacementCommit = serde_json::from_str(&body).map_err(|_| unavailable())?;
        if value.schema != 1
            || !valid_hex(&value.request, 32)
            || value.operation != operation
            || value.member != context.member
            || value.session != context.session
            || value.service != staged.service
            || value.go != go
            || value.frontier != frontier.frontier_data()
            || value.state != "committed"
            || value.human != staged.human
            || !valid_hex(&value.app_owner, 32)
        {
            return Err(unavailable());
        }
        frontier.owner.live()?;
        Ok(RuntimeCommit {
            activation: staged.clone(),
            app_owner: Some(value.app_owner),
        })
    })();
    if checked.is_err() {
        context.transport.release();
    }
    checked.map_err(ObservationError::Uncertain)
}

#[path = "frontend_retained_closed_hold.rs"]
mod closed_hold;
pub use closed_hold::ClosedCustody;
pub use closed_hold::PendingHold;

#[path = "frontend_retained_configured_runtime.rs"]
mod configured;
pub use configured::ConfiguredRuntimeAck;
pub use configured::ConfiguredRuntimeActivation;
pub use configured::ConfiguredRuntimeCommit;
pub use configured::ConfiguredRuntimeFrontier;
