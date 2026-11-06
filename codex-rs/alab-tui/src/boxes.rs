// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The box hooks (`ALAB(answer)`, CONTRACT §5 rows "question overlay", "plan box" and "goal
//! box"): what
//! the glue inside codex-tui calls while a box is the TUI's active view.
//!
//! - `glue/question_answer.rs` (a child of `bottom_pane::request_user_input`, ticked from the
//!   overlay's own `pre_draw_tick`) reports the `request_user_input` overlay and applies an
//!   answer through the overlay's own selection and submit: the answer leaves on the TUI's
//!   own connection exactly as a person's Enter sends it, the history cell is written and
//!   the overlay closes.
//! - `glue/plan_answer.rs` (a child of `bottom_pane::list_selection_view`, ticked from the
//!   view's `pre_draw_tick`) does the same for "Implement this plan?" and for "Resume paused
//!   goal?" (the goal box).
//! - `glue/session_observe.rs` hands over the frame request ([`set_waker`]) and ends each
//!   frame ([`end_frame`]), which closes a box no glue reported.
//!
//! Every hook is inert unless [`codex_alab_core::active`], save that a test may name its own
//! shared state for the current thread ([`scoped`]); upstream's tests do neither.

pub use codex_alab_core::boxes::BoxAnswer;
pub use codex_alab_core::boxes::BoxKind;
pub use codex_alab_core::boxes::BoxOption;
pub use codex_alab_core::boxes::BoxOutcome;
pub use codex_alab_core::boxes::BoxQuestion;
pub use codex_alab_core::boxes::Choice;
pub use codex_alab_core::boxes::GoalStop;
pub use codex_alab_core::boxes::OpenBox;
pub use codex_alab_core::session::Shared;
pub use codex_alab_core::session::now_ms;
use codex_alab_core::session::shared;
use std::cell::Cell;

/// "Implement this plan?" rows, as `tui/src/chatwidget/plan_implementation.rs` builds them
/// (the chat glue's test pins these to that file's own constants).
pub const PLAN_YES: &str = "Yes, implement this plan";
pub const PLAN_CLEAR_CONTEXT: &str = "Yes, clear context and implement";
pub const PLAN_NO: &str = "No, stay in Plan mode";
/// The user message the plan box's yes submits.
pub const PLAN_MESSAGE: &str = "Implement the plan.";
/// "Resume paused goal?" rows, as `tui/src/chatwidget/goal_menu.rs` builds them (the chat
/// glue's test pins these to that file's own view).
pub const GOAL_RESUME: &str = "Resume goal";
pub const GOAL_LEAVE: &str = "Leave paused";
/// The history line the goal box's `Leave paused` adds, and its hint.
pub const GOAL_LEFT: &str =
    "Goal left as it was: codex-alab answered the prompt under aterm's Autopilot";
pub const GOAL_LEFT_HINT: &str = "/goal resume continues it";

thread_local! {
    static SCOPED: Cell<Option<&'static Shared>> = const { Cell::new(None) };
}

/// The shared state a box glue acts on: a test's own (for this thread), else the process's
/// in the codex binary, else none (the glue does nothing).
pub fn tick_shared() -> Option<&'static Shared> {
    SCOPED
        .with(Cell::get)
        .or_else(|| codex_alab_core::active().then(shared))
}

/// Restores the previous scoped state when dropped.
pub struct Scope(Option<&'static Shared>);

impl Drop for Scope {
    fn drop(&mut self) {
        SCOPED.with(|cell| cell.set(self.0));
    }
}

/// For a test: the box glue on THIS thread acts on `state`, ALab active or not.
pub fn scoped(state: &'static Shared) -> Scope {
    Scope(SCOPED.with(|cell| cell.replace(Some(state))))
}

/// Whether a selection view's rows are the plan box's.
pub fn is_plan_box(labels: &[&str]) -> bool {
    labels == [PLAN_YES, PLAN_CLEAR_CONTEXT, PLAN_NO]
}

/// Whether a selection view's rows are the goal box's.
pub fn is_goal_box(labels: &[&str]) -> bool {
    labels == [GOAL_RESUME, GOAL_LEAVE]
}

thread_local! {
    static ANSWERED: Cell<Option<usize>> = const { Cell::new(None) };
}

/// The glue's box `id`, the view at `view` (its place in memory), took codex-alab's answer:
/// the box is shut, and if the view completed, the view stack's tick that follows
/// (`bottom_pane::tick_active_view`, [`take_answered`]) pops only that view, as a person's
/// Enter does, never the views under it.
pub fn close_answered(state: &Shared, id: &str, view: usize) {
    state.close_box(id);
    ANSWERED.with(|cell| cell.set(Some(view)));
}

/// ALAB(answer) hook in `bottom_pane::tick_active_view`, each tick, for the active view at
/// `view`: whether codex-alab's answer was what it took this tick ([`close_answered`]). Read
/// once. Upstream clears the whole view stack when a view completes in its tick (its own timed
/// completions); an answer must leave a box under it standing, as a person's Enter does
/// (review of 78440aa555, finding 1: an approval under "Resume paused goal?" went with it,
/// unanswered).
pub fn take_answered(view: usize) -> bool {
    ANSWERED.with(Cell::take) == Some(view)
}

/// ALAB(answer) glue, each frame after the views ticked (`session_observe.rs`): a box no
/// glue reported this frame is closed, with any answer pending for it.
pub fn end_frame() {
    if let Some(state) = tick_shared() {
        state.end_frame_boxes();
    }
}

/// Whether the box glue still has to hand over the frame request ([`set_waker`]).
pub fn needs_waker() -> bool {
    tick_shared().is_some_and(|state| !state.has_waker())
}

/// ALAB(answer) glue: the TUI's frame request, so a posted answer is applied without waiting
/// for a key. Set once.
pub fn set_waker(waker: impl Fn() + Send + Sync + 'static) {
    if let Some(state) = tick_shared()
        && !state.has_waker()
    {
        let _ = state.set_waker(Box::new(waker));
    }
}
