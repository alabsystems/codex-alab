// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual processor/configuration and Unix/WebSocket lifecycle controls. The
//! provider below supplies test-owned IO, never native SDK admission or a
//! fabricated configuration response.
use super::super::prepare;
use super::super::start_retained;
use super::*;
use crate::in_process::InProcessStartArgs;
use crate::outgoing_message::OutgoingEnvelope;
use crate::outgoing_message::OutgoingError;
use crate::outgoing_message::OutgoingMessage;
use crate::outgoing_message::QueuedOutgoingMessage;
use codex_alab_core::native_incoming::Accept;
use codex_alab_core::native_incoming::IncomingStreamProvider;
use codex_alab_core::native_incoming::Withdrawal;
use codex_app_server_protocol::ClientInfo;
use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::InitializeCapabilities;
use codex_app_server_protocol::InitializeParams;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadLoadedListResponse;
use codex_app_server_protocol::ThreadStartResponse;
use codex_arg0::Arg0DispatchPaths;
use codex_config::CloudConfigBundleLoader;
use codex_config::LoaderOverrides;
use codex_core::config::ConfigBuilder;
use codex_exec_server::EnvironmentManager;
use codex_feedback::CodexFeedback;
use codex_protocol::protocol::SessionSource;
use futures::SinkExt;
use futures::StreamExt;
use serde_json::Value;
use serde_json::json;
use std::sync::Mutex;
use std::time::Duration;
use tempfile::TempDir;
use tokio::net::UnixStream;
use tokio::sync::oneshot;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::client_async;
use tokio_tungstenite::tungstenite::Message;

type Wire = WebSocketStream<UnixStream>;

struct Provider {
    queue: Mutex<mpsc::Receiver<io::Result<UnixStream>>>,
    withdrawn: CancellationToken,
}

impl IncomingStreamProvider for Provider {
    fn accept(&self) -> Accept<'_> {
        Box::pin(std::future::poll_fn(|context| {
            self.queue.lock().unwrap().poll_recv(context).map(|next| {
                next.transpose().map(|next| {
                    next.map(codex_alab_core::native_incoming::AcceptedConnection::stream_only)
                })
            })
        }))
    }

    fn withdrawn(&self) -> Withdrawal<'_> {
        Box::pin(self.withdrawn.cancelled())
    }
}

fn input() -> (
    Incoming,
    mpsc::Sender<io::Result<UnixStream>>,
    CancellationToken,
) {
    let (send, receive) = mpsc::channel(4);
    let withdrawn = CancellationToken::new();
    let incoming = Incoming::new(Provider {
        queue: Mutex::new(receive),
        withdrawn: withdrawn.clone(),
    });
    (incoming, send, withdrawn)
}

async fn bounded<F: std::future::Future>(future: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(30), future)
        .await
        .expect("owned dispatcher hang detector")
}

fn initialize() -> InitializeParams {
    InitializeParams {
        client_info: ClientInfo {
            name: "owned-local-wire-control".into(),
            title: Some("Original frontend title".into()),
            version: "1.2.3".into(),
        },
        capabilities: Some(InitializeCapabilities {
            experimental_api: true,
            required_human_approval: true,
            ..Default::default()
        }),
    }
}

async fn arguments() -> (TempDir, InProcessStartArgs) {
    let home = TempDir::new().unwrap();
    std::fs::write(
        home.path().join("config.toml"),
        "model = 'first-config-model'\n",
    )
    .unwrap();
    // Match the existing embedded metadata fixtures: real ConfigBuilder and
    // bootstrap, with shell/plugin work and external provider IO disabled.
    let overrides = vec![
        (
            "features.shell_snapshot".into(),
            toml::Value::Boolean(false),
        ),
        ("features.plugins".into(), toml::Value::Boolean(false)),
        (
            "model_provider".into(),
            toml::Value::String("owned_dispatch_fixture".into()),
        ),
        (
            "model_providers.owned_dispatch_fixture".into(),
            toml::Value::Table(toml::toml! {
                name = "Owned dispatch fixture"
                base_url = "http://127.0.0.1:0/v1"
                supports_websockets = false
                requires_openai_auth = false
            }),
        ),
    ];
    let loader = LoaderOverrides::without_managed_config_for_tests();
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .cli_overrides(overrides.clone())
        .loader_overrides(loader.clone())
        .build()
        .await
        .unwrap();
    let args = InProcessStartArgs {
        arg0_paths: Arg0DispatchPaths::default(),
        config: Arc::new(config),
        cli_overrides: overrides,
        loader_overrides: loader,
        strict_config: false,
        cloud_config_bundle: CloudConfigBundleLoader::default(),
        embedded_network_policy: Default::default(),
        thread_config_loader: Arc::new(codex_config::NoopThreadConfigLoader),
        feedback: CodexFeedback::new(),
        log_db: None,
        state_db: None,
        environment_manager: Arc::new(EnvironmentManager::default_for_tests()),
        config_warnings: Vec::new(),
        session_source: SessionSource::Cli,
        enable_codex_api_key_env: false,
        initialize: initialize(),
        channel_capacity: 8,
    };
    (home, args)
}

