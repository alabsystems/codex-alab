// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! codex-alab's client for aterm's control socket.
//!
//! codex-alab speaks aterm's existing control-socket wire (the protocol `aterm ctl`
//! speaks) from the unsandboxed TUI process itself, on ONE close-on-exec connection it
//! owns; the model's tools never get it (CONTRACT §0). A second connection carries the
//! `subscribe` push stream, because a connection that turns into a stream stops serving
//! requests and aterm then clears the claim bound to it ([`events`]).
//!
//! Nothing here acts on its own: [`Client::connect_self`] answers `None` unless this is
//! the codex-alab binary ([`codex_alab_core::active`]) inside an aterm session, and a
//! claim is sent only after `harness policy` answered `OK effective=on`, so an older
//! aterm (no such verb) is never claimed. Every call is bounded by [`CALL_TIMEOUT`]; a
//! timeout is [`Error::Unanswered`] and closes the connection (which releases the claim).
//!
//! - [`locate`]: `$ATERM_PARENT_SESSION_ID`, `<dir>/graph/<sid>`, the token file.
//! - [`conn`]: `AUTH` + `version`, request/response framing.
//! - [`Client`]: `status`, `harness policy`, the claim, attention, `harness_act`,
//!   `harness_held`, `harness_land`, `harness_needs`, `harness fact` and its ack ([`facts`]),
//!   `story`, `appnotice`, `notice later` ([`Later`]), the session's `description`.
//! - [`events`]: `subscribe @self events` and the claim watch.
//! - [`context`]: `/ide`'s terminal context (`panes`, `blocks`, `blocktext`, `selection`).
//! - [`mail`]: this session's inbox for a typed `Inbox:` line, its receipt and report.
//! - `fake` (tests, or the `test-support` feature): an in-process aterm.
//!
//! Unix only: aterm's control socket is AF_UNIX, and on any other target this crate is
//! empty (there, codex-alab never finds an aterm).
#![cfg(unix)]

pub mod account_recovery;
pub mod accounts;
mod client;
pub mod conn;
pub mod context;
pub mod enrollment;
pub mod events;
pub mod exec_capsule;
pub mod exec_grant;
mod exec_selection;
pub mod facts;
pub mod locate;
pub mod mail;
pub mod managed_auth;
pub mod native_source;
pub mod policy;
pub mod renewal;
pub mod rotation;
pub mod status;
pub mod wire;

#[cfg(any(test, debug_assertions, feature = "test-support"))]
pub mod fake;

pub use client::ATTENTION_OWNER;
pub use client::ActOutcome;
pub use client::CALL_TIMEOUT;
pub use client::ClaimOutcome;
pub use client::Client;
pub use client::DESCRIPTION_MAX_BYTES;
pub use client::EndOutcome;
pub use client::HELD_ARMS;
pub use client::HeldOutcome;
pub use client::LAND_OPEN_WORDS;
pub use client::LATER_DETAIL_LINE_MAX_CHARS;
pub use client::LATER_DETAIL_LINES;
pub use client::LATER_FIELD_MAX_CHARS;
pub use client::LATER_KEY_MAX;
pub use client::LandLeg;
pub use client::LandOutcome;
pub use client::LandRead;
pub use client::LandReport;
pub use client::Later;
pub use client::LaterHow;
pub use client::LaterOutcome;
pub use client::NEED_TOLD_MAX_BYTES;
pub use client::NEEDS_OWNER;
pub use client::NEEDS_TOLD;
pub use client::NOTICE_MAX_BYTES;
pub use client::NeedsOutcome;
pub use client::ObserveLaterOutcome;
pub use client::ObservedLater;
pub use client::Options;
pub use client::STORY_MAX_BYTES;
pub use client::STORY_VERBS;
pub use client::WAIT_OWNER;
pub use client::holder_for;
pub use client::prepare_unknown;
pub use events::Event;
pub use events::Events;
pub use events::Signal;
pub use events::claim_signal;
pub use facts::Fact;
pub use facts::FactAnswer;
pub use facts::ToldOutcome;
pub use locate::Endpoint;
pub use policy::HarnessPolicy;
pub use policy::PolicyAnswer;
pub use status::Status;
pub use status::Whoami;

/// Why a call failed. Every one of them means: do nothing, stay passive.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The socket could not be reached or read.
    #[error("aterm socket: {0}")]
    Io(#[from] std::io::Error),
    /// No reply within the deadline; the connection was closed.
    #[error("aterm did not answer in time")]
    Unanswered,
    /// The connection is closed.
    #[error("aterm connection closed")]
    Closed,
    /// aterm is busy (switching versions, every lane taken): wait `retry_ms`, then retry.
    #[error("aterm busy: {line}")]
    Busy {
        /// The refusal as sent.
        line: String,
        /// Its `retry_ms=`, if any.
        retry_ms: Option<u64>,
    },
    /// An `ERR` reply (or `ERR auth` at connect).
    #[error("aterm refused: {0}")]
    Refused(String),
    /// A reply this client cannot read.
    #[error("unreadable aterm reply: {0}")]
    Malformed(String),
    /// A request this client will not send.
    #[error("bad request: {0}")]
    BadRequest(String),
}

#[cfg(test)]
mod tests;
