// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Distinct S2 physical and authenticated Native observation custody. These
//! cannot be constructed from R0/R1, RuntimeCommit, caller metadata or a bool.
//! The actual Source listener/UI/selected-primary consumer still owns its work.
use super::*;
use crate::frontend_control::retained_origin::WeakOrigin;
use crate::source_member::ObservationError;
use crate::source_member::RetainedQuery;
use std::sync::Weak;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Received,
    Acknowledging,
    Acknowledged,
    Observing,
    Observed,
    Committing,
    Committed,
    Released,
    Unknown,
}
pub(super) struct Record {
    frontier: String,
    nonce: [u8; 32],
    attachment: [u8; 32],
    origin: WeakOrigin,
    stage: Mutex<Stage>,
    observation: Mutex<Option<SelectionObservation>>,
    commit: Mutex<Option<SelectionCommit>>,
}
#[derive(Clone)]
pub struct SelectionFrontier {
    owner: Arc<GoalOwner>,
    source: Arc<super::Record>,
    actual: Arc<Record>,
    origin: AcceptedOrigin,
    // Selector DATA until the genuine Native action validates exact Go/Birth.
    go: Option<String>,
}
#[derive(Clone)]
pub struct SelectionAck {
    frontier: SelectionFrontier,
}
#[derive(Clone)]
pub struct SelectionObservation {
    source: Weak<super::Record>,
    actual: Weak<Record>,
    owner: Weak<GoalOwner>,
    service: String,
    go: Option<String>,
    human: u64,
    sequence: u64,
}
#[derive(Clone)]
pub struct SelectionCommit {
    observed: SelectionObservation,
    app_owner: Option<String>,
}
/// Actual SAME Owner scheduling release completion only. It is not a Native
/// currentness/effects/reader-settlement grant and has no public constructor.
#[derive(Clone)]
pub struct SelectionReleased {
    source: Weak<super::Record>,
    actual: Weak<Record>,
    owner: Weak<GoalOwner>,
    sequence: u64,
}
impl SelectionReleased {
    pub fn same_transition(&self, held: &HeldSourceTransition) -> bool {
        self.source.ptr_eq(&Arc::downgrade(&held.actual.record))
            && self.owner.ptr_eq(&Arc::downgrade(&held.actual.owner))
    }
}
impl SelectionCommit {
    pub fn same_frontier(&self, actual: &SelectionFrontier) -> bool {
        self.observed.same_frontier(actual)
    }
}
impl SelectionObservation {
    pub fn same_frontier(&self, actual: &SelectionFrontier) -> bool {
        self.source.ptr_eq(&Arc::downgrade(&actual.source))
            && self.actual.ptr_eq(&Arc::downgrade(&actual.actual))
            && self.owner.ptr_eq(&Arc::downgrade(&actual.owner))
            && self.go == actual.go
    }
    pub fn service_data(&self) -> &str {
        &self.service
    }
    pub fn go_data(&self) -> Option<&str> {
        self.go.as_deref()
    }
}
impl GoalOwner {
    pub(in crate::frontend_control) fn accept_source_selection(
        self: &Arc<Self>,
        packet: super::super::super::control_frame::Source,
    ) -> io::Result<()> {
        self.live()?;
        if tokio::time::Instant::now() > packet.deadline {
            return Err(unavailable());
        }
        let native = std::str::from_utf8(&packet.parent).map_err(|_| unavailable())?;
        let state = self.state.lock().map_err(|_| unavailable())?;
        let source = state
            .source_selections
            .values()
            .find_map(|record| {
                record
                    .progress
                    .lock()
                    .ok()
                    .filter(|progress| progress.selection.as_deref() == Some(native))
                    .map(|_| record.clone())
            })
            .ok_or_else(unavailable)?;
        let origin = state
            .origins
            .get(&packet.attachment)
            .and_then(WeakOrigin::upgrade)
            .ok_or_else(unavailable)?;
        drop(state);
        HeldSourceTransition {
            actual: SourceTransition {
                owner: self.clone(),
                record: source.clone(),
            },
        }
        .recheck_held()?;
        let key = std::str::from_utf8(&packet.operation)
            .map_err(|_| unavailable())?
            .to_owned();
        let mut frontiers = source.frontiers.lock().map_err(|_| unavailable())?;
        if let Some(old) = frontiers.get(&key) {
            return if old.nonce == packet.nonce
                && old.attachment == packet.attachment
                && old
                    .origin
                    .upgrade()
                    .is_some_and(|actual| actual.same(&origin))
            {
                Ok(())
            } else {
                Err(unavailable())
            };
        }
        if frontiers.len() >= MAX_SELECTIONS {
            return Err(unavailable());
        }
        frontiers.insert(
            key.clone(),
            Arc::new(Record {
                frontier: key,
                nonce: packet.nonce,
                attachment: packet.attachment,
                origin: origin.downgrade(),
                stage: Mutex::new(Stage::Received),
                observation: Mutex::new(None),
                commit: Mutex::new(None),
            }),
        );
        drop(frontiers);
        changed(&source);
        Ok(())
    }
}
impl HeldSourceTransition {
    pub async fn frontier(
        &self,
        frontier: &str,
        go: Option<&str>,
        origin: &AcceptedOrigin,
    ) -> io::Result<SelectionFrontier> {
        self.actual.owner.admitting_source()?;
        if !valid_hex(frontier, 32)
            || go.is_some_and(|go| !valid_hex(go, 32))
            || !origin.controlled_by(&self.actual.owner.controller)
        {
            return Err(unavailable());
        }
        origin.check_current().await?;
        let mut updates = self.actual.record.changed.subscribe();
        let mut owner_updates = self.actual.owner.changed.subscribe();
        tokio::time::timeout(IO_DEADLINE, async {
            loop {
                self.actual.owner.admitting_source()?;
                self.recheck_held()?;
                let actual = self.actual.record.frontiers.lock().map_err(|_| unavailable())?.get(frontier).cloned();
                if let Some(actual) = actual {
                    if actual.attachment != origin.attachment() || !actual.origin.upgrade().is_some_and(|stored| stored.same(origin)) {
                        return Err(unavailable());
                    }
                    return Ok(SelectionFrontier { owner: self.actual.owner.clone(), source: self.actual.record.clone(),
                        actual, origin: origin.clone(), go: go.map(ToOwned::to_owned) });
                }
                tokio::select! { biased; _ = self.actual.owner.withdrawn() => return Err(unavailable()),
                    result = owner_updates.changed() => result.map_err(|_| unavailable())?,
                    result = updates.changed() => result.map_err(|_| unavailable())?, }
            }
        }).await.map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
    }
}
impl SelectionFrontier {
    pub fn frontier_data(&self) -> &str {
        &self.actual.frontier
    }
    pub fn source_operation_data(&self) -> &str {
        &self.source.source
    }
    pub fn go_data(&self) -> Option<&str> {
        self.go.as_deref()
    }
    pub fn goal_gate(&self) -> Gate {
        self.owner.gate()
    }
    pub fn same_receipt(&self, actual: &AcceptedOrigin) -> bool {
        self.origin.same(actual)
    }
    pub fn same_transition(&self, actual: &HeldSourceTransition) -> bool {
        Arc::ptr_eq(&self.owner, &actual.actual.owner)
            && Arc::ptr_eq(&self.source, &actual.actual.record)
    }
    fn recheck(&self) -> io::Result<()> {
        HeldSourceTransition {
            actual: SourceTransition {
                owner: self.owner.clone(),
                record: self.source.clone(),
            },
        }
        .recheck_held()
    }
    /// Only the original serialized listener, AFTER real same-Core replay and
    /// history/settings/pending-request projection, may acknowledge this packet.
    pub async fn acknowledge_reconciled(
        &self,
        actual: &AcceptedOrigin,
    ) -> Result<SelectionAck, RuntimeObservationError> {
        self.recheck()?;
        if !self.same_receipt(actual) {
            return Err(unavailable().into());
        }
        actual.observe_retained_current().await?;
        {
            let mut stage = self.actual.stage.lock().map_err(|_| unavailable())?;
            if matches!(
                *stage,
                Stage::Acknowledged | Stage::Observed | Stage::Committed
            ) {
                return Ok(SelectionAck {
                    frontier: self.clone(),
                });
            }
            if *stage != Stage::Received {
                return Err(unavailable().into());
            }
            *stage = Stage::Acknowledging;
        }
        let mut guard = Guard {
            frontier: self.clone(),
            completed: false,
        };
        control(
            &self.owner.controller,
            "controller-retained-frontier-ack",
            &self.actual.frontier,
            Some(std::str::from_utf8(&self.actual.nonce).map_err(|_| unavailable())?),
        )
        .await?;
        self.recheck()?;
        *self.actual.stage.lock().map_err(|_| unavailable())? = Stage::Acknowledged;
        guard.completed = true;
        Ok(SelectionAck {
            frontier: self.clone(),
        })
    }
}
impl SelectionAck {
    pub fn same_frontier(&self, actual: &SelectionFrontier) -> bool {
        Arc::ptr_eq(&self.frontier.owner, &actual.owner)
            && Arc::ptr_eq(&self.frontier.source, &actual.source)
            && Arc::ptr_eq(&self.frontier.actual, &actual.actual)
            && self.frontier.go == actual.go
    }
    pub async fn observe_selection(&self) -> Result<SelectionObservation, RuntimeObservationError> {
        self.frontier.recheck()?;
        let previous = {
            let mut stage = self
                .frontier
                .actual
                .stage
                .lock()
                .map_err(|_| unavailable())?;
            if !matches!(
                *stage,
                Stage::Acknowledged | Stage::Observed | Stage::Committed
            ) {
                return Err(unavailable().into());
            }
            let old = *stage;
            *stage = Stage::Observing;
            old
        };
        let mut guard = Guard {
            frontier: self.frontier.clone(),
            completed: false,
        };
        let value = match observe(&self.frontier, false).await {
            Ok(value) => value,
            Err(ObservationError::Refused(error)) => {
                *self
                    .frontier
                    .actual
                    .stage
                    .lock()
                    .map_err(|_| unavailable())? = previous;
                guard.completed = true;
                return Err(RuntimeObservationError::Refused(error));
            }
            Err(error) => return Err(error.into()),
        };
        let mut stored = self
            .frontier
            .actual
            .observation
            .lock()
            .map_err(|_| unavailable())?;
        let sequence = stored
            .as_ref()
            .map_or(0, |old| old.sequence)
            .checked_add(1)
            .ok_or_else(unavailable)?;
        let value = SelectionObservation {
            source: Arc::downgrade(&self.frontier.source),
            actual: Arc::downgrade(&self.frontier.actual),
            owner: Arc::downgrade(&self.frontier.owner),
            service: value.service,
            go: value.go,
            human: value.human,
            sequence,
        };
        *stored = Some(value.clone());
        drop(stored);
        self.frontier
            .actual
            .commit
            .lock()
            .map_err(|_| unavailable())?
            .take();
        *self
            .frontier
            .actual
            .stage
            .lock()
            .map_err(|_| unavailable())? = Stage::Observed;
        guard.completed = true;
        Ok(value)
    }
    pub async fn observe_commit(
        &self,
        observed: &SelectionObservation,
    ) -> Result<SelectionCommit, RuntimeObservationError> {
        self.frontier.recheck()?;
        if !observed.same_frontier(&self.frontier) {
            return Err(unavailable().into());
        }
        let previous = {
            let mut stage = self
                .frontier
                .actual
                .stage
                .lock()
                .map_err(|_| unavailable())?;
            let stored = self
                .frontier
                .actual
                .observation
                .lock()
                .map_err(|_| unavailable())?
                .clone()
                .ok_or_else(unavailable)?;
            if !matches!(*stage, Stage::Observed | Stage::Committed)
                || stored.sequence != observed.sequence
            {
                return Err(unavailable().into());
            }
            let old = *stage;
            *stage = Stage::Committing;
            old
        };
        let mut guard = Guard {
            frontier: self.frontier.clone(),
            completed: false,
        };
        let value = match observe(&self.frontier, true).await {
            Ok(value) => value,
            Err(ObservationError::Refused(error)) => {
                *self
                    .frontier
                    .actual
                    .stage
                    .lock()
                    .map_err(|_| unavailable())? = previous;
                guard.completed = true;
                return Err(RuntimeObservationError::Refused(error));
            }
            Err(error) => return Err(error.into()),
        };
        if value.human != observed.human
            || value.service != observed.service
            || value.go != observed.go
        {
            return Err(unavailable().into());
        }
        let committed = SelectionCommit {
            observed: observed.clone(),
            app_owner: value.app_owner,
        };
        *self
            .frontier
            .actual
            .commit
            .lock()
            .map_err(|_| unavailable())? = Some(committed.clone());
        *self
            .frontier
            .actual
            .stage
            .lock()
            .map_err(|_| unavailable())? = Stage::Committed;
        guard.completed = true;
        Ok(committed)
    }
}
macro_rules! native_reply {
    ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct $name {
            schema: u32, request: String, operation: String, member: String,
            session: crate::accounts::Session, service: String, source_operation: String,
            selection: String, frontier: String, state: String, human: u64,
            $($field: $ty),*
        }
    };
}
native_reply!(InitialReply {});
native_reply!(ReplacementReply { go: String });
native_reply!(ReplacementCommitReply {
    go: String,
    app_owner: String
});
enum NativeReply {
    Initial(InitialReply),
    Replacement(ReplacementReply),
    Commit(ReplacementCommitReply),
}
struct Parts<'a> {
    schema: u32,
    request: &'a str,
    operation: &'a str,
    member: &'a str,
    session: &'a crate::accounts::Session,
    service: &'a str,
    source: &'a str,
    selection: &'a str,
    frontier: &'a str,
    state: &'a str,
    human: u64,
    go: Option<&'a str>,
    app_owner: Option<&'a str>,
}
impl NativeReply {
    fn decode(body: &str, replacement: bool, commit: bool) -> io::Result<Self> {
        match (replacement, commit) {
            (false, _) => serde_json::from_str(body).map(Self::Initial),
            (true, false) => serde_json::from_str(body).map(Self::Replacement),
            (true, true) => serde_json::from_str(body).map(Self::Commit),
        }
        .map_err(|_| unavailable())
    }
    fn parts(&self) -> Parts<'_> {
        macro_rules! parts {
            ($value:ident, $go:expr, $owner:expr) => {
                Parts {
                    schema: $value.schema,
                    request: &$value.request,
                    operation: &$value.operation,
                    member: &$value.member,
                    session: &$value.session,
                    service: &$value.service,
                    source: &$value.source_operation,
                    selection: &$value.selection,
                    frontier: &$value.frontier,
                    state: &$value.state,
                    human: $value.human,
                    go: $go,
                    app_owner: $owner,
                }
            };
        }
        match self {
            Self::Initial(value) => parts!(value, None, None),
            Self::Replacement(value) => parts!(value, Some(&value.go), None),
            Self::Commit(value) => parts!(value, Some(&value.go), Some(&value.app_owner)),
        }
    }
}
struct Native {
    service: String,
    go: Option<String>,
    human: u64,
    app_owner: Option<String>,
}
async fn observe(frontier: &SelectionFrontier, commit: bool) -> Result<Native, ObservationError> {
    let current = frontier.origin.observe_retained_current().await?;
    let (service, selection) = selectors(&frontier.source).map_err(ObservationError::Uncertain)?;
    if current.service_data() != service {
        return Err(ObservationError::Uncertain(unavailable()));
    }
    let op = operation();
    let mut fields = format!(
        "{} selection={} frontier={}",
        fields(&frontier.owner, &frontier.source, &service),
        selection,
        frontier.actual.frontier
    );
    if let Some(go) = &frontier.go {
        fields.push_str(&format!(" go={go}"));
    }
    let query = if commit {
        RetainedQuery::SourceSelectionCommit
    } else {
        RetainedQuery::SourceSelectionObserve
    };
    let body = frontier
        .owner
        .controller
        .transport
        .observe_retained(query, &op, &fields, &frontier.owner.controller.member)
        .await?;
    let checked = (|| {
        // Separate closed Serde structs reject duplicate/extra fields and
        // go:null; initial has no go/app_owner fields at all.
        let decoded = NativeReply::decode(&body, frontier.go.is_some(), commit)?;
        let value = decoded.parts();
        if value.schema != 1
            || !valid_hex(value.request, 32)
            || value.operation != op
            || value.member != frontier.owner.controller.member
            || value.session != &frontier.owner.controller.session
            || value.service != service
            || value.source != frontier.source.source
            || value.selection != selection
            || value.frontier != frontier.actual.frontier
            || value.state
                != if commit {
                    "selection-committed"
                } else {
                    "selection-observed"
                }
            || value.go != frontier.go.as_deref()
            || value.app_owner.is_some_and(|owner| !valid_hex(owner, 32))
        {
            return Err(unavailable());
        }
        frontier.recheck()?;
        Ok(Native {
            service,
            go: frontier.go.clone(),
            human: value.human,
            app_owner: value.app_owner.map(ToOwned::to_owned),
        })
    })();
    if checked.is_err() {
        frontier.owner.controller.transport.release();
    }
    checked.map_err(ObservationError::Uncertain)
}
struct Guard {
    frontier: SelectionFrontier,
    completed: bool,
}
impl Drop for Guard {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        *self
            .frontier
            .actual
            .stage
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Stage::Unknown;
        self.frontier
            .owner
            .failed
            .store(true, std::sync::atomic::Ordering::Release);
        self.frontier.owner.withdraw();
        self.frontier.owner.controller.transport.release();
    }
}

