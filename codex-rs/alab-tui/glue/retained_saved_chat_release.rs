// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::Book;
use crate::app::sidebar_loading::Target;
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::app_server_session::AppServerSession;
use codex_alab_core::diagnostic_id::SAVED_CHAT_RELEASE_PREFIX;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadUnsubscribeResponse;
use codex_app_server_protocol::ThreadUnsubscribeStatus;
use codex_protocol::ThreadId;
use futures::future::BoxFuture;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SavedChatReleaseOutcome {
    Acknowledged(ThreadUnsubscribeStatus),
    Rejected { code: i64 },
    TransportError,
    InvalidReply,
}

/// What the original connection reported for its reserved unsubscribe.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SavedChatReleaseReceipt {
    pub(crate) wire_request: RequestId,
    pub(crate) outcome: SavedChatReleaseOutcome,
    publication: Option<Arc<()>>,
}

impl SavedChatReleaseReceipt {
    fn diagnostic(&self) -> (&'static str, Option<i64>) {
        match &self.outcome {
            SavedChatReleaseOutcome::Acknowledged(ThreadUnsubscribeStatus::Unsubscribed) => {
                ("unsubscribed", None)
            }
            SavedChatReleaseOutcome::Acknowledged(ThreadUnsubscribeStatus::NotSubscribed) => {
                ("notSubscribed", None)
            }
            SavedChatReleaseOutcome::Acknowledged(ThreadUnsubscribeStatus::NotLoaded) => {
                ("notLoaded", None)
            }
            SavedChatReleaseOutcome::Rejected { code } => ("rejected", Some(*code)),
            SavedChatReleaseOutcome::TransportError => ("transportError", None),
            SavedChatReleaseOutcome::InvalidReply => ("invalidReply", None),
        }
    }

    fn record(&self, operation: Uuid, child: ThreadId) {
        let (outcome, server_code) = self.diagnostic();
        if matches!(self.outcome, SavedChatReleaseOutcome::Acknowledged(_)) {
            tracing::info!(%operation, %child, wire_request = %self.wire_request, outcome, "saved chat unsubscribe reply");
        } else {
            tracing::warn!(%operation, %child, wire_request = %self.wire_request, outcome, server_code, "saved chat unsubscribe did not yield a status");
        }
    }

    pub(crate) fn session_log(
        &self,
        timestamp: String,
        operation: Uuid,
        child: ThreadId,
    ) -> serde_json::Value {
        let (outcome, server_code) = self.diagnostic();
        serde_json::json!({
            "ts": timestamp,
            "dir": "to_tui",
            "kind": "app_event",
            "variant": "SavedChatReleased",
            "operation": operation,
            "child": child,
            "wire_request": self.wire_request,
            "outcome": outcome,
            "server_code": server_code,
        })
    }
}

/// The unsubscribe request and the physical session that must account for it.
/// The displayed session can change before the chooser discards the attachment.
pub(crate) struct SavedChatRelease {
    owner: Book,
    target: Target,
    operation: Uuid,
    wire_request: RequestId,
    unsubscribe: BoxFuture<'static, Result<ThreadUnsubscribeResponse, TypedRequestError>>,
    connection_replaced: bool,
}

impl SavedChatRelease {
    pub(crate) fn reserve(server: &AppServerSession, target: Target, operation: Uuid) -> Self {
        let wire_request = RequestId::String(format!("{SAVED_CHAT_RELEASE_PREFIX}{operation}"));
        Self {
            owner: server.alab_original_work.clone(),
            target,
            operation,
            unsubscribe: Box::pin(
                server.thread_unsubscribe_response(target.child, wire_request.clone()),
            ),
            wire_request,
            connection_replaced: false,
        }
    }

    /// A later UI claim on a replacement transport cannot own this subscription.
    pub(crate) fn retire_if_owned_by(&mut self, outgoing: &AppServerSession) -> bool {
        let outgoing_owns_request = self.owns_connection(outgoing);
        self.connection_replaced |= outgoing_owns_request;
        outgoing_owns_request
    }

    pub(crate) fn owns_connection(&self, session: &AppServerSession) -> bool {
        self.owner.belongs_to_connection(session)
    }

    pub(crate) fn same_connection(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.owner.0, &other.owner.0)
    }

    pub(crate) fn connection_replaced(&self) -> bool {
        self.connection_replaced
    }

    pub(crate) fn operation(&self) -> Uuid {
        self.operation
    }

    pub(crate) fn publish(self, sender: AppEventSender) {
        self.send(sender, None);
    }

    pub(crate) fn publish_historical(self, sender: AppEventSender) -> HistoricalRelease {
        let publication = Arc::new(());
        HistoricalRelease {
            target: self.target,
            request: self.operation,
            wire_request: self.wire_request.clone(),
            job: self.send(sender, Some(publication.clone())),
            publication,
            received: false,
        }
    }

    fn send(self, sender: AppEventSender, publication: Option<Arc<()>>) -> Arc<super::Job> {
        self.owner.spawn_native_original(async move {
            let outcome = match self.unsubscribe.await {
                Ok(response) => SavedChatReleaseOutcome::Acknowledged(response.status),
                Err(TypedRequestError::Server { source, .. }) => {
                    SavedChatReleaseOutcome::Rejected { code: source.code }
                }
                Err(TypedRequestError::Transport { .. }) => SavedChatReleaseOutcome::TransportError,
                Err(TypedRequestError::Deserialize { .. }) => SavedChatReleaseOutcome::InvalidReply,
            };
            let receipt = SavedChatReleaseReceipt {
                wire_request: self.wire_request,
                outcome,
                publication,
            };
            receipt.record(self.operation, self.target.child);
            sender.send(AppEvent::SavedChatReleased {
                target: self.target,
                request: self.operation,
                receipt,
            });
        })
    }
}

/// A retired connection can leave the backend registry before this one already
/// reserved unsubscribe runs. Only its own callback and joined job release it.
pub(crate) struct HistoricalRelease {
    target: Target,
    request: Uuid,
    wire_request: RequestId,
    publication: Arc<()>,
    job: Arc<super::Job>,
    received: bool,
}
impl HistoricalRelease {
    pub(crate) async fn observe_and_join(
        &mut self,
        target: Target,
        request: Uuid,
        receipt: &SavedChatReleaseReceipt,
    ) -> bool {
        if self.received
            || (self.target, self.request) != (target, request)
            || self.wire_request != receipt.wire_request
            || !receipt
                .publication
                .as_ref()
                .is_some_and(|p| Arc::ptr_eq(p, &self.publication))
        {
            return false;
        }
        self.received = true;
        // The matching receipt is the original future's last statement. The
        // RPC has already returned. Join only its remaining local completion.
        match self.job.join().await {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(%request, %error, "historical saved release worker could not be joined");
                false
            }
        }
    }
}
