// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! One accepted ordinary startup, owned by its backend through observation loss.
//! Claim bytes select this operation; only actual Native origin admits a caller.
use super::*;
use crate::outgoing_message::alab_initial_claim::Claim;
use crate::outgoing_message::alab_initial_claim::ClientPresentation;
use codex_alab_aterm::frontend_control::retained_service::AcceptedOrigin;
use codex_alab_core::native_incoming::Incoming;
use codex_alab_task_worktree::StartupBinding;
use futures::FutureExt;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::PoisonError;
use std::sync::Weak;
use tokio_util::task::TaskTracker;

#[path = "initial_session_frozen.rs"]
mod frozen;
use super::alab_initial_start::fallback;

use crate::in_process::InitialSessionKind as Kind;
use crate::in_process::InitialSessionPlan;
#[derive(Clone)]
pub(crate) enum Arguments {
    Start(ThreadStartParams),
    Fork(ThreadForkParams),
    Resume(ThreadResumeParams),
}
#[derive(Clone)]
pub(crate) struct ClientMetadata {
    pub(crate) name: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) extensions: ClientMcpExtensions,
}
#[derive(Clone)]
enum Reply {
    Start(Arc<ThreadStartResponse>, String),
    Fork(Arc<ThreadForkResponse>, String),
    Resume(Arc<ThreadResumeResponse>, String),
    Error(JSONRPCErrorError),
    Unknown,
}
// Bound only additional waiting observers; the accepted original worker is
// never rejected/canceled because a replay observer exceeds this Source limit.
const MAX_PENDING_REPLAYS: usize = 128;
#[path = "client_construction_registry.rs"]
mod client_registry;
#[path = "selected_primary_owner.rs"]
pub(crate) mod selected_primary;
#[path = "retained_side_scope.rs"]
mod side_boundary;
#[path = "retained_side_rollback.rs"]
mod side_rollback;
#[path = "retained_side_view_close.rs"]
mod side_view_close;
#[path = "retained_side_view_owner.rs"]
mod side_views;

