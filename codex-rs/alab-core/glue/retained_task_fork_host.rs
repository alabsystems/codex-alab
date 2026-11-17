// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One actual accepted fork worker; repeated observation never resubmits it.
use super::*;
use crate::request_processors::retained_task_fork::ForkArguments;
use crate::request_processors::retained_task_fork::ForkRequest;
use crate::request_processors::retained_task_fork::ForkScope;

#[cfg(test)]
pub(super) type AcceptedForkObservation = (
    String,
    String,
    Option<Option<String>>,
    Option<Arc<codex_app_server_protocol::ThreadForkResponse>>,
    Option<Arc<CodexThread>>,
);

#[derive(Clone)]
enum ForkObservation {
    Complete {
        result: Result<Arc<String>, Arc<String>>,
        custody_observed: bool,
    },
    Unobserved,
}
pub(super) struct AcceptedFork {
    origin: Arc<origin::ForkOrigin>,
    scope: ForkScope,
    completion: tokio::sync::watch::Sender<Option<ForkObservation>>,
    #[cfg(test)]
    observer: CancellationToken,
}
impl AcceptedFork {
    fn finish(&self, observation: ForkObservation) {
        let mut observation = Some(observation);
        self.completion.send_if_modified(|stored| {
            if stored.is_some() {
                return false;
            }
            *stored = observation.take();
            true
        });
    }
    pub(super) fn definite_for_retirement(&self) -> bool {
        matches!(
            &*self.completion.borrow(),
            Some(ForkObservation::Complete {
                custody_observed: true,
                ..
            })
        )
    }
    #[cfg(test)]
    pub(super) fn observed_for_tests(&self) -> AcceptedForkObservation {
        let (cutoff, response, created) = self.scope.observed_for_tests();
        (
            self.origin.turn.clone(),
            self.origin.call.clone(),
            cutoff,
            response,
            created,
        )
    }
    #[cfg(test)]
    pub(super) fn cancel_observer_for_tests(&self) {
        self.observer.cancel();
    }
    #[cfg(test)]
    pub(super) async fn wait_result_for_tests(
        &self,
    ) -> Result<Arc<codex_app_server_protocol::ThreadForkResponse>, String> {
        let mut receive = self.completion.subscribe();
        loop {
            let value = receive.borrow_and_update().clone();
            match value {
                Some(ForkObservation::Complete { result: Ok(_), .. }) => {
                    return self
                        .scope
                        .observed_for_tests()
                        .1
                        .ok_or_else(|| "actual fork completed without typed response".into());
                }
                Some(ForkObservation::Complete {
                    result: Err(error), ..
                }) => return Err(error.as_ref().clone()),
                Some(ForkObservation::Unobserved) => {
                    return Err("actual fork outcome unobserved".into());
                }
                None => {
                    receive
                        .changed()
                        .await
                        .map_err(|_| "actual fork observer closed".to_string())?;
                }
            }
        }
    }
}

struct ForkGuard {
    operation: Arc<AcceptedFork>,
    armed: bool,
}
impl ForkGuard {
    fn finish(mut self, value: ForkObservation) {
        self.operation.finish(value);
        self.armed = false;
    }
}
impl Drop for ForkGuard {
    fn drop(&mut self) {
        if self.armed {
            self.operation.finish(ForkObservation::Unobserved);
        }
    }
}

