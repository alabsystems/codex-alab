// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::fixture;
use super::goal;
use super::state;
use super::update;
use crate::terminal_program_status::ProgramStatus;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::AlabGoalPersistencePhase as Phase;
use codex_app_server_protocol::JSONRPCMessage;
use codex_protocol::ThreadId;
use futures::SinkExt;
use futures::StreamExt;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

pub(super) async fn exercise() {
    let (mut app, _events, _ops) = fixture::fixture().await;
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let endpoint =
        crate::resolve_remote_addr(&format!("ws://{}", listener.local_addr().unwrap())).unwrap();
    let codex_home = app.config.codex_home.clone();
    let handshake = async {
        let (stream, _) = listener.accept().await.unwrap();
        let mut peer = tokio_tungstenite::accept_async(stream).await.unwrap();
        let message = peer.next().await.unwrap().unwrap().into_text().unwrap();
        let JSONRPCMessage::Request(request) = serde_json::from_str(&message).unwrap() else {
            panic!("expected initialize request");
        };
        assert_eq!(request.method, "initialize");
        let response = serde_json::json!({
            "id": request.id,
            "result": {"userAgent": "goal-disconnect", "codexHome": codex_home}
        });
        peer.send(Message::text(response.to_string()))
            .await
            .unwrap();
        let message = peer.next().await.unwrap().unwrap().into_text().unwrap();
        let JSONRPCMessage::Notification(done) = serde_json::from_str(&message).unwrap() else {
            panic!("expected initialized notification");
        };
        assert_eq!(done.method, "initialized");
        peer
    };
    let (mut session, mut peer) =
        tokio::join!(fixture::connect_remote(&mut app, endpoint), handshake);
    let owner = session.backend_id();
    let thread = ThreadId::new();
    fixture::select(&mut app, owner, thread);
    goal(&mut app.chat_widget, thread);
    state(&app.chat_widget, ProgramStatus::Working);

    // Old-generation and duplicate-sequence Ready packets would both clear Held if accepted.
    for (generation, sequence, phase) in [
        (1, 2, Phase::Held),
        (0, 3, Phase::Ready),
        (1, 2, Phase::Ready),
    ] {
        let packet = update(
            thread,
            ("peer", generation),
            sequence,
            "goal-runtime",
            sequence,
            Some(phase),
            false,
        );
        peer.send(Message::text(serde_json::to_string(&packet).unwrap()))
            .await
            .unwrap();
        let event = session
            .next_event()
            .await
            .expect("expected the goal packet");
        assert_eq!(event.backend_id, owner);
        assert!(
            matches!(&event.event, AppServerEvent::ServerNotification(actual)
            if serde_json::to_value(actual).unwrap() == serde_json::to_value(&packet).unwrap())
        );
        fixture::dispatch(&mut app, &session, owner, event).await;
        state(&app.chat_widget, ProgramStatus::Done);
    }

    assert!(!app.is_offline());
    peer.close(None).await.unwrap();
    let event = session
        .next_event()
        .await
        .expect("expected physical disconnect");
    assert_eq!(event.backend_id, owner);
    assert!(matches!(event.event, AppServerEvent::Disconnected { .. }));
    fixture::dispatch(&mut app, &session, owner, event).await;
    assert!(app.is_offline());
    assert_eq!(app.chat_widget.thread_id(), Some(thread));
    assert!(app.chat_widget.alab_goal_runtime.current().is_none());
    state(&app.chat_widget, ProgramStatus::Working);
}
