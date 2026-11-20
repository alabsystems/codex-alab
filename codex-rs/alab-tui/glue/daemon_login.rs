// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A shared daemon keeps its authentication manager when another CLI changes the stored
//! login. Attach a new local UI only when that exact connection serves the launching login.

use crate::AppServerTarget;
use crate::app_server_connection;
use crate::legacy_core::config::Config;
use codex_app_server_client::AppServerClient;
use codex_app_server_protocol::AccountSessionsListParams;
use codex_app_server_protocol::AccountSessionsResponse;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_login::CodexAuth;
use std::io;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

/// A recovered transport error can construct an eyre report before the first screen.
pub(super) fn install_interactive_error_hook() -> io::Result<()> {
    static INSTALLED: OnceLock<Result<(), String>> = OnceLock::new();
    match INSTALLED.get_or_init(|| color_eyre::install().map_err(|error| error.to_string())) {
        Ok(()) => Ok(()),
        Err(error) => Err(io::Error::other(error.clone())),
    }
}

pub(crate) struct ReconnectOwner {
    pub(crate) backend: crate::backend_id::BackendId,
    pub(crate) login: Option<Arc<ManagedLogin>>,
}

impl From<crate::backend_id::BackendId> for ReconnectOwner {
    fn from(backend: crate::backend_id::BackendId) -> Self {
        Self {
            backend,
            login: None,
        }
    }
}

impl crate::app_server_session::AppServerSession {
    pub(crate) fn reconnect_owner(&self) -> ReconnectOwner {
        ReconnectOwner {
            backend: self.backend_id(),
            login: self.daemon_login.clone(),
        }
    }
}

enum LoginCheckError {
    Changed,
    Unavailable,
}

pub(crate) struct ManagedLogin {
    session: String,
    user: String,
    workspace: String,
}

impl ManagedLogin {
    fn from_auth(auth: &CodexAuth) -> Option<Self> {
        Self::new(
            auth.managed_account_session_id()?,
            auth.get_chatgpt_user_id()?,
            auth.get_account_id()?,
        )
    }

    pub(crate) fn new(session: String, user: String, workspace: String) -> Option<Self> {
        [&session, &user, &workspace]
            .iter()
            .all(|id| !id.is_empty() && id.trim() == id.as_str())
            .then_some(Self {
                session,
                user,
                workspace,
            })
    }

    fn is_served_by(&self, response: &AccountSessionsResponse) -> bool {
        if response.active_session_id.as_deref() != Some(self.session.as_str()) {
            return false;
        }
        let mut candidates = response
            .sessions
            .iter()
            .filter(|row| row.is_active || row.session_id == self.session);
        let Some(row) = candidates.next() else {
            return false;
        };
        candidates.next().is_none()
            && row.is_active
            && row.session_id == self.session
            && row.user_id.as_deref() == Some(self.user.as_str())
            && row.selected_workspace_account_id.as_deref() == Some(self.workspace.as_str())
    }