impl HostControl {
    pub(super) async fn accept_fork(
        self: &Arc<Self>,
        binding: &Arc<ThreadBinding>,
        issuer: (ThreadId, Arc<CodexThread>, ConstructionCapture),
        arguments: ForkArguments,
        claim: Uuid,
        observer: CancellationToken,
    ) -> Result<rmcp::model::CallToolResponse, McpError> {
        let (parent_id, parent, capture) = issuer;
        self.retire_expired_fork_calls();
        let operation = {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            origin::validate_bound(self, &registry, binding, parent_id, &parent)
                .map_err(|_| McpError::internal_error("fork issuing owner withdrew", None))?;
            let entry = registry.fork_calls.get(&claim).ok_or_else(|| {
                McpError::invalid_params("genuine fork invocation is unavailable", None)
            })?;
            if entry.pending_expired(std::time::Instant::now()) {
                return Err(McpError::invalid_params(
                    "fork invocation expired before acceptance",
                    None,
                ));
            }
            if entry.origin.binding_id != binding.key
                || entry.origin.parent_id != parent_id
                || !Arc::ptr_eq(&entry.origin.parent, &parent)
                || !entry
                    .origin
                    .binding
                    .upgrade()
                    .is_some_and(|actual| Arc::ptr_eq(&actual, binding))
            {
                return Err(McpError::invalid_params(
                    "fork invocation belongs to another runtime",
                    None,
                ));
            }
            if let Some((frozen, operation)) = &entry.accepted {
                if frozen != &arguments {
                    return Err(McpError::invalid_params(
                        "fork invocation arguments changed",
                        None,
                    ));
                }
                operation.clone()
            } else {
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
                let origin = entry.origin.clone();
                let scope = ForkScope::new(
                    ForkRequest {
                        parent_id,
                        parent,
                        issuing_turn: origin.turn.clone(),
                        arguments: arguments.clone(),
                        capture,
                        service_config: binding.config.clone(),
                        context: OperationContext {
                            span: tracing::info_span!("retained.task.fork", thread_id = %parent_id),
                            parent_trace: None,
                            cancellation: self.shutdown.child_token(),
                        },
                    },
                    self.incoming.clone(),
                    Arc::downgrade(self),
                );
                let (completion, _) = tokio::sync::watch::channel(None);
                let operation = Arc::new(AcceptedFork {
                    origin,
                    scope,
                    completion,
                    #[cfg(test)]
                    observer: observer.clone(),
                });
                let guard = ForkGuard {
                    operation: operation.clone(),
                    armed: true,
                };
                registry
                    .fork_calls
                    .get_mut(&claim)
                    .ok_or_else(|| McpError::internal_error("fork invocation vanished", None))?
                    .accepted = Some((arguments, operation.clone()));
                let accepted = operation.clone();
                let owner = self.clone();
                // Same admission mutex as shutdown, and guard before spawn.
                self.operations.spawn(async move {
                    let mut future =
                        Box::pin(AssertUnwindSafe(owner.forks.run(&accepted.scope)).catch_unwind());
                    let result = future.as_mut().await;
                    let destroyed = catch_unwind(AssertUnwindSafe(|| drop(future)));
                    let observation = match (result, destroyed) {
                        (Ok(result), Ok(())) => {
                            let custody_observed = if result.is_ok() {
                                true
                            } else {
                                accepted.scope.finish_error().await
                            };
                            if custody_observed {
                                accepted.scope.release_definite_custody();
                            }
                            ForkObservation::Complete {
                                result: result.map(Arc::new).map_err(Arc::new),
                                custody_observed,
                            }
                        }
                        _ => ForkObservation::Unobserved,
                    };
                    guard.finish(observation);
                    // Retain terminal dedup receipts while this actual binding
                    // exists; never forget a result and run the call again.
                    let owner_registered = owner
                        .registry
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .bindings
                        .contains_key(&accepted.origin.binding_id);
                    if !owner_registered {
                        owner.retire_fork_owner(accepted.origin.binding_id);
                    }
                });
                operation
            }
        };
        let mut receive = operation.completion.subscribe();
        loop {
            let value = receive.borrow_and_update().clone();
            if let Some(value) = value {
                return match value {
                    ForkObservation::Complete {
                        result: Ok(text), ..
                    } => Ok(CallToolResult::success(vec![ContentBlock::text(
                        text.as_ref().clone(),
                    )])
                    .into()),
                    ForkObservation::Complete {
                        result: Err(error), ..
                    } => Ok(CallToolResult::error(vec![ContentBlock::text(
                        retained_task_read::policy::truncate(
                            &error,
                            retained_task_read::policy::MAX_ERROR_CHARS,
                        ),
                    )])
                    .into()),
                    ForkObservation::Unobserved => Err(McpError::internal_error(
                        "fork outcome unobserved; do not replay",
                        None,
                    )),
                };
            }
            tokio::select! {
                biased;
                () = observer.cancelled() => return Err(McpError::internal_error("fork observation cancelled; work remains owned; do not replay", None)),
                () = self.incoming.withdrawn() => return Err(McpError::internal_error("fork observation withdrew; work remains owned; do not replay", None)),
                changed = receive.changed() => { changed.map_err(|_| McpError::internal_error("fork observation unavailable; do not replay", None))?; }
            }
        }
    }
    pub(super) fn has_unobserved_fork(&self) -> bool {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .fork_calls
            .values()
            .filter_map(|entry| entry.accepted.as_ref())
            .any(|(_, operation)| !operation.definite_for_retirement())
    }
    pub(super) async fn finish_forks_after_threads(&self) {
        let operations = self
            .registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .fork_calls
            .values()
            .filter_map(|entry| {
                entry
                    .accepted
                    .as_ref()
                    .map(|(_, operation)| operation.clone())
            })
            .collect::<Vec<_>>();
        for operation in operations {
            // Only a normally returned/destructed worker can retry exact-child
            // Source cleanup. Never infer a missing outcome from termination.
            let retry = matches!(
                &*operation.completion.borrow(),
                Some(ForkObservation::Complete {
                    result: Err(_),
                    custody_observed: false,
                })
            );
            if retry && operation.scope.finish_error().await {
                operation.scope.release_definite_custody();
                operation.completion.send_if_modified(|stored| {
                    if let Some(ForkObservation::Complete {
                        custody_observed, ..
                    }) = stored
                    {
                        *custody_observed = true;
                        return true;
                    }
                    false
                });
            }
        }
    }
    #[cfg(test)]
    pub(super) fn accepted_forks_for_tests(&self) -> Vec<Arc<AcceptedFork>> {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .fork_calls
            .values()
            .filter_map(|entry| {
                entry
                    .accepted
                    .as_ref()
                    .map(|(_, operation)| operation.clone())
            })
            .collect()
    }
    #[cfg(test)]
    pub(crate) async fn fork_before_recipe_for_tests(&self) {
        let gate = self
            .test_fork_gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(gate) = gate {
            gate.entered.notify_one();
            gate.release.notified().await;
        }
    }
    #[cfg(test)]
    pub(super) fn install_fork_gate_for_tests(&self) -> Arc<TestForkGate> {
        let gate = Arc::new(TestForkGate {
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        });
        *self
            .test_fork_gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(gate.clone());
        gate
    }
}
impl Registry {
    pub(super) fn accepted_forks(&self) -> usize {
        self.fork_calls
            .values()
            .filter(|entry| entry.accepted.is_some())
            .count()
    }
}

#[cfg(test)]
pub(super) struct TestForkGate {
    pub(super) entered: tokio::sync::Notify,
    pub(super) release: tokio::sync::Notify,
}
#[cfg(test)]
impl TestForkGate {
    pub(super) async fn wait_entered(&self) {
        self.entered.notified().await;
    }
    pub(super) fn release(&self) {
        self.release.notify_one();
    }
}
