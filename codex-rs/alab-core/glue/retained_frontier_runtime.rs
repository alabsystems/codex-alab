// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual selected Core/lease or distinct Source-construction S2 resource.
//! Both routes reuse the original listener/history/typed response machinery.
//! Metadata selects only; actual Native, Core Current and UI checks are real.
#[path = "retained_source_selection_runtime.rs"]
mod source_selection;
use super::*;
use crate::outgoing_message::RequestContext;
use crate::outgoing_message::alab_frontier_claim;
use alab_frontier_claim::Claim;
use alab_frontier_claim::Receipt;
use alab_frontier_claim::Selection;
use codex_alab_aterm::frontend_control::retained_service::AcceptedOrigin;
use codex_alab_aterm::frontend_control::retained_service::BoundBackend;
use codex_alab_aterm::frontend_control::retained_service::RuntimeAck;
use codex_alab_aterm::frontend_control::retained_service::RuntimeActivation;
use codex_alab_aterm::frontend_control::retained_service::RuntimeFrontier;
use codex_alab_aterm::frontend_control::retained_service::RuntimeObservationError;
use codex_alab_core::native_incoming::Incoming;
use futures::FutureExt as _;
use source_selection::PhysicalFrontier;
use source_selection::PrimaryResource;
use source_selection::ProjectionAck;
use std::collections::BTreeMap;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::sync::Mutex as StdMutex;
use std::sync::OnceLock;
use std::sync::PoisonError;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio::sync::watch;
use tokio_util::task::TaskTracker;

