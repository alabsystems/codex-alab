// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_app_server_protocol::ThreadItemPayload;
use codex_app_server_protocol::UserInput as ApiUserInput;
use codex_login::CodexAuth;
use codex_protocol::openai_models::ReasoningEffort;
use core_test_support::wait_for_successful_turn;
use pretty_assertions::assert_eq;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

use core_test_support::alab_test_process as test_process;

#[tokio::test]
async fn cold_thread_resume_promotes_saved_family_and_keeps_completed_history() -> Result<()> {
    if !test_process::child(concat!(
        module_path!(),
        "::cold_thread_resume_promotes_saved_family_and_keeps_completed_history"
    )) {
        return Ok(());
    }
    codex_alab_core::activate();
    for (old, new, auth) in [
        (
            "gpt-6.1-sol",
            "gpt-6.2-sol",
            CodexAuth::create_dummy_chatgpt_auth_for_testing(),
        ),
        (
            "gpt-6-astra",
            "gpt-6.2-astra",
            CodexAuth::from_api_key("test-only"),
        ),
    ] {
        resume_with_saved_turn(old, new, auth).await?;
    }
    Ok(())
}

async fn resume_with_saved_turn(old: &str, new: &str, auth: CodexAuth) -> Result<()> {
    const PROMPT: &str = "Remember this turn when I reopen the conversation.";
    const REPLY: &str = "This is the original reply.";
    let server = MockServer::start().await;
    let base_url = format!("{}/backend-api/codex", server.uri());
    let mut catalog = codex_models_manager::bundled_models_response()?;
    catalog.models.retain(|model| {
        matches!(
            model.slug.as_str(),
            "gpt-6.1-sol" | "gpt-6.2-sol" | "gpt-6-astra" | "gpt-6.2-astra"
        )
    });
    Mock::given(method("GET"))
        .and(path("/backend-api/codex/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(catalog))
        .mount(&server)
        .await;
    // A cold start uses the built-in provider's WebSocket negotiation. This
    // SSE-only server exercises its ordinary fallback to HTTP.
    Mock::given(method("GET"))
        .and(path("/backend-api/codex/responses"))
        .respond_with(ResponseTemplate::new(426))
        .mount(&server)
        .await;
    let original_request = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("saved-response"),
            responses::ev_assistant_message("saved-message", REPLY),
            responses::ev_completed("saved-response"),
        ]),
    )
    .await;
    let original_base_url = base_url.clone();
    let test = test_codex()
        .with_model(old)
        .with_auth(auth)
        .with_config(move |config| {
            config.model_provider.base_url = Some(original_base_url);
            config.model_reasoning_effort = Some(ReasoningEffort::High);
        })
        .build(&server)
        .await?;
    let thread_id = test.session_configured.thread_id;
    let TurnInputSubmission::Started { turn_id, .. } = test
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: PROMPT.into(),
            text_elements: Vec::new(),
        }]))
        .await?
    else {
        anyhow::bail!("historical turn did not start");
    };
    wait_for_successful_turn(&test.codex).await;
    let original_body = original_request.single_request().body_json();
    assert_eq!(original_body["model"], old);
    assert_eq!(original_body["reasoning"]["effort"], "high");
    test.thread_store
        .persist_thread(thread_id, PersistContext::Standard)
        .await?;
    test.codex.shutdown_and_wait().await?;
    test.thread_manager.remove_thread(&thread_id).await;
    assert!(test.thread_manager.get_thread(thread_id).await.is_err());

    let (processor, mut outgoing) = test_processor(&test).await?;
    let connection_id = ConnectionId(1);
    let request_id = RequestId::Integer(1);
    processor
        .connection_initialized(connection_id, ConnectionCapabilities::default())
        .await;
    let resume = processor.thread_resume(
        ThreadResumeTarget::Client(
            ConnectionRequestId {
                connection_id,
                request_id: request_id.clone(),
            }
            .into(),
        ),
        ThreadResumeParams {
            thread_id: thread_id.to_string(),
            config: Some([("openai_base_url".into(), serde_json::json!(base_url))].into()),
            ..Default::default()
        },
        Some("codex-tui".into()),
        None,
        Default::default(),
    );
    let receive = async {
        while let Some(envelope) = outgoing.recv().await {
            if let OutgoingEnvelope::ToConnection {
                connection_id: recipient,
                message,
                ..
            } = envelope
                && recipient == connection_id
            {
                match message {
                    OutgoingMessage::Response(response) if response.id == request_id => {
                        if let ClientResponsePayload::ThreadResume(resumed) = *response.result {
                            return Ok(resumed);
                        }
                        anyhow::bail!("unexpected response to thread/resume");
                    }
                    OutgoingMessage::Error(error) if error.id == request_id => {
                        anyhow::bail!("{}", error.error.message)
                    }
                    _ => {}
                }
            }
        }
        anyhow::bail!("thread/resume response channel closed");
    };
    let (_, resumed) = tokio::time::timeout(Duration::from_secs(15), async {
        tokio::try_join!(
            async { resume.await.map_err(|err| anyhow::anyhow!(err.message)) },
            receive
        )
    })
    .await??;
    assert_eq!(resumed.thread.id, thread_id.to_string());
    assert_eq!(resumed.model, new);
    assert_eq!(resumed.reasoning_effort, Some(ReasoningEffort::High));
    let [turn] = resumed.thread.turns.as_slice() else {
        anyhow::bail!(
            "expected one historical turn, got {:?}",
            resumed.thread.turns
        );
    };
    assert_eq!(turn.id, turn_id);
    assert_eq!(turn.status, TurnStatus::Completed);
    assert!(turn.item_payloads().any(|item|
        matches!(item, ThreadItemPayload::UserMessage { content, .. } if matches!(content.as_slice(), [ApiUserInput::Text { text, .. }] if text == PROMPT))
    ));
    assert!(
        turn.item_payloads().any(
            |item| matches!(item, ThreadItemPayload::AgentMessage { text, .. } if text == REPLY)
        )
    );
    let continued = responses::mount_sse_once(&server, responses::sse_completed("continued")).await;
    let thread = test.thread_manager.get_thread(thread_id).await?;
    let TurnInputSubmission::Started {
        turn_id: continued_id,
        ..
    } = thread
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Continue the old conversation.".into(),
            text_elements: Vec::new(),
        }]))
        .await?
    else {
        anyhow::bail!("continued turn did not start");
    };
    let completed = tokio::time::timeout(Duration::from_secs(20), async {
        while let Some(envelope) = outgoing.recv().await {
            if let OutgoingEnvelope::ToConnection {
                connection_id: recipient,
                message: OutgoingMessage::AppServerNotification(notification),
                ..
            } = envelope
                && recipient == connection_id
                && let ServerNotification::TurnCompleted(completed) = notification.notification
                && completed.turn.id == continued_id
            {
                return Ok(completed);
            }
        }
        anyhow::bail!("continued turn notification channel closed");
    })
    .await??;
    assert_eq!(completed.turn.status, TurnStatus::Completed);
    let continued_body = continued.single_request().body_json();
    assert_eq!(continued_body["model"], new);
    assert_eq!(continued_body["reasoning"]["effort"], "high");
    thread.shutdown_and_wait().await?;
    processor.thread_state_manager.clear_all_listeners().await;
    processor.skills_watcher.shutdown();
    Ok(())
}
