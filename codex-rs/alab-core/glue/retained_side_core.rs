// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! SAME original Core injection occurrence, including an actually queued batch.
//! No Source/Native permission, Current registration, UI ACK or settlement is
//! supplied here. Source must own the original worker and real admission.
use super::CodexThread;
use crate::session::session::Session;
use codex_history::ResponseItemEnvelope;
pub use codex_history::RetainedSideMarker;
use codex_history::RolloutItem;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::Weak;
use tokio::sync::watch;

const MAX_BOUNDARIES: usize = 4096;
tokio::task_local! { static ORIGINAL: Arc<Record>; }

#[derive(Default)]
struct Ledger(Mutex<HashMap<String, Weak<Record>>>);
#[derive(Default)]
struct Progress {
    arguments: Option<Vec<ResponseItem>>,
    invoked: bool,
    submitted: bool,
    queued: Option<bool>,
    observed: Option<Result<(), String>>,
    recorded: Option<Result<(String, bool), String>>,
    delivery_attempted: bool,
    unknown: bool,
}
struct Record {
    session: Weak<Session>,
    marker: RetainedSideMarker,
    progress: Mutex<Progress>,
    changed: watch::Sender<()>,
}
/// Prepared before the original injection future is polled. Clones retain
/// the SAME occurrence owner and can never resubmit the accepted batch.
#[derive(Clone)]
pub struct PreparedSideBoundary(Arc<Record>);
/// A real canonical recording fact bound to the original session allocation.
/// This is not Core Current, file durability, Native admission or an ACK.
#[derive(Clone)]
pub struct RecordedSideBoundary {
    actual: Arc<Record>,
    turn: String,
    queued: bool,
}
impl CodexThread {
    pub fn prepare_retained_side_boundary(&self) -> CodexResult<PreparedSideBoundary> {
        let ledger = self
            .session
            .services
            .thread_extension_data
            .get_or_init(Ledger::default);
        let mut records = ledger.0.lock().unwrap_or_else(PoisonError::into_inner);
        records.retain(|_, record| record.strong_count() != 0);
        if records.len() >= MAX_BOUNDARIES {
            return Err(invalid("retained Side occurrence capacity unavailable"));
        }
        let token = uuid::Uuid::new_v4().simple().to_string();
        if records.contains_key(&token) {
            return Err(invalid(
                "retained Side occurrence collided with a live owner",
            ));
        }
        let marker = RetainedSideMarker::new_local(token.clone()).map_err(invalid)?;
        let record = Arc::new(Record {
            session: Arc::downgrade(&self.session),
            marker,
            progress: Mutex::new(Progress::default()),
            changed: watch::channel(()).0,
        });
        records.insert(token, Arc::downgrade(&record));
        Ok(PreparedSideBoundary(record))
    }
}
impl PreparedSideBoundary {
    /// The guard and immutable arguments exist BEFORE the returned future can
    /// be dropped. The original validation/context/queue/checkpoint path runs
    /// exactly once; an observed error is returned unchanged to its caller.
    pub fn inject<'a>(
        &'a self,
        core: &'a CodexThread,
        items: Vec<ResponseItem>,
    ) -> impl std::future::Future<Output = CodexResult<()>> + use<'a> {
        let admission = self.begin(core, &items);
        boxed(|| async move {
            let mut guard = admission?;
            let result = ORIGINAL
                .scope(self.0.clone(), core.inject_response_items(items))
                .await;
            {
                let mut progress = self
                    .0
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                progress.observed = Some(result.as_ref().map(|_| ()).map_err(ToString::to_string));
            }
            self.0.changed.send_replace(());
            guard.complete = true;
            result
        })
    }
    fn begin(&self, core: &CodexThread, items: &[ResponseItem]) -> CodexResult<Invocation> {
        self.0.check(core)?;
        // Original Side fork policy ends with its actual plain-text user
        // delimiter. This structural condition prevents image expansion from
        // moving a last-envelope cut into the middle of one prepared item.
        if !matches!(items.last(), Some(ResponseItem::Message { role, content, .. })
            if role == "user" && !content.is_empty() && content.iter().all(|item| matches!(item, ContentItem::InputText { .. })))
        {
            return Err(invalid(
                "Side occurrence needs its original final textual user item",
            ));
        }
        let mut progress = self
            .0
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(arguments) = &progress.arguments
            && arguments != items
        {
            return Err(invalid(
                "Side occurrence changed complete original injection arguments",
            ));
        }
        if progress.invoked || progress.unknown {
            return Err(invalid(
                "Side occurrence cannot resubmit accepted or unknown injection",
            ));
        }
        progress.arguments = Some(items.to_vec());
        progress.invoked = true;
        Ok(Invocation {
            record: self.0.clone(),
            complete: false,
        })
    }
    pub async fn recorded(&self, core: &CodexThread) -> CodexResult<RecordedSideBoundary> {
        let mut changed = self.0.changed.subscribe();
        loop {
            self.0.check(core)?;
            {
                let progress = self
                    .0
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if progress.unknown {
                    return Err(invalid("Side occurrence remains unknown"));
                }
                if let Some(Err(error)) = &progress.observed {
                    return Err(CodexErr::InvalidRequest(error.clone()));
                }
                if let Some(Err(error)) = &progress.recorded {
                    return Err(CodexErr::InvalidRequest(error.clone()));
                }
                if matches!(progress.observed, Some(Ok(())))
                    && progress.delivery_attempted
                    && let Some(Ok((turn, queued))) = &progress.recorded
                {
                    return Ok(RecordedSideBoundary {
                        actual: self.0.clone(),
                        turn: turn.clone(),
                        queued: *queued,
                    });
                }
            }
            changed
                .changed()
                .await
                .map_err(|_| invalid("Side occurrence observation unavailable"))?;
        }
    }
}
impl Record {
    fn check(&self, core: &CodexThread) -> CodexResult<()> {
        if !self
            .session
            .upgrade()
            .is_some_and(|actual| Arc::ptr_eq(&actual, &core.session))
        {
            return Err(invalid(
                "Side occurrence changed original actual Core session",
            ));
        }
        Ok(())
    }
}
impl RecordedSideBoundary {
    pub fn marker_data(&self) -> &RetainedSideMarker {
        &self.actual.marker
    }
    pub fn same_delivery(&self, receipt: &Arc<()>) -> bool {
        self.actual
            .marker
            .local_delivery_identity()
            .is_some_and(|actual| Arc::ptr_eq(&actual, receipt))
    }
    pub fn turn_data(&self) -> &str {
        &self.turn
    }
    /// Actual original active-turn queue branch, only for display reducer
    /// context. This is not an active/idle/currentness or Native assertion.
    pub fn was_queued_data(&self) -> bool {
        self.queued
    }
    pub fn check_owner(&self, core: &CodexThread) -> CodexResult<()> {
        self.actual.check(core)?;
        let progress = self
            .actual
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if progress.unknown
            || !progress.delivery_attempted
            || !matches!(&progress.recorded, Some(Ok((turn, queued))) if turn == &self.turn && queued == &self.queued)
        {
            return Err(invalid("recorded Side occurrence lost its actual owner"));
        }
        Ok(())
    }
    /// Actual writer-local cutoff, or the original canonical ephemeral journal.
    /// It does not load by a logical ID/path, force materialization, claim a
    /// paginated inherited prefix or return empty on failure. Actual Source
    /// listener/lifecycle and last manager Current remain caller obligations.
    pub async fn canonical_local_items(
        &self,
        core: &CodexThread,
        max_bytes: usize,
    ) -> CodexResult<Vec<RolloutItem>> {
        self.check_owner(core)?;
        if max_bytes == 0 {
            return Err(invalid(
                "Side canonical observation needs a response budget",
            ));
        }
        let items = if let Some(actual) = core.session.live_thread() {
            actual
                .retained_local_history(max_bytes)
                .await
                .map_err(|error| CodexErr::InvalidRequest(error.to_string()))?
                .items
        } else {
            let actual = core
                .exec_recovery_history()
                .await
                .map_err(|error| CodexErr::InvalidRequest(error.to_string()))?;
            if actual.thread_id != core.session.thread_id {
                return Err(invalid("canonical ephemeral journal changed original Core"));
            }
            check_items_budget(&actual.items, max_bytes)?;
            actual.items
        };
        self.check_owner(core)?;
        Ok(items)
    }
}
fn check_items_budget(items: &[RolloutItem], max_bytes: usize) -> CodexResult<()> {
    struct Budget(usize);
    impl std::io::Write for Budget {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.checked_sub(bytes.len()).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "canonical Side memory exceeds response budget",
                )
            })?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(&mut Budget(max_bytes), items)
        .map_err(|error| CodexErr::InvalidRequest(error.to_string()))
}
struct Invocation {
    record: Arc<Record>,
    complete: bool,
}
impl Drop for Invocation {
    fn drop(&mut self) {
        if !self.complete {
            self.record
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .unknown = true;
            self.record.changed.send_replace(());
        }
    }
}
/// Called only at the original injection's envelope construction, before the
/// active-turn enqueue. Ordinary injections have no scoped occurrence owner.
pub(crate) fn annotate(
    session: &Session,
    mut items: Vec<ResponseItemEnvelope>,
) -> Vec<ResponseItemEnvelope> {
    let Ok(record) = ORIGINAL.try_with(Arc::clone) else {
        return items;
    };
    let same_session = std::ptr::eq(record.session.as_ptr(), session);
    let mut progress = record
        .progress
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if !same_session || !progress.invoked || progress.submitted || items.is_empty() {
        progress.unknown = true;
    } else if let Some(last) = items.last_mut() {
        last.metadata.get_or_insert_default().alab_side_boundary = Some(record.marker.clone());
        progress.submitted = true;
    }
    drop(progress);
    record.changed.send_replace(());
    items
}
/// Original active-turn branch supplies this fact while it still owns that
/// exact active-turn lock. It changes neither routing nor turn admission.
pub(crate) fn routed(session: &Session, items: &[ResponseItemEnvelope], queued: bool) {
    let Some(ledger) = session.services.thread_extension_data.get::<Ledger>() else {
        return;
    };
    for item in items {
        let Some(marker) = item
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.alab_side_boundary.as_ref())
        else {
            continue;
        };
        let record = ledger
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(marker.token_data())
            .and_then(Weak::upgrade);
        let Some(record) = record else {
            continue;
        };
        if !std::ptr::eq(record.session.as_ptr(), session) || !record.marker.same_local(marker) {
            continue;
        }
        let mut progress = record
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !progress.submitted || progress.queued.replace(queued).is_some() {
            progress.unknown = true;
        }
        drop(progress);
        record.changed.send_replace(());
    }
}
/// Actual canonical recording/persistence completion. A serde marker, copied
/// parent marker or token match alone cannot reach this local receipt.
pub(crate) async fn recorded(
    session: &Session,
    items: &[RolloutItem],
    turn: &str,
    persisted: bool,
) {
    let Some(ledger) = session.services.thread_extension_data.get::<Ledger>() else {
        return;
    };
    for item in items {
        let RolloutItem::ResponseItem(envelope) = item else {
            continue;
        };
        let Some(marker) = envelope
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.alab_side_boundary.as_ref())
        else {
            continue;
        };
        let record = ledger
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(marker.token_data())
            .and_then(Weak::upgrade);
        let Some(record) = record else {
            continue;
        };
        if !std::ptr::eq(record.session.as_ptr(), session) || !record.marker.same_local(marker) {
            continue;
        }
        {
            let mut progress = record
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !progress.submitted || progress.queued.is_none() || progress.recorded.is_some() {
                progress.unknown = true;
            }
            // Preserve late actual outcome even when the original worker was lost.
            let queued = progress.queued.unwrap_or(false);
            progress.recorded = Some(if persisted {
                Ok((turn.to_owned(), queued))
            } else {
                Err("original Side boundary recording failed".into())
            });
            drop(progress);
        }
        // This is delivered through SAME original Core event queue, without
        // adding a rollout record or serializing this local receipt. Original
        // listener consumption orders its active/live cut before S2 capture.
        if persisted && let Some(identity) = record.marker.local_delivery_identity() {
            crate::session::alab_side_delivery::deliver(session, turn, identity).await;
        }
        record
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .delivery_attempted = true;
        record.changed.send_replace(());
    }
}
fn invalid(message: &'static str) -> CodexErr {
    CodexErr::InvalidRequest(message.into())
}

#[inline(never)]
fn boxed<F: std::future::Future>(create: impl FnOnce() -> F) -> std::pin::Pin<Box<F>> {
    Box::pin(create())
}
