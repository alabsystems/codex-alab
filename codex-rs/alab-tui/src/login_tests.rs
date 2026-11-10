// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The sign-in screen's attention (`login.rs`), against alab-aterm's in-process FAKE aterm
//! (never a real one).

use std::time::Duration;

use codex_alab_aterm::Endpoint;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use codex_alab_aterm::Client;

use crate::login::KIND;
use crate::login::WHY;
use crate::login::during_with;
use crate::waiting::clear_stale;

async fn fake() -> (tempfile::TempDir, FakeAterm) {
    let dir = tempfile::tempdir().expect("scratch dir");
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .expect("fake aterm");
    (dir, fake)
}

/// A one-minute hang detector around a screen run, never a latency budget.
async fn bounded<F: std::future::Future>(run: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(60), run)
        .await
        .expect("the screen run hung")
}

/// While the sign-in screen runs, aterm shows codex-alab's keyed attention, at most 200
/// bytes; when it ends, the attention is gone, and the screen's own result comes back.
#[tokio::test]
async fn the_sign_in_screen_raises_attention_until_it_ends() {
    let (_dir, fake) = fake().await;
    let want = format!("{KIND}: {WHY}");
    assert!(want.len() <= 200, "{want}");
    let seen = bounded(during_with(Some(fake.endpoint()), || false, async {
        let shown = fake
            .wait_for(|f| f.attention().get("codex-alab") == Some(&want))
            .await;
        (shown, 7)
    }))
    .await;
    assert_eq!(seen, (true, 7), "{:?}", fake.requests());
    assert!(
        !fake.attention().contains_key("codex-alab"),
        "cleared at the end: {:?}",
        fake.attention()
    );
    let requests = fake.requests();
    assert!(
        requests
            .iter()
            .any(|r| r.ends_with("meta unset attention owner=codex-alab")),
        "{requests:?}"
    );
}

/// Negative control: no endpoint (outside aterm, not codex-alab, no sign-in step) runs the
/// screen alone, and aterm hears nothing.
#[tokio::test]
async fn no_endpoint_raises_nothing() {
    let (_dir, fake) = fake().await;
    let out = bounded(during_with(None, || false, async { 7 })).await;
    assert_eq!(out, 7);
    assert_eq!(fake.connections(), 0, "{:?}", fake.requests());
    assert!(fake.attention().is_empty());
}

/// An aterm that cannot be reached (a dead socket) fails closed: the screen runs and its
/// result comes back.
#[tokio::test]
async fn an_unreachable_aterm_leaves_the_screen_alone() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let gone = Endpoint::explicit(dir.path().join("none.sock"), "s-0000");
    let out = bounded(during_with(Some(gone), || false, async { 7 })).await;
    assert_eq!(out, 7);
}

/// An aterm that does not answer the raise: the screen still ends, and the clear, sent on
/// a connection of its own once the raise was cut off, is bounded too.
#[tokio::test]
async fn a_stalled_aterm_never_holds_the_screen() {
    let (_dir, fake) = fake().await;
    fake.stall("meta");
    let out = bounded(during_with(Some(fake.endpoint()), || false, async { 7 })).await;
    assert_eq!(out, 7);
}

/// Review of 4cdc96090, finding 6: the sign-in completes and the onboarding screen goes on
/// (the folder-trust step): the attention goes then, while the screen still runs, not when it
/// ends. Negative control: before the sign-in completes it stands, the screen running.
#[tokio::test]
async fn the_attention_goes_when_the_sign_in_completes_not_when_the_screen_ends() {
    let (_dir, fake) = fake().await;
    let want = format!("{KIND}: {WHY}");
    let signed_in = Arc::new(AtomicBool::new(false));
    let probe = Arc::clone(&signed_in);
    let seen = bounded(during_with(
        Some(fake.endpoint()),
        move || probe.load(Ordering::SeqCst),
        async {
            let shown = fake
                .wait_for(|f| f.attention().get("codex-alab") == Some(&want))
                .await;
            // Still signing in: it stands while the screen runs (another wake of the screen).
            tokio::time::sleep(Duration::from_millis(200)).await;
            let stood = fake.attention().get("codex-alab") == Some(&want);
            // Signed in; the screen goes on to its next step and waits there.
            signed_in.store(true, Ordering::SeqCst);
            let cleared = fake
                .wait_for(|f| !f.attention().contains_key("codex-alab"))
                .await;
            (shown, stood, cleared)
        },
    ))
    .await;
    assert_eq!(seen, (true, true, true), "{:?}", fake.requests());
}

