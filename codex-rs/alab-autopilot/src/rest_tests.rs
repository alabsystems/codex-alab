// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The record's rest around a turn (CONTRACT §4): a turn's start is counted as running and
//! written through to the record BEFORE the turn runs (the start hook returns only once the
//! record's write-through answered), and its stop, its abort or the thread's stop ends it.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use codex_alab_aterm::Options as ClientOptions;
use codex_alab_core::session::Shared;
use codex_alab_core::session::Unrest;
use codex_extension_api::ExtensionData;
use codex_extension_api::NoopExtensionEventSink;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStopInput;
use codex_extension_api::TurnAbortInput;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStartInput;
use codex_extension_api::TurnStopInput;
use codex_protocol::ThreadId;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::TurnItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::protocol::TurnAbortReason;

use crate::Autopilot;
use crate::Supervisor;
use crate::SupervisorOptions;
use crate::decide::Timing;
use crate::extension::ThreadRt;

/// What each flush of the record would have said (the unrest it saw).
type Flushes = Arc<Mutex<Vec<Option<Unrest>>>>;

/// An autopilot over a fresh shared state whose write-through notes what the record would
/// have said at each flush (the unrest it saw).
fn autopilot() -> (Arc<Autopilot>, &'static Shared, Flushes) {
    codex_alab_core::activate();
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let flushes = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&flushes);
    assert!(shared.set_flush(Box::new(move || {
        seen.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(shared.unrest());
        true
    })));
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(|| None),
        client: ClientOptions::default(),
    });
    let autopilot = Autopilot::new(
        sup,
        Arc::new(NoopExtensionEventSink),
        None,
        None,
        Timing::default(),
        |_| true,
    );
    (autopilot, shared, flushes)
}

fn thread() -> (ExtensionData, Arc<ThreadRt>) {
    let id = ThreadId::new();
    let store = ExtensionData::new(id.to_string());
    store.insert(ThreadRt::new(id, None, false, Duration::ZERO));
    let rt = store.get::<ThreadRt>().expect("inserted");
    (store, rt)
}

async fn start(autopilot: &Autopilot, thread_store: &ExtensionData, turn: &ExtensionData) {
    let mode = CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "gpt-5".to_string(),
            reasoning_effort: None,
            developer_instructions: None,
        },
    };
    let session = ExtensionData::new("session");
    autopilot
        .on_turn_start(TurnStartInput {
            turn_id: turn.level_id(),
            collaboration_mode: &mode,
            token_usage_at_turn_start: None,
            session_store: &session,
            thread_store,
            turn_store: turn,
        })
        .await;
}

