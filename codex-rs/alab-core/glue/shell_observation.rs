// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::UnifiedExecProcess;
use super::shell_output::ShellLog;
use crate::function_tool::FunctionCallError;
use crate::session::session::Session;
use crate::tools::context::ExecCommandToolOutput;
use crate::tools::context::ToolCallSource;
use crate::tools::context::ToolInvocation;
use codex_exec_server::ExecProcessEvent;
use codex_protocol::ThreadId;
use codex_state::PendingShellExit;
use codex_state::ShellLaunchContext;
use codex_thread_store::LocalThreadStore;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::Weak;
use uuid::Uuid;

#[cfg(unix)]
#[path = "shell_diagnostic_publication.rs"]
mod diagnostic_publication;
#[cfg(unix)]
pub(crate) use diagnostic_publication::reconcile_diagnostics;

#[cfg(not(unix))]
pub(crate) async fn reconcile_diagnostics(
    _db: &codex_state::StateRuntime,
    _home: &Path,
    _thread: ThreadId,
    _eligible: Option<&HashSet<Uuid>>,
) -> anyhow::Result<HashSet<Uuid>> {
    Ok(HashSet::new())
}

/// A subscription is installed only for an invocation the native tool stored.
/// It never keeps a session or its local state database alive.
#[derive(Clone, Debug)]
struct WatchTarget {
    db: Weak<codex_state::StateRuntime>,
    thread: ThreadId,
    number: i32,
}

impl WatchTarget {
    async fn persist(&self, log: &ShellLog, code: Option<i32>) {
        let Some(db) = self.db.upgrade() else { return };
        let invocation = log.invocation_id();
        if let Err(err) = db
            .record_watched_shell_exit(self.thread, invocation, self.number, code)
            .await
        {
            tracing::warn!("could not retain observed shell exit: {err}");
            return;
        }
        let Some(code) = code.filter(|code| *code != 0 && *code != -1) else {
            return;
        };
        if !log.diagnostic_ready() {
            return;
        }
        // One writer owns file publication through the database acknowledgment.
        let Ok(_permit) = log.diagnostic_writer.acquire().await else {
            return;
        };
        if !log.diagnostic_ready() {
            return;
        }
        // A deleted or evicted invocation no longer owns filesystem storage.
        match db.shell_invocation(self.thread, invocation).await {
            Ok(Some(row)) if row.session_id == self.number && row.exit_code == Some(code) => {}
            Ok(_) => {
                log.release_drained_reader(true);
                return;
            }
            Err(err) => {
                tracing::warn!("could not check shell diagnostic ownership: {err}");
                return;
            }
        }
        #[cfg(unix)]
        match diagnostic_publication::persist(&db, self.thread, self.number, code, log).await {
            Ok(attached) => {
                let capture = log.clone();
                if let Err(err) =
                    tokio::task::spawn_blocking(move || capture.finish_diagnostic(attached)).await
                {
                    tracing::warn!(
                        "could not adopt the retained file for a later shell poll: {err}"
                    );
                }
                log.release_drained_reader(!attached);
            }
            Err(err) => tracing::warn!("could not attach retained shell diagnostic: {err:#}"),
        }
        #[cfg(not(unix))]
        log.release_drained_reader(true);
    }
}

#[derive(Clone, Copy, Debug, Default)]
enum Receipt {
    #[default]
    Pending,
    Exited(Option<i32>),
}

#[derive(Debug, Default)]
pub(super) struct ExitWatch {
    received: Receipt,
    target: Option<WatchTarget>,
    sealed: bool,
    runtime: Option<tokio::runtime::Handle>,
}

impl ExitWatch {
    pub(super) fn confirmed_failure(&self) -> bool {
        matches!(self.received, Receipt::Exited(Some(code)) if code != 0 && code != -1)
    }

    /// Freeze the status the first local tool result established before its
    /// original reader can be retired while the SQLite target is still binding.
    pub(super) fn confirm_first_exit(&mut self, code: i32) {
        if code != 0 && code != -1 {
            let _ = self.receive(Some(code));
        }
    }

    fn receive(&mut self, code: Option<i32>) -> Option<WatchTarget> {
        // The local PTY and exec-server use -1 for failed waits and dropped
        // senders. It cannot establish a child exit or an authentic status.
        if self.sealed || code == Some(-1) {
            return None;
        }
        match (self.received, code) {
            (Receipt::Pending, _) | (Receipt::Exited(None), Some(_)) => {
                self.received = Receipt::Exited(code)
            }
            _ => return None,
        }
        self.target.clone()
    }

