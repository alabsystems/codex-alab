// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::AppServerClient;
use crate::AppServerEvent;
use codex_app_server_protocol::ErrorNotification;
use codex_app_server_protocol::TurnError;
use std::time::Duration;
fn check(value: bool, message: &'static str) -> io::Result<()> {
    if value {
        Ok(())
    } else {
        Err(io::Error::other(message))
    }
}

#[tokio::test]
async fn actual_cloud_closed_queue_is_observed_until_ordinary_delivery_reaches_empty_eof()
-> io::Result<()> {
    let (cloud, commands, events) = CloudClient::channel(2);
    let mut client = AppServerClient::Cloud(cloud);
    check(
        !client.alab_upgrade_events_pending()?,
        "empty original Cloud queue was pending",
    )?;
    for text in ["first", "second"] {
        events
            .send(AppServerEvent::Disconnected {
                message: text.into(),
            })
            .map_err(io::Error::other)?;
    }
    drop(events);
    drop(commands);
    for _ in 0..2 {
        check(
            client.alab_upgrade_events_pending()?,
            "closed Cloud buffer was consumed or treated as unavailable",
        )?;
    }
    for expected in ["first", "second"] {
        let actual = tokio::time::timeout(Duration::from_secs(60), client.next_event())
            .await
            .map_err(io::Error::other)?;
        check(
            matches!(actual, Some(AppServerEvent::Disconnected { message }) if message == expected),
            "Cloud event order changed",
        )?;
    }
    check(
        !client.alab_upgrade_events_pending()?,
        "ordinary Cloud delivery did not empty its queue",
    )?;
    check(
        client.next_event().await.is_none(),
        "ordinary Cloud EOF changed",
    )?;
    check(
        !client.alab_upgrade_events_pending()?,
        "closed empty Cloud queue acquired a liveness veto",
    )
}

#[tokio::test]
async fn actual_cloud_loss_publication_window_filter_and_uncertainty_remain_original()
-> io::Result<()> {
    let (mut client, commands, events) = CloudClient::channel(2);
    drop(commands);
    client.stream_losses.record("owned-thread");
    check(
        client.events.is_empty(),
        "loss window unexpectedly had a published event",
    )?;
    check(
        client.alab_raw_upgrade_events_pending()?,
        "record-before-send loss window was missed",
    )?;
    client.stream_losses.supersede("owned-thread");
    check(
        client.alab_raw_upgrade_events_pending()?,
        "superseded but unread original loss was consumed",
    )?;
    {
        let guard = client
            .stream_losses
            .0
            .lock()
            .map_err(|_| io::Error::other("owned original loss lock was poisoned early"))?;
        check(
            matches!(client.alab_raw_upgrade_events_pending(), Err(error) if error.kind() == io::ErrorKind::WouldBlock),
            "busy original accounting did not fail closed",
        )?;
        drop(guard);
    }
    events
        .send(AppServerEvent::ThreadStreamDisconnected(Box::new(
            ErrorNotification {
                thread_id: "owned-thread".into(),
                turn_id: String::new(),
                will_retry: false,
                error: TurnError {
                    message: "original loss".into(),
                    additional_details: None,
                    codex_error_info: None,
                    misalignment: None,
                },
            },
        )))
        .map_err(io::Error::other)?;
    events
        .send(AppServerEvent::Disconnected {
            message: "after-filter".into(),
        })
        .map_err(io::Error::other)?;
    drop(events);
    check(
        client.alab_raw_upgrade_events_pending()?,
        "closed event producer skipped original loss accounting",
    )?;
    let actual = tokio::time::timeout(Duration::from_secs(60), client.next_event())
        .await
        .map_err(io::Error::other)?;
    check(
        matches!(actual, Some(AppServerEvent::Disconnected { message }) if message == "after-filter"),
        "ordinary Cloud superseded-loss filtering changed",
    )?;
    check(
        !client.alab_raw_upgrade_events_pending()?,
        "ordinary Cloud filter did not retire its own accounting",
    )?;
    let original = client.stream_losses.0.clone();
    let poisoned = std::thread::spawn(move || {
        if let Ok(_guard) = original.lock() {
            panic!("owned loss-accounting poison control");
        }
    })
    .join();
    check(poisoned.is_err(), "owned poison control did not run")?;
    check(
        matches!(client.alab_raw_upgrade_events_pending(), Err(error) if error.kind() == io::ErrorKind::Other),
        "closed producer bypassed poisoned Cloud accounting",
    )
}

