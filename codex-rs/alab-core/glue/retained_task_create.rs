// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Complete create_thread work body. The actual backend MCP operation worker
//! owns this entire future before its first poll; observers never invoke it
//! again. This file creates no client/RPC context or Native permission.
#![cfg(unix)]

use super::BackendStart;
use super::BackendStartExecutor;
use super::BackendStartRequest;
use super::OperationContext;
use super::StartCustody;
use super::StartFailure;
use super::StartedThread;
use crate::request_processors::AccountRequestProcessor;
use crate::request_processors::BackendInitialTurn;
use crate::request_processors::CatalogRequestProcessor;
use crate::request_processors::ConfigRequestProcessor;
use crate::request_processors::PreparedChildBinding;
use codex_app_server_protocol::ConfigReadParams;
use codex_app_server_protocol::DynamicToolFunctionSpec;
use codex_app_server_protocol::DynamicToolSpec;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadSetNameParams;
use codex_app_server_protocol::ThreadStartParams;
use codex_core::CodexThread;
use codex_core::ThreadRegistration;
use codex_features::Feature;
use codex_features::Features;
use codex_protocol::ThreadId;
use codex_protocol::mcp::ClientMcpExtensions;
use codex_protocol::models::PermissionProfile;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;

#[path = "task_background_program.rs"]
pub(crate) mod background;
#[path = "task_create_policy.rs"]
pub(crate) mod policy;
pub(crate) use policy::CreateArguments;

pub(crate) fn create_tool_spec() -> DynamicToolSpec {
    let (name, description, properties, required) = policy::create_tool_definition();
    DynamicToolSpec::Function(DynamicToolFunctionSpec {
        name: name.into(),
        description: description.into(),
        input_schema: json!({"type":"object", "additionalProperties":false,
            "properties":properties, "required":required}),
        defer_loading: true,
    })
}

pub(crate) fn parse_arguments(value: Value) -> Result<CreateArguments, String> {
    serde_json::from_value(value).map_err(|error| format!("Invalid tool arguments: {error}"))
}

/// Captured by the genuine accepted backend invocation, never assembled from
/// HTTP threadId/turnId labels. Baseline params/features/client metadata are the
/// existing full MCP-start captures, not reconstructed defaults or tool inputs.
pub(crate) struct CreateRequest {
    pub(crate) parent_id: ThreadId,
    pub(crate) parent: Arc<CodexThread>,
    pub(crate) operation: OperationContext,
    pub(crate) arguments: CreateArguments,
    pub(crate) baseline: ThreadStartParams,
    pub(crate) features: Features,
    pub(crate) app_server_client_name: Option<String>,
    pub(crate) app_server_client_version: Option<String>,
    pub(crate) client_mcp_extensions: ClientMcpExtensions,
}

/// Stored in the host's actual accepted-operation owner outside the work future.
/// On panic/destruction it still exposes original START custody, including a
/// late captured post-birth runtime. It is not a Native settlement receipt.
pub(crate) struct CreateWork {
    request: CreateRequest,
    binding: PreparedChildBinding,
    startup: Option<BackendStart>,
    startup_failure: Option<Arc<StartFailure>>,
    captured_start_terminated: bool,
    started: Option<Arc<StartedThread>>,
    initial_turn_entered: bool,
    initial_turn_returned: bool,
    pending_notifications: Vec<ServerNotification>,
    running: bool,
}
impl CreateWork {
    pub(crate) fn new(request: CreateRequest, binding: PreparedChildBinding) -> Self {
        Self {
            request,
            binding,
            startup: None,
            startup_failure: None,
            captured_start_terminated: false,
            started: None,
            initial_turn_entered: false,
            initial_turn_returned: false,
            pending_notifications: Vec::new(),
            running: false,
        }
    }
    pub(crate) fn created(&self) -> Option<Arc<CodexThread>> {
        self.started
            .as_ref()
            .map(|started| started.thread.clone())
            .or_else(|| {
                self.startup_failure
                    .as_ref()
                    .and_then(|failure| failure.created())
            })
    }
    /// True only after the failed START's exact captured runtime completed
    /// Source Core shutdown. It is not a Native effects or cohort receipt.
    pub(crate) fn captured_start_terminated(&self) -> bool {
        self.captured_start_terminated
    }

