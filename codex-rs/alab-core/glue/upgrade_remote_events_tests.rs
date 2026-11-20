// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::AppServerEvent;
use std::collections::VecDeque;
use std::io;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
fn check(value: bool, message: &'static str) -> io::Result<()> {
    if value {
        Ok(())
    } else {
        Err(io::Error::other(message))
    }
}

// Same private owned-facade construction used by existing remote unit controls;
// this tests original buffer ownership, not a WebSocket or Native connection.
#[tokio::test]
async fn original_remote_startup_and_closed_live_queues_keep_order() -> io::Result<()> {
    let (command_tx, commands) = mpsc::channel(1);
    let (events, event_rx) = mpsc::unbounded_channel();
    let (stop, stopped) = oneshot::channel::<()>();
    let mut stop = Some(stop);
    let worker_handle = tokio::spawn(async move {
        let _ = stopped.await;
    });
    let mut client = RemoteAppServerClient {
        command_tx,
        event_rx,
        pending_events: VecDeque::from([AppServerEvent::Disconnected {
            message: "startup".into(),
        }]),
        metadata: Default::default(),
        worker_handle,
    };
    let body = async {
        events.send(AppServerEvent::Disconnected { message: "live".into() }).map_err(io::Error::other)?;
        drop(events);
        drop(commands);
        let Some(signal) = stop.take() else { return Err(io::Error::other("owned worker stop missing")); };
        check(signal.send(()).is_ok(), "owned worker stop was lost")?;
        tokio::time::timeout(Duration::from_secs(60), async {
            while !client.worker_handle.is_finished() { tokio::task::yield_now().await; }
        }).await.map_err(io::Error::other)?;
        for _ in 0..2 { check(client.alab_raw_upgrade_events_pending(), "closed remote queues were consumed or treated as unavailable")?; }
        for expected in ["startup", "live"] {
            let actual = tokio::time::timeout(Duration::from_secs(60), client.next_event()).await.map_err(io::Error::other)?;
            check(matches!(actual, Some(AppServerEvent::Disconnected { message }) if message == expected), "remote startup/live order changed")?;
        }
        check(!client.alab_raw_upgrade_events_pending(), "ordinary remote reads did not empty both original queues")?;
        check(client.next_event().await.is_none(), "ordinary remote EOF changed")?;
        check(!client.alab_raw_upgrade_events_pending(), "closed empty remote queues acquired a liveness veto")
    }.await;
    drop(stop);
    let joined = tokio::time::timeout(Duration::from_secs(60), &mut client.worker_handle)
        .await
        .map_err(io::Error::other)
        .and_then(|result| result.map_err(io::Error::other));
    body.and(joined)
}
