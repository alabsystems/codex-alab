// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A request is admitted locally before spawning; its effects outlive a dismissed UI.

use super::Cohort;
use codex_alab_core::assignment as domain;
use codex_alab_core::assignment_service as native;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

#[derive(Default)]
pub(super) struct State {
    running: AtomicBool,
    startup_receipt: AtomicBool,
    intent: Mutex<Option<domain::Intent>>,
    observation: Mutex<Option<domain::Observation>>,
}

struct Consumer {
    cohort: Arc<Cohort>,
    thread: String,
}

pub(super) fn register(thread: String, cohort: Arc<Cohort>) -> Arc<dyn domain::Consumer> {
    register_with_receipt(
        thread,
        cohort,
        codex_alab_core::session::shared().assignment_receipt(),
    )
}

fn register_with_receipt(
    thread: String,
    cohort: Arc<Cohort>,
    receipt: Option<domain::Receipt>,
) -> Arc<dyn domain::Consumer> {
    // A serialized receipt is a reconciliation locator, never a replacement
    // for the native caller/retirement permits lost when a process exits.
    if !cohort.coordinator.running.load(Ordering::Acquire)
        && let Some(receipt) = receipt
        && receipt.valid()
        && !cohort
            .coordinator
            .observation
            .lock()
            .is_ok_and(|observation| {
                observation.as_ref().is_some_and(|previous| {
                    previous.operation == receipt.operation
                        && matches!(
                            previous.progress,
                            domain::Progress::Applied | domain::Progress::Refused
                        )
                })
            })
    {
        let intent = domain::Intent {
            operation: receipt.operation,
            thread: receipt.thread,
            selection: receipt.selection,
        };
        if cohort.domain.hold_unresolved(intent.clone()).is_ok() {
            cohort
                .coordinator
                .startup_receipt
                .store(true, Ordering::Release);
            observe(&cohort, &intent, domain::Progress::Reconciling);
        }
    }
    let consumer: Arc<dyn domain::Consumer> = Arc::new(Consumer {
        cohort,
        thread: thread.clone(),
    });
    domain::register_consumer(&thread, &consumer);
    consumer
}

impl domain::Consumer for Consumer {
    fn request<'a>(&'a self, intent: domain::Intent) -> domain::AdmissionFuture<'a> {
        Box::pin(async move {
            if intent.thread != self.thread || !codex_alab_core::is_alab_binary() {
                return Err(domain::Refused::Invalid);
            }
            {
                let previous = self
                    .cohort
                    .coordinator
                    .intent
                    .lock()
                    .map_err(|_| domain::Refused::Unavailable)?;
                if self.cohort.coordinator.running.load(Ordering::Acquire)
                    && previous
                        .as_ref()
                        .is_some_and(|previous| previous != &intent)
                {
                    return Err(domain::Refused::Busy);
                }
                if previous
                    .as_ref()
                    .is_some_and(|previous| previous.operation == intent.operation)
                {
                    if previous.as_ref() != Some(&intent) {
                        return Err(domain::Refused::Busy);
                    }
                    if self
                        .cohort
                        .coordinator
                        .observation
                        .lock()
                        .map_err(|_| domain::Refused::Unavailable)?
                        .as_ref()
                        .is_some_and(|observation| {
                            observation.operation == intent.operation
                                && matches!(
                                    observation.progress,
                                    domain::Progress::Applied | domain::Progress::Refused
                                )
                        })
                    {
                        return Ok(());
                    }
                }
            }
            self.cohort.domain.request(intent.clone())?;
            if self
                .cohort
                .coordinator
                .startup_receipt
                .load(Ordering::Acquire)
            {
                return Err(domain::Refused::Busy);
            }
            if self
                .cohort
                .coordinator
                .running
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return Ok(());
            }
            if self.cohort.domain.phase() != domain::Phase::Pending {
                self.cohort
                    .coordinator
                    .running
                    .store(false, Ordering::Release);
                return Err(domain::Refused::Busy);
            }
            *self
                .cohort
                .coordinator
                .intent
                .lock()
                .map_err(|_| domain::Refused::Unavailable)? = Some(intent.clone());
            // A single owned coroutine keeps every admitted resource across retries.
            // Same-operation requests only observe it; they never start another prepare.
            let operation = Operation {
                cohort: Arc::clone(&self.cohort),
                intent,
            };
            tokio::spawn(operation.run());
            Ok(())
        })
    }
    fn observation(&self) -> Option<domain::Observation> {
        self.cohort.coordinator.observation.lock().ok()?.clone()
    }
}

