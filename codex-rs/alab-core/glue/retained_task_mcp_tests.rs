// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual MessageProcessor START -> private HTTP MCP -> complete CREATE -> real
//! delegated model request. Local protocol fixtures grant no Native SDK, source
//! admission, human approval or ordinary-TUI qualification.

#[path = "retained_side_occurrence_core_tests.rs"]
mod retained_side_occurrence_core_tests;
use super::*;
use crate::message_processor::ConnectionSessionState;
use crate::message_processor::MessageProcessor;
use crate::message_processor::message_processor_tracing_tests::build_test_processor;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::OutgoingEnvelope;
use crate::outgoing_message::OutgoingMessage;
use crate::transport::AppServerTransport;
use crate::transport::ConnectionOrigin;
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use app_test_support::MockResponsesConfig;
use codex_alab_core::native_incoming::Accept;
use codex_alab_core::native_incoming::IncomingStreamProvider;
use codex_alab_core::native_incoming::Withdrawal;
use codex_app_server_protocol::InitializeResponse;
use codex_app_server_protocol::ThreadLoadedListResponse;
use codex_app_server_protocol::ThreadStartResponse;
use codex_config::types::AuthKeyringBackendKind;
use codex_config::types::McpServerTransportConfig;
use codex_config::types::OAuthCredentialsStoreMode;
use codex_core::config::ConfigBuilder;
use codex_core::config::ConfigOverrides;
use codex_exec_server::Environment;
use codex_features::Feature;
use codex_login::AuthManager;
use codex_rmcp_client::ElicitationAction;
use codex_rmcp_client::ElicitationResponse;
use codex_rmcp_client::RmcpClient;
use core_test_support::fs_wait::wait_for_path_exists;
use core_test_support::process::wait_for_pid_file;
use core_test_support::responses;
use core_test_support::stdio_server_bin;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use rmcp::model::ClientCapabilities;
use rmcp::model::Implementation;
use rmcp::model::InitializeRequestParams;
use std::future::Future;
use std::io::Write;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

struct Input;
impl IncomingStreamProvider for Input {
    fn accept(&self) -> Accept<'_> {
        Box::pin(std::future::pending())
    }
    fn withdrawn(&self) -> Withdrawal<'_> {
        Box::pin(std::future::pending())
    }
}

async fn bounded<F: Future>(future: F) -> Result<F::Output> {
    tokio::time::timeout(Duration::from_secs(30), future)
        .await
        .context("task HTTP component hang detector")
}

pub(crate) struct Harness {
    _home: TempDir,
    pub(crate) processor: Arc<MessageProcessor>,
    pub(crate) input: Incoming,
    host: OwnedTaskMcpHost,
    pub(crate) messages: mpsc::UnboundedReceiver<OutgoingEnvelope>,
    drain: tokio::task::JoinHandle<()>,
    server_requests: Arc<AtomicUsize>,
    pub(crate) c1: Arc<ConnectionSessionState>,
    pub(crate) parent_id: ThreadId,
    pub(crate) parent: Arc<CodexThread>,
    root: ThreadStartResponse,
    state_db: Option<Arc<codex_state::StateRuntime>>,
}

impl Harness {
    pub(crate) async fn new(model_uri: &str) -> Result<Self> {
        Self::new_with_persistence(model_uri, false).await
    }

    pub(crate) async fn new_persistent(model_uri: &str) -> Result<Self> {
        Self::new_with_persistence(model_uri, true).await
    }

    async fn new_legacy(model_uri: &str) -> Result<Self> {
        Self::new_with_history_mode(
            model_uri,
            false,
            Some(codex_app_server_protocol::ThreadHistoryMode::Legacy),
        )
        .await
    }

    async fn new_with_persistence(model_uri: &str, persistent: bool) -> Result<Self> {
        Self::new_with_history_mode(model_uri, persistent, None).await
    }

    async fn new_with_history_mode(
        model_uri: &str,
        persistent: bool,
        history_mode: Option<codex_app_server_protocol::ThreadHistoryMode>,
    ) -> Result<Self> {
        Self::new_with_worktree_source(model_uri, persistent, history_mode, false).await
    }

    async fn new_persistent_worktrees(model_uri: &str) -> Result<Self> {
        Self::new_with_worktree_source(model_uri, true, None, true).await
    }

