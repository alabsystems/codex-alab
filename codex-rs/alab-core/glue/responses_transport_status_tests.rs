// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exhausted native retries can change transport without creating a standing warning.

use super::super::ResponsesStreamRequest;
use super::super::ResponsesStreamRetryState;
use super::super::handle_response_stream_error;
use crate::session::step_context::StepContext;
use crate::session::tests::make_session_and_context_with_auth_and_config_and_rx;
use codex_login::CodexAuth;
use codex_protocol::error::CodexErr;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::EventMsg;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;

const CHILD_ROOT: &str = "ALAB_TRANSPORT_STATUS_CHILD_ROOT";
const COMPLETE: &str = "ALAB_TRANSPORT_STATUS_CHILD_OK";

fn child_name() -> std::io::Result<String> {
    let (_, module) = module_path!()
        .split_once("::")
        .ok_or_else(|| std::io::Error::other("the test module has a crate prefix"))?;
    Ok(format!("{module}::isolated_transport_fallback_child"))
}

#[tokio::test]
async fn an_exhausted_websocket_retry_reports_http_recovery_without_a_warning()
-> std::io::Result<()> {
    assert!(!codex_alab_core::active(), "only the exact child activates");
    let private = tempfile::Builder::new()
        .prefix("alab-transport-")
        .tempdir()?
        .keep();
    for name in [
        "home", "codex", "tmp", "config", "state", "cache", "runtime",
    ] {
        std::fs::create_dir(private.join(name))?;
    }
    writeln!(
        std::io::stderr().lock(),
        "retained transport status child: {}",
        private.display()
    )?;
    let name = child_name()?;
    let mut command = tokio::process::Command::new(std::env::current_exe()?.canonicalize()?);
    command
        .args([
            "--exact",
            &name,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env(CHILD_ROOT, &private)
        .env("HOME", private.join("home"))
        .env("USERPROFILE", private.join("home"))
        .env("CODEX_HOME", private.join("codex"))
        .env("TMPDIR", private.join("tmp"))
        .env("TEMP", private.join("tmp"))
        .env("TMP", private.join("tmp"))
        .env("XDG_CONFIG_HOME", private.join("config"))
        .env("XDG_STATE_HOME", private.join("state"))
        .env("XDG_CACHE_HOME", private.join("cache"))
        .env("XDG_RUNTIME_DIR", private.join("runtime"))
        .env(
            "PATH",
            std::env::var_os("PATH")
                .ok_or_else(|| std::io::Error::other("contained PATH is required"))?,
        )
        .env("RUST_MIN_STACK", "8388608")
        .current_dir(&private)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for key in [
        "DYLD_LIBRARY_PATH",
        "LD_LIBRARY_PATH",
        "CARGO_BIN_EXE_test_stdio_server",
        "CARGO_BIN_EXE_codex-code-mode-host",
        "CARGO_BIN_EXE_codex_code_mode_host",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut child = command.spawn()?;
    let pid = child
        .id()
        .ok_or_else(|| std::io::Error::other("the child has an exact PID"))?;
    std::fs::write(private.join("child.pid"), pid.to_string())?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("owned child stdout"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("owned child stderr"))?;
    let out = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).await.map(|_| bytes)
    });
    let err = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).await.map(|_| bytes)
    });
    let (status, hung) = match tokio::time::timeout(Duration::from_secs(60), child.wait()).await {
        Ok(status) => (status?, false),
        Err(_) => {
            child.start_kill()?;
            (child.wait().await?, true)
        }
    };
    let stdout = out.await.map_err(std::io::Error::other)??;
    let stderr = err.await.map_err(std::io::Error::other)??;
    std::fs::write(private.join("child.stdout"), &stdout)?;
    std::fs::write(private.join("child.stderr"), &stderr)?;
    std::fs::write(private.join("child.exit"), status.to_string())?;
    if hung {
        return Err(std::io::Error::other(
            "transport child hang detector; output retained",
        ));
    }
    assert!(status.success(), "child failed; see {}", private.display());
    assert!(
        String::from_utf8_lossy(&stdout)
            .lines()
            .any(|line| line == COMPLETE),
        "the exact child completed its causal assertions; see {}",
        private.display()
    );
    assert!(!codex_alab_core::active(), "the parent remains inactive");
    Ok(())
}

