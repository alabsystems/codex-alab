// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Recover the recorded outcome of a chooser's admitted original resume.
//! A receipt can retire the exact C1 release but cannot install a live view.
use super::*;
use crate::app::backend_sessions::BackendSessions;
use crate::app_server_session::alab_construction::MissingSavedResume;
use crate::app_server_session::alab_session_work::Book;
use crate::app_server_session::alab_session_work::saved_chat::HistoricalRelease;
use crate::app_server_session::alab_session_work::saved_chat::SavedChatRelease;
use crate::app_server_session::alab_session_work::saved_chat::SavedChatReleaseReceipt;
#[cfg(unix)]
use crate::owned_local_ui_transfer::SavedResumeHandoff;
use codex_app_server_client::SavedResumeObservation;
use codex_app_server_client::SavedResumeQuery;
use std::collections::hash_map::Entry;
use std::io;
use std::time::Duration;
use tokio::time::Instant as TokioInstant;

// The retained backend bounds admitted Later constructions at 4096.
// Handoff tickets have their own bound and never take a live chooser slot.
const MAX_RECORDS: usize = 4096;
const MAX_READS: usize = 2;
const FIRST_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(60);
const MIN_BATCH_INTERVAL: Duration = Duration::from_millis(250);

impl sidebar::SidebarPane {
    pub(in crate::app) fn alab_reconcile_saved_opening(&mut self) {
        if let Some(opening) = &self.opening
            && self
                .chat_tabs()
                .choice_index(opening.target.child)
                .is_none()
        {
            // The flight still owns its reply and any unsubscribe. Only the
            // chooser's permission to display that reply disappeared.
            self.opening = None;
        }
    }
}

#[derive(Default)]
pub(super) struct History {
    records: HashMap<Uuid, Record>,
    releases: HashMap<Uuid, HistoricalRelease>,
    // The event loop consults only owners with timed work. Rows are ranked when
    // that owner's work changes, never while redrawing an unrelated pane.
    wakes: HashMap<BackendId, OwnerWake>,
}

struct OwnerWake {
    reader: Book,
    opening: Uuid,
    due: Option<TokioInstant>,
    next_batch: TokioInstant,
}

struct Record {
    backend: BackendId,
    child: ThreadId,
    missing: MissingSavedResume,
    origin: Origin,
    attempt: Option<(Uuid, Book)>,
    schedule: ReadSchedule,
}

struct ReadSchedule {
    next: Option<ReadDeadline>,
    delay: Duration,
    // Move the confirmed reader here when its timed read starts or demand expedites it.
    pending_during_read: Option<Book>,
}

struct ReadDeadline {
    at: TokioInstant,
    // Only a positive Pending observation permits an autonomous re-read.
    reader: Option<Book>,
}

impl ReadDeadline {
    fn eligible_on(
        &self,
        server: &AppServerSession,
        timer: bool,
        next_batch: TokioInstant,
        now: TokioInstant,
    ) -> bool {
        self.at <= now
            && self.reader.as_ref().is_none_or(|reader| {
                timer && next_batch <= now && reader.belongs_to_connection(server)
            })
    }
}

enum ReadTrigger {
    Demand,
    Deadline,
    Offline,
}

impl ReadSchedule {
    fn ready(now: TokioInstant) -> Self {
        Self {
            next: Some(ReadDeadline {
                at: now,
                reader: None,
            }),
            delay: FIRST_RETRY,
            pending_during_read: None,
        }
    }

    fn rearm(&mut self, now: TokioInstant) {
        let at = self
            .take_deadline()
            .map_or(now, |deadline| deadline.min(now));
        self.next = Some(ReadDeadline { at, reader: None });
    }

    fn started(&mut self) {
        let _ = self.take_deadline();
    }

    fn take_deadline(&mut self) -> Option<TokioInstant> {
        let next = self.next.take()?;
        if let Some(reader) = next.reader {
            self.pending_during_read = Some(reader);
        }
        Some(next.at)
    }

    fn pending_reader(&self) -> Option<&Book> {
        self.next
            .as_ref()
            .and_then(|next| next.reader.as_ref())
            .or(self.pending_during_read.as_ref())
    }

    fn observed(&mut self, outcome: &Outcome, now: TokioInstant, server: &AppServerSession) {
        let retry = matches!(outcome, Outcome::Pending)
            || matches!(outcome, Outcome::ReadOverloaded)
                && self
                    .pending_reader()
                    .is_some_and(|reader| reader.belongs_to_connection(server));
        self.pending_during_read = None;
        self.next = if retry {
            let at = now + self.delay;
            self.delay = (self.delay * 2).min(MAX_RETRY);
            Some(ReadDeadline {
                at,
                reader: Some(server.alab_physical_work_owner()),
            })
        } else {
            self.delay = FIRST_RETRY;
            None
        };
    }
}