fn observe(cohort: &Cohort, intent: &domain::Intent, progress: domain::Progress) {
    if let Ok(mut observation) = cohort.coordinator.observation.lock() {
        if observation.as_ref().is_some_and(|previous| {
            previous.operation == intent.operation && previous.progress == progress
        }) {
            return;
        }
        *observation = Some(domain::Observation {
            operation: intent.operation.clone(),
            progress,
            target: match &intent.selection {
                domain::Selection::Manual { account, .. } => Some(account.clone()),
                domain::Selection::Auto { .. } => None,
            },
            mode: Some(match intent.selection {
                domain::Selection::Manual { .. } => native::Mode::Manual,
                domain::Selection::Auto { .. } => native::Mode::Auto,
            }),
            reason: None,
        });
    }
    codex_alab_core::session::shared().mark_changed();
}

async fn clear_receipt(operation: &str) -> Result<(), domain::Refused> {
    let shared = codex_alab_core::session::shared();
    if shared.assignment_receipt().is_none() {
        return Ok(());
    }
    if !shared.clear_assignment_receipt(operation) {
        return Err(domain::Refused::Busy);
    }
    tokio::task::spawn_blocking(|| codex_alab_core::session::shared().flush_record())
        .await
        .map_err(|_| domain::Refused::Unavailable)?
        .then_some(())
        .ok_or(domain::Refused::Unavailable)
}

async fn cancel_pending(cohort: &Cohort, intent: &domain::Intent) -> Result<(), domain::Refused> {
    clear_receipt(&intent.operation).await?;
    cohort.domain.cancel_pending(&intent.operation)?;
    observe(cohort, intent, domain::Progress::Refused);
    Ok(())
}

async fn persist(receipt: domain::Receipt) -> Result<(), domain::Refused> {
    let shared = codex_alab_core::session::shared();
    if !shared.has_record_writer() || !shared.set_assignment_receipt(Some(receipt)) {
        return Err(domain::Refused::Unavailable);
    }
    tokio::task::spawn_blocking(|| codex_alab_core::session::shared().flush_record())
        .await
        .map_err(|_| domain::Refused::Unavailable)?
        .then_some(())
        .ok_or(domain::Refused::Unavailable)
}

// Kept only after the original exact ACK command returns its correlated
// Applied receipt. A public Poll or source observation cannot construct this
// actor state. Retain the old body even when current credentials advance.
struct Acknowledgement<S> {
    scope: S,
    _command: native::Command,
    _receipt: native::Status,
}

struct Operation {
    cohort: Arc<Cohort>,
    intent: domain::Intent,
}
impl Operation {
    async fn run(self) {
        observe(&self.cohort, &self.intent, domain::Progress::Preparing);
        drive(&self.cohort, &self.intent).await;
        self.cohort
            .coordinator
            .running
            .store(false, Ordering::Release);
    }
}

// These retries are inside the owned operation, rather than around drive(). The
// coroutine therefore retains the actual admission, source and boundary permits.
macro_rules! reconcile {
    ($cohort:expr, $intent:expr, $action:expr) => {{
        let mut delay = std::time::Duration::from_millis(250);
        loop {
            match $action {
                Ok(value) => break value,
                Err(_) => {
                    observe($cohort, $intent, domain::Progress::Reconciling);
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(std::time::Duration::from_secs(5));
                }
            }
        }
    }};
}

async fn settled(
    cohort: &Cohort,
    intent: &domain::Intent,
    command: native::Command,
) -> native::Status {
    reconcile!(
        cohort,
        intent,
        native::reconcile_command(
            command.clone(),
            |command| async move { native::command(&command).await },
            || observe(cohort, intent, domain::Progress::Reconciling)
        )
        .await
    )
}