const MAX_RECEIPTS: usize = 4096;
#[derive(Clone, Default)]
pub(crate) struct Slot(Arc<OnceLock<Weak<Binding>>>);
impl Slot {
    pub(crate) fn install(&self, actual: &Arc<Binding>) -> std::io::Result<()> {
        self.0
            .set(Arc::downgrade(actual))
            .map_err(|_| std::io::Error::other("retained frontier already installed"))
    }
    fn actual(&self) -> Result<Arc<Binding>, JSONRPCErrorError> {
        self.0
            .get()
            .and_then(Weak::upgrade)
            .ok_or_else(|| invalid_request("actual retained frontier owner unavailable"))
    }
    pub(crate) fn stop(&self) {
        if let Ok(actual) = self.actual() {
            actual.stop();
        }
    }
}
struct State {
    closed: bool,
    projections: BTreeMap<String, Arc<Projection>>,
    latest: Option<String>,
    observations: BTreeMap<String, Arc<ActivationWork>>,
    activations: BTreeMap<String, Arc<ActivationWork>>,
    configured: configured::ConfiguredState,
    callbacks: callbacks::CallbackState,
}
pub(crate) struct Binding {
    backend: BoundBackend,
    input: Incoming,
    processor: Arc<ThreadRequestProcessor>,
    tracker: TaskTracker,
    stopped: tokio::sync::Notify,
    state: StdMutex<State>,
}
pub(crate) struct Projection {
    current_display: bool,
    binding: Weak<Binding>,
    receipt: Receipt,
    request_id: ConnectionRequestId,
    client_name: Option<String>,
    client_version: Option<String>,
    origin: Arc<AcceptedOrigin>,
    primary: PrimaryResource,
    frontier: PhysicalFrontier,
    // Original real context/accepted resource is retained even if C1 leaves.
    _context: RequestContext,
    listener_started: AtomicBool,
    uncertain: AtomicBool,
    response_attempted: AtomicBool,
    tail_finished: AtomicBool,
    ack: StdMutex<Option<RuntimeAck>>,
    stage: StdMutex<Option<RuntimeActivation>>,
    memory_history_mode: StdMutex<Option<ThreadHistoryMode>>,
    result: watch::Sender<Option<Result<(), JSONRPCErrorError>>>,
}
impl Binding {
    pub(crate) fn new(backend: BoundBackend, processor: Arc<ThreadRequestProcessor>) -> Arc<Self> {
        Arc::new(Self {
            input: backend.incoming(),
            backend,
            processor,
            tracker: TaskTracker::new(),
            stopped: tokio::sync::Notify::new(),
            state: StdMutex::new(State {
                closed: false,
                projections: BTreeMap::new(),
                latest: None,
                observations: BTreeMap::new(),
                activations: BTreeMap::new(),
                configured: configured::ConfiguredState::default(),
                callbacks: callbacks::CallbackState::default(),
            }),
        })
    }
    fn open(&self) -> Result<(), JSONRPCErrorError> {
        if self.input.is_stopped() || self.input.withdrawn().now_or_never().is_some() {
            return Err(invalid_request("retained frontier backend withdrew"));
        }
        if self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .closed
        {
            return Err(invalid_request("retained frontier admission closed"));
        }
        Ok(())
    }
    fn stop(&self) {
        {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.closed = true;
            self.tracker.close();
        }
        self.stopped.notify_waiters();
        // No Source mutex crosses SDK state. This closes only new entry;
        // genuine retirement keeps the original live controller until cleanup.
        self.backend.begin_source_shutdown();
    }
    async fn stopped(&self) {
        loop {
            let stopped = self.stopped.notified();
            tokio::pin!(stopped);
            stopped.as_mut().enable();
            if self
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .closed
            {
                return;
            }
            stopped.await;
        }
    }
    pub(crate) async fn drain(&self) -> std::io::Result<()> {
        self.stop();
        // Always drain/drop the actual writer before any Source callback or
        // retirement that needs that writer. Preserve its error while still
        // joining all accepted workers and attempting original Core cleanup.
        let shutdown = self.backend.begin_source_shutdown().drain().await;
        self.tracker.wait().await;
        // SDK receive/Goal workers are joined after actual processor cleanup;
        // this call only observes all accepted Source projection operations.
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state
            .projections
            .values()
            .any(|work| work.result.borrow().is_none() || work.uncertain.load(Ordering::Acquire))
            || state.observations.values().any(|work| {
                work.result.borrow().is_none() || work.uncertain.load(Ordering::Acquire)
            })
            || state.activations.values().any(|work| {
                work.result.borrow().is_none() || work.uncertain.load(Ordering::Acquire)
            })
            || state.configured.unobserved()
            || state.callbacks.unobserved()
        {
            return Err(std::io::Error::other(
                "retained frontier outcome remains unobserved",
            ));
        }
        shutdown
    }
    pub(crate) async fn finish_after_threads(&self) -> std::io::Result<()> {
        self.backend.shutdown().await
    }
    pub(crate) async fn handle(
        self: &Arc<Self>,
        request_id: ConnectionRequestId,
        context: RequestContext,
        params: ThreadResumeParams,
        claim: Claim,
        client_name: Option<String>,
        client_version: Option<String>,
    ) -> Result<(), JSONRPCErrorError> {
        self.open()?;
        if context.alab_request_id() != &request_id {
            return Err(invalid_request(
                "retained original request context disagrees",
            ));
        }
        let origin =
            context
                .alab_connection_resource
                .as_ref()
                .and_then(
                    codex_alab_core::connection_resource::ConnectionResource::downcast::<
                        AcceptedOrigin,
                    >,
                )
                .ok_or_else(|| {
                    invalid_request("retained frontier requires actual physical A origin")
                })?;
        // Type/presence is custody only. Genuine Native currentness is required
        // before any exact-primary or runtime packet may be selected.
        origin.check_current().await.map_err(native_error)?;
        if params.thread_id != claim.0.thread_id() {
            return Err(invalid_request("retained frontier primary disagrees"));
        }
        match claim.0 {
            Selection::Project(selection) => {
                use super::alab_initial_session::selected_primary::CurrentPrimary;
                if selection.candidate.is_some() {
                    return Err(invalid_request(
                        "R reconciliation requires actual selected primary; construction requires S2",
                    ));
                }
                let primary = match self.processor.alab_initial_session.current_primary()? {
                    CurrentPrimary::Selected(actual) => PrimaryResource::Selected(Arc::new(actual)),
                    CurrentPrimary::Empty(_) => {
                        return Err(invalid_request(
                            "no UI-selected primary exists; use configured projection",
                        ));
                    }
                };
                if primary.thread_id().to_string() != params.thread_id
                    || !primary.belongs_to(&self.input)
                {
                    return Err(invalid_request(
                        "retained frontier primary resource disagrees",
                    ));
                }
                let frontier = self
                    .backend
                    .frontier(&selection.frontier, &origin)
                    .await
                    .map_err(native_error)?;
                let actual_gate = primary
                    .thread()
                    .thread_extension_data()
                    .get::<codex_alab_core::retained_goal_gate::Gate>()
                    .ok_or_else(|| invalid_request("actual primary has no retained goal gate"))?;
                if !actual_gate.same(&frontier.goal_gate()) {
                    return Err(invalid_request("primary belongs to another goal owner"));
                }
                let snapshot = token();
                let receipt = Receipt {
                    frontier: frontier.frontier_data().into(),
                    snapshot,
                    thread_id: params.thread_id.clone(),
                    task_tools_available: primary.backend_task_tools()?,
                };
                let work = Arc::new(Projection {
                    current_display: selection.history
                        == alab_frontier_claim::HistoryIntent::CurrentDisplay,
                    binding: Arc::downgrade(self),
                    receipt,
                    request_id: request_id.clone(),
                    client_name,
                    client_version,
                    origin,
                    primary,
                    frontier: PhysicalFrontier::Runtime(frontier),
                    _context: context,
                    listener_started: AtomicBool::new(false),
                    uncertain: AtomicBool::new(false),
                    response_attempted: AtomicBool::new(false),
                    tail_finished: AtomicBool::new(false),
                    ack: StdMutex::new(None),
                    stage: StdMutex::new(None),
                    memory_history_mode: StdMutex::new(None),
                    result: watch::channel(None).0,
                });
                self.start_projection(work, params)
            }
            Selection::Observe(receipt) => {
                self.accept_activation(
                    request_id,
                    context,
                    origin,
                    receipt,
                    ActivationPhase::Observe,
                )
                .await
            }
            Selection::Activate(receipt) => {
                self.accept_activation(
                    request_id,
                    context,
                    origin,
                    receipt,
                    ActivationPhase::Commit,
                )
                .await
            }
        }
    }
    fn start_projection(
        self: &Arc<Self>,
        work: Arc<Projection>,
        params: ThreadResumeParams,
    ) -> Result<(), JSONRPCErrorError> {
        let request_id = work.request_id.clone();
        let snapshot = work.receipt.snapshot.clone();
        let guard = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed {
                return Err(invalid_request("retained frontier admission closed"));
            }
            // A new actual presentation supersedes the prior snapshot.
            // Its stale receipt then only fails; it cannot request a
            // second Core effect or release any replacement Hold.
            let retire = state
                .projections
                .iter()
                .filter(|(key, prior)| {
                    prior.result.borrow().is_some()
                        && !prior.uncertain.load(Ordering::Acquire)
                        && state.observations.get(*key).is_none_or(|observation| {
                            observation.result.borrow().is_some()
                                && !observation.uncertain.load(Ordering::Acquire)
                        })
                        && state.activations.get(*key).is_none_or(|activation| {
                            activation.result.borrow().is_some()
                                && !activation.uncertain.load(Ordering::Acquire)
                        })
                })
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            if state.projections.len().saturating_sub(retire.len()) >= MAX_RECEIPTS {
                return Err(invalid_request("retained frontier capacity unavailable"));
            }
            let retired = retire
                .into_iter()
                .map(|key| {
                    (
                        state.projections.remove(&key),
                        state.observations.remove(&key),
                        state.activations.remove(&key),
                    )
                })
                .collect::<Vec<_>>();
            state.latest = Some(snapshot.clone());
            state.callbacks.frontier_changed(); // ALAB(retained-ui): wake obsolete Source observers; a local future Drop is not cancellation.
            state.projections.insert(snapshot, work.clone());
            (
                Guard {
                    work: work.clone(),
                    input: self.input.clone(),
                    _tracked: self.tracker.token(),
                    finished: false,
                },
                retired,
            )
        };
        let (guard, retired) = guard;
        drop(retired); // Actual Core/context resources leave the state lock first.
        let binding = self.clone();
        // Guard exists before spawn. Actual accepted context and Core
        // custody outlive the HTTP observer; no new Core/turn is sent.
        tokio::spawn(async move {
            let mut future = Box::pin(
                AssertUnwindSafe(binding.project(&work, request_id.clone(), params)).catch_unwind(),
            );
            let outcome = future.as_mut().await;
            let destroyed = catch_unwind(AssertUnwindSafe(|| drop(future)));
            match (outcome, destroyed) {
                (Ok(result), Ok(())) => {
                    if let Err(error) = &result {
                        if work.response_attempted.load(Ordering::Acquire) {
                            work.tail(false, &binding.processor.outgoing).await;
                        } else if !work.listener_started.load(Ordering::Acquire) {
                            binding
                                .processor
                                .outgoing
                                .send_error(request_id, error.clone())
                                .await;
                        }
                    }
                    work.result.send_replace(Some(result));
                    guard.finish();
                }
                _ => {
                    binding.input.fail(std::io::Error::other(
                        "retained projection outcome remains unobserved",
                    ));
                    work.uncertain.store(true, Ordering::Release);
                    let error = internal_error("retained projection outcome remains unobserved");
                    if work.response_attempted.load(Ordering::Acquire) {
                        work.tail(false, &binding.processor.outgoing).await;
                    } else if !work.listener_started.load(Ordering::Acquire) {
                        binding
                            .processor
                            .outgoing
                            .send_error(request_id, error.clone())
                            .await;
                    }
                    work.result.send_replace(Some(Err(error)));
                    guard.finish();
                }
            }
        });
        Ok(())
    }
    async fn project(
        self: &Arc<Self>,
        work: &Arc<Projection>,
        _request_id: ConnectionRequestId,
        params: ThreadResumeParams,
    ) -> Result<(), JSONRPCErrorError> {
        self.open()?;
        // Do not wait for a queued Core event while holding the goal-resume
        // mutex needed by the original idle/listener loop.
        work.wait_side_listener(self).await?;
        let id = work.primary.thread_id();
        let _lifecycle = self
            .processor
            .thread_state_manager
            .lock_goal_resume(id)
            .await;
        work.primary.recheck().await?;
        self.current(work)?;
        let actual = work.primary.thread();
        let state = self.processor.thread_state_manager.thread_state(id).await;
        self.processor
            .ensure_listener_task_running(id, actual.clone(), state.clone())
            .await?;
        let sender = state
            .lock()
            .await
            .listener_command_tx()
            .ok_or_else(|| thread_closing_for_resume(id))?;
        let (complete, observed) = tokio::sync::oneshot::channel();
        sender
            .send(
                crate::thread_state::ThreadListenerCommand::AlabRetainedProjection(Box::new(
                    ProjectionCommand {
                        work: work.clone(),
                        params,
                        complete,
                    },
                )),
            )
            .map_err(|_| thread_closing_for_resume(id))?;
        observed.await.map_err(|_| thread_closing_for_resume(id))?
    }
    async fn capture_in_listener(
        self: &Arc<Self>,
        work: &Arc<Projection>,
        actual: &Arc<CodexThread>,
        params: ThreadResumeParams,
    ) -> Result<(), JSONRPCErrorError> {
        self.open()?;
        let id = work.primary.thread_id();
        if !work.primary.is_exact(actual) || !work.primary.lease_is_for(actual) {
            return Err(invalid_request(
                "retained command belongs to another actual listener",
            ));
        }
        work.primary.recheck().await?;
        self.current(work)?;
        {
            let pending = self
                .processor
                .pending_thread_unloads
                .clone()
                .lock_owned()
                .await;
            if pending.contains_key(&id) || actual.has_shutdown_status() {
                return Err(thread_closing_for_resume(id));
            }
            // Subscribe the REAL initialized C2 before any history read. The
            // SAME listener is executing this command, so subsequent events
            // remain behind its response/replay/tail and cannot fall in a gap.
            if !self
                .processor
                .thread_state_manager
                .try_add_connection_to_thread(id, work.request_id.connection_id)
                .await
            {
                return Err(invalid_request(
                    "retained frontend connection closed before projection",
                ));
            }
        }
        ThreadRequestProcessor::set_app_server_client_info(
            actual.as_ref(),
            work.client_name.clone(),
            work.client_version.clone(),
        )
        .await?;
        let (config_snapshot, _) = actual.resume_config_snapshot().await;
        self.processor
            .config_manager
            .check_thread_model_provider(actual.config().await.as_ref())
            .await
            .map_err(|error| invalid_request(error.to_string()))?;
        let stored = self
            .processor
            .read_stored_thread_for_read(id, false)
            .await
            .map_err(thread_read_view_error)?;
        if let Some(stored) = &stored {
            thread_resume_account::check(
                &self.processor.config.features,
                work.client_name.as_deref(),
                &self.processor.auth_manager,
                stored,
            )?;
        }
        let persisted = stored.map(|stored| {
            self.processor.stored_thread_to_api_thread(
                stored,
                &config_snapshot.model_provider_id,
                false,
            )
        });
        let summary = self
            .processor
            .load_live_thread_view(id, false, actual.as_ref(), persisted)
            .await
            .map_err(thread_read_view_error)?;
        let paginated = matches!(
            summary.history_mode,
            codex_app_server_protocol::ThreadHistoryMode::Paginated
        );
        let side_turns = work
            .side_turns(actual, config_snapshot.history_mode)
            .await?;
        let captured = if side_turns.is_some() {
            projection_history::Captured {
                durable_pages: false,
                include_turns: true,
                history_items: Vec::new(),
                memory_mode: Some(config_snapshot.history_mode),
            }
        } else {
            projection_history::capture(
                id,
                actual.as_ref(),
                &config_snapshot,
                paginated,
                !params.exclude_turns,
                params.initial_turns_page.is_some(),
                work.current_display,
            )
            .await?
        };
        let projection_history::Captured {
            durable_pages,
            include_turns,
            history_items,
            memory_mode,
        } = captured;
        *work
            .memory_history_mode
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = memory_mode;
        if durable_pages && (include_turns || params.initial_turns_page.is_some()) {
            self.processor
                .thread_store
                .persist_thread(id, PersistContext::Standard)
                .await
                .map_err(thread_store_resume_read_error)?;
        }
        let (emit_thread_goal_update, thread_goal_state_db) = self
            .processor
            .thread_goal_processor
            .pending_resume_goal_state(actual.as_ref())
            .await;
        let is_side_cut = side_turns.is_some();
        let paginated_turns = if side_turns.is_some() {
            side_turns
        } else if durable_pages && include_turns {
            Some(self.processor.paginated_thread_full_turns(id).await?)
        } else {
            None
        };
        let page = if durable_pages {
            match params.initial_turns_page.as_ref() {
                Some(params) => Some(
                    self.processor
                        .paginated_resume_initial_turns_page(id, params)
                        .await?,
                ),
                None => None,
            }
        } else {
            None
        };
        let active_page = if durable_pages {
            match params.initial_turns_page.as_ref() {
                Some(params)
                    if matches!(
                        params.sort_direction.unwrap_or(SortDirection::Desc),
                        SortDirection::Desc
                    ) =>
                {
                    Some(
                        self.processor
                            .paginated_resume_initial_turns_page_with_active_slot(id, params)
                            .await?,
                    )
                }
                _ => None,
            }
        } else {
            None
        };
        work.primary.recheck().await?;
        self.current(work)?;
        let pending = crate::thread_state::PendingThreadResumeRequest {
            request_id: work.request_id.clone(),
            alab_retained_resume: None,
            history_items,
            cold_resume_token_usage_turn_id: None,
            config_snapshot,
            instruction_sources: actual.legacy_instruction_sources().await,
            thread_summary: summary,
            emit_thread_goal_update,
            thread_goal_state_db,
            include_turns,
            initial_turns_page: if is_side_cut {
                None
            } else {
                params.initial_turns_page
            },
            paginated_turns,
            paginated_initial_turns_page: page,
            paginated_initial_turns_page_with_active_slot: active_page,
            resume_cursor_store: durable_pages.then(|| self.processor.thread_store.clone()),
            redact_resume_payloads: should_redact_thread_resume_payloads(
                work.client_name.as_deref(),
            ),
            alab_retained_tail: Some(work.clone()),
        };
        let state = self.processor.thread_state_manager.thread_state(id).await;
        super::super::thread_lifecycle::handle_pending_thread_resume_request(
            id,
            actual,
            &self.processor.config.codex_home,
            &self.processor.thread_state_manager,
            &state,
            &self.processor.thread_watch_manager,
            &self.processor.outgoing,
            &self.processor.pending_thread_unloads,
            pending,
        )
        .await;
        if !work.tail_finished.load(Ordering::Acquire) {
            return Err(internal_error(
                "retained projection did not reach its listener tail",
            ));
        }
        Ok(())
    }
    fn current(&self, work: &Projection) -> Result<(), JSONRPCErrorError> {
        self.open()?;
        work.primary.recheck_owned()?;
        if !work.primary.belongs_to(&self.input)
            || !work.frontier.same_receipt(&work.origin)
            || self
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .latest
                .as_ref()
                != Some(&work.receipt.snapshot)
        {
            return Err(invalid_request("retained projection ownership changed"));
        }
        Ok(())
    }
    async fn accept_activation(
        self: &Arc<Self>,
        request: ConnectionRequestId,
        context: RequestContext,
        origin: Arc<AcceptedOrigin>,
        receipt: Receipt,
        phase: ActivationPhase,
    ) -> Result<(), JSONRPCErrorError> {
        let work = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .projections
            .get(&receipt.snapshot)
            .cloned()
            .ok_or_else(|| invalid_request("retained projection receipt is unavailable"))?;
        if receipt != work.receipt || !work.frontier.same_receipt(&origin) {
            return Err(invalid_request(
                "retained activation belongs to another physical delivery",
            ));
        }
        // A Ready notification can physically reach the frontend just before
        // this SAME snapshot worker stores its joined result. Wait for that
        // real outcome; a Source metadata echo never substitutes for it.
        work.wait_ready(&self.input).await?;
        if matches!(&work.frontier, PhysicalFrontier::Source(_)) {
            let previous = {
                let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                if state.closed {
                    return Err(invalid_request("Source activation observer closed"));
                }
                match phase {
                    ActivationPhase::Observe => state.observations.get(&receipt.snapshot),
                    ActivationPhase::Commit => state.activations.get(&receipt.snapshot),
                }
                .cloned()
            };
            if let Some(previous) = previous {
                if !Arc::ptr_eq(&previous.projection, &work) {
                    return Err(invalid_request("Source cached operation disagrees"));
                }
                let mut changed = previous.result.subscribe();
                let result = loop {
                    if let Some(result) = changed.borrow_and_update().clone() {
                        break result;
                    }
                    tokio::select! { biased; () = self.input.withdrawn() => return Err(invalid_request("Source observer withdrew")),
                    result = changed.changed() => result.map_err(|_| internal_error("Source completion observation unavailable"))?, }
                };
                Self::publish_activation(
                    &self.processor.outgoing,
                    request,
                    &previous.projection,
                    result,
                )
                .await;
                return Ok(());
            }
        }
        self.current(&work)?;
        let (accepted, duplicate) = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed || state.latest.as_ref() != Some(&receipt.snapshot) {
                return Err(invalid_request(
                    "retained activation snapshot was superseded",
                ));
            }
            let operations = match phase {
                ActivationPhase::Observe => &mut state.observations,
                ActivationPhase::Commit => &mut state.activations,
            };
            if let Some(actual) = operations.get(&receipt.snapshot) {
                if !Arc::ptr_eq(&actual.projection, &work) {
                    return Err(invalid_request("retained activation record disagrees"));
                }
                (None, Some(actual.clone()))
            } else {
                if operations.len() >= MAX_RECEIPTS {
                    return Err(invalid_request("retained activation capacity unavailable"));
                }
                let actual = Arc::new(ActivationWork {
                    phase,
                    projection: work,
                    _context: context.clone(),
                    uncertain: AtomicBool::new(false),
                    result: watch::channel(None).0,
                });
                operations.insert(receipt.snapshot.clone(), actual.clone());
                let guard = ActivationGuard {
                    actual: actual.clone(),
                    input: self.input.clone(),
                    _tracked: self.tracker.token(),
                    finished: false,
                };
                (Some((actual, guard)), None)
            }
        };
        if let Some(actual) = duplicate {
            // Only this observer can be cancelled. The SAME accepted worker
            // and stored result survive; no Native query/release is resent.
            let mut changed = actual.result.subscribe();
            let result = loop {
                if let Some(result) = changed.borrow_and_update().clone() {
                    break result;
                }
                changed
                    .changed()
                    .await
                    .map_err(|_| internal_error("retained activation observation unavailable"))?;
            };
            Self::publish_activation(
                &self.processor.outgoing,
                request,
                &actual.projection,
                result,
            )
            .await;
            return Ok(());
        }
        let Some((actual, guard)) = accepted else {
            return Err(internal_error("retained activation acceptance unavailable"));
        };
        let binding = self.clone();
        // The SAME accepted activation is owned before its first Native query.
        // Losing the HTTP/UI observer cannot cancel release halfway through.
        tokio::spawn(async move {
            let mut future = Box::pin(AssertUnwindSafe(binding.activate(&actual)).catch_unwind());
            let outcome = future.as_mut().await;
            let destroyed = catch_unwind(AssertUnwindSafe(|| drop(future)));
            let result = match (outcome, destroyed) {
                (Ok(result), Ok(())) => result,
                _ => {
                    binding.input.fail(std::io::Error::other(
                        "retained activation outcome remains unobserved",
                    ));
                    actual.uncertain.store(true, Ordering::Release);
                    Err(internal_error(
                        "retained activation outcome remains unobserved",
                    ))
                }
            };
            actual.result.send_replace(Some(result.clone()));
            Self::publish_activation(
                &binding.processor.outgoing,
                request,
                &actual.projection,
                result,
            )
            .await;
            guard.finish();
        });
        Ok(())
    }
    async fn publish_activation(
        outgoing: &Arc<OutgoingMessageSender>,
        request: ConnectionRequestId,
        projection: &Projection,
        result: Result<(), JSONRPCErrorError>,
    ) {
        let payload = result.and_then(|()| {
            let (key, value) = projection.frontier.metadata(&projection.receipt)?;
            codex_app_server_protocol::alab_response_metadata::acknowledgment(key, value)
                .map_err(|_| internal_error("retained receipt projection failed"))
        });
        match payload {
            Ok(payload) => outgoing.send_response(request, payload).await,
            Err(error) => outgoing.send_error(request, error).await,
        }
    }
    async fn activate(&self, activation: &ActivationWork) -> Result<(), JSONRPCErrorError> {
        let work = &activation.projection;
        let _lifecycle = self
            .processor
            .thread_state_manager
            .lock_goal_resume(work.primary.thread_id())
            .await;
        self.current(work)?;
        if let PhysicalFrontier::Source(source) = &work.frontier {
            return source.activate(self, work, activation).await;
        }
        let ack = work
            .ack
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| invalid_request("retained listener tail was not acknowledged"))?;
        let staged = match activation.phase {
            ActivationPhase::Observe => None,
            ActivationPhase::Commit => Some(
                work.stage
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone()
                    .ok_or_else(|| invalid_request("actual retained Native stage unavailable"))?,
            ),
        };
        activation.uncertain.store(true, Ordering::Release);
        match activation.phase {
            ActivationPhase::Observe => {
                let staged = match ack.observe_activation().await {
                    Ok(observed) => observed,
                    Err(RuntimeObservationError::Refused(error)) => {
                        activation.uncertain.store(false, Ordering::Release);
                        return Err(native_error(error));
                    }
                    Err(RuntimeObservationError::Uncertain(error)) => {
                        return Err(native_error(error));
                    }
                };
                // Preserve the actual Native stage before any further await.
                // A lost HTTP reply reuses this SAME owned operation/result.
                *work.stage.lock().unwrap_or_else(PoisonError::into_inner) = Some(staged);
                work.primary.recheck().await?;
                self.current(work)?;
                // The SAME initial/replacement Hold remains closed here.
            }
            ActivationPhase::Commit => {
                let staged = staged.as_ref().ok_or_else(|| {
                    internal_error("accepted retained commit has no actual stage")
                })?;
                let committed = match ack.observe_commit(staged).await {
                    Ok(observed) => observed,
                    Err(RuntimeObservationError::Refused(error)) => {
                        activation.uncertain.store(false, Ordering::Release);
                        return Err(native_error(error));
                    }
                    Err(RuntimeObservationError::Uncertain(error)) => {
                        return Err(native_error(error));
                    }
                };
                // The final Native query requires actual promoted App custody
                // on replacement. THEN run the LAST real Core Current await.
                work.primary.recheck().await?;
                self.current(work)?;
                ack.release_committed(&committed, || {
                    self.current(work)
                        .map_err(|_| "actual selected primary changed".into())
                })
                .await
                .map_err(native_error)?;
            }
        }
        activation.uncertain.store(false, Ordering::Release);
        Ok(())
    }
}
impl Projection {
    async fn wait_ready(&self, input: &Incoming) -> Result<(), JSONRPCErrorError> {
        let mut observed = self.result.subscribe();
        loop {
            let result = observed.borrow_and_update().clone();
            if let Some(result) = result {
                result?;
                if self.uncertain.load(Ordering::Acquire)
                    || !self.tail_finished.load(Ordering::Acquire)
                {
                    return Err(invalid_request(
                        "actual retained listener tail remains unobserved",
                    ));
                }
                return Ok(());
            }
            tokio::select! {
                biased;
                () = input.withdrawn() => return Err(invalid_request("retained projection owner withdrew")),
                changed = observed.changed() => changed.map_err(|_| internal_error("retained projection observation unavailable"))?,
            }
        }
    }
    /// Only the original response sender's return calls this. Serialization
    /// is not enqueue/write proof; the real UI must still receive/install it.
    pub(crate) fn response_attempted(&self) {
        self.response_attempted.store(true, Ordering::Release);
    }
    pub(crate) fn listener_started(&self) {
        self.listener_started.store(true, Ordering::Release);
    }
    /// Called only where the ORIGINAL listener has constructed its real typed
    /// ThreadResumeResponse, before sending it. Never invokes resume_inner.
    pub(crate) fn decorate(
        &self,
        mut response: ThreadResumeResponse,
    ) -> Result<codex_app_server_protocol::ClientResponsePayload, JSONRPCErrorError> {
        self.filter_side_response(&mut response)?;
        let complete_memory = self.current_display
            && self
                .memory_history_mode
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_some();
        if complete_memory
            && (response.turns_backwards_cursor.is_some()
                || response.items_backwards_cursor.is_some())
        {
            return Err(internal_error(
                "complete memory snapshot unexpectedly has durable cursors",
            ));
        }
        let (key, receipt) = self.frontier.metadata(&self.receipt)?;
        let metadata = match &self.frontier {
            PhysicalFrontier::Source(source) => {
                serde_json::json!({"receipt":receipt,"completeMemory":complete_memory,"sideBoundary":source.side_display()?})
            }
            PhysicalFrontier::Runtime(_) => {
                serde_json::json!({"receipt":receipt,"completeMemory":complete_memory})
            }
        };
        let result = codex_app_server_protocol::alab_response_metadata::decorate(
            response.into(),
            key,
            metadata,
        )
        .map_err(|_| internal_error("retained response metadata failed"))?;
        Ok(result)
    }
    /// Run in the ORIGINAL listener after actual C2 subscription. Any later
    /// Core settings event then stays behind this command in its normal FIFO;
    /// an earlier event missed before subscribe is covered by this live read.
    pub(crate) async fn refresh_settings(
        &self,
        actual: &Arc<CodexThread>,
    ) -> Result<
        (
            ThreadConfigSnapshot,
            Vec<codex_utils_path_uri::LegacyAppPathString>,
        ),
        JSONRPCErrorError,
    > {
        let binding = self
            .binding
            .upgrade()
            .ok_or_else(|| invalid_request("retained runtime owner withdrew"))?;
        if !self.primary.is_exact(actual) || !self.primary.lease_is_for(actual) {
            return Err(invalid_request(
                "retained settings belong to another listener",
            ));
        }
        binding.current(self)?;
        self.primary.recheck().await?;
        let (snapshot, _) = actual.resume_config_snapshot().await;
        if self
            .memory_history_mode
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some_and(|mode| mode != snapshot.history_mode)
        {
            return Err(invalid_request(
                "retained history mode changed during projection",
            ));
        }
        binding
            .processor
            .config_manager
            .check_thread_model_provider(actual.config().await.as_ref())
            .await
            .map_err(|error| invalid_request(error.to_string()))?;
        if let Some(stored) = binding
            .processor
            .read_stored_thread_for_read(self.primary.thread_id(), false)
            .await
            .map_err(thread_read_view_error)?
        {
            thread_resume_account::check(
                &binding.processor.config.features,
                self.client_name.as_deref(),
                &binding.processor.auth_manager,
                &stored,
            )?;
        }
        let instruction_sources = actual.legacy_instruction_sources().await;
        binding.current(self)?;
        Ok((snapshot, instruction_sources))
    }
    pub(crate) fn populate_history(&self, thread: &mut Thread, items: &[RolloutItem]) {
        if let Some(mode) = *self
            .memory_history_mode
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
        {
            thread.turns = memory_turns(items, mode);
        } else {
            populate_thread_turns_from_history(thread, items, None);
        }
    }
    pub(crate) fn initial_page(
        &self,
        items: &[RolloutItem],
        status: ThreadStatus,
        running: bool,
        active: Option<Turn>,
        params: &ThreadResumeInitialTurnsPageParams,
    ) -> Result<codex_app_server_protocol::TurnsPage, JSONRPCErrorError> {
        let mode = *self
            .memory_history_mode
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(mode) = mode else {
            return build_thread_resume_initial_turns_page(items, status, running, active, params);
        };
        let mut turns = memory_turns(items, mode);
        let live = running
            || active
                .as_ref()
                .is_some_and(|turn| matches!(turn.status, TurnStatus::InProgress));
        normalize_thread_turns_status(&mut turns, status, live);
        if let Some(active) = active {
            merge_turn_history_with_active_turn(&mut turns, active);
        }
        apply_thread_turns_items_view(
            &mut turns,
            params.items_view.unwrap_or(TurnItemsView::Summary),
        );
        let page = paginate_thread_turns(
            turns,
            None,
            params.limit,
            params.sort_direction.unwrap_or(SortDirection::Desc),
        )?;
        Ok(codex_app_server_protocol::TurnsPage {
            data: page.turns,
            next_cursor: page.next_cursor,
            backwards_cursor: page.backwards_cursor,
        })
    }
    pub(crate) async fn finish_listener(
        &self,
        actual: &Arc<CodexThread>,
        outgoing: &Arc<OutgoingMessageSender>,
    ) -> Result<(), JSONRPCErrorError> {
        if !self.primary.is_exact(actual) || !self.primary.lease_is_for(actual) {
            return Err(invalid_request(
                "retained tail belongs to another actual listener",
            ));
        }
        let binding = self
            .binding
            .upgrade()
            .ok_or_else(|| invalid_request("retained runtime owner withdrew"))?;
        self.primary.recheck().await?;
        binding.current(self)?;
        self.primary.recheck_owned()?;
        self.uncertain.store(true, Ordering::Release);
        let ack = match self.frontier.acknowledge_reconciled(&self.origin).await {
            Ok(ack) => ack,
            Err(RuntimeObservationError::Refused(error)) => {
                self.uncertain.store(false, Ordering::Release);
                return Err(native_error(error));
            }
            Err(RuntimeObservationError::Uncertain(error)) => return Err(native_error(error)),
        };
        self.uncertain.store(false, Ordering::Release);
        match ack {
            ProjectionAck::Runtime(ack) => {
                *self.ack.lock().unwrap_or_else(PoisonError::into_inner) = Some(ack)
            }
            ProjectionAck::Source(ack) => match &self.frontier {
                PhysicalFrontier::Source(source) => source.store_ack(ack),
                PhysicalFrontier::Runtime(_) => {
                    return Err(internal_error("Source ACK has no actual Source frontier"));
                }
            },
        }
        self.tail(true, outgoing).await;
        self.tail_finished.store(true, Ordering::Release);
        Ok(())
    }
    async fn tail(&self, ready: bool, outgoing: &Arc<OutgoingMessageSender>) {
        use codex_app_server_protocol::RetainedFrontierFailure;
        use codex_app_server_protocol::RetainedFrontierNotification;
        use codex_app_server_protocol::RetainedFrontierOutcome;
        outgoing
            .send_server_notification_to_connections(
                &[self.request_id.connection_id],
                ServerNotification::AlabRetainedFrontier(RetainedFrontierNotification {
                    frontier: self.receipt.frontier.clone(),
                    snapshot: self.receipt.snapshot.clone(),
                    thread_id: self.receipt.thread_id.clone(),
                    outcome: if ready {
                        RetainedFrontierOutcome::Ready {}
                    } else {
                        RetainedFrontierOutcome::Failed {
                            reason: RetainedFrontierFailure::Unavailable,
                        }
                    },
                }),
            )
            .await;
    }
}
struct Guard {
    work: Arc<Projection>,
    input: Incoming,
    _tracked: tokio_util::task::task_tracker::TaskTrackerToken,
    finished: bool,
}
impl Guard {
    fn finish(mut self) {
        self.finished = true;
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        if !self.finished {
            self.input.fail(std::io::Error::other(
                "retained projection outcome remains unobserved",
            ));
            self.work.uncertain.store(true, Ordering::Release);
            self.work.result.send_replace(Some(Err(internal_error(
                "retained projection outcome remains unobserved",
            ))));
        }
    }
}
fn native_error(_: std::io::Error) -> JSONRPCErrorError {
    invalid_request("actual retained Native observation unavailable")
}
fn token() -> String {
    format!(
        "{:016x}{:016x}",
        codex_alab_core::native_source::connection_request_prefix(),
        codex_alab_core::native_source::connection_request_prefix()
    )
}
impl ThreadRequestProcessor {
    pub(crate) async fn alab_retained_frontier(
        &self,
        id: ConnectionRequestId,
        context: RequestContext,
        params: ThreadResumeParams,
        claim: Claim,
        client_name: Option<String>,
        client_version: Option<String>,
    ) -> Result<(), JSONRPCErrorError> {
        self.alab_retained_frontier
            .actual()?
            .handle(id, context, params, claim, client_name, client_version)
            .await
    }
}

