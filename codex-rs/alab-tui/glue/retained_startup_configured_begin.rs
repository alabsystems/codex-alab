// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One actual Native begin, retained by the original constructor before await.
//! A cancelled observer does not replace the controller or resend the operation.
use super::*;
use crate::app_server_session::AppServerSession;
use crate::owned_local_frontend::FrontendTenure;
use crate::startup_draft::alab_configured::Pending;
use codex_alab_tui::frontend_control::FrontendFrontier;
use tokio::sync::Notify;
use tokio::sync::Semaphore;

#[derive(Clone)]
struct Failure {
    kind: io::ErrorKind,
    message: String,
}
impl Failure {
    fn error(&self) -> io::Error {
        io::Error::new(self.kind, self.message.clone())
    }
}
struct Progress {
    started: bool,
    result: Option<Result<FrontendFrontier, Failure>>,
    pending: Option<Arc<Pending>>,
}
impl Progress {
    fn reject_before_spawn(&mut self, error: &io::Error) {
        if !self.started && self.result.is_none() {
            self.result = Some(Err(Failure {
                kind: error.kind(),
                message: error.to_string(),
            }));
        }
    }
}
struct Worker {
    task: Option<tokio::task::JoinHandle<()>>,
    not_started: bool,
    joined: Option<Result<(), io::ErrorKind>>,
}
pub(crate) struct Begin {
    tenure: FrontendTenure,
    progress: Mutex<Progress>,
    changed: Notify,
    install: Semaphore,
    worker: Mutex<Worker>,
    join: Semaphore,
}
impl Constructor {
    pub(crate) fn configured_begin(&self, session: &AppServerSession) -> io::Result<Arc<Begin>> {
        self.check_current_scope()?;
        let tenure = session
            .retained_frontend()
            .ok_or_else(|| invalid("configured begin lacks actual final delivery"))?;
        let mut slot = self
            .configured_begin
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(original) = &*slot {
            if !original.tenure.same_delivery(tenure) {
                return Err(invalid(
                    "configured begin cannot replace its actual delivery",
                ));
            }
            return Ok(original.clone());
        }
        let owner = Arc::new(Begin {
            tenure: tenure.clone(),
            progress: Mutex::new(Progress {
                started: false,
                result: None,
                pending: None,
            }),
            changed: Notify::new(),
            install: Semaphore::new(1),
            worker: Mutex::new(Worker {
                task: None,
                not_started: true,
                joined: None,
            }),
            join: Semaphore::new(1),
        });
        *slot = Some(owner.clone()); // Original owner exists before spawn/await.
        if let Err(error) = tenure.start_configured_begin(&owner) {
            owner.admission_failed(&error);
            return Err(error);
        }
        Ok(owner)
    }
}
impl Begin {
    fn admission_failed(&self, error: &io::Error) {
        let mut state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        state.reject_before_spawn(error);
        drop(state);
        self.changed.notify_waiters();
    }
    pub(crate) fn start_registered(self: &Arc<Self>) -> io::Result<()> {
        let mut state = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if state.started {
            return Ok(());
        }
        let runtime = tokio::runtime::Handle::try_current().map_err(invalid)?;
        state.started = true;
        // The guard exists before spawning, including an unpolled task's Drop.
        let guard = Guard {
            owner: self.clone(),
            complete: false,
        };
        drop(state); // Preguard Drop may lock progress even for an unpolled task.
        self.worker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .not_started = false;
        let worker = runtime.spawn(async move {
            let mut guard = guard;
            let result = guard
                .owner
                .tenure
                .target
                .request_native_frontier(&guard.owner.tenure)
                .await;
            // Retain the genuine late Native resource before classifying any
            // observer result. No public constructor or boolean supplies it.
            guard
                .owner
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .result = Some(result.map_err(|error| Failure {
                kind: error.kind(),
                message: error.to_string(),
            }));
            guard.complete = true;
            guard.owner.changed.notify_waiters();
        });
        self.worker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .task = Some(worker);
        Ok(())
    }
    /// Actual delivery shutdown also joins this SAME task. A cancelled join
    /// restores its handle before releasing serial admission.
    pub(crate) async fn drain(self: &Arc<Self>) -> io::Result<()> {
        let _serial = self.join.acquire().await.map_err(invalid)?;
        let task = {
            let mut state = self.worker.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(joined) = state.joined {
                return joined
                    .map_err(|kind| io::Error::new(kind, "configured Native begin worker failed"));
            }
            // This is definite local registration refusal before spawn, never
            // a claim about an SDK/Native operation or a dropped task.
            if state.not_started {
                return Ok(());
            }
            state
                .task
                .take()
                .ok_or_else(|| invalid("configured Native begin lost actual task custody"))?
        };
        let mut lease = JoinLease {
            owner: self.clone(),
            task: Some(task),
        };
        let joined = lease
            .task
            .as_mut()
            .ok_or_else(|| invalid("configured begin join custody missing"))?
            .await
            .map_err(|_| io::ErrorKind::Other);
        let completed = lease.task.take();
        self.worker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .joined = Some(joined);
        drop(completed);
        joined.map_err(|kind| io::Error::new(kind, "configured Native begin worker failed"))
    }
    pub(crate) async fn observe(
        self: &Arc<Self>,
        session: &AppServerSession,
    ) -> io::Result<Arc<Pending>> {
        if !session
            .retained_frontend()
            .is_some_and(|actual| self.tenure.same_delivery(actual))
        {
            return Err(invalid("configured begin changed actual session"));
        }
        let observed = loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let result = self
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .result
                .clone();
            if let Some(result) = result {
                break result;
            }
            changed.await;
        };
        self.drain().await?;
        let native = observed.map_err(|failure| failure.error())?;
        let _install = self.install.acquire().await.map_err(invalid)?;
        let installed = self
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending
            .clone();
        if let Some(actual) = installed {
            if !actual.current_on(session) {
                return Err(invalid("configured begin lost its canonical pending"));
            }
            return Ok(actual);
        }
        // No progress lock is held while canonical delivery ownership installs.
        let pending = Pending::new(session, native)?;
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending = Some(pending.clone());
        Ok(pending)
    }
}
struct Guard {
    owner: Arc<Begin>,
    complete: bool,
}
impl Drop for Guard {
    fn drop(&mut self) {
        if self.complete {
            return;
        }
        let mut state = self
            .owner
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if state.result.is_none() {
            state.result = Some(Err(Failure {
                kind: io::ErrorKind::Other,
                message: "actual configured Native begin is unobserved".into(),
            }));
        }
        drop(state);
        self.owner.changed.notify_waiters();
    }
}

struct JoinLease {
    owner: Arc<Begin>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl Drop for JoinLease {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            let mut state = self
                .owner
                .worker
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            // The SAME join semaphore excludes another taker/restorer.
            debug_assert!(state.task.is_none() && state.joined.is_none());
            state.task = Some(task);
        }
    }
}

#[cfg(test)]
#[path = "retained_startup_configured_begin_tests.rs"]
mod tests;
