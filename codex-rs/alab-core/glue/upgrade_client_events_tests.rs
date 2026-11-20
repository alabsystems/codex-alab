// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::InProcessServerEvent;
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

// Actual facade value and actual original queue. The held fixture worker is not
// an embedded-server or Native-positive claim; it only owns its normal lifetime.
#[tokio::test]
async fn original_inprocess_queue_observation_preserves_closed_buffer_and_empty_eof()
-> io::Result<()> {
    let (command_tx, commands) = mpsc::channel(1);
    let (events, event_rx) = mpsc::unbounded_channel();
    let (stop, stopped) = oneshot::channel::<()>();
    let mut stop = Some(stop);
    let worker_handle = tokio::spawn(async move {
        let _ = stopped.await;
    });
    let mut client = InProcessAppServerClient {
        command_tx,
        event_rx,
        worker_handle,
    };
    let body = async {
        check(!client.alab_raw_upgrade_events_pending(), "empty actual in-process queue was pending")?;
        for skipped in [7, 11] { events.send(InProcessServerEvent::Lagged { skipped }).map_err(io::Error::other)?; }
        drop(events);
        drop(commands);
        let Some(signal) = stop.take() else { return Err(io::Error::other("owned worker stop missing")); };
        check(signal.send(()).is_ok(), "owned worker stop was lost")?;
        tokio::time::timeout(Duration::from_secs(60), async {
            while !client.worker_handle.is_finished() { tokio::task::yield_now().await; }
        }).await.map_err(io::Error::other)?;
        for _ in 0..2 { check(client.alab_raw_upgrade_events_pending(), "closed in-process buffers were consumed or treated as unavailable")?; }
        for expected in [7, 11] {
            let actual = tokio::time::timeout(Duration::from_secs(60), client.next_event()).await.map_err(io::Error::other)?;
            check(matches!(actual, Some(InProcessServerEvent::Lagged { skipped }) if skipped == expected), "ordinary in-process delivery order changed")?;
        }
        check(!client.alab_raw_upgrade_events_pending(), "ordinary in-process delivery did not empty its queue")?;
        check(client.next_event().await.is_none(), "ordinary in-process EOF changed")?;
        check(!client.alab_raw_upgrade_events_pending(), "closed empty in-process queue acquired a liveness veto")
    }.await;
    drop(stop);
    let joined = tokio::time::timeout(Duration::from_secs(60), &mut client.worker_handle)
        .await
        .map_err(io::Error::other)
        .and_then(|result| result.map_err(io::Error::other));
    body.and(joined)
}
