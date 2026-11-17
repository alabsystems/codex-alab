// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Actual tool lifetime, independently of the response-only exec qualification.
//! Uses the same private no-credentials server; this grants no SDK membership.
use super::*;

const SPEC: &str = "ALAB_TOOL_FRONTEND_SPEC";
const ATTACHED: &str = "ALAB_TOOL_FRONTEND_ATTACHED ";
const TOOL_DONE: &str = "the retained tool finished";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    root: PathBuf,
    addr: SocketAddr,
    previous: Option<Identity>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    frontend_pid: u32,
    tool_pid: u32,
    thread: String,
    session: String,
    turn: String,
}
fn child_name() -> String {
    format!(
        "{}::tool_frontend_child",
        module_path!()
            .split_once("::")
            .expect("integration module must have a crate prefix")
            .1
    )
}

#[tokio::test]
async fn tool_frontend_child() -> Result<()> {
    let Some(path) = std::env::var_os(SPEC) else {
        return Ok(());
    };
    assert_eq!(
        std::env::args().skip(1).collect::<Vec<_>>(),
        [
            child_name(),
            "--exact".into(),
            "--nocapture".into(),
            "--test-threads=1".into()
        ]
    );
    let path = PathBuf::from(path);
    let metadata = std::fs::symlink_metadata(&path)?;
    assert!(metadata.is_file() && metadata.len() < 16 * 1024);
    let spec: Spec = serde_json::from_slice(&std::fs::read(&path)?)?;
    assert_eq!(spec.root.canonicalize()?, spec.root);
    assert_eq!(path.parent(), Some(spec.root.as_path()));
    assert!(!codex_alab_core::active());
    let target = spec.previous.is_some();
    let mut client = Client::connect_logged(
        spec.addr,
        fresh_file(&spec.root.join(if target {
            "tool-target.jsonl"
        } else {
            "tool-source.jsonl"
        }))?,
        "tool-frontend",
    )
    .await?;
    let Some(previous) = spec.previous else {
        assert!(!OLD_IMAGE.swap(true, Ordering::SeqCst));
        let (thread, session) = create_thread(&mut client).await?;
        let turn = start_turn(
            &mut client,
            &thread,
            20,
            "retain this actual tool across frontend exec",
        )
        .await?;
        let ready = tokio::fs::File::open(spec.root.join("work/runtime-tool-ready")).await?;
        let mut output = String::new();
        BufReader::new(ready).read_line(&mut output).await?;
        let tool_pid = output
            .trim()
            .strip_prefix("tool-started:")
            .context("actual tool readiness message")?
            .parse::<u32>()?;
        assert!(tool_pid > 1);
        let next = spec.root.join("tool-target.json");
        write_json(
            &next,
            &Spec {
                previous: Some(Identity {
                    frontend_pid: std::process::id(),
                    tool_pid,
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
                child_name(),
                "--exact".into(),
                "--nocapture".into(),
                "--test-threads=1".into(),
            ])
            .env(SPEC, next)
            .exec();
        bail!("tool frontend exec returned: {error}");
    };
    assert_eq!(previous.frontend_pid, std::process::id());
    assert!(!OLD_IMAGE.load(Ordering::SeqCst));
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
    let (value, _) = client.response(30).await?;
    let resumed: ThreadResumeResponse = serde_json::from_value(value)?;
    assert_eq!(resumed.thread.session_id, previous.session);
    let turn = resumed.thread.turns.last().context("retained tool turn")?;
    assert_eq!(turn.id, previous.turn);
    assert_eq!(turn.status, TurnStatus::InProgress);
    let (started, _) = client.notification("item/started").await?;
    let started: codex_app_server_protocol::ItemStartedNotification =
        serde_json::from_value(started)?;
    assert_eq!(started.turn_id, previous.turn);
    assert!(
        matches!(&started.item.payload,
            ThreadItemPayload::CommandExecution {id, process_id: Some(_), status: codex_app_server_protocol::CommandExecutionStatus::InProgress, ..}
            if id == "retained-tool"
        ),
        "resume lost the original pending tool: {started:?}"
    );
    println!("{ATTACHED}{}", serde_json::to_string(&previous)?);
    std::io::stdout().flush()?;
    let (completed, _) = client.notification("turn/completed").await?;
    let completed: TurnCompletedNotification = serde_json::from_value(completed)?;
    assert_eq!(completed.turn.id, previous.turn);
    assert_eq!(completed.turn.status, TurnStatus::Completed);
    client
        .send(
            "thread/read",
            40,
            ThreadReadParams {
                thread_id: previous.thread.clone(),
                include_turns: true,
            },
        )
        .await?;
    let (value, _) = client.response(40).await?;
    let history: ThreadReadResponse = serde_json::from_value(value)?;
    assert_eq!(history.thread.turns.len(), 1);
    assert_eq!(assistant_text(&history.thread.turns[0]), [TOOL_DONE]);
    assert!(history.thread.turns[0].items.iter().any(|item| matches!(&item.payload,
        ThreadItemPayload::CommandExecution {id, exit_code:Some(0), status:codex_app_server_protocol::CommandExecutionStatus::Completed, ..}
        if id=="retained-tool"
    )), "canonical tool completion was lost");
    write_json(&spec.root.join("tool-frontend-finished.json"), &previous)?;
    Ok(())
}

#[tokio::test]
async fn actual_tool_pid_and_single_effect_survive_frontend_exec() -> Result<()> {
    let command = r#"printf 'started:%s\n' "$$" >> runtime-tool-effects
printf 'tool-started:%s\n' "$$" > runtime-tool-ready
IFS= read -r release < runtime-tool-control
test "$release" = continue || exit 9
printf 'finished:%s\n' "$$" >> runtime-tool-effects
printf 'tool-finished:%s\n' "$$""#;
    let call = Reply::Stream(VecDeque::from([(
        None,
        responses::sse(vec![
            responses::ev_response_created("tool-call"),
            responses::ev_function_call(
                "retained-tool",
                "exec_command",
                &json!({"cmd":command,"login":false,"yield_time_ms":30000}).to_string(),
            ),
            responses::ev_completed("tool-call"),
        ]),
    )]));
    let answer = Reply::Stream(VecDeque::from([(
        None,
        responses::sse(vec![
            responses::ev_response_created("tool-answer"),
            responses::ev_assistant_message("tool-answer-message", TOOL_DONE),
            responses::ev_completed("tool-answer"),
        ]),
    )]));
    using_provider(vec![call, answer], |provider| {
        Box::pin(async move {
            let fixture = Fixture::new("alab-runtime-tool-exec-", &provider.uri)?;
            let config_path = fixture.codex_home.join("config.toml");
            let mut config: toml::Value = toml::from_str(&std::fs::read_to_string(&config_path)?)?;
            let table = config.as_table_mut().context("fixture config table")?;
            table.insert("sandbox_mode".into(), "danger-full-access".into());
            table.insert("approval_policy".into(), "never".into());
            table
                .entry("features")
                .or_insert_with(|| toml::Value::Table(Default::default()))
                .as_table_mut()
                .context("fixture feature table")?
                .insert("unified_exec".into(), true.into());
            std::fs::write(config_path, toml::to_string(&config)?)?;
            let fifo = fixture.work.join("runtime-tool-control");
            assert!(
                std::process::Command::new("/usr/bin/mkfifo")
                    .arg(&fifo)
                    .status()?
                    .success()
            );
            // Open both ends before launching. The held reader prevents a recovery
            // cleanup write from blocking if the real child failed before its read.
            let mut control = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&fifo)?;
            let ready_fifo = fixture.work.join("runtime-tool-ready");
            assert!(
                std::process::Command::new("/usr/bin/mkfifo")
                    .arg(&ready_fifo)
                    .status()?
                    .success()
            );
            let _ready_reader = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(ready_fifo)?;
            let mut server = Some(Server::start(&fixture, true).await?);
            let backend_pid = server
                .as_ref()
                .unwrap()
                .process
                .id()
                .context("tool backend PID")?;
            let mut frontend: Option<Child> = None;
            let result = run_case(async {
                let path = fixture.root.join("tool-source.json");
                write_json(
                    &path,
                    &Spec {
                        root: fixture.root.clone(),
                        addr: server.as_ref().unwrap().addr,
                        previous: None,
                    },
                )?;
                frontend = Some(
                    Command::from(codex_utils_process::background_command(
                        std::env::current_exe()?,
                    ))
                    .args([
                        child_name(),
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
                    .env(SPEC, path)
                    .current_dir(&fixture.work)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(fresh_file(&fixture.root.join("tool-frontend-stderr.log"))?)
                    .kill_on_drop(true)
                    .spawn()?,
                );
                let frontend = frontend.as_mut().unwrap();
                let pid = frontend.id().context("tool frontend PID")?;
                let mut output =
                    BufReader::new(frontend.stdout.take().context("tool frontend stdout")?).lines();
                let mut log = fresh_file(&fixture.root.join("tool-frontend-stdout.log"))?;
                let attached: Identity =
                    serde_json::from_value(read_marker(&mut output, &mut log, ATTACHED).await?)?;
                assert_eq!(attached.frontend_pid, pid);
                // This PID came from the running shell's own output before exec.
                assert!(
                    std::process::Command::new("/bin/kill")
                        .args(["-0", &attached.tool_pid.to_string()])
                        .status()?
                        .success()
                );
                assert_eq!(
                    std::fs::read_to_string(fixture.work.join("runtime-tool-effects"))?,
                    format!("started:{}\n", attached.tool_pid)
                );
                assert_eq!(server.as_ref().unwrap().process.id(), Some(backend_pid));
                assert!(server.as_mut().unwrap().process.try_wait()?.is_none());
                assert_eq!(
                    provider.requests().len(),
                    1,
                    "tool must still be held in its original invocation"
                );
                writeln!(control, "continue")?;
                assert!(frontend.wait().await?.success());
                let finished: Identity = serde_json::from_slice(&std::fs::read(
                    fixture.root.join("tool-frontend-finished.json"),
                )?)?;
                assert_eq!(finished.tool_pid, attached.tool_pid);
                assert_eq!(
                    std::fs::read_to_string(fixture.work.join("runtime-tool-effects"))?,
                    format!(
                        "started:{}\nfinished:{}\n",
                        attached.tool_pid, attached.tool_pid
                    )
                );
                let requests = provider.requests();
                assert_eq!(requests.len(), 2);
                let receipts = requests[1]["input"]
                    .as_array()
                    .context("follow-up input")?
                    .iter()
                    .filter(|item| {
                        item["type"] == "function_call_output" && item["call_id"] == "retained-tool"
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    receipts.len(),
                    1,
                    "original tool result must reach the model exactly once"
                );
                assert!(
                    receipts[0]["output"]
                        .as_str()
                        .context("tool output receipt")?
                        .lines()
                        .any(|line| line == format!("tool-finished:{}", attached.tool_pid))
                );
                Ok(())
            })
            .await;
            // Always release our own shell before killing/reaping fixture processes.
            let released = writeln!(control, "continue");
            let frontend_cleanup = async {
                if let Some(frontend) = frontend.as_mut()
                    && frontend.try_wait()?.is_none()
                {
                    let _ = frontend.start_kill();
                    timeout(READ_LIMIT, frontend.wait())
                        .await
                        .context("tool frontend reap")??;
                }
                Ok::<(), anyhow::Error>(())
            }
            .await;
            let backend_cleanup = server.take().unwrap().stop().await;
            let retained = provider.retain(&fixture);
            released?;
            frontend_cleanup?;
            backend_cleanup?;
            retained?;
            propagate_case(result)
        })
    })
    .await
}
