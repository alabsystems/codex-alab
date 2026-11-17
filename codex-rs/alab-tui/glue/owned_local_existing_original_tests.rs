// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Lower original-object/one-shot ownership controls. These construct no Native
//! frontier, Pending, Source acceptance, replay or positive UI receipt.
use super::*;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

struct OriginalValue {
    identity: Arc<()>,
    drops: Arc<AtomicUsize>,
}
impl Drop for OriginalValue {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn cancelled_original_callback_restores_the_same_owned_object() -> Result<()> {
    let identity = Arc::new(());
    let drops = Arc::new(AtomicUsize::new(0));
    let payload = Arc::new(Payload::new(OriginalValue {
        identity: identity.clone(),
        drops: drops.clone(),
    }));
    let (entered, ready) = tokio::sync::oneshot::channel();
    let worker_payload = payload.clone();
    let worker = tokio::spawn(async move {
        let _actual = worker_payload.take()?;
        entered
            .send(())
            .map_err(|_| eyre!("callback observer left"))?;
        std::future::pending::<()>().await;
        Ok::<(), color_eyre::eyre::Report>(())
    });
    let observed = tokio::time::timeout(std::time::Duration::from_secs(5), ready).await;
    worker.abort();
    let joined = worker.await;
    observed??;
    assert!(joined.is_err_and(|error| error.is_cancelled()));
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    let original = payload.take()?.into_value()?;
    assert!(Arc::ptr_eq(&original.identity, &identity));
    drop(original);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn real_callback_slot_delivers_full_original_request_once_and_cannot_rearm() -> Result<()> {
    let params: ThreadResumeParams = serde_json::from_value(serde_json::json!({
        "threadId": "actual-original-thread-data",
        "model": "chosen-model", "modelProvider": "chosen-provider",
        "cwd": "/original/project", "excludeTurns": true,
        "config": {"model_reasoning_effort":"high", "mcp_servers":{"actual":{"command":"original"}}}
    }))?;
    let request = ClientRequest::ThreadResume {
        request_id: RequestId::String("original-id".into()),
        params,
    };
    let exact = serde_json::to_value(&request)?;
    let mut callback = CallbackOnce::empty();
    callback.arm(request)?;
    assert!(!callback.consumed());
    let delivered = callback.take()?;
    assert_eq!(serde_json::to_value(&delivered)?, exact);
    assert!(callback.consumed());
    assert!(callback.take().is_err());
    assert!(callback.arm(delivered).is_err());
    assert!(callback.consumed());
    Ok(())
}

#[test]
fn failed_second_arming_keeps_original_owned_value_until_single_consumption() -> Result<()> {
    let identity = Arc::new(());
    let drops = Arc::new(AtomicUsize::new(0));
    let mut callback = CallbackOnce::empty();
    callback.arm(OriginalValue {
        identity: identity.clone(),
        drops: drops.clone(),
    })?;
    let second = Arc::new(());
    assert!(
        callback
            .arm(OriginalValue {
                identity: second,
                drops: drops.clone()
            })
            .is_err()
    );
    assert_eq!(
        drops.load(Ordering::SeqCst),
        1,
        "only denied second value was dropped"
    );
    let original = callback.take()?;
    assert!(Arc::ptr_eq(&original.identity, &identity));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    drop(original);
    assert_eq!(drops.load(Ordering::SeqCst), 2);
    Ok(())
}

#[test]
fn late_accepted_value_is_retained_without_clearing_unknown() {
    let identity = Arc::new(());
    let drops = Arc::new(AtomicUsize::new(0));
    let mut custody = LateValue::empty();
    let observer_unknown = true;
    assert!(!custody.capture(
        OriginalValue {
            identity: identity.clone(),
            drops: drops.clone()
        },
        observer_unknown
    ));
    assert!(observer_unknown);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert!(
        custody
            .value
            .as_ref()
            .is_some_and(|value| Arc::ptr_eq(&value.identity, &identity))
    );
    drop(custody);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn conflicting_returned_value_quarantines_both_actual_resources() {
    let drops = Arc::new(AtomicUsize::new(0));
    let first = Arc::new(());
    let second = Arc::new(());
    let mut custody = LateValue::empty();
    assert!(custody.capture(
        OriginalValue {
            identity: first.clone(),
            drops: drops.clone()
        },
        false
    ));
    assert!(!custody.capture(
        OriginalValue {
            identity: second.clone(),
            drops: drops.clone()
        },
        false
    ));
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert!(
        custody
            .value
            .as_ref()
            .is_some_and(|value| Arc::ptr_eq(&value.identity, &first))
    );
    assert_eq!(custody.quarantined.len(), 1);
    assert!(Arc::ptr_eq(&custody.quarantined[0].identity, &second));
    drop(custody);
    assert_eq!(drops.load(Ordering::SeqCst), 2);
}
