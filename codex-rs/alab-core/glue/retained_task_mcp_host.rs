// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Backend-owned approval-gated task MCP service. Actual native input custody
//! controls lifetime; neither HTTP metadata nor discovery grants execution.
#![cfg(unix)]
use super::*;
use crate::request_processors::OperationContext;
use crate::request_processors::retained_task_create::BackendCreateExecutor;
use crate::request_processors::retained_task_create::CreateOutcome;
use crate::request_processors::retained_task_create::CreateRequest;
use crate::request_processors::retained_task_create::CreateWork;
use crate::request_processors::retained_task_create::create_tool_spec;
use crate::request_processors::retained_task_create::parse_arguments;
use crate::request_processors::retained_task_read;
use crate::request_processors::retained_task_read::BackendReadExecutor;
use crate::request_processors::retained_task_read::management::BackendManagementExecutor;
use crate::request_processors::retained_task_read::management::ManagementRequest;
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::Request;
use axum::http::StatusCode;
use axum::http::header::AUTHORIZATION;
use axum::response::IntoResponse;
use axum::response::Response;
use codex_alab_core::native_incoming::Incoming;
use codex_app_server_protocol::DynamicToolFunctionSpec;
use codex_app_server_protocol::DynamicToolSpec;
use codex_core::CodexThread;
use codex_core::ThreadManager;
use codex_core::ThreadRegistration;
use codex_core::config::Config;
use codex_protocol::ThreadId;
use futures::FutureExt;
use rmcp::ErrorData as McpError;
use rmcp::handler::server::ServerHandler;
use rmcp::model::CallToolRequestParams;
use rmcp::model::CallToolResult;
use rmcp::model::ContentBlock;
use rmcp::model::JsonObject;
use rmcp::model::ListToolsResult;
use rmcp::model::PaginatedRequestParams;
use rmcp::model::ServerCapabilities;
use rmcp::model::ServerConfig;
use rmcp::model::Tool;
use rmcp::model::ToolAnnotations;
use rmcp::service::RequestContext as McpRequestContext;
use rmcp::service::RoleServer;
use rmcp::transport::StreamableHttpServerConfig;
use rmcp::transport::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use serde_json::Value;
use serde_json::json;
use std::borrow::Cow;
use std::collections::HashMap;
use std::future::IntoFuture;
use std::io;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::sync::Arc;
use std::sync::PoisonError;
use std::sync::Weak;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use uuid::Uuid;

#[path = "retained_task_mcp_binding.rs"]
mod binding;
pub(crate) use binding::ConstructionCapture;
pub(crate) use binding::ForkConstruction;
pub(crate) use binding::PreparedChildBinding;
pub(crate) use binding::ResumeConstruction;
pub(crate) use binding::StartCapture;
use binding::ThreadBinding;

const MAX_BINDINGS: usize = 4096;
const MAX_ACCEPTED: usize = 4096;

struct Registry {
    closed: bool,
    bindings: HashMap<
        Uuid,
        (
            Arc<ThreadBinding>,
            StreamableHttpService<Handler, LocalSessionManager>,
        ),
    >,
    operations: HashMap<Uuid, Arc<AcceptedCreate>>,
    management: HashMap<Uuid, Arc<management::AcceptedManagement>>,
    sends: HashMap<Uuid, Arc<send::AcceptedSend>>,
    fork_calls: HashMap<Uuid, origin::ForkEntry>,
    worktree_jobs: HashMap<Uuid, Arc<worktree::AcceptedWorktree>>,
}

pub(crate) struct HostControl {
    incoming: Incoming,
    manager: Arc<ThreadManager>,
    create: BackendCreateExecutor,
    reads: BackendReadExecutor,
    management: BackendManagementExecutor,
    sends: crate::request_processors::retained_task_send::BackendSendExecutor,
    forks: crate::request_processors::retained_task_fork::BackendForkExecutor,
    worktrees: std::sync::OnceLock<
        crate::request_processors::retained_task_worktree::BackendWorktreeExecutor,
    >,
    initial_session: std::sync::OnceLock<crate::request_processors::InitialSessionSlot>,
    address: std::net::SocketAddr,
    registry: std::sync::Mutex<Registry>,
    operations: TaskTracker,
    monitors: TaskTracker,
    shutdown: CancellationToken,
    http_shutdown: CancellationToken,
    #[cfg(test)]
    test_last_create_error: std::sync::Mutex<Option<String>>,
    #[cfg(test)]
    test_fork_gate: std::sync::Mutex<Option<Arc<fork::TestForkGate>>>,
    #[cfg(test)]
    test_worktree_gate: std::sync::Mutex<Option<Arc<worktree::TestWorktreeGate>>>,
    #[cfg(test)]
    test_worktree_blocking_gate: std::sync::Mutex<Option<Arc<worktree::TestWorktreeBlockingGate>>>,
}

