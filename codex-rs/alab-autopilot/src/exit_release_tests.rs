// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Exit cleanup precedes claim release, including a reconnect's rejected take.

use std::future::Future;
use std::future::poll_fn;
use std::pin::Pin;
use std::task::Poll;

use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;

use super::*;

const HANG: Duration = Duration::from_secs(60);
const OWN: &str = "api: an acknowledged reason from the old connection";
const CLAIM: &str = "meta set supervisor";
const CLEAR: &str = " meta unset attention owner=codex-alab";

async fn fixture() -> (tempfile::TempDir, FakeAterm, Arc<Supervisor>) {
    let mut dir = tempfile::tempdir().expect("private exit aterm");
    dir.disable_cleanup(true);
    eprintln!("retained exit ordering fixture: {}", dir.path().display());
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .expect("fake aterm");
    let shared = Box::leak(Box::new(codex_alab_core::session::Shared::new()));
    let endpoint = fake.endpoint();
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(move || Some(endpoint.clone())),
        client: ClientOptions {
            timeout: HANG,
            ..ClientOptions::default()
        },
    });
    sup.install_release();
    (dir, fake, sup)
}

/// Drive the real session to an observed boundary; stopping its polling also stops its
/// next request, while the connection worker and exit task continue independently.
async fn drive_until<F: Future<Output = SessionEnd>>(
    mut session: Pin<&mut F>,
    ready: impl Fn() -> bool,
) -> Option<SessionEnd> {
    tokio::time::timeout(HANG, async {
        loop {
            match poll_fn(|cx| Poll::Ready(session.as_mut().poll(cx))).await {
                Poll::Ready(end) => return Some(end),
                Poll::Pending if ready() => return None,
                Poll::Pending => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        }
    })
    .await
    .expect("session boundary hang detector")
}

async fn until(ready: impl Fn() -> bool) {
    tokio::time::timeout(HANG, async {
        while !ready() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("exit boundary hang detector");
}

/// A receipt survives a dead socket. Its replacement proves a claim after exit began;
/// rejecting that take must not release ahead of the exit's blocked cleanup. The result
/// watch's subscriber count only locates the fixed wait; the verdict is actual wire order.
#[tokio::test(flavor = "current_thread")]
async fn a_reconnects_refused_claim_waits_for_the_exits_attention_clear() {
    let (_dir, fake, sup) = fixture().await;
    let (_tui, mut changes) = watch::channel(0);
    let mut tries = 0;
    let old = {
        let first = sup.session(fake.endpoint(), &mut changes, &mut tries, None);
        tokio::pin!(first);
        assert!(
            drive_until(first.as_mut(), || {
                let st = sup.lock();
                st.acting() && !st.attention_unknown() && !st.needs_unknown()
            })
            .await
            .is_none()
        );
        sup.attention("api", Some(OWN.to_owned())).await;
        assert_eq!(sup.lock().shown.as_deref(), Some(OWN));
        assert_eq!(
            fake.attention().get("codex-alab").map(String::as_str),
            Some(OWN)
        );
        let old = sup.client().expect("old actual session client");
        old.close();
        old.closed().await;
        assert!(matches!(
            tokio::time::timeout(HANG, first)
                .await
                .expect("first session ends"),
            SessionEnd::Retry(_)
        ));
        old
    };
    sup.lost_locally("the aterm connection ended");
    assert_eq!(
        sup.lock().shown.as_deref(),
        Some(OWN),
        "the real receipt survived"
    );

    let boundary = fake.requests().len();
    let description = sup
        .description_sync
        .acquire()
        .await
        .expect("description permit");
    fake.gate(CLAIM);
    let second = sup.session(fake.endpoint(), &mut changes, &mut tries, None);
    tokio::pin!(second);
    assert!(
        drive_until(second.as_mut(), || {
            fake.gated_requests().iter().any(|r| r.contains(CLAIM))
        })
        .await
        .is_none()
    );
    let client = sup.client().expect("replacement actual session client");
    assert!(client.is_open() && !old.is_open());
    let shared = sup.opts.shared;
    let release = tokio::task::spawn_blocking(move || shared.release_claim(HANG));
    until(|| sup.closing() && !sup.held()).await;
    let waiters = sup.exit_result.receiver_count();
    fake.open_gate(CLAIM);
    let finished = drive_until(second.as_mut(), || {
        sup.exit_result.receiver_count() > waiters
    })
    .await;
    drop(description);
    assert!(release.await.expect("exit release task"));
    if finished.is_none() {
        assert!(matches!(
            tokio::time::timeout(HANG, second)
                .await
                .expect("replacement session ends"),
            SessionEnd::Closing
        ));
    }
    let requests = fake.requests();
    let tail = &requests[boundary..];
    let clear = tail
        .iter()
        .position(|r| r.ends_with(CLEAR))
        .expect("exit clears its receipt");
    let release = format!(" meta unset supervisor holder={}", client.holder());
    let releases: Vec<_> = tail
        .iter()
        .enumerate()
        .filter(|(_, r)| r.ends_with(&release))
        .map(|(i, _)| i)
        .collect();
    assert!(!releases.is_empty(), "the claim is released: {tail:?}");
    assert!(
        releases.iter().all(|release| clear < *release),
        "exit cleanup precedes every release: {tail:?}"
    );
    assert!(fake.claim().is_none());
    assert!(!fake.attention().contains_key("codex-alab"));
    client.close();
    client.closed().await;
}

/// The exit can finish while a session's policy read is unanswered. Keeping that session
/// unpolled after the reply lets the exit release finish before the actual claim is queued.
/// That later proven claim must still be returned; merely leaving every refusal to exit leaks it.
/// A refused initial release still completes cleanup: duplicates retain that failure without
/// replaying it, and the later proven claim still owes its own release.
#[tokio::test(flavor = "current_thread")]
async fn a_claim_queued_after_completed_exit_is_still_given_back() {
    for refuse_release in [false, true] {
        let (_dir, fake, sup) = fixture().await;
        fake.gate("harness policy");
        let (_tui, mut changes) = watch::channel(0);
        let mut tries = 0;
        let session = sup.session(fake.endpoint(), &mut changes, &mut tries, None);
        tokio::pin!(session);
        assert!(
            drive_until(session.as_mut(), || {
                fake.gated_requests()
                    .iter()
                    .any(|r| r.contains("harness policy"))
            })
            .await
            .is_none()
        );
        let client = sup.client().expect("the paused session's actual client");
        if refuse_release {
            fake.reply_once(
                &format!(
                    "@{} meta unset supervisor holder={}",
                    client.endpoint().sid,
                    client.holder()
                ),
                "ERR exit release refused\n",
            );
        }
        let shared = sup.opts.shared;
        let release = tokio::task::spawn_blocking(move || shared.release_claim(HANG));
        until(|| sup.closing()).await;
        fake.open_gate("harness policy");
        assert_eq!(
            release
                .await
                .expect("exit finishes while session is paused"),
            !refuse_release
        );
        let before = fake.requests().len();
        assert_eq!(shared.release_claim(Duration::ZERO), !refuse_release);
        assert_eq!(
            fake.requests().len(),
            before,
            "completed duplicate sends no request"
        );
        assert!(matches!(
            tokio::time::timeout(HANG, session)
                .await
                .expect("late claim is returned"),
            SessionEnd::Closing
        ));
        let requests = fake.requests();
        let tail = &requests[before..];
        let claim = tail
            .iter()
            .position(|r| r.contains(CLAIM))
            .expect("claim really followed completed exit");
        let release = tail
            .iter()
            .position(|r| r.contains(" meta unset supervisor holder="))
            .expect("late claim is released");
        assert!(claim < release, "actual late claim then release: {tail:?}");
        assert!(fake.claim().is_none());
        client.close();
        client.closed().await;
    }
}

/// A caller whose budget is exhausted leaves the one cleanup running. A concurrent
/// caller waits for it, and a completed duplicate consumes the retained answer directly.
#[tokio::test(flavor = "current_thread")]
async fn repeated_exit_callers_share_cleanup_after_one_callers_budget_expires() {
    let (_dir, fake, sup) = fixture().await;
    let (_tui, mut changes) = watch::channel(0);
    let mut tries = 0;
    let session = sup.session(fake.endpoint(), &mut changes, &mut tries, None);
    tokio::pin!(session);
    assert!(
        drive_until(session.as_mut(), || {
            let st = sup.lock();
            st.acting() && !st.attention_unknown() && !st.needs_unknown()
        })
        .await
        .is_none()
    );
    sup.attention("api", Some(OWN.to_owned())).await;
    let client = sup.client().expect("actual session client");
    let boundary = fake.requests().len();
    fake.gate(CLEAR.trim_start());
    let shared = sup.opts.shared;
    let expired = tokio::task::spawn_blocking(move || shared.release_claim(Duration::ZERO));
    assert!(!expired.await.expect("bounded caller returns"));
    until(|| fake.gated_requests().iter().any(|r| r.ends_with(CLEAR))).await;
    let duplicate = tokio::task::spawn_blocking(move || shared.release_claim(HANG));
    until(|| sup.exit_result.receiver_count() >= 2).await;
    fake.open_gate(CLEAR.trim_start());
    assert!(
        duplicate
            .await
            .expect("concurrent caller shares the cleanup")
    );
    // This thread is the runtime's only worker. A completed answer needs no task scheduled
    // back onto it, and zero is an exhausted caller budget rather than a latency assertion.
    assert!(shared.release_claim(Duration::ZERO));
    let requests = fake.requests();
    let tail = &requests[boundary..];
    assert_eq!(
        tail.iter().filter(|r| r.ends_with(CLEAR)).count(),
        1,
        "{tail:?}"
    );
    let release = format!(" meta unset supervisor holder={}", client.holder());
    assert_eq!(
        tail.iter().filter(|r| r.ends_with(&release)).count(),
        1,
        "{tail:?}"
    );
    assert!(!fake.attention().contains_key("codex-alab"));
    assert!(fake.claim().is_none());
    assert!(matches!(
        tokio::time::timeout(HANG, session)
            .await
            .expect("session ends"),
        SessionEnd::Closing
    ));
    client.close();
    client.closed().await;
}

/// With no request connection, cleanup is already complete. The synchronous hook returns
/// that fact even when there is no runtime worker available to run a result bridge.
#[tokio::test(flavor = "current_thread")]
async fn an_exit_without_a_connection_is_immediately_complete() {
    let (_dir, fake, sup) = fixture().await;
    assert!(sup.opts.shared.release_claim(Duration::ZERO));
    assert!(sup.opts.shared.release_claim(Duration::ZERO));
    assert!(fake.requests().is_empty());
}

/// Closing retires the session while the exit callback is paused before its async work.
/// The exit must already own the healthy connection and acknowledged cleanup receipt;
/// an ordinary retained client rules out implicit socket close as a substitute for release.
#[tokio::test(flavor = "current_thread")]
async fn session_retirement_cannot_discard_the_exits_cleanup_connection() {
    let (_dir, fake, sup) = fixture().await;
    let (_tui, mut changes) = watch::channel(0);
    let mut tries = 0;
    let session = sup.session(fake.endpoint(), &mut changes, &mut tries, None);
    tokio::pin!(session);
    assert!(
        drive_until(session.as_mut(), || {
            let st = sup.lock();
            st.acting() && !st.attention_unknown() && !st.needs_unknown()
        })
        .await
        .is_none()
    );
    sup.attention("api", Some(OWN.to_owned())).await;
    assert_eq!(
        fake.attention().get("codex-alab").map(String::as_str),
        Some(OWN)
    );
    let client = sup.client().expect("ordinary healthy client clone");
    let boundary = fake.requests().len();
    let (entered, paused) = tokio::sync::oneshot::channel();
    let (resume, resumed) = std::sync::mpsc::channel();
    *sup.exit_before_cleanup.lock().expect("hook lock") = Some(Box::new(move || {
        let _ = entered.send(());
        resumed
            .recv_timeout(HANG)
            .expect("exit capture hook resumes");
    }));
    let shared = sup.opts.shared;
    let release = tokio::task::spawn_blocking(move || shared.release_claim(HANG));
    tokio::time::timeout(HANG, paused)
        .await
        .expect("exit reaches hook")
        .expect("hook sender");
    fake.set_effective(false, "fixture closing event");
    assert!(matches!(
        tokio::time::timeout(HANG, session)
            .await
            .expect("session retires"),
        SessionEnd::Closing
    ));
    // This is the actual outer run loop's next step, after session returned Closing.
    sup.lost_locally("the aterm connection ended");
    assert!(
        client.is_open(),
        "the ordinary clone keeps the healthy socket open"
    );
    resume.send(()).expect("resume exit callback");
    assert!(release.await.expect("exit callback completes"));
    let requests = fake.requests();
    let tail = &requests[boundary..];
    let clear = tail
        .iter()
        .position(|r| r.ends_with(CLEAR))
        .expect("exit must actually clear the acknowledged alert");
    let release = tail
        .iter()
        .position(|r| r.contains(" meta unset supervisor holder="))
        .expect("exit must actually release its healthy claim");
    assert!(clear < release, "cleanup precedes release: {tail:?}");
    assert!(!fake.attention().contains_key("codex-alab"));
    assert!(fake.claim().is_none());
    client.close();
    client.closed().await;
}
