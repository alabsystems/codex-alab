// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Deterministic overflow schedules. No test activates ALab process-wide.
//! Manual ready polling prevents the worker from running between the two sends.

use super::*;
use crate::app::FeedbackThreadEvent;
use crate::app::ThreadEventChannel;
use crate::app_command::AppCommand;
use crate::app_event::AppEvent;
use crate::app_event::FeedbackCategory;
use crate::app_event::HistoryBatchEntryResponse;
use crate::app_event::HistoryLookupResponse;
use codex_alab_tui::boxes;
use codex_alab_tui::session::ProgramState;
use codex_alab_tui::turns;
use codex_app_server_protocol::CodexErrorInfo;
use codex_app_server_protocol::ErrorNotification;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ToolRequestUserInputParams;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnError;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::WarningNotification;
use codex_features::Feature;
use codex_utils_absolute_path::AbsolutePathBuf;
use futures::FutureExt;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use std::future::Future;
use std::task::Poll;
use std::time::Duration;

const TURN: &str = "fifo-capacity-turn";
const NOTICE: &str = "Model is at capacity; retrying automatically (interrupt to stop)...";

fn ready<F: Future>(future: F) -> F::Output {
    // Contended locks or any genuinely asynchronous dependency are setup errors.
    // Disable only Tokio's artificial cooperative yield for this ready-only phase.
    tokio::task::unconstrained(future)
        .now_or_never()
        .expect("fixture ingress must finish in one manual poll without yielding")
}

fn scope(enabled: bool) -> interface::Scope {
    let state = Box::leak(Box::new(interface::State::new()));
    let scope = interface::scoped(state);
    interface::configure(enabled);
    scope
}

#[expect(
    clippy::print_stderr,
    reason = "records the retained private fixture home for failed test diagnosis"
)]
async fn app_fixture() -> (
    Box<App>,
    mpsc::UnboundedReceiver<AppEvent>,
    mpsc::UnboundedReceiver<AppCommand>,
) {
    let mut fixture = crate::app::tests::make_test_app_with_channels().await;
    // Keep the newly created fixture home before any assertion or App Drop.
    let home = fixture
        .0
        ._test_codex_home
        .take()
        .expect("FIFO fixture has a private Codex home")
        .keep();
    eprintln!("ALab retained FIFO test home: {}", home.display());
    let project = home.join("project");
    std::fs::create_dir_all(&project).expect("create private FIFO fixture project");
    fixture.0.config.cwd =
        AbsolutePathBuf::try_from(project).expect("absolute private FIFO fixture project");
    // FIFO switches need no curated-plugin startup or external catalog discovery.
    let _ = fixture.0.config.features.disable(Feature::Plugins);
    fixture
}

fn channel(
    app: &mut App,
    thread: ThreadId,
) -> (
    mpsc::Sender<ThreadBufferedEvent>,
    mpsc::Receiver<ThreadBufferedEvent>,
) {
    app.thread_event_channels
        .insert(thread, ThreadEventChannel::new(1));
    app.active_thread_id = Some(thread);
    ready(app.set_thread_active(thread, true));
    let channel = app.thread_event_channels.get_mut(&thread).unwrap();
    (channel.sender.clone(), channel.receiver.take().unwrap())
}

fn turn(status: TurnStatus) -> Turn {
    Turn {
        id: TURN.into(),
        root_turn_id: None,
        items_view: TurnItemsView::Full,
        items: Vec::new(),
        status,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }
}

fn retry(thread: ThreadId) -> ServerNotification {
    ServerNotification::Error(ErrorNotification {
        thread_id: thread.to_string(),
        turn_id: TURN.into(),
        will_retry: true,
        error: TurnError {
            message: NOTICE.into(),
            codex_error_info: Some(CodexErrorInfo::ServerOverloaded),
            additional_details: None,
            misalignment: None,
        },
    })
}

fn terminal(thread: ThreadId) -> ServerNotification {
    ServerNotification::TurnCompleted(TurnCompletedNotification {
        thread_id: thread.to_string(),
        turn: turn(TurnStatus::Completed),
    })
}

fn filler() -> ThreadBufferedEvent {
    ThreadBufferedEvent::HistoryEntryResponse(HistoryLookupResponse::Entry {
        offset: 999,
        log_id: 1,
        entry: None,
    })
}