/// Keep in Prepared until actual call drainage, Core shutdown, and final join.
/// Dropping a frontend does not own or drop this host.
pub(crate) struct OwnedTaskMcpHost {
    control: Arc<HostControl>,
    serve: Option<tokio::task::JoinHandle<io::Result<()>>>,
}

#[derive(Clone)]
enum Observation {
    Complete(Result<Arc<CreateOutcome>, Arc<String>>),
    Unobserved,
}

struct AcceptedCreate {
    // Separate from the polled recipe future, so cancellation/panic cannot
    // discard actual START/created-runtime custody or an uncertain turn.
    work: std::sync::Mutex<Option<CreateWork>>,
    restored: tokio::sync::Notify,
    cancellation: CancellationToken,
    completion: tokio::sync::watch::Sender<Option<Observation>>,
}
impl AcceptedCreate {
    async fn lease(&self) -> WorkLease<'_> {
        loop {
            let restored = self.restored.notified();
            tokio::pin!(restored);
            restored.as_mut().enable();
            let work = {
                let mut slot = self.work.lock().unwrap_or_else(PoisonError::into_inner);
                slot.take()
            };
            if let Some(work) = work {
                return WorkLease {
                    operation: self,
                    work: Some(work),
                };
            }
            // None means this exact Work is leased, never no-effect/no-child.
            restored.await;
        }
    }

    fn finish(&self, observation: Observation) {
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

/// Own the actual recipe across await without holding a mutex guard. The slot
/// is private: only constructor/lease/Drop write it, so one lease exclusively
/// owns the one missing value until synchronous restoration.
struct WorkLease<'a> {
    operation: &'a AcceptedCreate,
    work: Option<CreateWork>,
}
impl WorkLease<'_> {
    fn work(&mut self) -> io::Result<&mut CreateWork> {
        self.work
            .as_mut()
            .ok_or_else(|| io::Error::other("accepted work lease unavailable"))
    }
}
impl Drop for WorkLease<'_> {
    fn drop(&mut self) {
        if let Some(work) = self.work.take() {
            {
                let mut slot = self
                    .operation
                    .work
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                *slot = Some(work);
            }
            self.operation.restored.notify_waiters();
        }
    }
}

/// Created before spawn; even unpolled runtime destruction releases observers
/// with uncertainty while the registry retains Work. None is not no-effect.
struct OperationGuard {
    operation: Arc<AcceptedCreate>,
    armed: bool,
}
impl OperationGuard {
    fn finish(mut self, observation: Observation) {
        self.operation.finish(observation);
        self.armed = false;
    }
}
impl Drop for OperationGuard {
    fn drop(&mut self) {
        if self.armed {
            self.operation.finish(Observation::Unobserved);
        }
    }
}

#[derive(Clone)]
struct Handler {
    binding: Weak<ThreadBinding>,
}

impl HostControl {
    fn check_open(&self) -> io::Result<()> {
        if self.incoming.is_stopped()
            || self.shutdown.is_cancelled()
            || self
                .registry
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .closed
        {
            return Err(binding::withdrawn());
        }
        Ok(())
    }

