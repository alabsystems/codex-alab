// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One retained job owns preparation, real blocking Git, and attachment publication.
use super::*;
use codex_alab_task_worktree::AllocationCapture;
use codex_alab_task_worktree::COMPLETED_LIFETIME_SECS;
use codex_alab_task_worktree::MAX_OPERATIONS;
use codex_alab_task_worktree::Operation;
use std::time::Duration;
use std::time::Instant;

#[derive(Clone)]
enum JobObservation {
    Complete(Result<Arc<Value>, Arc<String>>),
    Unobserved,
}
enum WorkerFailure {
    Source(String),
    BlockingLost,
}
impl From<String> for WorkerFailure {
    fn from(error: String) -> Self {
        Self::Source(error)
    }
}
pub(super) struct AcceptedWorktree {
    parent_id: ThreadId,
    parent: Arc<CodexThread>,
    started: Instant,
    capture: Arc<AllocationCapture>,
    ready: tokio::sync::watch::Sender<Option<Result<(), Arc<String>>>>,
    completion: tokio::sync::watch::Sender<Option<JobObservation>>,
    #[cfg(test)]
    observed_requests: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    worker: std::sync::OnceLock<tokio::task::AbortHandle>,
}
impl AcceptedWorktree {
    fn finish(&self, value: JobObservation) {
        let mut value = Some(value);
        self.completion.send_if_modified(|stored| {
            if stored.is_some() {
                return false;
            }
            *stored = value.take();
            true
        });
    }
    fn ready(&self, result: Result<(), Arc<String>>) {
        let mut result = Some(result);
        self.ready.send_if_modified(|stored| {
            if stored.is_some() {
                return false;
            }
            *stored = result.take();
            true
        });
    }
    fn expired(&self) -> bool {
        matches!(
            &*self.completion.borrow(),
            Some(JobObservation::Complete(_))
        ) && self.started.elapsed() >= Duration::from_secs(COMPLETED_LIFETIME_SECS)
    }
    #[cfg(test)]
    pub(super) fn requests_for_tests(&self) -> usize {
        self.observed_requests
            .load(std::sync::atomic::Ordering::Acquire)
    }
    #[cfg(test)]
    pub(super) fn abort_worker_for_tests(&self) -> Result<(), String> {
        let worker = self
            .worker
            .get()
            .ok_or("actual accepted worker unavailable")?;
        worker.abort();
        Ok(())
    }
    #[cfg(test)]
    pub(super) async fn wait_for_tests(&self) {
        let mut completion = self.completion.subscribe();
        loop {
            if completion.borrow_and_update().is_some() {
                return;
            }
            if completion.changed().await.is_err() {
                return;
            }
        }
    }
    #[cfg(test)]
    pub(super) fn created_for_tests(
        &self,
    ) -> Option<Arc<codex_alab_task_worktree::ManagedCheckout>> {
        self.capture.created()
    }
}
struct WorktreeGuard {
    operation: Arc<AcceptedWorktree>,
    armed: bool,
}
impl WorktreeGuard {
    fn finish(mut self, value: JobObservation) {
        self.operation.finish(value);
        self.armed = false;
    }
}
impl Drop for WorktreeGuard {
    fn drop(&mut self) {
        if self.armed {
            self.operation.ready(Err(Arc::new("Worktree creation outcome is uncertain. Inspect this task's attachments and managed worktrees before retrying.".into())));
            self.operation.finish(JobObservation::Unobserved);
        }
    }
}
fn response(result: Result<Value, String>) -> rmcp::model::CallToolResponse {
    let rendered = codex_alab_task_worktree::render(result);
    let content = vec![ContentBlock::text(rendered.text)];
    if rendered.success {
        CallToolResult::success(content).into()
    } else {
        CallToolResult::error(content).into()
    }
}

