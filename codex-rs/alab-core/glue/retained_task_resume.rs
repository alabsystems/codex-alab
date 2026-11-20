// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A direct result target inside the original loaded/cold resume policy.
//! The existing accepted SEND worker retains this whole future. Client resume
//! keeps its real response, subscription, replay and history frontier.
use super::*;
use crate::message_processor::alab_task_mcp::ConstructionCapture;
use crate::message_processor::alab_task_mcp::HostControl;
use crate::request_processors::OperationContext;
use crate::request_processors::PreparedChildBinding;
use codex_alab_core::native_incoming::Incoming;
use codex_core::ThreadRegistration;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::Weak;

pub(crate) struct ResumedTarget {
    pub(crate) thread_id: ThreadId,
    pub(crate) thread: Arc<CodexThread>,
    pub(crate) model: String,
    pub(crate) model_provider: String,
}

#[derive(Default)]
struct Resolution {
    core_entered: bool,
    born: Option<Arc<CodexThread>>,
    selected: Option<Arc<CodexThread>>,
    result: Option<Arc<ResumedTarget>>,
    binding: Option<PreparedChildBinding>,
}

#[derive(Clone)]
pub(crate) struct BackendResume {
    parent_id: ThreadId,
    parent: Arc<CodexThread>,
    context: Arc<OperationContext>,
    incoming: Incoming,
    host: Weak<HostControl>,
    capture: ConstructionCapture,
    overlay: serde_json::Value,
    resolution: Arc<Mutex<Resolution>>,
}

impl BackendResume {
    pub(crate) fn new(
        parent_id: ThreadId,
        parent: Arc<CodexThread>,
        context: OperationContext,
        incoming: Incoming,
        host: Weak<HostControl>,
        capture: ConstructionCapture,
        overlay: serde_json::Value,
    ) -> Self {
        Self {
            parent_id,
            parent,
            context: Arc::new(context),
            incoming,
            host,
            capture,
            overlay,
            resolution: Arc::new(Mutex::new(Resolution::default())),
        }
    }

    pub(crate) fn core_entered(&self) -> bool {
        self.resolution
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .core_entered
    }

    pub(crate) fn created(&self) -> Option<Arc<CodexThread>> {
        self.resolution
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .born
            .clone()
    }

    pub(crate) fn selected(&self) -> Option<Arc<CodexThread>> {
        self.resolution
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .selected
            .clone()
    }

    pub(super) fn capture_loaded(&self, thread: &Arc<CodexThread>) {
        self.resolution
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .selected = Some(thread.clone());
    }

    pub(crate) fn result(&self) -> Result<Arc<ResumedTarget>, JSONRPCErrorError> {
        self.resolution
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .result
            .clone()
            .ok_or_else(|| internal_error("backend resume result was not produced"))
    }

    pub(super) async fn check(
        &self,
        processor: &ThreadRequestProcessor,
    ) -> Result<(), JSONRPCErrorError> {
        if self.incoming.is_stopped()
            || self.context.cancellation.is_cancelled()
            || self.parent.has_shutdown_status()
            || !matches!(
                processor
                    .thread_manager
                    .thread_registration(self.parent_id, &self.parent)
                    .await,
                ThreadRegistration::Current
            )
        {
            return Err(invalid_request("backend resume issuing runtime withdrew"));
        }
        if self.incoming.is_stopped()
            || self.context.cancellation.is_cancelled()
            || self.parent.has_shutdown_status()
        {
            return Err(invalid_request("backend resume issuing runtime withdrew"));
        }
        Ok(())
    }

    pub(super) fn trace(&self) -> Option<W3cTraceContext> {
        codex_otel::span_w3c_trace_context(&self.context.span)
            .or_else(|| self.context.parent_trace.clone())
    }

