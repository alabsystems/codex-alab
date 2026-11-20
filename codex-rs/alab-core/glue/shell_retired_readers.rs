// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::ProcessHandle;
use super::UnifiedExecProcess;
use crate::tools::context::ExecCommandToolOutput;
use crate::tools::context::ToolCallSource;
use crate::unified_exec::MAX_UNIFIED_EXEC_PROCESSES;
use crate::unified_exec::UnifiedExecContext;
use crate::unified_exec::UnifiedExecProcessManager;
use crate::unified_exec::shell_observation;
use crate::unified_exec::shell_output::ShellLog;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use tokio_util::task::TaskTracker;
use tokio_util::task::task_tracker::TaskTrackerToken;
use uuid::Uuid;

/// These subprocess owners have no entry in the writable terminal registry.
#[derive(Debug, Default)]
pub(crate) struct ReaderPool {
    readers: Mutex<VecDeque<RetiredReader>>,
    stopping: TaskTracker,
}

#[derive(Debug)]
pub(crate) struct RetiredReader {
    process: Arc<UnifiedExecProcess>,
    log: ShellLog,
}

impl super::OutputTaskGuard {
    pub(super) fn finish_native(&mut self, readers: codex_utils_pty::NativeOutputEof) {
        if let Some(log) = &self.shell_log {
            log.close_raw(readers);
        }
        self.complete = true;
    }
}

/// Installed before the server startup future first yields. The final native
/// owner retains the same cleanup obligation through startup, the live registry
/// and read-only retirement.
pub(super) struct ServerCapture {
    log: ShellLog,
    closing: Mutex<Option<TaskTrackerToken>>,
}

impl ServerCapture {
    pub(super) fn new(handle: &ProcessHandle, log: Option<&ShellLog>) -> Option<Self> {
        let ProcessHandle::ExecServer(original) = handle else {
            return None;
        };
        let log =
            log.filter(|log| original.started_with_local_output_observer() && log.durable_owner())?;
        Some(Self {
            log: log.clone(),
            closing: Mutex::new(log.launch_pool().and_then(|pool| pool.cleanup_token())),
        })
    }

    fn take_token(&self) -> Option<TaskTrackerToken> {
        self.closing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    fn protect_handoff(&self) {
        if !self.log.raw_closed() {
            *self
                .closing
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                self.log.launch_pool().and_then(|pool| pool.cleanup_token());
        }
    }

    fn closing_token(&self) -> Option<TaskTrackerToken> {
        self.take_token()
            .or_else(|| self.log.launch_pool().and_then(|pool| pool.cleanup_token()))
    }
}

impl UnifiedExecProcess {
    pub(crate) fn protect_first_handoff(&self) {
        if let Some(capture) = &self.server_capture {
            capture.protect_handoff();
        }
    }

    pub(super) fn release_startup_or_handoff(&self) {
        if let Some(capture) = &self.server_capture {
            drop(capture.take_token());
        }
    }

    pub(super) fn dispose_final_server_capture(&self) -> bool {
        let (ProcessHandle::ExecServer(original), Some(capture)) =
            (&self.process_handle, &self.server_capture)
        else {
            return false;
        };
        if capture.log.raw_closed() {
            return false;
        }
        let source = Arc::clone(original);
        let log = capture.log.clone();
        let token = capture.closing_token();
        tokio::spawn(async move {
            let _token = token;
            match source.dispose_owned_local_capture().await {
                Ok(true) => log.close(false),
                Ok(false) => {}
                Err(err) => tracing::warn!("could not release the original local capture: {err}"),
            }
        });
        self.finish_termination();
        true
    }

    fn start_exited_close(self: &Arc<Self>, log: &ShellLog) -> tokio::task::JoinHandle<()> {
        log.close(false);
        let token = self
            .server_capture
            .as_ref()
            .and_then(ServerCapture::closing_token);
        let process = Arc::clone(self);
        tokio::spawn(async move {
            let _token = token;
            process.close_exited_output().await;
        })
    }

    fn owns_local_output(&self) -> bool {
        match &self.process_handle {
            ProcessHandle::Local(original) => {
                let readers = original.native_output_eof();
                readers.observed().is_some() && !readers.forced_before_exit()
            }
            ProcessHandle::ExecServer(original) => original.started_with_local_output_observer(),
        }
    }

