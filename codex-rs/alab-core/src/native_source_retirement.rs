// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Last-use retirement owns its immutable metadata until the native Closed ACK.

use crate::managed_auth::unavailable;
use crate::native_source::Member;
use crate::native_source::Retirement;
use std::io;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio::sync::Notify;

struct State {
    closed: AtomicBool,
    changed: Notify,
}
/// Dropping a waiter cannot cancel native retirement or turn uncertainty into Closed.
#[derive(Clone)]
pub struct Receipt(Arc<State>);
impl std::fmt::Debug for Receipt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeRetirementReceipt(<private>)")
    }
}
impl Receipt {
    pub async fn closed(&self) {
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.0.closed.load(Ordering::Acquire) {
                return;
            }
            changed.await;
        }
    }
}
pub fn start(
    member: Arc<dyn Member>,
    preload_operation: Option<String>,
    registration: Option<String>,
) -> io::Result<Receipt> {
    let request = Retirement {
        operation: format!("{:032x}", rand::random::<u128>()),
        preload_operation,
        registration,
    };
    start_request(member, request)
}
fn start_request(member: Arc<dyn Member>, request: Retirement) -> io::Result<Receipt> {
    request.validate()?;
    let state = Arc::new(State {
        closed: AtomicBool::new(false),
        changed: Notify::new(),
    });
    let actor = Arc::clone(&state);
    tokio::runtime::Handle::try_current()
        .map_err(|_| unavailable())?
        .spawn(async move {
            loop {
                if member.retire_captured(&request).await.is_ok() {
                    actor.closed.store(true, Ordering::Release);
                    actor.changed.notify_waiters();
                    return;
                }
                // The retained release-only proof, not current Ready or foreground, is
                // used by the native adapter. Even permanent refusal is not a Closed ACK.
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        });
    Ok(Receipt(state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_source::Borrow;
    use crate::native_source::Loaded;
    use crate::native_source::PreloadFuture;
    use crate::native_source::ReadyFuture;
    use crate::native_source::UnitFuture;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Peer {
        requests: Mutex<Vec<Retirement>>,
        waiting: Notify,
        release: Notify,
    }
    impl Member for Peer {
        fn id(&self) -> &str {
            "11111111111111111111111111111111"
        }
        fn live(&self) -> bool {
            false
        }
        fn preload<'a>(&'a self, _: &'a str, _: Option<&'a Borrow>) -> PreloadFuture<'a> {
            Box::pin(async { Err(unavailable()) })
        }
        fn ready<'a>(&'a self, _: &'a str, _: Option<&'a str>, _: &'a Loaded) -> ReadyFuture<'a> {
            Box::pin(async { Err(unavailable()) })
        }
        fn retire<'a>(&'a self, _: Option<&'a str>) -> UnitFuture<'a> {
            Box::pin(async { panic!("owned preload retirement must not fall back to legacy") })
        }
        fn retire_captured<'a>(&'a self, request: &'a Retirement) -> UnitFuture<'a> {
            Box::pin(async move {
                let first = {
                    let mut requests = self.requests.lock().unwrap();
                    requests.push(request.clone());
                    requests.len() == 1
                };
                if first {
                    return Err(io::ErrorKind::BrokenPipe.into());
                }
                self.waiting.notify_one();
                self.release.notified().await;
                Ok(())
            })
        }
    }
    #[tokio::test]
    async fn native_source_retirement_retries_exact_metadata_after_eof_and_cancelled_waiter() {
        let peer = Arc::new(Peer::default());
        let receipt = start(peer.clone(), Some("b".repeat(32)), None).unwrap();
        let waiter = receipt.clone();
        let cancelled = tokio::spawn(async move { waiter.closed().await });
        cancelled.abort();
        peer.waiting.notified().await;
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), receipt.closed())
                .await
                .is_err()
        );
        let requests = peer.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].operation, requests[1].operation);
        assert_eq!(requests[0].preload_operation, Some("b".repeat(32)));
        assert_eq!(requests[0].preload_operation, requests[1].preload_operation);
        assert_eq!(requests[0].registration, None);
        assert_eq!(requests[1].registration, None);
        assert!(!format!("{:?}", requests[0]).contains(&requests[0].operation));
        peer.release.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(2), receipt.closed())
            .await
            .unwrap();
    }
    #[test]
    fn native_source_retirement_without_runtime_cannot_claim_closed_or_send() {
        let peer = Arc::new(Peer::default());
        assert!(start(peer.clone(), Some("b".repeat(32)), None).is_err());
        assert!(peer.requests.lock().unwrap().is_empty());
    }
}