fn oldest_saved_reads(
    candidates: impl Iterator<Item = (TokioInstant, Uuid)>,
    now: TokioInstant,
    available: usize,
) -> Vec<Uuid> {
    let limit = available.min(MAX_READS);
    let mut oldest = Vec::with_capacity(limit + 1);
    if limit > 0 {
        for candidate in candidates.filter(|(due, _)| *due <= now) {
            let position = oldest.partition_point(|older| older < &candidate);
            if position < limit {
                oldest.insert(position, candidate);
                oldest.truncate(limit);
            }
        }
    }
    oldest.into_iter().map(|(_, opening)| opening).collect()
}

fn retired_release(
    origin: Origin,
    server: &AppServerSession,
    newer_flight: Option<&Flight>,
) -> Option<SavedChatRelease> {
    let Origin::Flight(Some(release)) = origin else {
        return None;
    };
    if !release.connection_replaced()
        || release.owns_connection(server)
        || newer_flight.is_some_and(|flight| {
            flight
                .incoming
                .as_ref()
                .is_none_or(|other| release.same_connection(&other.release))
        })
    {
        return None;
    }
    Some(release)
}

enum Origin {
    Flight(Option<SavedChatRelease>),
    #[cfg(unix)]
    Handoff,
}

#[derive(Debug)]
pub(crate) struct Observation {
    pub(crate) backend: BackendId,
    opening: Uuid,
    attempt: Uuid,
    outcome: Outcome,
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Resumed,
    Rejected(i64),
    Pending,
    Unavailable,
    ReadOverloaded,
    ReadFailed,
}

impl From<io::Result<SavedResumeObservation>> for Outcome {
    fn from(result: io::Result<SavedResumeObservation>) -> Self {
        match result {
            Ok(SavedResumeObservation::Resumed) => Self::Resumed,
            Ok(SavedResumeObservation::Rejected(error)) => Self::Rejected(error.code),
            Ok(SavedResumeObservation::Pending) => Self::Pending,
            Ok(SavedResumeObservation::Unavailable) => Self::Unavailable,
            Err(error) if SavedResumeQuery::is_read_overloaded(&error) => Self::ReadOverloaded,
            Err(_) => Self::ReadFailed,
        }
    }
}

pub(in crate::app) async fn wait_for_saved_resume(
    wake: Option<(TokioInstant, BackendId)>,
) -> BackendId {
    let Some((at, owner)) = wake else {
        return std::future::pending().await;
    };
    tokio::time::sleep_until(at).await;
    owner
}

impl Openings {
    pub(super) fn alab_saved_history_full(&self) -> bool {
        self.saved_history_slots() >= MAX_RECORDS
    }

    pub(in crate::app) fn alab_owns_saved_opening_work(&self) -> bool {
        self.saved_history_slots() != 0
    }

    fn saved_history_slots(&self) -> usize {
        self.active.len()
            + self.saved_history.releases.len()
            + self
                .saved_history
                .records
                .values()
                .filter(|r| matches!(r.origin, Origin::Flight(_)))
                .count()
    }

    pub(super) fn alab_remember_saved_resume(
        &mut self,
        target: Target,
        opening: Uuid,
        missing: Option<MissingSavedResume>,
        release: Option<SavedChatRelease>,
        server: &AppServerSession,
        offline: bool,
    ) -> Option<SavedChatRelease> {
        let Some(missing) = missing else {
            return release;
        };
        if missing.query().opening() != opening.simple().to_string()
            || missing
                .query()
                .validate_child(&target.child.to_string())
                .is_err()
        {
            tracing::warn!(%opening, child = %target.child, "saved resume history did not match its original chooser");
            missing.finish_local_history();
            return None;
        }
        if self.saved_history.releases.contains_key(&opening) {
            tracing::warn!(%opening, "saved resume already has its historical release");
            missing.finish_local_history();
            return None;
        }
        let Entry::Vacant(entry) = self.saved_history.records.entry(opening) else {
            tracing::warn!(%opening, "saved resume history already has this chooser operation");
            return None;
        };
        entry.insert(Record {
            backend: target.backend_id,
            child: target.child,
            missing,
            origin: Origin::Flight(release),
            attempt: None,
            schedule: ReadSchedule::ready(TokioInstant::now()),
        });
        if !offline {
            self.pump_saved_reads(server);
        }
        None
    }