    async fn new_with_worktree_source(
        model_uri: &str,
        persistent: bool,
        history_mode: Option<codex_app_server_protocol::ThreadHistoryMode>,
        install_source: bool,
    ) -> Result<Self> {
        let home = TempDir::new()?;
        let workspace = home.path().join("workspace");
        std::fs::create_dir(&workspace)?;
        MockResponsesConfig::new(model_uri)
            .with_model("mock-model")
            .with_sandbox_mode("read-only")
            .with_root_config("thread_unload_delay_secs = 3600")
            .disable_feature(Feature::CliDaybreak)
            .write(home.path())?;
        let launch_overrides = ConfigOverrides {
            cwd: Some(workspace.clone()),
            ..Default::default()
        };
        let builder = ConfigBuilder::default()
            .codex_home(home.path().to_path_buf())
            .harness_overrides(launch_overrides.clone());
        let mut source_overrides = launch_overrides;
        source_overrides.cwd = None;
        // Keep the actual original builder before consuming the config build.
        let source = install_source.then(|| {
            codex_alab_task_worktree::SourceConfig::new(
                builder.clone().harness_overrides(source_overrides),
            )
        });
        let config = Arc::new(builder.build().await?);
        let auth = AuthManager::shared_from_config(config.as_ref(), false).await?;
        let input = Incoming::new(Input);
        let (listener, address) = bind_listener(&input).await?;
        // Unlike the original None-DB CREATE fixture, these read controls
        // exercise the local store's genuine paginated-history capability.
        let state_db = if persistent {
            Some(
                codex_state::StateRuntime::init(
                    config.sqlite_config().clone(),
                    config.model_provider_id.clone(),
                )
                .await?,
            )
        } else {
            None
        };
        let (processor, mut outgoing) = match state_db.as_ref() {
            Some(state_db) => build_persistent_test_processor(config, auth, state_db.clone()),
            None => build_test_processor(config, auth).await,
        };
        let (mut host, installed) =
            processor.alab_make_task_mcp_host(listener, address, input.clone());
        let installed = installed.and_then(|()| match source {
            Some(source) => processor.alab_install_worktree_source(&host, source),
            None => Ok(()),
        });
        let (send, messages) = mpsc::unbounded_channel();
        let server_requests = Arc::new(AtomicUsize::new(0));
        let counted = server_requests.clone();
        // Keep the real bounded outbox flowing independently of a UI event
        // loop. No DynamicToolThreadStarted or UI ACK implementation exists.
        let mut drain = tokio::spawn(async move {
            while let Some(envelope) = outgoing.recv().await {
                match envelope {
                    OutgoingEnvelope::ToConnection {
                        connection_id,
                        message,
                        write_complete_tx,
                    } => {
                        if matches!(&message, OutgoingMessage::Request(_)) {
                            counted.fetch_add(1, Ordering::Relaxed);
                        }
                        let _ = send.send(OutgoingEnvelope::ToConnection {
                            connection_id,
                            message,
                            write_complete_tx: None,
                        });
                        if let Some(done) = write_complete_tx {
                            let _ = done.send(());
                        }
                    }
                    envelope => {
                        if matches!(
                            &envelope,
                            OutgoingEnvelope::Broadcast {
                                message: OutgoingMessage::Request(_)
                            }
                        ) {
                            counted.fetch_add(1, Ordering::Relaxed);
                        }
                        let _ = send.send(envelope);
                    }
                }
            }
        });
        let c1 = Arc::new(ConnectionSessionState::new(ConnectionOrigin::WebSocket));
        // These are component transport IDs activated by real initialize, not
        // claims of a physically delivered Native frontend or SDK connection.
        let mut setup = Bootstrap {
            processor: processor.clone(),
            messages,
        };
        let bootstrap: Result<(ThreadStartResponse, ThreadId, Arc<CodexThread>)> = async {
            installed?;
            let _: InitializeResponse = setup
                .request(
                    ConnectionId(41),
                    &c1,
                    json!({
                        "id":1, "method":"initialize", "params": {
                            "clientInfo":{"name":"retained-http-c1","version":"1"},
                            "capabilities":{"experimentalApi":true,"requiredHumanApproval":true}
                        }
                    }),
                )
                .await?;
            let mut params = json!({"ephemeral":false,"cwd":workspace});
            if let Some(history_mode) = history_mode {
                params["historyMode"] = serde_json::to_value(history_mode)?;
            }
            let root: ThreadStartResponse = setup
                .request(
                    ConnectionId(41),
                    &c1,
                    json!({
                        "id":2,"method":"thread/start","params":params
                    }),
                )
                .await?;
            ensure!(!root.thread.ephemeral, "root unexpectedly ephemeral");
            let parent_id = ThreadId::from_string(&root.thread.id)?;
            let parent = host.control.manager.get_thread(parent_id).await?;
            ensure!(!parent.has_shutdown_status(), "root already shut down");
            // Success obtains the private route solely from the actual Core config
            // installed by shared thread/start, never manual binding construction.
            ensure!(
                parent
                    .config()
                    .await
                    .mcp_servers
                    .get()
                    .contains_key("codex_tui"),
                "shared START did not install host MCP config"
            );
            ensure!(
                host.control
                    .registry
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .bindings
                    .len()
                    == 1,
                "actual root did not acquire exactly one host binding"
            );
            Ok((root, parent_id, parent))
        }
        .await;
        let (root, parent_id, parent) = match bootstrap {
            Ok(actual) => actual,
            Err(error) => {
                let cleanup = cleanup_actual(&processor, &input, &mut host, &mut drain).await;
                if let Some(state_db) = state_db {
                    state_db.close().await;
                }
                if let Err(cleanup) = cleanup {
                    return Err(
                        error.context(format!("actual bootstrap cleanup failed: {cleanup}"))
                    );
                }
                return Err(error);
            }
        };
        Ok(Self {
            _home: home,
            processor,
            input,
            host,
            messages: setup.messages,
            drain,
            server_requests,
            c1,
            parent_id,
            parent,
            root,
            state_db,
        })
    }

