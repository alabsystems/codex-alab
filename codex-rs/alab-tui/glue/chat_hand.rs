// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(session) glue, compiled inside codex-tui as `chatwidget::alab_hand` (one `#[path]`
//! line at the end of `tui/src/chatwidget.rs`): the person's input the TUI still holds lives
//! in `ChatWidget`'s private input queue, and this turn's plan in its private transcript
//! state, which only a child of `chatwidget` can read. The session observe glue
//! (`session_observe.rs`) asks these questions each frame.
//!
//! The OSC 7501 report's two hooks live here too (`ALAB(turns)`, `codex_alab_tui::turns`):
//! one in `refresh_terminal_program_status` (each frame's report, MCP startup left out) and
//! one in the live `TurnStarted` arm of `chatwidget/protocol.rs` (a turn's rest-to-turn edge),
//! both reading state only a child of `chatwidget` can.

use super::ChatWidget;
use crate::terminal_program_status::ProgramStatus;
use crate::terminal_program_status::ProgramStatusKind;
use codex_alab_tui::session::BlockedKind;
use codex_alab_tui::session::ProgramState;
use codex_alab_tui::turns;

#[cfg(test)]
#[path = "goal_interrupt_tests.rs"]
mod goal_interrupt_tests;

#[cfg(test)]
#[path = "daybreak_recovery_tests.rs"]
mod daybreak_recovery_tests;
#[cfg(test)]
#[path = "turns_session_tests.rs"]
mod turns_session_tests;

impl ChatWidget {
    /// An active claimed root has no person available to settle a headless prompt.
    pub(crate) fn alab_autonomous_root_active(&self) -> bool {
        codex_alab_tui::boxes::tick_shared().is_some_and(|shared| {
            shared.autopilot() == codex_alab_tui::session::Autopilot::Active
                && shared.claim().is_some()
                && self
                    .thread_id
                    .is_some_and(|id| shared.is_root_thread(&id.to_string()))
        })
    }

    pub(crate) fn alab_autonomous_safety_retry_allowed(
        &self,
        turn: &crate::app_command::AppCommand,
    ) -> bool {
        self.alab_autonomous_root_active()
            && matches!(
                turn,
                crate::app_command::AppCommand::UserTurn { alab_safety_retries, .. }
                if *alab_safety_retries < 2
            )
    }

    pub(super) fn alab_safety_buffering_matches_thread(&self, thread_id: &str) -> bool {
        self.thread_id.is_some_and(|id| id.to_string() == thread_id)
    }

    /// Queued messages, steers waiting for the running turn, rejected steers, or the startup
    /// draft's submission: a person's input the TUI will still send.
    pub(crate) fn alab_queued_input(&self) -> bool {
        !self.input_queue.pending_steers.is_empty()
            || self.input_queue.has_queued_follow_up_messages()
            || self.input_queue.startup_submission.is_some()
    }

    /// The TUI's frame request (the box glue's waker).
    pub(crate) fn alab_frame_requester(&self) -> crate::tui::FrameRequester {
        self.frame_requester.clone()
    }

    /// A user turn submitted and not yet started (images still being prepared included).
    pub(crate) fn alab_pending_user_turn(&self) -> bool {
        self.input_queue.user_turn_pending_start || self.pending_image_submission.is_some()
    }

    /// The displayed thread's state, as upstream's OSC 7501 report says it: MCP startup is
    /// `working` here (the session record and the autopilot wait it out), where the report
    /// codex-alab writes inside aterm leaves it out ([`Self::alab_report_frame`]).
    pub(crate) fn alab_program_state(&self) -> ProgramState {
        program_state_of(self.alab_work_status(self.desired_program_status()))
    }

    // aterm's session/autopilot contract includes owned background terminals.
    // Upstream's foreground title may show Done while that work remains alive.
    fn alab_work_status(&self, status: ProgramStatus) -> ProgramStatus {
        if !matches!(status, ProgramStatus::Blocked(_)) && !self.unified_exec_processes.is_empty() {
            ProgramStatus::Working
        } else {
            status
        }
    }

