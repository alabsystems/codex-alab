// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Drive actual widget interruption and explicit goal commands. Continuous
//! work keeps its objective when a turn is interrupted; an explicit pause is
//! still the person's stop. Ordinary Codex and Off retain upstream behavior.

use crate::app_command::AppCommand;
use crate::app_event::AppEvent;
use crate::chatwidget::ChatWidget;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use codex_alab_tui::boxes;
use codex_alab_tui::session::Autopilot;
use codex_alab_tui::session::HarnessWord;
use codex_alab_tui::session::Shared;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadGoal;
use codex_app_server_protocol::ThreadGoalStatus;
use codex_app_server_protocol::ThreadGoalUpdatedNotification;
use codex_app_server_protocol::ToolRequestUserInputParams;
use codex_features::Feature;
use codex_protocol::ThreadId;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;

fn active_goal_turn(chat: &mut ChatWidget) -> ThreadId {
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);
    chat.show_welcome_banner = false;
    chat.set_feature_enabled(Feature::Goals, true);
    chat.handle_server_notification(
        ServerNotification::ThreadGoalUpdated(ThreadGoalUpdatedNotification {
            thread_id: thread_id.to_string(),
            turn_id: None,
            goal: ThreadGoal {
                thread_id: thread_id.to_string(),
                objective: "Continue the original compiler work".into(),
                status: ThreadGoalStatus::Active,
                token_budget: None,
                tokens_used: 0,
                time_used_seconds: 0,
                created_at: 0,
                updated_at: 0,
            },
        }),
        None,
    );
    chat.on_task_started();
    assert!(chat.is_active_goal_turn_running());
    thread_id
}

fn interrupt_keys() -> [KeyEvent; 2] {
    [
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ]
}

async fn interrupt_case(
    harness: Option<(Autopilot, HarnessWord)>,
    key: KeyEvent,
    question: bool,
    pauses_goal: bool,
) {
    let shared = Box::leak(Box::new(Shared::new()));
    if let Some((pilot, word)) = harness {
        shared.set_autopilot_with_word(pilot, word);
    }
    let _scope = harness.map(|_| boxes::scoped(shared));
    let (mut chat, _sender, mut events, mut operations) =
        make_chatwidget_manual_with_sender().await;
    let thread_id = active_goal_turn(&mut chat);
    if question {
        chat.handle_request_user_input_now(ToolRequestUserInputParams {
            thread_id: thread_id.to_string(),
            item_id: "goal-interrupt-question".into(),
            turn_id: "turn-1".into(),
            questions: Vec::new(),
            is_blocking: true,
            auto_resolution_ms: None,
        });
    }
    // Discard only setup events, before sending the actual interrupt key.
    while events.try_recv().is_ok() {}
    while operations.try_recv().is_ok() {}
    chat.handle_key_event(key);
    let mut interrupts = 0;
    let mut goal_changes = Vec::new();
    while let Ok(event) = events.try_recv() {
        match event {
            AppEvent::CodexOp(AppCommand::Interrupt) => interrupts += 1,
            AppEvent::SetThreadGoalStatus { thread_id, status } => {
                goal_changes.push((thread_id, status));
            }
            _ => {}
        }
    }
    while let Ok(operation) = operations.try_recv() {
        if matches!(operation, AppCommand::Interrupt) {
            interrupts += 1;
        }
    }
    assert_eq!(interrupts, 1, "{harness:?}, {key:?}, question={question}");
    let expected = if pauses_goal {
        vec![(thread_id, ThreadGoalStatus::Paused)]
    } else {
        Vec::new()
    };
    assert_eq!(
        goal_changes, expected,
        "{harness:?}, {key:?}, question={question}"
    );
}

#[tokio::test]
async fn continuous_turn_interrupt_keeps_the_goal_including_question_cancellation() {
    for pilot in [Autopilot::Active, Autopilot::Passive] {
        for word in [HarnessWord::Cruise, HarnessWord::Land] {
            for key in interrupt_keys() {
                for question in [false, true] {
                    interrupt_case(Some((pilot, word)), key, question, false).await;
                }
            }
        }
    }
}

#[tokio::test]
async fn ordinary_and_off_turn_interrupts_still_pause_the_goal() {
    for harness in [
        None,
        Some((Autopilot::Off, HarnessWord::Cruise)),
        Some((Autopilot::Off, HarnessWord::Land)),
        Some((Autopilot::Active, HarnessWord::Off)),
        Some((Autopilot::Active, HarnessWord::Unknown)),
    ] {
        for key in interrupt_keys() {
            interrupt_case(harness, key, false, true).await;
        }
    }
}

#[tokio::test]
async fn explicit_goal_pause_remains_effective_in_continuous_modes() {
    for pilot in [Autopilot::Active, Autopilot::Passive] {
        for word in [HarnessWord::Cruise, HarnessWord::Land] {
            let shared = Box::leak(Box::new(Shared::new()));
            shared.set_autopilot_with_word(pilot, word);
            let _scope = boxes::scoped(shared);
            let (mut chat, _sender, mut events, _operations) =
                make_chatwidget_manual_with_sender().await;
            let thread_id = active_goal_turn(&mut chat);
            while events.try_recv().is_ok() {}
            chat.bottom_pane
                .set_composer_text("/goal pause".into(), Vec::new(), Vec::new());
            chat.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            let goal_changes = std::iter::from_fn(|| events.try_recv().ok())
                .filter_map(|event| match event {
                    AppEvent::SetThreadGoalStatus { thread_id, status } => {
                        Some((thread_id, status))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                goal_changes,
                vec![(thread_id, ThreadGoalStatus::Paused)],
                "{pilot:?}/{word:?}"
            );
        }
    }
}
