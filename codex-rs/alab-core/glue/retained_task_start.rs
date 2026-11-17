// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Direct backend-owned thread START using the existing full startup path.
//! This component does not implement create_thread's initial turn or source
//! inheritance, and does not grant MCP, SDK or frontend authority.

use super::*;
#[cfg(unix)]
use codex_alab_core::native_incoming::Incoming;
use codex_core::ThreadRegistration;
use futures::FutureExt;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::sync::PoisonError;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::Span;

/// Actual host/tool-operation context, independent of a frontend RPC identity.
/// Supply the issuing operation's captured values; this creates no request ID.
pub(crate) struct OperationContext {
    pub(crate) span: Span,
    pub(crate) parent_trace: Option<W3cTraceContext>,
    pub(crate) cancellation: CancellationToken,
}

pub(super) enum StartContext {
    Client(RequestContext),
    Backend(OperationContext),
}
impl StartContext {
    pub(super) fn request_trace(&self) -> Option<W3cTraceContext> {
        match self {
            Self::Client(context) => context.request_trace(),
            Self::Backend(context) => codex_otel::span_w3c_trace_context(&context.span)
                .or_else(|| context.parent_trace.clone()),
        }
    }
    pub(super) fn span(&self) -> Span {
        match self {
            Self::Client(context) => context.span(),
            Self::Backend(context) => context.span.clone(),
        }
    }
}

/// Complete inputs already assembled by the issuing task operation. The caller
/// must retain normal source inheritance and approval policy; this is not a raw
/// MCP-argument adapter or a substitute for a live invocation check.
pub(crate) struct BackendStartRequest {
    #[cfg(unix)]
    pub(crate) task_binding: Option<crate::request_processors::PreparedChildBinding>,
    pub(crate) parent_id: ThreadId,
    pub(crate) parent: Arc<CodexThread>,
    pub(crate) params: ThreadStartParams,
    pub(crate) app_server_client_name: Option<String>,
    pub(crate) app_server_client_version: Option<String>,
    pub(crate) client_mcp_extensions: ClientMcpExtensions,
    pub(crate) context: OperationContext,
}

pub(crate) struct StartedThread {
    pub(crate) thread: Arc<CodexThread>,
    pub(crate) response: ThreadStartResponse,
}

impl std::fmt::Debug for StartedThread {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StartedThread")
            .field("thread_id", &self.response.thread.id)
            .finish_non_exhaustive()
    }
}

/// Source startup observation only; no variant certifies Native/effect closure.
pub(crate) enum StartCustody {
    /// The normally terminated helper never entered this Core-start path.
    /// Earlier validation/config work can still have had effects.
    NotStarted,
    /// The normally terminated helper captured this exact runtime.
    Captured(Arc<CodexThread>),
    /// Entry/termination is unresolved. The live shared slot retains late writes.
    Unknown,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HelperCompletion {
    Pending,
    Joined,
    Lost,
}

struct CreationState {
    core_entered: bool,
    created: Option<Arc<CodexThread>>,
    helper: HelperCompletion,
}

pub(crate) struct StartFailure {
    pub(crate) error: JSONRPCErrorError,
    /// A post-birth failure retains the original runtime, never a PID lookup or
    /// a claim that construction had no effects. The backend owns cleanup.
    creation: Arc<std::sync::Mutex<CreationState>>,
}
impl std::fmt::Debug for StartFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StartFailure")
            .field("code", &self.error.code)
            .field("created", &self.created().is_some())
            .finish_non_exhaustive()
    }
}
impl StartFailure {
    /// None means no runtime has been captured yet, not proof of no birth.
    /// If coordinator loss precedes a tracked startup's completion, this exact
    /// slot can later expose that original runtime without a thread-ID lookup.
    pub(crate) fn created(&self) -> Option<Arc<CodexThread>> {
        self.creation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .created
            .clone()
    }

    pub(crate) fn custody(&self) -> StartCustody {
        let creation = self.creation.lock().unwrap_or_else(PoisonError::into_inner);
        if creation.helper != HelperCompletion::Joined {
            return StartCustody::Unknown;
        }
        match &creation.created {
            Some(thread) => StartCustody::Captured(thread.clone()),
            None if !creation.core_entered => StartCustody::NotStarted,
            None => StartCustody::Unknown,
        }
    }
}
pub(crate) type StartResult = Result<Arc<StartedThread>, Arc<StartFailure>>;
type StagedResult = Result<Arc<StartedThread>, JSONRPCErrorError>;

