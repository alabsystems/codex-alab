// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real app-server/WebSocket/Responses integration. The parent never activates
//! ALab; only an exact, private child invocation runs the production library server.
//! These tests do not claim TUI rendering or listener replacement coverage.

use super::connection_handling_websocket::WsClient;
use super::connection_handling_websocket::connect_websocket;
use super::connection_handling_websocket::send_initialize_request;
use super::connection_handling_websocket::send_request;
use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use app_test_support::MockResponsesConfig;
use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::get;
use axum::routing::post;
use codex_alab_core::capacity::CAPACITY_RETRY_NOTICE;
use codex_app_server_protocol::CodexErrorInfo;
use codex_app_server_protocol::ErrorNotification;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::ThreadTurnsListParams;
use codex_app_server_protocol::ThreadTurnsListResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnInterruptParams;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_protocol::protocol::SessionSource;
use core_test_support::responses;
use futures::FutureExt;
use futures::StreamExt;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;
use std::collections::VecDeque;
use std::fs::File;
use std::fs::OpenOptions;
use std::future::Future;
use std::io::Write;
use std::net::SocketAddr;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::panic::AssertUnwindSafe;
use std::panic::resume_unwind;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::Command;
use tokio::sync::Notify;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::timeout;

const CHILD_MARKER: &str = "ALAB_CAPACITY_RECONNECT_CHILD";
const PROMPT: &str = "alab-reconnect-capacity-unique-input";
const READ_LIMIT: Duration = Duration::from_secs(60);
const CASE_LIMIT: Duration = Duration::from_secs(60);
const APP_SERVER_IMAGE: &str = "ALAB_CAPACITY_APP_SERVER_IMAGE";

