// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use codex_core::SuspendTurnOutcome;
use codex_core::TurnInputRequest;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_protocol::protocol::ThreadMemoryMode;
use codex_protocol::user_input::UserInput;
use codex_rollout::RolloutItem;
use codex_thread_store::LoadThreadHistoryParams;
use codex_thread_store::ResumeThreadParams;
use codex_thread_store::ThreadPersistenceMetadata;
use codex_thread_store::ThreadStoreError;
use core_test_support::ThreadIdle;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_response_once;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::received_responses_requests;
use core_test_support::responses::sse;
use core_test_support::responses::sse_completed;
use core_test_support::responses::sse_response;
use core_test_support::responses::start_mock_server;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::run_test_with_large_stack;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use core_test_support::wait_for_event_match;
use std::sync::Arc;
use std::time::Duration;

fn assistant_reply(response_id: &str, answer: &str) -> String {
    sse(vec![
        ev_response_created(response_id),
        ev_assistant_message(&format!("msg-{response_id}"), answer),
        ev_completed(response_id),
    ])
}

fn user_input_texts(request: &serde_json::Value) -> impl Iterator<Item = &str> + Clone {
    request["input"]
        .as_array()
        .expect("Responses input")
        .iter()
        .filter(|item| item["role"] == "user")
        .filter_map(|item| item["content"].as_array())
        .flatten()
        .filter_map(|content| content["text"].as_str())
}

async fn assert_live_turn(
    fixture: &TestCodex,
    prompt: &str,
    answer: &str,
) -> anyhow::Result<String> {
    let submission = fixture
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: prompt.into(),
            text_elements: vec![],
        }]))
        .await?;
    let codex_core::TurnInputSubmission::Started { turn_id, .. } = submission else {
        panic!("expected a fresh turn, got {submission:?}");
    };
    let assistant = wait_for_event_match(&fixture.codex, |event| match event {
        EventMsg::ItemCompleted(item) if matches!(item.item, TurnItem::AgentMessage(_)) => {
            Some((item.thread_id, item.turn_id.clone()))
        }
        _ => None,
    })
    .await;
    assert_eq!(
        assistant,
        (fixture.session_configured.thread_id, turn_id.clone())
    );
    let completed = wait_for_event_match(&fixture.codex, |event| match event {
        EventMsg::TurnComplete(turn) => Some(turn.clone()),
        _ => None,
    })
    .await;
    assert_eq!(completed.turn_id, turn_id);
    assert_eq!(completed.error, None);
    assert_eq!(completed.last_agent_message.as_deref(), Some(answer));
    Ok(turn_id)
}

#[test]
fn inactive_suspension_preserves_live_admission() -> anyhow::Result<()> {
    run_test_with_large_stack("inactive-suspension", || async {
        let server = start_mock_server().await;
        let fixture = test_codex().build_with_auto_env(&server).await?;
        let thread_id = fixture.session_configured.thread_id;
        assert_eq!(
            tokio::time::timeout(
                Duration::from_secs(10),
                fixture.codex.suspend_turn_and_shutdown()
            )
            .await??,
            SuspendTurnOutcome::NotActive
        );

        let prompt = "Can this idle thread still take a turn?";
        let answer = "This thread is still available.";
        mount_sse_once(
            &server,
            sse(vec![
                ev_response_created("resp1"),
                ev_assistant_message("msg1", answer),
                ev_completed("resp1"),
            ]),
        )
        .await;
        assert_live_turn(&fixture, prompt, answer).await?;
        fixture.codex.shutdown_and_wait().await?;

        let requests = received_responses_requests(&server).await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].header("thread-id"), Some(thread_id.to_string()));
        assert!(
            requests[0]
                .message_input_texts("user")
                .iter()
                .any(|text| text == prompt)
        );
        Ok(())
    })
}