    #[cfg(unix)]
    pub(crate) fn alab_restore_saved_resume_queries(
        &mut self,
        server: &AppServerSession,
        handoffs: Vec<SavedResumeHandoff>,
    ) -> io::Result<()> {
        let restored = self
            .saved_history
            .records
            .values()
            .filter(|r| matches!(r.origin, Origin::Handoff))
            .count();
        if handoffs.len() > MAX_RECORDS.saturating_sub(restored) {
            return Err(io::Error::other(
                "saved resume handoff exceeds backend admission capacity",
            ));
        }
        let mut records = HashMap::with_capacity(handoffs.len());
        for handoff in handoffs {
            let (opening, child) = handoff.checkpoint().saved_opening().ok_or_else(|| {
                io::Error::other("saved resume handoff lost its original chooser identity")
            })?;
            let opening = Uuid::parse_str(opening).map_err(io::Error::other)?;
            let child = ThreadId::from_string(child).map_err(io::Error::other)?;
            handoff.query().validate_child(&child.to_string())?;
            if self.saved_history.records.contains_key(&opening)
                || self.saved_history.releases.contains_key(&opening)
                || records.contains_key(&opening)
            {
                return Err(io::Error::other(
                    "duplicate saved resume opening in handoff",
                ));
            }
            records.insert(
                opening,
                Record {
                    backend: server.backend_id(),
                    child,
                    missing: MissingSavedResume::restored(server, handoff)?,
                    origin: Origin::Handoff,
                    attempt: None,
                    schedule: ReadSchedule::ready(TokioInstant::now()),
                },
            );
        }
        self.saved_history.records.extend(records);
        self.pump_saved_reads(server);
        Ok(())
    }

    pub(super) fn alab_retire_saved_history(&mut self, outgoing: &AppServerSession) {
        let now = TokioInstant::now();
        if self
            .saved_history
            .wakes
            .get(&outgoing.backend_id())
            .is_some_and(|wake| wake.reader.belongs_to_connection(outgoing))
        {
            self.saved_history.wakes.remove(&outgoing.backend_id());
        }
        for record in self.saved_history.records.values_mut() {
            if record.backend != outgoing.backend_id() || !record.missing.belongs_to(outgoing) {
                continue;
            }
            if let Origin::Flight(Some(release)) = &mut record.origin {
                release.retire_if_owned_by(outgoing);
            }
            // Any C1 observation can still arrive on the persistent completion queue.
            if record
                .attempt
                .as_ref()
                .is_some_and(|(_, reader)| reader.belongs_to_connection(outgoing))
                || record
                    .schedule
                    .pending_reader()
                    .is_some_and(|reader| reader.belongs_to_connection(outgoing))
            {
                record.attempt = None;
                record.schedule = ReadSchedule::ready(now);
            }
        }
    }

    pub(in crate::app) fn alab_retry_saved_history(
        &mut self,
        server: &AppServerSession,
        child: Option<ThreadId>,
    ) {
        let now = TokioInstant::now();
        for record in self.saved_history.records.values_mut() {
            if record.backend == server.backend_id()
                && record.missing.belongs_to(server)
                && child.is_none_or(|child| child == record.child)
                && record
                    .attempt
                    .as_ref()
                    .is_none_or(|(_, reader)| !reader.belongs_to_connection(server))
            {
                if record.attempt.is_some()
                    || record
                        .schedule
                        .pending_reader()
                        .is_some_and(|reader| !reader.belongs_to_connection(server))
                {
                    record.schedule = ReadSchedule::ready(now);
                } else {
                    record.schedule.rearm(now);
                }
                record.attempt = None;
            }
        }
        self.pump_saved_reads(server);
    }

    pub(super) fn alab_history_requires_observation(&self) -> bool {
        !self.saved_history.records.is_empty() || !self.saved_history.releases.is_empty()
    }

    pub(super) fn alab_retry_saved_history_for_upgrade(&mut self, backends: &BackendSessions) {
        let owners: std::collections::HashSet<_> = self
            .saved_history
            .records
            .values()
            .map(|record| record.backend)
            .collect();
        for owner in owners {
            if let Some(server) = backends.session(owner) {
                self.alab_retry_saved_history(server, None);
            }
        }
    }

    pub(in crate::app) fn alab_detach_saved_history_observer(&mut self, observation: &Observation) {
        if let Some(record) = self.saved_history.records.get_mut(&observation.opening)
            && record.backend == observation.backend
            && record
                .attempt
                .as_ref()
                .is_some_and(|(issued, _)| *issued == observation.attempt)
        {
            record.attempt = None;
            record.schedule = ReadSchedule::ready(TokioInstant::now());
        }
    }

