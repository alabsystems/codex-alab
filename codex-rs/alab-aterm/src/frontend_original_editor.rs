// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The original successful Child/TempPath, retained before any escrow await.
//! This local operation creates NO Native editor/Go/currentness permission.
//! Native escrow must accept this actual private resource before active E1 Go;
//! there is deliberately no PID/path/checkpoint restoration constructor here.
use std::collections::BTreeMap;
use std::io;
use std::process::ExitStatus;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::OnceLock;
use std::sync::PoisonError;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use tokio::process::Child;
use tokio::process::Command;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tokio::sync::watch;
use tokio::task::JoinHandle;

const MAX_JOBS: usize = 4096;
const CALLBACK_PENDING: u8 = 0;
const CALLBACK_APPLYING: u8 = 1;
const CALLBACK_APPLIED: u8 = 2;
const CALLBACK_UNKNOWN: u8 = 3;
fn unavailable(message: &'static str) -> io::Error {
    io::Error::other(message)
}
fn registry() -> &'static Mutex<BTreeMap<u64, Arc<Original>>> {
    static RECORDS: OnceLock<Mutex<BTreeMap<u64, Arc<Original>>>> = OnceLock::new();
    RECORDS.get_or_init(|| Mutex::new(BTreeMap::new()))
}
struct Progress {
    command: Option<Command>,
    child: Option<Child>,
    reaped: Option<ExitStatus>,
    quarantined_children: Vec<Child>,
    quarantined_outcomes: Vec<Arc<Value>>,
    started: bool,
    finished: Option<Arc<Value>>,
    c0_read: Option<Arc<c0::OriginalRead>>,
    unknown: bool,
}
#[derive(Default)]
struct Lifecycle {
    starting: bool,
    cancelled: bool,
    joined: bool,
    worker: Option<JoinHandle<()>>,
    join_failure: Option<tokio::task::JoinError>,
    spawn: Option<JoinHandle<()>>,
    spawn_joined: bool,
    spawn_join_failure: Option<tokio::task::JoinError>,
    read: Option<JoinHandle<()>>,
    read_joined: bool,
    read_join_failure: Option<tokio::task::JoinError>,
}
struct Original {
    key: u64,
    document: tempfile::TempPath,
    progress: Mutex<Progress>,
    lifecycle: Mutex<Lifecycle>,
    changed: watch::Sender<()>,
    progression: Arc<Semaphore>,
    join: Semaphore,
    spawn_join: Semaphore,
    read_join: Semaphore,
    callback: AtomicU8,
}
enum Value {
    StartFailed(Arc<io::Error>),
    Root {
        status: ExitStatus,
        document: Result<String, Arc<io::Error>>,
    },
}
/// Retain in the actual App before the first wait. Its registry independently
/// keeps the exact original command, document and late child/result alive.
pub struct Operation(Arc<Original>);
/// Synchronous pre-child exclusion. Keep this SAME guard through parser stop,
/// mode-owner installation and publication of every accepted owner into the
/// actual Job. Dropping it permits a real wait observer to start the command.
/// It is deliberately not Send and must never cross an await.
pub struct BeforeStart<'a> {
    original: &'a Arc<Original>,
    lifecycle: MutexGuard<'a, Lifecycle>,
}
/// Definite local no-child cancellation of THIS original operation. This is
/// not a Finished, Native handback, or permission to reopen another resource.
pub struct CancelledBeforeStart {
    original: Arc<Original>,
}
impl BeforeStart<'_> {
    pub fn is_for(&self, operation: &Operation) -> bool {
        Arc::ptr_eq(self.original, &operation.0)
    }
    /// Keep all partial owners retained on refusal. The receipt is issued only
    /// after exact registry retirement while the SAME start gate is still held.
    pub fn cancel(mut self) -> io::Result<CancelledBeforeStart> {
        self.lifecycle.cancelled = true;
        self.original.remove_exact()?;
        Ok(CancelledBeforeStart {
            original: self.original.clone(),
        })
    }
}
impl CancelledBeforeStart {
    pub fn is_for(&self, operation: &Operation) -> bool {
        Arc::ptr_eq(&self.original, &operation.0)
    }
}
pub struct Finished {
    original: Arc<Original>,
    value: Arc<Value>,
}
pub struct ExecFence {
    original: Arc<Original>,
    _progression: OwnedSemaphorePermit,
}
impl Operation {
    /// No task or child starts here. All refusal paths precede registration.
    pub fn prepare(command: Command, document: tempfile::TempPath) -> io::Result<Arc<Self>> {
        tokio::runtime::Handle::try_current().map_err(io::Error::other)?;
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let key = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| unavailable("original editor operation identity exhausted"))?;
        let (changed, _) = watch::channel(());
        let original = Arc::new(Original {
            key,
            document,
            progress: Mutex::new(Progress {
                command: Some(command),
                child: None,
                reaped: None,
                quarantined_children: Vec::new(),
                quarantined_outcomes: Vec::new(),
                started: false,
                finished: None,
                c0_read: None,
                unknown: false,
            }),
            lifecycle: Mutex::new(Lifecycle::default()),
            changed,
            progression: Arc::new(Semaphore::new(1)),
            join: Semaphore::new(1),
            spawn_join: Semaphore::new(1),
            read_join: Semaphore::new(1),
            callback: AtomicU8::new(CALLBACK_PENDING),
        });
        let mut records = registry()
            .lock()
            .map_err(|_| unavailable("original editor registry poisoned"))?;
        if records.len() >= MAX_JOBS {
            return Err(unavailable("original editor operation capacity"));
        }
        records.insert(key, original.clone());
        Ok(Arc::new(Self(original)))
    }
    /// Hold one actual lifecycle gate across ALL pre-child resource factories
    /// and synchronous publication into the actual owning Job. No child/task,
    /// parser or mode mutation occurs here.
    pub fn before_start(&self) -> io::Result<BeforeStart<'_>> {
        let lifecycle = self
            .0
            .lifecycle
            .lock()
            .map_err(|_| unavailable("original editor lifecycle poisoned"))?;
        if lifecycle.cancelled
            || lifecycle.starting
            || lifecycle.worker.is_some()
            || lifecycle.joined
        {
            return Err(unavailable("original editor already started or cancelled"));
        }
        Ok(BeforeStart {
            original: &self.0,
            lifecycle,
        })
    }
    /// Definite local cancellation only. Callers holding actual partial parser
    /// or mode owners use the typed receipt to perform their original handback.
    pub fn cancel_before_start(&self) -> io::Result<CancelledBeforeStart> {
        self.before_start()?.cancel()
    }
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    fn start(&self) -> io::Result<()> {
        let mut lifecycle = self
            .0
            .lifecycle
            .lock()
            .map_err(|_| unavailable("original editor lifecycle poisoned"))?;
        if lifecycle.cancelled {
            return Err(unavailable("original editor was cancelled before start"));
        }
        if lifecycle.starting {
            return Ok(());
        }
        lifecycle.starting = true;
        let original = self.0.clone();
        let guard = WorkerGuard {
            original: original.clone(),
            completed: false,
        };
        // No worker/progress mutex is held here. The lifecycle gate covers
        // actual spawn+handle storage, and the guard never reacquires it.
        lifecycle.worker = Some(tokio::spawn(async move {
            let mut guard = guard;
            if original.run_original().await.is_err() {
                original
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .unknown = true;
            }
            guard.completed = true;
            original.changed.send_replace(());
        }));
        Ok(())
    }
    /// Schedule the SAME original command after its actual launch event and
    /// pre-child bundle have been published. No wait/read result or Native
    /// permission is produced. Repeated original observers reuse this worker.
    /// Never call while retaining BeforeStart on this thread.
    pub fn start_original(&self) -> io::Result<()> {
        self.start()
    }
    pub async fn wait(&self) -> io::Result<Arc<Finished>> {
        self.start()?;
        let mut changes = self.0.changed.subscribe();
        loop {
            let value = {
                let progress = self
                    .0
                    .progress
                    .lock()
                    .map_err(|_| unavailable("original editor result poisoned"))?;
                if progress.unknown {
                    return Err(unavailable("original editor outcome remains unobserved"));
                }
                progress.finished.clone()
            };
            if let Some(value) = value {
                self.0.join_original().await?;
                return Ok(Arc::new(Finished {
                    original: self.0.clone(),
                    value,
                }));
            }
            changes
                .changed()
                .await
                .map_err(|_| unavailable("original editor result channel retired"))?;
        }
    }
    /// Observe BEFORE starting H1 on the final controller. No root wait/read
    /// can cross this same gate after the fence returns. Native escrow of the
    /// actual resource remains a separate required operation, not this fence.
    pub async fn freeze_before_h1(&self) -> io::Result<ExecFence> {
        self.start()?;
        let mut changes = self.0.changed.subscribe();
        loop {
            {
                let progress = self
                    .0
                    .progress
                    .lock()
                    .map_err(|_| unavailable("original editor state poisoned"))?;
                if progress.unknown {
                    return Err(unavailable("original editor outcome remains unobserved"));
                }
                if progress.started || progress.finished.is_some() {
                    break;
                }
            }
            changes
                .changed()
                .await
                .map_err(|_| unavailable("original editor start observation retired"))?;
        }
        // Join the actual blocking spawn before H1, even when its Child was
        // published a moment before the blocking worker returned.
        self.0.join_spawn().await?;
        let mut permit = self
            .0
            .progression
            .clone()
            .acquire_owned()
            .await
            .map_err(io::Error::other)?;
        let completed = {
            let progress = self
                .0
                .progress
                .lock()
                .map_err(|_| unavailable("original editor state poisoned"))?;
            if progress.unknown {
                return Err(unavailable("original editor outcome remains unobserved"));
            }
            progress.finished.is_some()
        };
        if completed {
            // A completed read may have published immediately before releasing
            // its gate. Join the SAME original worker before freezing a pending
            // callback; do not hold the progression permit while it finishes.
            drop(permit);
            self.0.join_original().await?;
            permit = self
                .0
                .progression
                .clone()
                .acquire_owned()
                .await
                .map_err(io::Error::other)?;
            if self
                .0
                .progress
                .lock()
                .map_err(|_| unavailable("original editor state poisoned"))?
                .unknown
            {
                return Err(unavailable("original editor outcome remains unobserved"));
            }
        }
        // The completed-worker join above temporarily released this permit.
        // A real callback may have applied/retired while that join was awaited.
        // Only the SAME still-pending registry owner can become a pending cut.
        if self.0.callback.load(Ordering::Acquire) != CALLBACK_PENDING {
            return Err(unavailable(
                "original editor callback is already applied or uncertain",
            ));
        }
        let records = registry()
            .lock()
            .map_err(|_| unavailable("original editor registry poisoned"))?;
        if !records
            .get(&self.0.key)
            .is_some_and(|actual| Arc::ptr_eq(actual, &self.0))
        {
            return Err(unavailable(
                "original editor pending registry owner retired",
            ));
        }
        drop(records);
        Ok(ExecFence {
            original: self.0.clone(),
            _progression: permit,
        })
    }
    pub fn is_for_fence(&self, actual: &ExecFence) -> bool {
        Arc::ptr_eq(&self.0, &actual.original)
    }
}
impl ExecFence {
    /// Actual already-reaped/read result from THIS SAME held pending owner.
    /// None means only that no source outcome is cached; it never says that
    /// the root is live/closed, or supplies Native/Go permission. The held gate
    /// makes a Some result stable until the caller explicitly releases its cut.
    pub fn finished_pending(&self) -> io::Result<Option<Arc<Finished>>> {
        if self.original.callback.load(Ordering::Acquire) != CALLBACK_PENDING {
            return Err(unavailable(
                "original frozen callback is applied or uncertain",
            ));
        }
        let records = registry()
            .lock()
            .map_err(|_| unavailable("original editor registry poisoned"))?;
        if !records
            .get(&self.original.key)
            .is_some_and(|actual| Arc::ptr_eq(actual, &self.original))
        {
            return Err(unavailable("original pending outcome registry changed"));
        }
        drop(records);
        let progress = self
            .original
            .progress
            .lock()
            .map_err(|_| unavailable("original editor state poisoned"))?;
        if progress.unknown {
            return Err(unavailable("original outcome remains unobserved"));
        }
        Ok(progress.finished.as_ref().map(|value| {
            Arc::new(Finished {
                original: self.original.clone(),
                value: value.clone(),
            })
        }))
    }
}
impl Original {
    async fn run_original(self: &Arc<Self>) -> io::Result<()> {
        let command = {
            let mut progress = self
                .progress
                .lock()
                .map_err(|_| unavailable("original editor state poisoned"))?;
            progress
                .command
                .take()
                .ok_or_else(|| unavailable("original editor command already moved"))?
        };
        let original = self.clone();
        let guard = WorkerGuard {
            original: original.clone(),
            completed: false,
        };
        {
            // Same lifecycle gate covers spawn +handle storage. The blocking
            // worker/preguard touches only progress, never this lifecycle lock.
            let mut lifecycle = self
                .lifecycle
                .lock()
                .map_err(|_| unavailable("original editor lifecycle poisoned"))?;
            if lifecycle.spawn.is_some() || lifecycle.spawn_joined {
                return Err(unavailable("original editor spawn worker already retained"));
            }
            lifecycle.spawn = Some(tokio::task::spawn_blocking(move || {
                let mut guard = guard; // Move the WHOLE Drop owner, not only its completed field.
                original.spawn_original(command);
                guard.completed = true;
                original.changed.send_replace(());
            }));
        }
        self.join_spawn().await?;
        loop {
            let permit = self
                .progression
                .clone()
                .acquire_owned()
                .await
                .map_err(io::Error::other)?;
            let status = {
                let mut progress = self
                    .progress
                    .lock()
                    .map_err(|_| unavailable("original editor state poisoned"))?;
                if progress.unknown {
                    return Err(unavailable("original editor wait custody unknown"));
                }
                if progress.finished.is_some() {
                    return Ok(());
                }
                let status = progress
                    .child
                    .as_mut()
                    .ok_or_else(|| unavailable("original editor Child absent"))?
                    .try_wait()?;
                if let Some(status) = status {
                    progress.reaped = Some(status);
                }
                status
            };
            if let Some(status) = status {
                let original = self.clone();
                let guard = WorkerGuard {
                    original: original.clone(),
                    completed: false,
                };
                {
                    // Store the actual blocking reader before any observation
                    // await. It owns the SAME progression gate through result
                    // capture, so no wait/read can cross a successful H1 cut.
                    let mut lifecycle = self
                        .lifecycle
                        .lock()
                        .map_err(|_| unavailable("original editor lifecycle poisoned"))?;
                    if lifecycle.read.is_some() || lifecycle.read_joined {
                        return Err(unavailable("original document reader already retained"));
                    }
                    lifecycle.read = Some(tokio::task::spawn_blocking(move || {
                        let mut guard = guard; // The accepted blocking reader owns its full preguard.
                        let _permit = permit;
                        original.capture_document(status);
                        guard.completed = true;
                        original.changed.send_replace(());
                    }));
                }
                self.join_read().await?;
                return Ok(());
            }
            drop(permit);
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }
    fn capture_document(&self, status: ExitStatus) {
        // EXACT original read policy/error, including a FIFO or symlink the
        // editor genuinely left behind. This owned blocking task preserves
        // original behavior without blocking the actual async update observer.
        let document = if status.success() {
            c0::read_original_document(self)
        } else {
            Ok(String::new())
        };
        let value = Arc::new(Value::Root { status, document });
        let (mut progress, poisoned) = match self.progress.lock() {
            Ok(progress) => (progress, false),
            Err(poisoned) => (poisoned.into_inner(), true),
        };
        progress.unknown |= poisoned;
        if progress.finished.is_none() {
            progress.finished = Some(value);
        } else {
            progress.quarantined_outcomes.push(value);
            progress.unknown = true;
        }
        let reaped = progress.child.take();
        drop(progress);
        drop(reaped); // This SAME Child was actually reaped before the read.
    }
    fn spawn_original(&self, mut command: Command) {
        // SAME original Command: no new argv, cwd, env, stdio, pre-exec gate or
        // Native reservation. The accepted record already owns this worker.
        let child = command.spawn();
        {
            // Retain an actual late Child/outcome BEFORE classifying poison or
            // invariant loss. No observer error is allowed to orphan its Drop.
            let (mut progress, poisoned) = match self.progress.lock() {
                Ok(progress) => (progress, false),
                Err(poisoned) => (poisoned.into_inner(), true),
            };
            progress.unknown |= poisoned;
            match child {
                Ok(child) => {
                    if let Some(previous) = progress.child.replace(child) {
                        progress.quarantined_children.push(previous);
                        progress.unknown = true;
                    }
                    progress.started = true;
                }
                Err(error) => {
                    let value = Arc::new(Value::StartFailed(Arc::new(error)));
                    if progress.finished.is_none() {
                        progress.finished = Some(value);
                    } else {
                        progress.quarantined_outcomes.push(value);
                        progress.unknown = true;
                    }
                }
            }
        };
        self.changed.send_replace(());
    }
    async fn join_spawn(self: &Arc<Self>) -> io::Result<()> {
        let _one = self.spawn_join.acquire().await.map_err(io::Error::other)?;
        let pending = {
            let mut lifecycle = self
                .lifecycle
                .lock()
                .map_err(|_| unavailable("original editor lifecycle poisoned"))?;
            if lifecycle.spawn_joined {
                return if lifecycle.spawn_join_failure.is_some() {
                    Err(unavailable("original blocking spawn join failed"))
                } else {
                    Ok(())
                };
            }
            lifecycle
                .spawn
                .take()
                .ok_or_else(|| unavailable("original blocking spawn handle absent"))?
        };
        let mut lease = SpawnLease {
            original: self.clone(),
            pending: Some(pending),
        };
        let result = lease
            .pending
            .as_mut()
            .ok_or_else(|| unavailable("original blocking spawn lease absent"))?
            .await;
        lease.pending.take();
        let mut lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| unavailable("original editor lifecycle poisoned"))?;
        lifecycle.spawn_joined = true;
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                lifecycle.spawn_join_failure = Some(error);
                self.progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .unknown = true;
                Err(unavailable("original blocking spawn join failed"))
            }
        }
    }
    async fn join_read(self: &Arc<Self>) -> io::Result<()> {
        let _one = self.read_join.acquire().await.map_err(io::Error::other)?;
        let pending = {
            let mut lifecycle = self
                .lifecycle
                .lock()
                .map_err(|_| unavailable("original editor lifecycle poisoned"))?;
            if lifecycle.read_joined {
                return if lifecycle.read_join_failure.is_some() {
                    Err(unavailable("original document read join failed"))
                } else {
                    Ok(())
                };
            }
            lifecycle
                .read
                .take()
                .ok_or_else(|| unavailable("original document reader handle absent"))?
        };
        let mut lease = ReadLease {
            original: self.clone(),
            pending: Some(pending),
        };
        let result = lease
            .pending
            .as_mut()
            .ok_or_else(|| unavailable("original document reader lease absent"))?
            .await;
        lease.pending.take();
        let mut lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| unavailable("original editor lifecycle poisoned"))?;
        lifecycle.read_joined = true;
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                lifecycle.read_join_failure = Some(error);
                self.progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .unknown = true;
                Err(unavailable("original document read join failed"))
            }
        }
    }
    async fn join_original(self: &Arc<Self>) -> io::Result<()> {
        let _one = self.join.acquire().await.map_err(io::Error::other)?;
        let pending = {
            let mut lifecycle = self
                .lifecycle
                .lock()
                .map_err(|_| unavailable("original editor lifecycle poisoned"))?;
            if lifecycle.joined {
                return if lifecycle.join_failure.is_some() {
                    Err(unavailable("original editor worker join failed"))
                } else {
                    Ok(())
                };
            }
            lifecycle
                .worker
                .take()
                .ok_or_else(|| unavailable("original editor worker handle absent"))?
        };
        let mut lease = JoinLease {
            original: self.clone(),
            pending: Some(pending),
        };
        let result = lease
            .pending
            .as_mut()
            .ok_or_else(|| unavailable("original editor join lease absent"))?
            .await;
        lease.pending.take();
        let mut lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| unavailable("original editor lifecycle poisoned"))?;
        lifecycle.joined = true;
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                lifecycle.join_failure = Some(error);
                self.progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .unknown = true;
                Err(unavailable("original editor worker join failed"))
            }
        }
    }
    fn remove_exact(self: &Arc<Self>) -> io::Result<()> {
        let retired = {
            let mut records = registry()
                .lock()
                .map_err(|_| unavailable("original editor registry poisoned"))?;
            if !records
                .get(&self.key)
                .is_some_and(|actual| Arc::ptr_eq(actual, self))
            {
                return Err(unavailable("original editor registry identity changed"));
            }
            records.remove(&self.key)
        };
        drop(retired);
        Ok(())
    }
}
struct WorkerGuard {
    original: Arc<Original>,
    completed: bool,
}
impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if !self.completed {
            self.original
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .unknown = true;
            self.original.changed.send_replace(());
        }
    }
}
struct JoinLease {
    original: Arc<Original>,
    pending: Option<JoinHandle<()>>,
}
impl Drop for JoinLease {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            self.original
                .lifecycle
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .worker = Some(pending);
        }
    }
}
struct SpawnLease {
    original: Arc<Original>,
    pending: Option<JoinHandle<()>>,
}
impl Drop for SpawnLease {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            self.original
                .lifecycle
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .spawn = Some(pending);
        }
    }
}
struct ReadLease {
    original: Arc<Original>,
    pending: Option<JoinHandle<()>>,
}
impl Drop for ReadLease {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            self.original
                .lifecycle
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .read = Some(pending);
        }
    }
}
pub enum Failure<'a> {
    Start(&'a io::Error),
    Exit(ExitStatus),
    Document(&'a io::Error),
}
impl std::fmt::Display for Failure<'_> {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Start(error) | Self::Document(error) => write!(output, "{error}"),
            Self::Exit(status) => write!(output, "editor exited with status {status}"),
        }
    }
}
impl Finished {
    pub fn is_for(&self, operation: &Operation) -> bool {
        Arc::ptr_eq(&self.original, &operation.0)
    }
    pub fn outcome(&self) -> Result<&str, Failure<'_>> {
        match self.value.as_ref() {
            Value::StartFailed(error) => Err(Failure::Start(error)),
            Value::Root { status, .. } if !status.success() => Err(Failure::Exit(*status)),
            Value::Root {
                document: Ok(text), ..
            } => Ok(text),
            Value::Root {
                document: Err(error),
                ..
            } => Err(Failure::Document(error)),
        }
    }
    /// The original App applies its real callback in this synchronous closure.
    /// Error/unwind retains exact outcome/resources and prevents duplicate apply.
    /// This local operation has not made a Native reservation; the future real
    /// escrow companion must pair actual Native retirement before removing it.
    pub fn apply_and_retire(&self, apply: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
        let _progression = self.original.progression.try_acquire().map_err(|_| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "actual editor outcome is frozen for the original C1 cut",
            )
        })?;
        if self
            .original
            .progress
            .lock()
            .map_err(|_| unavailable("original editor state poisoned"))?
            .unknown
        {
            return Err(unavailable("original editor outcome remains unobserved"));
        }
        self.original
            .callback
            .compare_exchange(
                CALLBACK_PENDING,
                CALLBACK_APPLYING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map_err(|_| unavailable("original editor callback already applied or uncertain"))?;
        let mut guard = ApplyGuard {
            original: self.original.clone(),
            completed: false,
        };
        apply()?;
        self.original
            .callback
            .store(CALLBACK_APPLIED, Ordering::Release);
        guard.completed = true;
        self.original.remove_exact()
    }
}
struct ApplyGuard {
    original: Arc<Original>,
    completed: bool,
}
impl Drop for ApplyGuard {
    fn drop(&mut self) {
        if !self.completed {
            self.original
                .callback
                .store(CALLBACK_UNKNOWN, Ordering::Release);
        }
    }
}

#[cfg(test)]
#[path = "frontend_original_editor_tests.rs"]
mod tests;

#[path = "frontend_c0_editor.rs"]
mod c0;
pub use c0::C0Capture;
pub use c0::C0Observation;
pub use c0::C0Phase;
pub use c0::C0SpawnCompleted;
pub use c0::OriginalReadObservation;
pub use c0::OriginalReadPhase;