async fn client(fake: &FakeAterm) -> Client {
    Client::connect(fake.endpoint(), codex_alab_aterm::Options::default())
        .await
        .expect("connect")
}

/// Review of 4cdc96090, finding 6: a sign-in screen that never reached its clear (a SIGTERM,
/// a SIGKILL, a panic) left codex-alab's `login` attention standing; the next start past the
/// sign-in clears it, once. Negative controls: codex-alab's other attention and another
/// owner's `login:` stand.
#[tokio::test]
async fn a_start_past_the_sign_in_clears_only_codex_alabs_own_standing_login() {
    let (_dir, fake) = fake().await;
    let c = client(&fake).await;
    fake.set_attention_as("codex-alab", &format!("{KIND}: {WHY}"));
    assert!(clear_stale(&c).await.expect("read"));
    assert!(fake.attention().is_empty(), "{:?}", fake.attention());
    assert!(
        !clear_stale(&c).await.expect("read"),
        "nothing left to clear"
    );

    fake.set_attention_as("codex-alab", "question: a question box codex-alab leaves");
    assert!(!clear_stale(&c).await.expect("read"));
    assert!(fake.attention().contains_key("codex-alab"));

    // The autopilot's own `login: …` attentions say something still true (review of
    // fea034d20, finding 4): only the sign-in screen's exact text is stale.
    for own in [
        "login: Codex's sign-in was refused or expired; sign in to Codex again (codex login)",
        "login: Codex asks for a sign-in",
    ] {
        fake.set_attention_as("codex-alab", own);
        assert!(!clear_stale(&c).await.expect("read"), "{own}");
        assert_eq!(
            fake.attention().get("codex-alab").map(String::as_str),
            Some(own)
        );
    }

    let (_dir2, other) = self::fake().await;
    let c = client(&other).await;
    other.set_attention_as("someone", "login: their own sign-in");
    assert!(!clear_stale(&c).await.expect("read"));
    assert_eq!(
        other.attention().get("someone").map(String::as_str),
        Some("login: their own sign-in")
    );
}

/// aterm shows only its most recently written attention entry: a stale sign-in attention under
/// another owner's newer entry is not seen and stands; once it is the newest it is cleared.
/// The order, not the owners' names, decides (the fake once showed the alphabetically last).
/// Setting the same text again is a no-op at aterm (`AttentionOwners::put`: its stamp stays),
/// so it does not make the entry the newest (review of 7e7622ff7, finding 9: the fake made it
/// so); a new text does.
#[tokio::test]
async fn only_the_newest_attention_entry_is_seen() {
    let (_dir, fake) = fake().await;
    let c = client(&fake).await;
    let sign_in = format!("{KIND}: {WHY}");
    fake.set_attention_as("codex-alab", &sign_in);
    fake.set_attention_as("supervisor", "stall: the tab is quiet");
    assert!(!clear_stale(&c).await.expect("read"));
    assert!(fake.attention().contains_key("codex-alab"));
    // The same text again moves nothing: the supervisor's entry is still the one shown.
    fake.set_attention_as("codex-alab", &sign_in);
    assert!(!clear_stale(&c).await.expect("read"));
    assert!(fake.attention().contains_key("codex-alab"));
    // Another text, then the sign-in's again: each is a write, the sign-in's the newest.
    fake.set_attention_as("codex-alab", "question: a question box codex-alab leaves");
    fake.set_attention_as("codex-alab", &sign_in);
    assert!(clear_stale(&c).await.expect("read"));
    assert!(!fake.attention().contains_key("codex-alab"));
    assert!(fake.attention().contains_key("supervisor"));
}