    /// ALAB(session): the displayed thread's active goal objective when Goals are enabled.
    pub(crate) fn alab_goal(&self) -> Option<&str> {
        self.config
            .features
            .enabled(crate::Feature::Goals)
            .then(|| {
                self.current_goal_status
                    .as_ref()
                    .and_then(super::GoalStatusState::active_objective)
            })
            .flatten()
    }

    /// ALAB(turns): this frame's facts for the OSC 7501 report: upstream's state, whether it
    /// says `working` only because MCP servers start (no turn running or submitted, no review,
    /// no image being readied, no background terminal, no active goal between turns: every
    /// other reason `desired_program_status` has), where the thread stands at rest, whether a
    /// submitted turn waits for the server, and whether a turn of a thread this widget does not
    /// show runs (a thread switch away from it and back begins none).
    pub(crate) fn alab_report_frame(&self, status: ProgramStatus) -> turns::Frame {
        let status = self.alab_work_status(status);
        let goal_between_turns = self.status_state.program_status_after_turn == ProgramStatus::Done
            && self
                .current_goal_status
                .as_ref()
                .is_some_and(super::GoalStatusState::is_active);
        turns::Frame {
            state: program_state_of(status),
            startup_only: status == ProgramStatus::Working
                && self.mcp_startup_status.is_some()
                && !self.is_user_turn_pending_or_running()
                && self.unified_exec_processes.is_empty()
                && !goal_between_turns,
            rest: program_state_of(self.status_state.program_status_after_turn),
            submitted: self.alab_pending_user_turn(),
            elsewhere: turns::running_elsewhere(self.thread_id.map(|id| id.to_string()).as_deref()),
            held: turns::seen_elsewhere(self.thread_id.map(|id| id.to_string()).as_deref()),
        }
    }

    /// ALAB(turns) hook, first thing in `refresh_terminal_program_status` (each frame, and the
    /// overlay's draw): where codex-alab writes the report (its binary, inside aterm), this
    /// frame's report goes through upstream's writer with MCP startup left out, and upstream's
    /// own write is skipped. Whether it was.
    pub(crate) fn alab_report_program_status(&self, status: ProgramStatus) -> bool {
        let frame = self.alab_report_frame(status);
        let Some(result) = turns::frame(frame, |words| self.alab_write_program_states(words))
        else {
            return false;
        };
        if let Err(err) = result {
            tracing::debug!(error = %err, "failed to set terminal program status");
        }
        true
    }

    /// ALAB(turns) hook in the live `TurnStarted` arm (`chatwidget/protocol.rs`), before
    /// `on_task_started` clears the submission that started it: the turn's rest-to-turn edge,
    /// where codex-alab writes the report. The TUI submitted the turn when
    /// `user_turn_pending_start` stands (a person's prompt, a driver's typed turn, a queued
    /// message, `/compact`, a review, a shell command); a turn it did not (a goal's
    /// continuation, an autopilot act through the app-server) begins none under standing work.
    pub(crate) fn alab_turn_starting(&self) {
        let asked = self.input_queue.user_turn_pending_start;
        if let Some(Err(err)) =
            turns::starting(asked, |words| self.alab_write_program_states(words))
        {
            tracing::debug!(error = %err, "failed to set terminal program status");
        }
    }

    /// ALAB(turns) hook in the `TurnCompleted` arm (`chatwidget/protocol.rs`): this widget took
    /// the end of `turn` of `thread`, live or replayed, after its `turn/started` and any edge
    /// that wrote; booked so codex-alab records a landing on it only after them
    /// (`Shared::turn_handled_end`).
    pub(crate) fn alab_turn_ended(&self, thread: &str, turn: &str) {
        turns::handled_end(thread, turn);
    }

