// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Concrete owned title/archive invocations. The existing host TaskTracker
//! owns every accepted whole helper future independently of HTTP observation.
use super::*;
use crate::request_processors::retained_task_read::management::ManagementRequest;

#[derive(Clone)]
enum ManagementObservation {
    Complete(Result<Arc<String>, Arc<String>>),
    Unobserved,
}

pub(super) struct AcceptedManagement {
    request: ManagementRequest,
    completion: tokio::sync::watch::Sender<Option<ManagementObservation>>,
}
impl AcceptedManagement {
    fn finish(&self, observation: ManagementObservation) {
        let mut observation = Some(observation);
        self.completion.send_if_modified(|stored| {
            if stored.is_some() {
                return false;
            }
            *stored = observation.take();
            true
        });
    }
}

/// Construct before the tracked worker's first poll. Unknown is terminal
/// observation only, never proof that the mutation did not occur.
struct ManagementGuard {
    operation: Arc<AcceptedManagement>,
    armed: bool,
}
impl ManagementGuard {
    fn finish(mut self, observation: ManagementObservation) {
        self.operation.finish(observation);
        self.armed = false;
    }
}
impl Drop for ManagementGuard {
    fn drop(&mut self) {
        if self.armed {
            self.operation.finish(ManagementObservation::Unobserved);
        }
    }
}

