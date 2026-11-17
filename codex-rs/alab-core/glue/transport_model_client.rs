// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Compiled inside core's model client so upstream needs only hook lines.
//!
//! Two things live here. The first is the turn's routing state: the `x-codex-turn-state`
//! token the turn's first response issued (a header over HTTPS, client metadata over
//! WebSocket) and, over WebSocket, the open socket with its incremental continuation. A
//! request the backend keeps refusing over HTTPS without naming a fault is replayed
//! without them, which is what the next turn would send and what upstream sends after an
//! auth ownership change.
//!
//! The second is the way back from an HTTPS fallback, whose policy is
//! `codex_alab_core::https_fallback`: upstream sets `disable_websockets` and never clears
//! it, and this clears it at the start of a request once the policy says so. That request
//! is a trial; if it fails, its caller orders the fallback again at once.

use codex_alab_core::https_fallback::Return;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::Ordering;
use std::time::Instant;

impl super::ModelClientSession {
    /// Whether this turn's next request replays a routing token the server issued.
    pub(crate) fn alab_replays_turn_state(&self) -> bool {
        self.turn_state.get().is_some()
    }

    /// Whether this turn's next request goes over the Responses WebSocket.
    pub(crate) fn alab_uses_websocket(&self) -> bool {
        self.client.responses_websocket_enabled()
    }

    /// The next request carries no routing token and opens its own socket. The transport
    /// is not changed: WebSocket stays enabled where it was.
    pub(crate) fn alab_discard_routing_state(&mut self) {
        // A stream already open keeps the old cell; only later requests read the new one.
        // A background compaction started earlier in the turn took the token by value
        // and may still send it once.
        self.turn_state = Arc::new(OnceLock::new());
        self.websocket_session.reset(Some("other"));
    }
}

impl super::ModelClient {
    /// Hook in `force_http_fallback`, when it moved the session to HTTPS.
    pub(super) fn alab_fell_back(&self) {
        if !codex_alab_core::active() {
            return;
        }
        let (websocket_return, after_secs) =
            match self.state.alab_https_fallback.fell_back(Instant::now()) {
                Return::After(interval) => ("scheduled", Some(interval.as_secs())),
                Return::ServerRefused => ("server_refused", None),
                Return::Exhausted => ("exhausted", None),
            };
        tracing::info!(
            websocket_return,
            after_secs,
            "Responses requests moved to HTTPS"
        );
    }
}

impl super::ModelClientSession {
    /// A request failed and its caller is deciding what follows. `unwell` is false for an
    /// answer the server gave, which does not postpone a return to the WebSocket.
    pub(crate) fn alab_request_failed(&self, unwell: bool) {
        if codex_alab_core::active() {
            let fallback = &self.client.state.alab_https_fallback;
            fallback.request_failed(Instant::now(), unwell);
        }
    }

    /// Whether the request that just failed was the one that returned to the WebSocket.
    /// Its caller moves the session back to HTTPS instead of retrying on the socket.
    pub(crate) fn alab_returned_socket_on_trial(&self) -> bool {
        codex_alab_core::active()
            && self.client.responses_websocket_enabled()
            && self.client.state.alab_https_fallback.on_trial()
    }

    /// Hook in `stream`, before it chooses a transport: end an HTTPS fallback that is due.
    /// The request then sends what a turn's first request sends: its full input on its
    /// own socket, with no routing token from the other transport.
    pub(super) fn alab_begin_request(&mut self) {
        if !codex_alab_core::active() {
            return;
        }
        let state = &self.client.state;
        let Some(on_https) = state.alab_https_fallback.begin_request(Instant::now()) else {
            // A socket another session's fallback left this one holding is not resumed.
            if self.websocket_session.connection.is_some()
                && !self.client.responses_websocket_enabled()
            {
                self.websocket_session.reset(Some("other"));
            }
            return;
        };
        state.disable_websockets.store(false, Ordering::Relaxed);
        let turn_state = if self.alab_replays_turn_state() {
            "dropped"
        } else {
            "absent"
        };
        self.alab_discard_routing_state();
        tracing::info!(
            on_https_secs = on_https.as_secs(),
            turn_state,
            "Responses requests return to the WebSocket after HTTPS fallback"
        );
    }
}