struct State {
    closed: bool,
    accepted: Option<Arc<Scope>>,
    next_claim: Option<String>,
    clients: HashMap<String, Arc<Scope>>,
    preparations: HashMap<String, Arc<client_registry::PreparedClient>>,
}
pub(crate) struct InitialSessionOperation {
    claim: String,
    kind: Kind,
    binding: Option<StartupBinding>,
    input: Incoming,
    bound: Option<codex_alab_aterm::frontend_control::retained_service::BoundBackend>,
    processor: Arc<ThreadRequestProcessor>,
    tracker: TaskTracker,
    state: Mutex<State>,
    selected_primary: Mutex<selected_primary::SelectionState>,
    side_views: Mutex<side_views::Views>,
    selection_changed: tokio::sync::Notify,
}
/// The real processor stores a weak association. The retained runtime owns the
/// operation strongly, avoiding processor->operation->processor self-custody.
#[derive(Clone, Default)]
pub(crate) struct InitialSlot(Arc<OnceLock<Weak<InitialSessionOperation>>>);
impl InitialSlot {
    pub(crate) fn retire_clients_for_terminated(&self, actual: &Arc<CodexThread>) {
        if let Some(owner) = self.0.get().and_then(Weak::upgrade) {
            owner.retire_clients_for_terminated(actual);
        }
    }
    pub(crate) async fn accept_client(
        &self,
        context: RequestContext,
        arguments: Arguments,
        metadata: ClientMetadata,
    ) -> Result<(), JSONRPCErrorError> {
        let owner = self
            .0
            .get()
            .and_then(Weak::upgrade)
            .ok_or_else(|| invalid_request("actual ordinary construction owner unavailable"))?;
        owner.accept_client(context, arguments, metadata).await
    }
    pub(crate) fn install(&self, actual: &Arc<InitialSessionOperation>) -> std::io::Result<()> {
        self.0.set(Arc::downgrade(actual)).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "initial session operation already installed",
            )
        })
    }
    pub(crate) async fn accept(
        &self,
        context: RequestContext,
        arguments: Arguments,
        metadata: ClientMetadata,
    ) -> Result<(), JSONRPCErrorError> {
        let claim = context
            .alab_initial_claim
            .clone()
            .ok_or_else(|| invalid_request("initial session metadata missing"))?;
        let actual =
            self.0.get().and_then(Weak::upgrade).ok_or_else(|| {
                invalid_request("actual initial session operation is unavailable")
            })?;
        actual.accept(claim, context, arguments, metadata).await
    }
    pub(crate) fn stop(&self) {
        if let Some(actual) = self.0.get().and_then(Weak::upgrade) {
            actual.stop();
        }
    }
}
struct Progress {
    entered: bool,
    created: Option<(ThreadId, Arc<CodexThread>)>,
    // Exact early loaded probe custody; it is not a birth or final selection.
    resolved: Option<(ThreadId, Arc<CodexThread>)>,
    loaded_task_tools: bool,
    next_selection: Option<crate::outgoing_message::alab_initial_claim::NextSelection>,
    staged: Option<Reply>,
    complete: Option<Reply>,
    worker_lost: bool,
    worker_finished: bool,
    held_selection:
        Option<Arc<codex_alab_aterm::frontend_control::retained_service::HeldSourceTransition>>,
    selection_retired: bool,
    selection_unknown: bool,
    history_fallback: bool,
    dynamic_tools_fallback: bool,
    task_binding: Option<crate::request_processors::PreparedChildBinding>,
    primary: Option<super::super::thread_lifecycle::alab_primary_lifetime::PrimaryLease>,
    primary_weak: Option<super::super::thread_lifecycle::alab_primary_lifetime::WeakPrimaryLease>,
    replays: Vec<RequestContext>,
}
impl Progress {
    fn may_offer_selection(&self) -> bool {
        !self.worker_lost
            && matches!(self.complete, Some(Reply::Error(_)))
            && !self.entered
            && self.created.is_none()
            && self.resolved.is_none()
    }
}
pub(super) struct Scope {
    operation: Weak<InitialSessionOperation>,
    input: Incoming,
    processor: Arc<ThreadRequestProcessor>,
    binding: Option<StartupBinding>,
    pub(super) context: RequestContext,
    frozen: frozen::FrozenRequest,
    arguments: Arguments,
    metadata: ClientMetadata,
    progress: Arc<Mutex<Progress>>,
    candidate: String,
    registered_selection: Option<
        Arc<codex_alab_aterm::frontend_control::retained_service::RegisteredSourceTransition>,
    >,
    prepared_offer: Option<crate::outgoing_message::alab_initial_claim::PreparationOffer>,
    preparation_error: Option<JSONRPCErrorError>,
    side_boundary: Mutex<side_boundary::SideSlot>,
    side_closings: Mutex<side_view_close::Closings>,
    side_rollback: Mutex<Option<Arc<side_rollback::Rollback>>>,
    prior_selection: selected_primary::PriorSelection,
    completed: tokio::sync::watch::Sender<()>,
}
impl InitialSessionOperation {
    pub(crate) fn new(
        plan: InitialSessionPlan,
        input: Incoming,
        processor: Arc<ThreadRequestProcessor>,
    ) -> Arc<Self> {
        Self::new_inner(plan, input, processor, None)
    }
    pub(crate) fn new_bound(
        plan: InitialSessionPlan,
        bound: codex_alab_aterm::frontend_control::retained_service::BoundBackend,
        processor: Arc<ThreadRequestProcessor>,
    ) -> Arc<Self> {
        Self::new_inner(plan, bound.incoming(), processor, Some(bound))
    }
    fn new_inner(
        plan: InitialSessionPlan,
        input: Incoming,
        processor: Arc<ThreadRequestProcessor>,
        bound: Option<codex_alab_aterm::frontend_control::retained_service::BoundBackend>,
    ) -> Arc<Self> {
        Arc::new(Self {
            claim: plan.claim,
            kind: plan.kind,
            binding: plan.binding,
            input,
            bound,
            processor,
            tracker: TaskTracker::new(),
            selected_primary: Mutex::new(selected_primary::SelectionState::new()),
            side_views: Mutex::new(side_views::Views::default()),
            selection_changed: tokio::sync::Notify::new(),
            state: Mutex::new(State {
                closed: false,
                accepted: None,
                next_claim: None,
                clients: HashMap::new(),
                preparations: HashMap::new(),
            }),
        })
    }
    fn matches_original_kind(&self, arguments: &Arguments) -> bool {
        matches!(
            (&self.kind, arguments),
            (Kind::Start, Arguments::Start(_))
                | (Kind::Fork, Arguments::Fork(_))
                | (Kind::Resume, Arguments::Resume(_))
                | (Kind::Selection, _)
        )
    }
    fn open(&self) -> Result<(), JSONRPCErrorError> {
        if self.input.is_stopped() || self.input.withdrawn().now_or_never().is_some() {
            Err(internal_error("initial session backend owner withdrew"))
        } else {
            Ok(())
        }
    }
    /// A new current C2 may only observe the same immutable operation. It cannot
    /// replace original context/metadata/arguments, repeat Core, or cancel C1's
    /// accepted work. Native check happens before actual acceptance/replay.
    pub(crate) async fn accept(
        self: &Arc<Self>,
        claim: Claim,
        context: RequestContext,
        arguments: Arguments,
        metadata: ClientMetadata,
    ) -> Result<(), JSONRPCErrorError> {
        self.open()?;
        if !matches!(
            (&arguments, &claim.selection.prospective_create),
            (Arguments::Start(_), None) | (Arguments::Fork(_) | Arguments::Resume(_), Some(_))
        ) {
            return Err(invalid_request(
                "initial fork/resume requires its actual prospective CREATE projection",
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
                    invalid_request("initial session requires actual retained connection origin")
                })?;
        origin
            .check_current()
            .await
            .map_err(|_| invalid_request("initial session native origin is not current"))?;
        self.open()?;
        let preparation = self.prepare_initial(&claim, &arguments, &origin).await?;
        let prior_selection = self.snapshot_selection();
        let (scope, completed, retired) = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed {
                return Err(internal_error("initial session admission closed"));
            }
            let renewed = state.accepted.as_ref().is_some_and(|previous| {
                state.next_claim.as_deref() == Some(claim.selection.claim.as_str())
                    && previous.can_offer_selection()
            });
            if let Some(scope) = &state.accepted
                && !renewed
            {
                if !scope.frozen.matches(&claim) {
                    return Err(invalid_request(
                        "initial session replay changed original arguments",
                    ));
                }
                let mut progress = scope
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                let complete = progress.complete.clone();
                if complete.is_none()
                    && !progress
                        .replays
                        .iter()
                        .any(|waiting| waiting.alab_request_id() == context.alab_request_id())
                {
                    if progress.replays.len() >= MAX_PENDING_REPLAYS {
                        return Err(invalid_request(
                            "initial session replay observer capacity reached",
                        ));
                    }
                    progress.replays.push(context.clone());
                }
                (None, complete.map(|reply| (scope.clone(), reply)), None)
            } else {
                if !self.matches_original_kind(&arguments)
                    || (!renewed
                        && (state.accepted.is_some() || claim.selection.claim != self.claim))
                {
                    return Err(invalid_request("initial session operation does not match"));
                }
                let (prepared, registered, offer, preparation_error) = match preparation {
                    client_registry::InitialPreparation::Registered(
                        prepared,
                        registered,
                        offer,
                    ) => (prepared, Some(registered), Some(offer), None),
                    client_registry::InitialPreparation::Refused(prepared, error) => {
                        (prepared, None, None, Some(error))
                    }
                    client_registry::InitialPreparation::Replay => {
                        return Err(invalid_request(
                            "initial replay lost its actual accepted Scope",
                        ));
                    }
                };
                // Fresh backend choice, with actual Native reservation OR only
                // a terminal known-unregistered no-Core error, never a grant.
                state.next_claim = None;
                let scope = Arc::new(Scope {
                    candidate: InitialSessionPlan::fresh_claim(),
                    registered_selection: registered,
                    prepared_offer: offer,
                    preparation_error,
                    side_boundary: Mutex::new(side_boundary::SideSlot::default()),
                    side_closings: Mutex::new(side_view_close::Closings::default()),
                    side_rollback: Mutex::new(None),
                    prior_selection: prior_selection.clone()?,
                    completed: tokio::sync::watch::channel(()).0,
                    operation: Arc::downgrade(self),
                    input: self.input.clone(),
                    processor: self.processor.clone(),
                    binding: self.binding.clone(),
                    context: context.clone(),
                    frozen: frozen::FrozenRequest::new(claim),
                    arguments,
                    metadata,
                    progress: Arc::new(Mutex::new(Progress {
                        entered: false,
                        created: None,
                        resolved: None,
                        loaded_task_tools: false,
                        next_selection: None,
                        staged: None,
                        complete: None,
                        worker_lost: false,
                        worker_finished: false,
                        held_selection: None,
                        selection_retired: false,
                        selection_unknown: false,
                        history_fallback: false,
                        dynamic_tools_fallback: false,
                        task_binding: None,
                        primary: None,
                        primary_weak: None,
                        replays: Vec::new(),
                    })),
                });
                let guard = Guard {
                    progress: scope.progress.clone(),
                    changed: scope.completed.clone(),
                    input: self.input.clone(),
                    completed: false,
                    _tracked: self.tracker.token(),
                };
                prepared.accept(&scope);
                let retired = state.accepted.replace(scope.clone());
                (Some((scope, guard)), None, retired)
            }
        };
        if let Some(previous) = &retired {
            self.retire_initial_preparation(previous);
        }
        drop(retired); // Release old observed no-effect request resources outside admission's mutex.
        if let Some((scope, complete)) = completed {
            scope.publish_replay(&context, &complete).await;
        }
        if let Some((running, guard)) = scope {
            self.run_owned(running, guard).await;
        }
        Ok(())
    }
    async fn run_owned(self: &Arc<Self>, running: Arc<Scope>, mut guard: Guard) {
        // This remains the ORIGINAL queued worker with its actual permits,
        // serialization/auth-originator scope and retained RequestContext.
        let mut work = Box::pin(AssertUnwindSafe(running.run()).catch_unwind());
        let result = work.as_mut().await;
        let destroyed = catch_unwind(AssertUnwindSafe(|| drop(work)));
        match (result, destroyed) {
            (Ok(reply), Ok(())) => {
                running
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .staged = Some(reply.clone());
                guard.policy_joined();
                running.retire_failed_selection().await;
                running.finish(reply).await;
                guard.finish();
                // A normal terminal no-Core operation can retire immediately;
                // a captured operation only after actual Core termination.
                self.retire_client_if_observed(&running);
            }
            _ => drop(guard),
        }
    }
    pub(crate) async fn accept_client(
        self: &Arc<Self>,
        context: RequestContext,
        arguments: Arguments,
        metadata: ClientMetadata,
    ) -> Result<(), JSONRPCErrorError> {
        self.open()?;
        let claim = context
            .alab_client_construction
            .clone()
            .ok_or_else(|| invalid_request("client construction projection missing"))?;
        match (&arguments, claim.method.as_ref()) {
            (Arguments::Start(params), "thread/start")
                if claim.selection.presentation == ClientPresentation::Primary
                    && &claim.selection.prospective_create == params => {}
            (Arguments::Fork(_), "thread/fork") | (Arguments::Resume(_), "thread/resume") => {}
            _ => {
                return Err(invalid_request(
                    "client construction changed its original method or Start projection",
                ));
            }
        }
        let offer = claim.selection.prepared.clone().ok_or_else(|| {
            invalid_request("client construction requires its observed preparation offer")
        })?;
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
                    invalid_request(
                        "client construction requires actual retained connection origin",
                    )
                })?;
        origin
            .check_current()
            .await
            .map_err(|_| invalid_request("client construction Native origin is not current"))?;
        self.open()?;
        self.sweep_finished_clients();
        let prior_selection = self.snapshot_selection();
        let (running, completed) = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.closed {
                return Err(invalid_request("client construction admission closed"));
            }
            let prepared = state
                .preparations
                .get(&format!("client:{}", offer.preparation))
                .cloned()
                .filter(|prepared| prepared.matches(&claim))
                .ok_or_else(|| {
                    invalid_request(
                        "client construction changed or lost its actual prepared request",
                    )
                })?;
            if let Some(scope) = state.clients.get(&offer.claim) {
                if !scope.frozen.matches_client(&claim) {
                    return Err(invalid_request(
                        "client replay changed original full arguments",
                    ));
                }
                let mut progress = scope
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                let completed = progress.complete.clone();
                if completed.is_none()
                    && !progress
                        .replays
                        .iter()
                        .any(|waiting| waiting.alab_request_id() == context.alab_request_id())
                {
                    if progress.replays.len() >= MAX_PENDING_REPLAYS {
                        return Err(invalid_request("client replay observer capacity reached"));
                    }
                    progress.replays.push(context.clone());
                }
                (None, completed.map(|reply| (scope.clone(), reply)))
            } else {
                if state.clients.len() >= 4096 {
                    return Err(invalid_request("client construction capacity reached"));
                }
                let registered = prepared.registered()?;
                let scope = Arc::new(Scope {
                    candidate: InitialSessionPlan::fresh_claim(),
                    registered_selection: Some(registered),
                    prepared_offer: Some(prepared.offer()?),
                    preparation_error: None,
                    side_boundary: Mutex::new(side_boundary::SideSlot::default()),
                    side_closings: Mutex::new(side_view_close::Closings::default()),
                    side_rollback: Mutex::new(None),
                    prior_selection: prior_selection?,
                    completed: tokio::sync::watch::channel(()).0,
                    operation: Arc::downgrade(self),
                    input: self.input.clone(),
                    processor: self.processor.clone(),
                    binding: None,
                    context: context.clone(),
                    frozen: frozen::FrozenRequest::client(claim),
                    arguments,
                    metadata,
                    progress: Arc::new(Mutex::new(Progress {
                        entered: false,
                        created: None,
                        resolved: None,
                        loaded_task_tools: false,
                        next_selection: None,
                        staged: None,
                        complete: None,
                        worker_lost: false,
                        worker_finished: false,
                        held_selection: None,
                        selection_retired: false,
                        selection_unknown: false,
                        history_fallback: false,
                        dynamic_tools_fallback: false,
                        task_binding: None,
                        primary: None,
                        primary_weak: None,
                        replays: Vec::new(),
                    })),
                });
                let guard = Guard {
                    progress: scope.progress.clone(),
                    changed: scope.completed.clone(),
                    input: self.input.clone(),
                    completed: false,
                    _tracked: self.tracker.token(),
                };
                prepared.accept(&scope);
                state.clients.insert(offer.claim, scope.clone());
                (Some((scope, guard)), None)
            }
        };
        if let Some((scope, reply)) = completed {
            scope.publish_replay(&context, &reply).await;
        }
        if let Some((scope, guard)) = running {
            self.run_owned(scope, guard).await;
        }
        Ok(())
    }
    fn sweep_finished_clients(&self) {
        // Bounded housekeeping on a real later request, not an expiry/pid/ID
        // probe loop. The public termination future must ACTUALLY be ready.
        // This also covers loaded runtimes without this host's MCP binding.
        let scopes = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clients
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for scope in scopes {
            self.retire_client_if_observed(&scope);
        }
    }
    fn retire_client_if_observed(&self, scope: &Arc<Scope>) {
        if scope.unobserved_side_close() || scope.unobserved_side_rollback() {
            return;
        }
        let Some(offer) = &scope.prepared_offer else {
            return;
        };
        let captured = {
            let progress = scope
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !progress.worker_finished
                || progress.worker_lost
                || progress.selection_unknown
                || !progress.selection_retired
                || matches!(progress.complete, None | Some(Reply::Unknown))
                || (progress.entered && progress.created.is_none())
            {
                return;
            }
            progress
                .created
                .as_ref()
                .or(progress.resolved.as_ref())
                .map(|(_, actual)| actual.clone())
        };
        // One actual termination observation after normal completion (not a
        // sleep/probe loop, consumed event, has_shutdown guess or Native proof).
        if captured
            .as_ref()
            .is_some_and(|actual| actual.wait_until_terminated().now_or_never().is_none())
        {
            return;
        }
        if let Some(actual) = &captured {
            self.release_side_view(scope, actual);
        } // Terminal exact Core only; drop live owner outside registry locks.
        let retired = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state
                .clients
                .get(&offer.claim)
                .is_some_and(|actual| Arc::ptr_eq(actual, scope))
            {
                (
                    state.clients.remove(&offer.claim),
                    state
                        .preparations
                        .remove(&format!("client:{}", offer.preparation)),
                )
            } else {
                (None, None)
            }
        };
        drop(retired); // Actual owner/resources are never destroyed under admission's lock.
    }
    fn retire_clients_for_terminated(&self, actual: &Arc<CodexThread>) {
        let scopes = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clients
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for scope in scopes {
            let same = {
                let progress = scope
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                progress
                    .created
                    .as_ref()
                    .or(progress.resolved.as_ref())
                    .is_some_and(|(_, candidate)| Arc::ptr_eq(candidate, actual))
            };
            if same {
                self.retire_client_if_observed(&scope);
            }
        }
    }
    fn complete_reply(
        &self,
        completed: &Scope,
        reply: Reply,
    ) -> (
        Reply,
        Vec<RequestContext>,
        Option<crate::request_processors::PreparedChildBinding>,
    ) {
        // Same order as acceptance/replay. No observer can read a terminal
        // error between its completion and infallible cached next-choice offer.
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut progress = completed
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if progress.complete.is_none() {
            progress.complete = Some(reply);
            if let Some(previous_claim) = completed.frozen.claim()
                && !state.closed
                && self.open().is_ok()
                && state
                    .accepted
                    .as_ref()
                    .is_some_and(|scope| std::ptr::eq(scope.as_ref(), completed))
                && progress.may_offer_selection()
                && progress.next_selection.is_none()
                && (completed.registered_selection.is_none() || progress.selection_retired)
                && let Some(Reply::Error(error)) = progress.complete.as_ref()
            {
                let next_claim = state
                    .next_claim
                    .clone()
                    .unwrap_or_else(InitialSessionPlan::fresh_claim);
                let next = crate::outgoing_message::alab_initial_claim::NextSelection {
                    previous_claim: previous_claim.into(),
                    claim: next_claim.clone(),
                };
                if next.valid() {
                    let error = crate::outgoing_message::alab_initial_claim::decorate_error(
                        error,
                        next.clone(),
                    );
                    progress.complete = Some(Reply::Error(error));
                    progress.next_selection = Some(next);
                    state.next_claim = Some(next_claim);
                }
            }
        }
        let reply = progress.complete.clone().unwrap_or(Reply::Unknown);
        let abandoned = if matches!(reply, Reply::Error(_))
            && !progress.entered
            && progress.created.is_none()
            && progress.resolved.is_none()
        {
            progress.task_binding.take()
        } else {
            None
        };
        (reply, std::mem::take(&mut progress.replays), abandoned)
    }
    /// Close new acceptance, join the SAME workers and original nested startup
    /// tasks BEFORE actual processor.shutdown_threads. Late births stay owned.
    fn stop(&self) {
        {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.closed = true;
            self.tracker.close();
        }
        self.selection_changed.notify_waiters();
    }
    pub(crate) fn release_primary_after_threads(&self) {
        self.release_side_views_after_threads();
        let retired_selection = self
            .selected_primary
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take_after_threads();
        let scopes = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state
                .accepted
                .iter()
                .chain(state.clients.values())
                .cloned()
                .collect::<Vec<_>>()
        };
        for scope in scopes {
            scope.release_primary();
        }
        drop(retired_selection); // Real owner shutdown; never destroy leases under the selection lock.
        self.selection_changed.notify_waiters();
    }
    pub(crate) async fn drain(self: &Arc<Self>) -> Result<(), String> {
        self.stop();
        self.tracker.wait().await;
        let preparations = self.retire_unaccepted_after_join().await;
        self.processor.drain_background_tasks().await;
        preparations?;
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state
            .accepted
            .iter()
            .chain(state.clients.values())
            .any(|scope| {
                if (scope.side_close_unknown() || scope.unknown_side_rollback()) {
                    return true;
                }
                let progress = scope
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                progress.worker_lost
                    || progress.selection_unknown
                    || matches!(progress.complete, Some(Reply::Unknown) | None)
                    || (progress.entered && progress.created.is_none())
            })
        {
            Err("initial session outcome remains unobserved".into())
        } else {
            Ok(())
        }
    }
}
struct Guard {
    progress: Arc<Mutex<Progress>>,
    changed: tokio::sync::watch::Sender<()>,
    input: Incoming,
    completed: bool,
    _tracked: tokio_util::task::task_tracker::TaskTrackerToken,
}
impl Guard {
    fn policy_joined(&mut self) {
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .worker_finished = true;
    }
    fn finish(mut self) {
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .worker_finished = true;
        self.completed = true;
        self.changed.send_replace(());
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        if !self.completed {
            {
                let mut progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
                progress.worker_lost = true;
                if progress.complete.is_none() {
                    progress.complete = Some(Reply::Unknown);
                }
            }
            self.input.fail(std::io::Error::other(
                "initial session worker outcome unobserved",
            ));
            self.changed.send_replace(());
        }
    }
}
impl Scope {
    pub(super) fn request(&self) -> &ConnectionRequestId {
        self.context.alab_request_id()
    }
    /// Service withdrawal before Core is a real owner condition. Accepted work
    /// is never canceled because its frontend observer/controller later closes.
    pub(super) fn check_owner(&self) -> Result<(), JSONRPCErrorError> {
        if self.input.is_stopped() || self.input.withdrawn().now_or_never().is_some() {
            return Err(internal_error("initial session backend owner withdrew"));
        }
        Ok(())
    }
    pub(super) fn before_core(&self) -> Result<(), JSONRPCErrorError> {
        self.check_owner()?;
        if self.registered_selection.is_some() {
            let held = self
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .held_selection
                .clone()
                .ok_or_else(|| {
                    internal_error("accepted construction has no drained Source transition")
                })?;
            held.mark_core_entry()
                .map_err(|_| internal_error("actual Source transition cannot enter Core"))?;
        }
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entered = true;
        Ok(())
    }
    pub(super) fn capture(
        &self,
        id: ThreadId,
        actual: &Arc<CodexThread>,
    ) -> Result<(), JSONRPCErrorError> {
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .created = Some((id, actual.clone()));
        if let Some(binding) = &self.binding {
            binding
                .capture(id, actual.clone())
                .map_err(internal_error)?;
        }
        Ok(())
    }
    pub(super) async fn configure_fork(
        &self,
        config: &mut Config,
    ) -> Result<(), JSONRPCErrorError> {
        self.check_owner()?;
        let Arguments::Fork(original) = &self.arguments else {
            return Err(internal_error("initial fork lost its actual request"));
        };
        let prospective = self
            .frozen
            .prospective()
            .cloned()
            .ok_or_else(|| invalid_request("initial fork prospective CREATE data missing"))?;
        let validated =
            Box::pin(self.processor.alab_validate_prospective_start(prospective)).await?;
        let capture = crate::message_processor::alab_task_mcp::ForkConstruction {
            _original: original.clone(),
            prospective: validated,
            _resolved: Arc::new(config.clone()),
            _config_manager: self.processor.config_manager.clone(),
            _auth_manager: self.processor.auth_manager.clone(),
            _manager: Arc::downgrade(&self.processor.thread_manager),
            app_server_client_name: self.metadata.name.clone(),
            app_server_client_version: self.metadata.version.clone(),
            client_mcp_extensions: self.metadata.extensions.clone(),
        };
        // Real host allocates a NEW binding. Both prospective and actual Config
        // pass the original constrained namespace setter before capture/Core.
        let binding = self
            .processor
            .alab_task_mcp
            .prepare_fork(capture, config)
            .await?;
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .task_binding = Some(binding);
        self.check_owner()
    }
    pub(super) fn capture_resume_loaded(&self, id: ThreadId, actual: &Arc<CodexThread>) {
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .resolved = Some((id, actual.clone()));
    }
    pub(super) async fn configure_resume(
        &self,
        config: &mut Config,
    ) -> Result<(), JSONRPCErrorError> {
        self.check_owner()?;
        let Arguments::Resume(original) = &self.arguments else {
            return Err(internal_error("initial resume lost its actual request"));
        };
        let prospective = self
            .frozen
            .prospective()
            .cloned()
            .ok_or_else(|| invalid_request("initial resume prospective CREATE data missing"))?;
        let validated =
            Box::pin(self.processor.alab_validate_prospective_start(prospective)).await?;
        let capture = crate::message_processor::alab_task_mcp::ResumeConstruction {
            _original: original.clone(),
            prospective: validated,
            _resolved: Arc::new(config.clone()),
            _config_manager: self.processor.config_manager.clone(),
            _auth_manager: self.processor.auth_manager.clone(),
            _manager: Arc::downgrade(&self.processor.thread_manager),
            app_server_client_name: self.metadata.name.clone(),
            app_server_client_version: self.metadata.version.clone(),
            client_mcp_extensions: self.metadata.extensions.clone(),
        };
        let binding = self
            .processor
            .alab_task_mcp
            .prepare_resume(capture, config)
            .await?;
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .task_binding = Some(binding);
        self.check_owner()
    }
    pub(super) async fn resume_ready(
        &self,
        context: ListenerTaskContext,
        id: ThreadId,
        actual: &Arc<CodexThread>,
        born: bool,
    ) -> Result<(), JSONRPCErrorError> {
        self.check_owner()?;
        if actual.has_shutdown_status()
            || !matches!(
                self.processor
                    .thread_manager
                    .thread_registration(id, actual)
                    .await,
                codex_core::ThreadRegistration::Current
            )
        {
            return Err(invalid_request("initial resume actual runtime withdrew"));
        }
        self.ensure_primary(&context, id, actual).await?;
        if born {
            let binding = self
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .task_binding
                .clone()
                .ok_or_else(|| internal_error("initial cold resume lost its real binding"))?;
            binding
                .bind_created(id, actual.clone())
                .await
                .map_err(|_| internal_error("initial cold resume binding withdrew"))?;
        } else {
            // Observe the real same-host binding by exact Arc; never replace a
            // loaded runtime's config/transport or infer ownership from JSON.
            let bound = self.processor.alab_task_mcp.bound_tools(id, actual).await?;
            self.progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .loaded_task_tools = bound;
        }
        super::super::thread_lifecycle::alab_retained_listener::ensure_backend_start_listener(
            context,
            id,
            actual.clone(),
        )
        .await?;
        self.check_owner()
    }
    pub(super) async fn stage_resume(
        &self,
        id: ThreadId,
        actual: &Arc<CodexThread>,
        response: ThreadResumeResponse,
        originator: String,
    ) -> Result<ClientResponsePayload, JSONRPCErrorError> {
        self.check_owner()?;
        if actual.has_shutdown_status()
            || !matches!(
                self.processor
                    .thread_manager
                    .thread_registration(id, actual)
                    .await,
                codex_core::ThreadRegistration::Current
            )
        {
            return Err(invalid_request(
                "initial resume actual runtime withdrew before publication",
            ));
        }
        self.check_owner()?;
        if actual.has_shutdown_status() {
            return Err(invalid_request(
                "initial resume shut down before publication",
            ));
        }
        // Keep real Core birth separate from final loaded selection. Never let
        // an early loaded probe conceal a new Core entry whose birth is unknown.
        {
            let mut progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
            if progress.entered {
                if !progress
                    .created
                    .as_ref()
                    .is_some_and(|(captured_id, captured)| {
                        *captured_id == id && Arc::ptr_eq(captured, actual)
                    })
                {
                    return Err(internal_error(
                        "initial resume Core entry has no exact birth capture",
                    ));
                }
            } else {
                progress.resolved = Some((id, actual.clone()));
            }
        }
        if let Some(binding) = &self.binding {
            binding
                .capture(id, actual.clone())
                .map_err(internal_error)?;
        }
        self.bind_worktree(id, actual).await?;
        self.check_owner()?;
        let wire = self.wire_response(&response.thread.id, response.clone().into())?;
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .staged = Some(Reply::Resume(Arc::new(response), originator));
        Ok(wire)
    }
    pub(super) async fn bind_fork_listener(
        &self,
        context: ListenerTaskContext,
        id: ThreadId,
        actual: &Arc<CodexThread>,
    ) -> Result<(), JSONRPCErrorError> {
        self.ensure_primary(&context, id, actual).await?;
        let binding = self
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .task_binding
            .clone()
            .ok_or_else(|| internal_error("initial fork lost its actual child binding"))?;
        binding
            .bind_created(id, actual.clone())
            .await
            .map_err(|_| internal_error("initial fork actual task binding withdrew"))?;
        super::super::thread_lifecycle::alab_retained_listener::ensure_backend_start_listener(
            context,
            id,
            actual.clone(),
        )
        .await
    }
    pub(super) async fn ensure_primary(
        &self,
        context: &ListenerTaskContext,
        id: ThreadId,
        actual: &Arc<CodexThread>,
    ) -> Result<(), JSONRPCErrorError> {
        self.check_owner()?;
        // Only explicit initial/later UI construction creates a Scope. Both
        // Main and Side retain a provisional exact lease through real S2 UI;
        // task children use their separate backend targets, never this owner.
        if let Some(lease) = self
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retained_primary()
        {
            return if lease.is_for(actual) {
                Ok(())
            } else {
                Err(internal_error("initial primary runtime changed"))
            };
        }
        if !Arc::ptr_eq(&context.thread_manager, &self.processor.thread_manager) {
            return Err(internal_error(
                "primary listener belongs to another processor",
            ));
        }
        let lease = self
            .processor
            .alab_acquire_primary(id, actual.clone(), &self.input)
            .await?;
        self.check_owner()?;
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .primary = Some(lease);
        Ok(())
    }
    fn release_primary(&self) {
        let lease = self
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .primary
            .take();
        drop(lease); // Wake idle eligibility outside progress's lock after real owner shutdown.
    }
    async fn bind_worktree(
        &self,
        id: ThreadId,
        actual: &Arc<CodexThread>,
    ) -> Result<(), JSONRPCErrorError> {
        let Some(binding) = &self.binding else {
            return Ok(());
        };
        // Serialize the actual child binding/publication precondition against
        // the same unload/cold-resume lifecycle, not an extra policy permit.
        let _lifecycle = self
            .processor
            .thread_state_manager
            .lock_goal_resume(id)
            .await;
        self.check_owner()?;
        binding
            .bind_captured(self.processor.thread_manager.as_ref())
            .await
            .map_err(internal_error)?;
        if actual.has_shutdown_status()
            || !matches!(
                self.processor
                    .thread_manager
                    .thread_registration(id, actual)
                    .await,
                codex_core::ThreadRegistration::Current
            )
            || actual.has_shutdown_status()
        {
            return Err(invalid_request(
                "initial worktree runtime withdrew before publication",
            ));
        }
        self.check_owner()
    }

