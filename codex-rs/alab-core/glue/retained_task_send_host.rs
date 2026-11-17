// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The real host owns one complete accepted SEND independently of HTTP
//! observation. Resume/turn custody lives in Work outside its polled future.
use super::*;
use crate::request_processors::retained_task_send::SendRequest;
use crate::request_processors::retained_task_send::SendWork;

#[derive(Clone)]
enum SendObservation {
    Complete(Result<Arc<String>, Arc<String>>),
    Unobserved,
}

pub(super) struct AcceptedSend {
    work: SendWork,
    completion: tokio::sync::watch::Sender<Option<SendObservation>>,
}
impl AcceptedSend {
    #[cfg(test)]
    pub(super) fn accepted_turn_for_tests(
        &self,
    ) -> Option<Arc<codex_app_server_protocol::TurnStartResponse>> {
        self.work.accepted_turn_for_tests()
    }
    fn finish(&self, value: SendObservation) {
        let mut value = Some(value);
        self.completion.send_if_modified(|stored| {
            if stored.is_some() {
                return false;
            }
            *stored = value.take();
            true
        });
    }
}

struct SendGuard {
    operation: Arc<AcceptedSend>,
    armed: bool,
}
impl SendGuard {
    fn finish(mut self, value: SendObservation) {
        self.operation.finish(value);
        self.armed = false;
    }
}
impl Drop for SendGuard {
    fn drop(&mut self) {
        if self.armed {
            self.operation.finish(SendObservation::Unobserved);
        }
    }
}

impl HostControl {
    pub(super) async fn accept_send(
        self: &Arc<Self>,
        request: SendRequest,
        capture: ConstructionCapture,
        observer: CancellationToken,
    ) -> Result<rmcp::model::CallToolResponse, McpError> {
        let work = SendWork::new(
            request,
            self.incoming.clone(),
            Arc::downgrade(self),
            capture,
        );
        let (completion, _) = tokio::sync::watch::channel(None);
        let operation = Arc::new(AcceptedSend { work, completion });
        let mut receive = operation.completion.subscribe();
        let key = Uuid::new_v4();
        {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            if registry.closed || self.incoming.is_stopped() {
                return Err(McpError::internal_error(
                    "task host withdrew before SEND acceptance",
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
            let guard = SendGuard {
                operation: operation.clone(),
                armed: true,
            };
            registry.sends.insert(key, operation.clone());
            let accepted = operation.clone();
            let owner = self.clone();
            // Same admission lock as stop_admission. The guard exists before
            // spawn; an unpolled task still leaves original Work in registry.
            self.operations.spawn(async move {
                let mut future =
                    Box::pin(AssertUnwindSafe(owner.sends.run(&accepted.work)).catch_unwind());
                let result = future.as_mut().await;
                let destroyed = catch_unwind(AssertUnwindSafe(|| drop(future)));
                let value = match (result, destroyed) {
                    (Ok(result), Ok(())) => {
                        SendObservation::Complete(result.map(Arc::new).map_err(Arc::new))
                    }
                    _ => SendObservation::Unobserved,
                };
                let successful = matches!(&value, SendObservation::Complete(Ok(_)));
                let joined = matches!(&value, SendObservation::Complete(_));
                guard.finish(value);
                if successful
                    || (joined
                        && !accepted.work.core_entered()
                        && !accepted.work.has_uncertain_effects())
                {
                    owner.retire_send(key, &accepted);
                } else if joined
                    && !accepted.work.has_uncertain_effects()
                    && let Some(target) = accepted.work.target()
                {
                    // A normal definite error after birth retains the exact
                    // runtime until Source termination. Unknown outcome/turn
                    // custody is NEVER recovered by this termination monitor;
                    // it stays in registry and makes final cleanup return Err.
                    // This is not a Native/cohort/effects settlement receipt.
                    let retained = accepted.clone();
                    let monitor = owner.clone();
                    owner.monitors.spawn(async move {
                        target.wait_until_terminated().await;
                        monitor.retire_send(key, &retained);
                    });
                }
            });
        }
        loop {
            let value = receive.borrow_and_update().clone();
            if let Some(value) = value {
                return match value {
                    SendObservation::Complete(Ok(text)) => {
                        Ok(
                            CallToolResult::success(vec![ContentBlock::text(
                                text.as_ref().clone(),
                            )])
                            .into(),
                        )
                    }
                    SendObservation::Complete(Err(error)) => {
                        Ok(CallToolResult::error(vec![ContentBlock::text(
                            retained_task_read::policy::truncate(
                                &error,
                                retained_task_read::policy::MAX_ERROR_CHARS,
                            ),
                        )])
                        .into())
                    }
                    SendObservation::Unobserved => Err(McpError::internal_error(
                        "SEND outcome unobserved; do not replay",
                        None,
                    )),
                };
            }
            tokio::select! {
                biased;
                () = observer.cancelled() => return Err(McpError::internal_error(
                    "SEND observation cancelled; accepted work remains owned; do not replay", None)),
                () = self.incoming.withdrawn() => return Err(McpError::internal_error(
                    "SEND observation withdrew; accepted work remains owned; do not replay", None)),
                changed = receive.changed() => { changed.map_err(|_| McpError::internal_error(
                    "SEND observation unavailable; do not replay", None))?; }
            }
        }
    }

    fn retire_send(&self, key: Uuid, operation: &Arc<AcceptedSend>) {
        let retired = {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            if registry
                .sends
                .get(&key)
                .is_some_and(|actual| Arc::ptr_eq(actual, operation))
            {
                registry.sends.remove(&key)
            } else {
                None
            }
        };
        // Prebirth binding Drop can re-enter the registry.
        drop(retired);
    }

    pub(super) fn has_unobserved_send(&self) -> bool {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .sends
            .values()
            .any(|operation| {
                !matches!(
                    &*operation.completion.borrow(),
                    Some(SendObservation::Complete(_))
                ) || operation.work.has_uncertain_effects()
            })
    }
}
