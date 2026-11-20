// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use crate::app_event::AppEvent;
use crate::app_event::RateLimitRefreshOrigin;
use crate::chatwidget::ReplayKind;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use crate::chatwidget::tests::set_chatgpt_auth;
use codex_app_server_protocol::Turn;
use codex_protocol::ThreadId;
use core_test_support::alab_test_process;
use serde_json::json;

#[tokio::test]
async fn saved_quota_error_preserves_the_current_logins_input() {
    if !alab_test_process::child(concat!(
        module_path!(),
        "::saved_quota_error_preserves_the_current_logins_input"
    )) {
        return;
    }
    codex_alab_core::activate();
    let original = "The original request was rate limited.";
    for error_code in [
        json!("usageLimitExceeded"),
        json!({ "responseTooManyFailedAttempts": { "httpStatusCode": 429 } }),
    ] {
        for kind in [
            ReplayKind::ResumeInitialMessages,
            ReplayKind::ThreadSnapshot,
        ] {
            let (mut chat, _sender, mut events, _ops) = make_chatwidget_manual_with_sender().await;
            set_chatgpt_auth(&mut chat);
            chat.thread_id = Some(ThreadId::new());
            let failed: Turn = serde_json::from_value(json!({
                "id": "saved-turn",
                "items": [],
                "status": "failed",
                "error": { "message": original, "codexErrorInfo": error_code }
            }))
            .unwrap();
            chat.replay_thread_turns(vec![failed], kind);

            assert!(!chat.input_queue.rate_limit_recovery_pending);
            let mut transcript = String::new();
            while let Ok(event) = events.try_recv() {
                match event {
                    AppEvent::InsertHistoryCell(cell) => {
                        for line in cell.transcript_lines(120) {
                            transcript.push_str(&line.to_string());
                        }
                    }
                    AppEvent::RefreshRateLimits {
                        origin: RateLimitRefreshOrigin::Recovery,
                    } => {
                        panic!("a saved quota error started live recovery");
                    }
                    _ => {}
                }
            }
            assert_eq!(transcript.matches(original).count(), 1);
            chat.handle_non_retry_error(
                original.to_owned(),
                Some(serde_json::from_value(error_code.clone()).unwrap()),
            );
            assert!(chat.input_queue.rate_limit_recovery_pending);
            assert!(
                std::iter::from_fn(|| events.try_recv().ok()).any(|event| matches!(
                    event,
                    AppEvent::RefreshRateLimits {
                        origin: RateLimitRefreshOrigin::Recovery
                    }
                ))
            );
        }
    }
    // Another response failure follows the existing general-error path.
    let (mut chat, _sender, mut events, _ops) = make_chatwidget_manual_with_sender().await;
    set_chatgpt_auth(&mut chat);
    chat.handle_non_retry_error(
        original.to_owned(),
        Some(
            serde_json::from_value(json!({
                "responseTooManyFailedAttempts": { "httpStatusCode": 500 }
            }))
            .unwrap(),
        ),
    );
    assert!(!chat.input_queue.rate_limit_recovery_pending);
    assert!(
        !std::iter::from_fn(|| events.try_recv().ok()).any(|event| matches!(
            event,
            AppEvent::RefreshRateLimits {
                origin: RateLimitRefreshOrigin::Recovery
            }
        ))
    );
}