    fn publish_historical_release(&mut self, release: SavedChatRelease) -> bool {
        let Entry::Vacant(entry) = self.saved_history.releases.entry(release.operation()) else {
            return false;
        };
        entry.insert(release.publish_historical(self.tx.clone()));
        true
    }

    pub(in crate::app) async fn alab_observe_historical_release(
        &mut self,
        target: Target,
        request: Uuid,
        receipt: &SavedChatReleaseReceipt,
    ) -> bool {
        let Entry::Occupied(mut entry) = self.saved_history.releases.entry(request) else {
            return false;
        };
        if !entry
            .get_mut()
            .observe_and_join(target, request, receipt)
            .await
        {
            return false;
        }
        entry.remove();
        true
    }

    fn current_saved_wake(
        &self,
        backends: &BackendSessions,
        offline: Option<BackendId>,
        owner: BackendId,
        wake: &OwnerWake,
    ) -> Option<TokioInstant> {
        let due = wake.due?;
        if offline == Some(owner) {
            return None;
        }
        let server = backends.session_at(backends.listening_slot(owner)?);
        let record = self.saved_history.records.get(&wake.opening)?;
        (wake.reader.belongs_to_connection(server) && record.missing.belongs_to(server))
            .then_some(due)
    }

    pub(in crate::app) fn alab_next_saved_wake(
        &self,
        backends: &BackendSessions,
        offline: Option<BackendId>,
    ) -> Option<(TokioInstant, BackendId)> {
        self.saved_history
            .wakes
            .iter()
            .filter_map(|(&owner, wake)| {
                self.current_saved_wake(backends, offline, owner, wake)
                    .map(|due| (due, owner))
            })
            .min()
    }

    pub(in crate::app) fn alab_wake_saved_history(
        &mut self,
        backends: &BackendSessions,
        offline: Option<BackendId>,
        owner: BackendId,
    ) {
        if let Some(wake) = self.saved_history.wakes.get(&owner)
            && self
                .current_saved_wake(backends, offline, owner, wake)
                .is_some_and(|due| due <= TokioInstant::now())
            && let Some(server) = backends.session(owner)
        {
            self.dispatch_saved_reads(server, ReadTrigger::Deadline);
        }
    }

    fn pump_saved_reads(&mut self, server: &AppServerSession) {
        self.dispatch_saved_reads(server, ReadTrigger::Demand);
    }

    fn dispatch_saved_reads(&mut self, server: &AppServerSession, trigger: ReadTrigger) {
        let now = TokioInstant::now();
        let backend = server.backend_id();
        let mut next_batch = self
            .saved_history
            .wakes
            .get(&backend)
            .filter(|wake| wake.reader.belongs_to_connection(server))
            .map_or(now, |wake| wake.next_batch);
        let mut active = self
            .saved_history
            .records
            .values()
            .filter(|r| {
                r.attempt
                    .as_ref()
                    .is_some_and(|(_, reader)| reader.belongs_to_connection(server))
            })
            .count();
        let candidates = self
            .saved_history
            .records
            .iter()
            .filter_map(|(&opening, record)| {
                let next = record.schedule.next.as_ref()?;
                (record.backend == backend
                    && record.attempt.is_none()
                    && next.eligible_on(
                        server,
                        matches!(trigger, ReadTrigger::Deadline),
                        next_batch,
                        now,
                    )
                    && record.missing.belongs_to(server))
                .then_some((next.at, opening))
            });
        let available = if matches!(trigger, ReadTrigger::Offline) {
            0
        } else {
            MAX_READS.saturating_sub(active)
        };
        for opening in oldest_saved_reads(candidates, now, available) {
            let Some(record) = self.saved_history.records.get_mut(&opening) else {
                continue;
            };
            if record
                .schedule
                .next
                .as_ref()
                .is_some_and(|next| next.reader.is_some())
            {
                next_batch = now + MIN_BATCH_INTERVAL;
            }
            let attempt = Uuid::new_v4();
            record.attempt = Some((attempt, server.alab_physical_work_owner()));
            record.schedule.started();
            active += 1;
            let handle = server.request_handle();
            let child = record.child.to_string();
            let query = Arc::clone(record.missing.query());
            let tx = self.tx.clone();
            server.alab_spawn_original_work(async move {
                let outcome = Outcome::from(query.read(&handle, &child).await);
                tx.send(AppEvent::AlabSavedResumeObserved(Observation {
                    backend,
                    opening,
                    attempt,
                    outcome,
                }));
            });
        }
        // A saturated owner has no timer until its matching completion returns
        // a slot. Other owners' sessions and saved openings remain independent.
        let earliest = self
            .saved_history
            .records
            .iter()
            .filter_map(|(&opening, record)| {
                let next = record.schedule.next.as_ref()?;
                (record.backend == backend
                    && next.reader.as_ref()?.belongs_to_connection(server)
                    && record.missing.belongs_to(server))
                .then_some((next.at, opening))
            })
            .min();
        if let Some((at, opening)) = earliest {
            self.saved_history.wakes.insert(
                backend,
                OwnerWake {
                    reader: server.alab_physical_work_owner(),
                    opening,
                    due: (active < MAX_READS).then_some(at.max(next_batch)),
                    next_batch,
                },
            );
        } else {
            self.saved_history.wakes.remove(&backend);
        }
    }
}

