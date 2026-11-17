// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A retained input owns callback waiting, never human or native permission.
//! The ordinary constructor leaves this disabled. No connection label or
//! capability selects it; only the host's actual Incoming allocation does.

#[cfg(unix)]
use super::OutgoingEnvelope;
#[cfg(unix)]
use super::OutgoingMessageSender;
#[cfg(unix)]
use codex_alab_core::native_incoming::Incoming;
#[cfg(unix)]
use codex_analytics::AnalyticsEventsClient;
use codex_app_server_protocol::CommandExecutionApprovalKind;
use codex_app_server_protocol::ServerRequest;
use codex_protocol::ThreadId;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
#[cfg(unix)]
use tokio::sync::mpsc;

#[derive(Default)]
pub(super) struct WaitOwner {
    #[cfg(unix)]
    incoming: Option<Incoming>,
    closed: AtomicBool,
}

impl WaitOwner {
    fn enabled(&self) -> bool {
        #[cfg(unix)]
        {
            self.incoming.is_some()
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    fn open(&self) -> bool {
        if self.closed.load(Ordering::Acquire) {
            return false;
        }
        #[cfg(unix)]
        {
            self.incoming
                .as_ref()
                .is_some_and(|incoming| !incoming.is_stopped())
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    pub(super) fn keep_gap(&self, request: &ServerRequest, thread: Option<ThreadId>) -> bool {
        thread.is_some()
            && matches!(request, ServerRequest::CommandExecutionRequestApproval { params, .. }
                if params.kind == CommandExecutionApprovalKind::HumanCommand)
            && self.open()
    }

    /// Called under the original callback-map lock immediately before insert.
    /// If withdrawal follows this check, the host's same-lock drain owns the
    /// registered callback. If the drain ran first, close stays sticky here.
    pub(super) fn registration_open(&self) -> bool {
        !self.enabled() || self.open()
    }

    /// Called under that same callback-map lock before cancel_all drains it.
    /// Ordinary senders keep their existing ability to register after a drain.
    pub(super) fn close(&self) {
        if self.enabled() {
            self.closed.store(true, Ordering::Release);
        }
    }
}

#[cfg(unix)]
impl OutgoingMessageSender {
    /// Construct only at retained-host preparation, before publishing the
    /// sender. There is no rebinding operation which could reopen withdrawal.
    pub(crate) fn alab_for_retained_input(
        sender: mpsc::Sender<OutgoingEnvelope>,
        analytics: AnalyticsEventsClient,
        incoming: Incoming,
    ) -> Self {
        let mut outgoing = Self::new(sender, analytics);
        outgoing.alab_callback_outcomes = Some(std::sync::Arc::new(
            super::alab_callback_outcomes::Ledger::for_input(incoming.clone()),
        ));
        outgoing.alab_approval_wait = WaitOwner {
            incoming: Some(incoming),
            closed: AtomicBool::new(false),
        };
        outgoing
    }
}

#[cfg(all(test, unix))]
#[path = "retained_human_approval_wait_tests.rs"]
mod tests;
