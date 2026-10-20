// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The per-thread timers, under tokio's paused clock: one pending timer per thread, fired
//! at its deadline, replaced by a newer one, and cancelled (and joined) when a turn starts
//! and when the thread stops.

use std::sync::Arc;
use std::time::Duration;

use codex_alab_aterm::Options as ClientOptions;
use codex_alab_core::session::Shared;
use codex_extension_api::ExtensionData;
use codex_extension_api::NoopExtensionEventSink;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStopInput;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStartInput;
use codex_protocol::ThreadId;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;

use crate::Autopilot;
use crate::Supervisor;
use crate::SupervisorOptions;
use crate::decide::Timing;
use crate::extension::ThreadRt;
use crate::supervisor::clock;

const MIN: Duration = Duration::from_secs(60);

fn autopilot() -> Arc<Autopilot> {
    codex_alab_core::activate();
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(|| None),
        client: ClientOptions::default(),
    });
    Autopilot::new(
        sup,
        Arc::new(NoopExtensionEventSink),
        None,
        None,
        Timing::default(),
        |_| true,
    )
}

fn thread() -> (ExtensionData, Arc<ThreadRt>) {
    let id = ThreadId::new();
    let store = ExtensionData::new(id.to_string());
    store.insert(ThreadRt::new(id, None, false, Duration::ZERO));
    let rt = store.get::<ThreadRt>().expect("inserted");
    (store, rt)
}

/// Let every task that can run, run.
async fn settle() {
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
}

async fn turn_start(autopilot: &Autopilot, thread_store: &ExtensionData) {
    let mode = CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "gpt-5".to_string(),
            reasoning_effort: None,
            developer_instructions: None,
        },
    };
    let session = ExtensionData::new("session");
    let turn = ExtensionData::new("turn-1");
    autopilot
        .on_turn_start(TurnStartInput {
            turn_id: "turn-1",
            collaboration_mode: &mode,
            token_usage_at_turn_start: None,
            session_store: &session,
            thread_store,
            turn_store: &turn,
        })
        .await;
}

#[tokio::test(start_paused = true)]
async fn a_timer_fires_at_its_deadline_and_leaves_the_slot() {
    let autopilot = autopilot();
    let (_store, rt) = thread();
    autopilot.schedule(&rt, clock() + 5 * MIN);
    assert!(rt.timer_pending());
    tokio::time::advance(4 * MIN).await;
    settle().await;
    assert_eq!(rt.evaluations(), 0);
    tokio::time::advance(MIN + Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(rt.evaluations(), 1);
    assert!(!rt.timer_pending());
}

#[tokio::test(start_paused = true)]
async fn a_newer_timer_replaces_the_older() {
    let autopilot = autopilot();
    let (_store, rt) = thread();
    autopilot.schedule(&rt, clock() + 5 * MIN);
    autopilot.schedule(&rt, clock() + 10 * MIN);
    tokio::time::advance(6 * MIN).await;
    settle().await;
    assert_eq!(rt.evaluations(), 0, "the replaced timer never fires");
    tokio::time::advance(5 * MIN).await;
    settle().await;
    assert_eq!(rt.evaluations(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_turn_start_cancels_and_joins_the_pending_timer() {
    let autopilot = autopilot();
    let (store, rt) = thread();
    autopilot.schedule(&rt, clock() + 5 * MIN);
    turn_start(&autopilot, &store).await;
    assert!(!rt.timer_pending());
    tokio::time::advance(10 * MIN).await;
    settle().await;
    assert_eq!(rt.evaluations(), 0);
    // Negative control: without the turn start the same timer fires.
    autopilot.schedule(&rt, clock() + 5 * MIN);
    tokio::time::advance(6 * MIN).await;
    settle().await;
    assert_eq!(rt.evaluations(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_thread_stop_cancels_and_joins_the_pending_timer() {
    let autopilot = autopilot();
    let (store, rt) = thread();
    autopilot.schedule(&rt, clock() + 5 * MIN);
    let session = ExtensionData::new("session");
    autopilot
        .on_thread_stop(ThreadStopInput {
            session_store: &session,
            thread_store: &store,
        })
        .await;
    assert!(!rt.timer_pending());
    tokio::time::advance(10 * MIN).await;
    settle().await;
    assert_eq!(rt.evaluations(), 0);
}

#[test]
fn aterm_harness_lease_is_looked_at_again_soon() {
    use crate::supervisor::HAND_RECHECK;
    use crate::supervisor::RECHECK;
    use crate::supervisor::recheck_after;
    use codex_alab_aterm::status::NotReady;
    // aterm's relaunch types under its harness's lease: the carry-on behind it looks soon.
    assert_eq!(
        recheck_after(&NotReady::Hand("lease:aterm-harness@4242".to_string())),
        HAND_RECHECK
    );
    // Negative control: another driver's hand is a longer wait.
    assert_eq!(
        recheck_after(&NotReady::Hand("lease:someone@1".to_string())),
        RECHECK
    );
}
