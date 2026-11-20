// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use codex_alab_core::shell_log_store::LogDirectory;
use codex_features::Feature;
use codex_protocol::ThreadId;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_custom_tool_call;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::test_codex::TestCodexHarness;
use core_test_support::test_codex::test_codex;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

// The guard outlives the harness and keeps its real Codex home available during cleanup.
struct FixtureShellLogs {
    home: Arc<TempDir>,
    thread_id: ThreadId,
}

impl FixtureShellLogs {
    fn from_harness(harness: &TestCodexHarness) -> Self {
        let fixture = harness.test();
        Self {
            home: Arc::clone(&fixture.home),
            thread_id: fixture.session_configured.thread_id,
        }
    }
}

impl Drop for FixtureShellLogs {
    fn drop(&mut self) {
        let id = self.thread_id.to_string();
        if let Err(err) =
            LogDirectory::open(self.home.path(), &id, false).and_then(LogDirectory::retire)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            if std::thread::panicking() {
                eprintln!("retiring shell logs for fixture thread {id}: {err}");
            } else {
                panic!("retiring shell logs for fixture thread {id}: {err}");
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleting_one_consumed_log_makes_room_for_the_next_build() -> Result<()> {
    core_test_support::skip_if_no_network!(Ok(()));
    codex_alab_core::activate();
    let _retire;
    let harness = TestCodexHarness::with_builder(test_codex().with_model("gpt-5.5")).await?;
    _retire = FixtureShellLogs::from_harness(&harness);
    let build = |id: &str| {
        let args = json!({
            "cmd": format!("printf '{id}-%0256d' 1"),
            "max_output_tokens": 16
        });
        sse(vec![
            ev_response_created(id),
            ev_function_call(id, "exec_command", &args.to_string()),
            ev_completed(id),
        ])
    };
    let done = |id: &str| {
        sse(vec![
            ev_response_created(id),
            ev_assistant_message(id, "done"),
            ev_completed(id),
        ])
    };
    let mut responses: Vec<_> = (0..32).map(|n| build(&format!("build-{n}"))).collect();
    responses.push(sse(vec![
        ev_response_created("at-capacity"),
        ev_function_call(
            "at-capacity",
            "exec_command",
            &json!({
                "cmd": "stty -echo; printf 'busy-%0256d' 1; read -r proceed; printf done; exit 7",
                "tty": true, "yield_time_ms": 1000, "max_output_tokens": 16
            })
            .to_string(),
        ),
        ev_completed("at-capacity"),
    ]));
    responses.push(done("first-turn-done"));
    mount_sse_sequence(harness.server(), responses).await;
    harness.submit("run the builds in order").await?;

    let full = harness.function_call_stdout("at-capacity").await;
    assert!(full.contains("Shell output log unavailable"), "{full}");
    let session_id: i32 = full
        .lines()
        .find_map(|line| line.strip_prefix("Process running with session ID "))
        .context("clipped command exited before the poll")?
        .parse()?;
    let log_path = |result: &str| {
        result
            .lines()
            .find_map(|line| line.strip_prefix("Full shell output saved to: "))
            .map(PathBuf::from)
            .with_context(|| format!("missing log in {result}"))
    };
    let consumed = log_path(&harness.function_call_stdout("build-0").await)?;
    let directory = consumed.parent().context("missing log directory")?;
    let mut survivors = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path != consumed {
            survivors.push((fs::read(&path)?, path));
        }
    }
    assert_eq!(survivors.len(), 31);
    fs::remove_file(&consumed)?;

    mount_sse_sequence(
        harness.server(),
        vec![
            sse(vec![
                ev_response_created("retry"),
                ev_function_call(
                    "retry",
                    "write_stdin",
                    &json!({
                        "session_id": session_id, "chars": "go\n", "yield_time_ms": 10000
                    })
                    .to_string(),
                ),
                ev_completed("retry"),
            ]),
            done("retry-done"),
        ],
    )
    .await;
    harness.submit("finish the waiting build").await?;
    let poll = harness.function_call_stdout("retry").await;
    assert!(poll.contains("Process exited with code 7"), "{poll}");
    let recovered = log_path(&poll)?;
    assert_eq!(
        fs::read(&recovered)?,
        format!("busy-{:0256}done", 1).as_bytes()
    );
    fs::remove_file(recovered)?;

    mount_sse_sequence(
        harness.server(),
        vec![build("next-build"), done("second-turn-done")],
    )
    .await;
    harness.submit("run the next build").await?;
    let next = log_path(&harness.function_call_stdout("next-build").await)?;
    assert!(fs::read_to_string(next)?.starts_with("next-build-"));
    assert!(!consumed.try_exists()?);
    assert_eq!(fs::read_dir(directory)?.count(), 32);
    for (bytes, path) in survivors {
        assert_eq!(fs::read(path)?, bytes);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clipped_shell_output_is_recoverable_through_both_tool_surfaces() -> Result<()> {
    core_test_support::skip_if_no_network!(Ok(()));
    codex_alab_core::activate();
    codex_core::test_support::set_deterministic_process_ids(true);

    let _retire;
    let harness =
        TestCodexHarness::with_auto_env_builder(test_codex().with_model("gpt-5.5")).await?;
    _retire = FixtureShellLogs::from_harness(&harness);
    let cmd = concat!(
        r#"printf 'before-'; head -c 8000 /dev/zero | tr '\000' a; read -r line; "#,
        r#"head -c 700000 /dev/zero | tr '\000' b; printf 'BURIED-DIAGNOSTIC\377'; "#,
        r#"head -c 700000 /dev/zero | tr '\000' c; printf '%s' '-after'; exit 7"#,
    );
    mount_sse_sequence(
        harness.server(),
        vec![
            sse(vec![
                ev_response_created("direct-start"),
                ev_function_call("start", "exec_command", &json!({
                    "cmd": cmd, "tty": true, "yield_time_ms": 1000, "max_output_tokens": 64
                }).to_string()),
                ev_completed("direct-start"),
            ]),
            sse(vec![
                ev_response_created("direct-poll"),
                ev_function_call("poll", "write_stdin", &json!({
                    "session_id": 1000, "chars": "go\n", "yield_time_ms": 10000, "max_output_tokens": 64
                }).to_string()),
                ev_completed("direct-poll"),
            ]),
            sse(vec![
                ev_response_created("direct-short"),
                ev_function_call("short", "exec_command", r#"{"cmd":"printf short-marker"}"#),
                ev_completed("direct-short"),
            ]),
            sse(vec![
                ev_response_created("direct-end"),
                ev_assistant_message("done", "done"),
                ev_completed("direct-end"),
            ]),
        ],
    )
    .await;
    harness
        .submit("exercise the interactive shell tool")
        .await?;

    let start = harness.function_call_stdout("start").await;
    assert!(
        start.contains("Process running with session ID 1000"),
        "{start}"
    );
    let path = start
        .lines()
        .find_map(|line| line.strip_prefix("Shell output is being saved to: "))
        .context("running command did not disclose its log")?;
    let poll = harness.function_call_stdout("poll").await;
    assert!(poll.contains("Process exited with code 7"), "{poll}");
    assert!(
        poll.contains(&format!("Full shell output saved to: {path}")),
        "{poll}"
    );
    assert!(poll.contains("omitted"), "{poll}");
    assert!(!poll.contains("BURIED-DIAGNOSTIC"), "{poll}");
    let contents = fs::read(path)?;
    assert!(contents.starts_with(b"before-"));
    assert!(
        contents
            .windows(b"BURIED-DIAGNOSTIC\xff".len())
            .any(|bytes| bytes == b"BURIED-DIAGNOSTIC\xff")
    );
    assert!(contents.ends_with(b"-after"));
    let log_dir = Path::new(path).parent().context("missing log directory")?;
    assert_eq!(fs::metadata(log_dir)?.permissions().mode() & 0o777, 0o700);
    assert_eq!(fs::metadata(path)?.permissions().mode() & 0o777, 0o600);
    assert_eq!(fs::read_dir(log_dir)?.count(), 1);

    let short = harness.function_call_stdout("short").await;
    assert!(short.ends_with("Output:\nshort-marker"), "{short}");
    assert!(!short.contains("shell output"), "{short}");

    let code = r#"
const start = await tools.exec_command({
    cmd: "stty -echo; head -c 8192 /dev/zero | tr '\\000' a; read -r line; printf 'AFTER-POLL:%s\\n' \"$line\"; exit 9",
    tty: true, yield_time_ms: 1000, max_output_tokens: 32
});
if (!start.full_output_path || start.full_output_status !== "in_progress" || !start.session_id || !start.invocation_id) {
    throw Error(JSON.stringify(start));
}
let rejection;
try {
    rejection = await tools.write_stdin({
        session_id: start.session_id, invocation_id: "00000000-0000-4000-8000-000000000000", chars: "wrong\n"
    });
} catch (error) {
    rejection = String(error);
}
if (!/invocation|recorded observation/i.test(JSON.stringify(rejection))) {
    throw Error("Expected the unknown invocation to be rejected: " + JSON.stringify(rejection));
}
const poll = await tools.write_stdin({session_id: start.session_id, invocation_id: start.invocation_id, chars: "go\n"});
if (poll.exit_code !== 9 || poll.session_id || poll.full_output_path !== start.full_output_path ||
    poll.full_output_status !== undefined) {
    throw Error(JSON.stringify(poll));
}
const quotePath = (path) => "'" + path.replaceAll("'", "'\\''") + "'";
const check = await tools.exec_command({cmd: "grep -aq 'AFTER-POLL:go' " + quotePath(poll.full_output_path)});
if (check.exit_code !== 0 || check.full_output_path !== undefined) {
    throw Error(JSON.stringify(check));
}
const running = await tools.exec_command({
    cmd: "printf '%0512d' 1; read -r line; printf done", tty: true,
    yield_time_ms: 1000, max_output_tokens: 32
});
if (!running.full_output_path || !running.session_id) throw Error(JSON.stringify(running));
const removed = await tools.exec_command({cmd: "rm -- " + quotePath(running.full_output_path)});
if (removed.exit_code !== 0) throw Error(JSON.stringify(removed));
const finished = await tools.write_stdin({session_id: running.session_id, chars: "go\n"});
if (finished.exit_code !== 0 || finished.full_output_path !== undefined ||
    finished.full_output_status !== "unavailable") {
    throw Error(JSON.stringify(finished));
}
const unbudgeted = await tools.exec_command({cmd: "head -c 8192 /dev/zero | tr '\\000' z"});
if (unbudgeted.output.length !== 8192 || unbudgeted.full_output_path !== undefined) {
    throw Error("code-mode shell output was clipped without a shell budget");
}
const capped = await tools.exec_command({
    cmd: "head -c 9437184 /dev/zero; printf LOG-CAP-END",
    yield_time_ms: 10000, max_output_tokens: 32
});
if (!capped.full_output_path || capped.full_output_status !== "partial" || capped.exit_code !== 0 ||
    capped.shell_output_tail !== undefined) {
    throw Error(JSON.stringify(capped));
}
const marker = "unproved: add commutes";
const failed = await tools.exec_command({
    cmd: "exec >&2; head -c 8388608 /dev/zero; head -c 8192 /dev/zero | tr '\\000' x; printf 'unproved: add commutes'; exit 23",
    yield_time_ms: 10000, max_output_tokens: 32
});
const end = 8388608 + 8192 + marker.length;
const tail = failed.shell_output_tail;
if (failed.exit_code !== 23 || failed.full_output_status !== "partial" || !failed.full_output_path ||
    !tail || tail.start_byte !== end - 4096 || tail.end_byte !== end || tail.encoding !== "utf8" ||
    tail.data !== "x".repeat(4096 - marker.length) + marker) {
    throw Error("Code Mode lost the final diagnostic or its byte range: " + JSON.stringify(failed));
}
text("VERIFIED-SHELL-LOG " + poll.full_output_path + "\nVERIFIED-PARTIAL " + capped.full_output_path);
"#;
    let _retire_code;
    let code_harness =
        TestCodexHarness::with_builder(test_codex().with_model("test-gpt-5.1-codex").with_config(
            |config| {
                let _ = config.features.enable(Feature::CodeMode);
            },
        ))
        .await?;
    _retire_code = FixtureShellLogs::from_harness(&code_harness);
    let code_responses = mount_sse_sequence(
        code_harness.server(),
        vec![
            sse(vec![
                ev_response_created("code-start"),
                ev_custom_tool_call("code", "exec", code),
                ev_completed("code-start"),
            ]),
            sse(vec![
                ev_response_created("code-end"),
                ev_assistant_message("done", "done"),
                ev_completed("code-end"),
            ]),
        ],
    )
    .await;
    code_harness
        .submit("inspect structured shell output")
        .await?;
    let tool_result = code_responses
        .last_request()
        .context("missing request carrying the code-mode result")?
        .custom_tool_call_output("code");
    let output = tool_result
        .get("output")
        .and_then(Value::as_array)
        .with_context(|| format!("expected code-mode content items: {tool_result}"))?
        .iter()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    let code_path = output
        .lines()
        .find_map(|line| line.strip_prefix("VERIFIED-SHELL-LOG "))
        .with_context(|| format!("code-mode did not recover its log: {output}"))?;
    assert_ne!(Path::new(code_path).parent(), Some(log_dir));
    let capped_path = output
        .lines()
        .find_map(|line| line.strip_prefix("VERIFIED-PARTIAL "))
        .with_context(|| format!("code-mode did not identify the partial log: {output}"))?;
    assert_eq!(fs::metadata(capped_path)?.len(), 8 * 1024 * 1024);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clipped_sandbox_denials_retain_only_emitted_output() -> Result<()> {
    use codex_protocol::models::PermissionProfile;
    use codex_protocol::permissions::FileSystemAccessMode;
    use codex_protocol::permissions::FileSystemPath;
    use codex_protocol::permissions::FileSystemSandboxEntry;
    use codex_protocol::permissions::FileSystemSandboxPolicy;
    use codex_protocol::permissions::NetworkSandboxPolicy;

    core_test_support::skip_if_no_network!(Ok(()));
    core_test_support::skip_if_sandbox!(Ok(()));
    codex_alab_core::activate();

    let _retire;
    let harness = TestCodexHarness::with_builder(test_codex().with_model("gpt-5.5")).await?;
    _retire = FixtureShellLogs::from_harness(&harness);
    harness
        .write_file("blocked.env", "WITHHELD-CONTENT")
        .await?;
    let mut policy = FileSystemSandboxPolicy::default();
    policy.entries.push(FileSystemSandboxEntry {
        path: FileSystemPath::GlobPattern {
            pattern: format!("{}/**/*.env", harness.cwd().display()),
        },
        access: FileSystemAccessMode::Deny,
        missing_path_behavior: None,
    });
    let profile =
        PermissionProfile::from_runtime_permissions(&policy, NetworkSandboxPolicy::Restricted);
    let denied = "cat blocked.env";
    let long = format!(
        "head -c 4000 /dev/zero | tr '\\000' a; printf 'BURIED-ALLOWED'; head -c 4000 /dev/zero | tr '\\000' b; {denied}"
    );
    let call = |id, tool, args: Value| {
        sse(vec![
            ev_response_created(id),
            ev_function_call(id, tool, &args.to_string()),
            ev_completed(id),
        ])
    };
    mount_sse_sequence(
        harness.server(),
        vec![
            call("fast", "exec_command", json!({"cmd": long, "max_output_tokens": 40})),
            call("late", "exec_command", json!({"cmd": format!("sleep 1; {long}"), "yield_time_ms": 10000, "max_output_tokens": 40})),
            call("start", "exec_command", json!({"cmd": format!("while [ ! -f release-poll ]; do sleep 0.1; done; {long}"), "yield_time_ms": 1000})),
            sse(vec![ev_response_created("ready"), ev_assistant_message("ready-msg", "ready"), ev_completed("ready")]),
        ],
    ).await;
    harness
        .submit_with_permission_profile("exercise a denied read", profile.clone())
        .await?;
    let start = harness.function_call_stdout("start").await;
    let session_id: i32 = start
        .lines()
        .find_map(|line| line.strip_prefix("Process running with session ID "))
        .with_context(|| format!("missing running process: {start}"))?
        .parse()?;
    harness.write_file("release-poll", "").await?;
    mount_sse_sequence(
        harness.server(),
        vec![
            call("poll", "write_stdin", json!({"session_id": session_id, "chars": "", "yield_time_ms": 10000, "max_output_tokens": 40})),
            call("short", "exec_command", json!({"cmd": denied})),
            sse(vec![ev_response_created("end"), ev_assistant_message("done", "done"), ev_completed("end")]),
        ],
    ).await;
    harness
        .submit_with_permission_profile("collect the denied command", profile)
        .await?;
    let mut paths = std::collections::HashSet::new();
    for id in ["fast", "late", "poll"] {
        let result = harness.function_call_stdout(id).await;
        assert!(
            result.contains("Process exited with code 1"),
            "{id}: {result}"
        );
        assert!(!result.contains("BURIED-ALLOWED"), "{id}: {result}");
        let path = result
            .lines()
            .find_map(|line| {
                line.strip_prefix("Full shell output saved to: ")
                    .or_else(|| line.strip_prefix("Partial shell output saved to: "))
            })
            .with_context(|| format!("{id} omitted the shell output path: {result}"))?;
        assert!(
            paths.insert(path.to_owned()),
            "distinct denied processes shared a log"
        );
        let captured = fs::read_to_string(path)?;
        assert!(
            captured.contains("BURIED-ALLOWED"),
            "{id} lost emitted output"
        );
        assert!(captured.contains("blocked.env"), "{id} lost the denial");
        assert!(
            !captured.contains("WITHHELD-CONTENT"),
            "{id} read forbidden content"
        );
    }
    let short = harness.function_call_stdout("short").await;
    assert!(short.contains("Process exited with code 1"), "{short}");
    assert!(short.contains("blocked.env"), "{short}");
    assert!(!short.contains("saved to: "), "{short}");
    Ok(())
}