    pub(crate) fn prepare_child_binding(self: &Arc<Self>) -> io::Result<PreparedChildBinding> {
        self.check_open()?;
        let key = Uuid::new_v4();
        let authorization = format!("Bearer {}", Uuid::new_v4());
        let config = json!({
            "url": format!("http://{}/mcp/{key}", self.address),
            "http_headers": {"Authorization": authorization},
            "default_tools_approval_mode": "approve",
            "tools": {
                "create_thread": {"approval_mode": "prompt"},
                "send_message_to_thread": {"approval_mode": "prompt"},
                "fork_thread": {"approval_mode": "prompt"},
                "create_worktree": {"approval_mode": "approve"}
            }
        });
        let bound = Arc::new(ThreadBinding {
            key,
            authorization,
            config,
            host: Arc::downgrade(self),
            parent: std::sync::RwLock::new(None),
            capture: std::sync::RwLock::new(None),
            leases: std::sync::atomic::AtomicUsize::new(1),
            fork_provider: std::sync::OnceLock::new(),
        });
        let weak = Arc::downgrade(&bound);
        let mut server_config = StreamableHttpServerConfig::default();
        server_config.cancellation_token = self.http_shutdown.child_token();
        let service = StreamableHttpService::new(
            move || {
                Ok(Handler {
                    binding: weak.clone(),
                })
            },
            Arc::new(LocalSessionManager::default()),
            server_config,
        );

        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        if registry.closed || self.incoming.is_stopped() {
            return Err(binding::withdrawn());
        }
        if registry.bindings.len() >= MAX_BINDINGS {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "task binding capacity exhausted",
            ));
        }
        registry.bindings.insert(key, (bound.clone(), service));
        Ok(PreparedChildBinding { binding: bound })
    }

    fn retire_binding(&self, binding: &Arc<ThreadBinding>) {
        {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            if registry
                .bindings
                .get(&binding.key)
                .is_some_and(|(current, _)| Arc::ptr_eq(current, binding))
            {
                registry.bindings.remove(&binding.key);
            }
        }
        self.retire_fork_owner(binding.key);
    }

    /// Atomically retire only this same record's restored Work. False means an
    /// active lease owns it, so callers must not drop or infer absence from None.
    fn retire_restored(
        &self,
        key: Uuid,
        operation: &Arc<AcceptedCreate>,
        eligible: impl FnOnce(&CreateWork) -> bool,
    ) -> bool {
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        if !registry
            .operations
            .get(&key)
            .is_some_and(|current| Arc::ptr_eq(current, operation))
        {
            return true;
        }
        let slot = operation
            .work
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(work) = slot.as_ref() else {
            return false;
        };
        if eligible(work) {
            registry.operations.remove(&key);
        }
        true
    }

    async fn retire_operations_for_terminated(&self, thread: &Arc<CodexThread>) {
        let operations = self
            .registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .operations
            .iter()
            .map(|(key, value)| (*key, value.clone()))
            .collect::<Vec<_>>();
        for (key, operation) in operations {
            loop {
                let restored = operation.restored.notified();
                tokio::pin!(restored);
                restored.as_mut().enable();
                if self.retire_restored(key, &operation, |work| {
                    work.created()
                        .is_some_and(|created| Arc::ptr_eq(&created, thread))
                }) {
                    break;
                }
                // Preserve the original monitor's eventual observation if the
                // runtime ended while Work was active. No guard crosses await.
                restored.await;
            }
        }
        if let Some(slot) = self.initial_session.get() {
            slot.retire_clients_for_terminated(thread);
        }
    }

    fn stop_admission(&self) {
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        registry.closed = true;
        for operation in registry.operations.values() {
            operation.cancellation.cancel();
        }
        self.shutdown.cancel();
        self.http_shutdown.cancel();
        self.operations.close();
    }
}

