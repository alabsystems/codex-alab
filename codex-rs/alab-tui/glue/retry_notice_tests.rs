// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exercise the actual TUI notification router under the existing thread-local
//! interface scope. The parent test process never activates ALab globally.

use super::*;
use crate::chatwidget::QueuedUserMessage;
use crate::chatwidget::UserMessage;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use codex_app_server_protocol::AgentMessageDeltaNotification;
use codex_app_server_protocol::ErrorNotification;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadNameUpdatedNotification;
use codex_app_server_protocol::ThreadTokenUsage;
use codex_app_server_protocol::ThreadTokenUsageUpdatedNotification;
use codex_app_server_protocol::TokenUsageBreakdown;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnError;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::ThreadId;

const TURN: &str = "capacity-wait-turn";
const NOTICE: &str = "Model is at capacity; retrying automatically (interrupt to stop)...";
const DETAILS: &str = "The selected model is at capacity.";

fn scope(enabled: bool) -> interface::Scope {
    let state = Box::leak(Box::new(interface::State::new()));
    let scope = interface::scoped(state);
    interface::configure(enabled);
    scope
}

fn thread_id(chat: &ChatWidget) -> String {
    chat.thread_id.expect("fixture thread identity").to_string()
}

fn turn(status: TurnStatus) -> Turn {
    Turn {
        id: TURN.into(),
        root_turn_id: None,
        items_view: TurnItemsView::Full,
        items: Vec::new(),
        status,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }
}

fn start(chat: &mut ChatWidget) {
    chat.thread_id = Some(ThreadId::new());
    chat.handle_server_notification(
        ServerNotification::TurnStarted(TurnStartedNotification {
            thread_id: thread_id(chat),
            turn: turn(TurnStatus::InProgress),
        }),
        None,
    );
}

fn retry(chat: &mut ChatWidget) {
    chat.handle_server_notification(
        ServerNotification::Error(ErrorNotification {
            thread_id: thread_id(chat),
            turn_id: TURN.into(),
            will_retry: true,
            error: TurnError {
                message: NOTICE.into(),
                codex_error_info: None,
                additional_details: Some(DETAILS.into()),
                misalignment: None,
            },
        }),
        None,
    );
}

fn metadata(chat: &ChatWidget) -> [ServerNotification; 5] {
    let usage = TokenUsageBreakdown {
        total_tokens: 12,
        input_tokens: 10,
        cached_input_tokens: 0,
        cache_write_input_tokens: 0,
        output_tokens: 2,
        reasoning_output_tokens: 0,
    };
    [
        ServerNotification::ThreadNameUpdated(ThreadNameUpdatedNotification {
            thread_id: thread_id(chat),
            thread_name: Some("renamed during capacity wait".into()),
        }),
        ServerNotification::ThreadTokenUsageUpdated(ThreadTokenUsageUpdatedNotification {
            thread_id: thread_id(chat),
            turn_id: TURN.into(),
            token_usage: ThreadTokenUsage {
                total: usage.clone(),
                last: usage,
                model_context_window: Some(100_000),
            },
        }),
        ServerNotification::ThreadGoalUpdated(
            codex_app_server_protocol::ThreadGoalUpdatedNotification {
                thread_id: thread_id(chat),
                turn_id: Some(TURN.into()),
                goal: codex_app_server_protocol::ThreadGoal {
                    thread_id: thread_id(chat),
                    objective: "Continue the existing task".into(),
                    status: codex_app_server_protocol::ThreadGoalStatus::Active,
                    token_budget: Some(100_000),
                    tokens_used: 12,
                    time_used_seconds: 1,
                    created_at: 0,
                    updated_at: 1,
                },
            },
        ),
        ServerNotification::ThreadGoalCleared(
            codex_app_server_protocol::ThreadGoalClearedNotification {
                thread_id: thread_id(chat),
            },
        ),
        ServerNotification::ThreadSettingsUpdated(
            codex_app_server_protocol::ThreadSettingsUpdatedNotification {
                thread_id: thread_id(chat),
                thread_settings: codex_app_server_protocol::ThreadSettings {
                    disabled_plugin_ids: Vec::new(),
                    cwd: chat.config.cwd.clone(),
                    environments: None,
                    approval_policy: codex_app_server_protocol::AskForApproval::OnRequest,
                    approvals_reviewer: codex_app_server_protocol::ApprovalsReviewer::AutoReview,
                    sandbox_policy: codex_app_server_protocol::SandboxPolicy::ReadOnly {
                        network_access: false,
                    },
                    active_permission_profile: Some(
                        codex_app_server_protocol::ActivePermissionProfile::read_only(),
                    ),
                    model: "gpt-5.5".into(),
                    model_provider: "openai".into(),
                    service_tier: None,
                    effort: None,
                    summary: None,
                    collaboration_mode: codex_protocol::config_types::CollaborationMode {
                        mode: codex_protocol::config_types::ModeKind::Default,
                        settings: codex_protocol::config_types::Settings {
                            model: "gpt-5.5".into(),
                            reasoning_effort: None,
                            developer_instructions: None,
                        },
                    },
                    multi_agent_mode: Default::default(),
                    personality: None,
                },
            },
        ),
    ]
}

