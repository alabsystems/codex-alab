// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A signed-out first launch inside aterm (CONTRACT §2 (a), §5 "login wall"): codex's
//! onboarding shows its sign-in screen before any thread exists, so before any claim, and
//! with aterm's Codex lane retired (aterm 83bd43c206) nothing in aterm answers it or says it
//! waits. Only a person can sign in, so codex-alab tells one: it raises its keyed attention
//! (`meta set attention owner=codex-alab login: …`, at most 200 bytes) while the sign-in
//! step is up, and clears it as soon as the sign-in completes (the onboarding screen may go
//! on, to the folder-trust step, with the person signed in), or when the onboarding screen
//! ends without it (the person gone).
//!
//! One hook (`ALAB(login)` in `tui/src/onboarding/onboarding_screen.rs`, around the screen's
//! run, into `glue/login_screen.rs`) calls [`during`]. It is inert unless this is the
//! codex-alab binary ([`codex_alab_core::active`]), the screen shows the login step, and this
//! process runs in an aterm session ([`Endpoint::from_env`]); outside aterm, an older aterm
//! that refuses keyed attention, or an aterm that does not answer, nothing is raised and the
//! screen runs exactly as upstream's (fail closed). The raise runs beside the screen on a
//! connection of its own, never delaying its first frame; the clear at the screen's end waits
//! at most [`CLEAR_TIMEOUT`] past the raise's own bound.
//!
//! A screen that never reaches its clear (a SIGTERM, a SIGKILL, a panic or a restore on the
//! sign-in screen) leaves the attention standing: the next start that gets past the sign-in
//! clears it once ([`clear_stale`], called by the autopilot's first connection), only
//! codex-alab's own entry and only while it reads exactly this screen's text ([`text`]): the
//! autopilot's own `login: …` attentions (a sign-in refused or expired at a turn's end, a
//! sign-in box) say something still true and stand.

use std::collections::BTreeMap;
use std::future::Future;
use std::time::Duration;

use codex_alab_aterm::ATTENTION_OWNER;
use codex_alab_aterm::Client;
use codex_alab_aterm::Endpoint;
use codex_alab_aterm::Error as AtermError;
use codex_alab_aterm::Options;
use codex_alab_core::active;

/// The attention's kind (aterm shows `login: <why>`).
pub const KIND: &str = "login";

/// Why it is raised.
pub const WHY: &str = "signed out; codex's sign-in screen waits for a person";

/// How long the raise may take to reach aterm (a local exchange answers in milliseconds).
pub const RAISE_TIMEOUT: Duration = Duration::from_secs(2);

/// How long the end of the onboarding screen waits for the clear.
pub const CLEAR_TIMEOUT: Duration = Duration::from_secs(2);

/// Run the onboarding `screen` with codex-alab's login attention raised while its sign-in
/// step waits (module doc). `login_screen` is whether the screen shows the sign-in step;
/// `signed_in` says whether that step is done (read each time the screen wakes).
pub async fn during<F: Future>(
    login_screen: bool,
    signed_in: impl Fn() -> bool,
    screen: F,
) -> F::Output {
    let endpoint = if active() && login_screen {
        Endpoint::from_env()
    } else {
        None
    };
    during_with(endpoint, signed_in, screen).await
}

