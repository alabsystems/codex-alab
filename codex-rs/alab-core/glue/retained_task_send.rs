// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Complete SEND on the same real retained processors. The host owns this work
//! outside its polled future, including exact cold-resume and turn custody.
use super::alab_task_resume::BackendResume;
use super::alab_task_resume::ResumedTarget;
use super::*;
use crate::message_processor::alab_task_mcp::ConstructionCapture;
use crate::message_processor::alab_task_mcp::HostControl;
use crate::request_processors::BackendInitialTurn;
use crate::request_processors::OperationContext;
use crate::request_processors::retained_task_create::background;
use crate::request_processors::retained_task_read::server_error;
use codex_alab_core::native_incoming::Incoming;
use codex_core::ThreadRegistration;
use codex_features::Features;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::Weak;

use crate::request_processors::retained_task_create::policy;
pub(crate) use policy::SendArguments;

pub(crate) fn tool_definition() -> (
    &'static str,
    &'static str,
    serde_json::Value,
    Vec<&'static str>,
) {
    policy::send_tool_definition()
}

pub(crate) fn parse_arguments(value: serde_json::Value) -> Result<SendArguments, String> {
    serde_json::from_value(value).map_err(|error| format!("Invalid tool arguments: {error}"))
}

pub(crate) struct SendRequest {
    pub(crate) parent_id: ThreadId,
    pub(crate) parent: Arc<CodexThread>,
    pub(crate) context: OperationContext,
    pub(crate) arguments: SendArguments,
    pub(crate) features: Features,
    pub(crate) service_config: serde_json::Value,
    pub(crate) client_name: Option<String>,
    pub(crate) client_version: Option<String>,
    pub(crate) client_mcp_extensions: ClientMcpExtensions,
}

#[derive(Default)]
struct TurnProgress {
    entered: bool,
    result: Option<Result<Arc<TurnStartResponse>, String>>,
}

pub(crate) struct SendWork {
    request: SendRequest,
    resume: BackendResume,
    turn: Mutex<TurnProgress>,
}
impl SendWork {
    pub(crate) fn new(
        request: SendRequest,
        incoming: Incoming,
        host: Weak<HostControl>,
        capture: ConstructionCapture,
    ) -> Self {
        let resume = BackendResume::new(
            request.parent_id,
            request.parent.clone(),
            OperationContext {
                span: request.context.span.clone(),
                parent_trace: request.context.parent_trace.clone(),
                cancellation: request.context.cancellation.clone(),
            },
            incoming,
            host,
            capture,
            request.service_config.clone(),
        );
        Self {
            request,
            resume,
            turn: Mutex::new(TurnProgress::default()),
        }
    }
    pub(crate) fn created(&self) -> Option<Arc<CodexThread>> {
        self.resume.created()
    }
    pub(crate) fn core_entered(&self) -> bool {
        self.resume.core_entered()
    }
    #[cfg(test)]
    pub(crate) fn accepted_turn_for_tests(&self) -> Option<Arc<TurnStartResponse>> {
        self.turn
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .result
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .cloned()
    }
    pub(crate) fn target(&self) -> Option<Arc<CodexThread>> {
        self.resume.selected()
    }
    pub(crate) fn has_uncertain_effects(&self) -> bool {
        let turn = self.turn.lock().unwrap_or_else(PoisonError::into_inner);
        (turn.entered && turn.result.is_none())
            || (self.resume.core_entered() && self.created().is_none())
    }
}

#[derive(Clone)]
pub(crate) struct BackendSendExecutor {
    processor: ThreadRequestProcessor,
    incoming: Incoming,
    accounts: Arc<AccountRequestProcessor>,
    catalog: CatalogRequestProcessor,
    turns: BackendInitialTurn,
}
impl BackendSendExecutor {
    pub(crate) fn new(
        processor: ThreadRequestProcessor,
        incoming: Incoming,
        accounts: Arc<AccountRequestProcessor>,
        catalog: CatalogRequestProcessor,
        turns: BackendInitialTurn,
    ) -> Self {
        Self {
            processor,
            incoming,
            accounts,
            catalog,
            turns,
        }
    }

    async fn check(&self, request: &SendRequest) -> Result<(), String> {
        if self.incoming.is_stopped()
            || request.context.cancellation.is_cancelled()
            || request.parent.has_shutdown_status()
            || !matches!(
                self.processor
                    .thread_manager
                    .thread_registration(request.parent_id, &request.parent)
                    .await,
                ThreadRegistration::Current
            )
        {
            return Err("backend SEND issuing runtime withdrew".into());
        }
        if self.incoming.is_stopped()
            || request.context.cancellation.is_cancelled()
            || request.parent.has_shutdown_status()
        {
            return Err("backend SEND issuing runtime withdrew".into());
        }
        Ok(())
    }

    pub(crate) async fn run(&self, work: &SendWork) -> Result<String, String> {
        let request = &work.request;
        let prompt = policy::send_prompt(&request.parent_id.to_string(), &request.arguments)?;
        self.check(request).await?;
        // Same original live fallback, saved history mode and pre-resume Daybreak.
        let source = self
            .processor
            .thread_read_response_inner(ThreadReadParams {
                thread_id: request.arguments.thread_id.clone(),
                include_turns: false,
            })
            .await
            .map_err(|error| server_error("thread/read", error))?
            .thread;
        let exclude_turns =
            source.history_mode == codex_app_server_protocol::ThreadHistoryMode::Paginated;
        let enabled = source.daybreak_enabled.unwrap_or(false);
        self.check(request).await?;
        // Preserve the original issuer's actual MCP overlay as ordinary config
        // data. The real host resource issues a new opaque binding only if the
        // shared resume policy actually constructs a cold incarnation.
        let params = ThreadResumeParams {
            thread_id: request.arguments.thread_id.clone(),
            exclude_turns,
            config: Some(HashMap::from([(
                "mcp_servers.codex_tui".to_owned(),
                request.service_config.clone(),
            )])),
            ..ThreadResumeParams::default()
        };
        self.processor
            .thread_resume(
                ThreadResumeTarget::Backend(Box::new(work.resume.clone())),
                params,
                request.client_name.clone(),
                request.client_version.clone(),
                request.client_mcp_extensions.clone(),
            )
            .await
            .map_err(|error| server_error("thread/resume", error))?;
        let resumed: Arc<ResumedTarget> = work.resume.result().map_err(|error| error.message)?;
        self.check(request).await?;
        let program = background::program(
            &self.accounts,
            &self.catalog,
            &request.features,
            resumed.thread_id,
            request.arguments.model.as_deref().unwrap_or(&resumed.model),
            &resumed.model_provider,
            enabled,
        )
        .await?;
        let params = policy::initial_tool_turn(
            resumed.thread_id.to_string(),
            "send_message_to_thread",
            prompt,
            request.arguments.model.clone(),
            None,
            program,
        )?;
        self.check(request).await?;
        work.turn
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entered = true;
        let result = self
            .turns
            .followup(
                request.parent_id,
                request.parent.clone(),
                resumed.thread.clone(),
                &request.context,
                params,
            )
            .await
            .map(Arc::new)
            .map_err(|error| server_error("turn/start", error));
        let failure = result.as_ref().err().cloned();
        work.turn
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .result = Some(result);
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(serde_json::json!({"threadId":request.arguments.thread_id}).to_string())
    }
}
