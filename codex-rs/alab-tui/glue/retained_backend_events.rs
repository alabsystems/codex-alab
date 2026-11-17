// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Queued original event DATA, never a reply handle or Source admission.
use codex_app_server_client::AppServerEvent;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EventData {
    kind: u8,
    payload: serde_json::Value,
}
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
impl EventData {
    pub(crate) fn capture(event: &AppServerEvent) -> io::Result<Self> {
        let (kind, payload) = match event {
            AppServerEvent::Lagged { skipped } => (0, serde_json::to_value(skipped)),
            AppServerEvent::ServerNotification(value) => (1, serde_json::to_value(value)),
            AppServerEvent::ServerRequest(value) => (2, serde_json::to_value(value)),
            AppServerEvent::ThreadStreamDisconnected(value) => (3, serde_json::to_value(value)),
            AppServerEvent::ThreadStreamReconnected(value) => (4, serde_json::to_value(value)),
            AppServerEvent::Disconnected { message } => (5, serde_json::to_value(message)),
        };
        Ok(Self {
            kind,
            payload: payload.map_err(invalid)?,
        })
    }
    pub(crate) fn is_request(&self) -> bool {
        self.kind == 2
    }
    pub(crate) fn validate_data(&self) -> io::Result<()> {
        let native = self.clone().into_data_event()?;
        if Self::capture(&native)? != *self {
            return Err(invalid(
                "queued event projects or omits original protocol DATA",
            ));
        }
        Ok(())
    }
    // Only caller's already-validated actual resource cut may use this DATA.
    pub(super) fn into_data_event(self) -> io::Result<AppServerEvent> {
        Ok(match self.kind {
            0 => AppServerEvent::Lagged {
                skipped: serde_json::from_value(self.payload).map_err(invalid)?,
            },
            1 => AppServerEvent::ServerNotification(
                serde_json::from_value(self.payload).map_err(invalid)?,
            ),
            2 => AppServerEvent::ServerRequest(
                serde_json::from_value(self.payload).map_err(invalid)?,
            ),
            3 => AppServerEvent::ThreadStreamDisconnected(
                serde_json::from_value(self.payload).map_err(invalid)?,
            ),
            4 => AppServerEvent::ThreadStreamReconnected(
                serde_json::from_value(self.payload).map_err(invalid)?,
            ),
            5 => AppServerEvent::Disconnected {
                message: serde_json::from_value(self.payload).map_err(invalid)?,
            },
            _ => return Err(invalid("unknown queued backend event kind")),
        })
    }
}
