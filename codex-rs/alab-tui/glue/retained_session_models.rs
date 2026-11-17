// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! SAME original model-prefetch task/result custody. Cache DATA is not Source
//! currentness, model permission, UI application or a Native/Core receipt.
use super::*;
use codex_app_server_client::retained_model_reads::Capture as ReadCapture;
use codex_app_server_client::retained_model_reads::Recorded;
use futures::FutureExt;
use futures::task::AtomicWaker;
use serde::Deserialize;
use serde::Serialize;
use std::any::Any;
use std::io;
use std::sync::PoisonError;
use std::sync::Weak;
use std::task::Context;
use std::task::Poll;
use tokio::task::JoinHandle;
#[path = "retained_session_model_data.rs"]
pub(crate) mod model;
const CONTEXT: &str = "model/list failed during TUI bootstrap";
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::other(message.to_string())
}
#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Data {
    Ready { models: Vec<model::Data> },
    Failed { reply: Recorded },
}
#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
enum WireData {
    Ready { models: Vec<model::Data> },
    Failed { reply: Recorded },
}
impl<'de> Deserialize<'de> for Data {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> std::result::Result<Self, D::Error> {
        let raw = Box::<serde_json::value::RawValue>::deserialize(decoder)?;
        Self::decode_raw(raw.get().as_bytes()).map_err(serde::de::Error::custom)
    }
}
impl Data {
    fn decode_raw(raw: &[u8]) -> io::Result<Self> {
        if raw.len() > 16 * 1024 * 1024 {
            return Err(invalid("original model prefetch DATA capacity"));
        }
        let wire: WireData = serde_json::from_slice(raw).map_err(invalid)?;
        let data = match wire {
            WireData::Ready { models } => Self::Ready { models },
            WireData::Failed { reply } => Self::Failed { reply },
        };
        let supplied: serde_json::Value = serde_json::from_slice(raw).map_err(invalid)?;
        if supplied != serde_json::to_value(&data).map_err(invalid)? {
            return Err(invalid(
                "original model prefetch wire differs from captured native shape",
            ));
        }
        data.validate_data()?;
        Ok(data)
    }
    fn capture(result: &Result<Vec<ModelPreset>>, read: &ReadCapture) -> io::Result<Self> {
        let data = match result {
            Ok(models) => Self::Ready {
                models: models.iter().map(model::Data::capture).collect(),
            },
            Err(actual) => {
                let reply = read.recorded()?;
                let error = match reply.decode::<codex_app_server_protocol::ModelListResponse>() {
                    Ok(_) => {
                        return Err(invalid(
                            "actual model error lost its original failing response",
                        ));
                    }
                    Err(error) => bootstrap_request_error(CONTEXT, error),
                };
                if actual.to_string() != error.to_string() {
                    return Err(invalid("original model prefetch error context differs"));
                }
                Self::Failed { reply }
            }
        };
        data.validate_data()?;
        Ok(data)
    }
    pub(crate) fn validate_data(&self) -> io::Result<()> {
        if serde_json::to_vec(self).map_err(invalid)?.len() > 16 * 1024 * 1024 {
            return Err(invalid("original model prefetch DATA capacity"));
        }
        if let Self::Ready { models } = self {
            for model in models {
                model.prepare()?;
            }
        }
        if let Self::Failed { reply } = self {
            reply.validate_data()?;
            if reply
                .decode::<codex_app_server_protocol::ModelListResponse>()
                .is_ok()
            {
                return Err(invalid("cached model failure contains a success reply"));
            }
        }
        Ok(())
    }
    fn native(self) -> Result<Vec<ModelPreset>> {
        match self {
            Self::Ready { models } => models
                .iter()
                .map(model::Data::prepare)
                .collect::<io::Result<Vec<_>>>()
                .map_err(Into::into),
            Self::Failed { reply } => {
                match reply.decode::<codex_app_server_protocol::ModelListResponse>() {
                    Ok(_) => Err(color_eyre::eyre::eyre!(
                        "cached model error DATA was not a failure"
                    )),
                    Err(error) => Err(bootstrap_request_error(CONTEXT, error)),
                }
            }
        }
    }
    /// Pure prepared cache only. No task/source/permission is created. The
    /// enclosing actual Session binder remains required before installation.
    pub(crate) fn prepare(self) -> io::Result<PreparedCache> {
        self.validate_data()?;
        let native = self.clone().native();
        Ok(PreparedCache { native, data: self })
    }
}
pub(crate) struct PreparedCache {
    native: Result<Vec<ModelPreset>>,
    data: Data,
}
struct Progress {
    native: Option<Result<Vec<ModelPreset>>>,
    data: Option<std::result::Result<Data, String>>,
    panic: Option<Box<dyn Any + Send + 'static>>,
    lost: bool,
    consumed: bool,
}
impl Default for Progress {
    fn default() -> Self {
        Self {
            native: None,
            data: None,
            panic: None,
            lost: false,
            consumed: false,
        }
    }
}
#[derive(Default)]
struct Lifecycle {
    started: bool,
    joined: bool,
    cached: bool,
    refused: bool,
    pending: Option<JoinHandle<()>>,
}
struct Job {
    progress: Mutex<Progress>,
    lifecycle: Mutex<Lifecycle>,
    join: Arc<Semaphore>,
    waker: AtomicWaker,
    book: Weak<BookState>,
}
struct WorkerGuard {
    job: Arc<Job>,
    complete: bool,
}
impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if !self.complete {
            self.job
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .lost = true;
            self.job.wake();
        }
    }
}
impl Job {
    fn prepare(book: Weak<BookState>) -> Arc<Self> {
        Arc::new(Self {
            progress: Mutex::new(Progress::default()),
            lifecycle: Mutex::new(Lifecycle::default()),
            join: Arc::new(Semaphore::new(1)),
            waker: AtomicWaker::new(),
            book,
        })
    }
    fn wake(&self) {
        self.waker.wake();
        if let Some(book) = self.book.upgrade() {
            book.changes.send_replace(());
        }
    }
    fn consumed(&self) -> bool {
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .consumed
    }
    fn start(
        self: &Arc<Self>,
        original: impl std::future::Future<Output = Result<Vec<ModelPreset>>> + Send + 'static,
    ) {
        // This lifecycle gate is NOT the worker's progress mutex. It covers
        // actual spawn +handle storage; the worker never takes it.
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if lifecycle.started {
            return;
        }
        lifecycle.started = true;
        let job = self.clone();
        let guard = WorkerGuard {
            job: job.clone(),
            complete: false,
        };
        let read = ReadCapture::prepare();
        lifecycle.pending = Some(tokio::spawn(async move {
            let mut guard = guard;
            let result = std::panic::AssertUnwindSafe(read.scope(original))
                .catch_unwind()
                .await;
            match result {
                Ok(native) => {
                    let data = Data::capture(&native, &read).map_err(|error| error.to_string());
                    let mut progress = job.progress.lock().unwrap_or_else(PoisonError::into_inner);
                    progress.data = Some(data);
                    progress.native = Some(native);
                }
                Err(panic) => {
                    job.progress
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .panic = Some(panic)
                }
            }
            guard.complete = true;
            job.wake();
        }));
    }
    async fn join(self: &Arc<Self>) -> io::Result<()> {
        let _one = self.join.clone().acquire_owned().await.map_err(invalid)?;
        let (complete, pending) = {
            let mut lifecycle = self.lifecycle.lock().map_err(invalid)?;
            let complete = lifecycle.joined || lifecycle.cached || lifecycle.refused;
            (
                complete,
                if complete {
                    None
                } else {
                    lifecycle.pending.take()
                },
            )
        };
        if complete {
            return self.check_terminal();
        }
        let Some(pending) = pending else {
            return Err(invalid("original model task has no retained handle"));
        };
        let mut lease = JoinLease {
            job: self.clone(),
            pending: Some(pending),
        };
        let result = match lease.pending.as_mut() {
            Some(pending) => pending.await,
            None => return Err(invalid("original model join lease missing")),
        };
        lease.pending.take();
        {
            let mut lifecycle = self.lifecycle.lock().map_err(invalid)?;
            lifecycle.joined = true;
        }
        if result.is_err() {
            self.progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .lost = true;
            self.wake();
            return Err(invalid("original model task failed before observed join"));
        }
        self.check_terminal()
    }
    fn check_terminal(&self) -> io::Result<()> {
        let progress = self.progress.lock().map_err(invalid)?;
        if progress.lost
            || progress.panic.is_some()
            || (!progress.consumed && progress.native.is_none())
        {
            return Err(invalid("original model task terminal outcome is uncertain"));
        }
        Ok(())
    }
    fn refused_before_spawn(&self) {
        self.lifecycle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .refused = true;
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .native = Some(Err(color_eyre::eyre::eyre!(
            "original session model admission is closing"
        )));
        self.wake();
    }
    fn snapshot(&self) -> io::Result<Data> {
        let ready = {
            let lifecycle = self.lifecycle.lock().map_err(invalid)?;
            lifecycle.joined || lifecycle.cached
        };
        if !ready {
            return Err(invalid("original model task has not actually joined"));
        }
        let progress = self.progress.lock().map_err(invalid)?;
        if progress.lost || progress.panic.is_some() {
            return Err(invalid("original model task outcome remains uncertain"));
        }
        match &progress.data {
            Some(Ok(data)) => Ok(data.clone()),
            Some(Err(error)) => Err(invalid(error)),
            None => Err(invalid("original model task result absent")),
        }
    }
}
struct JoinLease {
    job: Arc<Job>,
    pending: Option<JoinHandle<()>>,
}
impl Drop for JoinLease {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            self.job
                .lifecycle
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pending = Some(pending);
        }
    }
}
#[derive(Clone)]
pub(crate) struct Prefetch(Arc<Job>);
pub(super) struct Observer(Arc<Job>);
impl std::future::Future for Observer {
    type Output = Result<Vec<ModelPreset>>;
    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let job = &self.0;
        job.waker.register(cx.waker());
        let (native, panic, lost, already) = {
            let mut progress = job.progress.lock().unwrap_or_else(PoisonError::into_inner);
            let already = progress.consumed;
            let native = progress.native.take();
            let panic = progress.panic.take();
            let lost = progress.lost;
            if native.is_some() || panic.is_some() || lost {
                progress.consumed = true;
            }
            (native, panic, lost, already)
        };
        if already {
            return Poll::Ready(Err(color_eyre::eyre::eyre!(
                "original model prefetch result was already consumed"
            )));
        }
        if native.is_some() || panic.is_some() || lost {
            if let Some(book) = job.book.upgrade() {
                book.changed();
            }
        }
        if let Some(panic) = panic {
            std::panic::resume_unwind(panic);
        }
        if let Some(native) = native {
            return Poll::Ready(native);
        }
        if lost {
            return Poll::Ready(Err(color_eyre::eyre::eyre!(
                "original model prefetch worker was lost"
            )));
        }
        Poll::Pending
    }
}
#[derive(Default)]
struct BookProgress {
    jobs: Vec<Arc<Job>>,
    generation: u64,
    uncertain: bool,
    stopping: bool,
}
struct BookState {
    progress: Mutex<BookProgress>,
    changes: tokio::sync::watch::Sender<()>,
    shutdown: Mutex<Option<Arc<shutdown::Operation>>>,
}
impl BookState {
    fn changed(&self) {
        let mut progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        match progress.generation.checked_add(1) {
            Some(next) => progress.generation = next,
            None => progress.uncertain = true,
        }
        drop(progress);
        self.changes.send_replace(());
    }
}
#[derive(Clone)]
pub(crate) struct Book(Arc<BookState>);
impl Default for Book {
    fn default() -> Self {
        let (changes, _) = tokio::sync::watch::channel(());
        Self(Arc::new(BookState {
            progress: Mutex::new(BookProgress::default()),
            changes,
            shutdown: Mutex::new(None),
        }))
    }
}
impl Book {
    fn register_locked(progress: &mut BookProgress, job: &Arc<Job>) -> bool {
        if progress.stopping {
            return false;
        }
        progress.jobs.push(job.clone());
        match progress.generation.checked_add(1) {
            Some(next) => progress.generation = next,
            None => progress.uncertain = true,
        }
        true
    }
    fn start_job(
        &self,
        job: &Arc<Job>,
        original: impl std::future::Future<Output = Result<Vec<ModelPreset>>> + Send + 'static,
    ) -> bool {
        let mut progress = self
            .0
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !Self::register_locked(&mut progress, job) {
            return false;
        }
        // SAME close/start gate covers admission, actual spawn AND handle store.
        // A shutdown cut cannot observe an accepted job without its real handle.
        job.start(original);
        drop(progress);
        self.0.changes.send_replace(());
        true
    }
    #[cfg(test)]
    fn register(&self, job: &Arc<Job>) -> bool {
        let mut progress = self
            .0
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let accepted = Self::register_locked(&mut progress, job);
        drop(progress);
        self.0.changes.send_replace(());
        accepted
    }
    pub(crate) fn stop(&self) {
        let mut progress = self
            .0
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        progress.stopping = true;
        drop(progress);
        self.0.changes.send_replace(());
    }
    pub(crate) fn capture(&self) -> Capture {
        let progress = self
            .0
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        Capture {
            book: self.clone(),
            generation: progress.generation,
            jobs: progress.jobs.clone(),
            uncertain: progress.uncertain || progress.stopping,
        }
    }
    pub(crate) async fn drain(&self) -> io::Result<()> {
        let capture = self.capture();
        let mut first = None;
        // Start/inspect every original join, preserving the first error.
        for job in &capture.jobs {
            if let Err(error) = job.join().await {
                first.get_or_insert(error);
            }
        }
        first.map_or(Ok(()), Err)
    }
}
pub(crate) struct Capture {
    book: Book,
    generation: u64,
    jobs: Vec<Arc<Job>>,
    uncertain: bool,
}
pub(crate) struct Ready {
    book: Book,
    generation: u64,
}
impl Capture {
    pub(crate) fn changes(&self) -> tokio::sync::watch::Receiver<()> {
        self.book.0.changes.subscribe()
    }
    pub(crate) async fn observe(&self) -> io::Result<Ready> {
        if self.uncertain {
            return Err(invalid("original model job generation interrupted"));
        }
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
        let progress = self.book.0.progress.lock().map_err(invalid)?;
        if progress.uncertain || progress.stopping || progress.generation != self.generation {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "actual model job/consumer changed during pre-H1 capture",
            ));
        }
        Ok(())
    }
    pub(crate) fn current(&self, session: &AppServerSession) -> io::Result<Option<Data>> {
        self.check()?;
        if session.connection_owner.is_none()
            || !Arc::ptr_eq(&self.book.0, &session.alab_model_jobs.0)
        {
            return Err(invalid(
                "model job capture belongs to another actual session",
            ));
        }
        let data = match &session.prefetched_model_list {
            Some(actual) if !actual.0.consumed() => Some(actual.0.snapshot()?),
            _ => None,
        };
        self.check()?;
        Ok(data)
    }
}
pub(super) fn start(session: &mut AppServerSession) {
    let original = session.request_model_list();
    let job = Job::prepare(Arc::downgrade(&session.alab_model_jobs.0));
    session.prefetched_model_list = Some(Prefetch(job.clone())); // retained BEFORE actual spawn/any await.
    if !session.alab_model_jobs.start_job(&job, original) {
        job.refused_before_spawn();
    }
}
pub(super) fn observer(session: &mut AppServerSession) -> Option<Observer> {
    if session
        .prefetched_model_list
        .as_ref()
        .is_some_and(|actual| actual.0.consumed())
    {
        session.prefetched_model_list.take();
    }
    session
        .prefetched_model_list
        .as_ref()
        .map(|actual| Observer(actual.0.clone()))
}
impl PreparedCache {
    // Only the sibling actual Session-state binder will call this. Pure cache
    // preparation itself cannot allocate/apply a session or make a source grant.
    pub(super) fn into_prefetch(self, book: &Book) -> Prefetch {
        let job = Job::prepare(Arc::downgrade(&book.0));
        {
            let mut progress = job.progress.lock().unwrap_or_else(PoisonError::into_inner);
            progress.native = Some(self.native);
            progress.data = Some(Ok(self.data));
        }
        // No original task is invented: this is a cached-value owner, not a
        // claim that a new task/source effect completed in C2.
        job.lifecycle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .cached = true;
        Prefetch(job)
    }
}
pub(super) async fn shutdown_actual(mut session: AppServerSession) -> io::Result<()> {
    if session.connection_owner.is_none() {
        return Ok(());
    }
    let runtime = tokio::runtime::Handle::try_current().map_err(invalid)?;
    let book = session.alab_model_jobs.clone();
    // Publish SAME actual shutdown owner before any resource is consumed/polled.
    // Request-only views never reach this branch or close/drain the physical owner.
    let operation = {
        let mut slot = book
            .0
            .shutdown
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if slot.is_some() {
            return Err(invalid(
                "owning session already has an original shutdown operation",
            ));
        }
        book.stop();
        let work = session.alab_original_work.clone();
        work.close();
        let client = session
            .connection_owner
            .take()
            .ok_or_else(|| invalid("physical owner disappeared before original shutdown"))?;
        #[cfg(unix)]
        let tenure = session.owned_local.take();
        let same_book = book.clone();
        let operation = shutdown::Operation::prepare();
        *slot = Some(operation.clone());
        operation.start(runtime, book.clone(), async move {
            let physical = client.shutdown().await;
            let joined = same_book.drain().await;
            let background = work.drain().await;
            #[cfg(unix)]
            let native = match tenure {
                Some(actual) => actual.close().await,
                None => Ok(()),
            };
            #[cfg(not(unix))]
            let native = Ok(());
            physical.and(joined).and(background).and(native)
        });
        operation
    };
    operation.observe().await
}
#[path = "retained_session_shutdown.rs"]
mod shutdown;
impl AppServerSession {
    /// Obtain/retain this owner BEFORE await; the original App/Tui pump remains
    /// responsible for driving input while Capture.observe joins original work.
    pub(crate) fn alab_capture_original_model_jobs(&self) -> Capture {
        self.alab_model_jobs.capture()
    }
}
#[cfg(test)]
#[path = "retained_session_models_tests.rs"]
mod tests;