    /// ALAB(turns): pass the report's whole edge to upstream's writer/cache owner. It takes
    /// the cache before stdout, as a report or panic-hook clear does, and holds the writer
    /// across the prefix, final word and flush. Where upstream writes nothing (a test build,
    /// stdout no terminal), only the last word goes to it.
    fn alab_write_program_states(&self, words: &[ProgramState]) -> std::io::Result<()> {
        use std::io::IsTerminal;
        let Some((&last, before)) = words.split_last() else {
            return Ok(());
        };
        if before.is_empty() || cfg!(test) || !std::io::stdout().is_terminal() {
            return self.alab_write_program_state(last);
        }
        let rest: String = before
            .iter()
            .filter_map(|word| turns::sequence(*word))
            .collect();
        let Some(status) = program_status_of(last) else {
            return Ok(());
        };
        crate::terminal_program_status::set_terminal_program_status_with_prefix(
            status,
            self.iterm_session_detail(status),
            &rest,
        )
    }

    /// One report word through upstream's writer (its cache, its OSC 21337 detail in iTerm2).
    fn alab_write_program_state(&self, state: ProgramState) -> std::io::Result<()> {
        let Some(status) = program_status_of(state) else {
            return Ok(());
        };
        crate::terminal_program_status::set_terminal_program_status(
            status,
            self.iterm_session_detail(status),
        )
    }

    /// This turn's `update_plan` checklist as (completed, total) steps, the counts the title's
    /// `Tasks n/m` shows (`ALAB(progress)`). Only while the turn that sent it runs, or shows
    /// failed: a turn's flags reset only when the next one starts, so after the turn ends (a
    /// background terminal or an active goal keeps the thread working) its plan is no
    /// progress. `None` until this turn sends a plan: one an earlier turn left (the title keeps
    /// it) is not this turn's progress, nor is the last turn's while a new one is submitted and
    /// has not started.
    pub(crate) fn alab_plan_progress(&self) -> Option<(usize, usize)> {
        let its_turn = self.turn_lifecycle.agent_turn_running
            || self.alab_program_state() == ProgramState::Error;
        if !its_turn || !self.transcript.saw_plan_update_this_turn || self.alab_pending_user_turn()
        {
            return None;
        }
        self.transcript.last_plan_progress
    }
}

/// Upstream's status word as codex-alab's.
fn program_state_of(status: ProgramStatus) -> ProgramState {
    match status {
        ProgramStatus::Idle => ProgramState::Idle,
        ProgramStatus::Working => ProgramState::Working,
        ProgramStatus::Blocked(kind) => ProgramState::Blocked(match kind {
            ProgramStatusKind::Permission => BlockedKind::Permission,
            ProgramStatusKind::Question => BlockedKind::Question,
            ProgramStatusKind::Auth => BlockedKind::Auth,
        }),
        ProgramStatus::Done => ProgramState::Done,
        ProgramStatus::Error => ProgramState::Error,
    }
}

/// codex-alab's state word as upstream's; `None` for `Unknown`, which is never written.
fn program_status_of(state: ProgramState) -> Option<ProgramStatus> {
    Some(match state {
        ProgramState::Unknown => return None,
        ProgramState::Idle => ProgramStatus::Idle,
        ProgramState::Working => ProgramStatus::Working,
        ProgramState::Blocked(kind) => ProgramStatus::Blocked(match kind {
            BlockedKind::Permission => ProgramStatusKind::Permission,
            BlockedKind::Question => ProgramStatusKind::Question,
            BlockedKind::Auth => ProgramStatusKind::Auth,
        }),
        ProgramState::Done => ProgramStatus::Done,
        ProgramState::Error => ProgramStatus::Error,
    })
}

