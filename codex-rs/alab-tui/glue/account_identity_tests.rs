// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::app::tests::make_test_app_with_channels;
use crate::app_server_session::ThreadParamsMode;
use crate::status::StatusAccountDisplay;
use codex_app_server_protocol::JSONRPCMessage;
use futures::SinkExt;
use futures::StreamExt;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

fn scope(enabled: bool) -> interface::Scope {
    let state = Box::leak(Box::new(interface::State::new()));
    let scope = interface::scoped(state);
    interface::configure(enabled);
    scope
}

fn account(email: Option<&str>) -> Option<StatusAccountDisplay> {
    account_in_workspace(email, None)
}

fn account_in_workspace(
    email: Option<&str>,
    workspace: Option<&str>,
) -> Option<StatusAccountDisplay> {
    Some(StatusAccountDisplay::ChatGpt {
        email: email.map(str::to_string),
        plan: Some("Pro".to_string()),
        workspace: workspace.map(str::to_string),
    })
}

fn selected_workspace(app: &App) -> Option<&str> {
    match app.chat_widget.status_account_display() {
        Some(StatusAccountDisplay::ChatGpt { workspace, .. }) => workspace.as_deref(),
        _ => None,
    }
}

/// An actual connected app-server source, isolated from credentials and bootstrap
/// account data. Every account/read is observed; replies are controlled by the test.
async fn source() -> color_eyre::eyre::Result<(
    AppServerSession,
    mpsc::UnboundedReceiver<serde_json::Value>,
    mpsc::UnboundedSender<serde_json::Value>,
    JoinHandle<()>,
)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr()?))?;
    let (requests_tx, requests_rx) = mpsc::unbounded_channel();
    let (responses_tx, mut responses_rx) = mpsc::unbounded_channel::<serde_json::Value>();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        loop {
            tokio::select! {
                frame = socket.next() => {
                    let Some(Ok(Message::Text(text))) = frame else { break };
                    let Ok(JSONRPCMessage::Request(request)) = serde_json::from_str(&text) else {
                        continue;
                    };
                    if request.method == "account/read" {
                        requests_tx.send(json!({"id": request.id, "params": request.params})).unwrap();
                    } else {
                        let response = if request.method == "initialize" {
                            json!({"id": request.id, "result": {"userAgent": "alab-account-test/1.0.0"}})
                        } else {
                            json!({"id": request.id, "error": {"code": -32601, "message": "unsupported fixture method"}})
                        };
                        if socket.send(Message::Text(response.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                }
                response = responses_rx.recv() => {
                    let Some(response) = response else { break };
                    if socket.send(Message::Text(response.to_string().into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });
    let session = AppServerSession::new(
        crate::backend_id::BackendId::new(),
        crate::connect_remote_app_server(endpoint).await?,
        ThreadParamsMode::Remote,
    );
    Ok((session, requests_rx, responses_tx, server))
}

async fn request(requests: &mut mpsc::UnboundedReceiver<serde_json::Value>) -> serde_json::Value {
    tokio::time::timeout(Duration::from_secs(60), requests.recv())
        .await
        .expect("connected account request deadline")
        .expect("connected account request")
}

async fn loaded(events: &mut mpsc::UnboundedReceiver<AppEvent>) -> AppEvent {
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let event = events.recv().await.expect("App remains open");
            if matches!(event, AppEvent::AccountEmailLoaded { .. }) {
                return event;
            }
        }
    })
    .await
    .expect("bounded identity batch completion")
}

fn success(id: &serde_json::Value, email: &str) -> serde_json::Value {
    json!({"id": id, "result": {
        "account": {"type": "chatgpt", "email": email, "planType": "pro"},
        "requiresOpenaiAuth": true
    }})
}

fn success_in_workspace(
    id: &serde_json::Value,
    email: &str,
    name: &str,
    workspace_id: &str,
) -> serde_json::Value {
    let mut response = success(id, email);
    response["result"]["account"]["workspaceName"] = json!(name);
    response["result"]["workspaceRouting"] = json!({
        "chatgptAccountId": workspace_id,
        "backendOrigin": "https://workspace.example.test",
        "accountRoutingOverride": "NO_CONSTRAINT",
    });
    response
}

fn failure(id: &serde_json::Value) -> serde_json::Value {
    json!({"id": id, "error": {"code": -32000, "message": "temporary account fixture failure"}})
}

#[tokio::test]
async fn connected_identity_read_recovers_after_source_failure_and_publishes_only_verified_email()
-> color_eyre::eyre::Result<()> {
    let _scope = scope(true);
    let (mut app, mut events, mut ops) = make_test_app_with_channels().await;
    app.chat_widget
        .update_account_state(account(None), None, true, true);
    while ops.try_recv().is_ok() {}
    let (mut session, mut requests, responses, server) = source().await?;
    let id = Uuid::new_v4();
    app.account_email_request_id = Some(id);
    assert!(app.alab_start_account_email_read_with_delays(&session, id, [Duration::ZERO; 3]));
    let first = request(&mut requests).await;
    let params: GetAccountParams = serde_json::from_value(first["params"].clone())?;
    assert!(
        !params.refresh_token,
        "identity reads do not refresh credentials"
    );
    responses.send(failure(&first["id"]))?;
    let second = request(&mut requests).await;
    assert_ne!(second["id"], first["id"]);
    assert_eq!(
        interface::status_snapshot().unwrap().account,
        "Account unavailable"
    );
    responses.send(success_in_workspace(
        &second["id"],
        "selected@server.test",
        " Research ",
        "team-a",
    ))?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    app.handle_event(&mut tui, &mut session, loaded(&mut events).await)
        .await?;
    assert_eq!(
        interface::status_snapshot().unwrap().account,
        "selected@server.test"
    );
    assert_eq!(selected_workspace(&app), Some("Research (team-a)"));
    assert!(app.account_email_request_id.is_none());
    assert!(requests.try_recv().is_err());
    assert!(
        ops.try_recv().is_err(),
        "identity refresh cannot submit model work"
    );
    session.shutdown().await?;
    server.abort();
    Ok(())
}

#[tokio::test]
async fn failed_identity_batch_is_bounded_and_periodic_retries_respect_one_flight_and_cooldown()
-> color_eyre::eyre::Result<()> {
    let _scope = scope(true);
    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    app.chat_widget
        .update_account_state(account(None), None, true, true);
    let (mut session, mut requests, responses, server) = source().await?;
    let id = Uuid::new_v4();
    app.account_email_request_id = Some(id);
    app.alab_start_account_email_read_with_delays(&session, id, [Duration::ZERO; 3]);
    let started = app.alab_account_identity.started_at.unwrap();
    for _ in 0..3 {
        let read = request(&mut requests).await;
        app.alab_retry_missing_account_email_at(&session, started + RETRY_COOLDOWN);
        assert_eq!(
            app.account_email_request_id,
            Some(id),
            "a live batch owns its UUID"
        );
        responses.send(failure(&read["id"]))?;
    }
    let mut tui = crate::tui::test_support::make_test_tui()?;
    app.handle_event(&mut tui, &mut session, loaded(&mut events).await)
        .await?;
    assert!(app.account_email_request_id.is_none());
    assert_eq!(
        interface::status_snapshot().unwrap().account,
        "Account unavailable"
    );
    app.alab_retry_missing_account_email_at(
        &session,
        started + RETRY_COOLDOWN - Duration::from_nanos(1),
    );
    assert!(
        app.account_email_request_id.is_none(),
        "cooldown is strict before its deadline"
    );
    assert!(requests.try_recv().is_err());
    app.alab_retry_missing_account_email_at(&session, started + RETRY_COOLDOWN);
    let next_id = app.account_email_request_id.unwrap();
    assert_ne!(next_id, id);
    let retry = request(&mut requests).await;
    responses.send(success(&retry["id"], "recovered@server.test"))?;
    app.handle_event(&mut tui, &mut session, loaded(&mut events).await)
        .await?;
    assert_eq!(
        interface::status_snapshot().unwrap().account,
        "recovered@server.test"
    );
    session.shutdown().await?;
    server.abort();
    Ok(())
}

#[tokio::test]
async fn disconnect_and_account_changes_cancel_retries_and_reject_late_identity_results()
-> color_eyre::eyre::Result<()> {
    let _scope = scope(true);
    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    let (mut session, mut requests, responses, server) = source().await?;
    app.app_server_target = crate::AppServerTarget::Remote {
        endpoint: crate::resolve_remote_addr("ws://127.0.0.1:8765")?,
    };
    app.chat_widget
        .update_account_state(account(None), None, true, true);
    app.refresh_account_email(&session);
    let first_id = app.account_email_request_id.unwrap();
    let first = request(&mut requests).await;
    assert!(app.begin_reconnect());
    assert!(app.alab_account_identity.task.is_none());
    assert!(app.account_email_request_id.is_none());
    app.alab_retry_missing_account_email_at(&session, Instant::now() + RETRY_COOLDOWN);
    assert!(
        app.account_email_request_id.is_none(),
        "offline identity reads are suppressed"
    );
    responses.send(success(&first["id"], "old@server.test"))?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    app.handle_event(
        &mut tui,
        &mut session,
        AppEvent::AccountEmailLoaded {
            request_id: first_id,
            email: Some("old@server.test".into()),
            workspace: Some("Old (old-team)".into()),
        },
    )
    .await?;
    assert_eq!(
        interface::status_snapshot().unwrap().account,
        "Account unavailable"
    );
    assert_eq!(selected_workspace(&app), None);

    app.reconnect.offline = false;
    app.chat_widget
        .update_account_state(account(None), None, true, true);
    app.refresh_account_email(&session);
    let second_id = app.account_email_request_id.unwrap();
    let _second = request(&mut requests).await;
    app.handle_app_server_event(
        &session,
        codex_app_server_client::AppServerEvent::ServerNotification(Box::new(
            codex_app_server_protocol::ServerNotification::AccountUpdated(
                codex_app_server_protocol::AccountUpdatedNotification {
                    auth_mode: Some(codex_app_server_protocol::AuthMode::Chatgpt),
                    plan_type: None,
                },
            ),
        )),
    )
    .await;
    let third_id = app.account_email_request_id.unwrap();
    assert_ne!(third_id, second_id);
    let third = request(&mut requests).await;
    app.handle_event(
        &mut tui,
        &mut session,
        AppEvent::AccountEmailLoaded {
            request_id: second_id,
            email: Some("replaced@server.test".into()),
            workspace: Some("Replaced (replaced-team)".into()),
        },
    )
    .await?;
    assert_eq!(
        interface::status_snapshot().unwrap().account,
        "Account unavailable"
    );
    assert_eq!(selected_workspace(&app), None);
    responses.send(success_in_workspace(
        &third["id"],
        "current@server.test",
        "Current",
        "current-team",
    ))?;
    app.handle_event(&mut tui, &mut session, loaded(&mut events).await)
        .await?;
    assert_eq!(
        interface::status_snapshot().unwrap().account,
        "current@server.test"
    );
    assert_eq!(selected_workspace(&app), Some("Current (current-team)"));
    session.shutdown().await?;
    server.abort();
    Ok(())
}

#[tokio::test]
async fn failed_reads_preserve_verified_workspace_until_current_response_replaces_or_clears_it()
-> color_eyre::eyre::Result<()> {
    let _scope = scope(true);
    let (mut app, mut events, mut ops) = make_test_app_with_channels().await;
    app.chat_widget.update_account_state(
        account_in_workspace(None, Some("Bootstrap (bootstrap-team)")),
        None,
        true,
        true,
    );
    while ops.try_recv().is_ok() {}
    let (mut session, mut requests, responses, server) = source().await?;
    let id = Uuid::new_v4();
    app.account_email_request_id = Some(id);
    app.alab_start_account_email_read_with_delays(&session, id, [Duration::ZERO; 3]);
    for _ in 0..3 {
        let read = request(&mut requests).await;
        responses.send(failure(&read["id"]))?;
    }
    let mut tui = crate::tui::test_support::make_test_tui()?;
    app.handle_event(&mut tui, &mut session, loaded(&mut events).await)
        .await?;
    assert!(app.account_email_request_id.is_none());
    assert_eq!(selected_workspace(&app), Some("Bootstrap (bootstrap-team)"));
    assert!(requests.try_recv().is_err());

    let blank_id = Uuid::new_v4();
    app.account_email_request_id = Some(blank_id);
    app.alab_start_account_email_read_with_delays(&session, blank_id, [Duration::ZERO; 3]);
    let blank = request(&mut requests).await;
    responses.send(success_in_workspace(
        &blank["id"],
        "  ",
        "Fresh",
        "fresh-team",
    ))?;
    app.handle_event(&mut tui, &mut session, loaded(&mut events).await)
        .await?;
    assert_eq!(selected_workspace(&app), Some("Fresh (fresh-team)"));
    assert_eq!(
        interface::status_snapshot().unwrap().account,
        "Account unavailable"
    );

    let absent_id = Uuid::new_v4();
    app.account_email_request_id = Some(absent_id);
    app.alab_start_account_email_read_with_delays(&session, absent_id, [Duration::ZERO; 3]);
    let absent = request(&mut requests).await;
    responses.send(success(&absent["id"], "current@server.test"))?;
    app.handle_event(&mut tui, &mut session, loaded(&mut events).await)
        .await?;
    assert_eq!(selected_workspace(&app), None);
    assert_eq!(
        interface::status_snapshot().unwrap().account,
        "current@server.test"
    );
    assert!(app.account_email_request_id.is_none());
    assert!(
        ops.try_recv().is_err(),
        "identity reads cannot submit model work"
    );
    session.shutdown().await?;
    server.abort();
    Ok(())
}

#[tokio::test]
async fn off_runs_native_one_shot_and_a_later_toggle_can_replace_its_stranded_request()
-> color_eyre::eyre::Result<()> {
    let _scope = scope(false);
    let (mut app, _events, _ops) = make_test_app_with_channels().await;
    app.chat_widget
        .update_account_state(account(None), None, true, true);
    let (session, mut requests, responses, server) = source().await?;
    app.refresh_account_email(&session);
    let old_id = app.account_email_request_id.unwrap();
    let started = app.alab_account_identity.started_at.unwrap();
    let original = request(&mut requests).await;
    assert!(
        app.alab_account_identity.task.is_none(),
        "Off keeps the native one-shot reader"
    );
    responses.send(failure(&original["id"]))?;
    app.alab_retry_missing_account_email_at(&session, started + RETRY_COOLDOWN);
    assert_eq!(app.account_email_request_id, Some(old_id));
    interface::set_enabled(true);
    app.alab_retry_missing_account_email_at(&session, started + READ_TIMEOUT);
    assert_eq!(
        app.account_email_request_id,
        Some(old_id),
        "do not overlap the native request"
    );
    app.alab_retry_missing_account_email_at(&session, started + RETRY_COOLDOWN);
    assert_ne!(app.account_email_request_id, Some(old_id));
    let retry = request(&mut requests).await;
    responses.send(success(&retry["id"], "later@server.test"))?;
    app.alab_cancel_account_email_read();
    session.shutdown().await?;
    server.abort();
    Ok(())
}
