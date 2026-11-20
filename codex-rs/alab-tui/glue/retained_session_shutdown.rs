// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! The actual consuming physical shutdown remains owned when its observer leaves.
//! Completion here reports original shutdown/joins only, never Source/effects permission.
use super::*;

struct Progress {
    result: Option<io::Result<()>>,
    terminal: Option<std::result::Result<(), io::ErrorKind>>,
    lost: bool,
}
#[derive(Default)]
struct Lifecycle {
    pending: Option<JoinHandle<()>>,
    joined: bool,
}
pub(super) struct Operation {
    progress: Mutex<Progress>,
    lifecycle: Mutex<Lifecycle>,
    join: Arc<Semaphore>,
    changes: tokio::sync::watch::Sender<()>,
}
impl Operation {
    pub(super) fn prepare() -> Arc<Self> {
        let (changes, _) = tokio::sync::watch::channel(());
        Arc::new(Self {
            progress: Mutex::new(Progress {
                result: None,
                terminal: None,
                lost: false,
            }),
            lifecycle: Mutex::new(Lifecycle::default()),
            join: Arc::new(Semaphore::new(1)),
            changes,
        })
    }
    pub(super) fn start(
        self: &Arc<Self>,
        runtime: tokio::runtime::Handle,
        book: Book,
        original: impl std::future::Future<Output = io::Result<()>> + Send + 'static,
    ) {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let actual = self.clone();
        let guard = WorkerGuard {
            actual: actual.clone(),
            complete: false,
        };
        lifecycle.pending = Some(runtime.spawn(async move {
            let mut guard = guard;
            // A strong SAME Book pin survives canceled consuming shutdown. The
            // Book holds this operation/handle; no permanent back-reference is
            // stored after the actual worker completes.
            let _original_book = book;
            let result = original.await;
            let terminal = result.as_ref().copied().map_err(io::Error::kind);
            {
                let mut progress = actual
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                progress.result = Some(result);
                progress.terminal = Some(terminal);
            }
            guard.complete = true;
            actual.changes.send_replace(());
        }));
    }
    pub(super) async fn observe(self: &Arc<Self>) -> io::Result<()> {
        let _one = self.join.clone().acquire_owned().await.map_err(invalid)?;
        let pending = {
            let mut lifecycle = self.lifecycle.lock().map_err(invalid)?;
            if lifecycle.joined {
                None
            } else {
                lifecycle.pending.take()
            }
        };
        if let Some(pending) = pending {
            let mut lease = JoinLease {
                actual: self.clone(),
                pending: Some(pending),
            };
            let result = match lease.pending.as_mut() {
                Some(actual) => actual.await,
                None => return Err(invalid("original shutdown join lease absent")),
            };
            lease.pending.take();
            self.lifecycle.lock().map_err(invalid)?.joined = true;
            if result.is_err() {
                self.progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .lost = true;
            }
        }
        if !self.lifecycle.lock().map_err(invalid)?.joined {
            return Err(invalid("original shutdown has no retained join handle"));
        }
        let mut progress = self.progress.lock().map_err(invalid)?;
        if progress.lost {
            return Err(invalid("actual original shutdown task remains uncertain"));
        }
        // First ordinary observer obtains the exact native error. A repeat
        // cannot rerun physical shutdown or turn its retained failure into Ok.
        if let Some(result) = progress.result.take() {
            return result;
        }
        match progress.terminal {
            Some(Ok(())) => Ok(()),
            Some(Err(kind)) => Err(io::Error::new(
                kind,
                "original physical shutdown failure already observed",
            )),
            None => Err(invalid("original shutdown completion absent")),
        }
    }
}
struct WorkerGuard {
    actual: Arc<Operation>,
    complete: bool,
}
impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if !self.complete {
            self.actual
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .lost = true;
            self.actual.changes.send_replace(());
        }
    }
}
struct JoinLease {
    actual: Arc<Operation>,
    pending: Option<JoinHandle<()>>,
}
impl Drop for JoinLease {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            self.actual
                .lifecycle
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pending = Some(pending);
        }
    }
}
#[cfg(test)]
#[path = "retained_session_shutdown_tests.rs"]
mod tests;