impl OwnedTaskMcpHost {
    pub(crate) fn from_listener(
        listener: TcpListener,
        address: std::net::SocketAddr,
        incoming: Incoming,
        manager: Arc<ThreadManager>,
        create: BackendCreateExecutor,
        reads: BackendReadExecutor,
        management: BackendManagementExecutor,
    ) -> Self {
        let sends = create.send_executor();
        let forks = create.fork_executor();
        let control = Arc::new(HostControl {
            incoming,
            manager,
            create,
            reads,
            management,
            sends,
            forks,
            worktrees: std::sync::OnceLock::new(),
            initial_session: std::sync::OnceLock::new(),
            address,
            registry: std::sync::Mutex::new(Registry {
                closed: false,
                bindings: HashMap::new(),
                operations: HashMap::new(),
                management: HashMap::new(),
                sends: HashMap::new(),
                fork_calls: HashMap::new(),
                worktree_jobs: HashMap::new(),
            }),
            operations: TaskTracker::new(),
            monitors: TaskTracker::new(),
            shutdown: CancellationToken::new(),
            http_shutdown: CancellationToken::new(),
            #[cfg(test)]
            test_last_create_error: std::sync::Mutex::new(None),
            #[cfg(test)]
            test_fork_gate: std::sync::Mutex::new(None),
            #[cfg(test)]
            test_worktree_gate: std::sync::Mutex::new(None),
            #[cfg(test)]
            test_worktree_blocking_gate: std::sync::Mutex::new(None),
        });
        let router = Router::new().fallback(dispatch).with_state(control.clone());
        let serve_stop = control.http_shutdown.clone();
        let serve = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(serve_stop.cancelled_owned())
                .into_future()
                .await
        });
        Self {
            control,
            serve: Some(serve),
        }
    }

    pub(crate) fn stop_admission(&self) {
        self.control.stop_admission();
    }

    /// Caller keeps this host while actual runtime cleanup continues. Do not
    /// clear Work that reports an unobserved initial turn or pending custody.
    pub(crate) async fn drain_calls(&self) {
        self.stop_admission();
        self.control.operations.wait().await;
        let retained = self
            .control
            .registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .operations
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for operation in retained {
            let mut lease = operation.lease().await;
            if let Ok(work) = lease.work() {
                let _ = work.finish_owned().await;
            }
        }
    }

    /// Must follow processor.shutdown_threads. Exact-runtime termination
    /// retires transport bindings; it is not a Native all-effects receipt.
    pub(crate) async fn finish_after_threads(&mut self) -> io::Result<()> {
        self.control.monitors.close();
        self.control.monitors.wait().await;
        let retained = self
            .control
            .registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .operations
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut uncertain = false;
        for operation in retained {
            let mut lease = operation.lease().await;
            let work = lease.work()?;
            if work.finish_owned().await.is_err() {
                uncertain = true;
            }
            if let Some(thread) = work.created() {
                thread.wait_until_terminated().await;
            }
        }
        if let Some(serve) = self.serve.as_mut() {
            serve
                .await
                .map_err(|_| io::Error::other("task MCP server worker failed"))??;
            self.serve.take();
        }
        if uncertain {
            return Err(io::Error::other(
                "task operation initial-turn result remains unobserved",
            ));
        }
        self.control.finish_forks_after_threads().await;
        if self.control.has_unobserved_worktree() {
            return Err(io::Error::other(
                "worktree creation outcome remains unobserved",
            ));
        }
        if self.control.has_unobserved_fork() {
            return Err(io::Error::other("task fork outcome remains unobserved"));
        }
        if self.control.has_unobserved_send() {
            return Err(io::Error::other("task SEND outcome remains unobserved"));
        }
        if self.control.has_unobserved_management() {
            return Err(io::Error::other(
                "task management outcome remains unobserved",
            ));
        }
        let retired = {
            let mut registry = self
                .control
                .registry
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            std::mem::take(&mut registry.operations)
        };
        // Work can own an unbound PreparedChildBinding whose Drop re-enters
        // registry cleanup. Release actual records after unlocking the registry.
        drop(retired);
        let retired_sends = {
            let mut registry = self
                .control
                .registry
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            std::mem::take(&mut registry.sends)
        };
        drop(retired_sends);
        let retired_forks = {
            let mut registry = self
                .control
                .registry
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            std::mem::take(&mut registry.fork_calls)
        };
        drop(retired_forks);
        let retired_worktrees = {
            let mut registry = self
                .control
                .registry
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            std::mem::take(&mut registry.worktree_jobs)
        };
        drop(retired_worktrees);
        Ok(())
    }
}
impl Drop for OwnedTaskMcpHost {
    fn drop(&mut self) {
        self.stop_admission();
    }
}

async fn dispatch(State(host): State<Arc<HostControl>>, request: Request<Body>) -> Response {
    let key = request
        .uri()
        .path()
        .strip_prefix("/mcp/")
        .and_then(|key| Uuid::parse_str(key).ok());
    let entry = key.and_then(|key| {
        let registry = host.registry.lock().unwrap_or_else(PoisonError::into_inner);
        if registry.closed {
            return None;
        }
        registry
            .bindings
            .get(&key)
            .map(|(binding, service)| (binding.clone(), service.clone()))
    });
    let Some((binding, service)) = entry else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !request
        .headers()
        .get(AUTHORIZATION)
        .is_some_and(|header| header.as_bytes() == binding.authorization.as_bytes())
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    // Exact locked rmcp public inherent API. No new tower dependency or
    // presumed request-extension propagation is required.
    service.handle(request).await.map(Body::new)
}

