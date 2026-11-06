// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::InProcessClientMessage;
use super::InProcessClientSender;
use super::ProcessorCommand;
use super::SuspendedExec;
use crate::message_processor::alab_exec_suspend::Request;
use codex_protocol::ThreadId;
use std::io;
use tokio::sync::oneshot;

pub(super) fn forward(sender: &tokio::sync::mpsc::Sender<ProcessorCommand>, request: Request) {
    if let Err(error) = sender.try_send(ProcessorCommand::AlabExecSuspend(request))
        && let ProcessorCommand::AlabExecSuspend(request) = error.into_inner()
    {
        request.reject("exec suspension processor is unavailable");
    }
}

impl InProcessClientSender {
    pub async fn bootstrap_exec(
        &self,
        request: super::ExecBootstrapRequest,
    ) -> io::Result<super::BootstrappedExec> {
        let (reply, receive) = oneshot::channel();
        self.try_send_client_message(InProcessClientMessage::AlabExecSuspend(
            Request::Bootstrap { request, reply },
        ))?;
        receive
            .await
            .map_err(|_| io::Error::other("exec bootstrap reply was lost"))?
            .map_err(io::Error::other)
    }

    /// Stop this exec turn and wait for its root and descendant writers.
    ///
    /// Success is not authorization to select credentials or resume the turn.
    /// The embedded runtime remains closed to new starts after success. A lost
    /// reply or an error is not evidence that the old execution has stopped.
    /// Once queued, dropping this caller does not cancel the suspension.
    pub async fn suspend_exec_turn(
        &self,
        thread: ThreadId,
        turn: String,
        configuration: codex_core::config::ExecConfiguration,
    ) -> io::Result<SuspendedExec> {
        let (reply, receive) = oneshot::channel();
        self.try_send_client_message(InProcessClientMessage::AlabExecSuspend(Request::Suspend {
            thread,
            turn,
            configuration: Box::new(configuration),
            reply,
        }))?;
        receive
            .await
            .map_err(|_| io::Error::other("exec suspension reply was lost"))?
            .map_err(io::Error::other)
    }
}

impl InProcessClientSender {
    /// Observe live account-quota waits from this embedded exec thread.
    pub async fn watch_exec_quota(
        &self,
        thread: ThreadId,
    ) -> io::Result<codex_core::ExecQuotaWatch> {
        let (reply, receive) = oneshot::channel();
        self.try_send_client_message(InProcessClientMessage::AlabExecSuspend(
            Request::WatchQuota { thread, reply },
        ))?;
        receive
            .await
            .map_err(|_| io::Error::other("exec quota observation reply was lost"))?
            .map_err(io::Error::other)
    }
}

impl InProcessClientSender {
    pub async fn activate_exec_tree(
        &self,
        thread: ThreadId,
        turn: String,
    ) -> io::Result<codex_core::StartIfIdleSubmission> {
        let (reply, receive) = oneshot::channel();
        self.try_send_client_message(InProcessClientMessage::AlabExecSuspend(
            Request::ActivateTree {
                thread,
                turn,
                reply,
            },
        ))?;
        receive
            .await
            .map_err(|_| io::Error::other("exec tree activation reply was lost"))?
            .map_err(io::Error::other)
    }
}
