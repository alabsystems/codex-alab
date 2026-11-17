// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual original Session workers and descendant joins. No DATA/native grant
//! can reconstruct this task ledger or authorize the RPCs inside an original future.
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::other(message.to_string())
}
const CAPTURE_MAX: usize = 16384;
tokio::task_local! {static PARENT:Arc<Parent>;}
struct Parent {
    book: Arc<State>,
    active: AtomicBool,
}
impl Drop for Parent {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
    }
}
struct Progress {
    jobs: Vec<Arc<Job>>,
    generation: u64,
    closed: bool,
    uncertain: bool,
}
struct State {
    progress: Mutex<Progress>,
    changes: tokio::sync::watch::Sender<()>,
}
#[derive(Clone)]
pub(crate) struct Book(Arc<State>);
impl Book {
    pub(crate) fn new() -> Self {
        let (changes, _) = tokio::sync::watch::channel(());
        Self(Arc::new(State {
            progress: Mutex::new(Progress {
                jobs: Vec::new(),
                generation: 0,
                closed: false,
                uncertain: false,
            }),
            changes,
        }))
    }
    pub(crate) fn changes(&self) -> tokio::sync::watch::Receiver<()> {
        self.0.changes.subscribe()
    }
    fn bump(state: &mut Progress) {
        match state.generation.checked_add(1) {
            Some(next) => state.generation = next,
            None => state.uncertain = true,
        }
    }
    fn start<F: Future<Output = ()> + Send + 'static>(
        &self,
        original: F,
        parent: Option<&Arc<Parent>>,
        native_original: bool,
    ) -> std::result::Result<(), Refused<F>> {
        let runtime = match tokio::runtime::Handle::try_current() {
            Ok(runtime) => runtime,
            Err(error) => {
                return Err(Refused {
                    original,
                    error: invalid(error),
                });
            }
        };
        let mut retired = Vec::new();
        let mut state = self
            .0
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // Finished original policy has already handed its results/resources to
        // its native owner. Retire only ACTUALLY joined successful unit tasks.
        state.jobs.retain(|job| {
            if job.try_retire() {
                retired.push(job.clone());
                false
            } else {
                true
            }
        });
        if !retired.is_empty() {
            Self::bump(&mut state);
        }
        let descendant = parent.is_some_and(|actual| {
            Arc::ptr_eq(&actual.book, &self.0) && actual.active.load(Ordering::Acquire)
        });
        if state.closed && !descendant && !native_original {
            drop(state);
            drop(retired);
            return Err(Refused {
                original,
                error: invalid("actual Session worker admission is closing"),
            });
        }
        let job = Job::prepare(Arc::downgrade(&self.0));
        state.jobs.push(job.clone());
        Self::bump(&mut state);
        // SAME close/start gate covers acceptance, actual spawn +handle store.
        job.start(runtime, self.0.clone(), original);
        drop(state);
        drop(retired);
        self.0.changes.send_replace(());
        Ok(())
    }
    pub(crate) fn spawn_original<F: Future<Output = ()> + Send + 'static>(
        &self,
        original: F,
    ) -> std::result::Result<(), Refused<F>> {
        self.start(original, None, false)
    }
    /// This is the actual original Session caller's existing scheduling path,
    /// including cleanup after a read completes. Closing a local capture cut
    /// does not synthesize an ordinary failed model/title result. Every such
    /// native task remains owned and is included in actual shutdown joins.
    fn spawn_native_original<F: Future<Output = ()> + Send + 'static>(&self, original: F) {
        if let Err(refused) = self.start(original, None, true) {
            // Original tokio::spawn also panics without an actual runtime.
            // No native future/RPC has been polled on this definite refusal.
            panic!(
                "original Session worker has no Tokio runtime: {}",
                refused.error
            );
        }
    }
    pub(crate) fn close(&self) {
        let mut state = self
            .0
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.closed = true;
        drop(state);
        self.0.changes.send_replace(());
    }
    pub(crate) fn capture(&self) -> io::Result<Capture> {
        let mut retired = Vec::new();
        let mut state = self.0.progress.lock().map_err(invalid)?;
        state.jobs.retain(|job| {
            if job.try_retire() {
                retired.push(job.clone());
                false
            } else {
                true
            }
        });
        if !retired.is_empty() {
            Self::bump(&mut state);
        }
        if state.jobs.len() > CAPTURE_MAX || state.closed || state.uncertain {
            drop(state);
            drop(retired);
            return Err(invalid("actual original Session work cut is unavailable"));
        }
        let capture = Capture {
            book: self.clone(),
            generation: state.generation,
            jobs: state.jobs.clone(),
        };
        drop(state);
        drop(retired);
        Ok(capture)
    }
    /// Actual shutdown closes new ROOT admission first. Already accepted
    /// original workers may still register their real descendant cleanup work.
    /// Join every generation until no such worker can publish another child.
    pub(crate) async fn drain(&self) -> io::Result<()> {
        self.close();
        let mut first = None;
        loop {
            let jobs = { self.0.progress.lock().map_err(invalid)?.jobs.clone() };
            for job in &jobs {
                if let Err(error) = job.join().await {
                    first.get_or_insert(error);
                }
            }
            let state = self.0.progress.lock().map_err(invalid)?;
            // Exact held Arc set, not a wrapping counter, proves every current
            // node was joined. Successful joins have ended their parent scopes;
            // failed joins remain an explicit error, never Source settlement.
            if state
                .jobs
                .iter()
                .all(|current| jobs.iter().any(|joined| Arc::ptr_eq(current, joined)))
            {
                if state.uncertain {
                    first.get_or_insert_with(|| {
                        invalid("original Session task ledger generation is uncertain")
                    });
                }
                return first.map_or(Ok(()), Err);
            }
        }
    }
}
/// Definite LOCAL pre-spawn refusal retains the exact original future/resources
/// for the native caller's original failure/queue policy; never silently drop it.
pub(crate) struct Refused<F> {
    pub(crate) original: F,
    pub(crate) error: io::Error,
}
/// Only the actual running original worker creates this task-local parent.
/// Nested original helper tasks join SAME book even after root admission closes.
/// Outside a tracked worker, historical ordinary helper scheduling stays ordinary.
pub(crate) fn spawn_original_descendant<F: Future<Output = ()> + Send + 'static>(
    original: F,
) -> std::result::Result<(), Refused<F>> {
    match PARENT.try_with(Arc::clone) {
        Ok(parent) => Book(parent.book.clone()).start(original, Some(&parent), false),
        Err(_) => match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                let _ordinary = runtime.spawn(original);
                Ok(())
            }
            Err(error) => Err(Refused {
                original,
                error: invalid(error),
            }),
        },
    }
}
#[derive(Default)]
struct Lifecycle {
    worker: Option<JoinHandle<()>>,
    joined: bool,
    failure: Option<tokio::task::JoinError>,
}
struct Job {
    life: Mutex<Lifecycle>,
    join: Arc<Semaphore>,
    done: AtomicBool,
    lost: AtomicBool,
    book: Weak<State>,
}
impl Job {
    fn prepare(book: Weak<State>) -> Arc<Self> {
        Arc::new(Self {
            life: Mutex::new(Lifecycle::default()),
            join: Arc::new(Semaphore::new(1)),
            done: AtomicBool::new(false),
            lost: AtomicBool::new(false),
            book,
        })
    }
    fn wake(&self) {
        if let Some(book) = self.book.upgrade() {
            book.changes.send_replace(());
        }
    }
    fn start(
        self: &Arc<Self>,
        runtime: tokio::runtime::Handle,
        book: Arc<State>,
        original: impl Future<Output = ()> + Send + 'static,
    ) {
        let mut life = self.life.lock().unwrap_or_else(PoisonError::into_inner);
        let actual = self.clone();
        let guard = Guard {
            actual: actual.clone(),
            complete: false,
        };
        let parent = Arc::new(Parent {
            book,
            active: AtomicBool::new(true),
        });
        life.worker = Some(runtime.spawn(async move {
            let mut guard = guard;
            PARENT.scope(parent.clone(), original).await;
            parent.active.store(false, Ordering::Release);
            actual.done.store(true, Ordering::Release);
            guard.complete = true;
            actual.wake();
        }));
    }
    fn terminal(&self) -> io::Result<()> {
        if self.lost.load(Ordering::Acquire) || !self.done.load(Ordering::Acquire) {
            Err(invalid("original Session worker outcome remains uncertain"))
        } else {
            Ok(())
        }
    }
    async fn join(self: &Arc<Self>) -> io::Result<()> {
        let _one = self.join.clone().acquire_owned().await.map_err(invalid)?;
        let worker = {
            let mut life = self.life.lock().map_err(invalid)?;
            if life.joined {
                None
            } else {
                life.worker.take()
            }
        };
        if let Some(worker) = worker {
            let mut lease = JoinLease {
                actual: self.clone(),
                worker: Some(worker),
            };
            let result = match lease.worker.as_mut() {
                Some(actual) => actual.await,
                None => return Err(invalid("actual Session worker join lease absent")),
            };
            lease.worker.take();
            let mut life = self.life.lock().map_err(invalid)?;
            life.joined = true;
            if let Err(error) = result {
                self.lost.store(true, Ordering::Release);
                life.failure = Some(error);
            }
        }
        if !self.life.lock().map_err(invalid)?.joined {
            return Err(invalid("actual Session worker handle missing"));
        }
        self.terminal()
    }
    fn try_retire(&self) -> bool {
        let Ok(_one) = self.join.clone().try_acquire_owned() else {
            return false;
        };
        let mut life = self.life.lock().unwrap_or_else(PoisonError::into_inner);
        if !life.joined {
            if !life.worker.as_ref().is_some_and(JoinHandle::is_finished) {
                return false;
            }
            let waker = futures::task::noop_waker_ref();
            let mut cx = Context::from_waker(waker);
            let result = match life.worker.as_mut() {
                Some(worker) => Pin::new(worker).poll(&mut cx),
                None => return false,
            };
            let Poll::Ready(result) = result else {
                return false;
            };
            life.worker.take();
            life.joined = true;
            if let Err(error) = result {
                self.lost.store(true, Ordering::Release);
                life.failure = Some(error);
            }
        }
        drop(life);
        self.terminal().is_ok()
    }
}
struct Guard {
    actual: Arc<Job>,
    complete: bool,
}
impl Drop for Guard {
    fn drop(&mut self) {
        if !self.complete {
            self.actual.lost.store(true, Ordering::Release);
            self.actual.wake();
        }
    }
}
struct JoinLease {
    actual: Arc<Job>,
    worker: Option<JoinHandle<()>>,
}
impl Drop for JoinLease {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            self.actual
                .life
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .worker = Some(worker);
        }
    }
}
pub(crate) struct Capture {
    book: Book,
    generation: u64,
    jobs: Vec<Arc<Job>>,
}
pub(crate) struct Ready {
    book: Book,
    generation: u64,
}
impl Capture {
    pub(crate) fn changes(&self) -> tokio::sync::watch::Receiver<()> {
        self.book.changes()
    }
    pub(crate) async fn observe(&self) -> io::Result<Ready> {
        let mut first = None;
        for job in &self.jobs {
            if let Err(error) = job.join().await {
                first.get_or_insert(error);
            }
        }
        if let Some(error) = first {
            return Err(error);
        }
        let ready = Ready {
            book: self.book.clone(),
            generation: self.generation,
        };
        ready.check()?;
        Ok(ready)
    }
}
impl Ready {
    pub(crate) fn check(&self) -> io::Result<()> {
        let state = self.book.0.progress.lock().map_err(invalid)?;
        if state.closed || state.uncertain || state.generation != self.generation {
            Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "original Session worker cut changed",
            ))
        } else {
            Ok(())
        }
    }
    pub(crate) fn same_book(&self, actual: &Book) -> bool {
        Arc::ptr_eq(&self.book.0, &actual.0)
    }
}