fn fresh_file(path: &Path) -> Result<File> {
    Ok(OpenOptions::new().write(true).create_new(true).open(path)?)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut file = fresh_file(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    writeln!(file)?;
    Ok(())
}

struct Fixture {
    root: PathBuf,
    codex_home: PathBuf,
    work: PathBuf,
    codex_exe: PathBuf,
    sequence: AtomicUsize,
}

impl Fixture {
    fn new(name: &str, provider: &str) -> Result<Self> {
        // Retain from creation, including failed setup and child logs.
        let root = tempfile::Builder::new().prefix(name).tempdir()?.keep();
        let root = root.canonicalize()?;
        for name in [
            "home", "codex", "tmp", "work", "config", "cache", "data", "state", "runtime",
        ] {
            std::fs::create_dir(root.join(name))?;
        }
        let codex_home = root.join("codex");
        MockResponsesConfig::new(provider)
            .with_root_config(
                "thread_unload_delay_secs = 600\ncli_auth_credentials_store = \"file\"",
            )
            .enable_feature(codex_features::Feature::Goals)
            .disable_feature(codex_features::Feature::EnableRequestCompression)
            .disable_feature(codex_features::Feature::ShellSnapshot)
            .disable_feature(codex_features::Feature::ShellSnapshotV2)
            .write(&codex_home)?;
        let codex_exe = match std::env::var_os(APP_SERVER_IMAGE) {
            Some(path) => {
                let path = PathBuf::from(path);
                anyhow::ensure!(
                    path.is_absolute(),
                    "retained app-server image must be absolute"
                );
                anyhow::ensure!(
                    std::fs::symlink_metadata(&path)?.is_file(),
                    "retained app-server image must be regular"
                );
                path.canonicalize()?
            }
            None => codex_utils_cargo_bin::cargo_bin("codex-app-server")?.canonicalize()?,
        };
        eprintln!("ALab retained reconnect fixture: {}", root.display());
        Ok(Self {
            work: root.join("work"),
            root,
            codex_home,
            codex_exe,
            sequence: AtomicUsize::new(1),
        })
    }

    fn next(&self) -> usize {
        self.sequence.fetch_add(1, Ordering::SeqCst)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChildSpec {
    schema: u8,
    root: PathBuf,
    codex_exe: PathBuf,
    active: bool,
}

fn child_name() -> String {
    let Some((_, module)) = module_path!().split_once("::") else {
        panic!("integration module must have a crate prefix")
    };
    format!("{module}::app_server_child")
}

#[tokio::test]
async fn app_server_child() -> Result<()> {
    let Some(marker) = std::env::var_os(CHILD_MARKER) else {
        return Ok(());
    };
    let expected = vec![
        child_name(),
        "--exact".into(),
        "--nocapture".into(),
        "--test-threads=1".into(),
    ];
    assert_eq!(std::env::args().skip(1).collect::<Vec<_>>(), expected);
    let marker = PathBuf::from(marker);
    assert!(marker.is_absolute());
    let metadata = std::fs::symlink_metadata(&marker)?;
    assert!(metadata.is_file() && metadata.len() <= 16 * 1024);
    let spec: ChildSpec = serde_json::from_slice(&std::fs::read(&marker)?)?;
    assert_eq!(spec.schema, 1);
    assert_eq!(spec.root.canonicalize()?, spec.root);
    assert_eq!(
        marker.parent().and_then(Path::parent),
        Some(spec.root.as_path())
    );
    assert_eq!(std::env::current_dir()?, spec.root.join("work"));
    for (key, name) in [
        ("HOME", "home"),
        ("CODEX_HOME", "codex"),
        ("TMPDIR", "tmp"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_RUNTIME_DIR", "runtime"),
    ] {
        assert_eq!(
            std::env::var_os(key).map(PathBuf::from),
            Some(spec.root.join(name))
        );
    }
    for key in ["USER", "LOGNAME"] {
        assert_eq!(std::env::var(key).as_deref(), Ok("alab-test"));
    }
    assert_eq!(spec.codex_exe.canonicalize()?, spec.codex_exe);
    assert!(std::fs::symlink_metadata(&spec.codex_exe)?.is_file());
    assert_ne!(spec.codex_exe, std::env::current_exe()?);
    assert!(
        !codex_alab_core::active(),
        "parent activation must not leak into this process"
    );
    if spec.active {
        codex_alab_core::activate();
    }
    assert_eq!(codex_alab_core::active(), spec.active);
    eprintln!("ALAB_CAPACITY_LIBRARY_SERVER active={}", spec.active);
    let result = timeout(
        Duration::from_secs(300),
        codex_app_server::run_main_with_transport_options(
            codex_arg0::Arg0DispatchPaths {
                codex_self_exe: Some(spec.codex_exe),
                ..Default::default()
            },
            codex_utils_cli::CliConfigOverrides::default(),
            codex_config::LoaderOverrides::without_managed_config_for_tests(),
            false,
            false,
            codex_app_server::AppServerTransport::WebSocket {
                bind_address: "127.0.0.1:0".parse()?,
            },
            SessionSource::VSCode,
            codex_websocket_auth::WebsocketAuthSettings::default(),
            codex_app_server::AppServerRuntimeOptions {
                install_shutdown_signal_handler: false,
                plugin_startup_tasks: codex_app_server::PluginStartupTasks::Skip,
                remote_control_startup_mode:
                    codex_app_server::RemoteControlStartupMode::DisabledEphemeral,
                ..Default::default()
            },
        ),
    )
    .await
    .context("child server exceeded its independent backstop")??;
    bail!("test server unexpectedly returned {result:?}")
}

struct Server {
    process: Child,
    stderr_task: JoinHandle<Result<()>>,
    addr: SocketAddr,
    directory: PathBuf,
}

impl Server {
    async fn start(fixture: &Fixture, active: bool) -> Result<Self> {
        let dir = fixture.root.join(format!("server-{}", fixture.next()));
        std::fs::create_dir(&dir)?;
        let marker = dir.join("child.json");
        write_json(
            &marker,
            &ChildSpec {
                schema: 1,
                root: fixture.root.clone(),
                codex_exe: fixture.codex_exe.clone(),
                active,
            },
        )?;
        let stdout = fresh_file(&dir.join("stdout.log"))?;
        let mut stderr_log = fresh_file(&dir.join("stderr.log"))?;
        let mut process = Command::from(codex_utils_process::background_command(
            std::env::current_exe()?,
        ))
        .args([
            child_name(),
            "--exact".into(),
            "--nocapture".into(),
            "--test-threads=1".into(),
        ])
        .env_clear()
        .env("HOME", fixture.root.join("home"))
        .env("CODEX_HOME", &fixture.codex_home)
        .env("TMPDIR", fixture.root.join("tmp"))
        .env("XDG_CONFIG_HOME", fixture.root.join("config"))
        .env("XDG_CACHE_HOME", fixture.root.join("cache"))
        .env("XDG_DATA_HOME", fixture.root.join("data"))
        .env("XDG_STATE_HOME", fixture.root.join("state"))
        .env("XDG_RUNTIME_DIR", fixture.root.join("runtime"))
        .env("USER", "alab-test")
        .env("LOGNAME", "alab-test")
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("LANG", "C.UTF-8")
        .env("RUST_LOG", "warn")
        .env("RUST_MIN_STACK", "16777216")
        .env(CHILD_MARKER, &marker)
        .current_dir(&fixture.work)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
        let Some(stderr) = process.stderr.take() else {
            let killed = process.start_kill();
            let reaped = timeout(READ_LIMIT, process.wait()).await;
            killed?;
            reaped.context("child stderr setup reap timeout")??;
            bail!("child stderr was not piped");
        };
        let mut lines = BufReader::new(stderr).lines();
        let bound = async {
            let mut witness = false;
            while let Some(line) = lines.next_line().await? {
                writeln!(stderr_log, "{line}")?;
                if line == format!("ALAB_CAPACITY_LIBRARY_SERVER active={active}") {
                    witness = true;
                }
                if let Some(addr) = line
                    .split_whitespace()
                    .find_map(|word| word.strip_prefix("ws://"))
                    .and_then(|word| word.parse::<SocketAddr>().ok())
                {
                    if !witness {
                        bail!(
                            "actual active/inactive library bootstrap witness must precede listener"
                        );
                    }
                    return Ok::<_, anyhow::Error>(addr);
                }
            }
            bail!("child exited without a real listener")
        };
        let startup = timeout(READ_LIMIT, bound)
            .await
            .context("child startup timeout")
            .and_then(|result| result);
        let addr = match startup {
            Ok(addr) => addr,
            Err(error) => {
                // Setup failure still explicitly kills and reaps the owned child.
                let killed = process.start_kill();
                let reaped = timeout(READ_LIMIT, process.wait()).await;
                write_json(
                    &dir.join("startup-failure.json"),
                    &json!({
                        "error": error.to_string(), "killRequested": killed.is_ok(),
                        "reapObserved": matches!(&reaped, Ok(Ok(_))),
                    }),
                )?;
                killed?;
                reaped.context("startup child reap timeout")??;
                return Err(error);
            }
        };
        let stderr_task = tokio::spawn(async move {
            let mut bytes = 0;
            while let Some(line) = lines.next_line().await? {
                bytes += line.len();
                if bytes > 4 * 1024 * 1024 {
                    bail!("unexpected child log growth");
                }
                writeln!(stderr_log, "{line}")?;
            }
            Ok(())
        });
        Ok(Self {
            process,
            stderr_task,
            addr,
            directory: dir,
        })
    }

    async fn stop(mut self) -> Result<()> {
        let pid = self.process.id();
        let pre_stop = self.process.try_wait();
        let killed = self.process.start_kill();
        let reaped = timeout(READ_LIMIT, self.process.wait()).await;
        let mut stderr_task = self.stderr_task;
        let stderr = timeout(READ_LIMIT, &mut stderr_task).await;
        if stderr.is_err() {
            stderr_task.abort();
            let _ = timeout(READ_LIMIT, &mut stderr_task).await;
        }
        write_json(
            &self.directory.join("stop.json"),
            &json!({
                "pid": pid, "killRequested": killed.is_ok(),
                "preStop": match &pre_stop {
                    Ok(None) => json!({"running": true}),
                    Ok(Some(status)) => json!({"running": false, "status": format!("{status:?}")}),
                    Err(error) => json!({"error": error.to_string()}),
                },
                "reapObserved": matches!(&reaped, Ok(Ok(_))),
                "status": reaped.as_ref().ok().and_then(|value| value.as_ref().ok()).map(|value| format!("{value:?}")),
                "stderrDrained": matches!(&stderr, Ok(Ok(Ok(())))),
            }),
        )?;
        // Always complete wait/drain before classifying a premature exit or probe failure.
        let status = reaped.context("owned test server reap timeout")??;
        stderr.context("child stderr shutdown")???;
        if pre_stop
            .context("inspect owned server before shutdown")?
            .is_some()
        {
            bail!("server exited before explicit fixture shutdown: {status:?}");
        }
        killed.context("request owned test server termination")?;
        #[cfg(unix)]
        if status.signal() != Some(libc::SIGKILL) {
            bail!("owned server did not exit from the requested SIGKILL: {status:?}");
        }
        #[cfg(not(unix))]
        if status.success() {
            bail!("server must stay running until explicit fixture shutdown");
        }
        Ok(())
    }
}

enum Reply {
    Capacity(u64),
    Quota,
    Stream(VecDeque<(Option<oneshot::Receiver<()>>, String)>),
}

#[derive(Clone)]
struct ProviderState {
    replies: Arc<Mutex<VecDeque<Reply>>>,
    requests: Arc<Mutex<Vec<Value>>>,
    changed: Arc<Notify>,
}

struct Provider {
    uri: String,
    state: ProviderState,
    task: Option<JoinHandle<()>>,
    shutdown: Option<oneshot::Sender<()>>,
}

impl Drop for Provider {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl Provider {
    async fn start(replies: Vec<Reply>) -> Result<Self> {
        let state = ProviderState {
            replies: Arc::new(Mutex::new(replies.into())),
            requests: Arc::new(Mutex::new(Vec::new())),
            changed: Arc::new(Notify::new()),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let uri = format!("http://{}", listener.local_addr()?);
        let app = Router::new()
            .route(
                "/v1/models",
                get(|| async { Json(json!({"data": [], "object": "list"})) }),
            )
            .route(
                "/v1/responses",
                post(provider_response).get(|| async { StatusCode::UPGRADE_REQUIRED }),
            )
            .with_state(state.clone());
        let (shutdown, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .expect("local mock provider");
        });
        Ok(Self {
            uri,
            state,
            task: Some(task),
            shutdown: Some(shutdown),
        })
    }

    async fn stop(mut self) -> Result<()> {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let mut task = self.task.take().expect("owned provider task");
        match timeout(READ_LIMIT, &mut task).await {
            Ok(result) => {
                result.context("provider task failed")?;
                Ok(())
            }
            Err(error) => {
                task.abort();
                let drained = timeout(READ_LIMIT, &mut task).await;
                let _ = drained.context("provider abort drain timed out")?;
                Err(error).context("provider shutdown timed out")
            }
        }
    }

    fn requests(&self) -> Vec<Value> {
        self.state
            .requests
            .lock()
            .expect("provider request recorder is not poisoned")
            .clone()
    }

    async fn count(&self, count: usize) -> Result<()> {
        timeout(READ_LIMIT, async {
            loop {
                let changed = self.state.changed.notified();
                if self.requests().len() >= count {
                    return;
                }
                changed.await;
            }
        })
        .await
        .context("provider request witness")?;
        assert_eq!(self.requests().len(), count, "unexpected automatic request");
        Ok(())
    }

    fn retain(&self, fixture: &Fixture) -> Result<()> {
        write_json(
            &fixture
                .root
                .join(format!("provider-requests-{}.json", fixture.next())),
            &self.requests(),
        )
    }
}

async fn provider_response(
    State(state): State<ProviderState>,
    Json(body): Json<Value>,
) -> Response {
    state
        .requests
        .lock()
        .expect("provider request recorder is not poisoned")
        .push(body);
    state.changed.notify_one();
    let reply = state
        .replies
        .lock()
        .expect("provider reply queue is not poisoned")
        .pop_front();
    match reply {
        Some(Reply::Capacity(seconds)) => (
            StatusCode::SERVICE_UNAVAILABLE,
            [("retry-after", seconds.to_string())],
            Json(json!({"error": {"code": "server_is_overloaded", "message": "fixture model at capacity"}})),
        ).into_response(),
        Some(Reply::Quota) => (
            [("content-type", "text/event-stream"), ("retry-after", "600")],
            responses::sse_failed("quota", "organization_usage_limit_exceeded", "fixture organization quota"),
        ).into_response(),
        Some(Reply::Stream(chunks)) => {
            let stream = futures::stream::unfold(chunks, |mut chunks| async move {
                let (gate, body) = chunks.pop_front()?;
                if let Some(gate) = gate && gate.await.is_err() { return None; }
                Some((Ok::<_, std::io::Error>(body), chunks))
            });
            Response::builder().header("content-type", "text/event-stream")
                .body(Body::from_stream(stream)).expect("constant SSE header is valid")
        }
        None => (StatusCode::INTERNAL_SERVER_ERROR, "unexpected provider request").into_response(),
    }
}

fn gated_success() -> (Reply, oneshot::Sender<()>, oneshot::Sender<()>) {
    let (progress, progress_gate) = oneshot::channel();
    let (complete, complete_gate) = oneshot::channel();
    (
        Reply::Stream(VecDeque::from([
            (
                None,
                responses::sse(vec![responses::ev_response_created("success")]),
            ),
            (
                Some(progress_gate),
                responses::sse(vec![
                    responses::ev_message_item_added("message", ""),
                    responses::ev_output_text_delta("real progress"),
                ]),
            ),
            (
                Some(complete_gate),
                responses::sse(vec![
                    responses::ev_assistant_message("message", "real progress"),
                    responses::ev_completed("success"),
                ]),
            ),
        ])),
        progress,
        complete,
    )
}

struct Client {
    stream: WsClient,
    log: File,
    stashed: VecDeque<Value>,
}

impl Client {
    async fn connect(server: &Server, fixture: &Fixture, name: &str) -> Result<Self> {
        let mut client = Self {
            stream: connect_websocket(server.addr).await?,
            stashed: VecDeque::new(),
            log: fresh_file(
                &fixture
                    .root
                    .join(format!("client-{name}-{}.jsonl", fixture.next())),
            )?,
        };
        send_initialize_request(&mut client.stream, 1, name).await?;
        client.response(1).await?;
        Ok(client)
    }

    async fn send(&mut self, method: &str, id: i64, params: impl Serialize) -> Result<()> {
        let params = serde_json::to_value(params)?;
        writeln!(
            self.log,
            "{}",
            json!({"direction": "send", "id": id, "method": method, "params": params})
        )?;
        send_request(&mut self.stream, method, id, Some(params)).await
    }

    async fn next(&mut self) -> Result<Value> {
        if let Some(value) = self.stashed.pop_front() {
            return Ok(value);
        }
        self.next_wire().await
    }

    async fn next_wire(&mut self) -> Result<Value> {
        let value = timeout(READ_LIMIT, async {
            loop {
                let message = self
                    .stream
                    .next()
                    .await
                    .context("private websocket closed")??;
                match message {
                    tokio_tungstenite::tungstenite::Message::Text(text) => {
                        return Ok::<Value, anyhow::Error>(serde_json::from_str(&text)?);
                    }
                    tokio_tungstenite::tungstenite::Message::Binary(bytes) => {
                        return Ok(serde_json::from_slice(&bytes)?);
                    }
                    tokio_tungstenite::tungstenite::Message::Close(_) => {
                        bail!("private websocket closed")
                    }
                    _ => {}
                }
            }
        })
        .await
        .context("protocol setup/hang timeout, not an absence verdict")??;
        writeln!(
            self.log,
            "{}",
            json!({"direction": "receive", "message": value})
        )?;
        Ok(value)
    }

    async fn response(&mut self, id: i64) -> Result<(Value, Vec<Value>)> {
        let mut seen = Vec::new();
        loop {
            let value = self.next_wire().await?;
            seen.push(value.clone());
            if value["id"] == id {
                assert!(
                    value.get("error").is_none(),
                    "protocol request failed: {value}"
                );
                return Ok((value["result"].clone(), seen));
            }
        }
    }

    async fn notification(&mut self, method: &str) -> Result<(Value, Vec<Value>)> {
        let mut seen = Vec::new();
        loop {
            let value = self.next().await?;
            seen.push(value.clone());
            if value["method"] == method {
                return Ok((value["params"].clone(), seen));
            }
        }
    }

    async fn resume(
        &mut self,
        thread: &str,
        id: i64,
    ) -> Result<(ThreadResumeResponse, Vec<Value>)> {
        self.send(
            "thread/resume",
            id,
            ThreadResumeParams {
                thread_id: thread.into(),
                exclude_turns: true,
                initial_turns_page: None,
                ..Default::default()
            },
        )
        .await?;
        let (value, messages) = self.response(id).await?;
        let response: ThreadResumeResponse = serde_json::from_value(value)?;
        assert_eq!(response.thread.id, thread);
        assert!(
            response.thread.turns.is_empty(),
            "decisive route must be metadata-only"
        );
        assert!(response.initial_turns_page.is_none());
        Ok((response, messages))
    }

    async fn hydrate(
        &mut self,
        thread: &str,
        id: i64,
    ) -> Result<(ThreadTurnsListResponse, Vec<Value>)> {
        self.send(
            "thread/turns/list",
            id,
            ThreadTurnsListParams {
                thread_id: thread.into(),
                cursor: None,
                limit: Some(16),
                sort_direction: Some(codex_app_server_protocol::SortDirection::Desc),
                items_view: Some(TurnItemsView::NotLoaded),
            },
        )
        .await?;
        let (value, messages) = self.response(id).await?;
        Ok((serde_json::from_value(value)?, messages))
    }
}

fn capacity_errors(messages: &[Value]) -> Vec<ErrorNotification> {
    messages
        .iter()
        .filter(|message| message["method"] == "error")
        .map(|message| {
            serde_json::from_value::<ErrorNotification>(message["params"].clone())
                .expect("error notification has the app-server protocol shape")
        })
        .filter(|error| {
            error.will_retry
                && error.error.codex_error_info == Some(CodexErrorInfo::ServerOverloaded)
        })
        .collect()
}

fn exact_notice(error: &ErrorNotification, thread: &str, turn: &str) {
    assert_eq!(error.thread_id, thread);
    assert_eq!(error.turn_id, turn);
    assert!(error.will_retry);
    assert_eq!(
        error.error.codex_error_info,
        Some(CodexErrorInfo::ServerOverloaded)
    );
    assert_eq!(error.error.message, CAPACITY_RETRY_NOTICE);
}

async fn create_thread(client: &mut Client) -> Result<(String, String)> {
    client
        .send(
            "thread/start",
            10,
            ThreadStartParams {
                model: Some("mock-model".into()),
                ..Default::default()
            },
        )
        .await?;
    let (value, _) = client.response(10).await?;
    let response: ThreadStartResponse = serde_json::from_value(value)?;
    Ok((response.thread.id, response.thread.session_id))
}

async fn start_turn(client: &mut Client, thread: &str, id: i64, prompt: &str) -> Result<String> {
    client
        .send(
            "turn/start",
            id,
            TurnStartParams {
                thread_id: thread.into(),
                input: vec![UserInput::Text {
                    text: prompt.into(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            },
        )
        .await?;
    let (value, messages) = client.response(id).await?;
    // Preserve early events even if turn/start response delivery races the task.
    // The required ordering assertion concerns thread/resume, not turn/start.
    client.stashed.extend(
        messages
            .into_iter()
            .filter(|message| message.get("method").is_some()),
    );
    let response: TurnStartResponse = serde_json::from_value(value)?;
    assert_eq!(response.turn.status, TurnStatus::InProgress);
    Ok(response.turn.id)
}

async fn original_capacity(client: &mut Client, thread: &str, turn: &str) -> Result<()> {
    let (params, _) = client.notification("error").await?;
    let error: ErrorNotification = serde_json::from_value(params)?;
    assert_eq!(error.thread_id, thread);
    assert_eq!(error.turn_id, turn);
    assert!(error.will_retry);
    assert_eq!(error.error.message, CAPACITY_RETRY_NOTICE);
    // Core's existing notify_stream_error wraps the original capacity wait as a
    // disconnected stream. The new reconnect replay has its own explicit kind.
    assert_eq!(
        error.error.codex_error_info,
        Some(CodexErrorInfo::ResponseStreamDisconnected {
            http_status_code: None,
        })
    );
    Ok(())
}

async fn terminal(
    client: &mut Client,
    thread: &str,
    turn: &str,
    status: TurnStatus,
) -> Result<Vec<Value>> {
    let (params, messages) = client.notification("turn/completed").await?;
    let completed: TurnCompletedNotification = serde_json::from_value(params)?;
    assert_eq!(completed.thread_id, thread);
    assert_eq!(completed.turn.id, turn);
    assert_eq!(completed.turn.status, status);
    Ok(messages)
}

async fn interrupt(client: &mut Client, thread: &str, turn: &str, id: i64) -> Result<()> {
    client
        .send(
            "turn/interrupt",
            id,
            TurnInterruptParams {
                thread_id: thread.into(),
                turn_id: turn.into(),
            },
        )
        .await?;
    // The response is not a terminal witness; keep every message and assert the event.
    let (_, seen) = client.response(id).await?;
    if let Some(message) = seen
        .iter()
        .find(|value| value["method"] == "turn/completed")
    {
        let completed: TurnCompletedNotification =
            serde_json::from_value(message["params"].clone())?;
        assert_eq!(completed.thread_id, thread);
        assert_eq!(completed.turn.id, turn);
        assert_eq!(completed.turn.status, TurnStatus::Interrupted);
    } else {
        terminal(client, thread, turn, TurnStatus::Interrupted).await?;
    }
    Ok(())
}

async fn resume_window(
    client: &mut Client,
    thread: &str,
    first: i64,
) -> Result<Vec<ErrorNotification>> {
    let (_, before_response) = client.resume(thread, first).await?;
    assert!(
        capacity_errors(&before_response).is_empty(),
        "no notice before its resume response"
    );
    let (_, mut messages) = client.hydrate(thread, first + 1).await?;
    // Every loaded resume, including an existing subscriber, queues on the same
    // listener. Its response proves the previous inline replay has completed.
    let (_, barrier) = client.resume(thread, first + 2).await?;
    messages.extend(barrier);
    Ok(capacity_errors(&messages))
}

async fn run_case<T, F>(future: F) -> std::thread::Result<Result<T>>
where
    F: Future<Output = Result<T>>,
{
    // Preserve a real failed assertion, but only rethrow it after explicit child
    // cleanup. A timeout is a setup/hang error, never an absence-based verdict.
    AssertUnwindSafe(async {
        timeout(CASE_LIMIT, future)
            .await
            .context("integration setup/hang timeout")?
    })
    .catch_unwind()
    .await
}

fn propagate_case<T>(result: std::thread::Result<Result<T>>) -> Result<T> {
    match result {
        Ok(result) => result,
        Err(panic) => resume_unwind(panic),
    }
}

async fn complete_case(
    group: &str,
    fixture: &Fixture,
    provider: &Provider,
    server: Server,
    result: std::thread::Result<Result<()>>,
) -> Result<()> {
    let stopped = server.stop().await;
    let retention = provider.retain(fixture);
    retention?;
    stopped?;
    propagate_case(result)?;
    assert!(
        !codex_alab_core::active(),
        "parent test process stays inactive"
    );
    write_json(
        &fixture.root.join("completed.json"),
        &json!({"passed": true, "group": group, "scope": "actual app-server raw RPC; not TUI rendering"}),
    )?;
    Ok(())
}

async fn using_provider<F>(replies: Vec<Reply>, body: F) -> Result<()>
where
    F: for<'a> FnOnce(&'a Provider) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>>,
{
    let provider = Provider::start(replies).await?;
    let result = AssertUnwindSafe(body(&provider)).catch_unwind().await;
    let stopped = provider.stop().await;
    stopped?;
    propagate_case(result)
}

#[tokio::test]
async fn metadata_only_disconnect_rejoin_restores_notice_to_exact_connection_and_successor()
-> Result<()> {
    assert!(!codex_alab_core::active());
    let replies = vec![Reply::Capacity(600), Reply::Capacity(600)];
    using_provider(replies, |provider| {
        Box::pin(async move {
            let fixture = Fixture::new("alab-capacity-rejoin-", &provider.uri)?;
            let server = Server::start(&fixture, true).await?;
            let result = run_case(async {
                let mut a = Client::connect(&server, &fixture, "original").await?;
                let (thread, session) = create_thread(&mut a).await?;
                let turn = start_turn(&mut a, &thread, 20, PROMPT).await?;
                original_capacity(&mut a, &thread, &turn).await?;
                // A thread becomes resumable only after its first user turn materializes
                // the rollout. Drain this peer's legitimate initial replay before using
                // its later response as a barrier against accidental broadcasts.
                let mut peer = Client::connect(&server, &fixture, "peer").await?;
                let (_, before_peer) = peer.resume(&thread, 11).await?;
                assert!(capacity_errors(&before_peer).is_empty());
                let (params, _) = peer.notification("error").await?;
                exact_notice(&serde_json::from_value(params)?, &thread, &turn);
                provider.count(1).await?;
                a.stream.close(None).await?;
                drop(a);
                let mut rejoin = Client::connect(&server, &fixture, "rejoin").await?;
                let (response, before) = rejoin.resume(&thread, 30).await?;
                assert_eq!(response.thread.session_id, session, "same loaded runtime");
                assert!(capacity_errors(&before).is_empty());
                let (page, mut window) = rejoin.hydrate(&thread, 31).await?;
                assert!(
                    page.data
                        .iter()
                        .any(|entry| entry.id == turn && entry.status == TurnStatus::InProgress)
                );
                let (_, barrier) = rejoin.resume(&thread, 32).await?;
                window.extend(barrier);
                let notices = capacity_errors(&window);
                assert_eq!(
                    notices.len(),
                    1,
                    "the metadata-only first replay finishes before the next resume response"
                );
                let snapshot = window
                    .iter()
                    .position(|message| {
                        message["method"] == "thread/goal/cleared"
                            && message["params"]["threadId"] == thread
                    })
                    .context("real resumed goal snapshot must be observed")?;
                let retry = window
                    .iter()
                    .position(|message| message["method"] == "error")
                    .context("validated first capacity replay must be present")?;
                assert!(
                    snapshot < retry,
                    "resume snapshot must precede the restored retry notice"
                );
                assert!(
                    before
                        .iter()
                        .chain(window.iter())
                        .all(|message| message["method"] != "turn/started"),
                    "resume/hydration must not manufacture a turn/started event"
                );
                exact_notice(&notices[0], &thread, &turn);
                // Only now ask the peer for its own response barrier; any earlier mistaken
                // broadcast would already precede that response. Its later replay is valid.
                let (_, peer_barrier) = peer.resume(&thread, 40).await?;
                assert!(
                    capacity_errors(&peer_barrier).is_empty(),
                    "rejoin snapshot must not broadcast"
                );
                assert_eq!(
                    provider.requests().len(),
                    1,
                    "resume/hydration must not start model work"
                );
                assert_eq!(provider.requests()[0]["model"], "mock-model");
                assert_eq!(
                    provider.requests()[0]["input"]
                        .to_string()
                        .matches(PROMPT)
                        .count(),
                    1
                );
                interrupt(&mut rejoin, &thread, &turn, 50).await?;
                assert!(resume_window(&mut rejoin, &thread, 60).await?.is_empty());
                let successor = start_turn(&mut rejoin, &thread, 70, "new successor input").await?;
                assert_ne!(successor, turn);
                original_capacity(&mut rejoin, &thread, &successor).await?;
                let notices = resume_window(&mut rejoin, &thread, 80).await?;
                assert_eq!(notices.len(), 1);
                exact_notice(&notices[0], &thread, &successor);
                assert!(notices.iter().all(|notice| notice.turn_id != turn));
                assert_eq!(provider.requests().len(), 2);
                interrupt(&mut rejoin, &thread, &successor, 90).await?;
                Ok::<_, anyhow::Error>(())
            })
            .await;
            complete_case(
                "metadata_only_disconnect_rejoin_restores_notice_to_exact_connection_and_successor",
                &fixture,
                provider,
                server,
                result,
            )
            .await
        })
    })
    .await
}

#[tokio::test]
async fn actual_progress_and_completion_retire_the_capacity_snapshot() -> Result<()> {
    let (success, progress, complete) = gated_success();
    using_provider(vec![Reply::Capacity(1), success], |provider| {
        Box::pin(async move {
            let fixture = Fixture::new("alab-capacity-progress-", &provider.uri)?;
            let server = Server::start(&fixture, true).await?;
            let result = run_case(async {
                let mut client = Client::connect(&server, &fixture, "progress").await?;
                let (thread, _) = create_thread(&mut client).await?;
                let turn = start_turn(&mut client, &thread, 20, PROMPT).await?;
                original_capacity(&mut client, &thread, &turn).await?;
                provider.count(2).await?; // Actual retry, not a sleep-based readiness inference.
                progress
                    .send(())
                    .map_err(|_| anyhow::anyhow!("progress gate closed"))?;
                let (delta, _) = client.notification("item/agentMessage/delta").await?;
                assert_eq!(delta["threadId"], thread);
                assert_eq!(delta["turnId"], turn);
                assert_eq!(delta["delta"], "real progress");
                let mut rejoin = Client::connect(&server, &fixture, "after-progress").await?;
                assert!(resume_window(&mut rejoin, &thread, 30).await?.is_empty());
                assert_eq!(provider.requests().len(), 2);
                assert_eq!(
                    provider.requests()[0]["input"],
                    provider.requests()[1]["input"]
                );
                assert_eq!(
                    provider.requests()[0]["model"],
                    provider.requests()[1]["model"]
                );
                complete
                    .send(())
                    .map_err(|_| anyhow::anyhow!("completion gate closed"))?;
                terminal(&mut rejoin, &thread, &turn, TurnStatus::Completed).await?;
                assert!(resume_window(&mut rejoin, &thread, 40).await?.is_empty());
                Ok::<_, anyhow::Error>(())
            })
            .await;
            complete_case(
                "actual_progress_and_completion_retire_the_capacity_snapshot",
                &fixture,
                provider,
                server,
                result,
            )
            .await
        })
    })
    .await
}

#[tokio::test]
async fn inactive_capacity_and_active_permanent_quota_have_no_live_replay() -> Result<()> {
    for (active, reply) in [(false, Reply::Capacity(600)), (true, Reply::Quota)] {
        using_provider(vec![reply], |provider| {
            Box::pin(async move {
                let fixture = Fixture::new("alab-capacity-terminal-", &provider.uri)?;
                let server = Server::start(&fixture, active).await?;
                let result = run_case(async {
                    let mut client = Client::connect(&server, &fixture, "permanent").await?;
                    let (thread, _) = create_thread(&mut client).await?;
                    let turn = start_turn(&mut client, &thread, 20, PROMPT).await?;
                    let terminal_messages =
                        terminal(&mut client, &thread, &turn, TurnStatus::Failed).await?;
                    let errors = terminal_messages
                        .iter()
                        .filter(|message| message["method"] == "error")
                        .map(|message| {
                            serde_json::from_value::<ErrorNotification>(message["params"].clone())
                                .unwrap()
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(errors.len(), 1);
                    assert!(!errors[0].will_retry);
                    if active {
                        assert_eq!(
                            errors[0].error.codex_error_info,
                            Some(CodexErrorInfo::UsageLimitExceeded)
                        );
                    }
                    assert!(capacity_errors(&terminal_messages).is_empty());
                    assert_eq!(provider.requests().len(), 1);
                    let mut rejoin = Client::connect(&server, &fixture, "terminal-rejoin").await?;
                    assert!(resume_window(&mut rejoin, &thread, 30).await?.is_empty());
                    assert_eq!(provider.requests().len(), 1);
                    Ok::<_, anyhow::Error>(())
                })
                .await;
                complete_case(
                    "inactive_capacity_and_active_permanent_quota_have_no_live_replay",
                    &fixture,
                    provider,
                    server,
                    result,
                )
                .await
            })
        })
        .await?;
    }
    Ok(())
}

#[tokio::test]
async fn cold_process_resume_does_not_invent_a_live_capacity_wait() -> Result<()> {
    using_provider(vec![Reply::Capacity(600)], |provider| {
        Box::pin(async move {
            let fixture = Fixture::new("alab-capacity-cold-", &provider.uri)?;
            let first = Server::start(&fixture, true).await?;
            let prepared = run_case(async {
                let mut client = Client::connect(&first, &fixture, "before-cold").await?;
                let (thread, _) = create_thread(&mut client).await?;
                let turn = start_turn(&mut client, &thread, 20, PROMPT).await?;
                original_capacity(&mut client, &thread, &turn).await?;
                provider.count(1).await?;
                Ok::<_, anyhow::Error>((thread, turn))
            })
            .await;
            let stopped = first.stop().await; // Actual owned child is killed and reaped before new runtime.
            let retained = provider.retain(&fixture);
            retained?;
            stopped?;
            let (thread, turn) = propagate_case(prepared)?;
            let server = Server::start(&fixture, true).await?;
            let result = run_case(async {
                let mut client = Client::connect(&server, &fixture, "cold-rejoin").await?;
                assert!(resume_window(&mut client, &thread, 30).await?.is_empty());
                let (page, _) = client.hydrate(&thread, 40).await?;
                assert!(
                    page.data
                        .iter()
                        .all(|entry| entry.id != turn || entry.status != TurnStatus::InProgress)
                );
                assert_eq!(
                    provider.requests().len(),
                    1,
                    "cold resume must not start a new turn"
                );
                Ok::<_, anyhow::Error>(())
            })
            .await;
            complete_case(
                "cold_process_resume_does_not_invent_a_live_capacity_wait",
                &fixture,
                provider,
                server,
                result,
            )
            .await
        })
    })
    .await
}