    fn bind(&mut self, target: &WatchTarget) -> Receipt {
        if !self.sealed && self.target.is_none() {
            self.target = Some(target.clone());
            self.runtime = tokio::runtime::Handle::try_current().ok();
        }
        self.received
    }

    fn seal(&mut self) -> Option<(WatchTarget, Option<i32>)> {
        self.sealed = true;
        let target = self.target.take()?;
        match self.received {
            Receipt::Exited(code) => Some((target, code)),
            Receipt::Pending => None,
        }
    }
}

// A raw reader can close after the backend has already recorded the exit.
pub(super) fn output_drained(log: &ShellLog) {
    if !log.diagnostic_ready() {
        log.release_drained_reader(false);
        return;
    }
    let work = log.with_exit_watch(|watch| {
        let Receipt::Exited(Some(code)) = watch.received else {
            return None;
        };
        if code == 0 || code == -1 {
            return None;
        }
        Some((watch.target.clone()?, code, watch.runtime.clone()?))
    });
    if let Some((target, code, runtime)) = work {
        let log = log.clone();
        runtime.spawn(async move { target.persist(&log, Some(code)).await });
    }
}

/// Called only for an exit reported by the process backend. Closed output,
/// cancellation and a missing process handle are independent of this receipt.
async fn observe_exit(log: Option<&ShellLog>, code: Option<i32>) {
    if let Some(log) = log
        && let Some(target) = log.with_exit_watch(|watch| watch.receive(code))
    {
        target.persist(log, code).await;
    }
}

fn event_exit(event: Option<&ExecProcessEvent>, previous_seq: u64) -> Option<i32> {
    match event {
        Some(ExecProcessEvent::Exited { seq, exit_code, .. }) if *seq > previous_seq => {
            Some(*exit_code)
        }
        _ => None,
    }
}

pub(super) async fn observe_server_event(
    log: Option<&ShellLog>,
    event: Option<&ExecProcessEvent>,
    previous_seq: u64,
) {
    if let Some(code) = event_exit(event, previous_seq) {
        observe_exit(log, Some(code)).await;
    }
}

pub(super) async fn observe_server_read(log: Option<&ShellLog>, exited: bool, code: Option<i32>) {
    if exited {
        observe_exit(log, code).await;
    }
}

/// The local oneshot closing carries no exit report.
pub(super) async fn observe_local_exit(log: Option<&ShellLog>, code: Option<i32>) {
    if let Some(code) = code {
        observe_exit(log, Some(code)).await;
    }
}

/// Freeze already received evidence before requesting shutdown of this shell.
/// A compiler still awaiting a real exit receipt keeps its last tool observation.
pub(super) async fn settle_before_termination(process: &UnifiedExecProcess) {
    if process.invocation_id.is_none() {
        return;
    }
    let log = process
        .output_handles()
        .output_buffer
        .lock()
        .await
        .shell_log
        .clone();
    if let Some(log) = log
        && let Some((target, code)) = log.with_exit_watch(ExitWatch::seal)
    {
        if code.is_some_and(|code| code != 0 && code != -1) {
            log.finish_observed_failure().await;
        }
        target.persist(&log, code).await;
    }
}

fn local_durable(session: &Session) -> bool {
    session.live_thread().is_some()
        && session
            .services
            .thread_store
            .as_any()
            .is::<LocalThreadStore>()
}