pub(super) struct Completion {
    staged: std::sync::Mutex<Option<StagedResult>>,
    creation: Arc<std::sync::Mutex<CreationState>>,
    warnings: std::sync::Mutex<Vec<ConfigWarningNotification>>,
    finished: watch::Sender<Option<StartResult>>,
}
impl Completion {
    fn new() -> Self {
        let (finished, _) = watch::channel(None);
        Self {
            staged: std::sync::Mutex::new(None),
            creation: Arc::new(std::sync::Mutex::new(CreationState {
                core_entered: false,
                created: None,
                helper: HelperCompletion::Pending,
            })),
            warnings: std::sync::Mutex::new(Vec::new()),
            finished,
        }
    }
    fn failure(&self, error: JSONRPCErrorError) -> Arc<StartFailure> {
        Arc::new(StartFailure {
            error,
            creation: self.creation.clone(),
        })
    }
    fn stage(&self, result: StagedResult) {
        let mut staged = self.staged.lock().unwrap_or_else(PoisonError::into_inner);
        if staged.is_none() && self.finished.borrow().is_none() {
            *staged = Some(result);
        }
    }
    fn finish(&self, result: StartResult, helper: HelperCompletion) {
        // Registration and terminal publication share the same synchronous
        // mutex. A late worker cannot stage another runtime after this clear.
        let mut staged = self.staged.lock().unwrap_or_else(PoisonError::into_inner);
        staged.take();
        // Entry, exact runtime and terminal helper observation are read together.
        // Publish Joined only after the actual helper returned and was destroyed.
        let mut creation = self.creation.lock().unwrap_or_else(PoisonError::into_inner);
        let mut result = Some(result);
        self.finished.send_if_modified(|stored| {
            if stored.is_some() {
                return false;
            }
            creation.helper = helper;
            if result.as_ref().is_some_and(Result::is_ok) {
                creation.created.take();
            }
            *stored = result.take();
            true
        });
    }
}

/// Constructed before spawn, so even an unpolled or aborted coordinator stores
/// a terminal error. Waiters do not own a liveness sender without a drop path.
struct CoordinatorGuard {
    completion: Arc<Completion>,
    armed: bool,
}
impl CoordinatorGuard {
    fn finish(mut self, result: StartResult, helper: HelperCompletion) {
        self.completion.finish(result, helper);
        self.armed = false;
    }
}
impl Drop for CoordinatorGuard {
    fn drop(&mut self) {
        if self.armed {
            self.completion.finish(
                Err(self.completion.failure(internal_error(
                    "backend startup coordinator dropped; tracked child outcome may still exist",
                ))),
                HelperCompletion::Lost,
            );
        }
    }
}