async fn drive(cohort: &Cohort, intent: &domain::Intent) {
    let initial = reconcile!(
        cohort,
        intent,
        cohort.manager.alab_native_source_fence().await
    );
    let mut receipt = domain::Receipt {
        operation: intent.operation.clone(),
        boundary: None,
        thread: intent.thread.clone(),
        selection: intent.selection.clone(),
        source: initial.receipt_stamp(),
        destination: None,
        acknowledged: false,
    };
    drop(initial);
    reconcile!(cohort, intent, persist(receipt.clone()).await);
    let (pool_revision, selection) = match &intent.selection {
        domain::Selection::Manual { account, revision } => {
            (revision.clone(), Some(account.clone()))
        }
        domain::Selection::Auto { revision } => (revision.clone(), None),
    };
    let prepared = settled(
        cohort,
        intent,
        native::Command::Prepare {
            operation: intent.operation.clone(),
            pool_revision,
            expected_assignment: receipt.source.assignment_generation,
            selection,
        },
    )
    .await;
    if matches!(
        prepared.phase,
        native::Phase::Cancelled | native::Phase::Refused
    ) && prepared.assignment_generation.is_none()
        && prepared.boundary.is_none()
    {
        reconcile!(cohort, intent, cancel_pending(cohort, intent).await);
        return;
    }
    let target = reconcile!(cohort, intent, native::target(&intent.operation).await);
    reconcile!(
        cohort,
        intent,
        cohort.manager.alab_check_native_target(&target)
    );
    let admission = reconcile!(cohort, intent, domain::admit_process(&target).await);
    observe(cohort, intent, domain::Progress::Draining);
    let retirement = Arc::new(reconcile!(
        cohort,
        intent,
        cohort.domain.retire(&intent.operation).await
    ));
    let mut source = reconcile!(
        cohort,
        intent,
        cohort.manager.alab_native_source_fence().await
    );
    reconcile!(cohort, intent, cohort.retire(&retirement).await);
    let mut retirement = retirement;
    let retirement = loop {
        match Arc::try_unwrap(retirement) {
            Ok(owned) => break owned,
            Err(owned) => {
                retirement = owned;
                observe(cohort, intent, domain::Progress::Reconciling);
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }
    };
    let mut boundary = retirement.wait_drained().await;
    reconcile!(
        cohort,
        intent,
        boundary.set_native_intent(intent.operation.clone())
    );
    let boundary = Arc::new(boundary);
    reconcile!(
        cohort,
        intent,
        cohort
            .manager
            .alab_pin_current_native_source(&mut source, &boundary)
            .await
    );
    receipt.source = source.receipt_stamp();
    receipt.boundary = Some(boundary.boundary().into());
    reconcile!(cohort, intent, persist(receipt.clone()).await);
    reconcile!(cohort, intent, admission.validate());
    observe(cohort, intent, domain::Progress::Committing);
    let installed = settled(
        cohort,
        intent,
        native::Command::Commit {
            operation: intent.operation.clone(),
            boundary: boundary.boundary().into(),
            source_credential_generation: source.credential_generation(),
            generations: receipt.source.generations.clone(),
        },
    )
    .await;
    if matches!(
        installed.phase,
        native::Phase::Cancelled | native::Phase::Refused
    ) && installed.assignment_generation.is_none()
        && installed.credential_generation.is_none()
    {
        reconcile!(
            cohort,
            intent,
            cohort
                .manager
                .alab_restore_unchanged_native_source(&mut source, &boundary, &installed)
                .await
        );
        drop(source);
        reconcile!(cohort, intent, cohort.prepare_destination(&boundary).await);
        reconcile!(cohort, intent, admission.validate());
        let binding = reconcile!(cohort, intent, boundary.bound());
        reconcile!(cohort, intent, clear_receipt(&intent.operation).await);
        let completion = domain::AbortCompletion {
            operation: intent.operation.clone(),
            boundary: boundary.boundary().into(),
            binding,
        };
        reconcile!(cohort, intent, admission.validate());
        reconcile!(cohort, intent, boundary.try_abort(&completion));
        reconcile!(cohort, intent, cohort.resume().await);
        observe(cohort, intent, domain::Progress::Refused);
        return;
    }
    reconcile!(
        cohort,
        intent,
        (matches!(
            installed.phase,
            native::Phase::AwaitingAck | native::Phase::Applied
        ) && installed.boundary.as_deref() == Some(boundary.boundary()))
        .then_some(())
        .ok_or(domain::Refused::Unavailable)
    );
    observe(cohort, intent, domain::Progress::Rebinding);
    reconcile!(
        cohort,
        intent,
        cohort
            .manager
            .alab_rebind_native_source_fenced(&intent.operation, &source, &boundary)
            .await
    );
    reconcile!(cohort, intent, cohort.manager.alab_verify_native_target(&target));
    let mut binding = reconcile!(cohort, intent, boundary.bound());
    reconcile!(
        cohort,
        intent,
        (Some(binding.assignment_generation) == installed.assignment_generation
            && installed
                .credential_generation
                .is_some_and(|generation| binding.credential_generation >= generation)
            && binding.account == target.account
            && binding.user == target.user)
            .then_some(())
            .ok_or(domain::Refused::Unavailable)
    );
    // Destination initialization may refresh. Retain the actor/boundary, while
    // releasing the local refresh semaphore before initializing account resources.
    drop(source);
    let mut acknowledged = None::<Acknowledgement<_>>;
    let mut initialization_delay = std::time::Duration::from_millis(250);
    let acknowledgement_fence = loop {
        let acknowledgement_fence = loop {
            receipt.destination = Some(domain::ReceiptStamp {
                credential_generation: binding.credential_generation,
                assignment_generation: binding.assignment_generation,
                generations: binding.generations.clone(),
            });
            reconcile!(cohort, intent, persist(receipt.clone()).await);
            observe(cohort, intent, domain::Progress::Initializing);
            reconcile!(cohort, intent, cohort.manager.alab_verify_native_target(&target));
            let ready = cohort.prepare_destination(&boundary).await;
            let fence = reconcile!(
                cohort,
                intent,
                cohort.manager.alab_native_source_fence().await
            );
            reconcile!(
                cohort,
                intent,
                cohort
                    .manager
                    .alab_rebind_native_source_fenced(&intent.operation, &fence, &boundary)
                    .await
            );
            reconcile!(cohort, intent, cohort.manager.alab_verify_native_target(&target));
            let observed = reconcile!(cohort, intent, boundary.bound());
            reconcile!(
                cohort,
                intent,
                (observed.account == binding.account
                    && observed.user == binding.user
                    && observed.assignment_generation == binding.assignment_generation
                    && observed.credential_generation >= binding.credential_generation)
                    .then_some(())
                    .ok_or(domain::Refused::Unavailable)
            );
            if ready.is_ok() && observed == binding {
                break fence;
            }
            if observed != binding {
                initialization_delay = std::time::Duration::from_millis(250);
            }
            binding = observed;
            drop(fence);
            observe(cohort, intent, domain::Progress::Reconciling);
            reconcile!(cohort, intent, admission.validate());
            tokio::time::sleep(initialization_delay).await;
            initialization_delay =
                (initialization_delay * 2).min(std::time::Duration::from_secs(5));
        };
        reconcile!(cohort, intent, admission.validate());
        if let Some(acknowledgement) = &acknowledged {
            if acknowledgement_fence.continues_scope(&acknowledgement.scope) {
                break acknowledgement_fence;
            }
            // A different scope cannot inherit the old ACK. Keep the original
            // operation and freeze while re-reading actual current authority.
            drop(acknowledgement_fence);
            observe(cohort, intent, domain::Progress::Reconciling);
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            continue;
        }
        observe(cohort, intent, domain::Progress::Acknowledging);
        let command = native::Command::Ack {
            operation: intent.operation.clone(),
            boundary: boundary.boundary().into(),
            binding: binding.clone(),
        };
        let scope = acknowledgement_fence.scope();
        let mut applied = settled(cohort, intent, command.clone()).await;
        loop {
            if applied.phase == native::Phase::AwaitingAck {
                break;
            }
            if applied.phase == native::Phase::Applied
                && applied.boundary.as_deref() == Some(boundary.boundary())
                && applied.assignment_generation == Some(binding.assignment_generation)
                && applied.credential_generation == Some(binding.credential_generation)
            {
                acknowledged = Some(Acknowledgement {
                    scope,
                    _command: command,
                    _receipt: applied,
                });
                break;
            }
            observe(cohort, intent, domain::Progress::Reconciling);
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            let polled = reconcile!(
                cohort,
                intent,
                native::command(&native::Command::Poll {
                    operation: intent.operation.clone(),
                })
                .await
            );
            // An Applied observation is only a reason to request the exact old
            // ACK receipt; it cannot establish accepted digest/stamps itself.
            applied = if polled.phase == native::Phase::Applied {
                settled(cohort, intent, command.clone()).await
            } else {
                polled
            };
        }
        drop(acknowledgement_fence);
        // Even a successful immutable ACK is followed by a new private Source,
        // rebind and resource initialization. Another consumer may have rotated
        // this exact grant while the local refresh semaphore was held.
    };
    receipt.acknowledged = true;
    reconcile!(cohort, intent, persist(receipt.clone()).await);
    let completion = domain::Completion {
        operation: intent.operation.clone(),
        native_intent: intent.operation.clone(),
        boundary: boundary.boundary().into(),
        binding,
    };
    reconcile!(cohort, intent, admission.validate());
    reconcile!(cohort, intent, boundary.try_finish(&completion));
    drop(acknowledgement_fence);
    reconcile!(cohort, intent, cohort.resume().await);
    observe(cohort, intent, domain::Progress::Applied);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct Admission(Arc<AtomicUsize>);
    impl domain::ProcessAdmission for Admission {
        fn validate(&self) -> Result<(), domain::Refused> {
            Ok(())
        }
    }
    impl Drop for Admission {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }

    #[tokio::test]
    async fn unresolved_startup_receipt_fences_every_new_cohort_without_thread_or_boundary_inference()
     {
        let receipt = domain::Receipt {
            operation: "a".repeat(32),
            boundary: None,
            thread: "00000000-0000-0000-0000-000000000001".into(),
            selection: domain::Selection::Manual {
                account: "b".repeat(64),
                revision: "c".repeat(64),
            },
            source: domain::ReceiptStamp {
                credential_generation: 1,
                assignment_generation: 0,
                generations: domain::Generations {
                    auth: 0,
                    owner: 0,
                    source: 0,
                    login: 0,
                },
            },
            destination: None,
            acknowledged: false,
        };
        let mut cohorts = Vec::new();
        let mut consumers = Vec::new();
        for _ in 0..2 {
            let (session, _) = super::super::super::tests::make_session_and_context().await;
            let cohort = Arc::new(Cohort {
                domain: session.services.auth_manager.alab_assignment_domain(),
                manager: Arc::clone(&session.services.auth_manager),
                coordinator: State::default(),
                participants: Default::default(),
            });
            assert_ne!(session.thread_id().to_string(), receipt.thread);
            let inherited = cohort.domain.admit_work(None).unwrap();
            let consumer = register_with_receipt(
                session.thread_id().to_string(),
                Arc::clone(&cohort),
                Some(receipt.clone()),
            );
            assert_eq!(cohort.domain.phase(), domain::Phase::Uncertain);
            assert!(cohort.domain.admit_work(Some(&inherited)).is_err());
            assert!(cohort.domain.admit_resource(Some(&inherited)).is_err());
            assert!(cohort.domain.admit_start(None).is_err());
            assert!(
                cohort
                    .domain
                    .recover_retirement(&receipt.operation)
                    .is_err()
            );
            assert_eq!(consumer.observation().unwrap().operation, receipt.operation);
            cohorts.push(cohort);
            consumers.push(consumer);
        }
        assert!(
            !Arc::ptr_eq(&cohorts[0].domain, &cohorts[1].domain),
            "distinct managers still need actual proof of disjointness"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn local_reconciliation_retains_admission_and_boundary_while_dispatch_stays_live() {
        let (session, _) = super::super::super::tests::make_session_and_context().await;
        let cohort = Arc::new(Cohort {
            domain: session.services.auth_manager.alab_assignment_domain(),
            manager: Arc::clone(&session.services.auth_manager),
            coordinator: State::default(),
            participants: Default::default(),
        });
        let intent = domain::Intent {
            operation: "a".repeat(32),
            thread: session.thread_id().to_string(),
            selection: domain::Selection::Manual {
                account: "b".repeat(64),
                revision: "c".repeat(64),
            },
        };
        cohort.domain.request(intent.clone()).unwrap();
        let retirement = cohort.domain.retire(&intent.operation).await.unwrap();
        let mut boundary = retirement.wait_drained().await;
        boundary
            .set_native_intent(intent.operation.clone())
            .unwrap();
        let dropped = Arc::new(AtomicUsize::new(0));
        let admission: Box<dyn domain::ProcessAdmission> =
            Box::new(Admission(Arc::clone(&dropped)));
        let mut dispatch = super::super::DispatchGate::default();
        for stage in ["binding", "resource initialization"] {
            let mut attempts = 0;
            reconcile!(&cohort, &intent, {
                attempts += 1;
                admission.validate().unwrap();
                assert_eq!(dropped.load(Ordering::Acquire), 0);
                assert!(boundary.protects(
                    &cohort.domain,
                    &intent.operation,
                    Some(&intent.operation)
                ));
                assert!(cohort.domain.admit_work(None).is_err());
                let submission = super::super::Submission {
                    id: stage.into(),
                    op: codex_protocol::protocol::Op::Interrupt,
                    turn_extension_init: None,
                    trace: None,
                    parent_turn_id: None,
                    root_turn_id: None,
                    residency_guard: None,
                    message_delivery: None,
                };
                assert!(dispatch.admit(&session, submission).await.is_some());
                if attempts < 3 {
                    Err(domain::Refused::Unavailable)
                } else {
                    Ok(())
                }
            });
            assert_eq!(attempts, 3);
        }
        assert_eq!(dropped.load(Ordering::Acquire), 0);
        drop(admission);
        assert_eq!(dropped.load(Ordering::Acquire), 1);
        drop(boundary);
        assert!(
            cohort.domain.admit_work(None).is_err(),
            "dropping a failed actor cannot reopen the domain"
        );
    }
}
