// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Real Tokio task/future ownership controls. These supply no Config, native
//! SDK permission or effect-settlement result and do not qualify a TUI bridge.
use super::*;
use codex_alab_core::native_incoming::Accept;
use codex_alab_core::native_incoming::IncomingStreamProvider;
use codex_alab_core::native_incoming::Withdrawal;
use codex_app_server_protocol::ClientInfo;
use std::pin::Pin;
use std::sync::atomic::AtomicUsize;
use std::task::Context;
use std::time::Duration;
use tokio::sync::oneshot;

struct Provider {
    withdraw: CancellationToken,
    accepts: Arc<AtomicUsize>,
}
impl IncomingStreamProvider for Provider {
    fn accept(&self) -> Accept<'_> {
        self.accepts.fetch_add(1, Ordering::SeqCst);
        Box::pin(std::future::pending())
    }
    fn withdrawn(&self) -> Withdrawal<'_> {
        Box::pin(self.withdraw.cancelled())
    }
}

fn input() -> (Incoming, CancellationToken, Arc<AtomicUsize>) {
    let withdraw = CancellationToken::new();
    let accepts = Arc::new(AtomicUsize::new(0));
    (
        Incoming::new(Provider {
            withdraw: withdraw.clone(),
            accepts: accepts.clone(),
        }),
        withdraw,
        accepts,
    )
}

fn metadata() -> FrontendMetadata {
    FrontendMetadata {
        initialize: InitializeParams {
            client_info: ClientInfo {
                name: "owned-runtime-task-control".into(),
                title: Some("Original frontend".into()),
                version: "1.2.3".into(),
            },
            capabilities: None,
        },
        channel_capacity: 0,
    }
}

#[tokio::test]
async fn owned_runtime_startup_boundary_observes_actual_withdrawal_without_accepting() {
    let (incoming, withdraw, accepts) = input();
    assert!(!startup_withdrawn(&incoming).await);
    withdraw.cancel();
    assert!(startup_withdrawn(&incoming).await);
    assert!(incoming.accept().await.unwrap().is_none());
    assert_eq!(accepts.load(Ordering::SeqCst), 0);
}

async fn settle<F: Future>(work: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(5), work)
        .await
        .expect("owned runtime task hang detector")
}

async fn pending<F: Future>(mut work: Pin<&mut F>) {
    assert!(
        poll_fn(|context| Poll::Ready(work.as_mut().poll(context)))
            .await
            .is_pending(),
        "completion cannot precede the actual retained work"
    );
}