    async fn client(&self, wrong_bearer: bool) -> Result<Arc<RmcpClient>> {
        Self::client_for_thread(&self.parent, wrong_bearer).await
    }

    async fn client_for_thread(
        thread: &Arc<CodexThread>,
        wrong_bearer: bool,
    ) -> Result<Arc<RmcpClient>> {
        let config = thread.config().await;
        let server = config
            .mcp_servers
            .get()
            .get("codex_tui")
            .context("actual private MCP config absent")?;
        let McpServerTransportConfig::StreamableHttp {
            url, http_headers, ..
        } = &server.transport
        else {
            anyhow::bail!("shared START installed a non-HTTP transport");
        };
        let mut headers = http_headers.clone().context("private headers absent")?;
        if wrong_bearer {
            headers.insert("Authorization".into(), "Bearer deliberately-invalid".into());
        }
        let client = Arc::new(
            RmcpClient::new_streamable_http_client(
                "retained-host-component",
                url,
                None,
                Some(headers),
                None,
                OAuthCredentialsStoreMode::File,
                AuthKeyringBackendKind::default(),
                Environment::default_for_tests().get_http_client(),
                None,
            )
            .await?,
        );
        let initialized = bounded(client.initialize(
            InitializeRequestParams::new(
                ClientCapabilities::default(),
                Implementation::new("retained-host-test", "1"),
            ),
            Some(Duration::from_secs(10)),
            Box::new(|_, _| {
                Box::pin(async {
                    Ok(ElicitationResponse {
                        action: ElicitationAction::Cancel,
                        content: None,
                        meta: None,
                    })
                })
            }),
        ))
        .await?;
        if let Err(error) = initialized {
            client.shutdown().await;
            return Err(error);
        }
        Ok(client)
    }

    pub(crate) async fn request<T: serde::de::DeserializeOwned>(
        &mut self,
        id: ConnectionId,
        session: &Arc<ConnectionSessionState>,
        request: Value,
    ) -> Result<T> {
        request_with(&self.processor, &mut self.messages, id, session, request).await
    }

    pub(crate) async fn shutdown(mut self) -> Result<()> {
        let result = cleanup_actual(
            &self.processor,
            &self.input,
            &mut self.host,
            &mut self.drain,
        )
        .await;
        if let Some(state_db) = self.state_db.take() {
            state_db.close().await;
        }
        result
    }
}

