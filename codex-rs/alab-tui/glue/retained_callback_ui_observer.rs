// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual owned Source long-poll and private UI event. A local future being
//! dropped is not Source cancellation. Obsolete frontiers and physical closure
//! produce real terminal replies through the Source observer/wake contract.
use super::*;
use crate::app_event_sender::AppEventSender;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub(super) struct Observer {
    owner: std::sync::Weak<CallbackUi>,
    ticket: std::sync::Weak<Ticket>,
    pending: std::sync::Weak<Pending>,
    receipt: kind::Receipt,
    latest: Mutex<Option<CallbackObservation>>,
    applied: std::sync::atomic::AtomicU64,
    terminal: tokio::sync::watch::Sender<Option<Result<(), io::ErrorKind>>>,
    terminal_observed: std::sync::atomic::AtomicBool,
    revalidated: std::sync::atomic::AtomicBool,
    task: Mutex<Option<JoinHandle<()>>>,
    joined: Mutex<Option<Result<(), io::ErrorKind>>>,
    join: Semaphore,
    stop: CancellationToken,
}
/// Can only be constructed by the actual observer below; never deserialized.
/// The event owns the same result allocation already retained before enqueue.
pub(crate) struct Update(Arc<Observer>);
impl std::fmt::Debug for Update {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("CallbackUpdate(<actual Source observer; no grant>)")
    }
}
struct Guard {
    actual: Arc<Observer>,
    complete: bool,
}
impl Drop for Guard {
    fn drop(&mut self) {
        if !self.complete {
            self.actual
                .terminal
                .send_replace(Some(Err(io::ErrorKind::Other)));
        }
    }
}
impl Observer {
    fn start(
        owner: &Arc<CallbackUi>,
        ticket: &Arc<Ticket>,
        pending: &Arc<Pending>,
        receipt: kind::Receipt,
        initial: CallbackObservation,
        events: AppEventSender,
        handle: AppServerRequestHandle,
        revalidate: bool,
    ) -> io::Result<Arc<Self>> {
        let actual = Arc::new(Self {
            owner: Arc::downgrade(owner),
            ticket: Arc::downgrade(ticket),
            pending: Arc::downgrade(pending),
            receipt,
            latest: Mutex::new(Some(initial.clone())),
            applied: std::sync::atomic::AtomicU64::new(initial.sequence_data()),
            terminal: tokio::sync::watch::channel(None).0,
            terminal_observed: std::sync::atomic::AtomicBool::new(false),
            revalidated: std::sync::atomic::AtomicBool::new(!revalidate),
            task: Mutex::new(None),
            joined: Mutex::new(None),
            join: Semaphore::new(1),
            stop: owner.stop.child_token(),
        });
        // Guard and exact owner are stored before spawn/first poll. Only the
        // Source's real response ends a query already sent on the shared client.
        let guard = Guard {
            actual: actual.clone(),
            complete: false,
        };
        let worker = actual.clone();
        let capture = ticket
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .capture
            .clone()
            .ok_or_else(|| invalid("observer lacks actual accepted capture"))?;
        // Publish SAME allocation before any owned task can poll/return.
        ticket
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .observer = Some(actual.clone());
        owner
            .observers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(actual.clone());
        let task = tokio::spawn(async move {
            let mut guard = guard;
            let mut after = (!revalidate).then_some(initial);
            // Correlated refusals may be observed and retried as NEW readonly
            // queries. Do not spin through Source/Native while it is Busy.
            if revalidate {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            let outcome = loop {
                if worker.stop.is_cancelled() || events.app_event_tx.is_closed() {
                    worker.terminal_observed.store(true, Ordering::Release);
                    break Ok(());
                }
                let response = match &worker.receipt {
                    kind::Receipt::Runtime(receipt) => {
                        capture
                            .observe_runtime(&handle, read_request(), receipt, after.as_ref())
                            .await
                    }
                    kind::Receipt::Source(receipt) => {
                        capture
                            .observe_source(&handle, read_request(), receipt, after.as_ref())
                            .await
                    }
                };
                match response {
                    Ok(actual) => {
                        let retired = actual.ticket_retired();
                        *worker.latest.lock().unwrap_or_else(PoisonError::into_inner) =
                            Some(actual.clone());
                        worker.revalidated.store(true, Ordering::Release);
                        // Result retained BEFORE enqueue. An unrelated event,
                        // filtered receiver or lost observer cannot erase it.
                        events.send(crate::app_event::AppEvent::AlabRetainedCallback(Update(
                            worker.clone(),
                        )));
                        after = Some(actual);
                        if retired {
                            worker.terminal_observed.store(true, Ordering::Release);
                            break Ok(());
                        }
                    }
                    Err(error) => {
                        // Only a correlated Source terminal response permits a
                        // same-connection observer replacement. IO/decode loss
                        // remains uncertain until actual physical closure.
                        worker.terminal_observed.store(
                            matches!(
                                &error,
                                codex_app_server_client::TypedRequestError::Server { .. }
                            ),
                            Ordering::Release,
                        );
                        tracing::debug!(%error,"callback Source observer ended; exact ticket remains owned");
                        break Err(io::ErrorKind::Other);
                    }
                }
            };
            worker.terminal.send_replace(Some(outcome));
            guard.complete = true;
            // Surface a terminal query even when no newer facts were returned.
            // The actual App validates owner/tenure/projection before deciding
            // whether a NEW readonly query can revalidate that same resource.
            events.send(crate::app_event::AppEvent::AlabRetainedCallback(Update(
                worker.clone(),
            )));
        });
        *actual.task.lock().unwrap_or_else(PoisonError::into_inner) = Some(task);
        Ok(actual)
    }
    fn is_for(&self, pending: &Arc<Pending>) -> bool {
        self.pending
            .upgrade()
            .is_some_and(|actual| Arc::ptr_eq(&actual, pending))
    }
    fn is_reaped(&self) -> bool {
        self.terminal_observed.load(Ordering::Acquire)
            && matches!(
                *self.joined.lock().unwrap_or_else(PoisonError::into_inner),
                Some(Ok(()))
            )
    }
    async fn join(&self) -> io::Result<()> {
        let _permit = self.join.acquire().await.map_err(invalid)?;
        if let Some(result) = *self.joined.lock().unwrap_or_else(PoisonError::into_inner) {
            return result
                .map_err(|kind| io::Error::new(kind, "callback observer join unobserved"));
        }
        let task = self
            .task
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let Some(task) = task else {
            return Ok(());
        };
        let mut lease = JoinLease {
            actual: self,
            task: Some(task),
        };
        let result = lease
            .task
            .as_mut()
            .ok_or_else(|| invalid("observer handle missing"))?
            .await
            .map_err(|_| io::ErrorKind::Other);
        lease.task.take();
        *self.joined.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
        result.map_err(|kind| io::Error::new(kind, "actual callback observer task failed"))
    }
}
struct JoinLease<'a> {
    actual: &'a Observer,
    task: Option<JoinHandle<()>>,
}
impl Drop for JoinLease<'_> {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            *self
                .actual
                .task
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(task);
        }
    }
}

