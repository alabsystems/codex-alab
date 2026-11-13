// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Nonblocking, payload-free observations after a fixture selects its timeout.

use std::fmt;
use std::sync::Mutex;
use std::sync::TryLockError;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinHandle;

const RECENT_METHODS: usize = 16;

#[derive(Debug, PartialEq, Eq)]
enum RecorderState {
    Readable,
    Busy,
    Poisoned,
}

/// These fields are sampled separately, after timeout selection. A finished
/// task may have failed; a closed receiver may still contain buffered events.
pub struct RegistrationSnapshot {
    creation_finished: bool,
    proxy_finished: bool,
    events_closed: bool,
    queued_events: usize,
    recorder: RecorderState,
    recorded_count: Option<usize>,
    recent_methods: Vec<&'static str>,
}

/// Observe without polling tasks, receiving events, or waiting for the recorder.
/// The accessor is applied only to the retained tail; no request is formatted.
pub fn registration<T, P, E, R>(
    creation: &JoinHandle<T>,
    proxy: &JoinHandle<P>,
    events: &UnboundedReceiver<E>,
    requests: &Mutex<Vec<R>>,
    method: for<'a> fn(&'a R) -> &'a str,
) -> RegistrationSnapshot {
    let mut snapshot = RegistrationSnapshot {
        creation_finished: creation.is_finished(),
        proxy_finished: proxy.is_finished(),
        events_closed: events.is_closed(),
        queued_events: events.len(),
        recorder: RecorderState::Readable,
        recorded_count: None,
        recent_methods: Vec::new(),
    };
    match requests.try_lock() {
        Ok(requests) => {
            snapshot.recorded_count = Some(requests.len());
            snapshot.recent_methods = requests[requests.len().saturating_sub(RECENT_METHODS)..]
                .iter()
                .map(|request| method_label(method(request)))
                .collect();
        }
        Err(TryLockError::WouldBlock) => snapshot.recorder = RecorderState::Busy,
        Err(TryLockError::Poisoned(_)) => snapshot.recorder = RecorderState::Poisoned,
    }
    snapshot
}

fn method_label(method: &str) -> &'static str {
    match method {
        "initialize" => "initialize",
        "thread/read" => "thread/read",
        "config/read" => "config/read",
        "thread/resume" => "thread/resume",
        "thread/start" => "thread/start",
        "thread/fork" => "thread/fork",
        "thread/list" => "thread/list",
        "server/request/response" => "server/request/response",
        "server/request/error" => "server/request/error",
        _ => "other",
    }
}

impl fmt::Display for RegistrationSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "non-atomic snapshot after timeout selection: creation_finished={}, proxy_finished={}, events_closed={}, queued_events={}, recorder={}",
            self.creation_finished,
            self.proxy_finished,
            self.events_closed,
            self.queued_events,
            match self.recorder {
                RecorderState::Readable => "readable",
                RecorderState::Busy => "busy",
                RecorderState::Poisoned => "poisoned",
            },
        )?;
        if let Some(count) = self.recorded_count {
            write!(f, ", recorded_count={count}, recent_methods=[")?;
            for (index, method) in self.recent_methods.iter().enumerate() {
                if index > 0 {
                    f.write_str(", ")?;
                }
                f.write_str(method)?;
            }
            f.write_str("]")
        } else {
            f.write_str(", recorded_count=unavailable, recent_methods=unavailable")
        }
    }
}

/// Keep the upstream timeout arm a single hook while retaining its failure.
/// Each handle/recorder expression is evaluated once, by shared borrow.
#[macro_export]
macro_rules! mcp_registration_timeout {
    ($creation:expr, $proxy:expr, $events:expr, $requests:expr) => {{
        let snapshot = $crate::test_diagnostics::registration(
            &$creation,
            &$proxy,
            &$events,
            &$requests,
            |request| request.method.as_str(),
        );
        panic!("timed out waiting for MCP task registration: {snapshot}");
    }};
}

#[cfg(test)]
#[path = "test_diagnostics_tests.rs"]
mod tests;