// Keep the existing tracing-test helper and original CREATE fixture unchanged.
// This ALab-owned test constructor passes ONE actual DB allocation into the
// ordinary MessageProcessor, which shares it with its real Core/store builders.
fn build_persistent_test_processor(
    config: Arc<codex_core::config::Config>,
    auth_manager: Arc<AuthManager>,
    state_db: Arc<codex_state::StateRuntime>,
) -> (Arc<MessageProcessor>, mpsc::Receiver<OutgoingEnvelope>) {
    let (outgoing_tx, outgoing_rx) = mpsc::channel(16);
    let config_manager = crate::config_manager::ConfigManager::new(
        config.codex_home.to_path_buf(),
        Vec::new(),
        codex_config::LoaderOverrides::with_managed_config_path_for_tests(
            config.codex_home.join("managed_config.toml").to_path_buf(),
        ),
        /*strict_config*/ false,
        codex_config::CloudConfigBundleLoader::default(),
        codex_arg0::Arg0DispatchPaths::default(),
        Arc::new(codex_config::NoopThreadConfigLoader),
    );
    let analytics_events_client = crate::analytics_utils::analytics_events_client_from_config(
        Arc::clone(&auth_manager),
        config.as_ref(),
        /*runtime_auth_slot*/ None,
    );
    let outgoing = Arc::new(crate::outgoing_message::OutgoingMessageSender::new(
        outgoing_tx,
        analytics_events_client.clone(),
    ));
    let processor = Arc::new(MessageProcessor::new(
        crate::message_processor::MessageProcessorArgs {
            cloud_skill_provider: None,
            outgoing,
            analytics_events_client,
            arg0_paths: codex_arg0::Arg0DispatchPaths::default(),
            startup_cwd: Some(config.cwd.to_path_buf()),
            config,
            config_manager,
            environment_manager: Arc::new(
                codex_exec_server::EnvironmentManager::default_for_tests(),
            ),
            feedback: codex_feedback::CodexFeedback::new(),
            log_db: None,
            state_db: Some(state_db),
            config_warnings: Vec::new(),
            startup_warnings_tx: mpsc::unbounded_channel().0,
            session_source: codex_protocol::protocol::SessionSource::VSCode,
            user_verification: Arc::new(crate::user_verification::Service::new(Arc::clone(
                &auth_manager,
            ))),
            auth_manager,
            runtime_auth_slot: None,
            installation_id: "11111111-1111-4111-8111-111111111111".to_string(),
            code_mode_session_provider: None,
            rpc_transport: codex_analytics::AppServerRpcTransport::Stdio,
            remote_control_handle: None,
            plugin_startup_tasks: Some(crate::plugin_config_reload::PluginStartupConfig::Current),
        },
    ));
    (processor, outgoing_rx)
}

async fn cleanup_actual(
    processor: &Arc<MessageProcessor>,
    input: &Incoming,
    host: &mut OwnedTaskMcpHost,
    drain: &mut tokio::task::JoinHandle<()>,
) -> Result<()> {
    input.stop();
    host.stop_admission();
    host.drain_calls().await;
    processor.clear_runtime_references();
    processor.cancel_active_login().await;
    processor.clear_all_thread_listeners().await;
    processor.drain_background_tasks().await;
    processor.shutdown_threads().await;
    let result = bounded(host.finish_after_threads()).await;
    drain.abort();
    let _ = drain.await;
    result?.context("actual task host cleanup")
}

struct Bootstrap {
    processor: Arc<MessageProcessor>,
    messages: mpsc::UnboundedReceiver<OutgoingEnvelope>,
}
impl Bootstrap {
    async fn request<T: serde::de::DeserializeOwned>(
        &mut self,
        id: ConnectionId,
        session: &Arc<ConnectionSessionState>,
        request: Value,
    ) -> Result<T> {
        request_with(&self.processor, &mut self.messages, id, session, request).await
    }
}

async fn request_with<T: serde::de::DeserializeOwned>(
    processor: &Arc<MessageProcessor>,
    messages: &mut mpsc::UnboundedReceiver<OutgoingEnvelope>,
    id: ConnectionId,
    session: &Arc<ConnectionSessionState>,
    request: Value,
) -> Result<T> {
    let request_id = request.get("id").cloned().context("request id missing")?;
    let initialize = request.get("method").and_then(Value::as_str) == Some("initialize");
    processor
        .process_request(
            id,
            serde_json::from_value(request)?,
            &AppServerTransport::WebSocket {
                bind_address: "127.0.0.1:0".parse()?,
            },
            session.clone(),
        )
        .await;
    bounded(async {
        loop {
            let envelope = messages.recv().await.context("actual outbox ended")?;
            let OutgoingEnvelope::ToConnection {
                connection_id,
                message,
                ..
            } = envelope
            else {
                continue;
            };
            if connection_id != id {
                continue;
            }
            match message {
                OutgoingMessage::Response(response)
                    if serde_json::to_value(&response.id)? == request_id =>
                {
                    // The real WebSocket transport registers the parsed
                    // session after delivering its initialize response. Match
                    // that callback before shared START attaches this client.
                    if initialize {
                        processor
                            .connection_initialized(id, session.request_attestation())
                            .await;
                    }
                    return Ok(serde_json::from_value(serde_json::to_value(
                        response.result,
                    )?)?);
                }
                OutgoingMessage::Error(error) if serde_json::to_value(&error.id)? == request_id => {
                    anyhow::bail!("actual RPC rejected request, code {}", error.error.code);
                }
                _ => {}
            }
        }
    })
    .await?
}

