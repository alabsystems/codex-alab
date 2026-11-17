// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Retained native tool IO; adapted from core exec output handling.
use super::*;

pub(super) async fn consume_output(
    mut child: codex_alab_core::native_root_child::Child,
    expiration: ExecExpiration,
    capture_policy: ExecCapturePolicy,
    stdout_stream: Option<StdoutStream>,
) -> Result<RawExecToolCallOutput> {
    // Both stdout and stderr were configured with `Stdio::piped()`
    // above, therefore `take()` should normally return `Some`.  If it doesn't
    // we treat it as an exceptional I/O error

    let stdout_reader = child.stdout.take().ok_or_else(|| {
        CodexErr::Io(io::Error::other(
            "stdout pipe was unexpectedly not available",
        ))
    })?;
    let stderr_reader = child.stderr.take().ok_or_else(|| {
        CodexErr::Io(io::Error::other(
            "stderr pipe was unexpectedly not available",
        ))
    })?;

    let retained_bytes_cap = capture_policy.retained_bytes_cap();
    let stdout_handle = tokio::spawn(read_output(
        BufReader::new(stdout_reader),
        stdout_stream.clone(),
        /*is_stderr*/ false,
        retained_bytes_cap,
    ));
    let stderr_handle = tokio::spawn(read_output(
        BufReader::new(stderr_reader),
        stdout_stream.clone(),
        /*is_stderr*/ true,
        retained_bytes_cap,
    ));

    let expiration_wait = async {
        if capture_policy.uses_expiration() {
            Some(expiration.wait_with_outcome().await)
        } else {
            std::future::pending::<Option<ExecExpirationOutcome>>().await
        }
    };
    tokio::pin!(expiration_wait);
    let mut expiration_resolved = false;
    let (mut exit_status, mut timed_out) = tokio::select! {
        status_result = child.wait() => {
            let exit_status = status_result?;
            (exit_status, false)
        }
        outcome = &mut expiration_wait => {
            expiration_resolved = true;
            match outcome {
                Some(ExecExpirationOutcome::TimedOut) => {
                    child.stop().await?;
                    (
                        synthetic_exit_status(EXIT_CODE_SIGNAL_BASE + TIMEOUT_CODE),
                        true,
                    )
                }
                Some(ExecExpirationOutcome::Cancelled) => {
                    child.stop().await?;
                    (synthetic_exit_status_for_code(/*code*/ 1), false)
                }
                None => unreachable!("expiration wait only resolves while expiration is active"),
            }
        }
        _ = tokio::signal::ctrl_c() => {
            child.stop().await?;
            (synthetic_exit_status(EXIT_CODE_SIGNAL_BASE + SIGKILL_CODE), false)
        }
    };

    // We need mutable bindings so we can `abort()` them on timeout.
    use tokio::task::JoinHandle;

    async fn await_output(
        handle: &mut JoinHandle<std::io::Result<StreamOutput<Vec<u8>>>>,
        timeout: Duration,
    ) -> std::io::Result<StreamOutput<Vec<u8>>> {
        match tokio::time::timeout(timeout, &mut *handle).await {
            Ok(join_res) => match join_res {
                Ok(io_res) => io_res,
                Err(join_err) => Err(std::io::Error::other(join_err)),
            },
            Err(_elapsed) => {
                // Timeout: abort the task to avoid hanging on open pipes.
                handle.abort();
                Ok(StreamOutput {
                    text: Vec::new(),
                    truncated_after_lines: None,
                })
            }
        }
    }

    let mut stdout_handle = stdout_handle;
    let mut stderr_handle = stderr_handle;

    let (stdout, stderr) = if matches!(
        capture_policy,
        ExecCapturePolicy::FullBufferWithExpiration | ExecCapturePolicy::SensitiveFullBuffer
    ) {
        let mut stdout_done = false;
        let mut stderr_done = false;
        let drain = async {
            let stdout = (&mut stdout_handle).await;
            stdout_done = true;
            let stdout = stdout.map_err(io::Error::other)??;
            let stderr = (&mut stderr_handle).await;
            stderr_done = true;
            let stderr = stderr.map_err(io::Error::other)??;
            Ok::<_, io::Error>((stdout, stderr))
        };
        // The leader can exit while descendants still hold its pipes. Keep the original
        // expiration active until draining finishes, without polling a completed future again.
        let drained = tokio::select! {
            biased;
            outcome = &mut expiration_wait, if !expiration_resolved => {
                match outcome {
                    Some(ExecExpirationOutcome::TimedOut) => {
                        exit_status = synthetic_exit_status(EXIT_CODE_SIGNAL_BASE + TIMEOUT_CODE);
                        timed_out = true;
                    }
                    Some(ExecExpirationOutcome::Cancelled) => {
                        exit_status = synthetic_exit_status_for_code(/*code*/ 1);
                    }
                    None => unreachable!("full-buffer capture always uses expiration"),
                }
                None
            }
            result = tokio::time::timeout(capture_policy.io_drain_timeout(), drain) => {
                Some(result.unwrap_or_else(|_| Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "capture output pipes did not close before the drain deadline",
                ))))
            }
        };
        match drained {
            Some(Ok(output)) => output,
            failure => {
                let cleanup = child.stop().await;
                stdout_handle.abort();
                stderr_handle.abort();
                if !stdout_done {
                    let _ = stdout_handle.await;
                }
                if !stderr_done {
                    let _ = stderr_handle.await;
                }
                cleanup?;
                if let Some(Err(err)) = failure {
                    return Err(err.into());
                }
                (
                    StreamOutput {
                        text: Vec::new(),
                        truncated_after_lines: None,
                    },
                    StreamOutput {
                        text: Vec::new(),
                        truncated_after_lines: None,
                    },
                )
            }
        }
    } else {
        let stdout = await_output(&mut stdout_handle, capture_policy.io_drain_timeout()).await?;
        let stderr = await_output(&mut stderr_handle, capture_policy.io_drain_timeout()).await?;
        (stdout, stderr)
    };
    let aggregated_output = aggregate_output(&stdout, &stderr, retained_bytes_cap);

    Ok(RawExecToolCallOutput {
        exit_status,
        stdout,
        stderr,
        aggregated_output,
        timed_out,
    })
}