/// Record the native result before returning it to either tool surface.
/// Returns true after the durable row is acquired. A first-call reader whose
/// row could not be registered can then release its original process owner.
pub(crate) async fn record(
    session: &Session,
    source: &ToolCallSource,
    session_id: i32,
    output: &mut ExecCommandToolOutput,
    poll_bytes: Option<usize>,
) -> bool {
    let initial = poll_bytes.is_none();
    let one_shot = initial && output.process_id.is_none();
    let known_code = output.exit_code.filter(|code| *code != -1);
    let final_failure = output.process_id.is_none() && known_code.is_some_and(|code| code != 0);
    if one_shot && !final_failure {
        return false;
    }
    let Some(log) = output.raw_output.shell_log.clone() else {
        return false;
    };
    if !local_durable(session) {
        return false;
    }
    let poll_permit = if one_shot || poll_bytes.is_some() {
        log.diagnostic_writer.acquire().await.ok()
    } else {
        None
    };
    let disclosed = output.prepare_shell_observation(source, final_failure);
    let undisclosed_tail = output.undelivered_shell_tail();
    // The terminal result already collected the exit. A reader still owned by
    // this first call, or a tail excluded from its history, can supply new evidence.
    let first_reader = one_shot && log.first_exit_uncollected();
    if one_shot && !first_reader && undisclosed_tail.is_none() {
        return false;
    }
    let Some(db) = session.state_db() else {
        return false;
    };
    let capture = log.retained_basename();
    let output_uncollected = first_reader
        || (one_shot && undisclosed_tail.is_some())
        || poll_bytes.is_some_and(|bytes| log.collect_poll(bytes, final_failure, disclosed));
    let tail = if one_shot {
        undisclosed_tail
    } else {
        output.frozen_shell_tail()
    };
    // Only a local launch has a tool_cwd. It is the resolved directory even
    // when the tool call omitted workdir. A poll carries no new launch facts.
    let local_cwd = output.tool_cwd.as_ref().filter(|_| initial);
    let native_cwd = local_cwd.and_then(|cwd| cwd.to_abs_path().ok());
    let launch = local_cwd
        .and(output.hook_command.as_deref())
        .map(|command| {
            (
                command,
                native_cwd.as_ref().and_then(|cwd| cwd.as_path().to_str()),
            )
        });
    if let Err(err) = db
        .record_shell_invocation(
            session.thread_id,
            log.invocation_id(),
            session_id,
            output.process_id.is_none() && known_code.is_some(),
            known_code,
            capture.as_deref(),
            disclosed.is_some_and(|output| output.exact_path),
            &output.raw_output.text,
            initial,
            launch,
            output_uncollected,
            tail,
        )
        .await
    {
        tracing::warn!("could not retain shell invocation observation: {err}");
        return false;
    }
    if one_shot && !first_reader {
        return true;
    }
    drop(poll_permit);
    log.release_drained_reader(false);
    let target = WatchTarget {
        db: Arc::downgrade(&db),
        thread: session.thread_id,
        number: session_id,
    };
    let (receipt, sealed) = log.with_exit_watch(|watch| {
        let receipt = watch.bind(&target);
        if output_uncollected && matches!(receipt, Receipt::Pending | Receipt::Exited(None)) {
            // The terminal result can precede the background wait task. Its
            // confirmed status was collected; the original reader owns any update.
            watch.receive(known_code);
            (Receipt::Exited(known_code), watch.sealed)
        } else {
            (receipt, watch.sealed)
        }
    });
    if let Receipt::Exited(code) = receipt {
        // The backend may have reported an exit before the initial row existed.
        if sealed && code.is_some_and(|code| code != 0 && code != -1) {
            log.finish_observed_failure().await;
        }
        target.persist(&log, code).await;
    }
    true
}

#[cfg(unix)]
fn retained_path(home: &Path, thread: ThreadId, name: Option<&str>) -> Option<String> {
    let name = name?;
    let directory = crate::alab_shell_log_store::existing_directory(home, thread).ok()??;
    if !directory.retains_capture(name.as_ref()).ok()? {
        return None;
    }
    directory
        .path()
        .join(name)
        .into_os_string()
        .into_string()
        .ok()
}

#[cfg(not(unix))]
fn retained_path(_home: &Path, _thread: ThreadId, _name: Option<&str>) -> Option<String> {
    None
}

fn add_launch_fields(row: &mut serde_json::Value, launch: &ShellLaunchContext) {
    row["command"] = serde_json::json!(launch.command);
    if launch.command_truncated {
        row["command_truncated"] = serde_json::Value::Bool(true);
    }
    if let Some(cwd) = &launch.cwd {
        row["cwd"] = serde_json::json!(cwd);
    }
}

// Escape the JSON text after serialization so the model sees one delimited
// record even if a command or pathname contains a closing tag or XML entity.
fn tagged_json(value: &serde_json::Value) -> String {
    value
        .to_string()
        .replace('<', r"\u003c")
        .replace('>', r"\u003e")
        .replace('&', r"\u0026")
}