/// [`during`] for this `endpoint` (`None`: inert).
pub async fn during_with<F: Future>(
    endpoint: Option<Endpoint>,
    signed_in: impl Fn() -> bool,
    screen: F,
) -> F::Output {
    let Some(endpoint) = endpoint else {
        return screen.await;
    };
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    let mut attention = tokio::spawn(raise_until(endpoint, done_rx));
    let mut done_tx = Some(done_tx);
    let mut screen = std::pin::pin!(screen);
    let out = std::future::poll_fn(|cx| {
        let polled = screen.as_mut().poll(cx);
        // The screen wakes for every key, frame and answer of the login server: a sign-in
        // that completed shows right after the poll that took it, and the attention goes
        // then, not when the whole onboarding screen ends (review of 4cdc96090, finding 6).
        if done_tx.is_some()
            && signed_in()
            && let Some(done) = done_tx.take()
        {
            let _ = done.send(());
        }
        polled
    })
    .await;
    // The screen ended: the attention goes now if it has not (a sender dropped is the word).
    drop(done_tx);
    if tokio::time::timeout(RAISE_TIMEOUT * 2 + CLEAR_TIMEOUT, &mut attention)
        .await
        .is_err()
    {
        attention.abort();
    }
    out
}

/// Raise the attention, then clear it once `done` says so (or its sender is gone).
async fn raise_until(endpoint: Endpoint, done: tokio::sync::oneshot::Receiver<()>) {
    // The raise has its own bound: one cut off may have landed, and is cleared behind.
    let raised = tokio::time::timeout(RAISE_TIMEOUT * 2, raise(endpoint.clone()))
        .await
        .unwrap_or(Raised::Unknown);
    let _ = done.await;
    let clear = async {
        match raised {
            Raised::On(client) => {
                let _ = client.clear_attention().await;
                client.close();
            }
            // A raise cut off may have landed: clear on a connection of its own. Nothing else
            // of codex-alab's stands yet (the claim, and its attention, come after a thread).
            Raised::Unknown => {
                if let Ok(client) = Client::connect(endpoint, options()).await {
                    let _ = client.clear_attention().await;
                    client.close();
                }
            }
            Raised::No => {}
        }
    };
    let _ = tokio::time::timeout(CLEAR_TIMEOUT, clear).await;
}

/// The attention text the sign-in screen raises, as aterm shows it (`<kind>: <why>`).
pub fn text() -> String {
    format!("{KIND}: {WHY}")
}

/// Whether a `meta` reply's `fields` show a sign-in attention codex-alab left standing: the
/// entry aterm shows is codex-alab's (`attention_owner=codex-alab`) and reads exactly the
/// sign-in screen's [`text`] (review of fea034d20, finding 4: a `login: …` prefix also took
/// the autopilot's own sign-in attentions, still true). aterm shows only its newest entry, so
/// one under another owner's newer entry is not seen (and so not cleared: nothing is cleared
/// that cannot be read).
pub fn stale(fields: &BTreeMap<String, String>) -> bool {
    codex_alab_aterm::wire::field(fields, "attention_owner").as_deref() == Some(ATTENTION_OWNER)
        && codex_alab_aterm::wire::field(fields, "attention").as_deref() == Some(text().as_str())
}

/// At a start past the sign-in: clear codex-alab's own `login` attention a signed-out start
/// left standing ([`stale`]), and nothing else. `Ok(true)` when one was cleared.
pub async fn clear_stale(client: &Client) -> Result<bool, AtermError> {
    if !stale(&client.meta().await?) {
        return Ok(false);
    }
    client.clear_attention().await?;
    Ok(true)
}

/// What the raise did.
enum Raised {
    /// aterm took it, on this connection (the clear goes on it).
    On(Client),
    /// aterm did not take it (unreachable, refused): nothing to clear.
    No,
    /// It was cut off: it may have landed.
    Unknown,
}

fn options() -> Options {
    Options {
        timeout: RAISE_TIMEOUT,
        ..Options::default()
    }
}

/// Raise the attention on a connection of its own.
async fn raise(endpoint: Endpoint) -> Raised {
    let Ok(client) = Client::connect(endpoint, options()).await else {
        return Raised::No;
    };
    match client.set_attention(KIND, WHY).await {
        Ok(()) => Raised::On(client),
        Err(codex_alab_aterm::Error::Unanswered) => {
            client.close();
            Raised::Unknown
        }
        Err(_) => {
            client.close();
            Raised::No
        }
    }
}
