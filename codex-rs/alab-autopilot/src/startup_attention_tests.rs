// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Startup cleanup's ownership boundaries through the actual metadata transport.

use super::*;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;

const OLD: &str = "login: signed out; codex's sign-in screen waits for a person";

async fn fixture() -> (tempfile::TempDir, FakeAterm, Arc<Supervisor>, Client) {
    let mut dir = tempfile::tempdir().expect("private fake aterm");
    dir.disable_cleanup(true);
    eprintln!(
        "retained startup ownership fixture: {}",
        dir.path().display()
    );
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .expect("fake aterm");
    fake.set_attention_as("codex-alab", OLD);
    let shared = Box::leak(Box::new(codex_alab_core::session::Shared::new()));
    let endpoint = fake.endpoint();
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new({
            let endpoint = endpoint.clone();
            move || Some(endpoint.clone())
        }),
        client: ClientOptions::default(),
    });
    let client = Client::connect(endpoint, ClientOptions::default())
        .await
        .expect("client");
    (dir, fake, sup, client)
}

#[tokio::test(flavor = "current_thread")]
async fn a_foreign_holder_permanently_retires_startup_cleanup() {
    let (dir, fake, sup, client) = fixture().await;
    fake.claim_as("another-supervisor");
    sup.clear_stale_screen(&client).await;
    assert!(sup.startup_attention_retry_at().is_none());
    fake.clear_claim();
    // A later connection/start callback cannot reopen the retired startup scan.
    sup.clear_stale_screen(&client).await;
    assert_eq!(
        fake.attention().get("codex-alab").map(String::as_str),
        Some(OLD)
    );
    assert!(
        !fake
            .requests()
            .iter()
            .any(|line| line.ends_with(" meta unset attention owner=codex-alab"))
    );
    client.close();
    client.closed().await;
    eprintln!(
        "retained foreign-holder startup fixture: {}",
        dir.keep().display()
    );
}

/// A clear already in flight precedes the new claim's publication on the shared lane.
/// The new reason deliberately has the same words as the old startup entry (ABA control).
#[tokio::test(flavor = "current_thread")]
async fn a_new_claim_publishes_after_an_inflight_startup_clear() {
    let (dir, fake, sup, client) = fixture().await;
    let clear = "meta unset attention owner=codex-alab";
    fake.gate(clear);
    let cleanup = tokio::spawn({
        let sup = Arc::clone(&sup);
        let client = client.clone();
        async move { sup.clear_stale_screen(&client).await }
    });
    assert!(
        fake.wait_for(|fake| !fake.gated_requests().is_empty())
            .await
    );
    let claiming = Client::connect(fake.endpoint(), ClientOptions::default())
        .await
        .expect("claim connection");
    let mut claims = sup.claims.subscribe();
    let refresh = tokio::spawn({
        let sup = Arc::clone(&sup);
        let claiming = claiming.clone();
        async move { sup.refresh(&claiming).await }
    });
    tokio::time::timeout(Duration::from_secs(60), async {
        while sup.lock().claims_taken == 0 {
            claims.changed().await.expect("claim watch");
        }
    })
    .await
    .expect("the proven claim never advanced local custody");
    assert!(
        claiming.holds_claim(),
        "the fixture's claim is actually proven"
    );
    let _ = sup.want_attention_owned(None, "current", Some(OLD.into()));
    fake.open_gate(clear);
    tokio::time::timeout(Duration::from_secs(60), async {
        cleanup.await.expect("cleanup task");
        refresh.await.expect("claim refresh task");
    })
    .await
    .expect("publication lane hung");
    assert_eq!(
        fake.attention().get("codex-alab").map(String::as_str),
        Some(OLD)
    );
    assert!(sup.startup_attention_retry_at().is_none());
    assert!(fake.acts().is_empty());
    client.close();
    claiming.close();
    client.closed().await;
    claiming.closed().await;
    eprintln!(
        "retained claim-boundary startup fixture: {}",
        dir.keep().display()
    );
}

async fn until(done: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(60), async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("startup fixture hung");
}

