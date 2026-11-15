// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Shared custody for exact-child ALab contract tests.

use std::fmt;
use std::io;
use std::process::ExitStatus;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::process::Child;
use tokio::process::ChildStderr;
use tokio::process::ChildStdout;
use tokio::process::Command;

const RETAINED: usize = 64 * 1024;
const PREFIX: usize = RETAINED / 2;
const LINE_LIMIT: usize = 4096;
const MARKER_LIMIT: usize = 4096;
const CLEANUP: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug)]
pub(crate) struct TestSummary {
    pub passed: u64,
    pub failed: u64,
    ignored: u64,
    measured: u64,
    success: bool,
}

fn count_field(text: &str, suffix: &str) -> Option<u64> {
    let number = text.strip_suffix(suffix)?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    number.parse().ok()
}

fn summary_line(line: &[u8]) -> Option<TestSummary> {
    let text = std::str::from_utf8(line).ok()?.trim_end_matches('\r');
    let text = text.strip_prefix("test result: ")?;
    let (grade, fields) = text.split_once(". ")?;
    let success = match grade {
        "ok" => true,
        "FAILED" => false,
        _ => return None,
    };
    let mut fields = fields.split("; ");
    let summary = TestSummary {
        passed: count_field(fields.next()?, " passed")?,
        failed: count_field(fields.next()?, " failed")?,
        ignored: count_field(fields.next()?, " ignored")?,
        measured: count_field(fields.next()?, " measured")?,
        success,
    };
    count_field(fields.next()?, " filtered out")?;
    let elapsed = fields
        .next()?
        .strip_prefix("finished in ")?
        .strip_suffix('s')?;
    let (seconds, fraction) = elapsed.split_once('.')?;
    if seconds.is_empty()
        || fraction.is_empty()
        || !seconds.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || fields.next().is_some()
    {
        return None;
    }
    Some(summary)
}

struct RequiredMarker {
    needle: Vec<u8>,
    fallback: Vec<usize>,
    matched: usize,
    seen: bool,
}

impl RequiredMarker {
    fn new(needle: &[u8]) -> Self {
        let mut fallback = vec![0; needle.len()];
        let mut matched = 0;
        for index in 1..needle.len() {
            while matched > 0 && needle[index] != needle[matched] {
                matched = fallback[matched - 1];
            }
            if needle[index] == needle[matched] {
                matched += 1;
            }
            fallback[index] = matched;
        }
        Self {
            needle: needle.to_vec(),
            fallback,
            matched: 0,
            seen: false,
        }
    }