#[test]
fn unsupported_suspension_preserves_compaction_and_live_admission() -> anyhow::Result<()> {
    run_test_with_large_stack("unsupported-suspension", || async {
        let summary = "The compacted conversation retained the apricot inventory.";
        let prompt = "Continue after the inventory was compacted.";
        let answer = "Live inference resumed on the original thread.";
        let (release, compaction_gate) = tokio::sync::oneshot::channel();
        let (server, _) = start_streaming_sse_server(vec![
            vec![StreamingSseChunk {
                gate: None,
                body: sse_completed("seed"),
            }],
            vec![StreamingSseChunk {
                gate: Some(compaction_gate),
                body: assistant_reply("compaction", summary),
            }],
            vec![StreamingSseChunk {
                gate: None,
                body: assistant_reply("fresh", answer),
            }],
        ])
        .await;
        let fixture = test_codex()
            .with_model("gpt-5.4")
            .with_config(|config| {
                // This provider performs manual compaction via a regular Responses request.
                config.model_provider.name = "OpenAI (test)".to_string();
            })
            .build_with_streaming_server(&server)
            .await?;
        fixture
            .submit_text_turn("Remember the apricot inventory.")
            .await?;
        let compaction = fixture.codex.submit(Op::Compact).await?;
        let timeout = Duration::from_secs(10);
        tokio::time::timeout(timeout, server.wait_for_request_count(2)).await?;
        assert_eq!(
            tokio::time::timeout(timeout, fixture.codex.suspend_turn_and_shutdown()).await??,
            SuspendTurnOutcome::UnsupportedTask
        );
        release
            .send(())
            .expect("original compaction still awaiting its response");
        let item = wait_for_event_match(&fixture.codex, |event| match event {
            EventMsg::ItemCompleted(item)
                if matches!(item.item, TurnItem::ContextCompaction(_)) =>
            {
                Some((item.thread_id, item.turn_id.clone()))
            }
            _ => None,
        })
        .await;
        assert_eq!(
            item,
            (fixture.session_configured.thread_id, compaction.clone())
        );
        let completed = wait_for_event_match(&fixture.codex, |event| match event {
            EventMsg::TurnComplete(turn) => Some(turn.clone()),
            _ => None,
        })
        .await;
        assert_eq!(completed.turn_id, compaction);
        assert_eq!(completed.error, None);

        let next_turn = assert_live_turn(&fixture, prompt, answer).await?;
        assert_ne!(next_turn, compaction);
        fixture.codex.shutdown_and_wait().await?;
        let requests = server.requests().await;
        server.shutdown().await;
        assert_eq!(requests.len(), 3);
        let fresh: serde_json::Value = serde_json::from_slice(&requests[2])?;
        let mut texts = user_input_texts(&fresh);
        assert!(
            texts.clone().any(|text| text.contains(summary)),
            "compacted history missing from new turn"
        );
        assert!(
            texts.any(|text| text == prompt),
            "new prompt missing from model request"
        );
        Ok(())
    })
}

#[test]
fn preterminal_writer_failure_preserves_running_turn_and_live_admission() -> anyhow::Result<()> {
    run_test_with_large_stack("preterminal-writer-failure", || async {
        let (release, gate) = tokio::sync::oneshot::channel();
        let response = |id: &str, answer: &str, gate| {
            vec![StreamingSseChunk {
                gate,
                body: assistant_reply(id, answer),
            }]
        };
        let first_prompt = "Finish this original turn.";
        let next_prompt = "Start a new turn.";
        let first_answer = "Finished the original turn.";
        let next_answer = "Finished the new turn.";
        let (server, _) = start_streaming_sse_server(vec![
            response("held", first_answer, Some(gate)),
            response("next", next_answer, None),
        ])
        .await;
        let mut extensions = codex_extension_api::ExtensionRegistryBuilder::new();
        extensions.thread_lifecycle_contributor(Arc::new(ThreadIdle));
        let fixture = test_codex()
            .with_extensions(Arc::new(extensions.build()))
            .with_history_mode(ThreadHistoryMode::Legacy)
            .with_config(|config| config.memories.generate_memories = false)
            .build_with_streaming_server(&server)
            .await?;
        let thread_id = fixture.session_configured.thread_id;
        let store = &fixture.thread_store;
        let first = assert_live_turn(&fixture, first_prompt, first_answer);
        let refuse_suspension = async {
            server.wait_for_request_count(1).await;
            let rollout_path = fixture.codex.rollout_path().expect("original rollout");
            // Close only the fixture's recorder while inference is held. The root session stays live.
            store.shutdown_thread(thread_id).await?;
            let error = fixture.codex.suspend_turn_and_shutdown().await.unwrap_err();
            let cause = ThreadStoreError::ThreadNotFound { thread_id };
            assert!(
                matches!(error.details(), CodexErrorDetails::Fatal(message)
                if message == &format!("flush before root turn suspension failed: {cause}")),
                "{error}"
            );
            store
                .resume_thread(ResumeThreadParams {
                    thread_id,
                    rollout_path: Some(rollout_path),
                    history: None,
                    history_revision: None,
                    include_archived: false,
                    metadata: ThreadPersistenceMetadata {
                        cwd: Some(fixture.config.cwd.to_path_buf()),
                        model_provider: fixture.config.model_provider_id.clone(),
                        memory_mode: ThreadMemoryMode::Disabled,
                    },
                })
                .await?;
            release.send(()).expect("original request still waiting");
            anyhow::Ok(())
        };
        let (first_id, ()) = tokio::time::timeout(Duration::from_secs(20), async {
            tokio::try_join!(first, refuse_suspension)
        })
        .await??;
        ThreadIdle::wait(&fixture.codex).await;
        let next_id = assert_live_turn(&fixture, next_prompt, next_answer).await?;
        assert_ne!(first_id, next_id);
        fixture.codex.shutdown_and_wait().await?;
        let requests = server.requests().await;
        server.shutdown().await;
        assert_eq!(requests.len(), 2);
        for (request, prompt) in requests.iter().zip([first_prompt, next_prompt]) {
            let body = serde_json::from_slice(request)?;
            assert!(user_input_texts(&body).any(|text| text == prompt));
        }
        let history = store
            .load_history(LoadThreadHistoryParams {
                thread_id,
                include_archived: false,
            })
            .await?;
        assert_eq!(history.thread_id, thread_id);
        let completed: Vec<_> = history
            .items
            .iter()
            .filter_map(|item| match item {
                RolloutItem::EventMsg(EventMsg::TurnComplete(turn)) => Some(turn.turn_id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(completed, [first_id.as_str(), next_id.as_str()]);
        Ok(())
    })
}

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