struct Custody(Arc<AtomicBool>);
impl Drop for Custody {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

struct PendingStartup {
    owner: RetainedRuntimeOwner,
    incoming: Incoming,
    withdraw: CancellationToken,
    started: oneshot::Receiver<()>,
    finish_prepare: oneshot::Sender<()>,
    cleaning: oneshot::Receiver<()>,
    finish_cleanup: oneshot::Sender<()>,
    dropped: Arc<AtomicBool>,
    starts: Arc<AtomicUsize>,
    runs: Arc<AtomicUsize>,
    closed_during_cleanup: Arc<AtomicBool>,
}

fn pending_startup() -> PendingStartup {
    let (incoming, withdraw, _) = input();
    let (announce_start, started) = oneshot::channel();
    let (finish_prepare, prepare_finished) = oneshot::channel();
    let (announce_cleanup, cleaning) = oneshot::channel();
    let (finish_cleanup, cleanup_finished) = oneshot::channel();
    let dropped = Arc::new(AtomicBool::new(false));
    let custody = Custody(dropped.clone());
    let starts = Arc::new(AtomicUsize::new(0));
    let count_starts = starts.clone();
    let runs = Arc::new(AtomicUsize::new(0));
    let count_runs = runs.clone();
    let closed_during_cleanup = Arc::new(AtomicBool::new(false));
    let observe_closed = closed_during_cleanup.clone();
    let owner = start_owned(
        metadata(),
        incoming.clone(),
        move || async move {
            count_starts.fetch_add(1, Ordering::SeqCst);
            announce_start.send(()).unwrap();
            prepare_finished.await.unwrap();
            Ok(custody)
        },
        move |custody, input, stop| async move {
            count_runs.fetch_add(1, Ordering::SeqCst);
            let closed = stop.is_cancelled() && input.accept().await?.is_none();
            observe_closed.store(closed, Ordering::SeqCst);
            drop(custody);
            announce_cleanup.send(()).unwrap();
            cleanup_finished.await.unwrap();
            Ok(())
        },
    )
    .unwrap();
    PendingStartup {
        owner,
        incoming,
        withdraw,
        started,
        finish_prepare,
        cleaning,
        finish_cleanup,
        dropped,
        starts,
        runs,
        closed_during_cleanup,
    }
}

#[tokio::test]
async fn owned_runtime_cancelled_wait_retains_one_start_and_exact_stored_result() {
    let (incoming, _, _) = input();
    let (finish, released) = oneshot::channel();
    let starts = Arc::new(AtomicUsize::new(0));
    let count = starts.clone();
    let owner = start_owned(
        metadata(),
        incoming,
        move || async move {
            count.fetch_add(1, Ordering::SeqCst);
            released.await.unwrap();
            Ok(())
        },
        |(), _, _| async { Ok(()) },
    )
    .unwrap();
    let mut waiting = Box::pin(owner.wait());
    pending(waiting.as_mut()).await;
    drop(waiting);
    finish.send(()).unwrap();
    let first = settle(owner.wait()).await;
    assert!(first.is_ok());
    let second = settle(owner.wait()).await;
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(owner.frontend_metadata().channel_capacity, 0);
    assert_eq!(
        owner
            .frontend_metadata()
            .initialize
            .client_info
            .title
            .as_deref(),
        Some("Original frontend")
    );
}

#[tokio::test]
async fn owned_runtime_owner_drop_before_first_poll_closes_admission_without_starting() {
    let fixture = pending_startup();
    let observed = fixture.owner.completion.clone();
    drop(fixture.owner);
    assert!(settle(fixture.incoming.accept()).await.unwrap().is_none());
    let result = settle(wait_completion(observed)).await;
    assert!(result.is_ok());
    assert_eq!(fixture.starts.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.runs.load(Ordering::SeqCst), 0);
    assert!(fixture.dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn owned_runtime_provider_withdrawal_before_prepare_does_not_start_work() {
    let fixture = pending_startup();
    fixture.withdraw.cancel();
    assert!(settle(fixture.owner.wait()).await.is_ok());
    assert!(settle(fixture.incoming.accept()).await.unwrap().is_none());
    assert_eq!(fixture.starts.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.runs.load(Ordering::SeqCst), 0);
    assert!(fixture.dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn owned_runtime_input_failure_before_prepare_is_error_without_starting() {
    let fixture = pending_startup();
    fixture.incoming.fail(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "private provider failure sentinel",
    ));
    let result = settle(fixture.owner.wait()).await;
    assert_eq!(
        result.as_ref().as_ref().unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(fixture.starts.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.runs.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn owned_runtime_owner_drop_during_prepare_keeps_custody_until_cleanup_finishes() {
    let fixture = pending_startup();
    settle(fixture.started).await.unwrap();
    let observed = fixture.owner.completion.clone();
    drop(fixture.owner);
    assert!(settle(fixture.incoming.accept()).await.unwrap().is_none());
    assert!(!fixture.dropped.load(Ordering::SeqCst));
    let mut waiting = Box::pin(wait_completion(observed));
    pending(waiting.as_mut()).await;
    fixture.finish_prepare.send(()).unwrap();
    settle(fixture.cleaning).await.unwrap();
    assert!(fixture.dropped.load(Ordering::SeqCst));
    assert!(fixture.closed_during_cleanup.load(Ordering::SeqCst));
    pending(waiting.as_mut()).await;
    fixture.finish_cleanup.send(()).unwrap();
    assert!(settle(waiting).await.is_ok());
    assert_eq!(fixture.starts.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.runs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn owned_runtime_provider_withdrawal_during_prepare_keeps_original_startup() {
    let fixture = pending_startup();
    settle(fixture.started).await.unwrap();
    fixture.withdraw.cancel();
    settle(fixture.incoming.withdrawn()).await;
    assert!(!fixture.dropped.load(Ordering::SeqCst));
    let mut waiting = Box::pin(fixture.owner.wait());
    pending(waiting.as_mut()).await;
    fixture.finish_prepare.send(()).unwrap();
    settle(fixture.cleaning).await.unwrap();
    assert!(fixture.closed_during_cleanup.load(Ordering::SeqCst));
    pending(waiting.as_mut()).await;
    fixture.finish_cleanup.send(()).unwrap();
    assert!(settle(waiting).await.is_ok());
    assert_eq!(fixture.starts.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.runs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn owned_runtime_input_failure_during_prepare_survives_actual_cleanup() {
    let fixture = pending_startup();
    settle(fixture.started).await.unwrap();
    fixture.incoming.fail(io::Error::new(
        io::ErrorKind::ConnectionReset,
        "private provider failure sentinel",
    ));
    let first_failure = fixture.incoming.failure().unwrap();
    let mut waiting = Box::pin(fixture.owner.wait());
    pending(waiting.as_mut()).await;
    assert!(!fixture.dropped.load(Ordering::SeqCst));
    fixture.finish_prepare.send(()).unwrap();
    settle(fixture.cleaning).await.unwrap();
    assert!(fixture.closed_during_cleanup.load(Ordering::SeqCst));
    pending(waiting.as_mut()).await;
    fixture.finish_cleanup.send(()).unwrap();
    let result = settle(waiting).await;
    assert_eq!(
        result.as_ref().as_ref().unwrap_err().kind(),
        io::ErrorKind::ConnectionReset
    );
    assert!(Arc::ptr_eq(
        &first_failure,
        &fixture.incoming.failure().unwrap()
    ));
}

#[tokio::test]
async fn owned_runtime_prepare_error_preserves_an_actual_input_failure() {
    let (incoming, _, _) = input();
    let (announce, started) = oneshot::channel();
    let (finish, released) = oneshot::channel();
    let owner = start_owned(
        metadata(),
        incoming.clone(),
        move || async move {
            announce.send(()).unwrap();
            released.await.unwrap();
            Err::<(), _>(io::Error::other("private preparation failure sentinel"))
        },
        |(), _, _| async { panic!("failed preparation cannot produce a runtime") },
    )
    .unwrap();
    settle(started).await.unwrap();
    incoming.fail(io::Error::from(io::ErrorKind::UnexpectedEof));
    finish.send(()).unwrap();
    let result = settle(owner.wait()).await;
    assert_eq!(
        result.as_ref().as_ref().unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
}

#[tokio::test]
async fn owned_runtime_cancelled_shutdown_wait_cannot_discard_pending_startup() {
    let fixture = pending_startup();
    settle(fixture.started).await.unwrap();
    let mut shutdown = Box::pin(fixture.owner.shutdown());
    pending(shutdown.as_mut()).await;
    drop(shutdown);
    assert!(settle(fixture.incoming.accept()).await.unwrap().is_none());
    assert!(!fixture.dropped.load(Ordering::SeqCst));
    fixture.finish_prepare.send(()).unwrap();
    settle(fixture.cleaning).await.unwrap();
    assert!(fixture.closed_during_cleanup.load(Ordering::SeqCst));
    let mut waiting = Box::pin(fixture.owner.wait());
    pending(waiting.as_mut()).await;
    fixture.finish_cleanup.send(()).unwrap();
    let first = settle(waiting).await;
    let second = settle(fixture.owner.wait()).await;
    assert!(first.is_ok());
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(fixture.starts.load(Ordering::SeqCst), 1);
}

async fn panic_during_prepare(_custody: Custody) -> io::Result<Custody> {
    panic!("private startup panic payload sentinel");
}

#[tokio::test]
async fn owned_runtime_preparation_panic_releases_custody_and_stores_fixed_error() {
    let (incoming, _, _) = input();
    let dropped = Arc::new(AtomicBool::new(false));
    let custody = Custody(dropped.clone());
    let owner = start_owned(
        metadata(),
        incoming,
        move || panic_during_prepare(custody),
        |_, _, _| async { panic!("preparation must not produce a runtime") },
    )
    .unwrap();
    let first = settle(owner.wait()).await;
    assert_eq!(
        first.as_ref().as_ref().unwrap_err().to_string(),
        "retained runtime preparation panicked"
    );
    assert!(dropped.load(Ordering::SeqCst));
    assert!(Arc::ptr_eq(&first, &settle(owner.wait()).await));
}

#[tokio::test]
async fn owned_runtime_runtime_panic_releases_custody_and_stores_fixed_error() {
    let (incoming, _, _) = input();
    let dropped = Arc::new(AtomicBool::new(false));
    let custody = Custody(dropped.clone());
    let owner = start_owned(
        metadata(),
        incoming,
        move || async move { Ok(custody) },
        |custody, _, _| async move {
            let _custody = custody;
            panic!("private runtime panic payload sentinel");
        },
    )
    .unwrap();
    let first = settle(owner.wait()).await;
    assert_eq!(
        first.as_ref().as_ref().unwrap_err().to_string(),
        "retained runtime task panicked"
    );
    assert!(dropped.load(Ordering::SeqCst));
    assert!(Arc::ptr_eq(&first, &settle(owner.wait()).await));
}

struct DestructionPanic;
impl Future for DestructionPanic {
    type Output = io::Result<()>;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(Ok(()))
    }
}
impl Drop for DestructionPanic {
    fn drop(&mut self) {
        panic!("private destruction panic payload sentinel");
    }
}

#[tokio::test]
async fn owned_runtime_future_destruction_panic_cannot_be_reported_as_success() {
    let error = capture_work(DestructionPanic, &AtomicBool::new(false))
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "retained runtime work destruction panicked"
    );
}

#[test]
fn owned_runtime_start_without_runtime_returns_error_before_preparation() {
    let (incoming, _, accepts) = input();
    let result = start_owned(
        metadata(),
        incoming,
        || async { panic!("no runtime may start preparation") },
        |(): (), _, _| async { Ok(()) },
    );
    let error = match result {
        Ok(_) => panic!("a live Tokio runtime is required"),
        Err(error) => error,
    };
    assert_eq!(
        error.to_string(),
        "retained runtime requires an active Tokio runtime"
    );
    assert_eq!(accepts.load(Ordering::SeqCst), 0);
}