    async fn check(&self, client: AppServerClient) -> Result<AppServerClient, LoginCheckError> {
        // This branch of accountSession/list reads auth_cached() without refreshing tokens
        // or contacting the workspace service. It never sends or returns an access token.
        let handle = client.request_handle();
        let request =
            handle.request_typed::<AccountSessionsResponse>(ClientRequest::AccountSessionsList {
                request_id: RequestId::String("tui-local-daemon-login".into()),
                params: AccountSessionsListParams {
                    refresh_workspace_metadata: false,
                },
            });
        // The local RPC has no default deadline. Allow the same time as transport initialization.
        let error = match tokio::time::timeout(Duration::from_secs(10), request).await {
            Ok(Ok(response)) if self.is_served_by(&response) => return Ok(client),
            Ok(Ok(_)) => LoginCheckError::Changed,
            _ => LoginCheckError::Unavailable,
        };
        let _ = client.shutdown().await;
        Err(error)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("The shared daemon's login changed. Reopen Codex to use the current login.")]
pub(crate) struct DaemonLoginChanged;

pub(crate) async fn check_replacement(
    owner: Option<&ManagedLogin>,
    target: &AppServerTarget,
    client: AppServerClient,
) -> color_eyre::Result<AppServerClient> {
    let Some(owner) = owner else {
        return Ok(client);
    };
    if !matches!(target, AppServerTarget::LocalDaemon { .. }) {
        let _ = client.shutdown().await;
        return Err(DaemonLoginChanged.into());
    }
    owner.check(client).await.map_err(|error| match error {
        LoginCheckError::Changed => color_eyre::Report::new(DaemonLoginChanged),
        LoginCheckError::Unavailable => {
            color_eyre::eyre::eyre!("Could not verify the shared daemon's login.")
        }
    })
}

#[derive(Default)]
pub(super) struct Launch {
    pub(super) login: Option<Arc<ManagedLogin>>,
    connection: Option<AppServerClient>,
}

impl Launch {
    pub(super) async fn select(
        implicit: bool,
        target: &mut AppServerTarget,
        config: &Config,
    ) -> io::Result<Self> {
        if codex_alab_core::bound_picker::expected().is_some() {
            if !matches!(target, AppServerTarget::Embedded) || !implicit {
                return Err(codex_alab_core::bound_picker::unavailable());
            }
            // A wrong or vanished file fails the resume before onboarding, a turn, or
            // native account setup could treat it as a fresh sign-in.
            let auth = config
                .auth_config()
                .load_auth(false)
                .await
                .map_err(|_| codex_alab_core::bound_picker::unavailable())?
                .ok_or_else(codex_alab_core::bound_picker::unavailable)?;
            if !auth.is_chatgpt_auth() {
                return Err(codex_alab_core::bound_picker::unavailable());
            }
            return Ok(Self::default());
        }
        let AppServerTarget::LocalDaemon {
            allow_embedded_fallback,
            ..
        } = &*target
        else {
            return Ok(Self::default());
        };
        let allow_embedded_fallback = *allow_embedded_fallback;
        if !implicit || !codex_alab_core::is_alab_binary() {
            return Ok(Self::default());
        }
        let login = config
            .auth_config()
            .load_auth(/*enable_codex_api_key_env*/ false)
            .await
            .ok()
            .flatten()
            .as_ref()
            .and_then(ManagedLogin::from_auth);
        if let Some(login) = login {
            match app_server_connection::connect(target).await {
                Ok(client) => {
                    if let Ok(client) = login.check(client).await {
                        return Ok(Self {
                            login: Some(Arc::new(login)),
                            connection: Some(client),
                        });
                    }
                }
                Err(err) if !allow_embedded_fallback => {
                    return Err(io::Error::other(format!(
                        "{err:#}\n{}",
                        crate::daemon_startup::FAILURE_HINT
                    )));
                }
                Err(_) => {}
            }
        }
        *target = AppServerTarget::Embedded;
        tracing::debug!("using an embedded app server for the launching login");
        Ok(Self::default())
    }

    pub(super) async fn connection(
        &mut self,
        target: &AppServerTarget,
    ) -> color_eyre::Result<Option<AppServerClient>> {
        let Some(login) = &self.login else {
            return Ok(None);
        };
        if !matches!(target, AppServerTarget::LocalDaemon { .. }) {
            return Ok(None);
        }
        if let Some(client) = self.connection.take() {
            return Ok(Some(client));
        }
        // The picker can consume its initial connection. Keep a selected thread on
        // its owning backend and recheck any replacement stream before using it.
        let client = app_server_connection::connect(target).await?;
        check_replacement(Some(login), target, client)
            .await
            .map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_app_server_protocol::AccountSession;

    fn account(session: &str, user: &str, workspace: &str) -> AccountSessionsResponse {
        AccountSessionsResponse {
            active_session_id: Some(session.to_string()),
            sessions: vec![AccountSession {
                session_id: session.to_string(),
                email: Some("same@example.test".to_string()),
                user_id: Some(user.to_string()),
                display_name: None,
                image_url: None,
                last_used_at: 0,
                is_active: true,
                selected_workspace_account_id: Some(workspace.to_string()),
                workspaces: vec![],
            }],
        }
    }

    #[test]
    fn cached_daemon_login_requires_the_same_session_user_and_workspace() {
        let login = ManagedLogin::new("session".into(), "user".into(), "workspace".into()).unwrap();
        assert!(login.is_served_by(&account("session", "user", "workspace")));
        assert!(!login.is_served_by(&account("session", "user", "other-workspace")));
        assert!(!login.is_served_by(&account("other-session", "user", "workspace")));
        assert!(!login.is_served_by(&account("session", "other-user", "workspace")));
    }

    #[test]
    fn missing_or_ambiguous_active_daemon_login_cannot_authorize_attachment() {
        let login = ManagedLogin::new("s".into(), "u".into(), "w".into()).unwrap();
        assert!(ManagedLogin::new("s".into(), " ".into(), "w".into()).is_none());
        let mut response = account("s", "u", "w");
        response.sessions.push(response.sessions[0].clone());
        assert!(!login.is_served_by(&response));
        response.sessions.remove(0);
        response.sessions[0].is_active = false;
        assert!(!login.is_served_by(&response));
        response.sessions[0].is_active = true;
        response.sessions[0].selected_workspace_account_id = None;
        assert!(!login.is_served_by(&response));
        response.active_session_id = None;
        response.sessions.clear();
        assert!(!login.is_served_by(&response));
    }
}
