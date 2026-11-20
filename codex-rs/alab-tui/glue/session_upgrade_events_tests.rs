// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! The configured FIFO has already passed the original physical update path.
use super::*;
use crate::app_server_session::ThreadParamsMode;
use crate::backend_id::BackendId;
use codex_app_server_client::AppServerClient;
use codex_app_server_client::AppServerEvent;
use codex_app_server_client::CloudClient;
use codex_app_server_client::CloudOperation;
use std::time::Duration;
fn failure(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
fn check(value: bool, message: &'static str) -> io::Result<()> {
    if value { Ok(()) } else { Err(failure(message)) }
}

#[cfg(unix)]
#[tokio::test]
async fn actual_configured_fifo_observation_keeps_once_received_event_and_rejects_request_view()
-> io::Result<()> {
    let (client, mut commands, events) = CloudClient::channel(2);
    let mut session = AppServerSession::new(
        BackendId::new(),
        AppServerClient::Cloud(client),
        ThreadParamsMode::Cloud,
    );
    let body = async {
        events.send(AppServerEvent::Disconnected { message: "already-physical".into() }).map_err(failure)?;
        let event = tokio::time::timeout(Duration::from_secs(60), session.alab_next_physical_event()).await.map_err(failure)?
            .ok_or_else(|| failure("actual physical event missing"))?;
        // The real configured-tail helper uses this exact FIFO after the same
        // physical receive. No receipt, configured admission or Native grant.
        session.alab_configured_events.push_back(event);
        drop(events);
        for _ in 0..2 { check(session.alab_upgrade_events_pending()?, "configured FIFO was consumed or missed")?; }
        let view = session.request_view();
        check(view.alab_upgrade_events_pending().is_err(), "request-only view supplied event ownership")?;
        drop(view);
        let event = tokio::time::timeout(Duration::from_secs(60), session.next_event()).await.map_err(failure)?;
        check(matches!(event, Some(crate::app_server_session::AppServerSessionEvent { event: AppServerEvent::Disconnected { message }, .. }) if message == "already-physical"), "configured event was replayed through another physical receive or changed")?;
        check(!session.alab_upgrade_events_pending()?, "closed empty Session queue acquired a liveness veto")
    }.await;
    let serve = async {
        while let Some(command) = commands.recv().await {
            if matches!(&command.operation, CloudOperation::Shutdown) {
                command
                    .response
                    .send(Ok(Ok(serde_json::Value::Null)))
                    .map_err(|_| failure("original shutdown response lost"))?;
                return Ok::<_, io::Error>(());
            }
            let _ = command
                .response
                .send(Err(failure("owned configured queue cleanup")));
        }
        Err(failure("original shutdown command absent"))
    };
    let cleanup = tokio::time::timeout(Duration::from_secs(60), async {
        tokio::join!(session.shutdown(), serve)
    })
    .await
    .map_err(failure);
    let cleanup = match cleanup {
        Ok((closed, served)) => closed.and(served),
        Err(error) => Err(error),
    };
    body.and(cleanup)
}
