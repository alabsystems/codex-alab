// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Lexical actual-caller presentation DATA, only for original Resume. This
//! cannot supply Native origin/S1/S2/currentness, Core identity or UI approval.
use codex_app_server_client::ClientPresentation;
use codex_app_server_client::InitialAction;
use color_eyre::eyre::Result;
use color_eyre::eyre::bail;
use std::future::Future;

tokio::task_local! { static PRESENTATION: ClientPresentation; }

pub(super) fn for_action(
    action: &InitialAction,
    explicit: ClientPresentation,
) -> ClientPresentation {
    if matches!(action, InitialAction::Resume(_)) {
        PRESENTATION.try_with(|actual| *actual).unwrap_or(explicit)
    } else {
        // Start and genuine ForkPresentation retain their original explicit
        // caller policy even if another surrounding Resume future is scoped.
        explicit
    }
}
pub(super) async fn scope<T>(
    presentation: ClientPresentation,
    original: impl Future<Output = Result<T>>,
) -> Result<T> {
    if PRESENTATION.try_with(|_| ()).is_ok() {
        bail!("original Resume already owns its caller presentation intent");
    }
    PRESENTATION.scope(presentation, original).await
}

#[cfg(test)]
#[path = "owned_local_resume_intent_tests.rs"]
mod tests;