/// Lower controls enter the actual original native picker hook.
#[cfg(test)]
fn fetch_picker(
    session: &super::AppServerSession,
    request_id: uuid::Uuid,
    sender: crate::app_event_sender::AppEventSender,
) {
    session.fetch_models(request_id, sender);
}
#[cfg(test)]
#[path = "retained_session_work_tests.rs"]
mod tests;

impl super::AppServerSession {
    pub(crate) fn alab_spawn_original_work(
        &self,
        original: impl Future<Output = ()> + Send + 'static,
    ) {
        self.alab_original_work.spawn_native_original(original);
    }
    pub(crate) fn alab_capture_original_session_jobs(&self) -> io::Result<Capture> {
        self.alab_original_work.capture()
    }
}
impl Ready {
    pub(crate) fn current(&self, session: &super::AppServerSession) -> io::Result<()> {
        if session.connection_owner.is_none() || !self.same_book(&session.alab_original_work) {
            return Err(invalid(
                "original Session work cut belongs to another physical owner",
            ));
        }
        self.check()
    }
}
/// Original helper workers inherit only the actual running Session task's
/// private scheduling domain. Ordinary popup callers keep their native path.
pub(crate) fn spawn_original_helper(original: impl Future<Output = ()> + Send + 'static) {
    if let Err(refused) = spawn_original_descendant(original) {
        panic!(
            "original Session helper has no Tokio runtime: {}",
            refused.error
        );
    }
}