// Deliberately corrupt only the fixture's accounting. The observer must refuse
// without repairing it or taking the original events from their ordinary reader.
#[tokio::test]
async fn malformed_cloud_accounting_refuses_without_repairing_or_taking_original_delivery()
-> io::Result<()> {
    for (remaining, superseded) in [(0, 0), (1, 2)] {
        let (cloud, commands, events) = CloudClient::channel(2);
        drop(commands);
        let accounting = cloud.stream_losses.0.clone();
        let mut original_events = Vec::new();
        for thread_id in ["damaged-thread", "untouched-thread"] {
            cloud.stream_losses.record(thread_id);
            let error = Box::new(ErrorNotification {
                thread_id: thread_id.into(),
                turn_id: String::new(),
                will_retry: false,
                error: TurnError {
                    message: format!("original loss for {thread_id}"),
                    additional_details: None,
                    codex_error_info: None,
                    misalignment: None,
                },
            });
            original_events.push((thread_id, std::ptr::from_ref(error.as_ref()) as usize));
            events
                .send(AppServerEvent::ThreadStreamDisconnected(error))
                .map_err(io::Error::other)?;
        }
        drop(events);
        {
            let mut entries = accounting
                .lock()
                .map_err(|_| io::Error::other("fixture accounting was poisoned"))?;
            let damaged = entries
                .get_mut("damaged-thread")
                .ok_or_else(|| io::Error::other("original damaged-thread entry was missing"))?;
            damaged.remaining = remaining;
            damaged.superseded = superseded;
        }
        let snapshot = || -> io::Result<std::collections::BTreeMap<String, (usize, usize)>> {
            let entries = accounting
                .lock()
                .map_err(|_| io::Error::other("fixture accounting was poisoned"))?;
            Ok(entries
                .iter()
                .map(|(thread_id, entry)| (thread_id.clone(), (entry.remaining, entry.superseded)))
                .collect())
        };
        let original_accounting = snapshot()?;
        let mut client = AppServerClient::Cloud(cloud);
        for _ in 0..2 {
            check(
                matches!(client.alab_upgrade_events_pending(), Err(error) if error.kind() == io::ErrorKind::InvalidData),
                "malformed original accounting did not refuse the facade observation",
            )?;
            check(
                snapshot()? == original_accounting,
                "refused observation changed original Cloud accounting",
            )?;
        }
        // The ordinary consumer assumes valid counters. Restore the deliberate
        // corruption here, after proving refusal left it unchanged; this is a
        // fixture repair, not an observer recovery policy or malformed delivery.
        {
            let mut entries = accounting
                .lock()
                .map_err(|_| io::Error::other("fixture accounting was poisoned"))?;
            let damaged = entries
                .get_mut("damaged-thread")
                .ok_or_else(|| io::Error::other("refusal lost the original entry"))?;
            damaged.remaining = 1;
            damaged.superseded = 0;
        }
        check(
            client.alab_upgrade_events_pending()?,
            "fixture repair lost the original pending delivery",
        )?;
        for (thread_id, address) in original_events {
            let actual = tokio::time::timeout(Duration::from_secs(60), client.next_event())
                .await
                .map_err(io::Error::other)?;
            check(
                matches!(actual, Some(AppServerEvent::ThreadStreamDisconnected(error))
                    if error.thread_id == thread_id
                        && std::ptr::from_ref(error.as_ref()) as usize == address),
                "ordinary reader did not deliver the same original loss in order",
            )?;
        }
        check(
            snapshot()?.is_empty(),
            "ordinary Cloud reader did not retire its original accounting",
        )?;
        check(
            !client.alab_upgrade_events_pending()?,
            "ordinary delivery did not leave known-empty Cloud state",
        )?;
        check(
            client.next_event().await.is_none(),
            "closed original Cloud stream did not reach EOF",
        )?;
    }
    Ok(())
}
