// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Recover selected account identity using only the connected server's account/read.
//! Native request UUIDs remain the account boundary. Each batch is bounded and
//! cancellable; periodic quota reads can retry a failed batch after a cooldown.

use super::App;
use crate::app_event::AppEvent;
use crate::app_server_session::AppServerSession;
use codex_alab_tui::interface;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::Account;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::GetAccountParams;
use codex_app_server_protocol::GetAccountResponse;
use codex_app_server_protocol::RequestId;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use uuid::Uuid;

const READ_TIMEOUT: Duration = Duration::from_secs(30);
const RETRY_COOLDOWN: Duration = Duration::from_secs(60);
const RETRY_DELAYS: [Duration; 3] = [
    Duration::ZERO,
    Duration::from_secs(1),
    Duration::from_secs(4),
];

#[derive(Default)]
pub(super) struct State {
    task: Option<JoinHandle<()>>,
    started_at: Option<Instant>,
}

impl State {
    fn cancel(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.started_at = None;
    }
}

impl Drop for State {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Default)]
struct Identity {
    email: Option<String>,
    workspace: Option<String>,
}

async fn read_identity(
    handle: &AppServerRequestHandle,
    request_id: Uuid,
    attempt: usize,
) -> Result<Identity, ()> {
    tokio::time::timeout(
        READ_TIMEOUT,
        handle.request_typed::<GetAccountResponse>(ClientRequest::GetAccount {
            request_id: RequestId::String(format!("account-status-{request_id}-{attempt}")),
            params: GetAccountParams {
                refresh_token: false,
            },
        }),
    )
    .await
    .map_err(|_| ())?
    .map_err(|_| ())
    .map(|response| match response.account {
        Some(Account::Chatgpt {
            email,
            workspace_name,
            ..
        }) => Identity {
            email: email.filter(|email| !email.trim().is_empty()),
            workspace: crate::app_server_session::workspace_display(
                workspace_name,
                response.workspace_routing,
            ),
        },
        _ => Identity::default(),
    })
}

impl App {
    /// Present transport availability without changing native account, quota or task state.
    pub(super) fn alab_refresh_connection_state(&self) {
        let connection = if !self.reconnect.offline {
            interface::ConnectionState::Connected
        } else if self.reconnect.failed {
            interface::ConnectionState::Offline
        } else {
            interface::ConnectionState::Reconnecting
        };
        interface::set_connection_state(connection);
    }

    /// Usage belongs to the connected account, but its native display cache is
    /// widget-local. A replacement gets a fresh authoritative read rather than
    /// inheriting quota, billing or recovery state from the previous conversation.
    pub(super) fn alab_selected_widget_replaced(&self) {
        if interface::enabled() && self.chat_widget.rate_limit_refresh_interval().is_some() {
            self.app_event_tx.send(AppEvent::RefreshRateLimits {
                origin: crate::app_event::RateLimitRefreshOrigin::Periodic,
            });
        }
    }

    /// Called after native code allocates its request UUID. Off follows the
    /// original one-shot reader unchanged; remember its age for a later toggle.
    pub(super) fn alab_start_account_email_read(
        &mut self,
        app_server: &AppServerSession,
        request_id: Uuid,
    ) -> bool {
        self.alab_start_account_email_read_with_delays(app_server, request_id, RETRY_DELAYS)
    }

    fn alab_start_account_email_read_with_delays(
        &mut self,
        app_server: &AppServerSession,
        request_id: Uuid,
        delays: [Duration; 3],
    ) -> bool {
        if !interface::available() {
            return false;
        }
        self.alab_account_identity.cancel();
        self.alab_account_identity.started_at = Some(Instant::now());
        if !interface::enabled() {
            return false;
        }
        let handle = app_server.request_handle();
        let tx = self.app_event_tx.clone();
        // Failed reads establish no absence. The native UUID rejects this fallback
        // after an account/workspace switch or a disconnected transport.
        let fallback = match self.chat_widget.status_account_display() {
            Some(crate::status::StatusAccountDisplay::ChatGpt {
                email, workspace, ..
            }) => Identity {
                email: email.clone(),
                workspace: workspace.clone(),
            },
            _ => Identity::default(),
        };
        self.alab_account_identity.task = Some(tokio::spawn(async move {
            let mut identity = fallback;
            for (attempt, delay) in delays.into_iter().enumerate() {
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                if !interface::enabled() {
                    break;
                }
                if let Ok(verified) = read_identity(&handle, request_id, attempt).await {
                    identity = verified;
                    break;
                }
            }
            // A failed batch must release the native in-flight slot too. A
            // changed account/disconnected transport rejects this UUID as usual.
            tx.send(AppEvent::AccountEmailLoaded {
                request_id,
                email: identity.email,
                workspace: identity.workspace,
            });
        }));
        true
    }

    pub(super) fn alab_cancel_account_email_read(&mut self) {
        if interface::available() {
            self.alab_account_identity.cancel();
            self.account_email_request_id = None;
        }
    }

    pub(super) fn alab_retry_missing_account_email(&mut self, app_server: &AppServerSession) {
        self.alab_retry_missing_account_email_at(app_server, Instant::now());
    }

    fn alab_retry_missing_account_email_at(&mut self, app_server: &AppServerSession, now: Instant) {
        if !interface::enabled()
            || self.reconnect.offline
            || !self.chat_widget.has_chatgpt_account()
        {
            return;
        }
        if !matches!(
            self.chat_widget.status_account_display(),
            Some(crate::status::StatusAccountDisplay::ChatGpt { email, .. })
                if email.as_ref().is_none_or(|email| email.trim().is_empty())
        ) {
            return;
        }
        let state = &mut self.alab_account_identity;
        if self.account_email_request_id.is_some() {
            if state.task.is_some() {
                return;
            }
            // Native Off reads do not emit failures. Wait beyond their original
            // timeout before replacing a stranded one-shot UUID after toggling on.
            let started = state.started_at.get_or_insert(now);
            if now.saturating_duration_since(*started) < RETRY_COOLDOWN {
                return;
            }
            self.account_email_request_id = None;
        }
        if state
            .started_at
            .is_some_and(|started| now.saturating_duration_since(started) < RETRY_COOLDOWN)
        {
            return;
        }
        self.refresh_account_email(app_server);
    }
}

#[cfg(test)]
#[path = "account_identity_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "interface_ownership_tests.rs"]
mod ownership_tests;

#[cfg(test)]
#[path = "connection_status_tests.rs"]
mod connection_status_tests;