fn returned_thread_id(result: &rmcp::model::CallToolResult) -> Result<ThreadId> {
    let value = serde_json::to_value(result)?;
    ensure!(
        value.get("isError").and_then(Value::as_bool) != Some(true),
        "MCP reported failure: {value}"
    );
    let text = value
        .get("content")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str)
        .context("actual MCP result lacks text")?;
    let output: Value = serde_json::from_str(text)?;
    ThreadId::from_string(
        output["threadId"]
            .as_str()
            .context("actual MCP result lacks child ID")?,
    )
    .map_err(Into::into)
}

#[tokio::test]
async fn complete_http_create_survives_c1_close_with_real_child_model_work() -> Result<()> {
    let (release, gate) = oneshot::channel();
    let (model, _) = start_streaming_sse_server(vec![vec![StreamingSseChunk {
        gate: Some(gate),
        body: responses::sse(vec![
            responses::ev_response_created("host-child"),
            responses::ev_assistant_message("host-child", "Done"),
            responses::ev_completed("host-child"),
        ]),
    }]])
    .await;
    let mut h = match Harness::new(model.uri()).await {
        Ok(harness) => harness,
        Err(error) => {
            let _ = release.send(());
            model.shutdown().await;
            return Err(error);
        }
    };
    let barrier_file = h._home.path().join("allow-child-initialize");
    let pid_file = h._home.path().join("child-mcp.pid");
    let mut observed_client = None;
    let mut pending_create = None;
    let result: Result<()> = async {
        let client = h.client(false).await?;
        observed_client = Some(client.clone());
        let catalog = client.list_tools(None, Some(Duration::from_secs(10))).await?;
        ensure!(catalog.tools.iter().any(|tool| tool.name.as_ref() == "create_thread"), "complete CREATE was not advertised");
        let parent_before = h.parent.config_snapshot().await;
        let parent_id = h.parent_id;

        // Root START has finished. Only the child's genuine latest-config
        // reload sees this required process and blocks on its initialize file.
        let blocked = format!(
            r#"
[mcp_servers.blocked]
command = {}
required = true
startup_timeout_sec = 120

[mcp_servers.blocked.env]
MCP_TEST_INITIALIZE_BARRIER_FILE = {}
MCP_TEST_PID_FILE = {}
"#,
            toml::Value::String(stdio_server_bin()?),
            toml::Value::String(barrier_file.to_string_lossy().into_owned()),
            toml::Value::String(pid_file.to_string_lossy().into_owned()),
        );
        std::fs::OpenOptions::new()
            .append(true)
            .open(h._home.path().join("config.toml"))?
            .write_all(blocked.as_bytes())?;
        let create_client = client.clone();
        pending_create = Some(tokio::spawn(async move {
            create_client.call_tool(
                "create_thread".into(),
                Some(json!({
                    "prompt":"Inspect <selected> & report", "title":"Retained HTTP child"
                })),
                move |_| Ok(Some(json!({"threadId":parent_id.to_string()}))),
                Some(Duration::from_secs(90)),
            ).await
        }));
        wait_for_path_exists(&pid_file, Duration::from_secs(10)).await?;
        let pid = wait_for_pid_file(&pid_file).await?;
        ensure!(core_test_support::process::process_is_alive(&pid)?, "child MCP process exited before barrier release");
        ensure!(!barrier_file.exists(), "child MCP initialize barrier was released early");
        ensure!(!pending_create.as_ref().context("CREATE task missing")?.is_finished(), "CREATE completed while required child MCP startup was blocked");
        ensure!(model.requests().await.is_empty(), "child model work started before required MCP initialization");

        bounded(h.processor.connection_closed(ConnectionId(41), &h.c1)).await?;
        ensure!(Arc::ptr_eq(&h.parent, &h.host.control.manager.get_thread(h.parent_id).await?), "C1 close replaced the parent while CREATE was pending");
        let catalog = client.list_tools(None, Some(Duration::from_secs(10))).await?;
        ensure!(catalog.tools.iter().any(|tool| tool.name.as_ref() == "create_thread"), "same HTTP service stopped serving after C1 close");
        let c2 = Arc::new(ConnectionSessionState::new(ConnectionOrigin::WebSocket));
        let _: InitializeResponse = h.request(ConnectionId(42), &c2, json!({
            "id":3,"method":"initialize","params":{"clientInfo":{"name":"retained-http-c2","version":"1"}}
        })).await?;
        let pending_loaded: ThreadLoadedListResponse = h.request(ConnectionId(42), &c2, json!({
            "id":4,"method":"thread/loaded/list","params":{}
        })).await?;
        ensure!(pending_loaded.data.contains(&h.parent_id.to_string()), "C2 did not observe the original source while CREATE was pending");
        ensure!(!pending_create.as_ref().context("CREATE task missing")?.is_finished(), "CREATE did not remain blocked across C1 close and C2 initialize");
        ensure!(core_test_support::process::process_is_alive(&pid)?, "child MCP process exited during frontend replacement");

        std::fs::write(&barrier_file, b"release")?;
        let joined = bounded(pending_create.as_mut().context("CREATE task missing")?).await?;
        drop(pending_create.take());
        let tool = joined.context("HTTP CREATE task failed")?.map_err(|error| {
            let detail = h.host.control.test_last_create_error.lock().unwrap_or_else(PoisonError::into_inner).clone()
                .unwrap_or_else(|| "no backend rejection observed".to_string());
            anyhow::anyhow!("MCP CREATE failed: {error}; backend: {detail}")
        })?;
        let child_id = returned_thread_id(&tool)?;
        ensure!(child_id != h.parent_id, "CREATE returned the source runtime");
        let child = h.host.control.manager.get_thread(child_id).await?;
        ensure!(!child.has_shutdown_status(), "created child is not live");
        let ids = h.host.control.manager.list_thread_ids().await;
        ensure!(ids.len() == 2 && ids.contains(&h.parent_id) && ids.contains(&child_id), "CREATE did not retain exactly the source and one child");
        let snapshot = child.config_snapshot().await;
        ensure!(snapshot.cwd() == parent_before.cwd(), "child cwd did not inherit actual source");
        ensure!(snapshot.sandbox_policy() == parent_before.sandbox_policy(), "child permissions changed from actual source");
        ensure!(snapshot.model_provider_id == parent_before.model_provider_id, "child provider changed");
        ensure!(snapshot.model == parent_before.model, "child model changed without override");
        ensure!(snapshot.approval_policy == parent_before.approval_policy, "child approval policy changed");
        ensure!(snapshot.originator == parent_before.originator, "child source metadata changed");
        bounded(model.wait_for_request_count(1)).await?;
        let requests = model.requests().await;
        ensure!(requests.len() == 1, "CREATE issued another model request before release");
        let body: Value = serde_json::from_slice(&requests[0])?;
        let outputs = body["input"].as_array().context("model request input absent")?.iter()
            .filter(|item| item["type"] == "function_call_output" && item["name"] == "create_thread" && item["namespace"] == "codex_tui")
            .collect::<Vec<_>>();
        ensure!(outputs.len() == 1, "actual delegated input absent or duplicated");
        let delegated = outputs[0]["output"].as_str().context("delegated text missing")?;
        ensure!(delegated.contains(&format!("<source_thread_id>{}</source_thread_id>", h.parent_id)), "source identity changed in real model input");
        ensure!(delegated.contains("Inspect &lt;selected&gt; &amp; report"), "delegated prompt changed");
        ensure!(h.server_requests.load(Ordering::Relaxed) == 0, "CREATE requested a frontend tool/UI acknowledgement");
        ensure!(Arc::ptr_eq(&h.parent, &h.host.control.manager.get_thread(h.parent_id).await?), "CREATE replaced the original source");
        ensure!(Arc::ptr_eq(&child, &h.host.control.manager.get_thread(child_id).await?), "CREATE replaced the child runtime");
        let loaded: ThreadLoadedListResponse = h.request(ConnectionId(42), &c2, json!({
            "id":5,"method":"thread/loaded/list","params":{}
        })).await?;
        ensure!(loaded.data.contains(&h.parent_id.to_string()) && loaded.data.contains(&child_id.to_string()), "C2 did not observe the same loaded source/child");
        ensure!(h.root.thread.id == h.parent_id.to_string(), "root ID changed");
        Ok(())
    }.await;

    // Release both real barriers before joining the HTTP observer or draining
    // accepted host work, including every error after the process was started.
    let barrier_cleanup = std::fs::write(&barrier_file, b"release")
        .context("release required child MCP initialize barrier during cleanup");
    let _ = release.send(());
    let task_cleanup = if let Some(task) = pending_create.as_mut() {
        match bounded(task).await {
            Ok(joined) => {
                drop(pending_create.take());
                joined
                    .map(|_| ())
                    .context("join pending HTTP CREATE observer during cleanup")
            }
            Err(error) => Err(error),
        }
    } else {
        Ok(())
    };
    if let Some(client) = observed_client {
        client.shutdown().await;
    }
    let cleanup = h.shutdown().await;
    model.shutdown().await;
    result.and(barrier_cleanup).and(task_cleanup).and(cleanup)
}