impl App {
    /// Called after one counted slot was released. Admission spans every backend,
    /// and the queued request will recheck the displayed chooser before opening.
    fn wake_after_saved_history_slot(&self) {
        if self.waiting_saved_chat().is_some()
            && self.saved_openings.saved_history_slots() == MAX_RECORDS - 1
        {
            self.saved_openings.tx.send(AppEvent::StartQueuedSavedChat);
        }
    }

    pub(in crate::app) async fn alab_finish_historical_release(
        &mut self,
        target: Target,
        request: Uuid,
        receipt: &SavedChatReleaseReceipt,
    ) {
        if self
            .saved_openings
            .alab_observe_historical_release(target, request, receipt)
            .await
        {
            self.wake_after_saved_history_slot();
        }
    }

    pub(in crate::app) fn alab_finish_saved_history(
        &mut self,
        server: &AppServerSession,
        observed: Observation,
    ) {
        let Observation {
            backend,
            opening,
            attempt,
            outcome,
        } = observed;
        if backend != server.backend_id() {
            return;
        }
        let history = &mut self.saved_openings;
        let Some(record) = history.saved_history.records.get_mut(&opening) else {
            return;
        };
        let Some((issued, reader)) = &record.attempt else {
            return;
        };
        if *issued != attempt {
            return;
        }
        if !reader.belongs_to_connection(server)
            || record.backend != server.backend_id()
            || !record.missing.belongs_to(server)
        {
            record.attempt = None;
            record.schedule = ReadSchedule::ready(TokioInstant::now());
            return;
        }
        record
            .schedule
            .observed(&outcome, TokioInstant::now(), server);
        record.attempt = None;
        let child = record.child;
        let (disposition, server_code) = match outcome {
            Outcome::Resumed => ("resumed", None),
            Outcome::Rejected(code) => ("rejected", Some(code)),
            Outcome::Pending => ("pending", None),
            Outcome::Unavailable => ("unavailable", None),
            Outcome::ReadOverloaded => ("readOverloaded", None),
            Outcome::ReadFailed => ("readFailed", None),
        };
        tracing::info!(%opening, %child, disposition, server_code, "original saved resume observation");

        self.settle_saved_history_record(server, opening, &outcome);
        if self.app_server_backend_id != Some(backend) || !self.reconnect.offline {
            self.saved_openings.pump_saved_reads(server);
        } else {
            // An offline displayed owner may become parked on this same Book.
            // Keep its index current without issuing a read while it is offline.
            self.saved_openings
                .dispatch_saved_reads(server, ReadTrigger::Offline);
        }
    }

    fn settle_saved_history_record(
        &mut self,
        server: &AppServerSession,
        opening: Uuid,
        outcome: &Outcome,
    ) {
        if matches!(
            outcome,
            Outcome::Pending | Outcome::ReadOverloaded | Outcome::ReadFailed
        ) {
            return;
        }
        let history = &mut self.saved_openings;
        let Some(record) = history.saved_history.records.remove(&opening) else {
            return;
        };
        record.missing.finish_local_history();
        let counted_slot = matches!(record.origin, Origin::Flight(_));
        if matches!(outcome, Outcome::Resumed)
            && let Some(release) = retired_release(
                record.origin,
                server,
                history.active.get(&(record.backend, record.child)),
            )
            && history.publish_historical_release(release)
        {
            // The reserved unsubscribe now owns this slot until its actual worker joins.
            return;
        }
        if counted_slot {
            self.wake_after_saved_history_slot();
        }
    }
}

#[cfg(test)]
#[path = "saved_resume_history_tests.rs"]
mod tests;
