// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Complete fork recipe on the actual retained processors. No initial turn.
use super::super::alab_task_read::history;
use super::super::*;
use super::ForkTarget;
use crate::message_processor::alab_task_mcp::ConstructionCapture;
use crate::message_processor::alab_task_mcp::HostControl;
use crate::message_processor::alab_task_mcp::PreparedChildBinding;
use crate::request_processors::OperationContext;
use crate::request_processors::retained_task_read::server_error;
use codex_alab_core::native_incoming::Incoming;
use codex_core::ThreadRegistration;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::Weak;

#[path = "task_fork_policy.rs"]
pub(crate) mod policy;
pub(crate) use policy::ForkArguments;
#[cfg(test)]
pub(crate) type ForkProgressObservation = (
    Option<Option<String>>,
    Option<Arc<ThreadForkResponse>>,
    Option<Arc<CodexThread>>,
);

pub(crate) struct ForkRequest {
    pub(crate) parent_id: ThreadId,
    pub(crate) parent: Arc<CodexThread>,
    pub(crate) issuing_turn: String,
    pub(crate) context: OperationContext,
    pub(crate) arguments: ForkArguments,
    pub(crate) capture: ConstructionCapture,
    pub(crate) service_config: serde_json::Value,
}

#[derive(Default)]
struct Progress {
    core_entered: bool,
    born: Option<Arc<CodexThread>>,
    response: Option<Arc<ThreadForkResponse>>,
    binding: Option<PreparedChildBinding>,
    #[cfg(test)]
    cutoff: Option<Option<String>>,
    #[cfg(test)]
    test_response: Option<Arc<ThreadForkResponse>>,
    #[cfg(test)]
    test_created: Option<Arc<CodexThread>>,
}

