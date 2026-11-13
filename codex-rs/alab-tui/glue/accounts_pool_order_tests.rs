// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use super::*;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use crate::chatwidget::tests::render_bottom_popup;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

type Completion = oneshot::Sender<Result<String, codex_alab_aterm::Error>>;

fn controlled_fetch(
    view: Weak<Mutex<View>>,
    thread_id: Option<ThreadId>,
    tx: AppEventSender,
) -> (Completion, JoinHandle<()>) {
    let (complete, reply) = oneshot::channel::<Result<String, codex_alab_aterm::Error>>();
    let task = spawn_snapshot(view, thread_id, tx, async move {
        reply
            .await
            .expect("test supplies the fetch result")
            .map_err(|error| failure(&error).to_owned())
    })
    .expect("account view is open");
    (complete, task)
}

async fn completed_update(
    task: JoinHandle<()>,
    events: &mut UnboundedReceiver<AppEvent>,
) -> (Weak<Mutex<View>>, Option<ThreadId>, Update) {
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("fetch completes")
        .expect("fetch task succeeds");
    let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("snapshot update arrives")
        .expect("app channel is open");
    let AppEvent::AlabAccountsReady {
        view,
        thread_id,
        text,
    } = event
    else {
        panic!("fetch emits only local account metadata");
    };
    (view, thread_id, text)
}

fn apply(chat: &mut ChatWidget, ready: (Weak<Mutex<View>>, Option<ThreadId>, Update)) {
    chat.alab_accounts_ready(ready.0, ready.1, ready.2);
}

async fn refresh_with(
    chat: &mut ChatWidget,
    events: &mut UnboundedReceiver<AppEvent>,
    view: Weak<Mutex<View>>,
    tx: AppEventSender,
    result: Result<String, codex_alab_aterm::Error>,
) {
    let (done, task) = controlled_fetch(view, chat.thread_id(), tx);
    done.send(result).unwrap();
    apply(chat, completed_update(task, events).await);
}

#[tokio::test]
async fn cached_snapshot_survives_failed_refreshes_then_recovers() {
    let (mut chat, tx, mut events, mut ops) = make_chatwidget_manual_with_sender().await;
    chat.apply_external_edit("unfinished draft".into());
    while events.try_recv().is_ok() {}
    while ops.try_recv().is_ok() {}
    let view = chat.alab_accounts_view();
    let saved = "Saved account · 21% used\nObserved: Nov 12 09:10 -08:00";
    refresh_with(
        &mut chat,
        &mut events,
        view.clone(),
        tx.clone(),
        Ok(saved.into()),
    )
    .await;

    for error in [
        codex_alab_aterm::Error::Unanswered,
        codex_alab_aterm::Error::Refused("ERR private token or path".into()),
    ] {
        let before = render_bottom_popup(&chat, 100);
        let expected = failure(&error);
        let (done, task) = controlled_fetch(view.clone(), chat.thread_id(), tx.clone());
        assert_eq!(render_bottom_popup(&chat, 100), before);
        done.send(Err(error)).unwrap();
        let ready = completed_update(task, &mut events).await;
        let Update::Snapshot { token, result } = &ready.2 else {
            panic!("failure must retain its snapshot token");
        };
        assert_eq!(result.as_ref().unwrap_err(), expected);
        let duplicate = Update::Snapshot {
            token: token.clone(),
            result: Ok("duplicate failure response must not apply".into()),
        };
        apply(&mut chat, ready);
        let shown = render_bottom_popup(&chat, 100);
        assert!(shown.contains("Saved account · 21% used"));
        assert!(shown.contains("Observed: Nov 12 09:10 -08:00"));
        assert!(shown.contains(expected));
        assert_eq!(shown.matches("Showing previously loaded").count(), 1);
        assert!(!shown.contains("private token or path"));
        chat.alab_accounts_ready(view.clone(), chat.thread_id(), duplicate);
        assert_eq!(render_bottom_popup(&chat, 100), shown);
    }

    refresh_with(
        &mut chat,
        &mut events,
        view,
        tx,
        Ok("Current account · 35% used\nObserved: Nov 12 09:20 -08:00".into()),
    )
    .await;
    let shown = render_bottom_popup(&chat, 100);
    assert!(shown.contains("Current account · 35% used"));
    assert!(shown.contains("Observed: Nov 12 09:20 -08:00"));
    assert!(!shown.contains("Saved account"));
    assert!(!shown.contains("Showing previously loaded"));
    assert!(!shown.contains("unavailable"));
    assert!(!shown.contains("refused"));
    assert_eq!(chat.composer_text_with_pending(), "unfinished draft");
    assert!(events.try_recv().is_err(), "no transcript event is emitted");
    assert!(ops.try_recv().is_err(), "no model operation is emitted");
}

