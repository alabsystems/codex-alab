// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Keep the embedded runtime's event pump alive while exec awaits suspension.

use super::ClientCommand;
use super::InProcessAppServerClient;
use codex_app_server::in_process::InProcessClientSender;
use codex_app_server::in_process::SuspendedExec;
use codex_protocol::ThreadId;
use std::io;
use tokio::sync::oneshot;

pub(super) enum Request {
    BootstrapForest {
        request: codex_app_server::in_process::ForestBootstrapRequest,
        response: oneshot::Sender<io::Result<codex_app_server::in_process::BootstrappedForest>>,
    },
    ActivateForest {
        primary: ThreadId,
        response: oneshot::Sender<io::Result<()>>,
    },
    Bootstrap {
        request: codex_app_server::in_process::ExecBootstrapRequest,
        response: oneshot::Sender<io::Result<codex_app_server::in_process::BootstrappedExec>>,
    },
    WatchQuota {
        thread: ThreadId,
        response: oneshot::Sender<io::Result<codex_core::ExecQuotaWatch>>,
    },
    Suspend {
        thread: ThreadId,
        turn: String,
        configuration: Box<codex_core::config::ExecConfiguration>,
        response: oneshot::Sender<io::Result<SuspendedExec>>,
    },
    ActivateIdleTree {
        thread: ThreadId,
        response: oneshot::Sender<io::Result<()>>,
    },
    ActivateTree {
        thread: ThreadId,
        turn: String,
        response: oneshot::Sender<io::Result<codex_core::StartIfIdleSubmission>>,
    },
}

impl InProcessAppServerClient {
    pub async fn bootstrap_exec(
        &self,
        request: codex_app_server::in_process::ExecBootstrapRequest,
    ) -> io::Result<codex_app_server::in_process::BootstrappedExec> {
        let (response, receive) = oneshot::channel();
        self.command_tx
            .send(ClientCommand::AlabExecSuspend(Request::Bootstrap {
                request,
                response,
            }))
            .await
            .map_err(|_| io::Error::other("exec bootstrap worker is unavailable"))?;
        receive
            .await
            .map_err(|_| io::Error::other("exec bootstrap worker reply was lost"))?
    }

    /// Suspend the named exec turn without stopping this client's event pump.
    ///
    /// This preserves the original thread and turn and waits for its writers.
    /// It neither selects another account nor authorizes resumed execution.
    /// Failure or cancellation cannot be used as proof of completed suspension.
    pub async fn suspend_exec_turn(
        &self,
        thread: ThreadId,
        turn: String,
        configuration: codex_core::config::ExecConfiguration,
    ) -> io::Result<SuspendedExec> {
        let (response, receive) = oneshot::channel();
        self.command_tx
            .send(ClientCommand::AlabExecSuspend(Request::Suspend {
                thread,
                turn,
                configuration: Box::new(configuration),
                response,
            }))
            .await
            .map_err(|_| io::Error::other("exec suspension worker is unavailable"))?;
        receive
            .await
            .map_err(|_| io::Error::other("exec suspension worker reply was lost"))?
    }
}

pub(super) fn start(sender: InProcessClientSender, request: Request) {
    // The worker keeps draining runtime events while this operation waits for
    // root and descendant cleanup. Dropping the caller does not cancel it.
    tokio::spawn(async move {
        match request {
            Request::BootstrapForest { request, response } => {
                let _ = response.send(sender.bootstrap_upgrade_forest(request).await);
            }
            Request::ActivateForest { primary, response } => {
                let _ = response.send(sender.activate_upgrade_forest(primary).await);
            }
            Request::Bootstrap { request, response } => {
                let _ = response.send(sender.bootstrap_exec(request).await);
            }
            Request::WatchQuota { thread, response } => {
                let _ = response.send(sender.watch_exec_quota(thread).await);
            }
            Request::Suspend {
                thread,
                turn,
                configuration,
                response,
            } => {
                let _ = response.send(sender.suspend_exec_turn(thread, turn, *configuration).await);
            }
            Request::ActivateIdleTree { thread, response } => {
                let _ = response.send(sender.activate_idle_exec_tree(thread).await);
            }
            Request::ActivateTree {
                thread,
                turn,
                response,
            } => {
                let _ = response.send(sender.activate_exec_tree(thread, turn).await);
            }
        }
    });
}

