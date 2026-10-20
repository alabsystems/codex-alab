// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The boxes the TUI shows that codex-alab answers (CONTRACT §5 rows "question overlay" and
//! "plan box"), as plain data between the TUI's glue and the autopilot.
//!
//! The TUI's glue (`codex-rs/alab-tui/glue/`) reports the box that is the TUI's ACTIVE view
//! on each frame it draws ([`crate::session::Shared::report_box`]); a frame that drew none
//! closes it ([`crate::session::Shared::end_frame_boxes`]). The autopilot decides off the
//! frame path and posts a [`BoxAnswer`] for that box's id, then wakes the TUI; the glue
//! applies it on the next frame through the box's own selection and submit, and only to
//! the very box it was decided for, untouched since ([`BoxOutcome`]).

/// One option of a question.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoxOption {
    pub label: String,
    pub description: String,
}

/// One question of a `request_user_input` box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoxQuestion {
    pub id: String,
    pub header: String,
    pub question: String,
    /// Empty: a free-text question.
    pub options: Vec<BoxOption>,
    /// The TUI adds a "None of the above" row.
    pub is_other: bool,
    /// Its answer is a secret (never codex-alab's to give).
    pub is_secret: bool,
}

/// What the box is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoxKind {
    /// The `request_user_input` overlay (`item/tool/requestUserInput`).
    Question {
        thread: String,
        questions: Vec<BoxQuestion>,
        /// Plan mode's blocking request (it never auto-resolves on its own).
        blocking: bool,
        /// A person has begun answering it, and how: theirs to finish.
        begun: Option<String>,
    },
    /// "Implement this plan?": each row's label and why it is disabled, if it is.
    Plan { rows: Vec<(String, Option<String>)> },
    /// An approval box (a command or an edit) the approval contributor reviewed and would
    /// approve once, but not at the moment it was asked ([`Preapproval`]).
    Approval {
        thread: String,
        /// The box's own id (its approval id, else its call id).
        key: String,
        /// What it approves, in words ("exec_command").
        what: String,
    },
}

impl BoxKind {
    /// The word an attention reason or a journal line names the box by.
    pub fn word(&self) -> &'static str {
        match self {
            BoxKind::Question { .. } => "question",
            BoxKind::Plan { .. } => "plan",
            BoxKind::Approval { .. } => "approval",
        }
    }
}

/// The box the TUI shows now, as its glue reported it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenBox {
    /// Stable while the box stays open (a question: its thread and call id).
    pub id: String,
    pub kind: BoxKind,
    /// When this id was first reported (ms since the epoch).
    pub opened_ms: u64,
}

/// One question's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    /// The option at this index (0-based, as the box lists them).
    Option(usize),
    /// No option: this text in the question's notes (free text, or a policy text).
    Note(String),
}

/// An answer for one box, by its id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoxAnswer {
    /// Every question of the box, by question id.
    Question {
        id: String,
        answers: Vec<(String, Choice)>,
    },
    /// The plan box's row at this index.
    Plan { id: String, row: usize },
    /// The approval box's one-shot yes ("Yes, proceed"), never a standing one.
    Approval { id: String },
}

impl BoxAnswer {
    pub fn id(&self) -> &str {
        match self {
            BoxAnswer::Question { id, .. }
            | BoxAnswer::Plan { id, .. }
            | BoxAnswer::Approval { id } => id,
        }
    }
}

/// An approval the contributor reviewed and cleared (`approve=all`, none of the owner's
/// limits) while the before-every-act checks said "not now" (the grace after a claim, a
/// person's recent key): the box goes to the TUI, and the autopilot may answer it there
/// once the checks pass, as aterm's retired Codex lane pressed "Yes" once after the grace.
/// Keyed by the box's ids (`tool_call_id`, `approval_id`): the TUI names a box by its approval
/// id, else its call id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preapproval {
    pub keys: Vec<String>,
    pub what: String,
}

/// What the glue did with an answer: `Ok` when the box took it and closed, `Err(why)` when
/// it refused (the box changed, a person began, a row is disabled).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoxOutcome {
    pub id: String,
    pub applied: Result<(), String>,
}

/// The box slot inside [`crate::session::Shared`].
#[derive(Debug, Default)]
pub(crate) struct BoxSlot {
    pub(crate) open: Option<OpenBox>,
    /// The glue reported `open` since the last frame end.
    pub(crate) seen: bool,
    pub(crate) pending: Option<BoxAnswer>,
    /// When `pending` was posted (ms since the epoch).
    pub(crate) pending_ms: u64,
    pub(crate) outcome: Option<BoxOutcome>,
}