    pub(super) async fn stage_start(
        &self,
        id: ThreadId,
        actual: Arc<CodexThread>,
        response: ThreadStartResponse,
        originator: String,
    ) -> Result<(), JSONRPCErrorError> {
        self.capture(id, &actual)?;
        self.bind_worktree(id, &actual).await?;
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .staged = Some(Reply::Start(Arc::new(response), originator));
        Ok(())
    }
    pub(super) async fn stage_fork(
        &self,
        id: ThreadId,
        actual: &Arc<CodexThread>,
        response: &ThreadForkResponse,
        originator: &str,
    ) -> Result<(), JSONRPCErrorError> {
        self.capture(id, actual)?;
        self.bind_worktree(id, actual).await?;
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .staged = Some(Reply::Fork(Arc::new(response.clone()), originator.into()));
        Ok(())
    }
    pub(super) fn record_task_binding(
        &self,
        binding: crate::request_processors::PreparedChildBinding,
    ) -> Result<(), JSONRPCErrorError> {
        self.check_owner()?;
        let mut progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if progress.task_binding.is_some() {
            return Err(internal_error("initial task binding already recorded"));
        }
        progress.task_binding = Some(binding);
        Ok(())
    }
    /// Read only the actual completed Scope's binding observation for its
    /// exact captured/final-loaded Core. The host registry independently
    /// confirms that same Arc before publishing a protected projection.
    pub(super) fn task_tools_observed(
        &self,
        id: ThreadId,
        actual: &Arc<CodexThread>,
    ) -> Result<bool, JSONRPCErrorError> {
        self.check_owner()?;
        let progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if progress.worker_lost
            || !progress.worker_finished
            || (progress.entered && progress.created.is_none())
            || !matches!(
                progress.complete,
                Some(Reply::Start(..) | Reply::Fork(..) | Reply::Resume(..))
            )
            || !progress
                .created
                .as_ref()
                .or(progress.resolved.as_ref())
                .is_some_and(|(captured_id, captured)| {
                    *captured_id == id && Arc::ptr_eq(captured, actual)
                })
        {
            return Err(invalid_request(
                "task capability has no exact observed operation completion",
            ));
        }
        Ok(progress.task_binding.is_some() || progress.loaded_task_tools)
    }
    fn wire_response(
        &self,
        thread_id: &str,
        response: ClientResponsePayload,
    ) -> Result<ClientResponsePayload, JSONRPCErrorError> {
        use crate::outgoing_message::alab_initial_claim::CLIENT_RESULT_KEY;
        use crate::outgoing_message::alab_initial_claim::ClientConstructionResult;
        use crate::outgoing_message::alab_initial_claim::InitialSessionResult;
        use crate::outgoing_message::alab_initial_claim::RESULT_KEY;
        let (key, result) = {
            let progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
            let tools = progress.task_binding.is_some() || progress.loaded_task_tools;
            if let Some(claim) = self.frozen.claim() {
                (
                    RESULT_KEY,
                    serde_json::to_value(InitialSessionResult {
                        claim: claim.into(),
                        source_selection: self.prepared_offer.clone().ok_or_else(|| {
                            internal_error("initial result lost actual Source reservation")
                        })?,
                        thread_id: thread_id.into(),
                        history_fallback: progress.history_fallback,
                        dynamic_tools_fallback: progress.dynamic_tools_fallback,
                        backend_task_tools: tools,
                        primary_candidate: self
                            .frozen
                            .retains_candidate()
                            .then(|| self.candidate.clone()),
                    }),
                )
            } else {
                (
                    CLIENT_RESULT_KEY,
                    serde_json::to_value(ClientConstructionResult {
                        prepared: self.prepared_offer.clone().ok_or_else(|| {
                            internal_error("client result lost actual preparation")
                        })?,
                        thread_id: thread_id.into(),
                        history_fallback: progress.history_fallback,
                        dynamic_tools_fallback: progress.dynamic_tools_fallback,
                        backend_task_tools: tools,
                        primary_candidate: self
                            .frozen
                            .retains_candidate()
                            .then(|| self.candidate.clone()),
                    }),
                )
            }
        };
        let result =
            result.map_err(|_| internal_error("construction observation serialization failed"))?;
        codex_app_server_protocol::alab_response_metadata::decorate(response, key, result)
            .map_err(|_| internal_error("construction result metadata serialization failed"))
    }
    pub(super) fn start_wire(
        &self,
        response: &ThreadStartResponse,
    ) -> Result<ClientResponsePayload, JSONRPCErrorError> {
        self.wire_response(&response.thread.id, response.clone().into())
    }
    pub(super) fn fork_wire(
        &self,
        response: &ThreadForkResponse,
    ) -> Result<ClientResponsePayload, JSONRPCErrorError> {
        self.wire_response(&response.thread.id, response.clone().into())
    }
    pub(super) fn stage_error(&self, error: JSONRPCErrorError) {
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .staged = Some(Reply::Error(error));
    }
    fn can_offer_selection(&self) -> bool {
        let progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        progress.may_offer_selection()
            && (self.registered_selection.is_none() || progress.selection_retired)
    }
    fn error_wire(&self, error: &JSONRPCErrorError) -> JSONRPCErrorError {
        let progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        if progress.next_selection.is_some()
            && let Some(Reply::Error(wire)) = &progress.complete
        {
            return wire.clone();
        }
        error.clone()
    }
    fn no_core(&self) -> bool {
        let progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        !progress.entered && progress.created.is_none() && progress.resolved.is_none()
    }
    async fn run(self: &Arc<Self>) -> Reply {
        if let Some(error) = &self.preparation_error {
            return Reply::Error(error.clone());
        }
        if let Some(registered) = &self.registered_selection {
            match registered.begin().await {
                Ok(held) => {
                    self.progress
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .held_selection = Some(Arc::new(held));
                }
                Err(_) => {
                    // Shutdown may deny before SAME SDK Registered record has
                    // begun. Only its genuine phase-serialized Native cancel
                    // can establish normal no-Core failure. Inhibiting/Held/
                    // Unknown refuses retirement and retains uncertainty.
                    if registered.retire_unaccepted().await.is_ok() {
                        self.progress
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .selection_retired = true;
                        return Reply::Error(internal_error(
                            "accepted Source transition could not begin",
                        ));
                    }
                    self.progress
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .selection_unknown = true;
                    self.input.fail(std::io::Error::other(
                        "accepted Source transition outcome unobserved",
                    ));
                    return Reply::Unknown;
                }
            }
        }

        let mut arguments = self.arguments.clone();
        let metadata = &self.metadata;
        loop {
            let result = match &arguments {
                Arguments::Start(params) => {
                    Box::pin(self.processor.thread_start_inner(
                        alab_task_start::StartTarget::Initial(self.clone()),
                        params.clone(),
                        metadata.name.clone(),
                        metadata.version.clone(),
                        metadata.extensions.clone(),
                        alab_task_start::StartContext::Client(self.context.clone()),
                    ))
                    .await
                }
                Arguments::Fork(params) => {
                    Box::pin(self.processor.thread_fork_inner(
                        ForkTarget::Initial(self.clone()),
                        params.clone(),
                        metadata.name.clone(),
                        metadata.version.clone(),
                        metadata.extensions.clone(),
                    ))
                    .await
                }
                Arguments::Resume(params) => Box::pin(self.processor.thread_resume(
                    ThreadResumeTarget::Client(
                        super::alab_initial_resume::ResumeClientTarget::retained(self.clone()),
                    ),
                    params.clone(),
                    metadata.name.clone(),
                    metadata.version.clone(),
                    metadata.extensions.clone(),
                ))
                .await
                .map(|_| ()),
            };
            if let Err(error) = result {
                self.stage_error(error);
            }
            let reply = self
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .staged
                .clone()
                .unwrap_or_else(|| {
                    Reply::Error(internal_error("initial session returned no typed result"))
                });
            if let Reply::Error(error) = &reply {
                // The original helper returned normally and its tracked START
                // task was joined. Only exact original compatibility cases with
                // measured no entry/capture may repeat that original policy.
                if self.no_core() {
                    let retry = match &mut arguments {
                        Arguments::Start(params) if fallback::start_history(params, error) => {
                            params.history_mode = None;
                            self.progress
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .history_fallback = true;
                            true
                        }
                        Arguments::Start(params) if fallback::start_dynamic(params, error) => {
                            params.dynamic_tools = None;
                            self.progress
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .dynamic_tools_fallback = true;
                            true
                        }
                        Arguments::Fork(params) if fallback::fork_history(params, error) => {
                            params.exclude_turns = false;
                            self.progress
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .history_fallback = true;
                            true
                        }
                        Arguments::Resume(params) if fallback::resume_history(params, error) => {
                            params.exclude_turns = false;
                            self.progress
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .history_fallback = true;
                            true
                        }
                        _ => false,
                    };
                    if retry {
                        self.progress
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .staged = None;
                        continue;
                    }
                }
            }
            return reply;
        }
    }
    async fn finish(&self, reply: Reply) {
        let (reply, replays, abandoned) = match self.operation.upgrade() {
            Some(operation) => operation.complete_reply(self, reply),
            None => {
                let mut progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
                let reply = progress.complete.get_or_insert(reply).clone();
                (reply, std::mem::take(&mut progress.replays), None)
            }
        };
        drop(abandoned); // Actual resource destruction leaves both mutexes first.
        if matches!(reply, Reply::Error(_) | Reply::Unknown) {
            publish(self, &self.context, &reply).await;
        }
        for context in replays {
            self.publish_replay(&context, &reply).await;
        }
    }