/// The watch can consume a foreign claim and its release while the main loop is still
/// waiting on another request. A ready retry may then run before those queued events.
#[tokio::test(flavor = "current_thread")]
async fn startup_foreign_watch_history_retires_a_ready_cleanup() {
    let (dir, fake, sup, client) = fixture().await;
    fake.set_effective(false, "autopilot-off");
    fake.reply_once(
        &format!("@{} meta", fake.sid()),
        "ERR internal: metadata busy\n",
    );
    fake.gate("harness policy");
    let (_tui, mut changes) = watch::channel(0);
    let session = tokio::spawn({
        let sup = Arc::clone(&sup);
        let endpoint = fake.endpoint();
        async move { sup.session(endpoint, &mut changes, &mut 0, None).await }
    });
    assert!(
        fake.wait_for(|fake| fake
            .gated_requests()
            .iter()
            .any(|line| line.ends_with(" harness policy")))
            .await
    );
    assert!(
        sup.lock().startup_attention_retry.is_some(),
        "the first refusal left real cleanup debt"
    );
    // Make the existing debt ready before the competing history arrives; the main loop
    // remains gated, so invoking its handler models the valid timer-before-queued-events arm.
    sup.lock().startup_attention_retry = Some(clock());
    let observer = Client::connect(fake.endpoint(), ClientOptions::default())
        .await
        .expect("observer");
    let epoch = sup.lost_epoch.load(Ordering::SeqCst);
    assert!(
        observer
            .request(&format!(
                "@{} meta set supervisor foreign-holder ttl=60000",
                fake.sid()
            ))
            .await
            .expect("foreign claim")
            .ok()
            .is_some()
    );
    let fresh = "trust: /new is not trusted yet; codex's folder-trust screen waits for a person";
    observer
        .set_attention("trust", fresh.strip_prefix("trust: ").expect("kind"))
        .await
        .expect("successor's startup entry");
    assert!(
        observer
            .request(&format!(
                "@{} meta unset supervisor holder=foreign-holder",
                fake.sid()
            ))
            .await
            .expect("foreign release")
            .ok()
            .is_some()
    );
    until(|| sup.lost_epoch.load(Ordering::SeqCst) >= epoch + 2).await;
    sup.reconcile_startup_attention(&observer).await;
    let shown = fake.attention().get("codex-alab").cloned();
    let cleared = fake
        .requests()
        .iter()
        .any(|line| line.ends_with(" meta unset attention owner=codex-alab"));
    sup.closing.store(true, Ordering::SeqCst);
    fake.open_gate("harness policy");
    fake.end_subscriptions();
    tokio::time::timeout(Duration::from_secs(60), session)
        .await
        .expect("session ends")
        .expect("session task");
    client.close();
    observer.close();
    client.closed().await;
    observer.closed().await;
    eprintln!("retained foreign-watch fixture: {}", dir.keep().display());
    assert_eq!(shown.as_deref(), Some(fresh));
    assert!(
        !cleared,
        "the observed foreign lifecycle retired startup authority"
    );
}

/// A Busy answer is also an observed foreign lifecycle, even without a watch. A fresh
/// connection after that holder leaves must not recover the old startup clear's authority.
#[tokio::test(flavor = "current_thread")]
async fn startup_foreign_busy_claim_retires_cleanup_across_a_new_connection() {
    let (dir, fake, sup, client) = fixture().await;
    fake.reply_once(
        &format!("@{} meta", fake.sid()),
        "ERR internal: metadata busy\n",
    );
    sup.clear_stale_screen(&client).await;
    assert!(sup.lock().startup_attention_retry.is_some());
    fake.claim_as("foreign-holder");
    sup.refresh(&client).await;
    let (busy, claims) = {
        let state = sup.lock();
        (state.busy, state.claims_taken)
    };
    assert!(
        busy && claims == 0,
        "a real Busy response, not a proven claim"
    );
    fake.clear_claim();
    let fresh =
        "hooks: 1 hook for /new needs review; codex's hooks-review screen waits for a person";
    fake.set_attention_as("codex-alab", fresh);
    client.close();
    client.closed().await;
    let successor = Client::connect(fake.endpoint(), ClientOptions::default())
        .await
        .expect("new connection");
    sup.reconcile_startup_attention(&successor).await;
    let shown = fake.attention().get("codex-alab").cloned();
    successor.close();
    successor.closed().await;
    eprintln!("retained Busy startup fixture: {}", dir.keep().display());
    assert_eq!(shown.as_deref(), Some(fresh));
    assert!(
        !fake
            .requests()
            .iter()
            .any(|line| line.ends_with(" meta unset attention owner=codex-alab"))
    );
}
