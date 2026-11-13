// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Safety recovery through real embedded turns and the queued App event boundary.

use super::*;
use crate::app::session_lifecycle::ThreadAttachPresentation;
use crate::app::tests::make_test_app_with_channels;
use crate::app_event::AppEvent;
use app_test_support::MockResponsesConfig;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::ModelSafetyBufferingUpdatedNotification;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::TurnStatus;
use codex_features::Feature;
use codex_protocol::ThreadId;
use core_test_support::responses;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;
use wiremock::Mock;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

struct Fixture {
    app: Box<App>,
    server: AppServerSession,
    tui: crate::tui::Tui,
    events: UnboundedReceiver<AppEvent>,
    thread: ThreadId,
    _workspace: tempfile::TempDir,
}

impl Fixture {
    async fn new(uri: &str) -> color_eyre::Result<Self> {
        let (mut app, events, _operations) = make_test_app_with_channels().await;
        let workspace = tempfile::tempdir()?;
        let cwd = workspace.path().canonicalize()?;
        MockResponsesConfig::new(uri)
            .with_model("gpt-5.5")
            .disable_feature(Feature::EnableRequestCompression)
            .with_provider_config("requires_openai_auth = false\nsupports_websockets = false")
            .with_root_config("tools.update_plan.enabled = true")
            .write(&app.config.codex_home)?;
        app.config = app.rebuild_config_for_cwd(cwd).await?;
        let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let mut started = server.start_thread(&app.config).await?;
        let thread = started.session.thread_id;
        server.thread_set_name(thread, "Safety validation".into()).await?;
        started.session.thread_name = Some("Safety validation".into());
        let mut tui = crate::tui::test_support::make_test_tui()?;
        app.replace_chat_widget_with_app_server_thread(
            &mut tui,
            started,
            ThreadAttachPresentation::SessionLineage,
            None,
        )
        .await?;
        Ok(Self { app, server, tui, events, thread, _workspace: workspace })
    }

    fn claim(&self, shared: &codex_alab_tui::boxes::Shared) {
        let thread = self.thread.to_string();
        let config = self.app.chat_widget.config_ref();
        shared.observe(&codex_alab_tui::session::Frame {
            primary_thread: Some(&thread),
            displayed_thread: Some(&thread),
            rollout_path: None,
            cwd: config.cwd.as_path(),
            model: self.app.chat_widget.current_model(),
            codex_home: &config.codex_home,
            embedded: true,
            state: codex_alab_tui::session::ProgramState::Idle,
            draft_empty: true,
            queued_input: false,
            popup_open: false,
            editor_open: false,
            pending_user_turn: false,
        }, codex_alab_tui::boxes::now_ms());
        shared.note_root_thread(&thread);
        shared.set_claim(Some("codex-alab@safety-test".into()));
        shared.set_autopilot(codex_alab_tui::session::Autopilot::Active);
        assert!(self.app.chat_widget.alab_autonomous_root_active());
    }

    async fn submit(&mut self, text: &str) -> color_eyre::Result<String> {
        self.app.chat_widget.apply_external_edit(text.into());
        self.app.chat_widget.handle_key_event(KeyEvent::from(KeyCode::Enter));
        let command = loop {
            if let AppEvent::CodexOp(command @ AppCommand::UserTurn { .. }) =
                self.events.try_recv()?
            {
                break command;
            }
        };
        self.app.submit_thread_op(&mut self.server, self.thread, command).await?;
        self.started().await
    }

    async fn started(&mut self) -> color_eyre::Result<String> {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(10), self.server.next_event())
                .await?.ok_or_else(|| color_eyre::eyre::eyre!("embedded server closed"))?;
            let started = match &event.event {
                AppServerEvent::ServerNotification(notification) => match notification.as_ref() {
                    ServerNotification::TurnStarted(started) if started.thread_id == self.thread.to_string() => Some(started.turn.id.clone()),
                    _ => None,
                },
                _ => None,
            };
            self.app.handle_backend_event(&self.server, event).await;
            self.drain();
            if let Some(turn) = started { return Ok(turn); }
        }
    }

    fn drain(&mut self) {
        while let Some(event) = self.app.active_thread_rx.as_mut().and_then(|rx| rx.try_recv().ok()) {
            self.app.handle_thread_event_now(event);
        }
    }

    async fn complete(&mut self, turn: &str) -> color_eyre::Result<usize> {
        let mut plans = 0;
        loop {
            let event = tokio::time::timeout(Duration::from_secs(10), self.server.next_event())
                .await?.ok_or_else(|| color_eyre::eyre::eyre!("embedded server closed"))?;
            let completed = match &event.event {
                AppServerEvent::ServerNotification(notification) => match notification.as_ref() {
                    ServerNotification::TurnPlanUpdated(update) if update.thread_id == self.thread.to_string() => { plans += 1; false }
                    ServerNotification::TurnCompleted(done) if done.thread_id == self.thread.to_string() && done.turn.id == turn => true,
                    _ => false,
                },
                _ => false,
            };
            self.app.handle_backend_event(&self.server, event).await;
            self.drain();
            if completed { return Ok(plans); }
        }
    }

    fn notice(&mut self, thread: ThreadId, turn: &str) {
        self.app.chat_widget.handle_server_notification(
            ServerNotification::ModelSafetyBufferingUpdated(ModelSafetyBufferingUpdatedNotification {
                thread_id: thread.to_string(), turn_id: turn.into(), model: "gpt-5.5".into(),
                use_cases: Vec::new(), reasons: Vec::new(), show_buffering_ui: true,
                faster_model: Some("gpt-5.5".into()),
            }), None);
    }

    fn take_retry(&mut self) -> AppEvent {
        std::iter::from_fn(|| self.events.try_recv().ok())
            .find(|event| matches!(event, AppEvent::RetrySafetyBufferedTurn { autonomous: true, .. }))
            .expect("a currently authorized notice queues its retry")
    }
}