impl InProcessAppServerClient {
    /// Subscribe without stopping the embedded runtime's event pump.
    pub async fn watch_exec_quota(
        &self,
        thread: ThreadId,
    ) -> io::Result<codex_core::ExecQuotaWatch> {
        let (response, receive) = oneshot::channel();
        self.command_tx
            .send(ClientCommand::AlabExecSuspend(Request::WatchQuota {
                thread,
                response,
            }))
            .await
            .map_err(|_| io::Error::other("exec quota observation worker is unavailable"))?;
        receive
            .await
            .map_err(|_| io::Error::other("exec quota observation worker reply was lost"))?
    }
}

impl super::InProcessAppServerRequestHandle {
    /// Observe this embedded runtime while the owning client drains events.
    pub async fn watch_exec_quota(
        &self,
        thread: ThreadId,
    ) -> io::Result<codex_core::ExecQuotaWatch> {
        let (response, receive) = oneshot::channel();
        self.command_tx
            .send(ClientCommand::AlabExecSuspend(Request::WatchQuota {
                thread,
                response,
            }))
            .await
            .map_err(|_| io::Error::other("exec quota worker is unavailable"))?;
        receive
            .await
            .map_err(|_| io::Error::other("exec quota worker reply was lost"))?
    }
    /// Stop the original turn without taking ownership of the event receiver.
    pub async fn suspend_exec_turn(
        &self,
        thread: ThreadId,
        turn: String,
        configuration: codex_core::config::ExecConfiguration,
    ) -> io::Result<SuspendedExec> {
        let (response, receive) = oneshot::channel();
        self.command_tx
            .send(ClientCommand::AlabExecSuspend(Request::Suspend {
                thread,
                turn,
                configuration: Box::new(configuration),
                response,
            }))
            .await
            .map_err(|_| io::Error::other("exec suspension worker is unavailable"))?;
        receive
            .await
            .map_err(|_| io::Error::other("exec suspension worker reply was lost"))?
    }
}

impl InProcessAppServerClient {
    pub async fn activate_exec_tree(
        &self,
        thread: ThreadId,
        turn: String,
    ) -> io::Result<codex_core::StartIfIdleSubmission> {
        let (response, receive) = oneshot::channel();
        self.command_tx
            .send(ClientCommand::AlabExecSuspend(Request::ActivateTree {
                thread,
                turn,
                response,
            }))
            .await
            .map_err(|_| io::Error::other("exec tree activation worker is unavailable"))?;
        receive
            .await
            .map_err(|_| io::Error::other("exec tree activation reply was lost"))?
    }
}

impl InProcessAppServerClient {
    pub async fn activate_idle_exec_tree(&self, thread: ThreadId) -> io::Result<()> {
        let (response, receive) = oneshot::channel();
        self.command_tx
            .send(ClientCommand::AlabExecSuspend(Request::ActivateIdleTree {
                thread,
                response,
            }))
            .await
            .map_err(|_| io::Error::other("idle exec activation worker is unavailable"))?;
        receive
            .await
            .map_err(|_| io::Error::other("idle exec activation reply was lost"))?
    }
}

impl InProcessAppServerClient {
    pub async fn bootstrap_upgrade_forest(
        &self,
        request: codex_app_server::in_process::ForestBootstrapRequest,
    ) -> io::Result<codex_app_server::in_process::BootstrappedForest> {
        self.request_handle()
            .bootstrap_upgrade_forest(request)
            .await
    }

    pub async fn activate_upgrade_forest(&self, primary: ThreadId) -> io::Result<()> {
        self.request_handle().activate_upgrade_forest(primary).await
    }
}

