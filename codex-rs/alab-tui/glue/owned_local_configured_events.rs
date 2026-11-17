// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Same-session required configured tail with unrelated original events kept
//! in FIFO custody. Deferred events have ALREADY passed original session state
//! updates and are returned once, before later physical events.
use super::*;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::ServerNotification;
use std::io;
use std::sync::Arc;

const MAX_DEFERRED: usize = 4096;
impl AppServerSession {
    pub(crate) async fn next_event(&mut self) -> Option<AppServerSessionEvent> {
        if let Some(event) = self.alab_configured_events.pop_front() {
            return Some(event);
        }
        self.alab_next_physical_event().await
    }
    pub(crate) async fn alab_wait_configured_tail(
        &mut self,
        pending: &Arc<crate::startup_draft::alab_configured::Pending>,
    ) -> io::Result<()> {
        let mut guard = pending.begin_tail(self)?;
        loop {
            // Bypass only our OWN deferred queue; the original session update
            // path still handles each physical event exactly once.
            let event = self.alab_next_physical_event().await.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "configured tail connection ended",
                )
            })?;
            if !pending.current_on(self) || event.backend_id != self.backend_id() {
                self.alab_configured_events.push_back(event);
                return Err(io::Error::other("configured tail changed physical session"));
            }
            if let AppServerEvent::ServerNotification(notification) = &event.event
                && let ServerNotification::AlabRetainedConfigured(tail) = notification.as_ref()
                && guard.consume(tail)?
            {
                return Ok(());
            }
            let disconnected = matches!(&event.event, AppServerEvent::Disconnected { .. });
            self.alab_configured_events.push_back(event); // Preserve BEFORE any error/next await.
            if disconnected || self.alab_configured_events.len() > MAX_DEFERRED {
                return Err(io::Error::other(
                    "configured tail unavailable before deferred event bound",
                ));
            }
        }
    }
}

#[cfg(test)]
#[path = "owned_local_configured_events_tests.rs"]
mod tests;
