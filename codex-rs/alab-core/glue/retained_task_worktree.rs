// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual local worktree policy on the retained processor and configuration owner.
use super::*;
use codex_alab_core::native_incoming::Incoming;
use codex_alab_task_worktree::Allocated;
use codex_alab_task_worktree::Allocation;
use codex_alab_task_worktree::CreateArguments;
use codex_alab_task_worktree::list_result;
use codex_app_server_protocol::ThreadAttachmentAddParams;
use codex_app_server_protocol::ThreadAttachmentListParams;
use codex_app_server_protocol::ThreadAttachmentListResponse;
use codex_core::ThreadRegistration;
use std::path::PathBuf;
use tokio::sync::OnceCell;

#[derive(Clone)]
struct HostSettings {
    home: PathBuf,
    desktop: Option<HashMap<String, serde_json::Value>>,
}

pub(crate) struct BackendWorktreeExecutor {
    processor: ThreadRequestProcessor,
    incoming: Incoming,
    settings: OnceCell<Result<HostSettings, String>>,
    source: codex_alab_task_worktree::SourceConfig,
}
impl BackendWorktreeExecutor {
    pub(crate) fn new(
        processor: ThreadRequestProcessor,
        incoming: Incoming,
        source: codex_alab_task_worktree::SourceConfig,
    ) -> Self {
        Self {
            processor,
            incoming,
            settings: OnceCell::new(),
            source,
        }
    }
    async fn settings(&self) -> Result<&HostSettings, String> {
        self.settings
            .get_or_init(|| async {
                let config = &self.processor.config;
                // Exact original host-local settings loader: no source-project or
                // task CLI overlay changes the managed checkout allocation root.
                let host = codex_core::config::load_config_toml_with_layer_stack(
                    &config.codex_home,
                    None,
                    Vec::new(),
                    codex_config::ConfigLoadOptions::default(),
                )
                .await
                .map_err(|error| error.to_string())?;
                codex_alab_task_worktree::validate_host_settings(
                    &config.codex_home,
                    host.config_toml.desktop.as_ref(),
                )?;
                Ok(HostSettings {
                    home: config.codex_home.to_path_buf(),
                    desktop: host.config_toml.desktop,
                })
            })
            .await
            .as_ref()
            .map_err(Clone::clone)
    }
    pub(crate) async fn available(&self) -> bool {
        !self.processor.config.active_project.is_untrusted()
            && self.processor.thread_store.supports_thread_attachments()
            && self.settings().await.is_ok()
    }
    pub(crate) async fn check(
        &self,
        parent_id: ThreadId,
        parent: &Arc<CodexThread>,
    ) -> Result<(), String> {
        if self.incoming.is_stopped() || parent.has_shutdown_status() {
            return Err("retained worktree issuing runtime withdrew before allocation".into());
        }
        if !matches!(
            self.processor
                .thread_manager
                .thread_registration(parent_id, parent)
                .await,
            ThreadRegistration::Current
        ) {
            return Err("issuing task runtime is no longer current".into());
        }
        if self.incoming.is_stopped() || parent.has_shutdown_status() {
            return Err("retained worktree issuing runtime withdrew before allocation".into());
        }
        Ok(())
    }
    async fn attachments(
        &self,
        parent_id: ThreadId,
        cursor: Option<String>,
    ) -> Result<ThreadAttachmentListResponse, String> {
        match self
            .processor
            .thread_attachment_list(ThreadAttachmentListParams {
                thread_id: parent_id.to_string(),
                cursor,
                limit: Some(1),
            })
            .await
            .map_err(|error| {
                crate::request_processors::retained_task_read::server_error(
                    "thread/attachment/list",
                    error,
                )
            })? {
            Some(ClientResponsePayload::ThreadAttachmentList(page)) => Ok(page),
            _ => {
                Err("Managed attachments unavailable: original list returned no typed page".into())
            }
        }
    }
    pub(crate) async fn list(
        &self,
        parent_id: ThreadId,
        parent: &Arc<CodexThread>,
        cursor: Option<String>,
    ) -> Result<serde_json::Value, String> {
        self.check(parent_id, parent).await?;
        let result = list_result(
            self.attachments(parent_id, cursor)
                .await
                .map_err(|error| format!("Managed attachments unavailable: {error}"))?,
        );
        self.check(parent_id, parent).await?;
        Ok(result)
    }
    pub(crate) async fn prepare(
        &self,
        parent_id: ThreadId,
        parent: &Arc<CodexThread>,
        arguments: CreateArguments,
    ) -> Result<Allocation, String> {
        self.check(parent_id, parent).await?;
        let response = self
            .processor
            .thread_read_response_inner(ThreadReadParams {
                thread_id: parent_id.to_string(),
                include_turns: false,
            })
            .await
            .map_err(|error| {
                crate::request_processors::retained_task_read::server_error("thread/read", error)
            })?;
        if response.thread.ephemeral
            || matches!(response.thread.thread_source.as_ref(),
            Some(codex_app_server_protocol::ThreadSource::Feature(source)) if source == "tui_side_chat")
        {
            return Err("Side conversations cannot retain worktree attachments. Request a new worktree in the main conversation.".into());
        }
        self.attachments(parent_id, None)
            .await
            .map_err(|error| format!("Managed attachments unavailable: {error}"))?;
        let environments = response
            .thread
            .environments
            .as_ref()
            .ok_or("Server cannot verify the task's execution environment")?;
        if environments.is_empty()
            || environments.iter().any(|environment| {
                environment.environment_id != codex_exec_server::LOCAL_ENVIRONMENT_ID
            })
        {
            return Err("Managed worktree tools require a local execution environment".into());
        }
        let source_cwd = PathBuf::from(environments[0].cwd.as_str());
        // Preserve the EXACT original source builder. The caller's real
        // CLI/harness(cwd-cleared)/loader/cloud choices were already captured.
        self.source.check_trusted(source_cwd.clone()).await?;
        let settings = self.settings().await?;
        let allocation = Allocation::prepare(
            &settings.home,
            settings.desktop.as_ref(),
            source_cwd,
            arguments,
            parent_id.to_string(),
        )?;
        self.check(parent_id, parent).await?;
        Ok(allocation)
    }
    /// Allocation already crossed its guarded host entry. Retain the original
    /// whole registration helper through shutdown; do not cancel it mid-store.
    pub(crate) async fn register(
        &self,
        parent_id: ThreadId,
        allocated: Allocated,
    ) -> serde_json::Value {
        let registration = match allocated.registration {
            Err(error) => Err(error),
            Ok(()) => match self
                .processor
                .alab_attachment_add_response(ThreadAttachmentAddParams {
                    thread_id: parent_id.to_string(),
                    attachment_type: "worktree".into(),
                    identity_key: allocated.checkout.root.to_string_lossy().into_owned(),
                    payload: codex_alab_task_worktree::payload(&allocated.checkout),
                })
                .await
            {
                Err(error) => Err(crate::request_processors::retained_task_read::server_error(
                    "thread/attachment/add",
                    error,
                )),
                Ok(added) => {
                    if let Some(notification) = added.notification {
                        self.processor
                            .outgoing
                            .send_server_notification(ServerNotification::ThreadAttachmentUpdated(
                                notification,
                            ))
                            .await;
                    }
                    // The typed store response is consumed locally; no client
                    // response/registration ACK is fabricated by this route.
                    let _response = added.response;
                    Ok(())
                }
            },
        };
        codex_alab_task_worktree::creation_result(&allocated.checkout, registration.is_ok())
    }
}