impl ThreadRequestProcessor {
    pub(crate) fn alab_install_frontier(&self, actual: &Arc<Binding>) -> std::io::Result<()> {
        self.alab_retained_frontier.install(actual)
    }
    pub(crate) fn alab_stop_frontier(&self) {
        self.alab_retained_frontier.stop();
    }
}

#[derive(Clone, Copy)]
enum ActivationPhase {
    Observe,
    Commit,
}
struct ActivationWork {
    phase: ActivationPhase,
    projection: Arc<Projection>,
    uncertain: AtomicBool,
    _context: RequestContext,
    result: watch::Sender<Option<Result<(), JSONRPCErrorError>>>,
}
struct ActivationGuard {
    actual: Arc<ActivationWork>,
    input: Incoming,
    _tracked: tokio_util::task::task_tracker::TaskTrackerToken,
    finished: bool,
}
impl ActivationGuard {
    fn finish(mut self) {
        self.finished = true;
    }
}
impl Drop for ActivationGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.input.fail(std::io::Error::other(
                "retained activation outcome remains unobserved",
            ));
            self.actual.uncertain.store(true, Ordering::Release);
            self.actual.result.send_replace(Some(Err(internal_error(
                "retained activation outcome remains unobserved",
            ))));
        }
    }
}

fn memory_turns(items: &[RolloutItem], mode: ThreadHistoryMode) -> Vec<Turn> {
    let mut builder = codex_app_server_protocol::ThreadHistoryBuilder::new();
    for item in items {
        // Preserve the existing persisted/display privacy policy, including
        // canonical item output limits, before the original reducer sees it.
        if let Some(item) = codex_rollout::persisted_rollout_item(item, mode) {
            builder.handle_retained_history_item(item.as_ref(), mode);
        }
    }
    builder.finish()
}