pub(super) fn ensure(
    owner: &Arc<CallbackUi>,
    ticket: &Arc<Ticket>,
    pending: &Arc<Pending>,
    receipt: &kind::Receipt,
    cut: &Cut,
    events: AppEventSender,
    handle: AppServerRequestHandle,
) -> io::Result<()> {
    let Cut::Projected(initial) = cut else {
        return Ok(());
    };
    let mut state = ticket
        .progress
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let mut revalidate = false;
    if let Some(old) = state.observer.clone() {
        if old.is_for(pending) && old.terminal.borrow().is_none() {
            return if old.revalidated.load(Ordering::Acquire) {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "fresh actual Source callback cut is still pending",
                ))
            };
        }
        // The producer's genuine frontier-change wake must end the old actual
        // Source query. Keep old handle/result instead of cancelling/resending.
        if old.terminal.borrow().is_none() || !old.terminal_observed.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "old actual Source observer has not observably ended",
            ));
        }
        state.obsolete_observers.push(old.clone());
        revalidate = true;
    }
    // Release progress before capture/observer creation; no lock inversion.
    drop(state);
    let actual = Observer::start(
        owner,
        ticket,
        pending,
        receipt.clone(),
        initial.clone(),
        events,
        handle,
        revalidate,
    )?;
    let mut state = ticket
        .progress
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    // start already published this exact allocation before spawn.
    if !state
        .observer
        .as_ref()
        .is_some_and(|stored| Arc::ptr_eq(stored, &actual))
    {
        actual.stop.cancel();
        state.obsolete_observers.push(actual);
        return Err(invalid("actual observer publication changed"));
    }
    if revalidate {
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "replacement observer requires a fresh actual Source current-resource cut",
        ))
    } else {
        Ok(())
    }
}

