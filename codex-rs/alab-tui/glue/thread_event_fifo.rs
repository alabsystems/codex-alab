// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Ordered overflow for one exact TUI channel. App owns its receiver, so App
//! cannot await a full sender. One worker replaces the old per-event sends;
//! pending memory remains unbounded. Order covers enqueue calls, not earlier
//! store effects or settings applied outside the transcript queue.

use super::App;
use super::ThreadBufferedEvent;
use codex_alab_tui::interface;
use codex_protocol::ThreadId;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::Weak;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::error::TrySendError;
use tokio::task::AbortHandle;

#[derive(Debug)]
struct Inner<T> {
    pending: VecDeque<T>,
    generation: u64,
    running: bool,
    retired: bool,
    abort: Option<AbortHandle>,
}

#[derive(Debug)]
pub(super) struct Queue<T> {
    inner: Arc<Mutex<Inner<T>>>,
}

impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                pending: VecDeque::new(),
                generation: 0,
                running: false,
                retired: false,
                abort: None,
            })),
        }
    }
}

fn lock<T>(inner: &Mutex<Inner<T>>) -> MutexGuard<'_, Inner<T>> {
    // No callbacks or awaits belong in this lock. A worker unwind must not
    // erase its unsent deque merely because the mutex was poisoned.
    inner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl<T> Drop for Queue<T> {
    fn drop(&mut self) {
        let (abort, pending) = {
            let mut state = lock(&self.inner);
            state.retired = true;
            state.running = false;
            (state.abort.take(), std::mem::take(&mut state.pending))
        };
        // Cancellation may drop the worker guard and re-lock; do it outside.
        if let Some(abort) = abort {
            abort.abort();
        }
        drop(pending);
    }
}

#[derive(Debug)]
struct Handle<T> {
    inner: Weak<Mutex<Inner<T>>>,
    sender: mpsc::Sender<T>,
}

impl<T: Send + 'static> Queue<T> {
    /// Transfers the channel's published prefix and overflow tail before its widget leaves.
    /// A reserved permit still owns no event: selection and publication share this lock.
    fn take_buffered(&self, receiver: &mut mpsc::Receiver<T>) -> VecDeque<T> {
        let (events, abort) = {
            let mut state = lock(&self.inner);
            // An old worker or its guard must not publish or retire a later worker.
            match state.generation.checked_add(1) {
                Some(generation) => state.generation = generation,
                None => state.retired = true,
            }
            state.running = false;
            let abort = state.abort.take();
            let mut events = VecDeque::new();
            while let Ok(event) = receiver.try_recv() {
                events.push_back(event);
            }
            events.append(&mut state.pending);
            (events, abort)
        };
        // Aborting can drop the worker guard and re-lock the queue.
        if let Some(abort) = abort {
            abort.abort();
        }
        events
    }

    fn handle(&self, sender: mpsc::Sender<T>) -> Handle<T> {
        Handle {
            inner: Arc::downgrade(&self.inner),
            sender,
        }
    }

    fn try_send(&self, sender: &mpsc::Sender<T>, event: T) -> Result<(), TrySendError<T>> {
        enqueue(Arc::clone(&self.inner), sender.clone(), event)
    }
}

impl<T: Send + 'static> Handle<T> {
    fn try_send(&self, event: T) -> Result<(), TrySendError<T>> {
        let Some(inner) = self.inner.upgrade() else {
            return Err(TrySendError::Closed(event));
        };
        enqueue(inner, self.sender.clone(), event)
    }
}

fn enqueue<T: Send + 'static>(
    inner: Arc<Mutex<Inner<T>>>,
    sender: mpsc::Sender<T>,
    event: T,
) -> Result<(), TrySendError<T>> {
    let (generation, runtime) = {
        let mut state = lock(&inner);
        if state.retired || sender.is_closed() {
            return Err(TrySendError::Closed(event));
        }
        let event = if !state.running && state.pending.is_empty() {
            match sender.try_send(event) {
                Ok(()) => return Ok(()),
                Err(TrySendError::Closed(event)) => return Err(TrySendError::Closed(event)),
                Err(TrySendError::Full(event)) => event,
            }
        } else {
            event
        };
        if state.running {
            state.pending.push_back(event);
            return Ok(());
        }
        // No new overflow is accepted without a worker-capable runtime. This
        // branch is unreachable in the running App, but does not panic outside it.
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return Err(TrySendError::Closed(event));
        };
        let Some(generation) = state.generation.checked_add(1) else {
            return Err(TrySendError::Closed(event));
        };
        state.pending.push_back(event);
        state.generation = generation;
        state.running = true; // Register before spawn; later calls cannot bypass an unpolled worker.
        (generation, runtime)
    };

    // Construct outside the lock: a rejected runtime can drop this future
    // before its first poll, and the guard must be able to re-lock safely.
    let guard = WorkerGuard {
        inner: Arc::clone(&inner),
        generation,
    };
    let task = runtime.spawn(drain(sender, guard));
    let mut abort = Some(task.abort_handle());
    let old_abort = {
        let mut state = lock(&inner);
        if !state.retired && state.running && state.generation == generation {
            std::mem::replace(&mut state.abort, abort.take())
        } else {
            None
        }
    };
    drop(old_abort);
    if let Some(abort) = abort {
        abort.abort();
    }
    // Dropping a JoinHandle detaches the one worker; the channel owns its abort handle.
    drop(task);
    Ok(())
}