#[cfg(test)]
#[path = "retained_frontier_history_tests.rs"]
mod history_tests;

/// Only the actual protected worker constructs this. The existing listener
/// supplies its own current Core reference; no JSON/default context can do so.
pub(crate) struct ProjectionCommand {
    work: Arc<Projection>,
    params: ThreadResumeParams,
    complete: tokio::sync::oneshot::Sender<Result<(), JSONRPCErrorError>>,
}
impl ProjectionCommand {
    pub(crate) async fn run(self, actual: Option<&Arc<CodexThread>>) {
        let outcome = match (self.work.binding.upgrade(), actual) {
            (Some(binding), Some(actual)) => {
                binding
                    .capture_in_listener(&self.work, actual, self.params)
                    .await
            }
            _ => Err(invalid_request("retained actual listener unavailable")),
        };
        let _ = self.complete.send(outcome);
    }
}

/// The original Initial/ordinary Resume helper remains the sole publisher
/// unless this is the privately accepted physical projection. Decorating a
/// typed response cannot claim enqueue/write; only the sender return is marked.
pub(crate) async fn publish_resume(
    tail: Option<&Arc<Projection>>,
    publication: Option<super::alab_initial_resume::ResumePublication>,
    outgoing: &OutgoingMessageSender,
    request: ConnectionRequestId,
    id: ThreadId,
    actual: &Arc<CodexThread>,
    reply: (ThreadResumeResponse, String),
) -> Result<(), (ConnectionRequestId, JSONRPCErrorError)> {
    let (response, originator) = reply;
    let Some(tail) = tail else {
        super::alab_initial_resume::publish(
            publication,
            outgoing,
            request,
            id,
            actual,
            response,
            originator,
        )
        .await;
        return Ok(());
    };
    if publication.is_some() {
        return Err((
            request,
            invalid_request(
                "retained projection cannot replace another accepted Resume publication",
            ),
        ));
    }
    let response = tail
        .decorate(response)
        .map_err(|error| (request.clone(), error))?;
    outgoing
        .send_response_with_thread_originator(request, response, originator)
        .await;
    tail.response_attempted();
    Ok(())
}

#[path = "retained_configured_runtime.rs"]
mod configured;

#[path = "retained_projection_history.rs"]
pub(crate) mod projection_history;

#[path = "retained_reader_runtime.rs"]
mod readers; // ALAB(retained-readers): same actual bound origin, backend loader and tracked original response owner.

#[path = "retained_callback_runtime.rs"]
mod callbacks; // ALAB(retained-ui): private captured endpoint owners and ordered projection facts.

#[path = "retained_side_projection.rs"]
mod side_boundary;
