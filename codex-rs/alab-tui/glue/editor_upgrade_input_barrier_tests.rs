// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use std::collections::VecDeque;
use std::sync::atomic::AtomicUsize;

#[derive(Default)]
struct Source {
    events: VecDeque<EventResult>,
    polls: Arc<AtomicUsize>,
}
impl EventSource for Source {
    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<EventResult>> {
        self.polls.fetch_add(1, Ordering::Relaxed);
        match self.events.pop_front() {
            Some(event) => Poll::Ready(Some(event)),
            None => Poll::Pending,
        }
    }
}
fn stream(
    broker: Arc<EventBroker<Source>>,
    draws: broadcast::Receiver<()>,
) -> TuiEventStream<Source> {
    TuiEventStream::new(
        broker,
        draws,
        Arc::new(AtomicBool::new(true)),
        crate::tui::job_control::SuspendContext::new(),
        Arc::new(AtomicBool::new(false)),
    )
}
#[tokio::test(flavor = "current_thread")]
async fn runtime_barrier_does_not_poll_or_lose_input_when_startup_stream_is_replaced() {
    let scope = codex_alab_tui::upgrade_editor_startup::TestScope::deferred();
    let polls = Arc::new(AtomicUsize::new(0));
    let broker = Arc::new(EventBroker::new());
    *broker.state.lock().unwrap() = EventBrokerState::Running(Source {
        events: VecDeque::from([Ok(Event::Key(KeyCode::Char('x').into()))]),
        polls: polls.clone(),
    });
    let (draws, rx) = broadcast::channel(8);
    let mut startup = stream(broker.clone(), rx);
    let mut context = Context::from_waker(futures::task::noop_waker_ref());
    assert!(startup.poll_crossterm_event(&mut context).is_pending());
    assert_eq!(polls.load(Ordering::Relaxed), 0);
    drop(startup);
    let mut restored = stream(broker, draws.subscribe());
    assert!(restored.poll_crossterm_event(&mut context).is_pending());
    scope.release();
    assert!(matches!(
        restored.poll_crossterm_event(&mut context),
        Poll::Ready(Some(TuiEvent::Key(KeyEvent {
            code: KeyCode::Char('x'),
            ..
        })))
    ));
    assert_eq!(polls.load(Ordering::Relaxed), 1);
}