#[cfg(test)]
#[path = "frontend_source_selection_codec_tests.rs"]
mod tests;

impl GoalOwner {
    // One exact Source ledger lookup shared with the original Runtime release.
    // A moved-out Hold is NEVER confused with a genuinely completed release.
    pub(in crate::frontend_control::retained_backend::goal) fn selection_hold(
        &self,
        go: Option<&str>,
    ) -> io::Result<Option<Arc<Hold>>> {
        let state = self.state.lock().map_err(|_| unavailable())?;
        match go {
            None if state.initial_transferred => Err(unavailable()),
            None => Ok(state.initial.clone()),
            Some(go) => {
                let key: &[u8; 32] = go.as_bytes().try_into().map_err(|_| unavailable())?;
                let accepted = state.replacements.get(key).ok_or_else(unavailable)?;
                if *accepted.phase.borrow() != super::super::Phase::Acknowledged
                    || accepted
                        .transferred
                        .load(std::sync::atomic::Ordering::Acquire)
                {
                    return Err(unavailable());
                }
                Ok(accepted.hold.lock().map_err(|_| unavailable())?.clone())
            }
        }
    }
    fn retire_selection_hold(&self, go: Option<&str>, hold: &Arc<Hold>) -> io::Result<()> {
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        match go {
            None if !state.initial_transferred
                && state
                    .initial
                    .as_ref()
                    .is_some_and(|actual| Arc::ptr_eq(actual, hold)) =>
            {
                state.initial.take();
            }
            Some(go) => {
                let key: &[u8; 32] = go.as_bytes().try_into().map_err(|_| unavailable())?;
                let accepted = state.replacements.get(key).ok_or_else(unavailable)?;
                if accepted
                    .transferred
                    .load(std::sync::atomic::Ordering::Acquire)
                {
                    return Err(unavailable());
                }
                let mut stored = accepted.hold.lock().map_err(|_| unavailable())?;
                if !stored
                    .as_ref()
                    .is_some_and(|actual| Arc::ptr_eq(actual, hold))
                {
                    return Err(unavailable());
                }
                stored.take();
            }
            _ => return Err(unavailable()),
        }
        drop(state);
        self.notify();
        Ok(())
    }
}
impl SelectionAck {
    /// The actual Source owner calls this only after final native S2 commit,
    /// then its LAST actual Core Current await under the original lifecycle/
    /// goal lock. The closure repeats exact scope/candidate/lease/version facts
    /// and installs the selected owner. Native labels/RuntimeCommit cannot enter.
    pub async fn release_committed(
        &self,
        held: &HeldSourceTransition,
        committed: &SelectionCommit,
        check: impl FnOnce() -> Result<(), String>,
    ) -> io::Result<SelectionReleased> {
        self.release_selection(held, committed, Release::Main, check)
            .await
    }
    /// A real side-conversation UI frontier can retire only its own Source
    /// ticket. It neither installs the selected primary nor releases a closed
    /// initial/H1 Hold, including when the actual current primary slot is Empty.
    pub async fn release_ticket_committed(
        &self,
        held: &HeldSourceTransition,
        committed: &SelectionCommit,
        check: impl FnOnce() -> Result<(), String>,
    ) -> io::Result<SelectionReleased> {
        self.release_selection(held, committed, Release::Side, check)
            .await
    }
    async fn release_selection(
        &self,
        held: &HeldSourceTransition,
        committed: &SelectionCommit,
        mode: Release,
        check: impl FnOnce() -> Result<(), String>,
    ) -> io::Result<SelectionReleased> {
        let _mutation = self
            .frontier
            .owner
            .mutation
            .acquire()
            .await
            .map_err(|_| unavailable())?;
        self.frontier.recheck()?;
        if !self.frontier.same_transition(held) || !committed.same_frontier(&self.frontier) {
            return Err(unavailable());
        }
        {
            let stage = self
                .frontier
                .actual
                .stage
                .lock()
                .map_err(|_| unavailable())?;
            let stored = self
                .frontier
                .actual
                .commit
                .lock()
                .map_err(|_| unavailable())?
                .clone()
                .ok_or_else(unavailable)?;
            if !matches!(*stage, Stage::Committed | Stage::Released)
                || stored.observed.sequence != committed.observed.sequence
                || stored.app_owner != committed.app_owner
            {
                return Err(unavailable());
            }
        }
        let ticket = self
            .frontier
            .source
            .progress
            .lock()
            .map_err(|_| unavailable())?
            .inhibitor
            .clone()
            .ok_or_else(unavailable)?;
        let hold = match mode {
            Release::Main => self
                .frontier
                .owner
                .selection_hold(self.frontier.go.as_deref())?,
            Release::Side => None,
        };
        let mut guard = Guard {
            frontier: self.frontier.clone(),
            completed: false,
        };
        // Pure Source scheduling operation. Optional exact initial/H1 Hold and
        // exact ticket retire under ONE real Gate writer, after the same check.
        let released = self
            .frontier
            .owner
            .owner
            .release_selection_if(&ticket, hold.as_deref(), || {
                self.frontier
                    .owner
                    .live()
                    .map_err(|error| error.to_string())?;
                let progress = self
                    .frontier
                    .source
                    .progress
                    .lock()
                    .map_err(|_| "Source custody poisoned".to_owned())?;
                if !matches!(progress.phase, Phase::Held | Phase::Entered)
                    || !progress
                        .inhibitor
                        .as_ref()
                        .is_some_and(|actual| Arc::ptr_eq(actual, &ticket))
                {
                    return Err("Source selection custody changed".into());
                }
                check()
            })
            .await;
        if released.is_err() {
            // The pure Owner guarantees no mutation on error. Local refusal
            // keeps both exact barriers and original Native channel retryable.
            guard.completed = true;
            return Err(unavailable());
        }
        if let Some(hold) = hold {
            self.frontier
                .owner
                .retire_selection_hold(self.frontier.go.as_deref(), &hold)?;
        }
        {
            let mut progress = self
                .frontier
                .source
                .progress
                .lock()
                .map_err(|_| unavailable())?;
            progress.inhibitor.take();
            progress.phase = Phase::Selected;
        }
        *self
            .frontier
            .actual
            .stage
            .lock()
            .map_err(|_| unavailable())? = Stage::Released;
        guard.completed = true;
        Ok(SelectionReleased {
            source: Arc::downgrade(&self.frontier.source),
            actual: Arc::downgrade(&self.frontier.actual),
            owner: Arc::downgrade(&self.frontier.owner),
            sequence: committed.observed.sequence,
        })
    }
}