#[tokio::test]
async fn actual_private_http_rejects_wrong_parent_metadata_and_bearer_without_new_child()
-> Result<()> {
    let (model, _) = start_streaming_sse_server(Vec::new()).await;
    let h = match Harness::new(model.uri()).await {
        Ok(harness) => harness,
        Err(error) => {
            model.shutdown().await;
            return Err(error);
        }
    };
    let mut observed_client = None;
    let result: Result<()> = async {
        let client = h.client(false).await?;
        observed_client = Some(client.clone());
        let before = h.host.control.manager.list_thread_ids().await;
        let wrong = ThreadId::new();
        let rejected = bounded(client.call_tool(
            "create_thread".into(),
            Some(json!({"prompt":"Must not run"})),
            move |_| Ok(Some(json!({"threadId":wrong.to_string()}))),
            Some(Duration::from_secs(10)),
        ))
        .await?;
        ensure!(rejected.is_err(), "wrong claimed parent was accepted");
        if let Ok(unexpected) = h.client(true).await {
            unexpected.shutdown().await;
            anyhow::bail!("wrong private bearer initialized");
        }
        let after = h.host.control.manager.list_thread_ids().await;
        ensure!(
            before.len() == 1 && after == before,
            "rejected HTTP request created a Core thread"
        );
        ensure!(
            model.requests().await.is_empty(),
            "rejected request reached the model"
        );
        ensure!(
            h.server_requests.load(Ordering::Relaxed) == 0,
            "rejected request reached frontend acknowledgement"
        );
        Ok(())
    }
    .await;
    if let Some(client) = observed_client {
        client.shutdown().await;
    }
    let cleanup = h.shutdown().await;
    model.shutdown().await;
    result.and(cleanup)
}