fn task_mcp_tools(worktrees: bool) -> Result<Vec<Tool>, McpError> {
    let mut specs = vec![create_tool_spec()];
    if worktrees {
        specs.extend(codex_alab_task_worktree::specs());
    }
    let (name, description, properties, required) =
        crate::request_processors::retained_task_fork::policy::tool_definition();
    specs.push(DynamicToolSpec::Function(DynamicToolFunctionSpec {
        name: name.into(), description: description.into(),
        input_schema: json!({"type":"object","additionalProperties":false,"properties":properties,"required":required}),
        defer_loading: true,
    }));
    let (name, description, properties, required) =
        crate::request_processors::retained_task_send::tool_definition();
    specs.push(DynamicToolSpec::Function(DynamicToolFunctionSpec {
        name: name.into(), description: description.into(),
        input_schema: json!({"type":"object","additionalProperties":false,"properties":properties,"required":required}),
        defer_loading: true,
    }));
    specs.extend(retained_task_read::policy::tool_definitions().into_iter().chain(retained_task_read::policy::management::tool_definitions()).map(|(name, description, properties, required)| {
        DynamicToolSpec::Function(DynamicToolFunctionSpec {
            name: name.into(), description: description.into(),
            input_schema: json!({"type":"object","additionalProperties":false,"properties":properties,"required":required}),
            defer_loading: true,
        })
    }));
    specs
        .into_iter()
        .map(|spec| {
            let DynamicToolSpec::Function(spec) = spec else {
                return Err(McpError::internal_error(
                    "invalid actual task specification",
                    None,
                ));
            };
            let schema = serde_json::from_value::<JsonObject>(spec.input_schema)
                .map_err(|_| McpError::internal_error("invalid actual task schema", None))?;
            let mut tool = Tool::new(
                Cow::Owned(spec.name),
                Cow::Owned(spec.description),
                Arc::new(schema),
            );
            tool.annotations = Some(ToolAnnotations::new().read_only(
                retained_task_read::supports(tool.name.as_ref())
                    || codex_alab_task_worktree::read_only(tool.name.as_ref()),
            ));
            Ok(tool)
        })
        .collect()
}

