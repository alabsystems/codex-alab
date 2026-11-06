// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

const SLOT: &str = "736c6f742f6f6e65";
const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct HostFixture {
    requests: Mutex<Vec<Request>>,
    failures: AtomicUsize,
    entered: tokio::sync::Notify,
    block_first: bool,
    barrier: Option<tokio::sync::Barrier>,
}

impl HostFixture {
    fn new(failures: usize) -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::default(),
            failures: AtomicUsize::new(failures),
            entered: tokio::sync::Notify::new(),
            block_first: false,
            barrier: None,
        })
    }

    fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

impl Host for HostFixture {
    fn refresh<'a>(&'a self, request: &'a Request) -> RefreshFuture<'a> {
        Box::pin(async move {
            let first = {
                let mut requests = self.requests.lock().unwrap();
                requests.push(request.clone());
                requests.len() == 1
            };
            self.entered.notify_one();
            if self.block_first && first {
                return std::future::pending().await;
            }
            tokio::task::yield_now().await;
            if let Some(barrier) = &self.barrier {
                barrier.wait().await;
            }
            if self
                .failures
                .compare_exchange(1, 0, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return Err(io::Error::other("secret fixture error must not escape"));
            }
            Ok(Revision {
                generation: 2,
                digest: request.expected_digest.clone(),
            })
        })
    }
}

#[test]
fn explicit_slot_binding_is_strict_and_unmanaged_auth_stays_native() {
    assert_eq!(binding_of(true, None, false).unwrap(), None);
    assert_eq!(
        binding_of(false, Some(OsStr::new("bad")), true).unwrap(),
        None
    );
    assert_eq!(
        binding_of(true, Some(OsStr::new(SLOT)), true).unwrap(),
        Some(SLOT.into())
    );
    for value in ["", "aaA0", "abc", "00", "ff", "2e0a", "736c6f74\n"] {
        assert!(
            binding_of(true, Some(OsStr::new(value)), true).is_err(),
            "accepted {value:?}"
        );
    }
    assert!(binding_of(true, Some(OsStr::new(SLOT)), false).is_err());
}

#[tokio::test]
async fn concurrent_callers_share_one_refresh_and_one_exact_operation() {
    let host = HostFixture::new(0);
    let coordinator = Coordinator::new(host.clone());
    let (one, two) = tokio::join!(
        coordinator.refresh(SLOT, DIGEST),
        coordinator.refresh(SLOT, DIGEST)
    );
    assert_eq!(one.unwrap(), two.unwrap());
    assert_eq!(host.requests().len(), 1);
}

#[tokio::test]
async fn successful_same_byte_publication_does_not_suppress_a_later_forced_refresh() {
    let host = HostFixture::new(0);
    let coordinator = Coordinator::new(host.clone());
    assert_eq!(
        coordinator.refresh(SLOT, DIGEST).await.unwrap().digest,
        DIGEST
    );
    coordinator.refresh(SLOT, DIGEST).await.unwrap();
    let requests = host.requests();
    assert_eq!(requests.len(), 2);
    assert_ne!(requests[0].operation, requests[1].operation);
}

struct QueuedRevisionHost {
    requests: Mutex<Vec<Request>>,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

impl Host for QueuedRevisionHost {
    fn refresh<'a>(&'a self, request: &'a Request) -> RefreshFuture<'a> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request.clone());
            if request.expected_digest == DIGEST {
                self.entered.notify_one();
                self.release.notified().await;
                return Ok(Revision {
                    generation: 2,
                    digest: DIGEST.into(),
                });
            }
            Err(unavailable())
        })
    }
}