    /// Observe START and clean up only an exact captured failed startup. The
    /// established Source cleanup bound must observe Core loop termination; a
    /// timeout, submit failure or Unknown retains the original custody. A
    /// NotStarted result describes only Source Core entry, not Native effects.
    pub(crate) async fn finish_owned(&mut self) -> Result<(), UnobservedWork> {
        if let Some(startup) = self.startup.clone() {
            match startup.wait().await {
                Ok(started) => self.started = Some(started),
                Err(failure) => {
                    let custody = failure.custody();
                    self.startup_failure = Some(failure);
                    match custody {
                        StartCustody::NotStarted => {}
                        StartCustody::Captured(thread) => {
                            // START failed before the recipe could enter its
                            // initial turn. Never shut down a successful child
                            // through this failure-only cleanup path.
                            if self.initial_turn_entered {
                                return Err(UnobservedWork::InitialTurn);
                            }
                            if !self.captured_start_terminated {
                                use crate::request_processors::thread_lifecycle::ThreadShutdownResult;
                                use crate::request_processors::thread_lifecycle::wait_for_thread_shutdown;
                                match wait_for_thread_shutdown(&thread).await {
                                    ThreadShutdownResult::Complete => {
                                        self.captured_start_terminated = true;
                                    }
                                    ThreadShutdownResult::SubmitFailed
                                    | ThreadShutdownResult::TimedOut => {
                                        return Err(UnobservedWork::Startup);
                                    }
                                }
                            }
                        }
                        StartCustody::Unknown => return Err(UnobservedWork::Startup),
                    }
                }
            }
        }
        if self.initial_turn_entered && !self.initial_turn_returned {
            Err(UnobservedWork::InitialTurn)
        } else {
            Ok(())
        }
    }
}

pub(crate) enum UnobservedWork {
    Startup,
    InitialTurn,
}

pub(crate) struct CreateOutcome {
    pub(crate) thread_id: ThreadId,
}
impl CreateOutcome {
    pub(crate) fn tool_result(&self) -> Value {
        json!({"threadId": self.thread_id.to_string()})
    }
}

/// Actual processor captures supplied at retained host construction. The turn
/// and binding types are concrete required integrations, not default/mock ports.
#[derive(Clone)]
pub(crate) struct BackendCreateExecutor {
    pub(crate) starts: BackendStartExecutor,
    pub(crate) accounts: Arc<AccountRequestProcessor>,
    pub(crate) catalog: CatalogRequestProcessor,
    pub(crate) config: ConfigRequestProcessor,
    pub(crate) turns: BackendInitialTurn,
}
impl BackendCreateExecutor {
    pub(crate) fn fork_executor(
        &self,
    ) -> crate::request_processors::retained_task_fork::BackendForkExecutor {
        crate::request_processors::retained_task_fork::BackendForkExecutor::new(
            self.starts.processor.clone(),
        )
    }
    pub(crate) fn send_executor(
        &self,
    ) -> crate::request_processors::retained_task_send::BackendSendExecutor {
        crate::request_processors::retained_task_send::BackendSendExecutor::new(
            self.starts.processor.clone(),
            self.starts.incoming.clone(),
            self.accounts.clone(),
            self.catalog.clone(),
            self.turns.clone(),
        )
    }
    async fn check_parent(&self, work: &CreateWork) -> Result<(), String> {
        let request = &work.request;
        if self.starts.incoming.is_stopped() || request.operation.cancellation.is_cancelled() {
            return Err("backend create_thread operation withdrew".into());
        }
        if request.parent.has_shutdown_status()
            || !matches!(
                self.starts
                    .processor
                    .thread_manager
                    .thread_registration(request.parent_id, &request.parent)
                    .await,
                ThreadRegistration::Current
            )
        {
            return Err("backend create_thread source is no longer current".into());
        }
        if self.starts.incoming.is_stopped() || request.operation.cancellation.is_cancelled() {
            return Err("backend create_thread operation withdrew".into());
        }
        Ok(())
    }
    async fn check_created(
        &self,
        work: &CreateWork,
        id: ThreadId,
        child: &Arc<CodexThread>,
    ) -> Result<(), String> {
        self.check_parent(work).await?;
        if child.has_shutdown_status()
            || !matches!(
                self.starts
                    .processor
                    .thread_manager
                    .thread_registration(id, child)
                    .await,
                ThreadRegistration::Current
            )
        {
            return Err("created task runtime is no longer current".into());
        }
        self.check_parent(work).await
    }