impl ServerHandler for Handler {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: McpRequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let binding = self
            .binding
            .upgrade()
            .ok_or_else(|| McpError::internal_error("task binding withdrawn", None))?;
        let host = binding
            .host
            .upgrade()
            .ok_or_else(|| McpError::internal_error("task host withdrawn", None))?;
        host.check_open()
            .map_err(|_| McpError::internal_error("task host withdrawn", None))?;
        // Core discovers MCP while constructing the runtime. Static discovery
        // permits that handshake; it never opens execution on a pending binding.
        let worktrees = match host.worktrees.get() {
            Some(worktrees) => worktrees.available().await,
            None => false,
        };
        host.check_open()
            .map_err(|_| McpError::internal_error("task host withdrew during discovery", None))?;
        Ok(ListToolsResult::with_all_items(task_mcp_tools(worktrees)?))
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: McpRequestContext<RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, McpError> {
        if request.name.as_ref() != "create_thread"
            && !codex_alab_task_worktree::supports(request.name.as_ref())
            && request.name.as_ref() != "fork_thread"
            && request.name.as_ref() != "send_message_to_thread"
            && !retained_task_read::supports(request.name.as_ref())
            && !retained_task_read::management::supports(request.name.as_ref())
        {
            return Err(McpError::invalid_params("unsupported task operation", None));
        }
        let binding = self
            .binding
            .upgrade()
            .ok_or_else(|| McpError::internal_error("task binding withdrawn", None))?;
        let host = binding
            .host
            .upgrade()
            .ok_or_else(|| McpError::internal_error("task host withdrawn", None))?;
        let (parent_id, parent, capture, features) = binding
            .actual_parent()
            .await
            .map_err(|_| McpError::internal_error("actual bound task is unavailable", None))?;
        if context
            .meta
            .0
            .0
            .get("threadId")
            .and_then(Value::as_str)
            .is_some_and(|claimed| claimed != parent_id.to_string())
        {
            return Err(McpError::invalid_params(
                "task metadata disagrees with its actual binding",
                None,
            ));
        }
        if codex_alab_task_worktree::supports(request.name.as_ref()) {
            return host
                .call_worktree(
                    parent_id,
                    parent,
                    request.name.as_ref(),
                    Value::Object(request.arguments.unwrap_or_default()),
                    context.ct,
                )
                .await;
        }
        if request.name.as_ref() == "fork_thread" {
            let arguments = match serde_json::from_value::<
                crate::request_processors::retained_task_fork::ForkArguments,
            >(Value::Object(
                request.arguments.unwrap_or_default(),
            )) {
                Ok(arguments) => arguments,
                Err(error) => {
                    return Ok(CallToolResult::error(vec![ContentBlock::text(
                        retained_task_read::policy::truncate(
                            &format!("Invalid tool arguments: {error}"),
                            retained_task_read::policy::MAX_ERROR_CHARS,
                        ),
                    )])
                    .into());
                }
            };
            let claim = context
                .meta
                .0
                .0
                .get(origin::META_KEY)
                .and_then(Value::as_str)
                .and_then(|value| Uuid::parse_str(value).ok())
                .ok_or_else(|| {
                    McpError::invalid_params("genuine fork invocation is required", None)
                })?;
            return host
                .accept_fork(
                    &binding,
                    (parent_id, parent, capture),
                    arguments,
                    claim,
                    context.ct,
                )
                .await;
        }
        if request.name.as_ref() == "send_message_to_thread" {
            let arguments = match crate::request_processors::retained_task_send::parse_arguments(
                Value::Object(request.arguments.unwrap_or_default()),
            ) {
                Ok(arguments) => arguments,
                Err(error) => {
                    return Ok(CallToolResult::error(vec![ContentBlock::text(
                        retained_task_read::policy::truncate(
                            &error,
                            retained_task_read::policy::MAX_ERROR_CHARS,
                        ),
                    )])
                    .into());
                }
            };
            let operation = crate::request_processors::retained_task_send::SendRequest {
                parent_id,
                parent,
                arguments,
                features,
                service_config: binding.config.clone(),
                client_name: capture.client_name(),
                client_version: capture.client_version(),
                client_mcp_extensions: capture.extensions(),
                context: OperationContext {
                    span: tracing::info_span!("retained.task.send", thread_id = %parent_id),
                    parent_trace: None,
                    cancellation: host.shutdown.child_token(),
                },
            };
            return host.accept_send(operation, capture, context.ct).await;
        }
        if retained_task_read::management::supports(request.name.as_ref()) {
            let operation = match retained_task_read::policy::management::prepare(
                &parent_id.to_string(),
                request.name.as_ref(),
                Value::Object(request.arguments.unwrap_or_default()),
            ) {
                Ok(operation) => operation,
                Err(error) => {
                    return Ok(CallToolResult::error(vec![ContentBlock::text(
                        retained_task_read::policy::truncate(
                            &error,
                            retained_task_read::policy::MAX_ERROR_CHARS,
                        ),
                    )])
                    .into());
                }
            };
            // Actual accepted host work has its own IO-lifetime token. The
            // original HTTP request token cancels ONLY observation below.
            let request = ManagementRequest {
                parent_id,
                parent,
                operation,
                context: OperationContext {
                    span: tracing::info_span!("retained.task.management", tool = %request.name, thread_id = %parent_id),
                    parent_trace: None,
                    cancellation: host.shutdown.child_token(),
                },
            };
            return host.accept_management(request, context.ct).await;
        }
        if retained_task_read::supports(request.name.as_ref()) {
            let operation = OperationContext {
                span: tracing::info_span!("retained.task.read", tool = %request.name, thread_id = %parent_id),
                parent_trace: None,
                cancellation: context.ct,
            };
            let observed = host
                .reads
                .execute(
                    parent_id,
                    parent,
                    operation,
                    request.name.as_ref(),
                    Value::Object(request.arguments.unwrap_or_default()),
                )
                .await;
            let result = match observed {
                Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
                Err(error) => CallToolResult::error(vec![ContentBlock::text(
                    retained_task_read::policy::truncate(
                        &error,
                        retained_task_read::policy::MAX_ERROR_CHARS,
                    ),
                )]),
            };
            return Ok(result.into());
        }
        let arguments = parse_arguments(Value::Object(request.arguments.unwrap_or_default()))
            .map_err(|_| McpError::invalid_params("invalid task creation arguments", None))?;
        let child_binding = host
            .prepare_child_binding()
            .map_err(|_| McpError::internal_error("child task binding unavailable", None))?;
        let work = CreateWork::new(
            CreateRequest {
                parent_id,
                parent,
                arguments,
                baseline: capture.prospective_create(),
                features,
                app_server_client_name: capture.client_name(),
                app_server_client_version: capture.client_version(),
                client_mcp_extensions: capture.extensions(),
                operation: OperationContext {
                    span: tracing::info_span!("retained.task.create", thread_id = %parent_id),
                    parent_trace: None,
                    cancellation: context.ct.clone(),
                },
            },
            child_binding,
        );
        let (completed, _) = tokio::sync::watch::channel(None);
        let accepted = Arc::new(AcceptedCreate {
            work: std::sync::Mutex::new(Some(work)),
            restored: tokio::sync::Notify::new(),
            cancellation: context.ct,
            completion: completed,
        });
        let mut receive = accepted.completion.subscribe();
        let key = Uuid::new_v4();
        let guard = OperationGuard {
            operation: accepted.clone(),
            armed: true,
        };
        {
            let mut registry = host.registry.lock().unwrap_or_else(PoisonError::into_inner);
            if registry.closed || host.incoming.is_stopped() {
                return Err(McpError::internal_error("task host withdrawn", None));
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
            registry.operations.insert(key, accepted.clone());
            let owner = host.clone();
            host.operations.spawn(async move {
                let mut lease = accepted.lease().await;
                let work = match lease.work() {
                    Ok(work) => work,
                    Err(_) => {
                        guard.finish(Observation::Unobserved);
                        return;
                    }
                };
                let mut future =
                    Box::pin(AssertUnwindSafe(owner.create.run_create_work(work)).catch_unwind());
                let polled = future.as_mut().await;
                let destroyed = catch_unwind(AssertUnwindSafe(|| drop(future)));
                let observation = match (polled, destroyed) {
                    (Ok(result), Ok(())) => {
                        Observation::Complete(result.map(Arc::new).map_err(Arc::new))
                    }
                    _ => Observation::Unobserved,
                };
                #[cfg(test)]
                if let Observation::Complete(Err(error)) = &observation {
                    *owner
                        .test_last_create_error
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner) = Some((**error).clone());
                }
                let observed_owned = work.finish_owned().await.is_ok();
                let successful = matches!(&observation, Observation::Complete(Ok(_)));
                let failed = matches!(&observation, Observation::Complete(Err(_)));
                // Restore before publication or registry retirement. An aborted
                // worker takes this same Drop path, preserving original custody.
                drop(lease);
                guard.finish(observation);
                if observed_owned {
                    // Real recipe completion + owned START drainage retires
                    // only restored work. Captured failed START requires the
                    // recipe's actual exact-runtime termination observation.
                    owner.retire_restored(key, &accepted, |work| {
                        successful
                            || (failed
                                && (work.created().is_none() || work.captured_start_terminated()))
                    });
                }
            });
        }
        loop {
            let observed = receive.borrow_and_update().clone();
            if let Some(observed) = observed {
                return match observed {
                    Observation::Complete(Ok(outcome)) => {
                        Ok(CallToolResult::success(vec![ContentBlock::text(
                            outcome.tool_result().to_string(),
                        )])
                        .into())
                    }
                    Observation::Complete(Err(_)) => Err(McpError::internal_error(
                        "task creation failed; inspect retained task state",
                        None,
                    )),
                    Observation::Unobserved => Err(McpError::internal_error(
                        "task outcome unobserved; do not replay creation",
                        None,
                    )),
                };
            }
            receive
                .changed()
                .await
                .map_err(|_| McpError::internal_error("task observation unavailable", None))?;
        }
    }
}

/// Bind fallibly before MessageProcessor starts any resources, then hand this
/// actual listener directly to the host. No endpoint is reconstructed later.
pub(crate) async fn bind_listener(
    incoming: &Incoming,
) -> io::Result<(TcpListener, std::net::SocketAddr)> {
    if incoming.is_stopped() {
        return Err(binding::withdrawn());
    }
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    if incoming.is_stopped() {
        return Err(binding::withdrawn());
    }
    Ok((listener, address))
}

#[path = "retained_task_mcp_slot.rs"]
mod slot;
pub(crate) use slot::HostSlot;

#[cfg(all(test, unix))]
#[path = "retained_task_mcp_tests.rs"]
pub(crate) mod tests;

#[path = "retained_task_management_host.rs"]
mod management;

#[path = "retained_task_send_host.rs"]
mod send;

#[path = "retained_task_fork_host.rs"]
mod fork;
#[path = "retained_task_fork_origin.rs"]
mod origin;

#[path = "retained_task_worktree_host.rs"]
mod worktree;
