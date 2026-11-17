// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Actual original pair creation once. Census cannot keep the App channel open
//! or be reconstructed from a serialized channel/sender label.
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use std::io;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::mpsc::WeakUnboundedSender;

#[derive(Clone)]
pub(crate) struct Census {
    sender: WeakUnboundedSender<AppEvent>,
}
pub(crate) fn channel() -> (
    UnboundedSender<AppEvent>,
    UnboundedReceiver<AppEvent>,
    Census,
) {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let census = Census {
        sender: sender.downgrade(),
    };
    (sender, receiver, census)
}
impl Census {
    /// Actual paired receiver identity only; no Native/Source permission.
    pub(crate) fn validate_sender(&self, actual: &AppEventSender) -> io::Result<()> {
        self.require_live(actual)
    }
    pub(super) fn matches(&self, actual: &AppEventSender) -> bool {
        self.sender
            .upgrade()
            .is_some_and(|original| original.same_channel(&actual.app_event_tx))
    }
    pub(super) fn require_live(&self, actual: &AppEventSender) -> io::Result<()> {
        if self.matches(actual) && !actual.app_event_tx.is_closed() {
            Ok(())
        } else {
            Err(io::Error::other(
                "original actual App receiver census is absent, closed or different",
            ))
        }
    }
    pub(super) fn require_open(&self) -> io::Result<()> {
        if self
            .sender
            .upgrade()
            .is_some_and(|actual| !actual.is_closed())
        {
            Ok(())
        } else {
            Err(io::Error::other(
                "original actual App receiver closed before native capture",
            ))
        }
    }
}