    /// The real issuer's transport overlay is ordinary configuration DATA.
    /// Omit only that overlay while resolving base policy, so the later actual
    /// cold binding configure detects a configured namespace collision. The
    /// opaque binding is issued only in before_core; equality here grants no
    /// authority. params.config still drives original loaded mismatch policy.
    pub(super) fn base_overrides(
        &self,
        overrides: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<Option<HashMap<String, serde_json::Value>>, JSONRPCErrorError> {
        let Some(mut overrides) = overrides else {
            return Ok(None);
        };
        let key = "mcp_servers.codex_tui";
        if let Some(value) = overrides.remove(key)
            && value != self.overlay
        {
            return Err(invalid_request(
                "backend resume task overlay does not match its actual binding",
            ));
        }
        Ok((!overrides.is_empty()).then_some(overrides))
    }

    pub(super) async fn before_core(
        &self,
        processor: &ThreadRequestProcessor,
        config: &mut Config,
    ) -> Result<(), JSONRPCErrorError> {
        self.check(processor).await?;
        let host = self
            .host
            .upgrade()
            .ok_or_else(|| internal_error("backend task host withdrew"))?;
        // Allocate only for a real cold incarnation. A loaded SEND does not
        // consume another binding or fail merely because that capacity is full.
        let binding = host
            .prepare_child_binding()
            .map_err(|_| internal_error("cold SEND task binding unavailable"))?;
        binding
            .capture(&host, self.capture.clone())
            .map_err(|_| internal_error("cold SEND startup capture unavailable"))?;
        self.resolution
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .binding = Some(binding.clone());
        binding.configure_loaded(config).await.map_err(|_| {
            invalid_request("backend resume task namespace could not be registered")
        })?;
        self.check(processor).await?;
        // No await after this mark before the original Core start call.
        self.resolution
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .core_entered = true;
        Ok(())
    }

    /// Capture immediately on the original NewThread return, before any later
    /// fallible listener/config/metadata work. None never proves no birth.
    pub(super) fn capture_birth(&self, thread: &Arc<CodexThread>) {
        let mut resolution = self
            .resolution
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        resolution.selected = Some(thread.clone());
        resolution.born = Some(thread.clone());
    }

    pub(super) async fn complete(
        &self,
        processor: &ThreadRequestProcessor,
        selected: ResumedTarget,
        client: (Option<String>, Option<String>),
        born: bool,
    ) -> Result<(), JSONRPCErrorError> {
        let ResumedTarget {
            thread_id: id,
            thread,
            model,
            model_provider,
        } = selected;
        let (client_name, client_version) = client;
        self.check(processor).await?;
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
        if born {
            let binding = self
                .resolution
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .binding
                .clone()
                .ok_or_else(|| internal_error("cold SEND lost actual task binding"))?;
            binding
                .bind_created(id, thread.clone())
                .await
                .map_err(|_| thread_closing_for_resume(id))?;
        }
        // Loaded targets preserve their actual MCP config/owner. No namespace
        // overlay, endpoint comparison, extra binding or replacement is needed.
        ThreadRequestProcessor::set_app_server_client_info(
            thread.as_ref(),
            client_name,
            client_version,
        )
        .await?;
        super::super::thread_lifecycle::alab_retained_listener::ensure_backend_start_listener(
            processor.listener_task_context(),
            id,
            thread.clone(),
        )
        .await?;
        thread
            .emit_thread_idle_lifecycle_if_idle(ThreadIdleCause::Completed)
            .await;
        self.check(processor).await?;
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
        self.resolution
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .result = Some(Arc::new(ResumedTarget {
            thread_id: id,
            thread,
            model,
            model_provider,
        }));
        Ok(())
    }
}

/// Use the original loaded resume policy and return its exact probe to the
/// original cold loop. A client completion cannot be accepted as a backend one.
pub(super) async fn probe_loaded(
    processor: &ThreadRequestProcessor,
    target: &ThreadResumeTarget,
    params: &ThreadResumeParams,
    name: Option<String>,
    version: Option<String>,
) -> Result<ControlFlow<(), Option<Box<StoredThread>>>, JSONRPCErrorError> {
    match processor
        .resume_running_thread(target, params, name, version, None)
        .await?
    {
        RunningThreadResumeResult::BackendHandled => Ok(ControlFlow::Break(())),
        RunningThreadResumeResult::NotRunning(stored) => Ok(ControlFlow::Continue(stored)),
        RunningThreadResumeResult::Handled(_) | RunningThreadResumeResult::RefreshModel(_) => Err(internal_error(
            "backend resume received a client operation",
        )),
    }
}