#[tokio::test]
async fn initial_refresh_failure_shows_only_the_sanitized_error() {
    let (mut chat, tx, mut events, mut ops) = make_chatwidget_manual_with_sender().await;
    while events.try_recv().is_ok() {}
    while ops.try_recv().is_ok() {}
    let view = chat.alab_accounts_view();
    refresh_with(
        &mut chat,
        &mut events,
        view,
        tx,
        Err(codex_alab_aterm::Error::Refused(
            "ERR private token or path".into(),
        )),
    )
    .await;
    let shown = render_bottom_popup(&chat, 100);
    assert!(shown.contains("aterm refused account inspection"));
    assert!(!shown.contains("private token or path"));
    assert!(!shown.contains("Showing previously loaded"));
    assert!(!shown.contains("Loading subscription accounts"));
    assert!(!shown.contains("No accounts are configured"));
    assert!(events.try_recv().is_err());
    assert!(ops.try_recv().is_err());
}

#[tokio::test]
async fn failed_refresh_after_assignment_cannot_restore_the_old_account_snapshot() {
    let (mut chat, tx, mut events, mut ops) = make_chatwidget_manual_with_sender().await;
    chat.apply_external_edit("unfinished draft".into());
    while events.try_recv().is_ok() {}
    while ops.try_recv().is_ok() {}
    let thread = chat.thread_id();
    let view = chat.alab_accounts_view();
    refresh_with(
        &mut chat,
        &mut events,
        view.clone(),
        tx.clone(),
        Ok("Pre-assignment account · 21% used".into()),
    )
    .await;
    let (old_done, old_task) = controlled_fetch(view.clone(), thread, tx.clone());
    chat.alab_accounts_ready(
        view.clone(),
        thread,
        Update::Observation("Changing this session's account…".into()),
    );
    old_done
        .send(Err(codex_alab_aterm::Error::Unanswered))
        .unwrap();
    apply(&mut chat, completed_update(old_task, &mut events).await);
    assert!(render_bottom_popup(&chat, 100).contains("Changing this session's account"));

    chat.handle_key_event(crossterm::event::KeyCode::Enter.into());
    let AppEvent::AlabAccountsRefresh {
        view: refreshed,
        thread_id,
    } = events.try_recv().unwrap()
    else {
        panic!("Enter requests a new post-assignment snapshot");
    };
    let (done, task) = controlled_fetch(refreshed, thread_id, tx.clone());
    done.send(Err(codex_alab_aterm::Error::Unanswered)).unwrap();
    apply(&mut chat, completed_update(task, &mut events).await);
    let shown = render_bottom_popup(&chat, 100);
    assert!(shown.contains("aterm account information is unavailable"));
    assert!(!shown.contains("Pre-assignment account"));
    assert!(!shown.contains("Showing previously loaded"));

    refresh_with(
        &mut chat,
        &mut events,
        view,
        tx,
        Ok("Destination account · 35% used".into()),
    )
    .await;
    let shown = render_bottom_popup(&chat, 100);
    assert!(shown.contains("Destination account · 35% used"));
    assert!(!shown.contains("Pre-assignment account"));
    assert!(!shown.contains("unavailable"));
    assert_eq!(chat.composer_text_with_pending(), "unfinished draft");
    assert!(events.try_recv().is_err());
    assert!(ops.try_recv().is_err());
}

#[tokio::test]
async fn account_refresh_completions_cannot_overwrite_a_newer_result() {
    for older in [
        Ok("older account snapshot".to_owned()),
        Err(codex_alab_aterm::Error::Unanswered),
    ] {
        let (mut chat, tx, mut events, mut ops) = make_chatwidget_manual_with_sender().await;
        chat.apply_external_edit("unfinished draft".into());
        while events.try_recv().is_ok() {}
        while ops.try_recv().is_ok() {}
        let thread = chat.thread_id();
        let view = chat.alab_accounts_view();
        // Both reservations happen before either future runs; completion order must not
        // determine which request owns this pane.
        let (older_done, older_task) = controlled_fetch(view.clone(), thread, tx.clone());
        let (newer_done, newer_task) = controlled_fetch(view.clone(), thread, tx);

        newer_done
            .send(Ok("newer account snapshot".into()))
            .unwrap();
        let newer = completed_update(newer_task, &mut events).await;
        let Update::Snapshot { token, .. } = &newer.2 else {
            panic!("fetch result must carry its snapshot token");
        };
        let duplicate = Update::Snapshot {
            token: token.clone(),
            result: Ok("duplicate response must not apply".into()),
        };
        apply(&mut chat, newer);
        let newer_view = render_bottom_popup(&chat, 100);
        assert!(newer_view.contains("newer account snapshot"));

        older_done.send(older).unwrap();
        apply(&mut chat, completed_update(older_task, &mut events).await);
        assert_eq!(render_bottom_popup(&chat, 100), newer_view);
        chat.alab_accounts_ready(view.clone(), thread, duplicate);
        assert_eq!(render_bottom_popup(&chat, 100), newer_view);
        assert_eq!(chat.composer_text_with_pending(), "unfinished draft");
        assert!(events.try_recv().is_err(), "no transcript event is emitted");
        assert!(ops.try_recv().is_err(), "no model operation is emitted");
    }
}