fn completed(id: &str) -> ResponseTemplate {
    responses::sse_response(responses::sse(vec![responses::ev_response_created(id), responses::ev_completed(id)]))
}

async fn wait_for_requests(model: &wiremock::MockServer, count: usize) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if model.received_requests().await.unwrap().len() >= count { break; }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }).await.expect("the actual model request reaches the fixture");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_safety_retry_rechecks_revoked_authority_and_current_thread() -> color_eyre::Result<()> {
    for revoked in [false, true] {
        let shared = Box::leak(Box::new(codex_alab_tui::boxes::Shared::new()));
        let _scope = codex_alab_tui::boxes::scoped(shared);
        let model = responses::start_mock_server().await;
        Mock::given(method("POST")).and(path_regex(".*/responses$"))
            .respond_with(completed("held").set_delay(Duration::from_secs(60)))
            .expect(1).mount(&model).await;
        let mut fixture = Fixture::new(&model.uri()).await?;
        fixture.claim(shared);
        let turn = fixture.submit("Preserve the running source").await?;
        wait_for_requests(&model, 1).await;
        while fixture.events.try_recv().is_ok() {}
        fixture.notice(ThreadId::new(), &turn);
        assert!(std::iter::from_fn(|| fixture.events.try_recv().ok()).all(|event|
            !matches!(event, AppEvent::RetrySafetyBufferedTurn { .. })), "another thread cannot queue a retry under an active claim");
        fixture.notice(fixture.thread, &turn);
        let mut retry = fixture.take_retry();
        if revoked {
            shared.set_claim(None);
            assert!(!fixture.app.chat_widget.alab_autonomous_root_active());
        } else if let AppEvent::RetrySafetyBufferedTurn { thread_id, .. } = &mut retry {
            *thread_id = ThreadId::new();
        }
        fixture.app.handle_event(&mut fixture.tui, &mut fixture.server, retry).await?;
        let source = fixture.server.thread_read(fixture.thread, true).await?;
        assert_eq!(source.turns.last().map(|turn| &turn.status), Some(&TurnStatus::InProgress));
        assert_eq!(fixture.app.chat_widget.thread_id(), Some(fixture.thread));
        assert_eq!(model.received_requests().await.unwrap().len(), 1);
        fixture.server.shutdown().await?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn safety_fallback_preserves_an_acknowledged_tool_without_executing_it_again() -> color_eyre::Result<()> {
    let shared = Box::leak(Box::new(codex_alab_tui::boxes::Shared::new()));
    let _scope = codex_alab_tui::boxes::scoped(shared);
    let model = responses::start_mock_server().await;
    let requests = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let observed = Arc::clone(&requests);
    Mock::given(method("POST")).and(path_regex(".*/responses$"))
        .respond_with(move |request: &wiremock::Request| {
            let body: serde_json::Value = request.body_json().unwrap();
            let mut requests = observed.lock().unwrap();
            let index = requests.len();
            requests.push(body.clone());
            match index {
                0 => responses::sse_response(responses::sse(vec![
                    responses::ev_response_created("action"),
                    responses::ev_function_call("acknowledged-action", "update_plan", r#"{"plan":[{"step":"Acknowledged action","status":"completed"}]}"#),
                    responses::ev_completed("action"),
                ])),
                1 => completed("action-acknowledged"),
                2 => completed("blocked-turn").set_delay(Duration::from_secs(60)),
                3 => {
                    assert!(body["input"].as_array().unwrap().iter().any(|item|
                        item["type"] == "function_call_output" && item["call_id"] == "acknowledged-action"),
                        "fallback must give the model the already acknowledged tool result");
                    completed("fallback")
                }
                _ => panic!("fallback repeated model or tool work"),
            }
        }).expect(4).mount(&model).await;
    let mut fixture = Fixture::new(&model.uri()).await?;
    fixture.claim(shared);
    let prior = fixture.submit("Perform the action once").await?;
    assert_eq!(fixture.complete(&prior).await?, 1, "the real plan tool executes once");
    let active = fixture.submit("Continue the remaining work").await?;
    wait_for_requests(&model, 3).await;
    while fixture.events.try_recv().is_ok() {}
    fixture.notice(fixture.thread, &active);
    let retry = fixture.take_retry();
    let source = fixture.server.thread_read(fixture.thread, true).await?;
    std::fs::remove_file(source.path.expect("owned source rollout"))?;
    fixture.app.handle_event(&mut fixture.tui, &mut fixture.server, retry).await?;
    let fallback = fixture.started().await?;
    assert_ne!(fallback, active);
    assert_eq!(fixture.complete(&fallback).await?, 0, "no acknowledged plan action is executed again");
    assert_eq!(requests.lock().unwrap().len(), 4);
    assert_eq!(fixture.app.chat_widget.thread_id(), Some(fixture.thread));
    fixture.server.shutdown().await?;
    Ok(())
}