    /// Call exactly once from the actual retained host operation worker. The
    /// caller retains CreateWork while this future runs and after any error.
    pub(crate) async fn run_create_work(
        &self,
        work: &mut CreateWork,
    ) -> Result<CreateOutcome, String> {
        if work.running {
            return Err("create_thread work was already submitted".into());
        }
        work.running = true;
        self.check_parent(work).await?;
        let request = &work.request;
        policy::validate_prompt(&request.arguments.prompt, policy::MAX_INPUT_BYTES)?;
        let prompt =
            policy::delegated_prompt(&request.parent_id.to_string(), &request.arguments.prompt);
        policy::validate_prompt(&prompt, policy::MAX_DELEGATED_INPUT_BYTES)?;
        if request
            .arguments
            .title
            .as_deref()
            .is_some_and(|title| title.trim().is_empty())
        {
            return Err("title must not be empty".into());
        }

        // Capture settings from the actual source runtime, not a resume through
        // a fabricated client. Read project metadata through the existing store
        // helper and recheck that the same source allocation remains current.
        let (source, _) = request.parent.resume_config_snapshot().await;
        if source.ephemeral {
            return Err("ephemeral tasks cannot create inspectable background tasks".into());
        }
        let source_view = self
            .starts
            .processor
            .thread_read_response_inner(ThreadReadParams {
                thread_id: request.parent_id.to_string(),
                include_turns: false,
            })
            .await
            .map_err(|error| error.message)?;
        self.check_parent(work).await?;
        let mut params = request.baseline.clone();
        params.model_provider = Some(source.model_provider_id.clone());
        params.cwd = Some(source.cwd().to_string_lossy().into_owned());
        params.project_id = source_view.thread.project_id;
        params.ephemeral = Some(source.ephemeral);
        if request.features.enabled(Feature::CliDaybreak) && params.daybreak_enabled.is_none() {
            let defaults = self
                .config
                .read(ConfigReadParams {
                    include_layers: false,
                    cwd: Some(source.cwd().to_string_lossy().into_owned()),
                })
                .await
                .map_err(|error| error.message)?;
            params.daybreak_enabled = defaults
                .config
                .additional
                .get("daybreak")
                .and_then(Value::as_bool);
        }
        params.history_mode = (source_view.thread.history_mode
            == codex_app_server_protocol::ThreadHistoryMode::Paginated
            && self
                .starts
                .processor
                .thread_store
                .supports_paginated_history_lists())
        .then_some(codex_app_server_protocol::ThreadHistoryMode::Paginated);
        params.model = Some(source.model.clone());
        params.service_tier = Some(source.service_tier.clone());
        params.runtime_workspace_roots = Some(source.workspace_roots.clone());
        let permissions = policy::inherit_permissions(
            source
                .active_permission_profile
                .as_ref()
                .map(|profile| profile.id.as_str()),
            PermissionProfile::from_legacy_sandbox_policy_template(&source.sandbox_policy()),
        )?;
        params.approval_policy = Some(source.approval_policy.into());
        params.approvals_reviewer = Some(source.approvals_reviewer.into());
        params.permissions = permissions.profile;
        params.sandbox = permissions.sandbox;
        if let Some(model) = request.arguments.model.clone() {
            params.model = Some(model);
        }
        // START receives the actual opaque binding. Its configure hook owns
        // namespace/config insertion before Core construction; serialized
        // server data alone cannot identify or authorize this binding.
        self.check_parent(work).await?;
        let startup = self
            .starts
            .begin(BackendStartRequest {
                parent_id: request.parent_id,
                parent: request.parent.clone(),
                params,
                app_server_client_name: request.app_server_client_name.clone(),
                app_server_client_version: request.app_server_client_version.clone(),
                client_mcp_extensions: request.client_mcp_extensions.clone(),
                task_binding: Some(work.binding.clone()),
                context: OperationContext {
                    span: request.operation.span.clone(),
                    parent_trace: request.operation.parent_trace.clone(),
                    cancellation: request.operation.cancellation.clone(),
                },
            })
            .map_err(|error| error.message)?;
        work.startup = Some(startup.clone());
        let started = match startup.wait().await {
            Ok(started) => started,
            Err(failure) => {
                let error = failure.error.message.clone();
                work.startup_failure = Some(failure);
                return Err(error);
            }
        };
        work.started = Some(started.clone());
        let child_id = ThreadId::from_string(&started.response.thread.id)
            .map_err(|error| error.to_string())?;
        self.check_created(work, child_id, &started.thread).await?;
        // Required execution registration is backend-owned. There is no TUI
        // AppEvent or UI acknowledgement at this boundary.
        work.binding
            .bind_created(child_id, started.thread.clone())
            .await
            .map_err(|error| error.to_string())?;
        self.check_created(work, child_id, &started.thread).await?;
        let program = self
            .background_program(
                work,
                child_id,
                &started.response.model,
                &started.response.model_provider,
                started.response.thread.daybreak_enabled.unwrap_or(false),
            )
            .await?;
        if let Some(title) = request.arguments.title.as_ref() {
            match self
                .starts
                .processor
                .thread_set_name_response_inner(ThreadSetNameParams {
                    thread_id: child_id.to_string(),
                    name: title.trim().into(),
                })
                .await
            {
                Ok((_, Some(notification))) => work
                    .pending_notifications
                    .push(ServerNotification::ThreadNameUpdated(notification)),
                Ok((_, None)) => {}
                Err(error) => {
                    tracing::warn!(thread_id = %child_id, message = %error.message, "failed to name background task")
                }
            }
        }
        self.check_created(work, child_id, &started.thread).await?;
        let params = policy::initial_tool_turn(
            child_id.to_string(),
            "create_thread",
            prompt,
            None,
            permissions.turn_override,
            program,
        )?;
        // Concrete required seam from retained-task-turn-draft: shared complete
        // turn policy on the exact child, with actual operation trace/cancel.
        work.initial_turn_entered = true;
        let initial_turn = self
            .turns
            .start(
                request.parent_id,
                request.parent.clone(),
                started.thread.clone(),
                &request.operation,
                params,
            )
            .await;
        work.initial_turn_returned = true;
        initial_turn.map_err(|error| error.message)?;
        Ok(CreateOutcome {
            thread_id: child_id,
        })
    }

    async fn background_program(
        &self,
        work: &CreateWork,
        child: ThreadId,
        model: &str,
        provider: &str,
        enabled: bool,
    ) -> Result<Option<codex_app_server_protocol::CyberAccessProgram>, String> {
        background::program(
            &self.accounts,
            &self.catalog,
            &work.request.features,
            child,
            model,
            provider,
            enabled,
        )
        .await
    }
}