/// The native identity, launch and observed result enter the resumed request.
/// Return the invocation IDs whose retained file paths this request discloses.
pub(crate) fn resume_exit_text(
    home: &Path,
    thread: ThreadId,
    exits: &[PendingShellExit],
) -> (String, HashSet<Uuid>) {
    let mut disclosed_paths = HashSet::new();
    let rows: Vec<_> = exits
        .iter()
        .map(|exit| {
            let mut row = serde_json::json!({
                "invocation_id": exit.invocation,
                "session_id": exit.session_id,
                "exit_code": exit.exit_code,
            });
            if let Some(launch) = &exit.launch {
                add_launch_fields(&mut row, launch);
            }
            if !exit.capture_rejected
                && let Some(path) = retained_path(home, thread, exit.capture_name.as_deref())
            {
                row["retained_output_path"] = serde_json::json!(path);
                disclosed_paths.insert(exit.invocation);
                if exit.capture_partial == Some(true) {
                    row["retained_output_partial"] = serde_json::Value::Bool(true);
                }
            } else if exit.capture_rejected || exit.capture_name.is_some() {
                row["retained_output_unavailable"] = serde_json::Value::Bool(true);
            }
            if let Some(tail) = &exit.tail {
                row["shell_output_tail"] = serde_json::json!(tail);
            }
            row
        })
        .collect();
    let observations = tagged_json(&serde_json::Value::Array(rows));
    (
        format!("<observed_shell_exits>\n{observations}\n</observed_shell_exits>"),
        disclosed_paths,
    )
}

pub(crate) async fn stale(
    invocation: &ToolInvocation,
    supplied: Option<Uuid>,
    session_id: i32,
    max_tokens: Option<usize>,
) -> FunctionCallError {
    let Some(id) = supplied else {
        return FunctionCallError::RespondToModel(format!(
            "Shell session {session_id} has no unambiguous live binding. Pass the invocation_id from the original running exec_command result. No input was sent."
        ));
    };
    let session = &invocation.session;
    let mut message = format!(
        "Shell invocation {id} is not bound to live session {session_id}. No input was sent."
    );
    let db = session.state_db().filter(|_| local_durable(session));
    // A stale lookup only reads evidence. The returned tool error has not yet
    // reached history and cannot acknowledge a pending model notification.
    let observation = if let Some(db) = db {
        match db.shell_invocation(session.thread_id, id).await {
            Ok(record) => record,
            Err(err) => {
                tracing::warn!("could not read shell invocation observation: {err}");
                None
            }
        }
    } else {
        None
    };
    if let Some(record) = observation {
        message.push_str(&format!("\nOriginal session ID: {}", record.session_id));
        if let Some(code) = record.exit_code {
            message.push_str(&format!("\nLast observed exit code: {code}"));
        } else {
            let status = if record.finished {
                "finished (exit code unavailable)"
            } else {
                "in_progress"
            };
            message.push_str(&format!("\nLast observed status: {status}"));
        }
        if let Some(launch) = &record.launch {
            let mut fields = serde_json::json!({});
            add_launch_fields(&mut fields, launch);
            message.push_str(&format!(
                "\nOriginal shell launch (JSON): {}",
                tagged_json(&fields)
            ));
        }
        if !record.capture_rejected
            && let Some(path) = retained_path(
                &invocation.step_context.turn.config.codex_home,
                session.thread_id,
                record.capture_name.as_deref(),
            )
        {
            message.push_str(&format!("\nRetained shell output: {path}"));
            if record.capture_partial == Some(true) {
                message.push_str("\nRetained output is partial.");
            }
        } else if record.capture_rejected || record.capture_name.is_some() {
            message.push_str("\nRetained shell output unavailable.");
        }
        if let Some(tail) = &record.tail {
            message.push_str("\nShell output tail: ");
            message.push_str(&tagged_json(&serde_json::json!(tail)));
        }
        let requested = codex_utils_output_truncation::TruncationPolicy::Tokens(
            super::resolve_max_tokens(max_tokens),
        );
        let policy: codex_utils_output_truncation::TruncationPolicy = invocation
            .step_context
            .settings
            .model_info
            .truncation_policy
            .into();
        let budget = policy.byte_budget().min(requested.byte_budget());
        if budget > 0 && !record.preview.is_empty() {
            message.push_str("\nLast recorded output preview:\n");
            message.push_str(&codex_utils_output_truncation::truncate_text(
                &record.preview,
                codex_utils_output_truncation::TruncationPolicy::Bytes(budget),
            ));
        }
    } else {
        message.push_str("\nNo recorded observation for this invocation in this thread.");
    }
    FunctionCallError::RespondToModel(message)
}

#[cfg(test)]
#[path = "shell_observation_tests.rs"]
mod tests;