/// The box glue (`ALAB(answer)`) through the chat widget's real view stack: the hooks in the
/// overlay's and the selection view's `pre_draw_tick` are reached by the widget's own
/// `pre_draw_tick`, the answer leaves as a person's would, and the box closes.
#[cfg(test)]
mod tests {
    use super::super::plan_implementation;
    use crate::app_command::AppCommand;
    use crate::app_event::AppEvent;
    use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
    use crate::terminal_program_status::ProgramStatus;
    use crate::terminal_program_status::ProgramStatusKind;
    use codex_alab_tui::boxes;
    use codex_alab_tui::boxes::BoxAnswer;
    use codex_alab_tui::boxes::BoxKind;
    use codex_alab_tui::boxes::Choice;
    use codex_alab_tui::boxes::GoalStop;
    use codex_alab_tui::boxes::Shared;
    use codex_alab_tui::progress::Bar;
    use codex_alab_tui::session::ProgramState;
    use codex_app_server_protocol::ServerNotification;
    use codex_app_server_protocol::ToolRequestUserInputOption;
    use codex_app_server_protocol::ToolRequestUserInputParams;
    use codex_app_server_protocol::ToolRequestUserInputQuestion;
    use codex_protocol::config_types::ModeKind;
    use pretty_assertions::assert_eq;

