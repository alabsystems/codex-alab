// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Complete title/archive management on the same actual processor. A concrete
//! retained host worker owns this entire future after admission; no observer,
//! parent-termination or cancellation select may drop it after helper entry.
use super::ThreadRequestProcessor;
use super::policy::management::ManagementOperation;
use crate::request_processors::OperationContext;
use codex_alab_core::native_incoming::Incoming;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadArchiveParams;
use codex_app_server_protocol::ThreadArchivedNotification;
use codex_app_server_protocol::ThreadSetNameParams;
use codex_app_server_protocol::ThreadUnarchiveParams;
use codex_app_server_protocol::ThreadUnarchivedNotification;
use codex_core::CodexThread;
use codex_core::ThreadRegistration;
use codex_protocol::ThreadId;
use std::sync::Arc;
use tracing::Instrument;

pub(crate) fn supports(tool: &str) -> bool {
    matches!(tool, "set_thread_title" | "set_thread_archived")
}

pub(crate) struct ManagementRequest {
    pub(crate) parent_id: ThreadId,
    pub(crate) parent: Arc<CodexThread>,
    pub(crate) context: OperationContext,
    pub(crate) operation: ManagementOperation,
}

pub(crate) struct BackendManagementExecutor {
    processor: ThreadRequestProcessor,
    incoming: Incoming,
}
impl BackendManagementExecutor {
    pub(crate) fn new(processor: ThreadRequestProcessor, incoming: Incoming) -> Self {
        Self {
            processor,
            incoming,
        }
    }

    async fn check_entry(&self, request: &ManagementRequest) -> Result<(), String> {
        if self.incoming.is_stopped()
            || request.context.cancellation.is_cancelled()
            || request.parent.has_shutdown_status()
        {
            return Err("retained task management owner withdrew before entry".into());
        }
        if !matches!(
            self.processor
                .thread_manager
                .thread_registration(request.parent_id, &request.parent)
                .await,
            ThreadRegistration::Current
        ) {
            return Err("issuing task runtime is no longer current".into());
        }
        if self.incoming.is_stopped() || request.context.cancellation.is_cancelled() {
            return Err("retained task management owner withdrew before entry".into());
        }
        Ok(())
    }

    /// The actual accepted-operation worker calls this once. A normal error
    /// can follow partial source effects; neither it nor archive success says
    /// Native effects/cohort settlement. The exact whole helper is awaited.
    pub(crate) async fn run(&self, request: &ManagementRequest) -> Result<String, String> {
        let work = async {
            self.check_entry(request).await?;
            match &request.operation {
                ManagementOperation::SetTitle { thread_id, title } => {
                    // This shared helper owns its own list-state permit. An
                    // additional outer permit would deadlock the same semaphore.
                    let (_, notice) = self
                        .processor
                        .thread_set_name_response_inner(ThreadSetNameParams {
                            thread_id: thread_id.clone(),
                            name: title.clone(),
                        })
                        .await
                        .map_err(|error| super::server_error("thread/name/set", error))?;
                    if let Some(notice) = notice {
                        self.processor
                            .outgoing
                            .send_server_notification(ServerNotification::ThreadNameUpdated(notice))
                            .await;
                    }
                }
                ManagementOperation::SetArchived {
                    thread_id,
                    archived,
                } => {
                    // Exact original outer permit lifetime: include the whole
                    // helper AND every returned notification enqueue. This is
                    // source policy serialization, never a UI acknowledgement.
                    let _permit = self
                        .processor
                        .acquire_thread_list_state_permit()
                        .await
                        .map_err(|error| {
                            super::server_error(
                                if *archived {
                                    "thread/archive"
                                } else {
                                    "thread/unarchive"
                                },
                                error,
                            )
                        })?;
                    self.check_entry(request).await?;
                    if *archived {
                        let (_, ids) = self
                            .processor
                            .thread_archive_response(ThreadArchiveParams {
                                thread_id: thread_id.clone(),
                            })
                            .await
                            .map_err(|error| super::server_error("thread/archive", error))?;
                        for thread_id in ids {
                            self.processor
                                .outgoing
                                .send_server_notification(ServerNotification::ThreadArchived(
                                    ThreadArchivedNotification { thread_id },
                                ))
                                .await;
                        }
                    } else {
                        let (_, ids) = self
                            .processor
                            .thread_unarchive_response(ThreadUnarchiveParams {
                                thread_id: thread_id.clone(),
                            })
                            .await
                            .map_err(|error| super::server_error("thread/unarchive", error))?;
                        for thread_id in ids {
                            self.processor
                                .outgoing
                                .send_server_notification(ServerNotification::ThreadUnarchived(
                                    ThreadUnarchivedNotification {
                                        thread_id: thread_id.to_string(),
                                    },
                                ))
                                .await;
                        }
                    }
                }
            }
            // No recheck after effects: archiving an ancestor can legitimately
            // retire the issuing parent. Never rewrite that observed outcome or
            // replay the helper because an HTTP observer/parent went away.
            super::response::serialize(request.operation.result())
        };
        work.instrument(request.context.span.clone()).await
    }
}

#[cfg(test)]
impl BackendManagementExecutor {
    pub(crate) async fn hold_actual_list_state_for_test(
        &self,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, tokio::sync::AcquireError> {
        self.processor
            .thread_list_state_permit
            .clone()
            .acquire_owned()
            .await
    }
    pub(crate) async fn archived_state_for_test(
        &self,
        thread_id: ThreadId,
    ) -> codex_thread_store::ThreadStoreResult<bool> {
        self.processor
            .thread_store
            .read_thread(codex_thread_store::ReadThreadParams {
                thread_id,
                include_archived: true,
                include_history: false,
            })
            .await
            .map(|stored| stored.archived_at.is_some())
    }
}
