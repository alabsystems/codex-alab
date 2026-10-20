// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(session) glue, compiled inside codex-tui as `chatwidget::alab_hand` (one `#[path]`
//! line at the end of `tui/src/chatwidget.rs`): the person's input the TUI still holds lives
//! in `ChatWidget`'s private input queue, and this turn's plan in its private transcript
//! state, which only a child of `chatwidget` can read. The session observe glue
//! (`session_observe.rs`) asks these questions each frame.

use super::ChatWidget;
use crate::terminal_program_status::ProgramStatus;
use crate::terminal_program_status::ProgramStatusKind;
use codex_alab_tui::session::BlockedKind;
use codex_alab_tui::session::ProgramState;

impl ChatWidget {
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

    /// The displayed thread's state, as the OSC 7501 report `pre_draw_tick` wrote says it.
    pub(crate) fn alab_program_state(&self) -> ProgramState {
        match self.desired_program_status() {
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

    /// This turn's `update_plan` checklist as (completed, total) steps, the counts the title's
    /// `Tasks n/m` shows (`ALAB(progress)`). Only while the turn that sent it runs, or shows
    /// failed: a turn's flags reset only when the next one starts, so after the turn ends (a
    /// background terminal or an active goal keeps the thread working) its plan is no
    /// progress. `None` until this turn sends a plan: one an earlier turn left (the title keeps
    /// it) is not this turn's progress, nor is the last turn's while a new one is submitted and
    /// has not started.
    pub(crate) fn alab_plan_progress(&self) -> Option<(usize, usize)> {
        let its_turn = self.turn_lifecycle.agent_turn_running
            || self.desired_program_status() == ProgramStatus::Error;
        if !its_turn || !self.transcript.saw_plan_update_this_turn || self.alab_pending_user_turn()
        {
            return None;
        }
        self.transcript.last_plan_progress
    }
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
    fn turn(chat: &mut super::ChatWidget, id: &str, status: codex_app_server_protocol::TurnStatus) {
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