    fn state() -> &'static Shared {
        Box::leak(Box::new(Shared::new()))
    }

    #[test]
    fn the_plan_rows_and_message_are_the_plan_boxs_own() {
        let params = plan_implementation::selection_view_params(None, None, None);
        let names: Vec<&str> = params.items.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(
            names,
            vec![boxes::PLAN_YES, boxes::PLAN_CLEAR_CONTEXT, boxes::PLAN_NO]
        );
        assert!(boxes::is_plan_box(&names));
        assert_eq!(
            boxes::PLAN_MESSAGE,
            plan_implementation::PLAN_IMPLEMENTATION_CODING_MESSAGE
        );
    }

    #[tokio::test]
    async fn the_plan_boxs_yes_is_accepted_through_the_view_stack_and_never_clear_context() {
        let shared = state();
        let _scope = boxes::scoped(shared);
        let (mut chat, _tx, mut rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        chat.open_plan_implementation_prompt();
        while rx.try_recv().is_ok() {}
        chat.pre_draw_tick();
        let open = shared
            .open_box()
            .unwrap_or_else(|| panic!("the plan box is reported"));
        let BoxKind::Plan { rows } = &open.kind else {
            panic!("a plan box: {open:?}");
        };
        assert_eq!(rows[0], (boxes::PLAN_YES.to_owned(), None));

        assert!(shared.post_box_answer(BoxAnswer::Plan {
            id: open.id.clone(),
            row: 1,
        }));
        chat.pre_draw_tick();
        let refused = shared.take_box_outcome().map(|o| o.applied);
        assert!(matches!(refused, Some(Err(_))), "{refused:?}");
        assert!(chat.has_active_view(), "clear context is never chosen");
        assert!(rx.try_recv().is_err());

        assert!(shared.post_box_answer(BoxAnswer::Plan {
            id: open.id.clone(),
            row: 0,
        }));
        chat.pre_draw_tick();
        assert_eq!(shared.take_box_outcome().map(|o| o.applied), Some(Ok(())));
        assert!(!chat.has_active_view(), "the plan box closed");
        assert!(matches!(rx.try_recv(), Ok(AppEvent::FollowTranscript)));
        let event = rx.try_recv().unwrap_or_else(|_| panic!("the yes submits"));
        let AppEvent::SubmitUserMessageWithMode {
            text,
            collaboration_mode,
        } = event
        else {
            panic!("expected SubmitUserMessageWithMode, got {event:?}");
        };
        assert_eq!(
            text,
            plan_implementation::PLAN_IMPLEMENTATION_CODING_MESSAGE
        );
        assert_eq!(collaboration_mode.mode, Some(ModeKind::Default));
        assert!(
            shared.take_own_turn(boxes::PLAN_MESSAGE),
            "the turn the yes submits is codex-alab's own"
        );
        assert_eq!(shared.open_box(), None, "an accepted box is shut at once");
    }

    /// "Resume paused goal?" (`goal_menu.rs`'s own view, opened as a resume opens it) is the goal
    /// box exactly, so an upstream move of its rows stops the report here; it carries why the
    /// goal stopped (the App's hook says it before the view opens); only `Leave paused` is
    /// taken, which sends nothing and leaves a history line.
    #[tokio::test]
    async fn the_goal_boxs_leave_paused_is_taken_through_the_view_stack_and_never_resume() {
        let shared = state();
        let _scope = boxes::scoped(shared);
        let (mut chat, _tx, mut rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        shared.set_goal_prompt(GoalStop::Paused);
        chat.show_resume_paused_goal_prompt(
            codex_protocol::ThreadId::new(),
            "Finish the paused goal.".to_owned(),
        );
        while rx.try_recv().is_ok() {}
        chat.pre_draw_tick();
        let open = shared
            .open_box()
            .unwrap_or_else(|| panic!("the goal box is reported"));
        let BoxKind::Goal { rows, stop } = &open.kind else {
            panic!("a goal box: {open:?}");
        };
        assert_eq!(
            rows,
            &vec![boxes::GOAL_RESUME.to_owned(), boxes::GOAL_LEAVE.to_owned()]
        );
        assert_eq!(*stop, Some(GoalStop::Paused));

        assert!(shared.post_box_answer(BoxAnswer::Goal {
            id: open.id.clone(),
            row: 0,
        }));
        chat.pre_draw_tick();
        let refused = shared.take_box_outcome().map(|o| o.applied);
        assert!(matches!(refused, Some(Err(_))), "{refused:?}");
        assert!(chat.has_active_view(), "the goal is never resumed");
        assert!(rx.try_recv().is_err());

        assert!(shared.post_box_answer(BoxAnswer::Goal {
            id: open.id.clone(),
            row: 1,
        }));
        chat.pre_draw_tick();
        assert_eq!(shared.take_box_outcome().map(|o| o.applied), Some(Ok(())));
        assert!(!chat.has_active_view(), "the goal box closed");
        let event = rx
            .try_recv()
            .unwrap_or_else(|_| panic!("a history line says so"));
        let AppEvent::InsertHistoryCell(cell) = event else {
            panic!("expected InsertHistoryCell, got {event:?}");
        };
        let text: String = cell
            .display_lines(200)
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.to_string()))
            .collect();
        assert!(text.contains(boxes::GOAL_LEFT), "{text}");
        assert!(text.contains(boxes::GOAL_LEFT_HINT), "{text}");
        assert!(
            rx.try_recv().is_err(),
            "nothing else is sent: the goal stays as it was"
        );
        assert_eq!(shared.open_box(), None, "an accepted box is shut at once");
    }

    /// An answer pops only the box it answered, as a person's Enter does: a box under it (here
    /// the plan box, opened first) stays standing and is reported next as a box of its own.
    /// Upstream clears the whole view stack when a view completes in its own tick.
    #[tokio::test]
    async fn an_answered_box_leaves_the_box_under_it_standing() {
        let shared = state();
        let _scope = boxes::scoped(shared);
        let (mut chat, _tx, mut rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        chat.open_plan_implementation_prompt();
        shared.set_goal_prompt(GoalStop::Paused);
        chat.show_resume_paused_goal_prompt(
            codex_protocol::ThreadId::new(),
            "Finish the paused goal.".to_owned(),
        );
        while rx.try_recv().is_ok() {}
        chat.pre_draw_tick();
        let goal = shared
            .open_box()
            .unwrap_or_else(|| panic!("the goal box on top is reported"));
        assert!(matches!(goal.kind, BoxKind::Goal { .. }), "{goal:?}");
        assert!(shared.post_box_answer(BoxAnswer::Goal {
            id: goal.id.clone(),
            row: 1,
        }));
        chat.pre_draw_tick();
        assert_eq!(shared.take_box_outcome().map(|o| o.applied), Some(Ok(())));
        assert!(chat.has_active_view(), "the plan box under it stands");
        while rx.try_recv().is_ok() {}
        chat.pre_draw_tick();
        let plan = shared
            .open_box()
            .unwrap_or_else(|| panic!("the plan box is reported next"));
        assert!(matches!(plan.kind, BoxKind::Plan { .. }), "{plan:?}");
        // A box of its own: the next number in this state's count, never the goal box's.
        assert_eq!(goal.id, "goal-1");
        assert_eq!(plan.id, "plan-2");
    }

    /// NEGATIVE CONTROL: a selection view with the goal box's rows in another order, or with one
    /// more, is no goal box; it is never reported, so no answer reaches it.
    #[tokio::test]
    async fn a_view_that_is_not_the_goal_box_exactly_is_not_reported() {
        use crate::bottom_pane::SelectionItem;
        use crate::bottom_pane::SelectionViewParams;
        let rows_of = |names: &[&str]| SelectionViewParams {
            title: Some("Resume paused goal?".to_owned()),
            items: names
                .iter()
                .map(|name| SelectionItem {
                    name: (*name).to_owned(),
                    dismiss_on_select: true,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        for names in [
            vec![boxes::GOAL_LEAVE, boxes::GOAL_RESUME],
            vec![boxes::GOAL_RESUME, boxes::GOAL_LEAVE, "Delete goal"],
        ] {
            let shared = state();
            let _scope = boxes::scoped(shared);
            let (mut chat, _tx, mut rx, _op_rx) = make_chatwidget_manual_with_sender().await;
            shared.set_goal_prompt(GoalStop::Paused);
            chat.show_selection_view(rows_of(&names));
            while rx.try_recv().is_ok() {}
            chat.pre_draw_tick();
            assert_eq!(shared.open_box(), None, "{names:?}");
            assert!(chat.has_active_view());
        }
    }

    #[tokio::test]
    async fn a_question_overlay_answered_through_the_view_stack_closes() {
        let shared = state();
        let _scope = boxes::scoped(shared);
        let (mut chat, _tx, mut rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        chat.handle_request_user_input_now(ToolRequestUserInputParams {
            thread_id: "00000000-0000-0000-0000-000000000001".to_owned(),
            turn_id: "turn-7".to_owned(),
            item_id: "call-7".to_owned(),
            questions: vec![ToolRequestUserInputQuestion {
                id: "q".to_owned(),
                header: "Mode".to_owned(),
                question: "Which mode?".to_owned(),
                is_other: false,
                is_secret: false,
                options: Some(vec![
                    ToolRequestUserInputOption {
                        label: "Fast (Recommended)".to_owned(),
                        description: "Quick.".to_owned(),
                    },
                    ToolRequestUserInputOption {
                        label: "Thorough".to_owned(),
                        description: "Slow.".to_owned(),
                    },
                ]),
            }],
            is_blocking: true,
            auto_resolution_ms: None,
        });
        while rx.try_recv().is_ok() {}
        chat.pre_draw_tick();
        assert_eq!(
            chat.desired_program_status(),
            ProgramStatus::Blocked(ProgramStatusKind::Question)
        );
        assert_eq!(shared.open_box().map(|b| b.id), Some("call-7".to_owned()));
        assert!(shared.post_box_answer(BoxAnswer::Question {
            id: "call-7".to_owned(),
            answers: vec![("q".to_owned(), Choice::Option(0))],
        }));
        chat.pre_draw_tick();
        assert!(!chat.has_active_view(), "the overlay closed");
        assert_ne!(
            chat.desired_program_status(),
            ProgramStatus::Blocked(ProgramStatusKind::Question)
        );
        let mut answer = None;
        while let Ok(event) = rx.try_recv() {
            if let AppEvent::SubmitThreadOp {
                op: AppCommand::UserInputAnswer { id, response },
                ..
            } = event
            {
                answer = Some((id, response));
            }
        }
        let (id, response) = answer.unwrap_or_else(|| panic!("the answer is submitted"));
        assert_eq!(id, "turn-7");
        assert_eq!(
            response.answers["q"].answers,
            vec!["Fast (Recommended)".to_owned()]
        );
    }

    /// One turn notification for the widget's thread.
    pub(super) fn turn(
        chat: &mut super::ChatWidget,
        id: &str,
        status: codex_app_server_protocol::TurnStatus,
    ) {
        let thread_id = chat
            .thread_id()
            .map(|id| id.to_string())
            .unwrap_or_default();
        let turn = codex_app_server_protocol::Turn {
            id: id.to_owned(),
            root_turn_id: None,
            items_view: codex_app_server_protocol::TurnItemsView::Full,
            items: Vec::new(),
            status: status.clone(),
            error: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
        };
        let notification = match status {
            codex_app_server_protocol::TurnStatus::InProgress => ServerNotification::TurnStarted(
                codex_app_server_protocol::TurnStartedNotification { thread_id, turn },
            ),
            _ => ServerNotification::TurnCompleted(
                codex_app_server_protocol::TurnCompletedNotification { thread_id, turn },
            ),
        };
        chat.handle_server_notification(notification, /*replay_kind*/ None);
    }

    /// `update_plan` with `done` of `total` steps completed (the next one in progress).
    fn plan(chat: &mut super::ChatWidget, turn_id: &str, done: usize, total: usize) {
        use codex_app_server_protocol::TurnPlanStepStatus;
        let thread_id = chat
            .thread_id()
            .map(|id| id.to_string())
            .unwrap_or_default();
        let plan = (0..total)
            .map(|step| codex_app_server_protocol::TurnPlanStep {
                step: format!("step {step}"),
                status: if step < done {
                    TurnPlanStepStatus::Completed
                } else if step == done {
                    TurnPlanStepStatus::InProgress
                } else {
                    TurnPlanStepStatus::Pending
                },
            })
            .collect();
        chat.handle_server_notification(
            ServerNotification::TurnPlanUpdated(
                codex_app_server_protocol::TurnPlanUpdatedNotification {
                    thread_id,
                    turn_id: turn_id.to_owned(),
                    explanation: None,
                    plan,
                },
            ),
            /*replay_kind*/ None,
        );
    }

    /// ALAB(progress) through the real widget: the bar the frame glue computes from
    /// `alab_program_state` and `alab_plan_progress` as a plan of four steps fills, the turn
    /// ends, a later turn starts without it, and a failed turn stops where its plan stood.
    #[tokio::test]
    async fn a_turns_plan_fills_aterms_bar_and_a_later_turn_starts_without_it() {
        use codex_app_server_protocol::TurnStatus;
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        let bar = |chat: &super::ChatWidget| {
            Bar::of(chat.alab_program_state(), chat.alab_plan_progress())
        };
        assert_eq!(bar(&chat), Bar::Hidden, "at rest");

        turn(&mut chat, "t1", TurnStatus::InProgress);
        assert_eq!(bar(&chat), Bar::Indeterminate, "a turn with no plan yet");
        let mut fills = Vec::new();
        for done in 0..=4 {
            plan(&mut chat, "t1", done, 4);
            fills.push(bar(&chat));
        }
        assert_eq!(
            fills,
            [0, 25, 50, 75, 100].map(Bar::Plan).to_vec(),
            "the plan fills the bar"
        );
        turn(&mut chat, "t1", TurnStatus::Completed);
        assert_eq!(bar(&chat), Bar::Hidden, "the turn's end");

        // The next turn is submitted and has not started: t1's flags still stand, its plan is
        // not this turn's.
        chat.input_queue.user_turn_pending_start = true;
        assert_eq!(chat.alab_program_state(), ProgramState::Working);
        assert_eq!(
            bar(&chat),
            Bar::Indeterminate,
            "a submitted turn has no plan yet"
        );
        turn(&mut chat, "t2", TurnStatus::InProgress);
        // Negative control: upstream's title still holds t1's 4 of 4; the bar does not.
        assert_eq!(chat.transcript.last_plan_progress, Some((4, 4)));
        assert_eq!(chat.alab_plan_progress(), None);
        assert_eq!(bar(&chat), Bar::Indeterminate, "t1's plan is not t2's");
        plan(&mut chat, "t2", 1, 4);
        assert_eq!(bar(&chat), Bar::Plan(25));
        turn(&mut chat, "t2", TurnStatus::Failed);
        assert_eq!(bar(&chat), Bar::Error(25), "red where the plan stood");
        chat.acknowledge_program_status();
        assert_eq!(bar(&chat), Bar::Hidden, "a person's key: rest");
    }

    /// A turn that ends while the thread still works (a background terminal it left running,
    /// an active goal between its turns) leaves no percent standing: its plan was its own.
    #[tokio::test]
    async fn an_ended_turns_plan_does_not_outlive_it_while_the_thread_still_works() {
        use super::super::GoalStatusState;
        use super::super::exec_state::UnifiedExecProcessSummary;
        use codex_app_server_protocol::ThreadGoal;
        use codex_app_server_protocol::ThreadGoalStatus;
        use codex_app_server_protocol::TurnStatus;
        let (mut chat, _tx, _rx, _op_rx) = make_chatwidget_manual_with_sender().await;
        let bar = |chat: &super::ChatWidget| {
            Bar::of(chat.alab_program_state(), chat.alab_plan_progress())
        };

        // `npm run dev` left running in a background terminal past the turn's end.
        turn(&mut chat, "t1", TurnStatus::InProgress);
        plan(&mut chat, "t1", 3, 4);
        assert_eq!(bar(&chat), Bar::Plan(75));
        chat.unified_exec_processes.push(UnifiedExecProcessSummary {
            key: "proc-1".to_owned(),
            call_id: "call-1".to_owned(),
            command_display: "npm run dev".to_owned(),
            recent_chunks: Vec::new(),
        });
        turn(&mut chat, "t1", TurnStatus::Completed);
        // Negative control: the thread still reports working, and t1's flags still stand.
        assert_eq!(chat.alab_program_state(), ProgramState::Working);
        assert!(chat.transcript.saw_plan_update_this_turn);
        assert_eq!(
            bar(&chat),
            Bar::Indeterminate,
            "the terminal works, not t1's plan"
        );
        // A later turn fails with the terminal still running: the thread works, so no red at
        // its plan until the terminal goes and the failure is what the thread shows.
        turn(&mut chat, "t2", TurnStatus::InProgress);
        plan(&mut chat, "t2", 2, 4);
        assert_eq!(bar(&chat), Bar::Plan(50));
        turn(&mut chat, "t2", TurnStatus::Failed);
        assert_eq!(chat.alab_program_state(), ProgramState::Working);
        assert_eq!(bar(&chat), Bar::Indeterminate, "the terminal still works");
        chat.unified_exec_processes.clear();
        assert_eq!(
            bar(&chat),
            Bar::Error(50),
            "the terminal gone: red where t2 stood"
        );
        chat.acknowledge_program_status();
        assert_eq!(bar(&chat), Bar::Hidden, "a person's key: rest");

        // An active goal between its chained turns.
        chat.current_goal_status = Some(GoalStatusState::new(
            ThreadGoal {
                thread_id: chat
                    .thread_id()
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
                objective: "Keep improving the benchmark".to_owned(),
                status: ThreadGoalStatus::Active,
                token_budget: None,
                tokens_used: 0,
                time_used_seconds: 0,
                created_at: 0,
                updated_at: 0,
            },
            std::time::Instant::now(),
        ));
        turn(&mut chat, "t3", TurnStatus::InProgress);
        plan(&mut chat, "t3", 1, 4);
        assert_eq!(bar(&chat), Bar::Plan(25));
        turn(&mut chat, "t3", TurnStatus::Completed);
        assert_eq!(chat.alab_program_state(), ProgramState::Working);
        assert_eq!(bar(&chat), Bar::Indeterminate, "between the goal's turns");
        turn(&mut chat, "t4", TurnStatus::InProgress);
        assert_eq!(bar(&chat), Bar::Indeterminate, "t4 has no plan yet");
        plan(&mut chat, "t4", 2, 4);
        assert_eq!(bar(&chat), Bar::Plan(50));
    }
}
