// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Bounded, payload-free observations of the fake host's version handshake.

use serde::Serialize;
use std::io;
use std::time::Instant;

const MAX_EVENTS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HandshakeEvents {
    /// Snapshot time on the same monotonic clock as the events.
    pub captured_elapsed_micros: u64,
    pub events: Vec<HandshakeEvent>,
    /// Events beyond the retained prefix. A nonzero count means this trace is incomplete.
    pub dropped_events: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HandshakeEvent {
    /// Diagnostic accept order, independent of the fake's protocol connection IDs.
    pub connection: u64,
    /// Monotonic microseconds since this fake host was created, not wall-clock time.
    pub elapsed_micros: u64,
    #[serde(flatten)]
    pub stage: HandshakeStage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum HandshakeStage {
    Accepted,
    Authenticated,
    AuthenticationFailed,
    VersionReceived,
    VersionReplyDropped,
    VersionWriteStarted {
        reply_bytes: usize,
    },
    VersionWriteFinished {
        /// Intended reply length; an error may follow a partial write.
        reply_bytes: usize,
        error: Option<HandshakeIoError>,
    },
}

/// Error classification only: never the error's display text or a wire value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HandshakeIoError {
    pub kind: String,
    pub raw_os_error: Option<i32>,
}

impl HandshakeIoError {
    pub(super) fn from_error(error: &io::Error) -> Self {
        Self {
            kind: format!("{:?}", error.kind()),
            raw_os_error: error.raw_os_error(),
        }
    }
}

pub(super) struct HandshakeLog {
    started: Instant,
    next_connection: u64,
    events: Vec<HandshakeEvent>,
    dropped_events: u64,
}

impl HandshakeLog {
    pub(super) fn new() -> Self {
        Self {
            started: Instant::now(),
            next_connection: 0,
            events: Vec::new(),
            dropped_events: 0,
        }
    }

    pub(super) fn accepted(&mut self, at: Instant) -> u64 {
        self.next_connection = self.next_connection.saturating_add(1);
        let connection = self.next_connection;
        self.record(connection, at, HandshakeStage::Accepted);
        connection
    }

    pub(super) fn record(&mut self, connection: u64, at: Instant, stage: HandshakeStage) {
        if self.events.len() == MAX_EVENTS {
            self.dropped_events = self.dropped_events.saturating_add(1);
            return;
        }
        self.events.push(HandshakeEvent {
            connection,
            elapsed_micros: u64::try_from(at.duration_since(self.started).as_micros())
                .unwrap_or(u64::MAX),
            stage,
        });
    }

    pub(super) fn snapshot(&self) -> HandshakeEvents {
        HandshakeEvents {
            captured_elapsed_micros: u64::try_from(self.started.elapsed().as_micros())
                .unwrap_or(u64::MAX),
            events: self.events.clone(),
            dropped_events: self.dropped_events,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_retained_prefix_is_bounded_and_reports_loss() {
        let mut log = HandshakeLog::new();
        for connection in 0..MAX_EVENTS as u64 + 3 {
            log.record(connection, Instant::now(), HandshakeStage::Accepted);
        }
        let snapshot = log.snapshot();
        assert_eq!(snapshot.events.len(), MAX_EVENTS);
        assert_eq!(snapshot.events.first().unwrap().connection, 0);
        assert_eq!(snapshot.events.last().unwrap().connection, 255);
        assert_eq!(snapshot.dropped_events, 3);
        assert!(snapshot.captured_elapsed_micros >= snapshot.events.last().unwrap().elapsed_micros);
    }
}
