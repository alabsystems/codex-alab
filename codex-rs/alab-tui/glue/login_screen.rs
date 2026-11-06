// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(login) glue, compiled inside codex-tui as `onboarding::onboarding_screen::alab_login`
//! (one `#[path]` line at the end of `tui/src/onboarding/onboarding_screen.rs`): inside aterm,
//! a signed-out sign-in screen raises codex-alab's keyed attention (`codex_alab_tui::login`,
//! CONTRACT §2 (a)) until the sign-in completes or the screen ends.
//!
//! The one call is [`during`], around `run_onboarding_screen` in `run_onboarding_app`. It
//! reads, before the screen runs, whether it shows the sign-in step, and keeps that step's own
//! sign-in state to read whether the person is signed in yet: the attention goes as soon as
//! they are, not when the folder-trust step after it ends.

use std::future::Future;
use std::sync::Arc;

use super::OnboardingScreen;
use super::Step;
use crate::onboarding::auth::SignInState;

/// Run `run(screen)` with the sign-in attention (`codex_alab_tui::login::during`). A plain
/// function, so the screen's future goes on the heap as the hook calls in (an `async fn` here
/// carried the whole screen in its own state too: `codex_alab_tui::waiting`'s module doc).
pub(super) fn during<F, Fut>(screen: OnboardingScreen, run: F) -> impl Future<Output = Fut::Output>
where
    F: FnOnce(OnboardingScreen) -> Fut,
    Fut: Future,
{
    let state = screen.steps.iter().find_map(|step| match step {
        Step::Auth(widget) => Some(Arc::clone(&widget.sign_in_state)),
        _ => None,
    });
    let login_screen = state.is_some();
    let done = move || {
        state
            .as_ref()
            .is_some_and(|state| state.read().is_ok_and(|state| signed_in(&state)))
    };
    codex_alab_tui::login::during(login_screen, done, run(screen))
}

/// Whether the sign-in step's state is signed in: the step's own complete states, and the
/// ChatGPT success message shown before it (the sign-in itself is done there). A state this
/// glue does not know reads as not yet: the attention then goes when the screen ends.
fn signed_in(state: &SignInState) -> bool {
    matches!(
        state,
        SignInState::ChatGptSuccessMessage
            | SignInState::ChatGptSuccess
            | SignInState::ApiKeyConfigured
            | SignInState::BedrockConfigured
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The probe says signed in exactly where the sign-in is done (negative control: the
    /// state that still waits for the person to pick a way in).
    #[test]
    fn signed_in_is_the_steps_done_states() {
        assert!(signed_in(&SignInState::ChatGptSuccessMessage));
        assert!(signed_in(&SignInState::ChatGptSuccess));
        assert!(signed_in(&SignInState::ApiKeyConfigured));
        assert!(signed_in(&SignInState::BedrockConfigured));
        assert!(!signed_in(&SignInState::PickMode));
    }
}
