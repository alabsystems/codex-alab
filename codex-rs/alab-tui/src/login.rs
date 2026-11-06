// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A signed-out first launch inside aterm (CONTRACT §2 (a), §5 "login wall"): codex's
//! onboarding shows its sign-in screen before any thread exists, so before any claim, and
//! with aterm's Codex lane retired (aterm 83bd43c206) nothing in aterm answers it or says it
//! waits. Only a person can sign in, so codex-alab tells one: it raises its keyed attention
//! (`meta set attention owner=codex-alab login: …`, at most 200 bytes) while the sign-in
//! step is up, and clears it as soon as the sign-in completes (the onboarding screen may go
//! on, to the folder-trust step, with the person signed in), or when the onboarding screen
//! ends without it (the person gone). The raise and the clear are every startup screen's
//! ([`crate::waiting`]).
//!
//! One hook (`ALAB(login)` in `tui/src/onboarding/onboarding_screen.rs`, around the screen's
//! run, into `glue/login_screen.rs`) calls [`during`]. It is inert unless this is the
//! codex-alab binary ([`codex_alab_core::active`]), the screen shows the login step, and this
//! process runs in an aterm session ([`Endpoint::from_env`]); outside aterm, an older aterm
//! that refuses keyed attention, or an aterm that does not answer, nothing is raised and the
//! screen runs exactly as upstream's (fail closed).
//!
//! A screen that never reaches its clear (a SIGTERM, a SIGKILL, a panic or a restore on the
//! sign-in screen) leaves the attention standing: the next start that gets past the sign-in
//! begins one pending cleanup lifetime in the supervisor (`waiting::stale`): transient errors
//! or another visible owner are retried before a claim or foreign ownership retires it. Only
//! codex-alab's own visible entry reading exactly this screen's text ([`text`]) is cleared: the
//! autopilot's own `login: …` attentions (a sign-in refused or expired at a turn's end, a
//! sign-in box) say something still true and stand.

use std::future::Future;

use codex_alab_aterm::Endpoint;
use codex_alab_core::active;

use crate::waiting::Waiting;

pub use crate::waiting::CLEAR_TIMEOUT;
pub use crate::waiting::RAISE_TIMEOUT;

/// The attention's kind (aterm shows `login: <why>`).
pub const KIND: &str = "login";

/// Why it is raised.
pub const WHY: &str = "signed out; codex's sign-in screen waits for a person";

/// Run the onboarding `screen` with codex-alab's login attention raised while its sign-in
/// step waits (module doc). `login_screen` is whether the screen shows the sign-in step;
/// `signed_in` says whether that step is done (read each time the screen wakes).
/// The screen goes on the heap as the hook calls in ([`crate::waiting`]'s module doc: an
/// `async fn` carried the whole screen in its own state).
pub fn during<F: Future, D: Fn() -> bool>(
    login_screen: bool,
    signed_in: D,
    screen: F,
) -> impl Future<Output = F::Output> + use<F, D> {
    let endpoint = if active() && login_screen {
        Endpoint::from_env()
    } else {
        None
    };
    during_with(endpoint, signed_in, Box::pin(screen))
}

/// [`during`] for this `endpoint` (`None`: inert).
pub async fn during_with<F: Future>(
    endpoint: Option<Endpoint>,
    signed_in: impl Fn() -> bool,
    screen: F,
) -> F::Output {
    crate::waiting::during_with(endpoint, waiting(), signed_in, screen).await
}

/// The sign-in screen's attention.
pub fn waiting() -> Waiting {
    Waiting {
        kind: KIND,
        why: WHY.to_string(),
    }
}

/// The attention text the sign-in screen raises, as aterm shows it (`<kind>: <why>`).
pub fn text() -> String {
    format!("{KIND}: {WHY}")
}