/// Waiting can be canceled without discarding the backend's owned startup work.
/// Completion means the startup helper joined, not native-effect settlement.
#[derive(Clone)]
pub(crate) struct BackendStart {
    completion: Arc<Completion>,
}
impl BackendStart {
    pub(crate) async fn wait(&self) -> StartResult {
        let mut finished = self.completion.finished.subscribe();
        loop {
            let result = finished.borrow_and_update().clone();
            if let Some(result) = result {
                return result;
            }
            if finished.changed().await.is_err() {
                return Err(self
                    .completion
                    .failure(internal_error("backend startup completion unavailable")));
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn warnings(&self) -> Vec<ConfigWarningNotification> {
        self.completion
            .warnings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

pub(super) struct BackendScope {
    #[cfg(unix)]
    task_binding: Option<crate::request_processors::PreparedChildBinding>,
    #[cfg(unix)]
    incoming: Incoming,
    parent_id: ThreadId,
    parent: Arc<CodexThread>,
    cancellation: CancellationToken,
}

impl BackendScope {
    fn input_stopped(&self) -> bool {
        #[cfg(unix)]
        {
            self.incoming.is_stopped()
        }
        #[cfg(not(unix))]
        {
            true
        }
    }
}

#[derive(Clone)]
pub(super) enum StartTarget {
    Client(ConnectionRequestId),
    #[cfg(unix)]
    Initial(Arc<super::alab_initial_session::Scope>),
    Backend {
        scope: Arc<BackendScope>,
        completion: Arc<Completion>,
    },
}
impl StartTarget {
    pub(super) async fn check_before_policy(
        &self,
        manager: &ThreadManager,
    ) -> Result<(), JSONRPCErrorError> {
        #[cfg(unix)]
        if let Self::Initial(scope) = self {
            scope.check_owner()?;
        }
        if let Self::Backend { scope, .. } = self {
            if scope.input_stopped() || scope.cancellation.is_cancelled() {
                return Err(internal_error(
                    "backend startup owner withdrew before construction",
                ));
            }
            if scope.parent.has_shutdown_status()
                || !matches!(
                    manager
                        .thread_registration(scope.parent_id, &scope.parent)
                        .await,
                    ThreadRegistration::Current
                )
            {
                return Err(invalid_request(
                    "backend startup parent is no longer registered",
                ));
            }
        }
        Ok(())
    }

    pub(super) async fn warning(
        &self,
        context: &ListenerTaskContext,
        notification: ConfigWarningNotification,
    ) {
        match self {
            Self::Client(request) => {
                context
                    .outgoing
                    .send_server_notification_to_connections(
                        &[request.connection_id],
                        ServerNotification::ConfigWarning(notification),
                    )
                    .await
            }
            #[cfg(unix)]
            Self::Initial(scope) => {
                context
                    .outgoing
                    .send_server_notification_to_connections(
                        &[scope.request().connection_id],
                        ServerNotification::ConfigWarning(notification),
                    )
                    .await;
            }
            Self::Backend { completion, .. } => {
                completion
                    .warnings
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(notification);
                // Keep backend warnings with this operation. A future UI projection
                // must choose its actual recipients; an empty slice broadcasts.
            }
        }
    }

    pub(super) async fn before_start(
        &self,
        context: &ListenerTaskContext,
        store: &dyn ThreadStore,
        thread_id: ThreadId,
        reserved: Option<ThreadId>,
    ) -> Result<(), JSONRPCErrorError> {
        if let Err(error) = self
            .check_before_policy(context.thread_manager.as_ref())
            .await
        {
            remove_pending_thread_metadata(store, reserved).await;
            return Err(error);
        }
        let connections = match self {
            Self::Client(request) => vec![request.connection_id],
            #[cfg(unix)]
            Self::Initial(scope) => vec![scope.request().connection_id],
            Self::Backend { .. } => Vec::new(),
        };
        // Preserve prewarm registration for the actual requesting client. The
        // backend target has no client subscription; attestation may be absent
        // under the existing no-recipient policy. It invents no subscriber.
        for connection in connections {
            context
                .thread_state_manager
                .try_add_connection_to_thread(thread_id, connection)
                .await;
        }
        #[cfg(unix)]
        if let Self::Initial(scope) = self {
            scope.before_core()?;
        }
        if let Self::Backend { completion, .. } = self {
            // No await separates this conservative entry mark from returning
            // to the caller's real Core start. Failure after it stays Unknown
            // unless the exact resulting runtime was actually captured.
            completion
                .creation
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .core_entered = true;
        }
        Ok(())
    }

    pub(super) fn retain_created(
        &self,
        id: ThreadId,
        thread: &Arc<CodexThread>,
    ) -> Result<(), JSONRPCErrorError> {
        #[cfg(unix)]
        if let Self::Initial(scope) = self {
            scope.capture(id, thread)?;
        }
        #[cfg(not(unix))]
        let _ = id;
        if let Self::Backend { completion, .. } = self {
            completion
                .creation
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .created = Some(thread.clone());
        }
        Ok(())
    }

    async fn check_created(
        &self,
        context: &ListenerTaskContext,
        id: ThreadId,
        thread: &Arc<CodexThread>,
    ) -> Result<(), JSONRPCErrorError> {
        let mut retained = matches!(self, Self::Backend { .. });
        #[cfg(unix)]
        {
            retained |= matches!(self, Self::Initial(_));
        }
        if retained {
            if matches!(self, Self::Backend { .. }) {
                self.check_before_policy(context.thread_manager.as_ref())
                    .await?;
            }
            if thread.has_shutdown_status()
                || !matches!(
                    context.thread_manager.thread_registration(id, thread).await,
                    ThreadRegistration::Current
                )
            {
                return Err(invalid_request(
                    "created backend thread is no longer registered",
                ));
            }
            if thread.has_shutdown_status() {
                return Err(invalid_request("created backend thread shut down"));
            }
        }
        Ok(())
    }

    pub(super) async fn attach_listener(
        &self,
        context: &ListenerTaskContext,
        thread_id: ThreadId,
        thread: Arc<CodexThread>,
        raw_events: bool,
    ) -> Result<(), JSONRPCErrorError> {
        match self {
            Self::Client(request) => {
                log_listener_attach_result(
                    super::super::thread_lifecycle::ensure_conversation_listener(
                        context.clone(),
                        thread_id,
                        request.connection_id,
                        raw_events,
                    )
                    .instrument(tracing::info_span!(
                        "app_server.thread_start.attach_listener",
                        otel.name = "app_server.thread_start.attach_listener",
                        thread_start.experimental_raw_events = raw_events,
                    ))
                    .await,
                    thread_id,
                    request.connection_id,
                    "thread",
                );
                Ok(())
            }
            #[cfg(unix)]
            Self::Initial(scope) => {
                self.check_created(context, thread_id, &thread).await?;
                scope.ensure_primary(context, thread_id, &thread).await?;
                let request = scope.request();
                log_listener_attach_result(
                    super::super::thread_lifecycle::ensure_conversation_listener(
                        context.clone(),
                        thread_id,
                        request.connection_id,
                        raw_events,
                    )
                    .await,
                    thread_id,
                    request.connection_id,
                    "thread",
                );
                super::super::thread_lifecycle::alab_retained_listener::ensure_backend_start_listener(
                    context.clone(), thread_id, thread.clone()).await?;
                self.check_created(context, thread_id, &thread).await
            }
            Self::Backend { .. } => {
                self.check_created(context, thread_id, &thread).await?;
                super::super::thread_lifecycle::alab_retained_listener::ensure_backend_start_listener(
                    context.clone(),
                    thread_id,
                    thread.clone(),
                )
                .await?;
                self.check_created(context, thread_id, &thread).await
            }
        }
    }

    pub(super) async fn publish_response(
        &self,
        context: &ListenerTaskContext,
        id: ThreadId,
        response: ThreadStartResponse,
        originator: String,
        thread: Arc<CodexThread>,
    ) -> Result<(), JSONRPCErrorError> {
        self.check_created(context, id, &thread).await?;
        let outgoing = context.outgoing.as_ref();
        match self {
            Self::Client(request) => {
                outgoing
                    .send_response_with_thread_originator(request.clone(), response, originator)
                    .instrument(tracing::info_span!(
                        "app_server.thread_start.send_response",
                        otel.name = "app_server.thread_start.send_response",
                    ))
                    .await
            }
            #[cfg(unix)]
            Self::Initial(scope) => {
                scope
                    .stage_start(id, thread, response.clone(), originator.clone())
                    .await?;
                let wire = scope.start_wire(&response)?;
                outgoing
                    .send_response_with_thread_originator(scope.request().clone(), wire, originator)
                    .instrument(tracing::info_span!(
                        "app_server.thread_start.send_response",
                        otel.name = "app_server.thread_start.send_response"
                    ))
                    .await;
            }
            Self::Backend { completion, .. } => {
                completion.stage(Ok(Arc::new(StartedThread { thread, response })))
            }
        }
        Ok(())
    }

    pub(super) async fn publish_started(
        &self,
        outgoing: &OutgoingMessageSender,
        notification: codex_app_server_protocol::ThreadStartedNotification,
    ) {
        let client = match self {
            Self::Client(_) => true,
            #[cfg(unix)]
            Self::Initial(_) => true,
            Self::Backend { .. } => false,
        };
        if client {
            outgoing
                .send_server_notification(ServerNotification::ThreadStarted(notification))
                .instrument(tracing::info_span!(
                    "app_server.thread_start.notify_started",
                    otel.name = "app_server.thread_start.notify_started",
                ))
                .await;
        }
        // Backend completion already owns the typed thread result. Publishing
        // a later UI projection is independent; a full outbox cannot gate START.
    }

    pub(super) async fn publish_error(
        &self,
        outgoing: &OutgoingMessageSender,
        error: JSONRPCErrorError,
    ) {
        match self {
            Self::Client(request) => outgoing.send_error(request.clone(), error).await,
            #[cfg(unix)]
            Self::Initial(scope) => scope.stage_error(error),
            Self::Backend { completion, .. } => completion.stage(Err(error)),
        }
    }
}

/// Construct from the actual retained processor and same input allocation.
/// This executor is held by the backend service, never a frontend session.
#[cfg(unix)]
#[derive(Clone)]
pub(crate) struct BackendStartExecutor {
    processor: ThreadRequestProcessor,
    incoming: Incoming,
}
#[cfg(unix)]
impl ThreadRequestProcessor {
    pub(crate) fn alab_backend_start_executor(&self, incoming: Incoming) -> BackendStartExecutor {
        BackendStartExecutor {
            processor: self.clone(),
            incoming,
        }
    }
}
#[cfg(unix)]
impl BackendStartExecutor {
    pub(crate) fn begin(
        &self,
        request: BackendStartRequest,
    ) -> Result<BackendStart, JSONRPCErrorError> {
        tokio::runtime::Handle::try_current()
            .map_err(|_| internal_error("backend startup requires the host runtime"))?;
        if self.incoming.is_stopped() {
            return Err(internal_error("backend startup owner withdrew"));
        }
        let BackendStartRequest {
            task_binding,
            parent_id,
            parent,
            params,
            app_server_client_name,
            app_server_client_version,
            client_mcp_extensions,
            context,
        } = request;
        let completion = Arc::new(Completion::new());
        let scope = Arc::new(BackendScope {
            task_binding,
            incoming: self.incoming.clone(),
            parent_id,
            parent,
            cancellation: context.cancellation.clone(),
        });
        let target = StartTarget::Backend {
            scope,
            completion: completion.clone(),
        };
        let processor = self.processor.clone();
        let finished = completion.clone();
        let guard = CoordinatorGuard {
            completion: completion.clone(),
            armed: true,
        };
        // Both this coordinator and the existing startup worker are retained by
        // the host's original TaskTracker. Dropping a waiter aborts neither.
        self.processor.background_tasks.spawn(async move {
            let mut work = Box::pin(
                AssertUnwindSafe(processor.thread_start_inner(
                    target,
                    params,
                    app_server_client_name,
                    app_server_client_version,
                    client_mcp_extensions,
                    StartContext::Backend(context),
                ))
                .catch_unwind(),
            );
            let outcome = work.as_mut().await;
            let destruction = catch_unwind(AssertUnwindSafe(|| drop(work)));
            // A normal helper return includes its awaited worker JoinHandle,
            // even when that observed JoinError was mapped into a typed Err.
            // This says nothing about panic absence or prior Native effects.
            let helper = if destruction.is_ok() && outcome.is_ok() {
                HelperCompletion::Joined
            } else {
                HelperCompletion::Lost
            };
            let result = if destruction.is_err() {
                Err(finished.failure(internal_error(
                    "backend startup coordinator destruction panicked; child outcome may exist",
                )))
            } else {
                match outcome {
                    Ok(Ok(())) => {
                        let staged = finished
                            .staged
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .take()
                            .unwrap_or_else(|| {
                                Err(internal_error(
                                    "backend startup produced no typed completion",
                                ))
                            });
                        staged.map_err(|error| finished.failure(error))
                    }
                    Ok(Err(error)) => Err(finished.failure(error)),
                    Err(_) => Err(finished.failure(internal_error(
                        "backend startup coordinator panicked; child outcome may exist",
                    ))),
                }
            };
            // Only successful typed completion discards the staging slot. A
            // failure retains the exact shared child slot, including late birth
            // after coordinator loss; it never proves child absence.
            guard.finish(result, helper);
        });
        Ok(BackendStart { completion })
    }
}

#[cfg(all(test, unix))]
#[path = "retained_task_start_tests.rs"]
pub(crate) mod tests;

#[path = "retained_task_mcp_start.rs"]
mod mcp_start;
pub(super) use mcp_start::TaskBindingPlan;
pub(super) use mcp_start::TaskHostSlot;

// ALAB(upgrade): complete backend CREATE consumes the genuine typed START owner.
#[cfg(unix)]
#[path = "retained_task_create.rs"]
pub(crate) mod create_operation;
