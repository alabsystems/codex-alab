// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Private-worker feasibility: real Core/Responses work survives a frontend exec.
//! This is an isolated no-credentials provider fixture, not SDK child admission,
//! a production backend launcher, TUI restoration, or native upgrade Go.
use super::*;
use codex_app_server_protocol::ThreadItemPayload;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use std::os::unix::process::CommandExt;
use std::sync::atomic::AtomicBool;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

const FRONTEND_SPEC: &str = "ALAB_RUNTIME_FRONTEND_SPEC";
const READY: &str = "ALAB_FRONTEND_REATTACHED ";
const EXECUTED: &str = "ALAB_FRONTEND_EXECUTED ";
const MIDDLE: &str = "while frontend was detached; ";
const PREFIX: &str = "before frontend replacement; ";
const SUFFIX: &str = "after frontend replacement";
static OLD_IMAGE: AtomicBool = AtomicBool::new(false);

#[path = "runtime_tool_reconnect_tests.rs"]
mod tool;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FrontendSpec {
    root: PathBuf,
    addr: SocketAddr,
    backend_lost: bool,
    previous: Option<Previous>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Previous {
    pid: u32,
    thread: String,
    session: String,
    turn: String,
}

fn frontend_name() -> String {
    format!(
        "{}::frontend_exec_child",
        module_path!()
            .split_once("::")
            .expect("integration module must have a crate prefix")
            .1
    )
}

fn assistant_text(turn: &codex_app_server_protocol::Turn) -> Vec<&str> {
    turn.items
        .iter()
        .filter_map(|item| match &item.payload {
            ThreadItemPayload::AgentMessage { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn frontend_exec_child() -> Result<()> {
    let Some(path) = std::env::var_os(FRONTEND_SPEC) else {
        return Ok(());
    };
    assert_eq!(
        std::env::args().skip(1).collect::<Vec<_>>(),
        [
            frontend_name(),
            "--exact".into(),
            "--nocapture".into(),
            "--test-threads=1".into()
        ]
    );
    let path = PathBuf::from(path);
    let metadata = std::fs::symlink_metadata(&path)?;
    assert!(metadata.is_file() && metadata.len() < 16 * 1024);
    let spec: FrontendSpec = serde_json::from_slice(&std::fs::read(&path)?)?;
    assert_eq!(spec.root.canonicalize()?, spec.root);
    assert_eq!(path.parent(), Some(spec.root.as_path()));
    assert!(!codex_alab_core::active());
    let phase = if spec.previous.is_some() {
        "target"
    } else {
        "source"
    };
    let connect = || {
        Client::connect_logged(
            spec.addr,
            fresh_file(&spec.root.join(format!("frontend-{phase}.jsonl"))).unwrap(),
            phase,
        )
    };
    let Some(previous) = spec.previous else {
        let mut client = connect().await?;
        assert!(!OLD_IMAGE.swap(true, Ordering::SeqCst));
        let (thread, session) = create_thread(&mut client).await?;
        let turn = start_turn(
            &mut client,
            &thread,
            20,
            "preserve this one response across frontend exec",
        )
        .await?;
        let (delta, _) = client.notification("item/agentMessage/delta").await?;
        assert_eq!(delta["delta"], PREFIX);
        let target = spec.root.join("frontend-target.json");
        write_json(
            &target,
            &FrontendSpec {
                previous: Some(Previous {
                    pid: std::process::id(),
                    thread,
                    session,
                    turn,
                }),
                ..spec
            },
        )?;
        client.stream.close(None).await?;
        drop(client);
        let error = std::process::Command::new(std::env::current_exe()?)
            .args([
                frontend_name(),
                "--exact".into(),
                "--nocapture".into(),
                "--test-threads=1".into(),
            ])
            .env(FRONTEND_SPEC, target)
            .exec();
        bail!("frontend exec returned: {error}");
    };
    assert_eq!(
        previous.pid,
        std::process::id(),
        "a spawned replacement is not an exec"
    );
    assert!(
        !OLD_IMAGE.load(Ordering::SeqCst),
        "the old frontend image survived"
    );
    println!("{EXECUTED}{}", json!({"pid":std::process::id()}));
    std::io::stdout().flush()?;
    let mut release = [0u8];
    tokio::io::stdin().read_exact(&mut release).await?;
    assert_eq!(release, [b'\n']);
    let mut client = connect().await?;
    client
        .send(
            "thread/resume",
            30,
            ThreadResumeParams {
                thread_id: previous.thread.clone(),
                ..Default::default()
            },
        )
        .await?;
    let (value, before_reply) = client.response(30).await?;
    assert!(
        !before_reply
            .iter()
            .any(|message| message["method"] == "item/started"),
        "live replay preceded its resume reply"
    );
    let resumed: ThreadResumeResponse = serde_json::from_value(value)?;
    assert_eq!(resumed.thread.session_id, previous.session);
    let running = resumed.thread.turns.last().context("missing live turn")?;
    assert_eq!(running.id, previous.turn);
    assert_eq!(running.status, TurnStatus::InProgress);
    // Persisted history intentionally excludes unfinished assistant text.
    // Its canonical live snapshot must follow the reply on this connection.
    let (started, _) = client.notification("item/started").await?;
    assert_eq!(started["turnId"], previous.turn);
    assert_eq!(started["item"]["id"], "held-message");
    assert_eq!(started["item"]["text"], format!("{PREFIX}{MIDDLE}"));
    assert_eq!(started["item"]["completedAtMs"], Value::Null);
    println!(
        "{READY}{}",
        json!({"pid":std::process::id(), "turn":previous.turn})
    );
    std::io::stdout().flush()?;
    let completed = client.notification("turn/completed").await;
    if spec.backend_lost {
        let error = completed.expect_err("a dead backend cannot complete its held response");
        let closed = error.to_string() == "private websocket closed" || error.downcast_ref::<tokio_tungstenite::tungstenite::Error>().is_some_and(|error| matches!(error,
            tokio_tungstenite::tungstenite::Error::ConnectionClosed |
            tokio_tungstenite::tungstenite::Error::AlreadyClosed |
            tokio_tungstenite::tungstenite::Error::Protocol(tokio_tungstenite::tungstenite::error::ProtocolError::ResetWithoutClosingHandshake)));
        anyhow::ensure!(closed, "not an actual backend closure: {error:#}");
        write_json(
            &spec.root.join("frontend-finished.json"),
            &json!({"backendClosed":true,"pid":std::process::id()}),
        )?;
        return Ok(());
    }
    let (completed, _) = completed?;
    let completed: TurnCompletedNotification = serde_json::from_value(completed)?;
    assert_eq!(completed.turn.id, previous.turn);
    assert_eq!(completed.turn.status, TurnStatus::Completed);
    client
        .send(
            "thread/read",
            40,
            ThreadReadParams {
                thread_id: previous.thread,
                include_turns: true,
            },
        )
        .await?;
    let (read, _) = client.response(40).await?;
    let read: ThreadReadResponse = serde_json::from_value(read)?;
    assert_eq!(read.thread.turns.len(), 1);
    assert_eq!(
        assistant_text(&read.thread.turns[0]),
        [format!("{PREFIX}{MIDDLE}{SUFFIX}").as_str()]
    );
    write_json(
        &spec.root.join("frontend-finished.json"),
        &json!({"completed":true,"pid":std::process::id()}),
    )?;
    Ok(())
}

async fn read_marker<R: tokio::io::AsyncBufRead + Unpin>(
    lines: &mut tokio::io::Lines<R>,
    log: &mut File,
    marker: &str,
) -> Result<Value> {
    loop {
        let line = lines
            .next_line()
            .await?
            .context("frontend exited before its expected phase")?;
        anyhow::ensure!(line.len() < 64 * 1024, "unexpected frontend output growth");
        writeln!(log, "{line}")?;
        if let Some((_, payload)) = line.split_once(marker) {
            return Ok(serde_json::from_str(payload)?);
        }
    }
}

#[tokio::test]
async fn live_response_owner_survives_frontend_exec_and_backend_loss_is_not_recovery() -> Result<()>
{
    assert!(!codex_alab_core::active());
    for backend_lost in [false, true] {
        let (release_middle, middle_gate) = oneshot::channel();
        let (release, gate) = oneshot::channel();
        let reply = Reply::Stream(VecDeque::from([
            (
                None,
                responses::sse(vec![
                    responses::ev_response_created("held-response"),
                    responses::ev_message_item_added("held-message", ""),
                    responses::ev_output_text_delta(PREFIX),
                ]),
            ),
            (
                Some(middle_gate),
                responses::sse(vec![responses::ev_output_text_delta(MIDDLE)]),
            ),
            (
                Some(gate),
                responses::sse(vec![
                    responses::ev_output_text_delta(SUFFIX),
                    responses::ev_assistant_message(
                        "held-message",
                        &format!("{PREFIX}{MIDDLE}{SUFFIX}"),
                    ),
                    responses::ev_completed("held-response"),
                ]),
            ),
        ]));
        using_provider(vec![reply], move |provider| {
            Box::pin(async move {
                let fixture = Fixture::new("alab-runtime-worker-exec-", &provider.uri)?;
                let server = Server::start(&fixture, true).await?;
                let backend_pid = server.process.id().context("backend PID")?;
                let mut server = Some(server);
                let mut frontend: Option<Child> = None;
                let result = run_case(async {
                    let marker = fixture.root.join("frontend-source.json");
                    write_json(
                        &marker,
                        &FrontendSpec {
                            root: fixture.root.clone(),
                            addr: server.as_ref().unwrap().addr,
                            backend_lost,
                            previous: None,
                        },
                    )?;
                    frontend = Some(
                        Command::from(codex_utils_process::background_command(
                            std::env::current_exe()?,
                        ))
                        .args([
                            frontend_name(),
                            "--exact".into(),
                            "--nocapture".into(),
                            "--test-threads=1".into(),
                        ])
                        .env_clear()
                        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
                        .env("HOME", fixture.root.join("home"))
                        .env("CODEX_HOME", &fixture.codex_home)
                        .env("TMPDIR", fixture.root.join("tmp"))
                        .env("RUST_MIN_STACK", "8388608")
                        .env(FRONTEND_SPEC, marker)
                        .current_dir(&fixture.work)
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(fresh_file(&fixture.root.join("frontend-stderr.log"))?)
                        .kill_on_drop(true)
                        .spawn()?,
                    );
                    let frontend = frontend.as_mut().unwrap();
                    let frontend_pid = frontend.id().context("frontend PID")?;
                    let mut output =
                        BufReader::new(frontend.stdout.take().context("frontend output")?).lines();
                    let mut output_log = fresh_file(&fixture.root.join("frontend-stdout.log"))?;
                    let executed = read_marker(&mut output, &mut output_log, EXECUTED).await?;
                    assert_eq!(executed["pid"], frontend_pid);
                    let target: FrontendSpec = serde_json::from_slice(&std::fs::read(
                        fixture.root.join("frontend-target.json"),
                    )?)?;
                    let previous = target.previous.as_ref().context("source checkpoint")?;
                    let mut observer =
                        Client::connect(server.as_ref().unwrap(), &fixture, "backend-observer")
                            .await?;
                    observer
                        .send(
                            "thread/resume",
                            50,
                            ThreadResumeParams {
                                thread_id: previous.thread.clone(),
                                ..Default::default()
                            },
                        )
                        .await?;
                    observer.response(50).await?;
                    let (prefix, _) = observer.notification("item/started").await?;
                    assert_eq!(prefix["item"]["text"], PREFIX);
                    release_middle.send(()).map_err(|_| {
                        anyhow::anyhow!("original response lost before middle chunk")
                    })?;
                    let (middle, _) = observer.notification("item/agentMessage/delta").await?;
                    assert_eq!(middle["delta"], MIDDLE);
                    assert_eq!(middle["turnId"], previous.turn);
                    frontend
                        .stdin
                        .as_mut()
                        .context("frontend control pipe")?
                        .write_all(b"\n")
                        .await?;
                    let ready = read_marker(&mut output, &mut output_log, READY).await?;
                    assert_eq!(ready["pid"], frontend_pid);
                    // A real later response is a FIFO barrier on the observer's
                    // connection; targeted replay must not leak to that peer.
                    observer
                        .send(
                            "thread/read",
                            60,
                            ThreadReadParams {
                                thread_id: previous.thread.clone(),
                                include_turns: false,
                            },
                        )
                        .await?;
                    let (_, observer_window) = observer.response(60).await?;
                    assert!(
                        !observer_window
                            .iter()
                            .any(|message| message["method"] == "item/started"
                                && message["params"]["item"]["id"] == "held-message"),
                        "targeted prefix replay reached another subscriber"
                    );
                    assert_eq!(
                        provider.requests().len(),
                        1,
                        "reattachment must not start a second model request"
                    );
                    assert_eq!(server.as_ref().unwrap().process.id(), Some(backend_pid));
                    assert!(server.as_mut().unwrap().process.try_wait()?.is_none());
                    if backend_lost {
                        server.take().unwrap().stop().await?;
                        drop(release);
                    } else {
                        release
                            .send(())
                            .map_err(|_| anyhow::anyhow!("original model response disconnected"))?;
                    }
                    let status = frontend.wait().await?;
                    assert!(
                        status.success(),
                        "frontend failed: {status}; see {}",
                        fixture.root.display()
                    );
                    let result: Value = serde_json::from_slice(&std::fs::read(
                        fixture.root.join("frontend-finished.json"),
                    )?)?;
                    assert_eq!(
                        result[if backend_lost {
                            "backendClosed"
                        } else {
                            "completed"
                        }],
                        true
                    );
                    assert_eq!(result["pid"], frontend_pid);
                    assert_eq!(provider.requests().len(), 1);
                    Ok(())
                })
                .await;
                let frontend_cleanup = async {
                    let Some(frontend) = frontend.as_mut() else {
                        return Ok(());
                    };
                    match frontend.try_wait()? {
                        Some(_) => Ok(()),
                        None => {
                            let _ = frontend.start_kill();
                            timeout(READ_LIMIT, frontend.wait())
                                .await
                                .context("frontend reap timeout")??;
                            Ok::<(), anyhow::Error>(())
                        }
                    }
                }
                .await;
                let backend_cleanup = match server {
                    Some(server) => server.stop().await,
                    None => Ok(()),
                };
                let retained = provider.retain(&fixture);
                frontend_cleanup?;
                backend_cleanup?;
                retained?;
                propagate_case(result)
            })
        })
        .await?;
    }
    Ok(())
}
