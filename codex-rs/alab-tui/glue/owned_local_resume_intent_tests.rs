// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Lower caller-intent routing only; no Native/Core/S1 resource fabricated.
use super::*;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

fn resume() -> InitialAction {
    InitialAction::Resume("original-thread-data".into())
}
fn fork() -> InitialAction {
    InitialAction::Fork("original-parent-data".into())
}

#[tokio::test]
async fn actual_scoped_resume_intent_changes_only_resume_and_restores_afterward() -> Result<()> {
    assert_eq!(
        for_action(&resume(), ClientPresentation::Primary),
        ClientPresentation::Primary
    );
    scope(ClientPresentation::SideConversation, async {
        assert_eq!(
            for_action(&resume(), ClientPresentation::Primary),
            ClientPresentation::SideConversation
        );
        tokio::task::yield_now().await;
        assert_eq!(
            for_action(&resume(), ClientPresentation::Primary),
            ClientPresentation::SideConversation
        );
        assert_eq!(
            for_action(&InitialAction::Start, ClientPresentation::Primary),
            ClientPresentation::Primary
        );
        assert_eq!(
            for_action(&fork(), ClientPresentation::Primary),
            ClientPresentation::Primary
        );
        assert_eq!(
            for_action(&fork(), ClientPresentation::SideConversation),
            ClientPresentation::SideConversation
        );
        Ok(())
    })
    .await?;
    assert_eq!(
        for_action(&resume(), ClientPresentation::Primary),
        ClientPresentation::Primary
    );
    Ok(())
}

#[tokio::test]
async fn nested_changed_intent_is_denied_before_the_original_future_is_polled() -> Result<()> {
    let polls = AtomicUsize::new(0);
    scope(ClientPresentation::SideConversation, async {
        let changed = scope(ClientPresentation::Primary, async {
            polls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .await;
        assert!(changed.is_err());
        assert_eq!(polls.load(Ordering::SeqCst), 0);
        assert_eq!(
            for_action(&resume(), ClientPresentation::Primary),
            ClientPresentation::SideConversation
        );
        Ok(())
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn cancelled_intent_scope_does_not_leak_into_the_next_actual_poll() -> Result<()> {
    let mut work = Box::pin(scope(ClientPresentation::SideConversation, async {
        assert_eq!(
            for_action(&resume(), ClientPresentation::Primary),
            ClientPresentation::SideConversation
        );
        std::future::pending::<Result<()>>().await
    }));
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(work.as_mut().poll(cx).is_pending()))
            .await
    );
    assert_eq!(
        for_action(&resume(), ClientPresentation::Primary),
        ClientPresentation::Primary,
        "interleaved caller polls must not inherit the suspended future's intent"
    );
    drop(work);
    assert_eq!(
        for_action(&resume(), ClientPresentation::Primary),
        ClientPresentation::Primary
    );
    Ok(())
}