#[tokio::test]
async fn queued_new_digest_keeps_its_uncertain_receipt_after_older_completion() {
    let host = Arc::new(QueuedRevisionHost {
        requests: Mutex::default(),
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let coordinator = Arc::new(Coordinator::new(host.clone()));
    let first_coordinator = Arc::clone(&coordinator);
    let first = tokio::spawn(async move { first_coordinator.refresh(SLOT, DIGEST).await });
    host.entered.notified().await;
    let next_digest = "b".repeat(64);
    let mut second = Box::pin(coordinator.refresh(SLOT, &next_digest));
    // Capture a queued new-revision caller before the older owner completes,
    // without relying on scheduler timing or an implementation-only test hook.
    std::future::poll_fn(|context| {
        assert!(second.as_mut().poll(context).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    host.release.notify_one();
    first.await.unwrap().unwrap();
    assert!(second.await.is_err());
    assert!(coordinator.refresh(SLOT, &next_digest).await.is_err());
    let requests = host.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[1], requests[2],
        "new revision lost its exact retry receipt"
    );
}

#[tokio::test]
async fn uncertain_failure_retries_the_same_operation_without_echoing_host_text() {
    let host = HostFixture::new(1);
    let coordinator = Coordinator::new(host.clone());
    let error = coordinator
        .refresh(SLOT, DIGEST)
        .await
        .unwrap_err()
        .to_string();
    assert!(!error.contains("secret fixture"));
    coordinator.refresh(SLOT, DIGEST).await.unwrap();
    let requests = host.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
    assert!(valid_hex(&requests[0].operation, 32));
}

#[tokio::test]
async fn cancellation_keeps_the_operation_for_exact_retry() {
    let host = Arc::new(HostFixture {
        block_first: true,
        ..Arc::try_unwrap(HostFixture::new(0)).ok().unwrap()
    });
    let coordinator = Arc::new(Coordinator::new(host.clone()));
    let pending = Arc::clone(&coordinator);
    let task = tokio::spawn(async move { pending.refresh(SLOT, DIGEST).await });
    host.entered.notified().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    coordinator.refresh(SLOT, DIGEST).await.unwrap();
    let requests = host.requests();
    assert_eq!(requests[0], requests[1]);
}

#[tokio::test]
async fn independent_machine_slots_do_not_block_each_other() {
    let host = Arc::new(HostFixture {
        barrier: Some(tokio::sync::Barrier::new(2)),
        ..Arc::try_unwrap(HostFixture::new(0)).ok().unwrap()
    });
    let coordinator = Coordinator::new(host.clone());
    let (one, two) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        tokio::join!(
            coordinator.refresh(SLOT, DIGEST),
            coordinator.refresh("736c6f742f74776f", DIGEST)
        )
    })
    .await
    .expect("one slot must not hold another's refresh");
    one.unwrap();
    two.unwrap();
    assert_ne!(host.requests()[0].operation, host.requests()[1].operation);
}

#[cfg(unix)]
#[test]
fn document_read_preserves_exact_bytes_and_refuses_public_or_linked_files() -> io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    use std::os::unix::fs::PermissionsExt as _;
    let root = std::env::temp_dir().join(format!("alab-managed-{:032x}", rand::random::<u128>()));
    std::fs::create_dir(&root)?;
    let path = root.join("auth.json");
    let body = b"{\"tokens\":{},\"future_field\": 17}\n";
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    file.write_all(body)?;
    assert_eq!(read_document(&path)?.as_slice(), body);
    assert_eq!(binding_for_path(true, None, Some(&path))?, None);
    let state = state_path(&path);
    crate::home::save_auth_bytes(&state, b"unknown or pending custody")?;
    assert!(binding_for_path(true, None, Some(&path)).is_err());
    assert!(binding_for_path(true, Some(OsStr::new(SLOT)), Some(&path)).is_err());
    std::fs::remove_file(state)?;
    let marker = marker_path(&path);
    crate::home::save_auth_bytes(&marker, format!("{SLOT_MARKER_PREFIX}{SLOT}\n").as_bytes())?;
    assert_eq!(
        binding_for_path(true, None, Some(&path))?,
        Some(SLOT.into())
    );
    assert!(binding_for_path(true, Some(OsStr::new("736c6f742f74776f")), Some(&path)).is_err());
    crate::home::save_auth_bytes(
        &marker,
        format!("{SLOT_MARKER_PREFIX}{SLOT}\nextra\n").as_bytes(),
    )?;
    assert!(binding_for_path(true, None, Some(&path)).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))?;
    assert!(read_document(&path).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    let link = root.join("linked.json");
    std::os::unix::fs::symlink(&path, &link)?;
    assert!(read_document(&link).is_err());
    std::fs::remove_dir_all(root)
}