#[tokio::test]
async fn metadata_retains_the_live_notice_only_when_enhanced() {
    assert!(!interface::active());
    for enabled in [true, false] {
        let _scope = scope(enabled);
        let (mut chat, _tx, _events, _ops) = make_chatwidget_manual_with_sender().await;
        start(&mut chat);
        chat.bottom_pane
            .set_composer_text("unsent draft".into(), Vec::new(), Vec::new());
        chat.input_queue
            .queued_user_messages
            .push_back(QueuedUserMessage::from(UserMessage::from(
                "queued follow-up",
            )));
        let queued = chat.input_queue.queued_user_messages.clone();
        retry(&mut chat);
        for notification in metadata(&chat) {
            assert_eq!(
                ChatWidget::alab_preserve_retry_notice(&notification),
                enabled
            );
            chat.handle_server_notification(notification, None);
            let status = chat
                .bottom_pane
                .status_widget()
                .expect("visible capacity wait");
            assert_eq!(status.header(), if enabled { NOTICE } else { "Working" });
            assert_eq!(status.details(), enabled.then_some(DETAILS));
            assert_eq!(
                chat.status_state.retry_status_header.as_deref(),
                enabled.then_some("Working")
            );
            assert!(chat.turn_lifecycle.agent_turn_running);
            assert!(chat.bottom_pane.is_task_running());
            assert_eq!(chat.input_queue.queued_user_messages, queued);
            assert_eq!(chat.bottom_pane.composer_text(), "unsent draft");
            assert_eq!(interface::enabled(), enabled);
        }
        assert_eq!(
            chat.thread_name.as_deref(),
            Some("renamed during capacity wait")
        );
    }
    assert!(!interface::active());
}

#[tokio::test]
async fn inactive_metadata_keeps_the_upstream_restoration() {
    assert!(!interface::active());
    let (mut chat, _tx, _events, _ops) = make_chatwidget_manual_with_sender().await;
    start(&mut chat);
    for notification in metadata(&chat) {
        retry(&mut chat);
        assert!(!ChatWidget::alab_preserve_retry_notice(&notification));
        chat.handle_server_notification(notification, None);
        assert_eq!(chat.status_state.current_status.header, "Working");
        assert_eq!(chat.status_state.current_status.details, None);
        assert!(chat.status_state.retry_status_header.is_none());
        assert!(chat.bottom_pane.is_task_running());
    }
    assert!(!interface::active());
}

#[tokio::test]
async fn actual_content_and_item_progress_restore_the_previous_status() {
    let _scope = scope(true);
    let (mut chat, _tx, _events, _ops) = make_chatwidget_manual_with_sender().await;
    start(&mut chat);
    retry(&mut chat);
    chat.handle_server_notification(
        ServerNotification::AgentMessageDelta(AgentMessageDeltaNotification {
            thread_id: thread_id(&chat),
            turn_id: TURN.into(),
            item_id: "recovered-answer".into(),
            delta: "Recovery resumed".into(),
        }),
        None,
    );
    assert_eq!(chat.status_state.current_status.header, "Working");
    assert_eq!(chat.status_state.current_status.details, None);
    assert!(chat.status_state.retry_status_header.is_none());
    assert!(chat.bottom_pane.is_task_running());
    retry(&mut chat);
    chat.handle_server_notification(
        ServerNotification::ItemStarted(ItemStartedNotification {
            thread_id: thread_id(&chat),
            turn_id: TURN.into(),
            started_at_ms: 0,
            item: ThreadItem::untimed(
                codex_app_server_protocol::ThreadItemPayload::ContextCompaction {
                    id: "compact".into(),
                },
            ),
        }),
        None,
    );
    assert!(chat.status_state.retry_status_header.is_none());
    assert_eq!(
        chat.bottom_pane.status_widget().unwrap().header(),
        "Compacting context"
    );
    retry(&mut chat);
    chat.handle_server_notification(
        ServerNotification::ItemCompleted(ItemCompletedNotification {
            thread_id: thread_id(&chat),
            turn_id: TURN.into(),
            completed_at_ms: 0,
            content_token_count: None,
            item: ThreadItem::untimed(
                codex_app_server_protocol::ThreadItemPayload::ContextCompaction {
                    id: "compact".into(),
                },
            ),
        }),
        None,
    );
    assert!(chat.status_state.retry_status_header.is_none());
    assert_eq!(chat.status_state.current_status.header, "Working");
}

#[tokio::test]
async fn terminal_error_and_completion_end_the_retry_display() {
    for failed in [false, true] {
        let _scope = scope(false);
        let (mut chat, _tx, _events, _ops) = make_chatwidget_manual_with_sender().await;
        start(&mut chat);
        retry(&mut chat);
        let notification = if failed {
            ServerNotification::Error(ErrorNotification {
                thread_id: thread_id(&chat),
                turn_id: TURN.into(),
                will_retry: false,
                error: TurnError {
                    message: "terminal refusal".into(),
                    codex_error_info: Some(
                        codex_app_server_protocol::CodexErrorInfo::ServerOverloaded,
                    ),
                    additional_details: None,
                    misalignment: None,
                },
            })
        } else {
            ServerNotification::TurnCompleted(TurnCompletedNotification {
                thread_id: thread_id(&chat),
                turn: turn(TurnStatus::Completed),
            })
        };
        chat.handle_server_notification(notification, None);
        assert!(chat.status_state.retry_status_header.is_none());
        assert!(!chat.turn_lifecycle.agent_turn_running);
        assert!(!chat.bottom_pane.is_task_running());
        assert_ne!(chat.status_state.current_status.header, NOTICE);
    }
}