    async fn publish_replay(&self, context: &RequestContext, reply: &Reply) {
        let result = async {
            if !matches!(reply, Reply::Start(..) | Reply::Fork(..) | Reply::Resume(..)) { return Ok(()); }
            self.check_owner()?;
            let (id, actual) = {
                let progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
                if progress.entered && progress.created.is_none() {
                    return Err(internal_error("initial replay Core birth remains unobserved"));
                }
                progress.created.clone().or_else(|| progress.resolved.clone())
                    .ok_or_else(|| internal_error("initial replay lost its actual Core runtime"))?
            };
            let raw = match &self.arguments {
                Arguments::Start(params) => params.experimental_raw_events,
                Arguments::Fork(_) | Arguments::Resume(_) => false,
            };
            // Use the exact captured Arc in the original serialized listener
            // body. A new incarnation cannot be selected by a ThreadId lookup.
            match super::super::thread_lifecycle::alab_retained_listener::subscribe_initial_runtime(
                self.processor.listener_task_context(), id, actual.clone(),
                context.alab_request_id().connection_id, raw,
            ).await? {
                super::super::thread_lifecycle::EnsureConversationListenerResult::Attached => {},
                super::super::thread_lifecycle::EnsureConversationListenerResult::ConnectionClosed => {
                    return Err(invalid_request("initial replay connection closed"));
                }
            }
            self.check_owner()?;
            if actual.has_shutdown_status() || !matches!(self.processor.thread_manager
                .thread_registration(id, &actual).await, codex_core::ThreadRegistration::Current)
                || actual.has_shutdown_status() {
                return Err(invalid_request("initial replay runtime withdrew"));
            }
            self.check_owner()?;
            // This rechecks ONLY a replay observer after awaited attachment.
            // It never cancels or retries the accepted original C1 operation.
            let origin = context.alab_connection_resource.as_ref()
                .and_then(codex_alab_core::connection_resource::ConnectionResource::downcast::<AcceptedOrigin>)
                .ok_or_else(|| invalid_request("initial replay lost actual Native origin"))?;
            origin.check_current().await.map_err(|_| invalid_request("initial replay native origin is no longer current"))?;
            if actual.has_shutdown_status() || !matches!(self.processor.thread_manager
                .thread_registration(id, &actual).await, codex_core::ThreadRegistration::Current)
                || actual.has_shutdown_status() {
                return Err(invalid_request("initial replay runtime withdrew"));
            }
            self.check_owner()
        }.await;
        match result {
            Ok(()) => publish(self, context, reply).await,
            Err(error) => {
                self.processor
                    .outgoing
                    .send_error(context.alab_request_id().clone(), error)
                    .await
            }
        }
    }
}
async fn publish(scope: &Scope, context: &RequestContext, reply: &Reply) {
    let outgoing = &scope.processor.outgoing;
    let id = context.alab_request_id().clone();
    match reply {
        Reply::Start(response, originator) => match scope.start_wire(response) {
            Ok(wire) => {
                outgoing
                    .send_response_with_thread_originator(id, wire, originator.clone())
                    .await
            }
            Err(error) => outgoing.send_error(id, error).await,
        },
        Reply::Fork(response, originator) => match scope.fork_wire(response) {
            Ok(wire) => {
                outgoing
                    .send_response_with_thread_originator(id, wire, originator.clone())
                    .await
            }
            Err(error) => outgoing.send_error(id, error).await,
        },
        Reply::Resume(response, originator) => {
            match scope.wire_response(&response.thread.id, response.as_ref().clone().into()) {
                Ok(wire) => {
                    outgoing
                        .send_response_with_thread_originator(id, wire, originator.clone())
                        .await
                }
                Err(error) => outgoing.send_error(id, error).await,
            }
        }
        Reply::Error(error) => outgoing.send_error(id, scope.error_wire(error)).await,
        Reply::Unknown => {
            outgoing
                .send_error(
                    id,
                    internal_error("initial session outcome remains unobserved"),
                )
                .await
        }
    }
}

impl ThreadRequestProcessor {
    pub(crate) fn alab_install_initial_session(
        &self,
        actual: &Arc<InitialSessionOperation>,
    ) -> std::io::Result<()> {
        self.alab_initial_session.install(actual)
    }
    pub(crate) fn alab_initial_slot(&self) -> InitialSlot {
        self.alab_initial_session.clone()
    }
    pub(crate) fn alab_stop_initial_session(&self) {
        self.alab_initial_session.stop();
    }
}

#[cfg(test)]
#[path = "initial_session_tests.rs"]
mod tests;

#[cfg(all(test, unix))]
#[path = "initial_selection_publication_tests.rs"]
mod publication_tests;

#[cfg(all(test, unix))]
#[path = "client_start_policy_controls.rs"]
mod client_start_policy_controls;