fn flushed(flushes: &Mutex<Vec<Option<Unrest>>>) -> Vec<Option<Unrest>> {
    flushes
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turns_start_is_written_busy_before_it_runs_and_its_stop_ends_it() {
    let (autopilot, shared, flushes) = autopilot();
    let (store, _rt) = thread();
    let session = ExtensionData::new("session");
    let turn = ExtensionData::new("turn-1");
    start(&autopilot, &store, &turn).await;
    // The start hook returned: the write-through already ran, and saw the turn running.
    assert_eq!(flushed(&flushes), vec![Some(Unrest::TurnRunning)]);
    assert!(shared.turn_running());
    autopilot
        .on_turn_stop(TurnStopInput {
            session_store: &session,
            thread_store: &store,
            turn_store: &turn,
        })
        .await;
    assert!(!shared.turn_running(), "the stop ends it");
    assert_eq!(shared.unrest(), None);

    // An abort ends a turn as a stop does.
    let aborted = ExtensionData::new("turn-2");
    start(&autopilot, &store, &aborted).await;
    assert!(shared.turn_running());
    autopilot
        .on_turn_abort(TurnAbortInput {
            reason: TurnAbortReason::Interrupted,
            session_store: &session,
            thread_store: &store,
            turn_store: &aborted,
        })
        .await;
    assert!(!shared.turn_running(), "the abort ends it");

    // The thread's stop ends a turn whose own end never came.
    let stranded = ExtensionData::new("turn-3");
    start(&autopilot, &store, &stranded).await;
    assert!(shared.turn_running());
    autopilot
        .on_thread_stop(ThreadStopInput {
            session_store: &session,
            thread_store: &store,
        })
        .await;
    assert!(!shared.turn_running(), "the thread's stop ends it");
    assert_eq!(flushed(&flushes).len(), 3, "one write-through per start");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_thread_that_is_not_the_autopilots_leaves_the_record_alone() {
    // Negative control: a thread with no ThreadRt (a sub-agent, exec) counts no turn and
    // writes nothing.
    let (autopilot, shared, flushes) = autopilot();
    let store = ExtensionData::new(ThreadId::new().to_string());
    let turn = ExtensionData::new("turn-1");
    start(&autopilot, &store, &turn).await;
    assert!(!shared.turn_running());
    assert!(flushed(&flushes).is_empty());
}

/// The worker's final reply `text`, as the turn's item stream completes it.
async fn reply(
    autopilot: &Autopilot,
    thread_store: &ExtensionData,
    turn: &ExtensionData,
    text: &str,
) {
    let item = TurnItem::AgentMessage(AgentMessageItem {
        id: "msg-1".to_string(),
        content: vec![AgentMessageContent::Text {
            text: text.to_string(),
        }],
        phase: Some(MessagePhase::FinalAnswer),
        memory_citation: None,
        delivery: None,
        questions: None,
    });
    autopilot.on_item_completed(thread_store, turn, &item).await;
}

/// The TUI's own root thread: registered by a frame, and kept by the autopilot.
fn root(shared: &Shared, rt: &ThreadRt) {
    shared.observe(
        &codex_alab_core::session::Frame {
            primary_thread: Some(rt.key()),
            displayed_thread: Some(rt.key()),
            rollout_path: None,
            cwd: std::path::Path::new("/work"),
            model: "gpt-5",
            codex_home: std::path::Path::new("/h/.codex"),
            embedded: true,
            state: codex_alab_core::session::ProgramState::Idle,
            draft_empty: true,
            queued_input: false,
            popup_open: false,
            editor_open: false,
            pending_user_turn: false,
        },
        1,
    );
    shared.note_root_thread(rt.key());
    assert!(shared.is_root_thread(rt.key()));
}

/// F1b (review of 6efbf7518), finding 5 (review of 148a9fedb): the carry-on chain ends only
/// where the worker's turn ended done (or a person's turn began). A turn cut short (the stall's
/// restart aborts it) leaves the count standing on the record, and so does a turn that merely
/// ended cleanly: relaunch, carry-on, a clean end, the autopilot's continue, a stall again
/// must not carry on a second time.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_a_done_turn_end_ends_the_carry_on_chain() {
    let (autopilot, shared, _flushes) = autopilot();
    let (store, rt) = thread();
    root(shared, &rt);
    let session = ExtensionData::new("session");
    // A relaunched process: its task from the resumed history, its carry-on counted.
    rt.with_state(|s| s.observe_resume(true, Some("Working through stage 2."), false));
    shared.set_carried_on(1);
    // The continuation's turn is cut short (the teardown a SIGTERM starts aborts it).
    let cut = ExtensionData::new("turn-1");
    start(&autopilot, &store, &cut).await;
    autopilot
        .on_turn_abort(TurnAbortInput {
            reason: TurnAbortReason::Interrupted,
            session_store: &session,
            thread_store: &store,
            turn_store: &cut,
        })
        .await;
    assert_eq!(shared.carried_on(), 1, "an aborted turn ends no chain");
    // A turn that ends cleanly but not done ends none either (the negative control).
    let clean = ExtensionData::new("turn-2");
    start(&autopilot, &store, &clean).await;
    reply(
        &autopilot,
        &store,
        &clean,
        "Stage 2 is half done; continuing next.",
    )
    .await;
    autopilot
        .on_turn_stop(TurnStopInput {
            session_store: &session,
            thread_store: &store,
            turn_store: &clean,
        })
        .await;
    assert_eq!(
        shared.carried_on(),
        1,
        "a clean end that is not done ends no chain"
    );
    // Finding 1 (review of cf084e3b7): nor does a first done phrase, which the done check
    // answers ("...otherwise continue"): the worker may carry on from it and stall again.
    let phrase = ExtensionData::new("turn-2b");
    start(&autopilot, &store, &phrase).await;
    reply(
        &autopilot,
        &store,
        &phrase,
        "Build completed successfully; next I'll run the tests.",
    )
    .await;
    autopilot
        .on_turn_stop(TurnStopInput {
            session_store: &session,
            thread_store: &store,
            turn_store: &phrase,
        })
        .await;
    assert_eq!(
        shared.carried_on(),
        1,
        "a first done phrase gets the done check, and ends no chain"
    );
    // A turn that ends done does.
    let done = ExtensionData::new("turn-3");
    start(&autopilot, &store, &done).await;
    reply(&autopilot, &store, &done, "All landed.\nATERM-TASK-DONE").await;
    autopilot
        .on_turn_stop(TurnStopInput {
            session_store: &session,
            thread_store: &store,
            turn_store: &done,
        })
        .await;
    assert_eq!(shared.carried_on(), 0, "a done turn end ends the chain");
}

/// Finding 1 (review of cf084e3b7), the positive control: a done phrase AFTER the done check
/// was typed is the worker's done (`decide` finishes there), and ends the chain.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_done_reply_after_the_done_check_ends_the_carry_on_chain() {
    let (autopilot, shared, _flushes) = autopilot();
    let (store, rt) = thread();
    root(shared, &rt);
    let session = ExtensionData::new("session");
    let phrase = "Build completed successfully; nothing left to do.";
    for checked in [false, true] {
        rt.with_state(|s| {
            s.observe_resume(true, Some("Working through stage 2."), false);
            s.done_checked = checked;
        });
        shared.set_carried_on(1);
        let turn = ExtensionData::new(format!("turn-{checked}"));
        start(&autopilot, &store, &turn).await;
        reply(&autopilot, &store, &turn, phrase).await;
        autopilot
            .on_turn_stop(TurnStopInput {
                session_store: &session,
                thread_store: &store,
                turn_store: &turn,
            })
            .await;
        assert_eq!(
            shared.carried_on(),
            if checked { 0 } else { 1 },
            "done check typed: {checked}"
        );
    }
}

/// Negative control: a side thread's turn (a title, a memory) is not the session's and ends
/// no chain, even one that ends done.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_side_threads_turn_ends_no_carry_on_chain() {
    let (autopilot, shared, _flushes) = autopilot();
    let (store, _rt) = thread();
    let session = ExtensionData::new("session");
    shared.set_carried_on(1);
    let turn = ExtensionData::new("turn-1");
    start(&autopilot, &store, &turn).await;
    reply(&autopilot, &store, &turn, "ATERM-TASK-DONE").await;
    autopilot
        .on_turn_stop(TurnStopInput {
            session_store: &session,
            thread_store: &store,
            turn_store: &turn,
        })
        .await;
    assert_eq!(shared.carried_on(), 1);
}