    async fn close_exited_output(&self) {
        if let ProcessHandle::ExecServer(original) = &self.process_handle
            && let Err(err) = original.stop_local_output_after_exit().await
        {
            tracing::warn!("could not close retired local shell reader: {err}");
        }
        self.terminate();
    }
}

impl UnifiedExecProcessManager {
    /// Both upstream launch paths still own the original process after finalizing
    /// their public ID. Arm only this already-exited process's native reader.
    pub(crate) async fn finish_first_shell_result(
        &self,
        context: &UnifiedExecContext,
        process: &Arc<UnifiedExecProcess>,
        number: i32,
        source: &ToolCallSource,
        result: &mut ExecCommandToolOutput,
        bytes: usize,
    ) {
        let Some(code) = result.exit_code else { return };
        let Some(log) = result.raw_output.shell_log.as_ref() else {
            return;
        };
        if !log.first_failure_result() || !process.owns_local_output() {
            return;
        }

        let disclosed = result.prepare_shell_observation(source, true);
        let Some(log) = result.raw_output.shell_log.as_mut() else {
            return;
        };
        log.collect_first_exit(bytes, disclosed, code);
        process.protect_first_handoff();

        let admission = {
            let store = self.process_store.lock().await;
            let readers = &store.readers;
            if log.launched_in(readers)
                && !readers.is_closed()
                && !context.cancellation_token.is_cancelled()
            {
                let stopped = readers.keep(process, Some(log), Some(code), store.processes.len());
                log.remember_first_admission();
                let registration = if log.first_exit_uncollected()
                    || log.undelivered_tail().is_some()
                {
                    // Admit the DB write in the same store-lock interval as its reader. A
                    // canceled caller drops only its wait; bulk shutdown waits the write.
                    let invocation = log.invocation_id();
                    let owner = Arc::clone(readers);
                    let session = Arc::clone(&context.session);
                    let source = source.clone();
                    let mut receipt = result.clone();
                    Some(readers.stopping.spawn(async move {
                        if !shell_observation::record(&session, &source, number, &mut receipt, None)
                            .await
                            && let Some(reader) = owner.take(invocation)
                        {
                            let _ = reader.stop(false, &owner.stopping).await;
                        }
                    }))
                } else {
                    None
                };
                process.release_startup_or_handoff();
                Some((registration, stopped))
            } else {
                None
            }
        };
        let Some((registration, stopped)) = admission else {
            let Some(log) = result.raw_output.shell_log.as_mut() else {
                return;
            };
            let stopped = process.start_exited_close(log);
            log.remember_sealed_first_exit();
            // Retire the source without reviving its old generation. Only bytes
            // or coverage that survived this seal can acquire a durable receipt.
            if !context.cancellation_token.is_cancelled()
                && (log.first_exit_uncollected() || log.undelivered_tail().is_some())
            {
                shell_observation::record(&context.session, source, number, result, None).await;
            }
            let _ = stopped.await;
            return;
        };
        if let Some(registration) = registration
            && let Err(err) = registration.await
        {
            tracing::warn!("first shell output registration stopped: {err}");
        }
        if let Some(stopped) = stopped {
            let _ = stopped.await;
        }
    }
}

impl ReaderPool {
    fn cleanup_token(&self) -> Option<TaskTrackerToken> {
        let _readers = self
            .readers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (!self.is_closed()).then(|| self.stopping.token())
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.stopping.is_closed()
    }

    pub(crate) fn keep(
        self: &Arc<Self>,
        process: &Arc<UnifiedExecProcess>,
        log: Option<&ShellLog>,
        code: Option<i32>,
        active_processes: usize,
    ) -> Option<tokio::task::JoinHandle<()>> {
        let log = log?;
        if !code.is_some_and(|code| code != 0 && code != -1)
            || process.timed_out()
            || process.termination_requested.load(Ordering::Acquire)
        {
            return None;
        }
        if self.is_closed()
            || matches!(&process.process_handle,
            ProcessHandle::ExecServer(original) if !original.started_with_local_output_observer())
        {
            return None;
        }
        let mut readers = self
            .readers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !log.retain_reader(Arc::downgrade(self)) {
            return None;
        }
        if let ProcessHandle::Local(original) = &process.process_handle {
            original.close_stdin();
        }
        let incoming = RetiredReader {
            process: Arc::clone(process),
            log: log.clone(),
        };
        if active_processes >= MAX_UNIFIED_EXEC_PROCESSES {
            // A fast/unregistered root never occupied a writable slot. An
            // all-live store keeps those processes and freezes the new prefix.
            drop(readers);
            return Some(incoming.stop(false, &self.stopping));
        }
        let oldest = if active_processes.saturating_add(readers.len()) >= MAX_UNIFIED_EXEC_PROCESSES
        {
            readers.pop_front()
        } else {
            None
        };
        readers.push_back(incoming);
        drop(readers);
        oldest.map(|reader| reader.stop(false, &self.stopping))
    }

    fn take(&self, invocation: Uuid) -> Option<RetiredReader> {
        let mut readers = self
            .readers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let index = readers
            .iter()
            .position(|reader| reader.log.invocation_id() == invocation)?;
        readers.remove(index)
    }

    pub(crate) fn release(&self, invocation: Uuid) {
        drop(self.take(invocation));
    }

    pub(crate) fn make_room(&self, active_processes: usize) -> Option<tokio::task::JoinHandle<()>> {
        let mut readers = self
            .readers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active_processes.saturating_add(readers.len()) >= MAX_UNIFIED_EXEC_PROCESSES {
            let removed = readers.pop_front();
            drop(readers);
            return removed.map(|reader| reader.stop(false, &self.stopping));
        }
        None
    }

    pub(crate) fn close_all(&self) -> TaskTracker {
        let readers = {
            let mut readers = self
                .readers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            self.stopping.close();
            std::mem::take(&mut *readers)
        };
        for reader in readers {
            drop(reader.stop(true, &self.stopping));
        }
        self.stopping.clone()
    }
}

impl RetiredReader {
    /// Start the close before handing the caller its wait. A cancelled tool
    /// cannot strand the original readers after they leave the manager.
    fn stop(self, allow_drain: bool, tracker: &TaskTracker) -> tokio::task::JoinHandle<()> {
        if !allow_drain {
            self.log.close(false);
        }
        tracker.spawn(async move {
            if allow_drain {
                self.log.finish_observed_failure().await;
            }
            self.log.close(false);
            // Release native I/O before SQLite. Its closed prefix and receipt survive.
            self.process.close_exited_output().await;
            shell_observation::settle_before_termination(&self.process).await;
        })
    }
}