#[tokio::test]
async fn older_completion_cannot_consume_the_pending_newer_refresh() {
    let (mut chat, tx, mut events, mut ops) = make_chatwidget_manual_with_sender().await;
    chat.apply_external_edit("unfinished draft".into());
    while events.try_recv().is_ok() {}
    while ops.try_recv().is_ok() {}
    let thread = chat.thread_id();
    let view = chat.alab_accounts_view();
    let before = render_bottom_popup(&chat, 100);
    let (older_done, older_task) = controlled_fetch(view.clone(), thread, tx.clone());
    let (newer_done, newer_task) = controlled_fetch(view, thread, tx);

    older_done
        .send(Ok("older account snapshot".into()))
        .unwrap();
    apply(&mut chat, completed_update(older_task, &mut events).await);
    assert_eq!(render_bottom_popup(&chat, 100), before);

    newer_done
        .send(Ok("newer account snapshot".into()))
        .unwrap();
    apply(&mut chat, completed_update(newer_task, &mut events).await);
    assert!(render_bottom_popup(&chat, 100).contains("newer account snapshot"));
    assert_eq!(chat.composer_text_with_pending(), "unfinished draft");
    assert!(events.try_recv().is_err(), "no transcript event is emitted");
    assert!(ops.try_recv().is_err(), "no model operation is emitted");
}

#[tokio::test]
async fn assignment_observation_invalidates_refresh_until_a_later_explicit_refresh() {
    let (mut chat, tx, mut events, mut ops) = make_chatwidget_manual_with_sender().await;
    chat.apply_external_edit("unfinished draft".into());
    while events.try_recv().is_ok() {}
    while ops.try_recv().is_ok() {}
    let thread = chat.thread_id();
    let view = chat.alab_accounts_view();
    let (older_done, older_task) = controlled_fetch(view.clone(), thread, tx.clone());
    chat.alab_accounts_ready(
        view.clone(),
        thread,
        Update::Observation("Changing this session's account…".into()),
    );
    older_done
        .send(Ok("pre-assignment snapshot".into()))
        .unwrap();
    apply(&mut chat, completed_update(older_task, &mut events).await);
    assert!(render_bottom_popup(&chat, 100).contains("Changing this session's account"));

    chat.handle_key_event(crossterm::event::KeyCode::Enter.into());
    let AppEvent::AlabAccountsRefresh { view, thread_id } = events.try_recv().unwrap() else {
        panic!("Enter requests an explicit refresh of the same pane");
    };
    let (newer_done, newer_task) = controlled_fetch(view, thread_id, tx);
    newer_done
        .send(Ok("post-assignment snapshot".into()))
        .unwrap();
    apply(&mut chat, completed_update(newer_task, &mut events).await);
    assert!(render_bottom_popup(&chat, 100).contains("post-assignment snapshot"));
    assert_eq!(chat.composer_text_with_pending(), "unfinished draft");
    assert!(events.try_recv().is_err(), "no transcript event is emitted");
    assert!(ops.try_recv().is_err(), "no model operation is emitted");
}

#[tokio::test]
async fn snapshot_completions_respect_thread_and_view_ownership() {
    let (mut chat, tx, mut events, mut ops) = make_chatwidget_manual_with_sender().await;
    chat.apply_external_edit("unfinished draft".into());
    while events.try_recv().is_ok() {}
    while ops.try_recv().is_ok() {}
    let thread = Some(ThreadId::new());
    chat.thread_id = thread;
    let view = chat.alab_accounts_view();
    let (done, task) = controlled_fetch(view.clone(), thread, tx.clone());
    chat.thread_id = Some(ThreadId::new());
    done.send(Ok("wrong thread snapshot".into())).unwrap();
    apply(&mut chat, completed_update(task, &mut events).await);
    assert!(render_bottom_popup(&chat, 100).contains("Loading subscription accounts"));

    chat.thread_id = thread;
    let (done, task) = controlled_fetch(view.clone(), thread, tx);
    let replacement = chat.alab_accounts_view();
    assert!(view.upgrade().is_none());
    done.send(Ok("replaced view snapshot".into())).unwrap();
    apply(&mut chat, completed_update(task, &mut events).await);
    assert!(replacement.upgrade().is_some());
    assert!(render_bottom_popup(&chat, 100).contains("Loading subscription accounts"));
    assert_eq!(chat.composer_text_with_pending(), "unfinished draft");
    assert!(events.try_recv().is_err(), "no transcript event is emitted");
    assert!(ops.try_recv().is_err(), "no model operation is emitted");
}