#[tokio::test]
#[ignore = "exact private child; the parent supplies its isolated environment"]
async fn isolated_transport_fallback_child() -> std::io::Result<()> {
    assert!(!codex_alab_core::active());
    let private = std::env::var_os(CHILD_ROOT)
        .map(PathBuf::from)
        .ok_or_else(|| std::io::Error::other("missing private transport fixture"))?;
    assert_eq!(
        std::env::var_os("CODEX_HOME").map(PathBuf::from),
        Some(private.join("codex"))
    );
    tokio::time::timeout(Duration::from_secs(60), native_fallback_exercise())
        .await
        .map_err(|_| std::io::Error::other("native fallback exercise hang detector"))?;
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "\n{COMPLETE}")?;
    stdout.flush()?;
    Ok(())
}

async fn native_fallback_exercise() {
    const DIAGNOSTIC: &str = "the fixture WebSocket closed before its response completed";
    codex_alab_core::activate();
    let (session, turn, events) = make_session_and_context_with_auth_and_config_and_rx(
        CodexAuth::from_api_key("private-transport-fixture-not-a-key"),
        Vec::new(),
        |config| config.model_provider.supports_websockets = true,
    )
    .await;
    assert!(
        session.services.model_client.responses_websocket_enabled(),
        "the actual model client begins with WebSockets enabled"
    );
    while events.try_recv().is_ok() {}
    let mut client = session.services.model_client.new_session();
    let mut state = ResponsesStreamRetryState {
        retries: 2,
        ..Default::default()
    };
    let error = CodexErr::Stream(DIAGNOSTIC.into());
    assert!(
        error.retry_delay(3).is_some(),
        "the stream error is retryable"
    );
    assert!(
        error.retry_after().is_none(),
        "no sleep or provider call is needed"
    );
    let step = StepContext::for_test(turn);
    let recovered = handle_response_stream_error(
        &mut state,
        /*max_retries*/ 2,
        error,
        &mut client,
        &session,
        &step,
        ResponsesStreamRequest::Sampling,
    )
    .await;
    assert!(
        recovered.is_ok(),
        "the actual handler resumes its loop: {recovered:?}"
    );
    assert!(
        !session.services.model_client.responses_websocket_enabled(),
        "the exhausted handler actually selected HTTP fallback"
    );
    assert_eq!(state.retries, 0, "fallback reset the ordinary retry count");
    let notices = std::iter::from_fn(|| events.try_recv().ok()).collect::<Vec<_>>();
    assert_eq!(notices.len(), 1, "one actual fallback notice: {notices:?}");
    let notice = &notices[0];
    assert_eq!(
        notice.id, step.turn.sub_id,
        "the same native turn owns the notice"
    );
    let (message, details) = match &notice.msg {
        EventMsg::Warning(warning) => (warning.message.as_str(), None),
        EventMsg::StreamError(error) => {
            (error.message.as_str(), error.additional_details.as_deref())
        }
        other => panic!("the actual handler emitted an unexpected notice: {other:?}"),
    };
    assert!(
        message.contains(DIAGNOSTIC) || details.is_some_and(|text| text.contains(DIAGNOSTIC)),
        "the actual stream diagnostic remains observable: {notice:?}"
    );
    assert!(
        matches!(
            &notice.msg,
            EventMsg::StreamError(error)
                if error.codex_error_info == Some(CodexErrorInfo::ResponseStreamDisconnected {
                    http_status_code: None,
                })
        ),
        "successful HTTP fallback reports a transient retry, not a persistent warning: {notice:?}"
    );
}

#[test]
fn inactive_and_unrelated_fallback_notices_keep_their_original_events()
-> Result<(), serde_json::Error> {
    use codex_protocol::protocol::StreamErrorEvent;
    use codex_protocol::protocol::WarningEvent;

    let error = CodexErr::Stream("a separate transport error".into());
    let status = || {
        EventMsg::StreamError(StreamErrorEvent {
            message: "an existing retry status".into(),
            codex_error_info: Some(CodexErrorInfo::ResponseStreamDisconnected {
                http_status_code: Some(503),
            }),
            additional_details: Some("original status details".into()),
        })
    };
    for (active, event) in [
        (
            false,
            EventMsg::Warning(WarningEvent {
                message: "the original transport diagnostic".into(),
            }),
        ),
        (false, status()),
        (true, status()),
    ] {
        let before = serde_json::to_vec(&event)?;
        let observed = super::fallback_notice_when_active(active, event, &error);
        assert_eq!(
            serde_json::to_vec(&observed)?,
            before,
            "inactive or unrelated input stays byte-equivalent (active={active})"
        );
    }
    Ok(())
}
