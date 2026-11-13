// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use crate::chatwidget::tests::render_bottom_popup;
use std::collections::VecDeque;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

struct ScriptedConsumer {
    requests: Mutex<Vec<assignment::Intent>>,
    observations: Mutex<VecDeque<assignment::Observation>>,
    reads: AtomicUsize,
    refused: Option<assignment::Refused>,
}

impl ScriptedConsumer {
    fn new(observations: impl IntoIterator<Item = assignment::Observation>) -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(Vec::new()),
            observations: Mutex::new(observations.into_iter().collect()),
            reads: AtomicUsize::new(0),
            refused: None,
        })
    }

    /// Core's answer when it starts nothing; the observation stays another operation's.
    fn refusing(refused: assignment::Refused) -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(Vec::new()),
            observations: Mutex::new(
                [observation(&"c".repeat(32), assignment::Progress::Applied)].into(),
            ),
            reads: AtomicUsize::new(0),
            refused: Some(refused),
        })
    }
}

impl assignment::Consumer for ScriptedConsumer {
    fn request<'a>(&'a self, intent: assignment::Intent) -> assignment::AdmissionFuture<'a> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(intent);
            self.refused.map_or(Ok(()), Err)
        })
    }

    fn observation(&self) -> Option<assignment::Observation> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.observations.lock().unwrap().pop_front()
    }
}

fn observation(operation: &str, progress: assignment::Progress) -> assignment::Observation {
    assignment::Observation {
        operation: operation.into(),
        progress,
        target: None,
        mode: None,
        reason: None,
    }
}

fn intent(thread: ThreadId) -> assignment::Intent {
    assignment::Intent {
        operation: "a".repeat(32),
        thread: thread.to_string(),
        selection: assignment::Selection::Auto {
            revision: "b".repeat(64),
        },
    }
}

async fn apply_next(
    chat: &mut ChatWidget,
    events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) {
    let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("account observation must arrive")
        .expect("app event channel remains open");
    let AppEvent::AlabAccountsReady {
        view,
        thread_id,
        text,
    } = event
    else {
        panic!("assignment must emit only local account metadata");
    };
    assert!(
        matches!(&text, Update::Observation(_)),
        "assignment progress must invalidate pending snapshots"
    );
    chat.alab_accounts_ready(view, thread_id, text);
}

#[tokio::test]
async fn reconciling_assignment_keeps_observing_until_the_same_operation_applies() {
    let (mut chat, tx, mut events, mut ops) = make_chatwidget_manual_with_sender().await;
    while events.try_recv().is_ok() {}
    while ops.try_recv().is_ok() {}
    chat.apply_external_edit("unfinished prompt".into());
    let thread = ThreadId::new();
    chat.thread_id = Some(thread);
    let view = chat.alab_accounts_view();
    let intent = intent(thread);
    let consumer = ScriptedConsumer::new([
        observation(&"c".repeat(32), assignment::Progress::Applied),
        observation(&intent.operation, assignment::Progress::Reconciling),
        observation(&intent.operation, assignment::Progress::Applied),
    ]);
    tokio::time::pause();
    let running = tokio::spawn(request_and_observe_assignment(
        consumer.clone(),
        intent.clone(),
        view,
        Some(thread),
        tx,
    ));

    apply_next(&mut chat, &mut events).await;
    assert!(render_bottom_popup(&chat, 100).contains("needs reconciliation"));
    apply_next(&mut chat, &mut events).await;
    assert!(render_bottom_popup(&chat, 100).contains("account selection is applied"));
    tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("terminal observation stops polling")
        .expect("observation task succeeds");

    assert_eq!(*consumer.requests.lock().unwrap(), vec![intent]);
    assert_eq!(consumer.reads.load(Ordering::SeqCst), 3);
    assert_eq!(chat.composer_text_with_pending(), "unfinished prompt");
    assert!(events.try_recv().is_err(), "no transcript event is emitted");
    assert!(ops.try_recv().is_err(), "no model operation is emitted");
}

#[tokio::test]
async fn closing_a_reconciling_assignment_view_stops_observation_without_another_request() {
    let (mut chat, tx, mut events, mut ops) = make_chatwidget_manual_with_sender().await;
    while events.try_recv().is_ok() {}
    while ops.try_recv().is_ok() {}
    let thread = ThreadId::new();
    chat.thread_id = Some(thread);
    let view = chat.alab_accounts_view();
    let intent = intent(thread);
    let consumer = ScriptedConsumer::new([
        observation(&intent.operation, assignment::Progress::Reconciling),
        observation(&intent.operation, assignment::Progress::Applied),
    ]);
    tokio::time::pause();
    let running = tokio::spawn(request_and_observe_assignment(
        consumer.clone(),
        intent.clone(),
        view.clone(),
        Some(thread),
        tx,
    ));

    apply_next(&mut chat, &mut events).await;
    assert!(render_bottom_popup(&chat, 100).contains("needs reconciliation"));
    chat.bottom_pane.dismiss_view_by_id("alab-accounts");
    assert!(view.upgrade().is_none());
    tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("closing the view stops polling")
        .expect("observation task succeeds");

    assert_eq!(*consumer.requests.lock().unwrap(), vec![intent]);
    assert_eq!(consumer.reads.load(Ordering::SeqCst), 1);
    assert_eq!(consumer.observations.lock().unwrap().len(), 1);
    assert!(events.try_recv().is_err());
    assert!(ops.try_recv().is_err());
}

#[tokio::test]
async fn a_refused_request_shows_its_reason_once_and_observes_nothing() {
    use assignment::Refused;
    for (refused, sentence) in [
        (
            Refused::Unavailable,
            "Account changes are unavailable on this host.",
        ),
        (
            Refused::Busy,
            "Another account change is already pending for this session.",
        ),
        (
            Refused::Pending,
            "Another account change is already pending for this session.",
        ),
        (
            Refused::Invalid,
            "The configured account could not be verified.",
        ),
    ] {
        let (mut chat, tx, mut events, mut ops) = make_chatwidget_manual_with_sender().await;
        while events.try_recv().is_ok() {}
        while ops.try_recv().is_ok() {}
        let thread = ThreadId::new();
        chat.thread_id = Some(thread);
        let view = chat.alab_accounts_view();
        let intent = intent(thread);
        let consumer = ScriptedConsumer::refusing(refused);
        // Not spawned: a refusal returns without polling, so this completes on its own.
        tokio::time::timeout(
            Duration::from_secs(5),
            request_and_observe_assignment(
                consumer.clone(),
                intent.clone(),
                view,
                Some(thread),
                tx,
            ),
        )
        .await
        .expect("a refused request does not wait for an observation");

        apply_next(&mut chat, &mut events).await;
        let pane = render_bottom_popup(&chat, 100);
        assert!(pane.contains(sentence), "{refused:?}: {pane}");
        assert!(
            !pane.contains("unavailable or an existing request is pending"),
            "{refused:?} is worded by its variant: {pane}"
        );
        assert_eq!(*consumer.requests.lock().unwrap(), vec![intent]);
        assert_eq!(
            consumer.reads.load(Ordering::SeqCst),
            0,
            "{refused:?}: another operation's observation is never shown as this one's"
        );
        assert!(events.try_recv().is_err(), "one local update only");
        assert!(ops.try_recv().is_err(), "no model operation is emitted");
    }
}