#[derive(Clone)]
pub(crate) struct ForkScope {
    request: Arc<ForkRequest>,
    incoming: Incoming,
    host: Weak<HostControl>,
    progress: Arc<Mutex<Progress>>,
}
impl ForkScope {
    pub(crate) fn new(request: ForkRequest, incoming: Incoming, host: Weak<HostControl>) -> Self {
        Self {
            request: Arc::new(request),
            incoming,
            host,
            progress: Arc::new(Mutex::new(Progress::default())),
        }
    }
    pub(crate) fn created(&self) -> Option<Arc<CodexThread>> {
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .born
            .clone()
    }
    pub(crate) fn core_entered(&self) -> bool {
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .core_entered
    }
    pub(in crate::request_processors) fn trace(&self) -> Option<W3cTraceContext> {
        codex_otel::span_w3c_trace_context(&self.request.context.span)
            .or_else(|| self.request.context.parent_trace.clone())
    }
    pub(in crate::request_processors) async fn check(
        &self,
        processor: &ThreadRequestProcessor,
    ) -> Result<(), JSONRPCErrorError> {
        if self.incoming.is_stopped()
            || self.request.context.cancellation.is_cancelled()
            || self.request.parent.has_shutdown_status()
            || !matches!(
                processor
                    .thread_manager
                    .thread_registration(self.request.parent_id, &self.request.parent)
                    .await,
                ThreadRegistration::Current
            )
        {
            return Err(invalid_request("backend fork issuing runtime withdrew"));
        }
        if self.incoming.is_stopped()
            || self.request.context.cancellation.is_cancelled()
            || self.request.parent.has_shutdown_status()
        {
            return Err(invalid_request("backend fork issuing runtime withdrew"));
        }
        Ok(())
    }
    /// Before reserving metadata, preserve the configured namespace until the
    /// actual opaque binding checks it. Serialized endpoint data grants nothing.
    pub(in crate::request_processors) async fn configure(
        &self,
        processor: &ThreadRequestProcessor,
        config: &mut Config,
    ) -> Result<(), JSONRPCErrorError> {
        self.check(processor).await?;
        let host = self
            .host
            .upgrade()
            .ok_or_else(|| internal_error("fork host withdrew"))?;
        let binding = host
            .prepare_child_binding()
            .map_err(|_| internal_error("fork binding unavailable"))?;
        binding
            .capture(&host, self.request.capture.clone())
            .map_err(|_| internal_error("fork inputs unavailable"))?;
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .binding = Some(binding.clone());
        binding
            .configure_loaded(config)
            .await
            .map_err(|_| invalid_request("retained fork namespace or policy refused"))?;
        self.check(processor).await
    }
    pub(in crate::request_processors) fn base_overrides(
        &self,
        overrides: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<Option<HashMap<String, serde_json::Value>>, JSONRPCErrorError> {
        let Some(mut overrides) = overrides else {
            return Ok(None);
        };
        if let Some(value) = overrides.remove("mcp_servers.codex_tui")
            && value != self.request.service_config
        {
            return Err(invalid_request("fork task overlay changed"));
        }
        Ok((!overrides.is_empty()).then_some(overrides))
    }
    pub(in crate::request_processors) async fn before_core(
        &self,
        processor: &ThreadRequestProcessor,
    ) -> Result<(), JSONRPCErrorError> {
        self.check(processor).await?;
        self.progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .core_entered = true;
        Ok(())
    }
    pub(in crate::request_processors) fn capture_birth(&self, thread: &Arc<CodexThread>) {
        let mut progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        progress.born = Some(thread.clone());
        #[cfg(test)]
        {
            progress.test_created = Some(thread.clone());
        }
    }
    pub(in crate::request_processors) async fn bind_listener(
        &self,
        processor: &ThreadRequestProcessor,
        id: ThreadId,
        thread: &Arc<CodexThread>,
    ) -> Result<(), JSONRPCErrorError> {
        self.check(processor).await?;
        let binding = self
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .binding
            .clone()
            .ok_or_else(|| internal_error("fork lost actual binding"))?;
        binding
            .bind_created(id, thread.clone())
            .await
            .map_err(|_| thread_closing_for_resume(id))?;
        super::super::super::thread_lifecycle::alab_retained_listener::ensure_backend_start_listener(
            processor.listener_task_context(), id, thread.clone()).await?;
        self.check(processor).await?;
        if thread.has_shutdown_status()
            || !matches!(
                processor
                    .thread_manager
                    .thread_registration(id, thread)
                    .await,
                ThreadRegistration::Current
            )
        {
            return Err(thread_closing_for_resume(id));
        }
        self.check(processor).await
    }
    pub(in crate::request_processors) async fn complete(
        &self,
        processor: &ThreadRequestProcessor,
        response: ThreadForkResponse,
    ) -> Result<(), JSONRPCErrorError> {
        self.check(processor).await?;
        let id = ThreadId::from_string(&response.thread.id)
            .map_err(|error| internal_error(error.to_string()))?;
        let thread = self
            .created()
            .ok_or_else(|| internal_error("fork returned no captured runtime"))?;
        if thread.has_shutdown_status()
            || !matches!(
                processor
                    .thread_manager
                    .thread_registration(id, &thread)
                    .await,
                ThreadRegistration::Current
            )
        {
            return Err(thread_closing_for_resume(id));
        }
        self.check(processor).await?;
        let response = Arc::new(response);
        let mut progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        #[cfg(test)]
        {
            progress.test_response = Some(response.clone());
        }
        progress.response = Some(response);
        Ok(())
    }
    /// The one helper has returned normally with an error and entered no turn.
    /// Shutdown only its exact captured newborn, never the source/issuing runtime.
    pub(crate) async fn finish_error(&self) -> bool {
        if !self.core_entered() {
            return true;
        }
        let Some(thread) = self.created() else {
            return false;
        };
        use crate::request_processors::thread_lifecycle::ThreadShutdownResult;
        use crate::request_processors::thread_lifecycle::wait_for_thread_shutdown;
        matches!(
            wait_for_thread_shutdown(&thread).await,
            ThreadShutdownResult::Complete
        )
    }
    pub(crate) fn release_definite_custody(&self) {
        let retired = {
            let mut progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
            (
                progress.born.take(),
                progress.response.take(),
                progress.binding.take(),
            )
        };
        // Unbound binding destruction can re-enter host registry cleanup.
        drop(retired);
    }
    #[cfg(test)]
    pub(crate) fn observed_for_tests(&self) -> ForkProgressObservation {
        let progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
        (
            progress.cutoff.clone(),
            progress.test_response.clone(),
            progress.test_created.clone(),
        )
    }
}

#[derive(Clone)]
pub(crate) struct BackendForkExecutor {
    processor: ThreadRequestProcessor,
}
impl BackendForkExecutor {
    pub(crate) fn new(processor: ThreadRequestProcessor) -> Self {
        Self { processor }
    }
    pub(crate) async fn run(&self, scope: &ForkScope) -> Result<String, String> {
        let request = &scope.request;
        scope
            .check(&self.processor)
            .await
            .map_err(|error| server_error("fork", error))?;
        #[cfg(test)]
        if let Some(host) = scope.host.upgrade() {
            host.fork_before_recipe_for_tests().await;
        }
        scope
            .check(&self.processor)
            .await
            .map_err(|error| server_error("fork", error))?;
        let target = request
            .arguments
            .thread_id
            .clone()
            .unwrap_or_else(|| request.parent_id.to_string());
        let source = self
            .processor
            .thread_read_response_inner(ThreadReadParams {
                thread_id: target.clone(),
                include_turns: false,
            })
            .await
            .map_err(|error| server_error("thread/read", error))?
            .thread;
        let before_turn_id =
            if crate::request_processors::retained_task_read::policy::same_thread_id(
                &target,
                &request.parent_id.to_string(),
            ) {
                Some(request.issuing_turn.clone())
            } else if matches!(source.status, ThreadStatus::Active { .. }) {
                let turns = match self
                    .processor
                    .thread_turns_list_response_inner(ThreadTurnsListParams {
                        thread_id: target.clone(),
                        cursor: None,
                        limit: Some(1),
                        sort_direction: Some(SortDirection::Desc),
                        items_view: Some(TurnItemsView::NotLoaded),
                    })
                    .await
                {
                    Ok(page) => page.data,
                    Err(error) if history::is_history_pagination_unsupported(&error) => {
                        self.processor
                            .thread_read_response_inner(ThreadReadParams {
                                thread_id: target.clone(),
                                include_turns: true,
                            })
                            .await
                            .map_err(|error| server_error("thread/read", error))?
                            .thread
                            .turns
                    }
                    Err(error) => return Err(server_error("thread/turns/list", error)),
                };
                turns
                    .into_iter()
                    .find(|turn| turn.status == codex_app_server_protocol::TurnStatus::InProgress)
                    .map(|turn| turn.id)
            } else {
                None
            };
        #[cfg(test)]
        {
            scope
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .cutoff = Some(before_turn_id.clone());
        }
        let exclude_turns =
            source.history_mode == codex_app_server_protocol::ThreadHistoryMode::Paginated;
        let params = |exclude_turns| ThreadForkParams {
            thread_id: target.clone(),
            before_turn_id: before_turn_id.clone(),
            ephemeral: source.ephemeral,
            exclude_turns,
            config: Some(HashMap::from([(
                "mcp_servers.codex_tui".into(),
                request.service_config.clone(),
            )])),
            ..ThreadForkParams::default()
        };
        let capture = &request.capture;
        let result = Box::pin(self.processor.thread_fork_inner(
            ForkTarget::Backend(scope.clone()),
            params(exclude_turns),
            capture.client_name(),
            capture.client_version(),
            capture.extensions(),
        ))
        .await;
        if let Err(error) = result {
            if exclude_turns
                && history::is_history_pagination_unsupported(&error)
                && !scope.core_entered()
            {
                Box::pin(self.processor.thread_fork_inner(
                    ForkTarget::Backend(scope.clone()),
                    params(false),
                    capture.client_name(),
                    capture.client_version(),
                    capture.extensions(),
                ))
                .await
                .map_err(|error| server_error("thread/fork", error))?;
            } else {
                return Err(server_error("thread/fork", error));
            }
        }
        let response = scope
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .response
            .clone()
            .ok_or_else(|| "fork result was not produced".to_string())?;
        Ok(policy::result(&target, &response.thread.id).to_string())
    }
}
