// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

struct Request {
    method: String,
    _id: String,
    _params: String,
}

fn method(request: &Request) -> &str {
    &request.method
}

fn requests() -> Mutex<Vec<Request>> {
    Mutex::new(
        (0..64)
            .map(|index| Request {
                method: if index % 2 == 0 {
                    "thread/start".into()
                } else {
                    "private-method\nprivate-token".into()
                },
                _id: "private-id".into(),
                _params: "private-params".into(),
            })
            .collect(),
    )
}

#[tokio::test]
async fn snapshot_is_bounded_private_and_does_not_consume_events_or_tasks()
-> Result<(), Box<dyn Error>> {
    let creation = tokio::spawn(async { 42 });
    let proxy = tokio::spawn(async { Err::<(), _>("private-task-error") });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !creation.is_finished() || !proxy.is_finished() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await?;
    let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
    sender.send("buffered-event")?;
    drop(sender);
    let snapshot = registration(&creation, &proxy, &events, &requests(), method);
    assert!(snapshot.creation_finished && snapshot.proxy_finished);
    assert!(snapshot.events_closed);
    assert_eq!(snapshot.queued_events, 1);
    assert_eq!(snapshot.recorded_count, Some(64));
    assert_eq!(snapshot.recent_methods.len(), RECENT_METHODS);
    let text = snapshot.to_string();
    assert!(text.len() < 1024);
    assert!(!text.contains("private-"));
    assert_eq!(events.recv().await, Some("buffered-event"));
    assert_eq!(creation.await?, 42);
    assert_eq!(proxy.await?, Err("private-task-error"));
    Ok(())
}

#[tokio::test]
async fn a_busy_recorder_does_not_block_the_timeout_diagnostic() -> Result<(), Box<dyn Error>> {
    let (received, creation, proxy) = {
        let requests = Arc::new(requests());
        let guard = requests
            .lock()
            .map_err(|_| "unexpected poisoned recorder")?;
        let creation = tokio::spawn(std::future::pending::<()>());
        let proxy = tokio::spawn(std::future::pending::<()>());
        let (_sender, events) = tokio::sync::mpsc::unbounded_channel::<()>();
        let observed = Arc::clone(&requests);
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let snapshot = registration(&creation, &proxy, &events, &observed, method);
            let _ = sender.send(snapshot.to_string());
            (creation, proxy)
        });
        // Release even after failure, so a blocking-lock mutation can be joined.
        let received = receiver.recv_timeout(Duration::from_secs(2));
        drop(guard);
        let (creation, proxy) = worker.join().map_err(|_| "diagnostic worker panicked")?;
        (received, creation, proxy)
    };
    creation.abort();
    proxy.abort();
    assert!(creation.await.is_err());
    assert!(proxy.await.is_err());
    let text = received?;
    assert!(text.contains("recorder=busy"));
    assert!(text.contains("recorded_count=unavailable"));
    Ok(())
}

#[tokio::test]
async fn a_poisoned_recorder_is_reported_without_unwinding() -> Result<(), Box<dyn Error>> {
    let requests = Arc::new(requests());
    let poisoned = Arc::clone(&requests);
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoned
                .lock()
                .unwrap_or_else(|_| panic!("unexpected poison"));
            panic!("fixture poisons its recorder");
        })
        .join()
        .is_err()
    );
    let creation = tokio::spawn(async {});
    let proxy = tokio::spawn(async {});
    let (_sender, events) = tokio::sync::mpsc::unbounded_channel::<()>();
    let text = registration(&creation, &proxy, &events, &requests, method).to_string();
    assert!(text.contains("recorder=poisoned"));
    assert!(text.contains("recorded_count=unavailable"));
    creation.await?;
    proxy.await?;
    Ok(())
}

#[tokio::test]
async fn the_hook_preserves_the_original_failure_prefix() -> Result<(), Box<dyn Error>> {
    let creation = tokio::spawn(async {});
    let proxy = tokio::spawn(async {});
    let (_sender, events) = tokio::sync::mpsc::unbounded_channel::<()>();
    let requests = requests();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::mcp_registration_timeout!(creation, proxy, events, requests);
    }));
    let payload = failure.err().ok_or("the hook must panic")?;
    let text = payload.downcast_ref::<String>().ok_or("string panic")?;
    assert!(text.starts_with("timed out waiting for MCP task registration: "));
    assert!(text.contains("non-atomic snapshot after timeout selection"));
    assert!(!text.contains("private-"));
    creation.await?;
    proxy.await?;
    Ok(())
}