async fn response(socket: &mut Wire, id: Value) -> Value {
    bounded(async {
        while let Some(message) = socket.next().await {
            let message = message.unwrap();
            let Message::Text(text) = message else {
                continue;
            };
            let reply: Value = serde_json::from_str(&text).unwrap();
            if reply.get("id") == Some(&id) {
                assert!(reply.get("error").is_none(), "actual RPC returned an error");
                return reply["result"].clone();
            }
        }
        panic!("actual processor closed before its response");
    })
    .await
}

async fn connect(queue: &mpsc::Sender<io::Result<UnixStream>>) -> Wire {
    let (client, server) = UnixStream::pair().unwrap();
    queue.send(Ok(server)).await.unwrap();
    let (mut socket, _) = bounded(client_async("ws://localhost/rpc", client))
        .await
        .unwrap();
    socket
        .send(Message::Text(
            json!({
                "id": "initialize", "method": "initialize", "params": initialize()
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let _ = response(&mut socket, json!("initialize")).await;
    socket
        .send(Message::Text(
            json!({"method": "initialized"}).to_string().into(),
        ))
        .await
        .unwrap();
    socket
}

async fn request(socket: &mut Wire, id: Value, method: &str, params: Value) -> Value {
    socket
        .send(Message::Text(
            json!({"id": id, "method": method, "params": params})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    response(socket, id).await
}

async fn config(socket: &mut Wire) -> ConfigReadResponse {
    serde_json::from_value(
        request(
            socket,
            json!(1),
            "config/read",
            json!({"includeLayers": false, "cwd": null}),
        )
        .await,
    )
    .unwrap()
}

async fn closed(socket: &mut Wire) {
    bounded(async {
        while let Some(message) = socket.next().await {
            if matches!(message, Err(_) | Ok(Message::Close(_))) {
                return;
            }
        }
    })
    .await;
}

async fn clean_prepared(prepared: Prepared, incoming: Incoming) {
    let stop = CancellationToken::new();
    stop.cancel();
    bounded(run(prepared, incoming, stop)).await.unwrap();
}

#[tokio::test]
async fn owned_local_dispatch_human_approval_waits_during_frontend_gap() {
    use codex_app_server_protocol::CommandExecutionApprovalKind;
    use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
    use codex_app_server_protocol::ServerRequestPayload;

    let (_home, args) = arguments().await;
    let (incoming, _queue, _withdrawn) = input();
    let prepared = prepare(args, incoming.clone()).await.unwrap().unwrap();
    let thread = codex_protocol::ThreadId::new();
    let (_id, mut response) = prepared
        .outgoing
        .send_request_to_connections(
            Some(&[]),
            ServerRequestPayload::CommandExecutionRequestApproval(
                CommandExecutionRequestApprovalParams {
                    kind: CommandExecutionApprovalKind::HumanCommand,
                    thread_id: thread.to_string(),
                    turn_id: "gap-turn".into(),
                    item_id: "gap-command".into(),
                    started_at_ms: 0,
                    approval_id: None,
                    environment_id: None,
                    reason: None,
                    network_approval_context: None,
                    command: Some("printf retained-control".into()),
                    cwd: None,
                    command_actions: None,
                    additional_permissions: None,
                    proposed_execpolicy_amendment: None,
                    proposed_network_policy_amendments: None,
                    available_decisions: None,
                },
            ),
            Some(thread),
        )
        .await;
    let pending = matches!(
        response.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    );
    clean_prepared(prepared, incoming).await;
    assert!(
        pending,
        "frontend absence must leave the actual approval pending"
    );
}

#[tokio::test]
async fn owned_local_dispatch_two_clients_keep_host_and_read_current_configuration() {
    let (home, args) = arguments().await;
    let (incoming, queue, withdrawn) = input();
    let owner = start_retained(args, incoming).unwrap();
    let mut first = connect(&queue).await;
    let initial = config(&mut first).await;
    assert_eq!(initial.config.model.as_deref(), Some("first-config-model"));
    assert_eq!(
        initial.config.model_provider.as_deref(),
        Some("owned_dispatch_fixture")
    );
    let started: ThreadStartResponse = serde_json::from_value(
        request(
            &mut first,
            json!(2),
            "thread/start",
            json!({"ephemeral": true}),
        )
        .await,
    )
    .unwrap();
    assert!(started.thread.ephemeral);
    assert!(started.thread.path.is_none());
    let thread_id = started.thread.id;
    first.close(None).await.unwrap();
    closed(&mut first).await;
    drop(first);
    assert!(owner.wait().now_or_never().is_none());

    std::fs::write(
        home.path().join("config.toml"),
        "model = 'second-config-model'\n",
    )
    .unwrap();
    let mut second = connect(&queue).await;
    let current = config(&mut second).await;
    assert_eq!(current.config.model.as_deref(), Some("second-config-model"));
    assert_eq!(
        current.config.model_provider.as_deref(),
        Some("owned_dispatch_fixture")
    );
    // An ephemeral thread has no persisted rollout to cold-resume. A second
    // processor created for C2 cannot reconstruct this live in-memory state.
    let loaded: ThreadLoadedListResponse = serde_json::from_value(
        request(&mut second, json!(2), "thread/loaded/list", json!({})).await,
    )
    .unwrap();
    assert!(loaded.data.contains(&thread_id));
    assert!(!withdrawn.is_cancelled());
    assert!(owner.wait().now_or_never().is_none());
    assert!(bounded(owner.shutdown()).await.is_ok());
    closed(&mut second).await;
}

#[tokio::test]
async fn owned_local_dispatch_provider_withdrawal_closes_live_wire_and_finishes_owner() {
    let (_home, args) = arguments().await;
    let (incoming, queue, withdrawn) = input();
    let owner = start_retained(args, incoming).unwrap();
    let mut socket = connect(&queue).await;
    assert_eq!(
        config(&mut socket).await.config.model.as_deref(),
        Some("first-config-model")
    );
    withdrawn.cancel();
    assert!(bounded(owner.wait()).await.is_ok());
    closed(&mut socket).await;
}

#[tokio::test]
async fn owned_local_dispatch_provider_failure_is_retained_as_the_original_owner_error() {
    let (_home, args) = arguments().await;
    let (incoming, queue, withdrawn) = input();
    let owner = start_retained(args, incoming).unwrap();
    let mut socket = connect(&queue).await;
    let _ = config(&mut socket).await;
    queue
        .send(Err(io::Error::new(
            io::ErrorKind::ConnectionReset,
            "fixture provider failure",
        )))
        .await
        .unwrap();
    let first = bounded(owner.wait()).await;
    let error = first
        .as_ref()
        .as_ref()
        .expect_err("provider failure must not become success");
    assert_eq!(error.kind(), io::ErrorKind::ConnectionReset);
    assert!(!withdrawn.is_cancelled());
    let again = bounded(owner.wait()).await;
    assert!(Arc::ptr_eq(&first, &again));
    closed(&mut socket).await;
}

#[tokio::test]
async fn owned_local_dispatch_rejects_invalid_native_connection_shapes() {
    let (_home, args) = arguments().await;
    let (incoming, _queue, _withdrawn) = input();
    let prepared = prepare(args, incoming.clone()).await.unwrap().unwrap();
    for (origin, has_disconnect) in [
        (ConnectionOrigin::InProcess, true),
        (ConnectionOrigin::WebSocket, false),
    ] {
        let (events, mut receive) = mpsc::channel(1);
        let (controls, _controls_receive) = mpsc::channel(1);
        let (_warnings, warnings_receive) = mpsc::unbounded_channel();
        let (writer, _writer_receive) = mpsc::channel(1);
        let disconnect = CancellationToken::new();
        events
            .send(TransportEvent::ConnectionOpened {
                connection_id: ConnectionId(91),
                origin,
                auth: None,
                resource: None,
                writer,
                disconnect_sender: has_disconnect.then(|| disconnect.clone()),
            })
            .await
            .unwrap();
        let mut connections = Connections::default();
        let result = bounded(dispatch(
            &prepared.processor,
            &prepared.outgoing,
            &controls,
            &mut receive,
            warnings_receive,
            &mut connections,
            &CancellationToken::new(),
        ))
        .await;
        assert!(result.is_err());
        assert!(connections.active.is_empty());
        if has_disconnect {
            assert!(disconnect.is_cancelled());
        }
    }
    clean_prepared(prepared, incoming).await;
}

#[tokio::test]
async fn owned_local_dispatch_prewithdrawn_prepare_returns_no_runtime() {
    let (_home, args) = arguments().await;
    let (incoming, _queue, withdrawn) = input();
    withdrawn.cancel();
    assert!(prepare(args, incoming).await.unwrap().is_none());
}

fn error_message(id: i64) -> OutgoingMessage {
    OutgoingMessage::Error(OutgoingError {
        id: RequestId::Integer(id),
        error: JSONRPCErrorError {
            code: -32603,
            message: "queue control".into(),
            data: None,
        },
    })
}

#[tokio::test]
async fn owned_local_dispatch_full_writer_disconnects_without_stalling_router() {
    let (envelopes, outgoing) = mpsc::channel(1);
    let (controls, control_receive) = mpsc::channel(1);
    let (writer, mut writer_receive) = mpsc::channel(1);
    writer
        .try_send(QueuedOutgoingMessage {
            message: error_message(1),
            write_complete_tx: None,
        })
        .unwrap();
    let disconnect = CancellationToken::new();
    controls
        .send(OutboundControlEvent::Opened {
            connection_id: ConnectionId(7),
            writer,
            disconnect_sender: Some(disconnect.clone()),
            initialized: Arc::new(AtomicBool::new(true)),
            experimental_api_enabled: Arc::new(AtomicBool::new(true)),
            opted_out_notification_methods: Arc::new(RwLock::new(HashSet::new())),
        })
        .await
        .unwrap();
    let routing = tokio::spawn(route(outgoing, control_receive));
    envelopes
        .send(OutgoingEnvelope::ToConnection {
            connection_id: ConnectionId(7),
            message: error_message(2),
            write_complete_tx: None,
        })
        .await
        .unwrap();
    bounded(disconnect.cancelled()).await;
    assert!(writer_receive.recv().await.is_some());
    assert!(bounded(writer_receive.recv()).await.is_none());
    drop(controls);
    drop(envelopes);
    bounded(routing).await.unwrap();
}

struct ListenerCustody {
    _processor: Arc<crate::message_processor::MessageProcessor>,
    dropped: Arc<AtomicBool>,
}

impl Drop for ListenerCustody {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn owned_local_dispatch_listener_cancellation_is_joined_before_cleanup_returns() {
    let (_home, args) = arguments().await;
    let (incoming, _queue, _withdrawn) = input();
    let prepared = prepare(args, incoming.clone()).await.unwrap().unwrap();
    let dropped = Arc::new(AtomicBool::new(false));
    let custody = ListenerCustody {
        _processor: prepared.processor.clone(),
        dropped: dropped.clone(),
    };
    let (entered, started) = oneshot::channel();
    let mut listeners = JoinSet::new();
    listeners.spawn(async move {
        let _custody = custody;
        entered.send(()).unwrap();
        std::future::pending::<()>().await;
    });
    bounded(started).await.unwrap();
    let mut result = Ok(());
    bounded(cancel_listeners(&mut listeners, &mut result)).await;
    assert!(result.is_ok());
    assert!(listeners.is_empty());
    assert!(dropped.load(Ordering::SeqCst));
    clean_prepared(prepared, incoming).await;
}

#[tokio::test]
async fn owned_local_dispatch_router_failure_closes_active_wire_with_provider_still_live() {
    let (_home, args) = arguments().await;
    let (incoming, queue, withdrawn) = input();
    let prepared = prepare(args, incoming.clone()).await.unwrap().unwrap();
    let (fail, failure) = oneshot::channel();
    let running = tokio::spawn(run_with_router(
        prepared,
        incoming,
        CancellationToken::new(),
        move |outgoing, controls| async move {
            tokio::select! {
                () = route(outgoing, controls) => {},
                _ = failure => panic!("injected router failure"),
            }
        },
    ));
    let mut socket = connect(&queue).await;
    let _ = config(&mut socket).await;
    fail.send(()).unwrap();
    assert!(bounded(running).await.unwrap().is_err());
    assert!(!withdrawn.is_cancelled());
    closed(&mut socket).await;
}

#[path = "retained_goal_runtime_tests.rs"]
mod retained_goal_runtime_tests;