fn tag(event: &ThreadBufferedEvent) -> &'static str {
    match event {
        ThreadBufferedEvent::Notification(notification) => match notification.as_ref() {
            ServerNotification::Error(_) => "retry",
            ServerNotification::TurnCompleted(_) => "terminal",
            ServerNotification::Warning(_) => "warning",
            _ => "notification",
        },
        ThreadBufferedEvent::Request(_) => "request",
        ThreadBufferedEvent::HistoryEntryResponse(_) => "history",
        ThreadBufferedEvent::FeedbackSubmission(_) => "feedback",
    }
}

async fn receive<T>(receiver: &mut mpsc::Receiver<T>) -> T {
    tokio::time::timeout(Duration::from_secs(60), receiver.recv())
        .await
        .expect("hang bound, never an ordering/absence verdict")
        .expect("receiver closed")
}

fn render(app: &App) -> String {
    app.with_chat_widget_frame(160, |height, widget| {
        let height = height.max(1);
        let area = Rect::new(0, 0, 160, height);
        let mut buffer = Buffer::empty(area);
        widget.render(area, &mut buffer);
        let mut output = String::new();
        for y in 0..height {
            for x in 0..160 {
                output.push_str(buffer[(x, y)].symbol());
            }
            output.push('\n');
        }
        output
    })
}