#[path = "retained_task_work_lease_tests.rs"]
mod work_lease_tests;

#[path = "retained_task_read_mcp_tests.rs"]
mod read_wait; // ALAB(upgrade): actual private HTTP read/wait and cancellation controls.

#[path = "retained_task_management_mcp_tests.rs"]
mod management_tools;

#[path = "retained_task_send_tests.rs"]
mod send_tests;

#[path = "retained_task_fork_tests.rs"]
mod fork_tests;

#[path = "retained_task_worktree_tests.rs"]
mod worktree_tests;

#[path = "initial_session_missing_origin_tests.rs"]
mod alab_initial_missing_origin; // ALAB(owned-local): actual initialized request cannot fabricate Native provenance.

#[path = "retained_primary_lifetime_tests.rs"]
mod alab_primary_lifetime_tests; // ALAB(owned-local): real C1/C2 idle deadline and exact owned runtime controls.

#[path = "retained_deferred_core_tests.rs"]
mod alab_retained_deferred; // ALAB(retained-history): actual fresh Core writer delegation.

#[path = "client_preparation_missing_origin_tests.rs"]
mod client_preparation_missing_origin;

#[path = "retained_projection_memory_tests.rs"]
mod current_display_memory_tests;

#[path = "retained_side_missing_origin_tests.rs"]
mod retained_side_missing_origin_tests;