impl super::InProcessAppServerRequestHandle {
    pub async fn bootstrap_upgrade_forest(
        &self,
        request: codex_app_server::in_process::ForestBootstrapRequest,
    ) -> io::Result<codex_app_server::in_process::BootstrappedForest> {
        let (response, receive) = oneshot::channel();
        self.command_tx
            .send(ClientCommand::AlabExecSuspend(Request::BootstrapForest {
                request,
                response,
            }))
            .await
            .map_err(|_| io::Error::other("forest bootstrap worker is unavailable"))?;
        receive
            .await
            .map_err(|_| io::Error::other("forest bootstrap reply was lost"))?
    }
    pub async fn activate_upgrade_forest(&self, primary: ThreadId) -> io::Result<()> {
        let (response, receive) = oneshot::channel();
        self.command_tx
            .send(ClientCommand::AlabExecSuspend(Request::ActivateForest {
                primary,
                response,
            }))
            .await
            .map_err(|_| io::Error::other("forest activation worker is unavailable"))?;
        receive
            .await
            .map_err(|_| io::Error::other("forest activation reply was lost"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InProcessClientStartArgs;
    use codex_app_server_protocol::ClientRequest;
    use codex_app_server_protocol::ConfigRequirementsReadResponse;
    use codex_app_server_protocol::RequestId;
    use codex_config::LoaderOverrides;
    use codex_core::config::ConfigBuilder;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn exec_suspension_refusal_keeps_the_embedded_client_usable() {
        // App-server startup has a large future; match the existing core and
        // app-server fixture stack without depending on runner environment.
        std::thread::Builder::new()
            .name("exec-suspension-client".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("test runtime")
                    .block_on(Box::pin(async {
                        let home = tempfile::tempdir().expect("private home");
                        let overrides = vec![
                            ("features.plugins".into(), toml::Value::Boolean(false)),
                            (
                                "features.shell_snapshot".into(),
                                toml::Value::Boolean(false),
                            ),
                        ];
                        let config = ConfigBuilder::default()
                            .codex_home(home.path().to_path_buf())
                            .cli_overrides(overrides.clone())
                            .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
                            .build()
                            .await
                            .expect("isolated configuration");
                        let client = InProcessAppServerClient::start(InProcessClientStartArgs {
                            required_human_approval: false,
                            arg0_paths: Default::default(),
                            config: Arc::new(config.clone()),
                            cli_overrides: overrides,
                            loader_overrides: LoaderOverrides::without_managed_config_for_tests(),
                            strict_config: false,
                            cloud_config_bundle: Default::default(),
                            embedded_network_policy: Default::default(),
                            feedback: codex_feedback::CodexFeedback::new(),
                            log_db: None,
                            state_db: None,
                            environment_manager: Arc::new(
                                codex_exec_server::EnvironmentManager::default_for_tests(),
                            ),
                            config_warnings: Vec::new(),
                            session_source: codex_protocol::protocol::SessionSource::Exec,
                            enable_codex_api_key_env: false,
                            client_name: "codex-exec-suspension-test".into(),
                            client_version: "0.0.0-test".into(),
                            experimental_api: false,
                            mcp_server_openai_form_elicitation: false,
                            opt_out_notification_methods: Vec::new(),
                            channel_capacity: 1,
                        })
                        .await
                        .expect("embedded client");
                        let absent = ThreadId::new();
                        let tree: codex_core::ExecTreeSnapshot = serde_json::from_value(serde_json::json!({
                            "version": 1, "root": absent, "turn": "absent-turn", "threads": [],
                            "inputs": {"version": 2, "frames": [], "idle": [],
                                "input": {"version": 2, "items": [], "publications": [], "group_ends": []}}
                        })).expect("empty untrusted tree");
                        let request = codex_app_server::in_process::ExecBootstrapRequest::new(config.clone(), tree,
                            tokio::time::Instant::now() + Duration::from_secs(10), |_, _| Box::pin(async { panic!("invalid tree reached native admission") }));
                        assert!(client.bootstrap_exec(request).await.is_err());
                        let refusal = tokio::time::timeout(
                            Duration::from_secs(10),
                            client.suspend_exec_turn(ThreadId::new(), "absent-turn".into(),
                                codex_core::config::ExecConfiguration::capture(&config, &Default::default()).expect("invocation")),
                        )
                        .await
                        .expect("suspension must not deadlock the event pump");
                        assert!(
                            refusal.is_err(),
                            "an absent thread cannot yield a shutdown proof"
                        );
                        let refusal = tokio::time::timeout(
                            Duration::from_secs(10),
                            client.watch_exec_quota(ThreadId::new()),
                        )
                        .await
                        .expect("quota subscription must not deadlock the event pump");
                        assert!(
                            refusal.is_err(),
                            "an absent thread cannot expose request identity"
                        );
                        assert!(client.activate_exec_tree(ThreadId::new(), "absent-turn".into()).await.is_err());
                        let _: ConfigRequirementsReadResponse = client
                            .request_typed(ClientRequest::ConfigRequirementsRead {
                                request_id: RequestId::Integer(1),
                                params: None,
                            })
                            .await
                            .expect("rejected suspension must leave ordinary requests usable");
                        client.shutdown().await.expect("client shutdown");
                    }));
            })
            .expect("test thread")
            .join()
            .expect("client suspension test");
    }
}
