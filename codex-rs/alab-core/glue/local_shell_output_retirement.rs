// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::ExecServerError;
use super::LocalExecProcess;
use super::ProcessEntry;
use super::RunningProcess;

/// The capture owner seals its accepted bytes before retiring these readers.
/// Late queued chunks can complete the ordinary event stream without claiming
/// that the retired capture saw every byte.
pub(super) async fn stop(handle: &LocalExecProcess) -> Result<(), ExecServerError> {
    let mut processes = handle.backend.inner.processes.lock().await;
    if let Some(ProcessEntry::Running(process)) = processes.get_mut(&handle.process_id)
        && process.wake_tx.same_channel(&handle.wake_tx)
        && process.exit_code.is_some()
        && process.output_observer.take().is_some()
    {
        abort_readers(process);
    }
    Ok(())
}

fn abort_readers(process: &RunningProcess) {
    for reader in [
        process.session.stdout_reader(),
        process.session.stderr_reader(),
    ]
    .into_iter()
    .flatten()
    {
        reader.abort();
    }
}

/// Resolve final ownership against this original handle before either an
/// ordinary live-process termination or retirement of failed, inherited pipes.
pub(super) async fn dispose(handle: &LocalExecProcess) -> Result<bool, ExecServerError> {
    loop {
        {
            let mut processes = handle.backend.inner.processes.lock().await;
            let Some(ProcessEntry::Running(process)) = processes.get_mut(&handle.process_id) else {
                return Ok(false);
            };
            if !process.wake_tx.same_channel(&handle.wake_tx) {
                return Ok(false);
            }
            // The native waiter can precede the server event and briefly exposes
            // has_exited before it stores the code. Once exited, never signal it.
            if let Some(code) = process.exit_code.or_else(|| process.session.exit_code()) {
                if code != 0
                    && code != -1
                    && !process.termination_requested
                    && !process.session.native_output_eof().forced_before_exit()
                    && process.output_observer.take().is_some()
                {
                    abort_readers(process);
                    return Ok(true);
                }
                return Ok(false);
            }
            if !process.session.has_exited() {
                // A final owner of a still-running process retains ordinary Drop behavior.
                if let Some(shutdown) = &process.network_policy_shutdown {
                    shutdown.cancel();
                }
                process.termination_requested = true;
                process.session.terminate();
                return Ok(false);
            }
        }
        tokio::task::yield_now().await;
    }
}
