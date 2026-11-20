// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::DynamicToolFunctionSpec;
use codex_app_server_protocol::DynamicToolSpec;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ThreadForkParams;
use codex_app_server_protocol::ThreadForkResponse;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadSampleResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_protocol::models::ResponseItem;
use codex_rollout::RolloutItem;
use codex_rollout::RolloutRecorder;
use core_test_support::responses;
use serde_json::json;
use tempfile::TempDir;
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(60);
const UNKNOWN: &str =
    "No result was recorded in this history. Check the current state before repeating this action.";

async fn start_candidate(home: &Path) -> Result<TestAppServer> {
    let codex = codex_utils_cargo_bin::cargo_bin("codex")?;
    TestAppServer::builder()
        .with_program(&codex)
        .with_plugin_startup_tasks()
        .with_args(&["app-server"])
        .with_codex_home(home)
        .build_initialized_with_timeout(WAIT)
        .await
}

fn inspection_thread(mode: ThreadHistoryMode) -> ThreadStartParams {
    ThreadStartParams {
        history_mode: Some(mode),
        dynamic_tools: Some(vec![DynamicToolSpec::Function(DynamicToolFunctionSpec {
            name: "inspect_package".to_string(),
            description: "Inspect the installation.".to_string(),
            input_schema: json!({"type": "object", "properties": {}}),
            defer_loading: false,
        })]),
        ..Default::default()
    }
}

fn text_turn(thread_id: &str, text: &str) -> TurnStartParams {
    TurnStartParams {
        thread_id: thread_id.to_string(),
        input: vec![UserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }],
        ..Default::default()
    }
}

#[tokio::test]
async fn a_sample_and_fork_report_an_unknown_outcome_while_the_parent_receives_the_result()
-> Result<()> {
    const CALL: &str = "pending-call";
    const RESULT: &str = "Package is installed.";
    let server = responses::start_mock_server().await;
    let model = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(CALL, "inspect_package", "{}"),
                responses::ev_completed("parent"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("sample-answer", "Inspect the installation."),
                responses::ev_completed("sample"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("fork-answer", "Inspect the package."),
                responses::ev_completed("fork"),
            ]),
            responses::sse(vec![responses::ev_completed("parent-done")]),
        ],
    )
    .await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        .enable_feature(Feature::MultiAgentV2)
        .write(home.path())?;
    let mut client = start_candidate(home.path()).await?;
    let thread = client
        .start_thread(inspection_thread(ThreadHistoryMode::Paginated))
        .await?
        .thread;
    let turn_id = client
        .send_turn_start_request(text_turn(&thread.id, "Check the package."))
        .await?;
    let turn: TurnStartResponse = timeout(WAIT, client.read_response(turn_id)).await??;
    let ServerRequest::DynamicToolCall { request_id, .. } =
        timeout(WAIT, client.read_stream_until_request_message()).await??
    else {
        anyhow::bail!("expected a pending inspection");
    };

    // The client holds the tool response until both children have sent their model input.
    let sample_id = client
        .send_raw_request(
            "thread/sample",
            Some(json!({
                "threadId": thread.id, "expectedTurnId": turn.turn.id,
                "allowInProgressTurn": true, "prompt": "What should happen next?"
            })),
        )
        .await?;
    let sample: ThreadSampleResponse = timeout(WAIT, client.read_response(sample_id)).await??;
    assert_eq!(sample.source_turn_id, turn.turn.id);
    let fork_id = client
        .send_thread_fork_request(ThreadForkParams {
            thread_id: thread.id.clone(),
            exclude_turns: true,
            ..Default::default()
        })
        .await?;
    let fork: ThreadForkResponse = timeout(WAIT, client.read_response(fork_id)).await??;
    assert_eq!(
        fork.thread.forked_from_id.as_deref(),
        Some(thread.id.as_str())
    );
    let fork_turn = timeout(
        WAIT,
        client.start_turn_and_wait_for_completion(text_turn(
            &fork.thread.id,
            "Continue in the fork.",
        )),
    )
    .await??;
    assert_eq!(fork_turn.turn.status, TurnStatus::Completed);
    client
        .send_response(
            request_id,
            json!({"contentItems": [{"type":"inputText", "text":RESULT}], "success":true}),
        )
        .await?;
    let completed: TurnCompletedNotification =
        timeout(WAIT, client.read_notification("turn/completed")).await??;
    assert_eq!(completed.turn.id, turn.turn.id);
    assert_eq!(completed.turn.status, TurnStatus::Completed);
    let requests = model.requests();
    let [_, sampled, forked, parent] = requests.as_slice() else {
        anyhow::bail!("expected the initial, sampled, forked and resumed parent requests");
    };
    for child in [sampled, forked] {
        assert_eq!(
            child.function_call_output_text(CALL).as_deref(),
            Some(UNKNOWN)
        );
    }
    assert_eq!(
        parent.function_call_output_text(CALL).as_deref(),
        Some(RESULT)
    );
    assert!(sampled.message_input_texts("developer").iter().any(|text| {
        text.contains("<thread_snapshot>")
            && text.contains(
                "A tool call that was pending at capture may still finish in the source thread.",
            )
    }));
    assert!(!sampled.body_contains_text("<turn_aborted>"));
    assert!(forked.message_input_texts("developer").iter().any(|text| {
        text.contains("<turn_aborted>")
            && text.contains("This history was copied before its last turn had a recorded outcome.")
            && text.contains("A pending tool call may already have acted.")
    }));
    assert!(!forked.body_contains_text("The user interrupted the previous turn on purpose."));
    assert!(!parent.body_contains_text("<turn_aborted>"));
    assert!(client.shutdown_gracefully().await?.success());
    Ok(())
}

#[tokio::test]
async fn cold_restart_keeps_recorded_and_unknown_outcomes_in_legacy_history() -> Result<()> {
    cold_restart(ThreadHistoryMode::Legacy).await
}

#[tokio::test]
async fn cold_restart_keeps_recorded_and_unknown_outcomes_in_paginated_history() -> Result<()> {
    cold_restart(ThreadHistoryMode::Paginated).await
}

