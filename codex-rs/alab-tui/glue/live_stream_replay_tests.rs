// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exercise the real receiver used by the listener's targeted live-prefix replay.
use crate::app_event::AppEvent;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use codex_app_server_protocol::AgentMessageDeltaNotification;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItemPayload;

#[tokio::test]
async fn resumed_prefix_and_later_delta_render_once_on_completion() {
    let (mut chat, _sender, mut events, _ops) = make_chatwidget_manual_with_sender().await;
    let item = |text: &str| ThreadItemPayload::AgentMessage {
        id: "retained-message".into(),
        text: text.into(),
        moderation: None,
        phase: None,
        memory_citation: None,
        delivery: None,
        questions: None,
    };
    // The original item began before attachment. Its canonical prefix arrives
    // after thread/resume; completion still carries the complete canonical text.
    chat.handle_server_notification(
        ServerNotification::ItemStarted(ItemStartedNotification::new(
            "thread-1".into(),
            "turn-1".into(),
            item("retained prefix "),
            123,
        )),
        None,
    );
    chat.handle_server_notification(
        ServerNotification::AgentMessageDelta(AgentMessageDeltaNotification {
            thread_id: "thread-1".into(),
            turn_id: "turn-1".into(),
            item_id: "retained-message".into(),
            delta: "later suffix".into(),
        }),
        None,
    );
    chat.handle_server_notification(
        ServerNotification::ItemCompleted(ItemCompletedNotification::new(
            "thread-1".into(),
            "turn-1".into(),
            item("retained prefix later suffix"),
            Some(123),
            456,
        )),
        None,
    );
    let mut rendered = String::new();
    while let Ok(event) = events.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            for line in cell.display_lines(120) {
                for span in line.spans {
                    rendered.push_str(&span.content);
                }
                rendered.push('\n');
            }
        }
    }
    assert_eq!(
        rendered.matches("retained prefix later suffix").count(),
        1,
        "{rendered}"
    );
    assert_eq!(rendered.matches("retained prefix").count(), 1, "{rendered}");
    assert_eq!(rendered.matches("later suffix").count(), 1, "{rendered}");
}