struct WorkerGuard<T> {
    inner: Arc<Mutex<Inner<T>>>,
    generation: u64,
}

impl<T> Drop for WorkerGuard<T> {
    fn drop(&mut self) {
        let abort = {
            let mut state = lock(&self.inner);
            if state.generation == self.generation {
                state.running = false;
                state.abort.take()
            } else {
                None
            }
        };
        drop(abort);
    }
}

async fn drain<T: Send + 'static>(sender: mpsc::Sender<T>, guard: WorkerGuard<T>) {
    loop {
        let (finished, abort) = {
            let mut state = lock(&guard.inner);
            if state.retired || state.generation != guard.generation {
                (true, None)
            } else if state.pending.is_empty() {
                // The next caller may return to try_send only after this atomic idle transition.
                state.running = false;
                (true, state.abort.take())
            } else {
                (false, None)
            }
        };
        drop(abort);
        if finished {
            return;
        }

        // Retain the head while waiting: cancellation before a permit does not lose it.
        let permit = match sender.clone().reserve_owned().await {
            Ok(permit) => permit,
            Err(_) => {
                let (abort, pending) = {
                    let mut state = lock(&guard.inner);
                    if state.generation != guard.generation {
                        return;
                    }
                    state.retired = true;
                    state.running = false;
                    (state.abort.take(), std::mem::take(&mut state.pending))
                };
                drop(abort);
                drop(pending);
                tracing::warn!("thread event overflow receiver closed");
                return;
            }
        };
        let published_sender = {
            let mut state = lock(&guard.inner);
            if state.retired || state.generation != guard.generation {
                return;
            }
            // No await, callbacks, or serialization between selecting the head and sending it.
            state.pending.pop_front().map(|event| permit.send(event))
        };
        drop(published_sender);
    }
}

pub(super) struct Delivery<T> {
    sender: mpsc::Sender<T>,
    queue: Option<Handle<T>>,
}

impl<T: Send + 'static> Delivery<T> {
    pub(super) async fn send(&self, event: T) -> Result<(), SendError<T>> {
        if let Some(queue) = &self.queue {
            queue.try_send(event).map_err(|error| match error {
                TrySendError::Full(event) | TrySendError::Closed(event) => SendError(event),
            })
        } else {
            self.sender.send(event).await
        }
    }
}

impl App {
    /// Finishes the old widget's routed events before its transcript and receiver are saved.
    pub(super) async fn alab_finish_thread_events_before_switch(&mut self) {
        let Some(mut receiver) = self.active_thread_rx.take() else {
            return;
        };
        let queue = self
            .active_thread_id
            .filter(|_| interface::available())
            .and_then(|thread| self.thread_event_channels.get(&thread))
            // The active receiver has been taken above. A replacement channel whose own
            // receiver is still parked here cannot lend its FIFO to that older receiver.
            .filter(|channel| channel.receiver.is_none());
        if let Some(channel) = queue {
            let events = channel.alab_fifo.take_buffered(&mut receiver);
            for event in events {
                self.handle_thread_event_now_recovering_file_changes(event)
                    .await;
            }
        } else {
            while let Ok(event) = receiver.try_recv() {
                self.handle_thread_event_now_recovering_file_changes(event)
                    .await;
            }
        }
        self.active_thread_rx = Some(receiver);
    }

    fn alab_matching_thread_queue(
        &self,
        thread: ThreadId,
        sender: &mpsc::Sender<ThreadBufferedEvent>,
    ) -> Option<Handle<ThreadBufferedEvent>> {
        if !interface::available() {
            return None;
        }
        let channel = self.thread_event_channels.get(&thread)?;
        channel
            .sender
            .same_channel(sender)
            .then(|| channel.alab_fifo.handle(sender.clone()))
    }

    pub(super) fn alab_try_send_thread_event(
        &self,
        thread: ThreadId,
        sender: &mpsc::Sender<ThreadBufferedEvent>,
        event: ThreadBufferedEvent,
    ) -> Result<(), TrySendError<ThreadBufferedEvent>> {
        if interface::available()
            && let Some(channel) = self.thread_event_channels.get(&thread)
            && channel.sender.same_channel(sender)
        {
            return channel.alab_fifo.try_send(sender, event);
        }
        sender.try_send(event)
    }

    pub(super) fn alab_thread_event_sender(
        &self,
        thread: ThreadId,
        sender: mpsc::Sender<ThreadBufferedEvent>,
    ) -> Delivery<ThreadBufferedEvent> {
        Delivery {
            queue: self.alab_matching_thread_queue(thread, &sender),
            sender,
        }
    }
}

#[cfg(test)]
#[path = "thread_event_fifo_tests.rs"]
mod tests;