    fn record(&mut self, bytes: &[u8]) {
        // KMP keeps chunk boundaries and discarded retention bytes irrelevant.
        for byte in bytes {
            while self.matched > 0 && *byte != self.needle[self.matched] {
                self.matched = self.fallback[self.matched - 1];
            }
            if *byte == self.needle[self.matched] {
                self.matched += 1;
            }
            if self.matched == self.needle.len() {
                self.seen = true;
                self.matched = self.fallback[self.matched - 1];
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct CapturedOutput {
    prefix: Vec<u8>,
    tail: Vec<u8>,
    pub total_bytes: u64,
    summary: Option<TestSummary>,
    line: Vec<u8>,
    discard_line: bool,
    eof: bool,
    read_error: Option<String>,
    required_marker: Option<RequiredMarker>,
}

impl CapturedOutput {
    fn record(&mut self, bytes: &[u8], scan_summary: bool) {
        if let Some(marker) = &mut self.required_marker {
            marker.record(bytes);
        }
        self.total_bytes = self.total_bytes.saturating_add(bytes.len() as u64);
        let prefix = (PREFIX - self.prefix.len()).min(bytes.len());
        self.prefix.extend_from_slice(&bytes[..prefix]);
        let remainder = &bytes[prefix..];
        if remainder.len() >= PREFIX {
            self.tail.clear();
            self.tail
                .extend_from_slice(&remainder[remainder.len() - PREFIX..]);
        } else {
            let excess = (self.tail.len() + remainder.len()).saturating_sub(PREFIX);
            self.tail.drain(..excess);
            self.tail.extend_from_slice(remainder);
        }
        if !scan_summary {
            return;
        }
        for byte in bytes {
            if *byte == b'\n' {
                self.finish_line();
            } else if !self.discard_line {
                if self.line.len() == LINE_LIMIT {
                    if self.line.starts_with(b"test result: ") {
                        self.summary = None;
                    }
                    self.line.clear();
                    self.discard_line = true;
                } else {
                    self.line.push(*byte);
                }
            }
        }
    }

    fn finish_line(&mut self) {
        if !self.discard_line && self.line.starts_with(b"test result: ") {
            self.summary = summary_line(&self.line);
        }
        self.line.clear();
        self.discard_line = false;
    }

    pub(crate) fn prefix(&self) -> &[u8] {
        &self.prefix
    }

    pub(crate) fn tail(&self) -> &[u8] {
        &self.tail
    }

    pub(crate) fn retained_len(&self) -> usize {
        self.prefix.len() + self.tail.len()
    }

    pub(crate) fn is_truncated(&self) -> bool {
        self.total_bytes > self.retained_len() as u64
    }

    pub(crate) fn summary(&self) -> Option<TestSummary> {
        self.summary
    }

    pub(crate) fn complete_eof(&self) -> bool {
        self.eof && self.read_error.is_none()
    }

    pub(crate) fn required_marker_seen(&self) -> Option<bool> {
        self.required_marker.as_ref().map(|marker| marker.seen)
    }

    fn render(&self) -> String {
        let mut rendered = String::from_utf8_lossy(self.prefix()).into_owned();
        if self.is_truncated() {
            rendered.push_str(&format!(
                "\n[{} output bytes omitted]\n",
                self.total_bytes.saturating_sub(self.retained_len() as u64)
            ));
        }
        rendered.push_str(&String::from_utf8_lossy(self.tail()));
        rendered
    }
}

impl fmt::Debug for CapturedOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CapturedOutput")
            .field("total_bytes", &self.total_bytes)
            .field("retained_bytes", &self.retained_len())
            .field("truncated", &self.is_truncated())
            .field("summary", &self.summary)
            .field("eof", &self.eof)
            .field("read_error", &self.read_error)
            .field("required_marker_seen", &self.required_marker_seen())
            .field(
                "required_marker_len",
                &self
                    .required_marker
                    .as_ref()
                    .map(|marker| marker.needle.len()),
            )
            .finish()
    }
}

#[derive(Debug)]
pub(crate) struct Report {
    pub status: ExitStatus,
    pub stdout: CapturedOutput,
    pub stderr: CapturedOutput,
}

impl Report {
    pub(crate) fn assert_test_success(&self, exact: &str) {
        let one_passed = self.stdout.summary().is_some_and(|summary| {
            summary.success
                && summary.passed == 1
                && summary.failed == 0
                && summary.ignored == 0
                && summary.measured == 0
        });
        assert!(
            self.status.success()
                && one_passed
                && self.stdout.required_marker_seen() != Some(false)
                && self.stdout.complete_eof()
                && self.stderr.complete_eof(),
            "{exact}: status={}; stdout={:?}; stderr={:?}\n{}\n{}",
            self.status,
            self.stdout,
            self.stderr,
            self.stdout.render(),
            self.stderr.render(),
        );
    }
}

#[derive(Debug)]
pub(crate) struct Failure {
    pub kind: &'static str,
    detail: String,
    pid: Option<u32>,
    pub status: Option<ExitStatus>,
    pub stdout: CapturedOutput,
    pub stderr: CapturedOutput,
    pub cleanup_error: Option<String>,
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "child {}: {}; pid={:?}; status={:?}; cleanup={:?}; stdout={:?}; stderr={:?}\n{}\n{}",
            self.kind,
            self.detail,
            self.pid,
            self.status,
            self.cleanup_error,
            self.stdout,
            self.stderr,
            self.stdout.render(),
            self.stderr.render()
        )
    }
}

