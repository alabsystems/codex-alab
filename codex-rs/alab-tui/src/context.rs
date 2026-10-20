// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! `/ide` inside aterm: the context comes from aterm, not an IDE.
//!
//! When the person turns IDE context on in a codex-alab session that holds a proven aterm
//! claim, each message they send carries their terminal's context, read from aterm over its
//! control socket by the autopilot's supervisor (`codex_alab_aterm::context`): the selection,
//! this tab's commands from before codex started, and a split pane's last failed command with
//! its output. Nothing is written into `~/.codex` (no IDE socket is faked); outside aterm, or
//! without the claim, `/ide` is upstream's. The hooks are two lines in
//! `tui/src/chatwidget/ide_context.rs`, into `glue/aterm_context.rs`.

use std::time::Duration;

use codex_alab_core::active;
use codex_alab_core::session::shared;

pub use codex_alab_core::session::TerminalContext;

/// How long the submit path waits for aterm (upstream's IDE fetch waits up to 5 s; aterm's
/// reads are local and answer in milliseconds).
pub const BUDGET: Duration = Duration::from_millis(750);

/// Whether `/ide` takes its context from aterm now: the codex-alab binary, inside aterm, with a
/// proven claim held.
pub fn available() -> bool {
    active() && shared().context_available()
}

/// This session's terminal context, waiting at most [`BUDGET`]; `None` when aterm cannot give
/// it now.
pub fn read() -> Option<TerminalContext> {
    if !active() {
        return None;
    }
    shared().terminal_context(BUDGET)
}

/// The info line `/ide on` (and `/ide status`) shows inside aterm.
pub const STATUS: &str = "IDE context is on, from aterm.";
/// Its hint: what each message will carry.
pub const STATUS_HINT: &str = "Each message will include your selection, this tab's commands \
     from before Codex started, and a split pane's last failed command with its output.";

/// The info line for one message: what went to the model.
pub fn attached(context: &TerminalContext) -> String {
    format!("aterm context attached: {}", context.summary)
}

/// The info line when aterm did not answer in time.
pub const SKIPPED: &str = "aterm context was skipped for this message.";