impl HeldSourceTransition {
    /// Retire ONLY the actual Native selection operation after the SAME Source
    /// scheduling release was observed. This cleanup claims no Core shutdown,
    /// Native effects or reader settlement. The original accepted Source owner
    /// must retain this future/resource through observer loss.
    pub async fn finish_selection(&self, released: &SelectionReleased) -> io::Result<()> {
        if !released.same_transition(self) {
            return Err(unavailable());
        }
        let actual = released.actual.upgrade().ok_or_else(unavailable)?;
        if *actual.stage.lock().map_err(|_| unavailable())? != Stage::Released
            || actual
                .commit
                .lock()
                .map_err(|_| unavailable())?
                .as_ref()
                .is_none_or(|stored| stored.observed.sequence != released.sequence)
        {
            return Err(unavailable());
        }
        {
            let mut tasks = self.actual.owner.tasks.lock().map_err(|_| unavailable())?;
            self.actual.owner.live()?;
            let mut progress = self
                .actual
                .record
                .progress
                .lock()
                .map_err(|_| unavailable())?;
            if progress.phase == Phase::Complete {
                return Ok(());
            }
            if progress.phase != Phase::Completing {
                if progress.phase != Phase::Selected || progress.inhibitor.is_some() {
                    return Err(unavailable());
                }
                progress.phase = Phase::Completing;
                drop(progress);
                let owner = self.actual.owner.clone();
                let record = self.actual.record.clone();
                let guard = super::Guard {
                    owner: owner.clone(),
                    record: record.clone(),
                    completed: false,
                };
                tasks.push(tokio::spawn(async move {
                    let mut guard = guard;
                    let (service, selection) = match selectors(&record) {
                        Ok(value) => value,
                        Err(_) => return,
                    };
                    let op = operation();
                    let fields = format!(
                        "{} selection={selection}",
                        fields(&owner, &record, &service)
                    );
                    let body = match owner
                        .controller
                        .transport
                        .call(
                            "controller-source-selection-retire",
                            &op,
                            &fields,
                            Some(&owner.controller.member),
                        )
                        .await
                    {
                        Ok(body) => body,
                        Err(_) => return,
                    };
                    if decode(
                        &owner,
                        &record,
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
                    let Ok(mut progress) = record.progress.lock() else {
                        return;
                    };
                    if progress.phase != Phase::Completing || progress.inhibitor.is_some() {
                        return;
                    }
                    progress.phase = Phase::Complete;
                    drop(progress);
                    changed(&record);
                    guard.completed = true;
                    owner.forget_known_source(&record);
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
                Phase::Complete => return Ok(()),
                Phase::Completing => {}
                _ => return Err(unavailable()),
            }
            tokio::select! {biased;_=self.actual.owner.withdrawn()=>return Err(unavailable()),
            result=updates.changed()=>result.map_err(|_|unavailable())?,}
        }
    }
}

#[derive(Clone, Copy)]
enum Release {
    Main,
    Side,
}

impl std::fmt::Debug for SelectionFrontier {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("SelectionFrontier(<same actual resource; no Native grant>)")
    }
}

impl std::fmt::Debug for SelectionAck {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("SelectionAck(<same actual resource; no Native grant>)")
    }
}

impl std::fmt::Debug for SelectionObservation {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("SelectionObservation(<same actual resource; no Native grant>)")
    }
}

impl std::fmt::Debug for SelectionCommit {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("SelectionCommit(<same actual resource; no Native grant>)")
    }
}

impl std::fmt::Debug for SelectionReleased {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("SelectionReleased(<same actual resource; no Native grant>)")
    }
}