/// Remove only completed, joined tasks whose actual Source query termination
/// was observed. Unknown query/JoinError custody remains owned through shutdown.
pub(super) async fn reap(owner: &Arc<CallbackUi>) -> io::Result<()> {
    let jobs = owner
        .observers
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    for job in jobs {
        let finished = job
            .task
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(JoinHandle::is_finished);
        if finished && job.terminal_observed.load(Ordering::Acquire) {
            job.join().await?;
        }
    }
    let removed = {
        let mut jobs = owner
            .observers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut removed = Vec::new();
        let mut index = 0;
        while index < jobs.len() {
            if jobs[index].is_reaped() {
                removed.push(jobs.remove(index));
            } else {
                index += 1;
            }
        }
        removed
    };
    let tickets = owner
        .tickets
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let mut retired = Vec::new();
    for ticket in tickets {
        let mut state = ticket
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut index = 0;
        while index < state.obsolete_observers.len() {
            if state.obsolete_observers[index].is_reaped() {
                retired.push(state.obsolete_observers.remove(index));
            } else {
                index += 1;
            }
        }
    }
    drop(removed);
    drop(retired); // Native/client/receipt contexts drop outside maps.
    Ok(())
}
impl App {
    pub(in crate::app) async fn alab_callback_observed(
        &mut self,
        session: &AppServerSession,
        update: Update,
    ) -> io::Result<()> {
        let Some(owner) = ui(self, session)? else {
            return Ok(());
        };
        let Some(actual_owner) = update.0.owner.upgrade() else {
            return Ok(());
        };
        if !Arc::ptr_eq(&owner, &actual_owner) {
            return Err(invalid("callback observer changed actual UI owner"));
        }
        let Some(ticket) = update.0.ticket.upgrade() else {
            return Ok(());
        };
        if !owner
            .tickets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|actual| Arc::ptr_eq(actual, &ticket))
        {
            return Ok(()); // Already genuinely retired canonical ticket.
        }
        if !ticket
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .observer
            .as_ref()
            .is_some_and(|actual| Arc::ptr_eq(actual, &update.0))
        {
            return Err(invalid("callback event is not the owned current observer"));
        }
        let Some(pending) = update.0.pending.upgrade() else {
            return Err(invalid("callback observer lost original projection owner"));
        };
        if !pending.current_on(session) {
            return Ok(());
        } // Keep exact result for a fresh real projection.
        let Some(current) = pending
            .callback_receipt(session)
            .map_err(io::Error::other)?
        else {
            return Ok(());
        };
        if std::mem::discriminant(&current) != std::mem::discriminant(&update.0.receipt)
            || current.frontier_data() != update.0.receipt.frontier_data()
            || current.snapshot_data() != update.0.receipt.snapshot_data()
            || current.thread_id_data() != update.0.receipt.thread_id_data()
        {
            return Err(invalid("callback observer changed exact actual projection"));
        }
        if let Some(Err(_)) = *update.0.terminal.borrow() {
            let cut = Cut::Projected(
                update
                    .0
                    .latest
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone()
                    .ok_or_else(|| invalid("terminal observer lost actual Source cut"))?,
            );
            // ensure distinguishes observed terminal refusal from uncertain
            // transport loss. A retry must receive a NEW actual Source cut.
            return ensure(
                &owner,
                &ticket,
                &pending,
                &current,
                &cut,
                self.app_event_tx.clone(),
                session.request_handle(),
            );
        }
        let observation = update
            .0
            .latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| invalid("callback event lacks actual Source cut"))?;
        if observation.sequence_data() <= update.0.applied.load(Ordering::Acquire) {
            return Ok(());
        }
        let cut = Cut::Projected(observation.clone());
        let closed = validate_cut(&ticket.thread, &ticket.ids, &cut)?;
        for id in &closed {
            self.alab_apply_closed_callback(&ticket.thread, id);
        }
        for transfer in owner.all() {
            let mut applications = transfer
                .applications
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(applied) = applications.get_mut(&ticket.key)
                && Arc::ptr_eq(&applied.pending, &pending)
            {
                applied.observation = cut.clone();
                applied.closed = closed.clone();
            }
        }
        update
            .0
            .applied
            .store(observation.sequence_data(), Ordering::Release);
        self.alab_ack_applied_callbacks(session).await
    }
    /// Call AFTER actual physical-client shutdown, which triggers Source's real
    /// readonly-close token. No accepted Source work or ticket is cancelled.
    pub(in crate::app) async fn alab_join_callback_observers(&mut self) -> io::Result<()> {
        let Some(owner) = self
            .pending_app_server_requests
            .alab_retained_replies
            .owner
            .as_ref()
            .map(|owner| owner.callbacks.clone())
        else {
            return Ok(());
        };
        owner.stop.cancel();
        let jobs = owner
            .observers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let mut first = None;
        for job in jobs {
            if let Err(error) = job.join().await
                && first.is_none()
            {
                first = Some(error);
            }
        }
        match first {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}