async fn cold_restart(mode: ThreadHistoryMode) -> Result<()> {
    const RECORDED: &str = "recorded-inspection";
    const PENDING: &str = "pending-inspection";
    const RESULT: &str = "The previous installation was inspected.";
    let server = responses::start_mock_server().await;
    let model = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(RECORDED, "inspect_package", "{}"),
                responses::ev_completed("recorded"),
            ]),
            responses::sse(vec![
                responses::ev_function_call(PENDING, "inspect_package", "{}"),
                responses::ev_completed("pending"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("recovered-answer", "Inspect the current state."),
                responses::ev_completed("recovered"),
            ]),
        ],
    )
    .await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri()).write(home.path())?;
    let mut original = start_candidate(home.path()).await?;
    let thread = original.start_thread(inspection_thread(mode)).await?.thread;
    let start = original
        .send_turn_start_request(text_turn(&thread.id, "Inspect both installations."))
        .await?;
    let turn: TurnStartResponse = timeout(WAIT, original.read_response(start)).await??;
    for expected in [RECORDED, PENDING] {
        let ServerRequest::DynamicToolCall { request_id, params } =
            timeout(WAIT, original.read_stream_until_request_message()).await??
        else {
            anyhow::bail!("expected a dynamic tool call for {expected}");
        };
        assert_eq!(params.thread_id, thread.id);
        assert_eq!(params.turn_id, turn.turn.id);
        assert_eq!(params.call_id, expected);
        if expected == RECORDED {
            original
                .send_response(
                    request_id,
                    json!({"contentItems": [{"type":"inputText", "text":RESULT}], "success":true}),
                )
                .await?;
        }
    }

    // A delivered client request alone does not establish that its model call reached disk.
    let path = thread.path.as_deref().context("missing rollout path")?;
    timeout(WAIT, async {
        loop {
            match RolloutRecorder::load_rollout_items(path).await {
                Ok((items, _, _)) => {
                    let mut recorded_output = false;
                    let mut pending_call = false;
                    for item in items {
                        if let RolloutItem::ResponseItem(envelope) = item {
                            match &envelope.item {
                                ResponseItem::FunctionCall { call_id, .. }
                                    if call_id == PENDING =>
                                {
                                    pending_call = true;
                                }
                                ResponseItem::FunctionCallOutput {
                                    call_id: Some(call_id),
                                    ..
                                } => {
                                    assert_ne!(
                                        call_id, PENDING,
                                        "unanswered call has a recorded result"
                                    );
                                    recorded_output |= call_id == RECORDED;
                                }
                                _ => {}
                            }
                        }
                    }
                    if recorded_output && pending_call {
                        return Ok::<_, anyhow::Error>(());
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err.into()),
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("pending call and previous result did not reach the rollout")??;
    timeout(WAIT, original.kill_and_reap()).await??;
    drop(original);

    let mut replacement = start_candidate(home.path()).await?;
    let resume = replacement
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: thread.id.clone(),
            exclude_turns: true,
            ..Default::default()
        })
        .await?;
    let resumed: ThreadResumeResponse = timeout(WAIT, replacement.read_response(resume)).await??;
    assert_eq!(resumed.thread.id, thread.id);
    assert_eq!(resumed.thread.history_mode, mode);
    let next = replacement
        .send_turn_start_request(text_turn(&thread.id, "Continue from the recorded results."))
        .await?;
    let next: TurnStartResponse = timeout(WAIT, replacement.read_response(next)).await??;
    timeout(WAIT, async {
        loop {
            match replacement.read_next_message().await? {
                JSONRPCMessage::Request(request) if request.method == "item/tool/call" => {
                    anyhow::bail!("replacement dispatched a tool again: {:?}", request.params);
                }
                JSONRPCMessage::Notification(note) if note.method == "turn/completed" => {
                    let completed: TurnCompletedNotification = serde_json::from_value(
                        note.params.context("missing turn/completed params")?,
                    )?;
                    if completed.turn.id == next.turn.id {
                        assert_eq!(completed.thread_id, thread.id);
                        assert_eq!(completed.turn.status, TurnStatus::Completed);
                        return Ok::<_, anyhow::Error>(());
                    }
                }
                _ => {}
            }
        }
    })
    .await??;

    let requests = model.requests();
    let [_, before_crash, recovered] = requests.as_slice() else {
        anyhow::bail!("expected two original model requests and one from the replacement");
    };
    assert_eq!(
        before_crash.function_call_output_text(RECORDED).as_deref(),
        Some(RESULT)
    );
    let outcomes = recovered.inputs_of_type("function_call_output");
    for (call, outcome) in [(RECORDED, RESULT), (PENDING, UNKNOWN)] {
        assert!(
            recovered.has_function_call(call),
            "missing original call {call}"
        );
        let recorded: Vec<_> = outcomes
            .iter()
            .filter(|item| item["call_id"] == call)
            .collect();
        let [output] = recorded.as_slice() else {
            anyhow::bail!(
                "expected exactly one output for {call}, found {}",
                recorded.len()
            );
        };
        assert_eq!(output["output"], outcome);
    }
    assert!(
        timeout(WAIT, replacement.shutdown_gracefully())
            .await??
            .success()
    );
    Ok(())
}

#[cfg(unix)]
mod durable_shell_logs {
    use super::*;
    use codex_app_server_protocol::AskForApproval;
    use codex_app_server_protocol::ItemCompletedNotification;
    use codex_app_server_protocol::ItemStartedNotification;
    use codex_app_server_protocol::SandboxPolicy;
    use codex_app_server_protocol::ThreadDeleteParams;
    use codex_app_server_protocol::ThreadDeleteResponse;
    use codex_app_server_protocol::ThreadItemPayload;
    use codex_app_server_protocol::ThreadStartResponse;
    use codex_app_server_protocol::TurnEnvironmentParams;
    use codex_utils_path_uri::PathUri;
    use sha2::Digest;
    use std::fs;
    use std::io::Seek;
    use std::io::SeekFrom;
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::PathBuf;
    use tokio::io::AsyncReadExt;

    const DIAGNOSTIC: &str = "error[E0308]: expected a u32";

    #[tokio::test]
    async fn terminal_poll_recovers_stderr_from_the_original_direct_and_exec_server_shells()
    -> Result<()> {
        recover_late_stderr(TerminalResult::Poll, None).await
    }

    #[tokio::test]
    async fn initial_terminal_result_recovers_stderr_from_direct_and_exec_server_shells()
    -> Result<()> {
        recover_late_stderr(TerminalResult::Launch, None).await
    }

    #[tokio::test]
    async fn hard_kill_after_publication_recovers_the_unattached_diagnostic_once() -> Result<()> {
        recover_late_stderr(TerminalResult::Poll, Some("exec-server")).await
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum TerminalResult {
        Launch,
        Poll,
    }

    struct LateBuild {
        backend: &'static str,
        cwd: PathBuf,
        bytes: Vec<u8>,
        command: String,
        invocation: String,
        number: i32,
        history: Vec<(String, String)>,
    }

    async fn block_attachment(
        sqlite: &codex_state::SqliteConfig,
        thread: codex_protocol::ThreadId,
        binding: &str,
    ) -> Result<sqlx::SqlitePool> {
        let pool = sqlite.open_read_write_pool(&sqlite.state_db_path()).await?;
        // The CLI uses its own connection, so its static trigger reads a
        // persistent selector in this fixture's isolated database.
        sqlx::query(
            "CREATE TABLE fixture_attachment_target (
                thread_id TEXT NOT NULL, invocation BLOB NOT NULL, PRIMARY KEY(thread_id, invocation))",
        )
        .execute(&pool)
        .await?;
        let target = sqlx::query(
            "INSERT INTO fixture_attachment_target (thread_id, invocation)
             SELECT thread_id, invocation FROM alab_shell_invocations WHERE thread_id = ? AND
             lower(hex(invocation)) = replace(?, '-', '') AND capture_name IS NULL",
        )
        .bind(thread.to_string())
        .bind(binding)
        .execute(&pool)
        .await?;
        assert_eq!(target.rows_affected(), 1);
        sqlx::query(
            "CREATE TRIGGER fixture_block_attachment BEFORE UPDATE OF capture_name ON alab_shell_invocations
             WHEN OLD.capture_name IS NULL AND NEW.capture_name IS NOT NULL AND EXISTS (
                SELECT 1 FROM fixture_attachment_target WHERE thread_id = OLD.thread_id AND invocation = OLD.invocation)
             BEGIN SELECT RAISE(ABORT, 'fixture_block_attachment'); END",
        )
        .execute(&pool)
        .await?;
        Ok(pool)
    }

    async fn recover_late_stderr(
        terminal: TerminalResult,
        interrupt_backend: Option<&str>,
    ) -> Result<()> {
        const EXIT: i32 = 71;
        let home = TempDir::new()?;
        let workspace = TempDir::new()?;
        let root = workspace.path().canonicalize()?;
        let server = responses::start_mock_server().await;
        MockResponsesConfig::new(&server.uri())
            .with_approval_policy("never")
            .with_sandbox_mode("workspace-write")
            .enable_feature(Feature::UnifiedExec)
            .enable_feature(Feature::ShellSnapshotV2)
            .with_extra_config(&format!(
                "[shell_environment_policy.set]\nHOME = {}",
                serde_json::to_string(home.path())?
            ))
            .write(home.path())?;
        let codex = codex_utils_cargo_bin::cargo_bin("codex")?;
        let mut builder = TestAppServer::builder()
            .with_program(&codex)
            .with_plugin_startup_tasks()
            .with_args(&["app-server"])
            .with_codex_home(home.path())
            .without_auto_env()
            .with_json_logging("warn,codex_otel.trace_safe=info");
        if interrupt_backend.is_some() {
            // SIGKILL skips TempDir destructors in the fixture CLI.
            let scratch = home.path().join("cli-tmp");
            fs::create_dir(&scratch)?;
            builder = builder.with_env_overrides(&[(
                "TMPDIR",
                Some(scratch.to_str().context("fixture TMPDIR")?),
            )]);
        }
        let mut original = builder.build_initialized_with_timeout(WAIT).await?;
        let start = original
            .send_thread_start_request(ThreadStartParams {
                cwd: Some(root.to_string_lossy().into_owned()),
                environments: Some(vec![TurnEnvironmentParams {
                    environment_id: codex_exec_server::LOCAL_ENVIRONMENT_ID.into(),
                    cwd: PathUri::from_host_native_path(&root)?.into(),
                    runtime_workspace_roots: None,
                }]),
                history_mode: Some(ThreadHistoryMode::Paginated),
                ..Default::default()
            })
            .await?;
        let started: ThreadStartResponse = timeout(WAIT, original.read_response(start)).await??;
        let owner = &started.thread.id;
        let _captures = FixtureCaptures {
            home: home.path(),
            thread: owner.clone(),
        };
        let sqlite =
            codex_state::SqliteConfig::new_for_testing(home.path().to_path_buf().try_into()?);
        let state = codex_state::StateRuntime::init(sqlite.clone(), "mock_provider".into()).await?;
        let thread = codex_protocol::ThreadId::from_string(owner)?;
        let tool = |id: &str, name: &str, args: serde_json::Value| {
            responses::sse(vec![
                responses::ev_response_created(id),
                responses::ev_function_call(id, name, &args.to_string()),
                responses::ev_completed(id),
            ])
        };
        let mut builds = Vec::new();
        let mut attachment_fault = None;
        let mut interrupted_capture = None;
        for (name, login) in [("direct", false), ("exec-server", true)] {
            let cwd = root.join(name);
            fs::create_dir(&cwd)?;
            let bytes = [name.as_bytes(), b": undefined symbol\n\0\xff"].concat();
            fs::write(cwd.join("late-output"), &bytes)?;
            assert!(
                std::process::Command::new("mkfifo")
                    .arg(cwd.join("parent-release"))
                    .arg(cwd.join("helper-release"))
                    .arg(cwd.join("helper-progress"))
                    .status()?
                    .success()
            );
            let open_fifo = |file: &str| {
                fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .custom_flags(libc::O_NONBLOCK)
                    .open(cwd.join(file))
            };
            let mut parent = open_fifo("parent-release")?;
            let mut helper = open_fifo("helper-release")?;
            let mut progress =
                tokio::net::unix::pipe::Receiver::from_file(open_fifo("helper-progress")?)?;
            // At the selected local cwd, a POSIX login shell takes the exec-server
            // snapshot route. A nonlogin invocation of the same shell uses direct pipes.
            // For a terminal launch the helper lets its parent exit immediately
            // after acquiring stderr. Only the fixture can release the diagnostic.
            let ready = match terminal {
                TerminalResult::Launch => format!("printf '{EXIT}\\n' > parent-release; "),
                TerminalResult::Poll => String::new(),
            };
            let command = format!(
                concat!(
                    "cd {} || exit 1; printf 'started\\n' >> observed-builds; ",
                    "(trap '' HUP; exec 3<helper-release; printf R > helper-progress; ",
                    "{}",
                    "IFS= read -r permit <&3 && cat late-output >&2 || exit 1; ",
                    "exec 2>&-; printf D > helper-progress) </dev/null >/dev/null & ",
                    r#"IFS= read -r code < parent-release && exit "$code""#,
                ),
                name, ready
            );
            let yield_ms = if terminal == TerminalResult::Launch {
                30000
            } else {
                1000
            };
            let launch = responses::mount_sse_sequence(
                &server,
                vec![
                    tool(
                        name,
                        "exec_command",
                        json!({
                            "cmd": command, "workdir": root, "shell": "/bin/sh", "login": login,
                            "tty": false, "yield_time_ms": yield_ms, "max_output_tokens": 64
                        }),
                    ),
                    responses::sse_completed("started"),
                ],
            )
            .await;
            complete(&mut original, owner, "Start the build.").await?;
            let launch = shell_result(&launch, name)?;
            let number = match terminal {
                TerminalResult::Poll => terminal_number(&launch)?,
                TerminalResult::Launch => {
                    // The terminal tool result exposes no running session. Record the
                    // native command item's process ID independently of the later receipt.
                    timeout(WAIT, async {
                        loop {
                            let started: ItemStartedNotification =
                                original.read_notification("item/started").await?;
                            if started.thread_id == *owner
                                && let ThreadItemPayload::CommandExecution {
                                    id, process_id, ..
                                } = started.item.payload
                                && id == name
                            {
                                let number =
                                    process_id.context("missing original native process ID")?;
                                return Ok::<i32, anyhow::Error>(number.parse()?);
                            }
                        }
                    })
                    .await
                    .context("missing original command item")??
                }
            };
            if login {
                original
                    .wait_for_matching_json_log_event(
                        "local exec-server start for the observed invocation",
                        |event| {
                            event["fields"]["event.name"]
                                == "codex.unified_exec.process_start_requested"
                                && event["fields"]["unified_exec_process_id"] == number
                        },
                        WAIT,
                    )
                    .await?;
            }
            assert_eq!(
                timeout(WAIT, progress.read_u8())
                    .await
                    .context("helper did not acquire stderr")??,
                b'R'
            );
            let poll_id = format!("{name}-poll");
            let stale_id = format!("{name}-retired-write");
            let mut attempts = Vec::new();
            let mut write_args = json!({"session_id":number, "chars":"unexpected input\n"});
            let launch_binding = if terminal == TerminalResult::Poll {
                let binding = original_invocation(&launch)?;
                writeln!(parent, "{EXIT}")?;
                attempts.push(tool(&poll_id, "write_stdin", json!({
                    "session_id":number, "invocation_id":binding, "chars":"", "yield_time_ms":10000
                })));
                write_args["invocation_id"] = json!(binding);
                Some(binding)
            } else {
                None
            };
            attempts.push(tool(&stale_id, "write_stdin", write_args));
            attempts.push(responses::sse_completed("retired"));
            let retired = responses::mount_sse_sequence(&server, attempts).await;
            complete(&mut original, owner, "Inspect the retired shell.").await?;
            let final_output = match terminal {
                TerminalResult::Poll => shell_result(&retired, &poll_id)?,
                TerminalResult::Launch => launch.clone(),
            };
            assert!(
                final_output.contains(&format!("Process exited with code {EXIT}")),
                "{final_output}"
            );
            assert_eq!(
                final_output
                    .split_once("\nOutput:")
                    .map(|(_, body)| body.trim()),
                Some("")
            );
            assert!(
                !final_output.contains("Process running with session ID"),
                "{final_output}"
            );
            let stale = shell_result(&retired, &stale_id)?;
            assert!(
                stale.contains(&number.to_string()) && stale.contains("No input was sent."),
                "{stale}"
            );
            if let Some(binding) = launch_binding {
                assert!(stale.contains(binding), "{stale}");
            }
            let original_build = |receipt: &codex_state::PendingShellExit| {
                receipt.session_id == number
                    && receipt
                        .launch
                        .as_ref()
                        .is_some_and(|launch| launch.command == command)
            };
            assert!(
                !state
                    .pending_shell_exits(thread)
                    .await?
                    .iter()
                    .any(original_build),
                "the model already received the exit and the helper still holds its diagnostic"
            );
            if interrupt_backend == Some(name) {
                attachment_fault = Some(
                    block_attachment(
                        &sqlite,
                        thread,
                        launch_binding.context("fault needs the original invocation")?,
                    )
                    .await?,
                );
            }
            // The model has already seen the terminal result. The diagnostic comes from
            // its original helper, whose separate progress pipe signals closed stderr.
            writeln!(helper, "emit")?;
            assert_eq!(
                timeout(WAIT, progress.read_u8())
                    .await
                    .context("helper could not emit its late diagnostic")??,
                b'D'
            );
            fs::remove_file(cwd.join("late-output"))?;
            // A closed writer precedes native EOF and its database commit. A read
            // identifies the original invocation without acknowledging the receipt.
            let binding = if interrupt_backend == Some(name) {
                let binding = launch_binding.context("missing original invocation")?;
                original
                    .wait_for_matching_json_log_event(
                        "the CLI attempted the final diagnostic attachment",
                        |event| {
                            event["fields"]
                                .to_string()
                                .contains("fixture_block_attachment")
                        },
                        WAIT,
                    )
                    .await?;
                let publications = state.pending_shell_diagnostic_publications(thread).await?;
                let [intent] = publications.as_slice() else {
                    anyhow::bail!(
                        "expected just the interrupted publication, found {}",
                        publications.len()
                    );
                };
                assert_eq!(intent.invocation.to_string(), binding);
                assert_eq!(intent.thread_id, thread);
                assert_eq!(intent.session_id, number);
                assert_eq!(intent.exit_code, EXIT);
                assert!(!intent.partial);
                assert!(intent.tail.is_none());
                assert_eq!(usize::try_from(intent.expected_bytes)?, bytes.len());
                assert_eq!(
                    intent.prefix_sha256,
                    format!("{:x}", sha2::Sha256::digest(&bytes))
                );
                let directory = codex_alab_core::shell_log_store::LogDirectory::open(
                    home.path(),
                    owner,
                    false,
                )?;
                assert!(directory.retains_capture(intent.capture_name.as_ref())?);
                let path = directory.path().join(&intent.capture_name);
                assert_eq!(fs::read(&path)?, bytes);
                let row = state
                    .shell_invocation(thread, intent.invocation)
                    .await?
                    .context("missing original terminal observation")?;
                assert!(row.finished && row.exit_code == Some(EXIT) && row.capture_name.is_none());
                assert!(
                    !state
                        .pending_shell_exits(thread)
                        .await?
                        .iter()
                        .any(original_build)
                );
                interrupted_capture = Some((binding.to_owned(), path));
                binding.to_owned()
            } else {
                timeout(WAIT, async {
                    loop {
                        if let Some(receipt) = state
                            .pending_shell_exits(thread)
                            .await?
                            .into_iter()
                            .find(|row| original_build(row) && row.capture_partial == Some(false))
                        {
                            assert_eq!(receipt.exit_code, Some(EXIT));
                            assert!(receipt.capture_name.is_some());
                            return Ok::<_, anyhow::Error>(receipt.invocation.to_string());
                        }
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                })
                .await
                .with_context(|| format!("{name} did not commit its complete diagnostic"))??
            };
            if let Some(expected) = launch_binding {
                assert_eq!(binding, expected);
            }
            let mut history = vec![(name.to_owned(), launch), (stale_id, stale)];
            if terminal == TerminalResult::Poll {
                history.push((poll_id, final_output));
            }
            builds.push(LateBuild {
                backend: name,
                cwd,
                bytes,
                command,
                invocation: binding,
                number,
                history,
            });
        }
        if interrupt_backend.is_some() {
            let pool =
                attachment_fault.context("the requested attachment fault was never installed")?;
            timeout(WAIT, original.kill_and_reap()).await??;
            sqlx::query("DROP TRIGGER fixture_block_attachment")
                .execute(&pool)
                .await?;
            sqlx::query("DROP TABLE fixture_attachment_target")
                .execute(&pool)
                .await?;
        } else {
            assert!(
                timeout(WAIT, original.shutdown_gracefully())
                    .await??
                    .success()
            );
        }
        drop(original);

        let mut inspection = resume_candidate(home.path(), owner).await?;
        let first = responses::mount_sse_once(&server, responses::sse_completed("inspected")).await;
        complete(&mut inspection, owner, "Inspect the finished builds.").await?;
        let request = first.single_request();
        let summaries = shell_exit_summaries(&request)?;
        let [entries] = summaries.as_slice() else {
            anyhow::bail!("expected one resumed shell summary, received {summaries:?}")
        };
        assert_eq!(entries.len(), builds.len());
        let mut reads = Vec::new();
        let outputs = request.inputs_of_type("function_call_output");
        for build in &builds {
            let name = build.backend;
            for (id, result) in &build.history {
                assert!(request.has_function_call(id));
                assert_eq!(
                    outputs
                        .iter()
                        .filter(|item| item["call_id"] == id.as_str())
                        .count(),
                    1
                );
                assert_eq!(
                    request.function_call_output_text(id).as_deref(),
                    Some(result.as_str())
                );
            }
            let entry = entries
                .iter()
                .find(|entry| entry["invocation_id"].as_str() == Some(build.invocation.as_str()))
                .with_context(|| format!("missing original diagnostic for {name}"))?;
            assert_eq!(entry["session_id"], build.number);
            assert_eq!(entry["exit_code"], EXIT);
            assert_eq!(entry["command"], json!(build.command));
            assert_eq!(entry["cwd"], json!(root));
            assert!(entry.get("retained_output_partial").is_none());
            let path = entry["retained_output_path"]
                .as_str()
                .context("missing original diagnostic log")?;
            if let Some((binding, original_path)) = &interrupted_capture
                && binding == &build.invocation
            {
                assert_eq!(Path::new(path), original_path);
                assert!(
                    state
                        .pending_shell_diagnostic_publications(thread)
                        .await?
                        .is_empty()
                );
            }
            reads.push(tool(&format!("{name}-cold-write"), "write_stdin", json!({
                "session_id":build.number, "invocation_id":build.invocation, "chars":"unexpected cold input\n"
            })));
            reads.push(app_test_support::create_command_execution_sse_response(
                vec!["cp".into(), path.into(), "recovered-diagnostic".into()],
                Some(&build.cwd),
                None,
                &format!("{name}-recover"),
            )?);
        }
        drop(state);
        assert!(
            timeout(WAIT, inspection.shutdown_gracefully())
                .await??
                .success()
        );
        drop(inspection);
        let mut replacement = resume_candidate(home.path(), owner).await?;
        reads.push(responses::sse_completed("recovered"));
        let recovered = responses::mount_sse_sequence(&server, reads).await;
        complete(
            &mut replacement,
            owner,
            "Read the retained diagnostic files.",
        )
        .await?;
        for request in recovered.requests() {
            assert!(shell_exit_summaries(&request)?.is_empty());
        }
        for build in builds {
            let stale = shell_result(&recovered, &format!("{}-cold-write", build.backend))?;
            assert!(
                stale.contains(&build.invocation) && stale.contains("No input was sent."),
                "{stale}"
            );
            assert_eq!(
                fs::read(build.cwd.join("recovered-diagnostic"))?,
                build.bytes
            );
            assert_eq!(fs::read(build.cwd.join("observed-builds"))?, b"started\n");
        }
        delete(&mut replacement, owner.clone()).await?;
        assert!(
            timeout(WAIT, replacement.shutdown_gracefully())
                .await??
                .success()
        );
        Ok(())
    }

    #[tokio::test]
    async fn resumed_input_observes_the_original_invocation_and_only_reaches_its_bound_shell()
    -> Result<()> {
        const WATCHED: &str = "watched-build";
        const WATCHED_EXIT: i32 = 23;
        const QUIET: &str = "quiet-build";
        const QUIET_EXIT: i32 = 41;
        const LATE: &str = "late-build";
        const LATE_EXIT: i32 = 53;
        const CAPPED: &str = "capped-build";
        const CAPPED_EXIT: i32 = 65;
        const LATE_BYTES: &[u8] = b"error[E0382]: use of moved value\n\0\xff";
        const LOG_CAP: u64 = 8 * 1024 * 1024;
        const OMITTED_BYTES: u64 = 128 * 1024;
        const TAIL_BYTES: usize = 4096;
        const PREFIX_END: &[u8] = b"last bytes in the original compiler log\n";
        const FINAL_DIAGNOSTIC: &[u8] = b"linker: undefined symbol in final object\n\0\xff";
        const ONE_SHOT_DIAGNOSTIC: &str = "trustc: cannot prove length ≤ capacity\n";
        const BUILD: &str = concat!(
            "printf 'started\\n' >> observed-builds; ",
            "if test -f compiler-output; then cat compiler-output; fi; ",
            "IFS= read -r result < build-release || exit 1; ",
            "if test -f late-output; then cat late-output >&2; fi; ",
            r#"exit "$result""#
        );
        let home = TempDir::new()?;
        let workspace = TempDir::new()?;
        let watched_cwd = workspace.path().canonicalize()?;
        let quiet_cwd = watched_cwd.join("other checkout");
        let late_cwd = watched_cwd.join("late checkout");
        let capped_cwd = watched_cwd.join("capped checkout");
        fs::create_dir(&quiet_cwd)?;
        fs::create_dir(&late_cwd)?;
        fs::create_dir(&capped_cwd)?;
        fs::write(
            watched_cwd.join("compiler-output"),
            format!("{:02048}\n{DIAGNOSTIC}\n{:02048}\n", 0, 0),
        )?;
        fs::write(late_cwd.join("late-output"), LATE_BYTES)?;
        let prefix = |file: &mut fs::File| -> std::io::Result<()> {
            file.write_all(LATE_BYTES)?;
            file.seek(SeekFrom::Start(LOG_CAP - PREFIX_END.len() as u64))?;
            file.write_all(PREFIX_END)
        };
        prefix(&mut fs::File::create(capped_cwd.join("expected-prefix"))?)?;
        let mut capped_source = fs::File::create(capped_cwd.join("late-output"))?;
        prefix(&mut capped_source)?;
        capped_source.write_all(b"omitted compiler output begins here\n")?;
        let start_byte = LOG_CAP + OMITTED_BYTES;
        let end_byte = start_byte + TAIL_BYTES as u64;
        let mut tail_bytes = vec![b'.'; TAIL_BYTES];
        tail_bytes[TAIL_BYTES - FINAL_DIAGNOSTIC.len()..].copy_from_slice(FINAL_DIAGNOSTIC);
        capped_source.seek(SeekFrom::Start(start_byte))?;
        capped_source.write_all(&tail_bytes)?;
        drop(capped_source);
        let expected_tail = json!({
            "start_byte": start_byte, "end_byte": end_byte, "encoding": "base64",
            "data": codex_exec_server::ByteChunk::from(tail_bytes.clone()),
        });
        let one_shot_tail =
            ".".repeat(TAIL_BYTES - ONE_SHOT_DIAGNOSTIC.len()) + ONE_SHOT_DIAGNOSTIC;
        let mut one_shot_source = fs::File::create(capped_cwd.join("one-shot-output"))?;
        one_shot_source.seek(SeekFrom::Start(start_byte))?;
        one_shot_source.write_all(one_shot_tail.as_bytes())?;
        drop(one_shot_source);
        assert!(
            std::process::Command::new("mkfifo")
                .arg(watched_cwd.join("build-release"))
                .arg(quiet_cwd.join("build-release"))
                .arg(late_cwd.join("build-release"))
                .arg(capped_cwd.join("build-release"))
                .status()?
                .success()
        );
        let server = responses::start_mock_server().await;
        MockResponsesConfig::new(&server.uri())
            .with_approval_policy("never")
            .with_sandbox_mode("workspace-write")
            .enable_feature(Feature::UnifiedExec)
            .disable_feature(Feature::ShellSnapshot)
            .write(home.path())?;
        let _logs;
        let mut original = start_candidate(home.path()).await?;
        let thread = original
            .start_thread(ThreadStartParams {
                cwd: Some(workspace.path().to_string_lossy().into_owned()),
                history_mode: Some(ThreadHistoryMode::Paginated),
                ..Default::default()
            })
            .await?
            .thread;
        let owner = &thread.id;
        _logs = FixtureCaptures {
            home: home.path(),
            thread: owner.clone(),
        };
        let call = |name: &str, tool: &str, args: serde_json::Value| {
            responses::sse(vec![
                responses::ev_response_created(name),
                responses::ev_function_call(name, tool, &args.to_string()),
                responses::ev_completed(name),
            ])
        };
        let wait_for_input = |name: &str, script: &str, cwd: &Path, tty: bool| {
            call(
                name,
                "exec_command",
                json!({
                    "cmd": script, "workdir": cwd, "login": false, "tty": tty,
                    "yield_time_ms": 1000, "max_output_tokens": 64
                }),
            )
        };
        let setup = responses::mount_sse_sequence(
            &server,
            vec![
                wait_for_input(
                    "original-build",
                    &format!("stty -echo; printf '%02048d\\n%s\\n%02048d\\n' 0 '{DIAGNOSTIC}' 0; read -r line"),
                    workspace.path(),
                    true,
                ),
                wait_for_input(
                    "completed-build",
                    "stty -echo; printf waiting; read -r line; printf 'collected diagnostic\\n'; exit 7",
                    workspace.path(),
                    true,
                ),
                wait_for_input(WATCHED, BUILD, &watched_cwd, true),
                wait_for_input(QUIET, BUILD, &quiet_cwd, true),
                wait_for_input(LATE, BUILD, &late_cwd, false),
                wait_for_input(CAPPED, BUILD, &capped_cwd, false),
                responses::sse_completed("builds-started"),
            ],
        )
        .await;
        complete(&mut original, owner, "Start the waiting builds.").await?;
        let old = shell_result(&setup, "original-build")?;
        let old_binding = original_invocation(&old)?;
        let log = old
            .lines()
            .find_map(|l| l.strip_prefix("Shell output is being saved to: "))
            .context("original command did not disclose its captured prefix")?;
        assert!(!old.contains(DIAGNOSTIC), "{old}");
        assert!(fs::read_to_string(log)?.contains(DIAGNOSTIC));

        let terminal = shell_result(&setup, "completed-build")?;
        let completed_binding = original_invocation(&terminal)?;
        let watched = shell_result(&setup, WATCHED)?;
        let watched_binding = original_invocation(&watched)?;
        let quiet = shell_result(&setup, QUIET)?;
        let quiet_binding = original_invocation(&quiet)?;
        assert_ne!(quiet_binding, watched_binding);
        let late = shell_result(&setup, LATE)?;
        let capped = shell_result(&setup, CAPPED)?;
        let late_binding = original_invocation(&late)?;
        let capped_binding = original_invocation(&capped)?;
        for launch in [&quiet, &late, &capped] {
            assert!(
                !launch.contains("Shell output is being saved to:"),
                "{launch}"
            );
            assert_eq!(
                launch.split_once("\nOutput:").map(|(_, body)| body.trim()),
                Some("")
            );
        }
        let watched_log = watched
            .lines()
            .find_map(|line| line.strip_prefix("Shell output is being saved to: "))
            .context("watched build did not disclose its log")?;
        assert!(!watched.contains(DIAGNOSTIC), "{watched}");
        // Hold each FIFO open until its watcher confirms the exit, even if the shell
        // has not entered read(2) when the fixture releases it.
        for (call_id, output, cwd, expected_exit) in [
            (WATCHED, &watched, &watched_cwd, WATCHED_EXIT),
            (QUIET, &quiet, &quiet_cwd, QUIET_EXIT),
            (LATE, &late, &late_cwd, LATE_EXIT),
            (CAPPED, &capped, &capped_cwd, CAPPED_EXIT),
        ] {
            let number = terminal_number(output)?.to_string();
            let mut release = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(cwd.join("build-release"))?;
            writeln!(release, "{expected_exit}")?;
            timeout(WAIT, async {
                loop {
                    let done: ItemCompletedNotification =
                        original.read_notification("item/completed").await?;
                    if done.thread_id == *owner
                        && let ThreadItemPayload::CommandExecution {
                            id,
                            process_id,
                            exit_code,
                            ..
                        } = done.item.payload
                        && id == call_id
                    {
                        assert_eq!(process_id.as_deref(), Some(number.as_str()));
                        assert_eq!(exit_code, Some(expected_exit));
                        return Ok::<_, anyhow::Error>(());
                    }
                }
            })
            .await
            .with_context(|| format!("watcher did not report {call_id}'s exit between turns"))??;
        }
        let control = responses::mount_sse_sequence(&server, vec![
            call("finish-build", "write_stdin", json!({
                "session_id": terminal_number(&terminal)?, "chars": "finish\n", "yield_time_ms": 10000
            })),
            call("one-shot-build", "exec_command", json!({
                "cmd": "printf 'started\\n' >> one-shot-runs; cat one-shot-output >&2 && exit 73",
                "workdir": capped_cwd, "login": false, "tty": false,
                "yield_time_ms": 30000, "max_output_tokens": 64
            })),
            responses::sse_completed("exit-observed"),
        ]).await;
        complete(
            &mut original,
            owner,
            "Complete the second build and run the one-shot check.",
        )
        .await?;
        let finish = shell_result(&control, "finish-build")?;
        assert!(finish.contains("Process exited with code 7"), "{finish}");
        assert!(finish.contains("collected diagnostic"), "{finish}");
        let one_shot = shell_result(&control, "one-shot-build")?;
        assert!(
            one_shot.contains("Process exited with code 73"),
            "{one_shot}"
        );
        assert!(
            !one_shot.contains("Process running with session ID"),
            "{one_shot}"
        );
        assert_eq!(
            tool_shell_tail(&one_shot)?,
            json!({
                "start_byte": start_byte, "end_byte": end_byte, "encoding": "utf8",
                "data": one_shot_tail,
            })
        );
        // The command-end notification can precede the raw reader's SQLite commit.
        // Observe the original invocation before shutdown without consuming its receipt.
        let sqlite =
            codex_state::SqliteConfig::new_for_testing(home.path().to_path_buf().try_into()?);
        let state = codex_state::StateRuntime::init(sqlite, "mock_provider".into()).await?;
        let thread_id = codex_protocol::ThreadId::from_string(owner)?;
        let capped_invocation = capped_binding.parse()?;
        let stored_tail = timeout(WAIT, async {
            loop {
                if let Some(tail) = state
                    .shell_invocation(thread_id, capped_invocation)
                    .await?
                    .and_then(|row| row.tail)
                {
                    return Ok::<_, anyhow::Error>(tail);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .context("capped build did not persist its output tail")??;
        assert_eq!(stored_tail.start_byte(), start_byte);
        assert_eq!(stored_tail.end_byte(), end_byte);
        assert_eq!(stored_tail.bytes(), tail_bytes.as_slice());
        drop(state);
        assert!(
            timeout(WAIT, original.shutdown_gracefully())
                .await??
                .success()
        );
        drop(original);
        fs::remove_file(late_cwd.join("late-output"))?;
        fs::remove_file(capped_cwd.join("late-output"))?;
        fs::remove_file(capped_cwd.join("one-shot-output"))?;

        let mut inspection = resume_candidate(home.path(), owner).await?;
        let rejected = responses::mount_sse_once(
            &server,
            responses::sse_failed(
                "refused-inspection",
                "invalid_prompt",
                "Rejected by fixture",
            ),
        )
        .await;
        let failed = timeout(
            WAIT,
            inspection.start_turn_and_wait_for_completion(text_turn(
                owner,
                "Inspect the finished build.",
            )),
        )
        .await??;
        assert_eq!(failed.turn.status, TurnStatus::Failed);
        let first_request = rejected.single_request();
        let first_summaries = shell_exit_summaries(&first_request)?;
        let retained_path = |binding: &str| {
            first_summaries
                .iter()
                .flatten()
                .find(|entry| entry["invocation_id"].as_str() == Some(binding))
                .and_then(|entry| entry["retained_output_path"].as_str())
                .with_context(|| format!("missing late output path for {binding}"))
        };
        let late_log = retained_path(late_binding)?;
        let capped_log = retained_path(capped_binding)?;
        assert_ne!(late_log, capped_log);
        let capture_directory = Path::new(capped_log)
            .parent()
            .context("capture directory")?;
        let parked_capture = tempfile::NamedTempFile::new_in(capture_directory)?.into_temp_path();
        fs::rename(capped_log, &parked_capture)?;
        let continued = responses::mount_sse_sequence(
            &server,
            vec![
                responses::sse(vec![
                    responses::ev_response_created("inspection-interrupted"),
                    json!({
                        "type": "response.incomplete",
                        "response": {
                            "id": "inspection-interrupted",
                            "status": "incomplete",
                            "incomplete_details": { "reason": "interrupted" }
                        }
                    }),
                ]),
                responses::sse_completed("inspection-accepted"),
            ],
        )
        .await;
        complete(&mut inspection, owner, "Inspect the finished build again.").await?;

        let continued_requests = continued.requests();
        assert_eq!(
            continued_requests.len(),
            2,
            "interruption must continue in the same turn"
        );
        for (request, capture_available) in std::iter::once((first_request, true))
            .chain(continued_requests.into_iter().map(|req| (req, false)))
        {
            let summaries = shell_exit_summaries(&request)?;
            let [entries] = summaries.as_slice() else {
                anyhow::bail!("expected one native shell summary, received {summaries:?}");
            };
            assert_eq!(
                entries.len(),
                4,
                "expected all uncollected exits: {entries:?}"
            );
            for (binding, output, cwd, code, log) in [
                (
                    watched_binding,
                    &watched,
                    &watched_cwd,
                    WATCHED_EXIT,
                    Some(watched_log),
                ),
                (quiet_binding, &quiet, &quiet_cwd, QUIET_EXIT, None),
                (late_binding, &late, &late_cwd, LATE_EXIT, Some(late_log)),
                (
                    capped_binding,
                    &capped,
                    &capped_cwd,
                    CAPPED_EXIT,
                    capture_available.then_some(capped_log),
                ),
            ] {
                let entry = entries
                    .iter()
                    .find(|entry| entry["invocation_id"].as_str() == Some(binding))
                    .with_context(|| format!("missing build {binding} in {entries:?}"))?;
                assert_eq!(entry["session_id"], json!(terminal_number(output)?));
                assert_eq!(entry["exit_code"], json!(code));
                assert_eq!(entry["command"], BUILD);
                assert_eq!(entry["cwd"], json!(cwd));
                assert_eq!(
                    entry.get("retained_output_path"),
                    log.map(serde_json::Value::from).as_ref()
                );
                assert_eq!(
                    entry.get("retained_output_partial"),
                    (binding == capped_binding && capture_available)
                        .then_some(json!(true))
                        .as_ref()
                );
                assert_eq!(
                    entry.get("retained_output_unavailable"),
                    (binding == capped_binding && !capture_available)
                        .then_some(json!(true))
                        .as_ref()
                );
                assert_eq!(
                    entry.get("shell_output_tail"),
                    (binding == capped_binding).then_some(&expected_tail)
                );
            }
            for (call_id, original) in [
                (WATCHED, &watched),
                (QUIET, &quiet),
                (LATE, &late),
                (CAPPED, &capped),
                ("original-build", &old),
                ("completed-build", &terminal),
                ("finish-build", &finish),
                ("one-shot-build", &one_shot),
            ] {
                assert_eq!(
                    request.function_call_output_text(call_id).as_ref(),
                    Some(original)
                );
            }
        }
        assert!(
            timeout(WAIT, inspection.shutdown_gracefully())
                .await??
                .success()
        );
        drop(inspection);

        fs::rename(&parked_capture, capped_log)?;
        // A completed model response acknowledged the exits before any shell tool read them.
        let mut replacement = resume_candidate(home.path(), owner).await?;
        let fresh = responses::mount_sse_sequence(
            &server,
            vec![
                wait_for_input(
                    "replacement-build",
                    r#"stty -echo; printf 'ready\n'; IFS= read -r line && printf '%s\n' "$line" > accepted-input"#,
                    workspace.path(),
                    true,
                ),
                app_test_support::create_command_execution_sse_response(
                    vec!["cp".into(), late_log.into(), "recovered-diagnostic".into()],
                    Some(&late_cwd),
                    None,
                    "read-late-diagnostic",
                )?,
                app_test_support::create_command_execution_sse_response(
                    vec!["cmp".into(), capped_log.into(), "expected-prefix".into()],
                    Some(&capped_cwd),
                    None,
                    "read-capped-diagnostic",
                )?,
                responses::sse_completed("replacement-ready"),
            ],
        )
        .await;
        complete(
            &mut replacement,
            owner,
            "Start a replacement build and recover the late diagnostics.",
        )
        .await?;
        assert_eq!(fs::read(late_cwd.join("recovered-diagnostic"))?, LATE_BYTES);
        let compared = shell_result(&fresh, "read-capped-diagnostic")?;
        assert!(
            compared.contains("Process exited with code 0"),
            "{compared}"
        );
        fs::remove_file(capped_log)?;
        for cwd in [&late_cwd, &capped_cwd] {
            assert_eq!(fs::read(cwd.join("observed-builds"))?, b"started\n");
        }
        for request in fresh.requests() {
            assert!(shell_exit_summaries(&request)?.is_empty());
            assert_eq!(
                request.function_call_output_text(WATCHED).as_ref(),
                Some(&watched)
            );
        }
        let live = shell_result(&fresh, "replacement-build")?;
        let live_id = terminal_number(&live)?;
        let live_binding = original_invocation(&live)?;
        assert_ne!(live_binding, old_binding);
        assert_ne!(live_binding, completed_binding);

        // A numeric collision presents this same pair to the replacement runtime.
        let bound_write = |name: &str, binding: &str, chars: &str| {
            call(
                name,
                "write_stdin",
                json!({
                    "session_id": live_id, "invocation_id": binding, "chars": chars, "yield_time_ms": 10000
                }),
            )
        };
        let results = responses::mount_sse_sequence(
            &server,
            vec![
                bound_write("stale-write", old_binding, "old input\n"),
                bound_write("completed-poll", completed_binding, ""),
                bound_write("watched-poll", watched_binding, ""),
                bound_write("capped-poll", capped_binding, ""),
                call(
                    "numeric-write",
                    "write_stdin",
                    json!({
                        "session_id": live_id, "chars": "unbound input\n", "yield_time_ms": 10000
                    }),
                ),
                bound_write("bound-write", live_binding, "bound input\n"),
                responses::sse_completed("writes-observed"),
            ],
        )
        .await;
        complete(
            &mut replacement,
            owner,
            "Recover the prior observations and operate the new shell.",
        )
        .await?;
        let stale = shell_result(&results, "stale-write")?;
        assert!(stale.contains(log), "{stale}");
        assert!(
            stale.contains("Last observed status: in_progress"),
            "{stale}"
        );
        assert!(!stale.contains("Last observed exit code:"), "{stale}");
        assert!(fs::read_to_string(log)?.contains(DIAGNOSTIC));
        let exited = shell_result(&results, "completed-poll")?;
        assert!(exited.contains("Last observed exit code: 7"), "{exited}");
        let capped_poll = shell_result(&results, "capped-poll")?;
        assert!(
            capped_poll
                .lines()
                .any(|line| line == "Retained shell output unavailable."),
            "{capped_poll}"
        );
        assert!(!capped_poll.contains(capped_log), "{capped_poll}");
        assert!(
            capped_poll.contains(&format!("Last observed exit code: {CAPPED_EXIT}")),
            "{capped_poll}"
        );
        assert_eq!(tool_shell_tail(&capped_poll)?, expected_tail);
        assert_eq!(fs::read(capped_cwd.join("one-shot-runs"))?, b"started\n");
        let recovered = shell_result(&results, "watched-poll")?;
        let launch: serde_json::Value = serde_json::from_str(
            recovered
                .lines()
                .find_map(|line| line.strip_prefix("Original shell launch (JSON): "))
                .context("missing original launch on stale lookup")?,
        )?;
        assert_eq!(launch["command"], BUILD);
        assert_eq!(launch["cwd"], json!(watched_cwd));
        assert!(
            recovered
                .lines()
                .any(|line| line == format!("Last observed exit code: {WATCHED_EXIT}")),
            "{recovered}"
        );
        assert!(
            recovered
                .lines()
                .any(|line| line == format!("Retained shell output: {watched_log}")),
            "{recovered}"
        );
        assert!(fs::read_to_string(watched_log)?.contains(DIAGNOSTIC));
        assert_eq!(
            fs::read_to_string(workspace.path().join("observed-builds"))?,
            "started\n"
        );
        let unbound = shell_result(&results, "numeric-write")?;
        assert!(unbound.contains("invocation_id"), "{unbound}");
        let accepted = shell_result(&results, "bound-write")?;
        assert!(
            accepted.contains("Process exited with code 0"),
            "{accepted}"
        );
        assert_eq!(
            fs::read_to_string(workspace.path().join("accepted-input"))?,
            "bound input\n"
        );
        delete(&mut replacement, owner.clone()).await?;
        assert!(!Path::new(log).try_exists()?);
        assert!(!Path::new(watched_log).try_exists()?);
        assert!(!Path::new(late_log).try_exists()?);
        assert!(!Path::new(capped_log).try_exists()?);
        assert!(
            timeout(WAIT, replacement.shutdown_gracefully())
                .await??
                .success()
        );
        Ok(())
    }

    fn shell_result(mock: &responses::ResponseMock, call: &str) -> Result<String> {
        mock.function_call_output_text(call)
            .with_context(|| format!("missing shell result for {call}"))
    }

    fn tool_shell_tail(result: &str) -> Result<serde_json::Value> {
        let text = result
            .lines()
            .find_map(|line| line.strip_prefix("Shell output tail: "))
            .context("tool result omitted its shell output tail")?;
        Ok(serde_json::from_str(text)?)
    }

    fn shell_exit_summaries(
        request: &responses::ResponsesRequest,
    ) -> Result<Vec<Vec<serde_json::Value>>> {
        let mut summaries = Vec::new();
        for text in request.message_input_texts("user") {
            if let Some(body) = text.strip_prefix("<observed_shell_exits>\n") {
                let body = body
                    .strip_suffix("\n</observed_shell_exits>")
                    .context("incomplete native shell summary")?;
                summaries.push(serde_json::from_str(body)?);
            }
        }
        Ok(summaries)
    }

    async fn resume_candidate(home: &Path, thread: &str) -> Result<TestAppServer> {
        let mut app = start_candidate(home).await?;
        let request = app
            .send_thread_resume_request(ThreadResumeParams {
                thread_id: thread.to_owned(),
                exclude_turns: true,
                ..Default::default()
            })
            .await?;
        let resumed: ThreadResumeResponse = timeout(WAIT, app.read_response(request)).await??;
        assert_eq!(resumed.thread.id, thread);
        Ok(app)
    }

    fn original_invocation(result: &str) -> Result<&str> {
        result
            .lines()
            .find_map(|line| line.strip_prefix("Invocation ID: "))
            .filter(|id| !id.is_empty())
            .with_context(|| format!("missing original invocation ID in {result}"))
    }

    fn terminal_number(result: &str) -> Result<i32> {
        Ok(result
            .lines()
            .find_map(|line| line.strip_prefix("Process running with session ID "))
            .with_context(|| format!("missing running terminal in {result}"))?
            .parse()?)
    }

    struct FixtureCaptures<'a> {
        home: &'a Path,
        thread: String,
    }

    impl Drop for FixtureCaptures<'_> {
        fn drop(&mut self) {
            use codex_alab_core::shell_log_store::LogDirectory;
            if let Err(error) =
                LogDirectory::open(self.home, &self.thread, false).and_then(LogDirectory::retire)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                eprintln!(
                    "failed to retire fixture captures for {}: {error}",
                    self.thread
                );
            }
        }
    }

    fn build_response(id: &str) -> String {
        let args = json!({
            "cmd": format!("printf '%02048d\\n%s\\n%02048d\\n' 0 '{DIAGNOSTIC}' 0; exit 7"),
            "login": false,
            "max_output_tokens": 160
        });
        responses::sse(vec![
            responses::ev_response_created(id),
            responses::ev_function_call(id, "exec_command", &args.to_string()),
            responses::ev_completed(id),
        ])
    }

    fn captured_path(mock: &responses::ResponseMock, call: &str) -> Result<PathBuf> {
        let result = shell_result(mock, call)?;
        assert!(result.contains("Process exited with code 7"), "{result}");
        assert!(!result.contains(DIAGNOSTIC), "{result}");
        result
            .lines()
            .find_map(|line| line.strip_prefix("Full shell output saved to: "))
            .map(PathBuf::from)
            .with_context(|| format!("missing complete log for {call}: {result}"))
    }

    async fn complete(app: &mut TestAppServer, thread: &str, text: &str) -> Result<()> {
        let turn = timeout(
            WAIT,
            app.start_turn_and_wait_for_completion(text_turn(thread, text)),
        )
        .await??;
        assert_eq!(turn.turn.status, TurnStatus::Completed);
        Ok(())
    }

    async fn delete(app: &mut TestAppServer, thread: String) -> Result<()> {
        let request = app
            .send_thread_delete_request(ThreadDeleteParams { thread_id: thread })
            .await?;
        let _: ThreadDeleteResponse = timeout(WAIT, app.read_response(request)).await??;
        Ok(())
    }

    #[tokio::test]
    async fn resumed_shell_reads_and_consumes_its_original_build_diagnostic() -> Result<()> {
        let home = TempDir::new()?;
        let workspace = TempDir::new()?;
        let server = responses::start_mock_server().await;
        MockResponsesConfig::new(&server.uri())
            .with_approval_policy("never")
            .with_sandbox_mode("workspace-write")
            .enable_feature(Feature::UnifiedExec)
            .disable_feature(Feature::ShellSnapshot)
            .write(home.path())?;

        let first = responses::mount_sse_sequence(
            &server,
            vec![
                build_response("build"),
                build_response("second-build"),
                responses::sse_completed("built"),
                build_response("scratch-build"),
                responses::sse_completed("scratch-done"),
            ],
        )
        .await;
        let mut original = start_candidate(home.path()).await?;
        let params = ThreadStartParams {
            cwd: Some(workspace.path().to_string_lossy().into_owned()),
            history_mode: Some(ThreadHistoryMode::Paginated),
            ..Default::default()
        };
        let source = original.start_thread(params.clone()).await?;
        assert_eq!(source.approval_policy, AskForApproval::Never);
        assert!(matches!(
            source.sandbox,
            SandboxPolicy::WorkspaceWrite {
                exclude_tmpdir_env_var: false,
                exclude_slash_tmp: false,
                ..
            }
        ));
        let owner = source.thread.id;
        complete(&mut original, &owner, "Run the two builds.").await?;
        let path = captured_path(&first, "build")?;
        let second = captured_path(&first, "second-build")?;
        let bytes = format!("{:02048}\n{DIAGNOSTIC}\n{:02048}\n", 0, 0).into_bytes();
        assert_eq!(fs::read(&path)?, bytes);
        assert_eq!(fs::read(&second)?, bytes);
        let scratch = original
            .start_thread(ThreadStartParams {
                ephemeral: Some(true),
                ..params
            })
            .await?
            .thread;
        complete(&mut original, &scratch.id, "Run a scratch build.").await?;
        let temporary = captured_path(&first, "scratch-build")?;
        assert_eq!(fs::read(&temporary)?, bytes);
        assert!(
            timeout(WAIT, original.shutdown_gracefully())
                .await??
                .success()
        );
        drop(original);
        assert!(!temporary.try_exists()?);
        assert_eq!(fs::read(&path)?, bytes);
        assert_eq!(fs::read(&second)?, bytes);

        let mut replacement = start_candidate(home.path()).await?;
        let request = replacement
            .send_thread_resume_request(ThreadResumeParams {
                thread_id: owner.clone(),
                exclude_turns: true,
                ..Default::default()
            })
            .await?;
        let resume: ThreadResumeResponse =
            timeout(WAIT, replacement.read_response(request)).await??;
        assert_eq!(resume.thread.id, owner);
        let consume_script =
            r#"grep -Fx -- "$1" "$2" > recovered-error && cat recovered-error && rm -- "$2""#;
        let consume = app_test_support::create_command_execution_sse_response(
            vec![
                "sh".into(),
                "-c".into(),
                consume_script.into(),
                "sh".into(),
                DIAGNOSTIC.into(),
                path.to_string_lossy().into_owned(),
            ],
            Some(workspace.path()),
            None,
            "recover",
        )?;
        let replay = responses::mount_sse_sequence(
            &server,
            vec![
                consume,
                responses::sse_completed("recovered"),
                build_response("fork-build"),
                responses::sse_completed("fork-done"),
            ],
        )
        .await;
        complete(
            &mut replacement,
            &owner,
            "Recover and consume the first diagnostic.",
        )
        .await?;
        assert_eq!(captured_path(&replay, "build")?, path);
        let result = replay
            .function_call_output_text("recover")
            .context("missing recovery result")?;
        assert!(result.contains("Process exited with code 0"), "{result}");
        assert!(result.lines().any(|line| line == DIAGNOSTIC), "{result}");
        assert_eq!(
            fs::read_to_string(workspace.path().join("recovered-error"))?,
            format!("{DIAGNOSTIC}\n")
        );
        assert!(!path.try_exists()?);

        let request = replacement
            .send_thread_fork_request(ThreadForkParams {
                thread_id: owner.clone(),
                exclude_turns: true,
                ..Default::default()
            })
            .await?;
        let fork: ThreadForkResponse = timeout(WAIT, replacement.read_response(request)).await??;
        let child = fork.thread.id;
        complete(&mut replacement, &child, "Run a build in this fork.").await?;
        let child_log = captured_path(&replay, "fork-build")?;
        assert_eq!(fs::read(&child_log)?, bytes);
        delete(&mut replacement, child).await?;
        assert!(!child_log.try_exists()?);
        assert_eq!(fs::read(&second)?, bytes);
        delete(&mut replacement, owner).await?;
        assert!(!second.try_exists()?);
        assert!(
            timeout(WAIT, replacement.shutdown_gracefully())
                .await??
                .success()
        );
        Ok(())
    }
}