impl HostControl {
    pub(super) async fn call_worktree(
        self: &Arc<Self>,
        parent_id: ThreadId,
        parent: Arc<CodexThread>,
        tool: &str,
        arguments: Value,
        observer: CancellationToken,
    ) -> Result<rmcp::model::CallToolResponse, McpError> {
        let Some(worktrees) = self.worktrees.get() else {
            return Err(McpError::invalid_params(
                "Worktree tools are unavailable on this connection",
                None,
            ));
        };
        if !worktrees.available().await {
            return Err(McpError::invalid_params(
                "Worktree tools are unavailable on this connection",
                None,
            ));
        }
        let operation = match codex_alab_task_worktree::parse(tool, arguments) {
            Ok(operation) => operation,
            Err(error) => return Ok(response(Err(error))),
        };
        match operation {
            Operation::List(cursor) => {
                let result = tokio::select! {
                    biased;
                    () = observer.cancelled() => Err("worktree list observation cancelled".into()),
                    () = self.incoming.withdrawn() => Err("worktree list owner withdrew".into()),
                    () = parent.wait_until_terminated() => Err("issuing task runtime terminated".into()),
                    result = worktrees.list(parent_id, &parent, cursor) => result,
                };
                Ok(response(result))
            }
            Operation::Status(id) => {
                worktrees
                    .check(parent_id, &parent)
                    .await
                    .map_err(|_| McpError::internal_error("actual bound task withdrew", None))?;
                let operation = Uuid::parse_str(&id)
                    .ok()
                    .and_then(|key| {
                        self.registry
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .worktree_jobs
                            .get(&key)
                            .cloned()
                    })
                    .filter(|operation| {
                        operation.parent_id == parent_id && Arc::ptr_eq(&operation.parent, &parent)
                    });
                let Some(operation) = operation else {
                    return Ok(response(Err("Unknown operation. Inspect this task's worktree attachments before creating another checkout.".into())));
                };
                let value = operation.completion.borrow().clone();
                Ok(response(match value {
                    None => Ok(codex_alab_task_worktree::pending(&id, false)),
                    Some(JobObservation::Complete(result)) => result.map(|value| value.as_ref().clone()).map_err(|error| error.as_ref().clone()),
                    Some(JobObservation::Unobserved) => Err("Worktree creation outcome is uncertain. Inspect this task's attachments and managed worktrees before retrying.".into()),
                }))
            }
            Operation::Create(arguments) => {
                let mut retired = Vec::new();
                let (key, operation) = {
                    let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
                    if registry.closed
                        || self.incoming.is_stopped()
                        || self.shutdown.is_cancelled()
                        || parent.has_shutdown_status()
                    {
                        return Err(McpError::internal_error(
                            "task host withdrew before worktree acceptance",
                            None,
                        ));
                    }
                    let expired = registry
                        .worktree_jobs
                        .iter()
                        .filter_map(|(key, job)| job.expired().then_some(*key))
                        .collect::<Vec<_>>();
                    for key in expired {
                        if let Some(job) = registry.worktree_jobs.remove(&key) {
                            retired.push(job);
                        }
                    }
                    if let Some((key, job)) = registry.worktree_jobs.iter().find(|(_, job)| {
                        job.parent_id == parent_id
                            && Arc::ptr_eq(&job.parent, &parent)
                            && job.completion.borrow().is_none()
                    }) {
                        (*key, job.clone())
                    } else {
                        if registry.worktree_jobs.len() >= MAX_OPERATIONS {
                            return Ok(response(Err(
                                "Too many worktree operations; inspect existing checkouts first"
                                    .into(),
                            )));
                        }
                        if registry
                            .operations
                            .len()
                            .saturating_add(registry.management.len())
                            .saturating_add(registry.sends.len())
                            .saturating_add(registry.accepted_forks())
                            .saturating_add(registry.worktree_jobs.len())
                            >= MAX_ACCEPTED
                        {
                            return Err(McpError::internal_error(
                                "retained operation capacity exhausted",
                                None,
                            ));
                        }
                        let key = Uuid::new_v4();
                        let (ready, _) = tokio::sync::watch::channel(None);
                        let (completion, _) = tokio::sync::watch::channel(None);
                        let operation = Arc::new(AcceptedWorktree {
                            parent_id,
                            parent: parent.clone(),
                            started: Instant::now(),
                            capture: Arc::new(AllocationCapture::default()),
                            ready,
                            completion,
                            #[cfg(test)]
                            observed_requests: std::sync::atomic::AtomicUsize::new(0),
                            #[cfg(test)]
                            worker: std::sync::OnceLock::new(),
                        });
                        let guard = WorktreeGuard {
                            operation: operation.clone(),
                            armed: true,
                        };
                        registry.worktree_jobs.insert(key, operation.clone());
                        let owner = self.clone();
                        let accepted = operation.clone();
                        let worker = self.operations.spawn(async move {
                            let mut future = Box::pin(AssertUnwindSafe(async {
                                let worktrees = owner.worktrees.get().ok_or_else(|| WorkerFailure::Source("worktree source unavailable".into()))?;
                                let allocation = worktrees.prepare(parent_id, &accepted.parent, arguments).await?;
                                #[cfg(test)] owner.worktree_before_allocation_for_tests().await;
                                worktrees.check(parent_id, &accepted.parent).await?;
                                let capture = accepted.capture.clone();
                                #[cfg(test)]
                                let blocking_gate = owner.test_worktree_blocking_gate.lock().unwrap_or_else(PoisonError::into_inner).clone();
                                let task = {
                                    let registry = owner.registry.lock().unwrap_or_else(PoisonError::into_inner);
                                    if registry.closed || owner.incoming.is_stopped() || owner.shutdown.is_cancelled() || accepted.parent.has_shutdown_status() {
                                        return Err(WorkerFailure::Source("retained worktree owner withdrew before allocation".into()));
                                    }
                                    // Track actual blocking Git independently in the SAME host
                                    // tracker, so outer worker destruction cannot detach it.
                                    owner.operations.spawn_blocking(move || {
                                        #[cfg(test)]
                                        {
                                            if let Some(gate) = blocking_gate { gate.block(); }
                                            allocation.run(&capture)
                                        }
                                        #[cfg(not(test))]
                                        allocation.run(&capture)
                                    })
                                };
                                accepted.ready(Ok(()));
                                let allocated = task.await.map_err(|_| WorkerFailure::BlockingLost)??;
                                Ok(worktrees.register(parent_id, allocated).await)
                            }).catch_unwind());
                            let result = future.as_mut().await;
                            let destroyed = catch_unwind(AssertUnwindSafe(|| drop(future)));
                            match (result, destroyed) {
                                (Ok(Ok(value)), Ok(())) => guard.finish(JobObservation::Complete(Ok(Arc::new(value)))),
                                (Ok(Err(WorkerFailure::Source(error))), Ok(())) => {
                                    accepted.ready(Err(Arc::new(error.clone())));
                                    let before_allocation = accepted.capture.created().is_none() && accepted.ready.borrow().as_ref().is_some_and(Result::is_err);
                                    guard.finish(JobObservation::Complete(Err(Arc::new(error))));
                                    if before_allocation {
                                        let retired = owner.registry.lock().unwrap_or_else(PoisonError::into_inner).worktree_jobs.remove(&key);
                                        drop(retired);
                                    }
                                }
                                _ => drop(guard),
                            }
                        });
                        #[cfg(test)]
                        let _ = operation.worker.set(worker.abort_handle());
                        drop(worker);
                        (key, operation)
                    }
                };
                drop(retired);
                #[cfg(test)]
                operation
                    .observed_requests
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                let mut ready = operation.ready.subscribe();
                loop {
                    if let Some(value) = ready.borrow_and_update().clone() {
                        return Ok(response(
                            value
                                .map(|()| codex_alab_task_worktree::pending(&key.to_string(), true))
                                .map_err(|error| error.as_ref().clone()),
                        ));
                    }
                    tokio::select! {
                        biased;
                        () = observer.cancelled() => return Err(McpError::internal_error("worktree observation cancelled; job remains owned; do not replay", None)),
                        () = self.incoming.withdrawn() => return Err(McpError::internal_error("worktree observation withdrew; job remains owned; do not replay", None)),
                        changed = ready.changed() => { changed.map_err(|_| McpError::internal_error("worktree observation unavailable; do not replay", None))?; }
                    }
                }
            }
        }
    }
    pub(super) fn has_unobserved_worktree(&self) -> bool {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .worktree_jobs
            .values()
            .any(|job| !matches!(&*job.completion.borrow(), Some(JobObservation::Complete(_))))
    }
    #[cfg(test)]
    pub(super) fn accepted_worktrees_for_tests(&self) -> Vec<(Uuid, Arc<AcceptedWorktree>)> {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .worktree_jobs
            .iter()
            .map(|(key, job)| (*key, job.clone()))
            .collect()
    }
    #[cfg(test)]
    async fn worktree_before_allocation_for_tests(&self) {
        let gate = self
            .test_worktree_gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(gate) = gate {
            gate.entered.notify_one();
            gate.release.notified().await;
        }
    }
    #[cfg(test)]
    pub(super) fn install_worktree_blocking_gate_for_tests(&self) -> Arc<TestWorktreeBlockingGate> {
        let gate = Arc::new(TestWorktreeBlockingGate {
            entered: tokio::sync::Notify::new(),
            released: std::sync::Mutex::new(false),
            release: std::sync::Condvar::new(),
        });
        *self
            .test_worktree_blocking_gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(gate.clone());
        gate
    }
    #[cfg(test)]
    pub(super) fn install_worktree_gate_for_tests(&self) -> Arc<TestWorktreeGate> {
        let gate = Arc::new(TestWorktreeGate {
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        });
        *self
            .test_worktree_gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(gate.clone());
        gate
    }
}
#[cfg(test)]
pub(super) struct TestWorktreeGate {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
#[cfg(test)]
impl TestWorktreeGate {
    pub(super) async fn wait_entered(&self) {
        self.entered.notified().await;
    }
    pub(super) fn release(&self) {
        self.release.notify_one();
    }
}

#[cfg(test)]
pub(super) struct TestWorktreeBlockingGate {
    entered: tokio::sync::Notify,
    released: std::sync::Mutex<bool>,
    release: std::sync::Condvar,
}
#[cfg(test)]
impl TestWorktreeBlockingGate {
    fn block(&self) {
        self.entered.notify_one();
        let mut released = self.released.lock().unwrap_or_else(PoisonError::into_inner);
        while !*released {
            released = self
                .release
                .wait(released)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
    pub(super) async fn wait_entered(&self) {
        self.entered.notified().await;
    }
    pub(super) fn release(&self) {
        *self.released.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.release.notify_all();
    }
}