async fn drain<R: AsyncRead + Unpin>(
    pipe: &mut R,
    capture: &mut CapturedOutput,
    scan_summary: bool,
) -> io::Result<()> {
    // Cleanup retries unfinished pipes, never an already failed or complete one.
    if capture.eof || capture.read_error.is_some() {
        return Ok(());
    }
    let mut buffer = [0; 8192];
    let mut reads = 0;
    loop {
        match pipe.read(&mut buffer).await {
            Ok(0) => {
                if scan_summary {
                    capture.finish_line();
                }
                capture.eof = true;
                return Ok(());
            }
            Ok(count) => {
                // Commit evidence synchronously before another cancellation point.
                capture.record(&buffer[..count], scan_summary);
                reads += 1;
                if reads == 16 {
                    reads = 0;
                    tokio::task::yield_now().await;
                }
            }
            Err(error) => {
                capture.read_error = Some(error.to_string());
                return Err(error);
            }
        }
    }
}

async fn collect(
    child: &mut Child,
    stdout: &mut ChildStdout,
    stderr: &mut ChildStderr,
    status: &mut Option<ExitStatus>,
    output: &mut CapturedOutput,
    errors: &mut CapturedOutput,
) -> io::Result<()> {
    tokio::try_join!(
        async {
            if status.is_none() {
                *status = Some(child.wait().await?);
            }
            Ok::<(), io::Error>(())
        },
        drain(stdout, output, true),
        drain(stderr, errors, false),
    )?;
    Ok(())
}

/// Drain real EOFs while bounding retained evidence, then reap the exact child.
///
/// Internal timeout/I/O failure owns bounded kill, reap and final pipe drainage.
/// External cancellation can only request `kill_on_drop`; it does not promise an
/// asynchronous reap. Adapters await this runner's own configured deadline.
#[allow(
    dead_code,
    reason = "Some test modules use only the marked entry point."
)]
pub(crate) async fn run(command: Command, timeout: Duration) -> Result<Report, Box<Failure>> {
    run_with_stdout_marker(command, timeout, None).await
}

/// Observe an optional literal stdout marker in addition to normal child custody.
pub(crate) async fn run_with_stdout_marker(
    mut command: Command,
    timeout: Duration,
    expected_stdout_marker: Option<&str>,
) -> Result<Report, Box<Failure>> {
    let mut output = CapturedOutput::default();
    if let Some(marker) = expected_stdout_marker {
        if marker.is_empty() || marker.len() > MARKER_LIMIT {
            return Err(Box::new(Failure {
                kind: "setup",
                detail: format!(
                    "stdout marker length {} is outside 1..={MARKER_LIMIT} bytes",
                    marker.len(),
                ),
                pid: None,
                status: None,
                stdout: output,
                stderr: CapturedOutput::default(),
                cleanup_error: None,
            }));
        }
        output.required_marker = Some(RequiredMarker::new(marker.as_bytes()));
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Err(Box::new(Failure {
                kind: "spawn",
                detail: error.to_string(),
                pid: None,
                status: None,
                stdout: output,
                stderr: CapturedOutput::default(),
                cleanup_error: None,
            }));
        }
    };
    let pid = child.id();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let mut errors = CapturedOutput::default();
    let mut status = None;
    let completed = tokio::time::timeout(
        timeout,
        collect(
            &mut child,
            &mut stdout,
            &mut stderr,
            &mut status,
            &mut output,
            &mut errors,
        ),
    )
    .await;
    match completed {
        Ok(Ok(())) => Ok(Report {
            status: status.expect("successful collection reaps its child"),
            stdout: output,
            stderr: errors,
        }),
        failure => {
            let kill_error = if status.is_none() {
                child.start_kill().err().map(|error| error.to_string())
            } else {
                None
            };
            let cleanup = tokio::time::timeout(
                CLEANUP,
                collect(
                    &mut child,
                    &mut stdout,
                    &mut stderr,
                    &mut status,
                    &mut output,
                    &mut errors,
                ),
            )
            .await;
            let cleanup_error = match cleanup {
                Ok(Ok(())) => kill_error,
                Ok(Err(error)) => Some(format!("kill={kill_error:?}; reap/drain={error}")),
                Err(_) => Some(format!("kill={kill_error:?}; reap/drain deadline")),
            };
            Err(Box::new(Failure {
                kind: if failure.is_err() {
                    "timeout"
                } else {
                    "wait/capture"
                },
                detail: format!("{failure:?}"),
                pid,
                status,
                stdout: output,
                stderr: errors,
                cleanup_error,
            }))
        }
    }
}
