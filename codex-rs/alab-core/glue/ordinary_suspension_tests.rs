// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use codex_core::SuspendTurnOutcome;
use codex_core::TurnInputRequest;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_protocol::protocol::EventMsg;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_response_once;
use core_test_support::responses::sse;
use core_test_support::responses::sse_response;
use core_test_support::responses::start_mock_server;
use core_test_support::test_codex::run_test_with_large_stack;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use std::time::Duration;

#[test]
fn ordinary_suspension_ignores_dropped_and_independently_live_api_managers() -> anyhow::Result<()> {
    run_test_with_large_stack("ordinary-suspension-catalog", || async {
        // This is a real admitted manager that is now gone, not a NoNativeReader proof.
        let dropped =
            AuthManager::from_auth_for_testing(CodexAuth::from_api_key("fixture-dropped"));
        let dropped_record = dropped.alab_assignment_domain().catalog().unwrap().clone();
        drop(dropped);
        assert!(dropped_record.domain().is_none());
        let independent =
            AuthManager::from_auth_for_testing(CodexAuth::from_api_key("fixture-independent"));
        let independent_work = independent
            .alab_assignment_domain()
            .admit_work(None)
            .unwrap();
        let server = start_mock_server().await;
        mount_response_once(
            &server,
            sse_response(sse(vec![ev_response_created("held"), ev_completed("held")]))
                .set_delay(Duration::from_secs(60)),
        )
        .await;
        let fixture = test_codex()
            .with_model("gpt-5.6-sol")
            .build_with_auto_env(&server)
            .await
            .unwrap();
        let started = fixture
            .codex
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "keep the actual unfinished turn".into(),
                text_elements: vec![],
            }]))
            .await
            .unwrap();
        let codex_core::TurnInputSubmission::Started { turn_id, .. } = started else {
            panic!("expected actual root turn")
        };
        wait_for_event(&fixture.codex, |event| {
            matches!(event, EventMsg::TurnStarted(_))
        })
        .await;
        let outcome = tokio::time::timeout(
            Duration::from_secs(10),
            fixture.codex.suspend_turn_and_shutdown(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(outcome, SuspendTurnOutcome::Suspended { turn_id });
        assert!(independent.auth_cached().is_some());
        assert!(
            independent
                .alab_assignment_domain()
                .admit_work(None)
                .is_ok()
        );
        let later =
            AuthManager::from_auth_for_testing(CodexAuth::from_api_key("fixture-after-suspension"));
        assert!(
            later.auth_cached().is_some(),
            "ordinary suspension must not close process factory admission"
        );
        assert!(later.alab_assignment_domain().admit_work(None).is_ok());
        drop(independent_work);
        Ok(())
    })
}