impl HostControl {
    pub(super) async fn accept_management(
        self: &Arc<Self>,
        request: ManagementRequest,
        observer: CancellationToken,
    ) -> Result<rmcp::model::CallToolResponse, McpError> {
        let (completion, _) = tokio::sync::watch::channel(None);
        let accepted = Arc::new(AcceptedManagement {
            request,
            completion,
        });
        let mut receive = accepted.completion.subscribe();
        let key = Uuid::new_v4();
        {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            if registry.closed || self.incoming.is_stopped() {
                return Err(McpError::internal_error(
                    "task host withdrew before management acceptance",
                    None,
                ));
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
            let guard = ManagementGuard {
                operation: accepted.clone(),
                armed: true,
            };
            registry.management.insert(key, accepted.clone());
            let owner = self.clone();
            let operation = accepted.clone();
            // Same admission mutex as stop_admission: once inserted, this
            // actual host owns the worker before a caller can lose its observer.
            self.operations.spawn(async move {
                let observed =
                    observe_management_future(owner.management.run(&operation.request)).await;
                let joined = matches!(&observed, ManagementObservation::Complete(_));
                guard.finish(observed);
                if joined {
                    owner.retire_management(key, &operation);
                }
            });
        }
        loop {
            let observed = receive.borrow_and_update().clone();
            if let Some(observed) = observed {
                return match observed {
                    ManagementObservation::Complete(Ok(text)) => {
                        Ok(
                            CallToolResult::success(vec![ContentBlock::text(
                                text.as_ref().clone(),
                            )])
                            .into(),
                        )
                    }
                    ManagementObservation::Complete(Err(error)) => {
                        Ok(CallToolResult::error(vec![ContentBlock::text(
                            retained_task_read::policy::truncate(
                                &error,
                                retained_task_read::policy::MAX_ERROR_CHARS,
                            ),
                        )])
                        .into())
                    }
                    ManagementObservation::Unobserved => Err(McpError::internal_error(
                        "task management outcome unobserved; do not replay mutation",
                        None,
                    )),
                };
            }
            tokio::select! {
                biased;
                ()=observer.cancelled()=>return Err(McpError::internal_error("management observation cancelled; accepted mutation remains owned; do not replay",None)),
                ()=self.incoming.withdrawn()=>return Err(McpError::internal_error("management observation withdrew; accepted mutation remains owned; do not replay",None)),
                changed=receive.changed()=>{changed.map_err(|_|McpError::internal_error("management observation unavailable; do not replay mutation",None))?;}
            }
        }
    }

    fn retire_management(&self, key: Uuid, operation: &Arc<AcceptedManagement>) {
        let retired = {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            if registry
                .management
                .get(&key)
                .is_some_and(|actual| Arc::ptr_eq(actual, operation))
            {
                registry.management.remove(&key)
            } else {
                None
            }
        };
        // Request owns the actual issuing runtime. Drop it after releasing the
        // registry mutex; no destructor is permitted to re-enter this lock.
        drop(retired);
    }

    pub(super) fn has_unobserved_management(&self) -> bool {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .management
            .values()
            .any(|operation| {
                !matches!(
                    &*operation.completion.borrow(),
                    Some(ManagementObservation::Complete(_))
                )
            })
    }
}

// One local observation helper, shared by the actual worker and fault controls.
// It neither submits work nor constructs ownership, credentials or client IDs.
async fn observe_management_future<F>(work: F) -> ManagementObservation
where
    F: std::future::Future<Output = Result<String, String>>,
{
    let mut future = Box::pin(AssertUnwindSafe(work).catch_unwind());
    let polled = future.as_mut().await;
    let destroyed = catch_unwind(AssertUnwindSafe(|| drop(future)));
    match (polled, destroyed) {
        (Ok(result), Ok(())) => {
            ManagementObservation::Complete(result.map(Arc::new).map_err(Arc::new))
        }
        _ => ManagementObservation::Unobserved,
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use anyhow::Result;
    use anyhow::ensure;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use std::task::Context;
    use std::task::Poll;

    pub(crate) enum GuardFailure {
        BeforePoll,
        PollPanic,
        DestructionPanic,
    }
    struct DestructionPanic;
    impl Future for DestructionPanic {
        type Output = std::result::Result<String, String>;
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
            Poll::Ready(Ok(
                "controlled result must not survive destruction panic".into()
            ))
        }
    }
    impl Drop for DestructionPanic {
        fn drop(&mut self) {
            panic!("controlled management future destruction");
        }
    }
    async fn poll_panic() -> std::result::Result<String, String> {
        panic!("controlled management worker panic");
    }
    async fn join_control(
        mut worker: tokio::task::JoinHandle<()>,
    ) -> Result<std::result::Result<(), tokio::task::JoinError>> {
        match tokio::time::timeout(std::time::Duration::from_secs(30), &mut worker).await {
            Ok(joined) => Ok(joined),
            Err(_) => {
                worker.abort();
                let _ = worker.await;
                anyhow::bail!("management guard control did not join");
            }
        }
    }

    pub(crate) async fn inject_guard_failure(
        host: &Arc<HostControl>,
        request: ManagementRequest,
        failure: GuardFailure,
    ) -> Result<Uuid> {
        let (completion, _) = tokio::sync::watch::channel(None);
        let operation = Arc::new(AcceptedManagement {
            request,
            completion,
        });
        let key = Uuid::new_v4();
        {
            host.registry
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .management
                .insert(key, operation.clone());
        }
        let guard = ManagementGuard {
            operation: operation.clone(),
            armed: true,
        };
        match failure {
            GuardFailure::BeforePoll => {
                let entered = Arc::new(AtomicBool::new(false));
                let observed = entered.clone();
                let worker = host.operations.spawn(async move {
                    let outcome = observe_management_future(async move {
                        observed.store(true, Ordering::SeqCst);
                        std::future::pending::<std::result::Result<String, String>>().await
                    })
                    .await;
                    guard.finish(outcome);
                });
                // current_thread + no await between spawn/abort; verify the
                // control precondition, without treating it as no-effects proof.
                worker.abort();
                let joined = join_control(worker).await?;
                ensure!(
                    joined
                        .as_ref()
                        .is_err_and(tokio::task::JoinError::is_cancelled),
                    "unpolled guard worker did not cancel"
                );
                ensure!(
                    !entered.load(Ordering::SeqCst),
                    "unpolled control actually entered its future"
                );
            }
            GuardFailure::PollPanic => {
                let worker = host.operations.spawn(async move {
                    let outcome = observe_management_future(poll_panic()).await;
                    guard.finish(outcome);
                });
                ensure!(
                    join_control(worker).await?.is_ok(),
                    "production observation helper did not contain polling panic"
                );
            }
            GuardFailure::DestructionPanic => {
                let worker = host.operations.spawn(async move {
                    let outcome = observe_management_future(DestructionPanic).await;
                    guard.finish(outcome);
                });
                ensure!(
                    join_control(worker).await?.is_ok(),
                    "production observation helper did not contain destruction panic"
                );
            }
        }
        Ok(key)
    }

    pub(crate) fn unknown_record_retained(
        host: &HostControl,
        key: Uuid,
        parent_id: ThreadId,
        parent: &Arc<CodexThread>,
    ) -> bool {
        let operation = {
            host.registry
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .management
                .get(&key)
                .cloned()
        };
        operation.is_some_and(|operation| {
            operation.request.parent_id == parent_id
                && Arc::ptr_eq(&operation.request.parent, parent)
                && matches!(
                    &*operation.completion.borrow(),
                    Some(ManagementObservation::Unobserved)
                )
        })
    }
}