fn start(
    app: &mut App,
    thread: ThreadId,
    receiver: &mut mpsc::Receiver<ThreadBufferedEvent>,
) -> color_eyre::Result<()> {
    ready(app.enqueue_thread_notification(
        thread,
        ServerNotification::TurnStarted(TurnStartedNotification {
            thread_id: thread.to_string(),
            turn: turn(TurnStatus::InProgress),
        }),
    ))?;
    app.handle_thread_event_now(receiver.try_recv().unwrap());
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn inactive_actual_ingress_documents_the_unpolled_sender_overtake() -> color_eyre::Result<()>
{
    assert!(!interface::available());
    let (mut app, _events, _ops) = app_fixture().await;
    let thread = app.chat_widget.thread_id().unwrap_or_else(ThreadId::new);
    let (sender, mut receiver) = channel(&mut app, thread);
    start(&mut app, thread, &mut receiver)?;
    sender.try_send(filler()).unwrap();
    ready(app.enqueue_thread_notification(thread, retry(thread)))?;
    receiver.try_recv().unwrap(); // Free RX without polling the spawned old send future.
    ready(app.enqueue_thread_notification(thread, terminal(thread)))?;
    let later = receiver
        .try_recv()
        .expect("legacy later terminal overtakes the unpolled Error");
    assert_eq!(tag(&later), "terminal");
    app.handle_thread_event_now(later);
    assert!(!app.chat_widget.is_task_running_for_test());
    let earlier = receive(&mut receiver).await;
    assert_eq!(tag(&earlier), "retry");
    app.handle_thread_event_now(earlier);
    assert!(
        render(&app).contains("Model is at capacity"),
        "late old Error can restore stale display"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn actual_active_ingress_cannot_publish_terminal_ahead_of_an_unpolled_retry()
-> color_eyre::Result<()> {
    assert!(!interface::available());
    for enabled in [true, false] {
        let _scope = scope(enabled);
        let (mut app, _events, _ops) = app_fixture().await;
        let thread = app.chat_widget.thread_id().unwrap_or_else(ThreadId::new);
        let (sender, mut receiver) = channel(&mut app, thread);
        start(&mut app, thread, &mut receiver)?;
        sender.try_send(filler()).unwrap();
        ready(app.enqueue_thread_notification(thread, retry(thread)))?;
        receiver.try_recv().unwrap();
        ready(app.enqueue_thread_notification(thread, terminal(thread)))?;
        assert!(
            matches!(receiver.try_recv(), Err(mpsc::error::TryRecvError::Empty)),
            "later terminal bypassed the older unpolled overflow event"
        );
        let earlier = receive(&mut receiver).await;
        assert_eq!(tag(&earlier), "retry");
        app.handle_thread_event_now(earlier);
        assert!(render(&app).contains("Model is at capacity"));
        let later = receive(&mut receiver).await;
        assert_eq!(tag(&later), "terminal");
        app.handle_thread_event_now(later);
        assert!(!app.chat_widget.is_task_running_for_test());
        assert!(!render(&app).contains("Model is at capacity"));
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn all_four_ingresses_and_background_delivery_share_one_fifo() -> color_eyre::Result<()> {
    let _scope = scope(false);
    let (mut app, _events, _ops) = app_fixture().await;
    let thread = ThreadId::new();
    let (sender, mut receiver) = channel(&mut app, thread);
    let warning = app.alab_thread_event_sender(thread, sender.clone());
    sender.try_send(filler()).unwrap();
    ready(app.enqueue_thread_notification(thread, retry(thread)))?;
    ready(app.enqueue_thread_request(
        thread,
        ServerRequest::ToolRequestUserInput {
            request_id: RequestId::Integer(17),
            params: ToolRequestUserInputParams {
                thread_id: thread.to_string(),
                turn_id: TURN.into(),
                item_id: "input".into(),
                questions: Vec::new(),
                is_blocking: true,
                auto_resolution_ms: None,
            },
        },
    ))?;
    ready(app.enqueue_thread_history_entry_response(
        thread,
        HistoryLookupResponse::Entry {
            offset: 1,
            log_id: 1,
            entry: Some("history".into()),
        },
    ))?;
    ready(app.enqueue_thread_feedback_event(
        thread,
        FeedbackThreadEvent {
            category: FeedbackCategory::Other,
            include_logs: false,
            feedback_audience: crate::bottom_pane::FeedbackAudience::External,
            result: Ok(crate::app_event::FeedbackReceipt::Uploaded {
                thread_id: "feedback".into(),
            }),
        },
    ));
    ready(warning.send(ThreadBufferedEvent::Notification(Box::new(
        ServerNotification::Warning(WarningNotification {
            thread_id: Some(thread.to_string()),
            message: "interrupt failure".into(),
        }),
    ))))
    .unwrap();
    receiver.try_recv().unwrap();
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    let mut observed = Vec::new();
    for _ in 0..5 {
        observed.push(tag(&receive(&mut receiver).await));
    }
    assert_eq!(
        observed,
        ["retry", "request", "history", "feedback", "warning"]
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn a_replacement_sender_never_borrows_the_new_channels_fifo() {
    let _scope = scope(true);
    let (mut app, _events, _ops) = app_fixture().await;
    let thread = ThreadId::new();
    let (old_sender, mut old_receiver) = channel(&mut app, thread);
    let old_delivery = app.alab_thread_event_sender(thread, old_sender.clone());
    let (new_sender, mut new_receiver) = channel(&mut app, thread);
    // Missing/mismatched raw sender keeps its original path; it cannot write the new queue.
    app.alab_try_send_thread_event(thread, &old_sender, filler())
        .unwrap();
    assert_eq!(tag(&old_receiver.try_recv().unwrap()), "history");
    assert!(matches!(
        new_receiver.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    assert!(
        old_delivery.send(filler()).await.is_err(),
        "captured retired queue must not fall back"
    );
    app.alab_try_send_thread_event(thread, &new_sender, filler())
        .unwrap();
    assert_eq!(tag(&new_receiver.try_recv().unwrap()), "history");
    app.thread_event_channels.remove(&thread);
    app.alab_try_send_thread_event(thread, &new_sender, filler())
        .unwrap();
    assert_eq!(
        tag(&new_receiver.try_recv().unwrap()),
        "history",
        "missing state retains the original raw sender path"
    );

    // A stale active receiver cannot take a replacement channel's overflow. The new
    // channel still owns its parked receiver, so the switch helper only drains the old one.
    let replacement = ThreadEventChannel::new(1);
    replacement.sender.try_send(filler()).unwrap();
    replacement
        .alab_fifo
        .try_send(&replacement.sender, filler())
        .unwrap();
    let abort = lock(&replacement.alab_fifo.inner).abort.clone().unwrap();
    app.thread_event_channels.insert(thread, replacement);
    old_sender.try_send(filler()).unwrap();
    app.active_thread_rx = Some(old_receiver);
    ready(app.alab_finish_thread_events_before_switch());
    let replacement = app.thread_event_channels.get_mut(&thread).unwrap();
    assert_eq!(lock(&replacement.alab_fifo.inner).pending.len(), 1);
    assert_eq!(
        tag(&replacement.receiver.as_mut().unwrap().try_recv().unwrap()),
        "history",
        "the replacement receiver's published prefix also stays its own"
    );
    app.thread_event_channels.remove(&thread);
    worker_finished(&abort).await;
}

#[tokio::test(flavor = "current_thread")]
async fn receiver_take_and_return_preserves_the_channel_owner() {
    assert!(!interface::available());
    for choice in [None, Some(false), Some(true)] {
        let _scope = choice.map(scope);
        assert_eq!(interface::available(), choice.is_some());
        let (mut app, _events, _ops) = app_fixture().await;
        let thread = ThreadId::new();
        let (sender, receiver) = channel(&mut app, thread);
        app.thread_event_channels.get_mut(&thread).unwrap().receiver = Some(receiver);
        let receiver = app
            .thread_event_channels
            .get_mut(&thread)
            .unwrap()
            .receiver
            .take()
            .unwrap();
        sender.try_send(filler()).unwrap();
        let queue = &app.thread_event_channels[&thread].alab_fifo;
        queue.try_send(&sender, filler()).unwrap();
        let abort = lock(&queue.inner).abort.clone().unwrap();
        app.active_thread_rx = Some(receiver);
        ready(app.alab_finish_thread_events_before_switch());
        assert!(matches!(
            app.active_thread_rx.as_mut().unwrap().try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        assert_eq!(
            lock(&app.thread_event_channels[&thread].alab_fifo.inner)
                .pending
                .len(),
            usize::from(choice.is_none()),
            "the inactive binary retains its raw-channel drain without taking ALab's queue"
        );
        app.thread_event_channels.remove(&thread);
        worker_finished(&abort).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn switching_threads_finishes_the_old_turn_before_its_held_face() -> color_eyre::Result<()> {
    // Both a completed turn and a still-running snapshot cross the real switch route.
    for completed in [true, false] {
        let _interface = scope(true);
        let shared: &'static boxes::Shared = Box::leak(Box::new(boxes::Shared::new()));
        let _boxes = boxes::scoped(shared);
        let tape = turns::scoped();
        let (mut app, _events, _ops) = app_fixture().await;
        let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let root = ThreadId::new();
        let a = ThreadId::new();
        let b = ThreadId::new();
        app.primary_thread_id = Some(root);
        for thread in [root, a, b] {
            let channel = ThreadEventChannel::new(2);
            channel.store.lock().await.set_session(
                crate::app::tests::test_thread_session(thread, app.config.cwd.to_path_buf()),
                Vec::new(),
            );
            app.thread_event_channels.insert(thread, channel);
        }
        for thread in [a, b] {
            // Loaded siblings select their existing channels without an RPC or a resume.
            app.side_threads
                .insert(thread, crate::app::SideThreadState::new(root));
        }
        let session = ready(app.thread_event_channels[&a].store.lock())
            .session
            .clone()
            .unwrap();
        app.chat_widget.handle_thread_session_quiet(session);
        ready(app.activate_thread_channel(a));
        assert!(
            app.chat_widget
                .alab_report_program_status(app.chat_widget.desired_program_status())
        );
        tape.take();

        let sender = app.thread_event_channels[&a].sender.clone();
        sender.try_send(filler()).unwrap();
        sender.try_send(filler()).unwrap();
        assert!(shared.turn_started(&a.to_string(), TURN));
        ready(app.enqueue_thread_notification(
            a,
            ServerNotification::TurnStarted(TurnStartedNotification {
                thread_id: a.to_string(),
                turn: turn(TurnStatus::InProgress),
            }),
        ))?;
        if completed {
            shared.turn_ended(&a.to_string(), TURN);
            ready(app.enqueue_thread_notification(a, terminal(a)))?;
        }
        let abort = lock(&app.thread_event_channels[&a].alab_fifo.inner)
            .abort
            .clone()
            .unwrap();
        let mut tui = crate::tui::test_support::make_test_tui()?;
        // Ready-only polling keeps the spawned overflow worker unpolled until the actual
        // switch has transferred A. Emptying only the raw receiver misses its Start/End.
        ready(Box::pin(app.select_agent_thread(&mut tui, &mut server, b)))?;
        assert_eq!(app.active_thread_id, Some(b));
        assert_eq!(shared.tui_handled_end(&a.to_string(), TURN), completed);
        assert_eq!(
            turns::aterm_turns(&format!(
                "{}{}",
                turns::sequence(ProgramState::Idle).unwrap(),
                tape.take()
            )),
            1,
            "the original start reached A before the widget left"
        );
        worker_finished(&abort).await;
        assert!(
            app.chat_widget
                .alab_report_program_status(app.chat_widget.desired_program_status())
        );
        let rest_or_running = tape.take();
        assert_eq!(
            rest_or_running,
            if completed {
                turns::sequence(ProgramState::Idle).unwrap()
            } else {
                String::new()
            }
        );

        // This is the held-face cut for an ended turn: B rests, and no old A start may
        // follow it. A running turn instead keeps Working across both real switches.
        ready(Box::pin(app.select_agent_thread(&mut tui, &mut server, a)))?;
        ready(app.drain_active_thread_events(&mut tui))?;
        assert_eq!(app.chat_widget.is_task_running_for_test(), !completed);
        assert!(
            app.chat_widget
                .alab_report_program_status(app.chat_widget.desired_program_status())
        );
        assert_eq!(
            tape.take(),
            "",
            "snapshot replay and frames begin no old turn"
        );

        if !completed {
            shared.turn_ended(&a.to_string(), TURN);
            ready(app.enqueue_thread_notification(a, terminal(a)))?;
            ready(app.drain_active_thread_events(&mut tui))?;
            assert!(
                app.chat_widget
                    .alab_report_program_status(app.chat_widget.desired_program_status())
            );
            tape.take();
        }
        // A genuinely new native turn is still live after the snapshot cut.
        assert!(shared.turn_started(&a.to_string(), "fresh-turn"));
        let mut fresh = turn(TurnStatus::InProgress);
        fresh.id = "fresh-turn".into();
        ready(app.enqueue_thread_notification(
            a,
            ServerNotification::TurnStarted(TurnStartedNotification {
                thread_id: a.to_string(),
                turn: fresh,
            }),
        ))?;
        ready(app.drain_active_thread_events(&mut tui))?;
        assert!(app.chat_widget.is_task_running_for_test());
        assert!(
            app.chat_widget
                .alab_report_program_status(app.chat_widget.desired_program_status())
        );
        assert_eq!(
            turns::aterm_turns(&format!(
                "{}{}",
                turns::sequence(ProgramState::Idle).unwrap(),
                tape.take()
            )),
            1,
            "a fresh turn still writes its one begin"
        );
        tokio::time::timeout(Duration::from_secs(60), server.shutdown()).await??;
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn taking_a_reserved_fifo_keeps_history_and_cannot_retire_its_successor() {
    let entry = |offset| HistoryLookupResponse::Entry {
        offset,
        log_id: 1,
        entry: Some(format!("entry-{offset}")),
    };
    // Active history batches are deliberately not copied into ThreadEventStore.
    let batch = HistoryLookupResponse::Batch {
        cursor: codex_message_history::HistoryBatchCursor::new(1),
        log_id: 1,
        entries: vec![HistoryBatchEntryResponse {
            offset: 0,
            entry: Some("the unstored batch".into()),
        }],
        next_older_cursor: None,
    };
    let queue = Queue::default();
    let (sender, mut receiver) = mpsc::channel(2);
    sender.try_send(entry(0)).unwrap();
    sender.try_send(entry(1)).unwrap();
    {
        let mut state = lock(&queue.inner);
        state.generation = 1;
        state.running = true;
        state.pending.extend([batch.clone(), entry(3)]);
    }
    let guard = WorkerGuard {
        inner: Arc::clone(&queue.inner),
        generation: 1,
    };
    let mut old_worker = Box::pin(tokio::task::unconstrained(drain(sender.clone(), guard)));
    std::future::poll_fn(|cx| {
        assert!(matches!(old_worker.as_mut().poll(cx), Poll::Pending));
        Poll::Ready(())
    })
    .await;
    assert_eq!(
        queue.take_buffered(&mut receiver),
        VecDeque::from([entry(0), entry(1), batch, entry(3)]),
        "raw prefix and pending tail transfer once, in order, including unstored history"
    );
    // The old reserve now owns one freed slot. Fill the other so a new ingress starts a
    // real successor worker before the old production future resumes and drops its guard.
    sender.try_send(entry(4)).unwrap();
    queue.try_send(&sender, entry(5)).unwrap();
    let (generation, abort) = {
        let state = lock(&queue.inner);
        assert!(state.running);
        (state.generation, state.abort.clone().unwrap())
    };
    ready(old_worker.as_mut());
    {
        let state = lock(&queue.inner);
        assert_eq!(state.generation, generation);
        assert!(state.running, "the old guard cannot clear the successor");
    }
    assert_eq!(receiver.try_recv().unwrap(), entry(4));
    assert_eq!(receive(&mut receiver).await, entry(5));
    worker_finished(&abort).await;
    queue.try_send(&sender, entry(6)).unwrap();
    assert_eq!(receiver.try_recv().unwrap(), entry(6));
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_pending_reserve_keeps_the_head_for_the_next_ingress() {
    let queue = Queue::<u8>::default();
    let (sender, mut receiver) = mpsc::channel(1);
    sender.try_send(0).unwrap();
    {
        let mut state = lock(&queue.inner);
        state.generation = 1;
        state.running = true;
        state.pending.push_back(1);
    }
    let guard = WorkerGuard {
        inner: Arc::clone(&queue.inner),
        generation: 1,
    };
    let mut worker = Box::pin(tokio::task::unconstrained(drain(sender.clone(), guard)));
    std::future::poll_fn(|cx| {
        assert!(
            matches!(worker.as_mut().poll(cx), Poll::Pending),
            "actual reserve must block"
        );
        Poll::Ready(())
    })
    .await;
    drop(worker); // Actual production future/guard drop, not a timer assumption.
    {
        let state = lock(&queue.inner);
        assert!(!state.running);
        assert_eq!(state.pending.iter().copied().collect::<Vec<_>>(), vec![1]);
    }
    queue.try_send(&sender, 2).unwrap();
    assert_eq!(receiver.try_recv().unwrap(), 0);
    assert_eq!(receive(&mut receiver).await, 1);
    assert_eq!(receive(&mut receiver).await, 2);
    queue.try_send(&sender, 3).unwrap();
    assert_eq!(
        receiver.try_recv().unwrap(),
        3,
        "drained worker atomically restored the idle fast path"
    );
}

async fn worker_finished(abort: &AbortHandle) {
    tokio::time::timeout(Duration::from_secs(60), async {
        while !abort.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("actual task-completion witness, not a sleep-based absence assertion");
}

#[tokio::test(flavor = "current_thread")]
async fn receiver_close_and_owner_drop_retire_pending_workers_and_handles() {
    for drop_owner in [false, true] {
        let queue = Queue::<u8>::default();
        let (sender, receiver) = mpsc::channel(1);
        sender.try_send(0).unwrap();
        queue.try_send(&sender, 1).unwrap();
        let handle = queue.handle(sender.clone());
        let abort = lock(&queue.inner).abort.clone().unwrap();
        if drop_owner {
            drop(queue);
            assert!(matches!(handle.try_send(2), Err(TrySendError::Closed(2))));
            worker_finished(&abort).await;
            assert!(handle.inner.upgrade().is_none());
            drop(receiver);
        } else {
            drop(receiver);
            assert!(matches!(handle.try_send(2), Err(TrySendError::Closed(2))));
            worker_finished(&abort).await;
            let state = lock(&queue.inner);
            assert!(state.retired);
            assert!(state.pending.is_empty());
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn independent_channels_and_the_idle_fast_path_do_not_wait_for_a_full_peer() {
    let first = Queue::<u8>::default();
    let second = Queue::<u8>::default();
    let (a, mut a_rx) = mpsc::channel(1);
    let (b, mut b_rx) = mpsc::channel(1);
    a.try_send(0).unwrap();
    first.try_send(&a, 1).unwrap();
    second.try_send(&b, 7).unwrap();
    assert_eq!(
        b_rx.try_recv().unwrap(),
        7,
        "independent idle queue stays synchronous"
    );
    assert_eq!(a_rx.try_recv().unwrap(), 0);
    assert_eq!(receive(&mut a_rx).await, 1);
    first.try_send(&a, 2).unwrap();
    assert_eq!(receive(&mut a_rx).await, 2);
}

#[test]
fn an_old_generation_guard_cannot_clear_a_new_worker() {
    struct NotDefault;
    let queue = Queue::<NotDefault>::default();
    {
        let mut state = lock(&queue.inner);
        state.generation = 2;
        state.running = true;
    }
    drop(WorkerGuard {
        inner: Arc::clone(&queue.inner),
        generation: 1,
    });
    assert!(lock(&queue.inner).running);
}
