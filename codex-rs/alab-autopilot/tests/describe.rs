// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The session's aterm description from the supervisor's side, against alab-aterm's
//! in-process FAKE aterm: written only under the proven claim, once per change, never over a
//! person's own description, and never the title.

use std::sync::Arc;
use std::time::Duration;

use codex_alab_aterm::Options as ClientOptions;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use codex_alab_autopilot::Supervisor;
use codex_alab_autopilot::SupervisorOptions;
use codex_alab_autopilot::describe::describe;
use codex_alab_core::session::Frame;
use codex_alab_core::session::Predecessor;
use codex_alab_core::session::ProgramState;
use codex_alab_core::session::Shared;
use codex_alab_core::session::now_ms;
use pretty_assertions::assert_eq;

const THREAD: &str = "019a0000-0000-7000-8000-0000000000aa";

async fn held() -> (FakeAterm, Arc<Supervisor>, tempfile::TempDir) {
    held_after(false).await
}

/// A held claim; `crashed`: this thread's dead predecessor left a record (a crash or an
/// aterm restore relaunched it).
async fn held_after(crashed: bool) -> (FakeAterm, Arc<Supervisor>, tempfile::TempDir) {
    codex_alab_core::activate();
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .unwrap_or_else(|err| panic!("fake aterm: {err}"));
    fake.set_policy_key("human_grace_s", "1");
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    if crashed {
        shared.note_predecessor(Predecessor {
            pid: 1,
            session_id: THREAD.to_string(),
            ..Predecessor::default()
        });
    }
    let endpoint = fake.endpoint();
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(move || Some(endpoint.clone())),
        client: ClientOptions::default(),
    });
    sup.start();
    shared.observe(
        &Frame {
            primary_thread: Some(THREAD),
            displayed_thread: Some(THREAD),
            rollout_path: None,
            cwd: std::path::Path::new("/work"),
            model: "gpt-5",
            codex_home: dir.path(),
            embedded: true,
            state: ProgramState::Idle,
            draft_empty: true,
            queued_input: false,
            popup_open: false,
            editor_open: false,
            pending_user_turn: false,
        },
        now_ms(),
    );
    wait_until("the claim is held", || sup.held()).await;
    (fake, sup, dir)
}

/// Wait until `done` holds, at most a minute: a hang detector, never a latency budget.
async fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while !done() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "never happened: {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_goal_and_step_are_written_once_per_change_and_never_as_the_title() {
    let (fake, sup, _dir) = held().await;
    sup.describe(describe(Some("ship the inbox"), None)).await;
    assert_eq!(fake.description().as_deref(), Some("goal: ship the inbox"));
    // The same text again writes nothing.
    sup.describe(describe(Some("ship the inbox"), None)).await;
    assert_eq!(fake.descriptions().len(), 1, "{:?}", fake.descriptions());
    // A plan step moves it once.
    sup.describe(describe(Some("ship the inbox"), Some("write tests")))
        .await;
    assert_eq!(
        fake.descriptions(),
        vec![
            "goal: ship the inbox".to_string(),
            "goal: ship the inbox · now: write tests".to_string(),
        ]
    );
    assert!(
        !fake.requests().iter().any(|r| r.contains("meta set title")),
        "{:?}",
        fake.requests()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_persons_description_stands() {
    let (fake, sup, _dir) = held().await;
    fake.describe_as_person("the build box, do not touch");
    sup.describe(describe(None, Some("write tests"))).await;
    assert_eq!(
        fake.description().as_deref(),
        Some("the build box, do not touch")
    );
    // One that only looks like codex-alab's is a person's too: no predecessor wrote it.
    fake.describe_as_person("plan: my own notes");
    sup.describe(describe(None, Some("write tests"))).await;
    sup.describe(None).await;
    assert_eq!(fake.description().as_deref(), Some("plan: my own notes"));
    assert!(fake.descriptions().is_empty(), "{:?}", fake.descriptions());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_crashed_predecessors_description_is_replaced() {
    // Positive control: a dead predecessor's record says its text may still stand.
    let (fake, sup, _dir) = held_after(true).await;
    fake.describe_as_person("plan: an older step");
    sup.describe(describe(None, Some("write tests"))).await;
    assert_eq!(fake.description().as_deref(), Some("plan: write tests"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_crashed_predecessors_description_goes_when_there_is_nothing_to_say() {
    let (fake, sup, _dir) = held_after(true).await;
    fake.describe_as_person("goal: a goal since completed");
    sup.describe(None).await;
    assert_eq!(fake.description(), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_description_is_withdrawn_when_nothing_is_left_to_say() {
    let (fake, sup, _dir) = held().await;
    sup.describe(describe(Some("ship the inbox"), None)).await;
    assert_eq!(fake.description().as_deref(), Some("goal: ship the inbox"));
    // The goal completed and no plan step is in progress: withdrawn, not left standing.
    sup.describe(describe(None, None)).await;
    assert_eq!(fake.description(), None);
    assert!(
        fake.requests()
            .iter()
            .any(|r| r.ends_with("meta unset description")),
        "{:?}",
        fake.requests()
    );
    // A person's text set after codex-alab's is never withdrawn.
    sup.describe(describe(None, Some("write tests"))).await;
    fake.describe_as_person("mine now");
    sup.describe(None).await;
    assert_eq!(fake.description().as_deref(), Some("mine now"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_description_goes_with_the_claim() {
    let (fake, sup, _dir) = held().await;
    sup.describe(describe(Some("ship the inbox"), None)).await;
    assert_eq!(fake.description().as_deref(), Some("goal: ship the inbox"));
    // A person clears the claim: nothing keeps the text true, so it is withdrawn.
    fake.clear_claim();
    wait_until("passive", || !sup.held()).await;
    assert!(
        fake.wait_for(|f| f.description().is_none()).await,
        "{:?}",
        sup.journal()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unanswered_description_read_never_costs_the_claim() {
    // An unanswered exchange closes the connection it ran on: never the claim's.
    let (fake, sup, _dir) = held().await;
    // Settle the claim's initial metadata cleanup before stalling metadata reads;
    // otherwise the stall can also close the claim's own connection.
    sup.show_needs().await;
    let claim_conn = fake.claim().and_then(|c| c.conn);
    fake.stall("meta");
    sup.describe(describe(Some("ship the inbox"), None)).await;
    fake.unstall("meta");
    assert!(sup.held(), "{:?}", sup.journal());
    assert_eq!(fake.claim().and_then(|c| c.conn), claim_conn);
    assert!(
        sup.journal().iter().any(|l| l.contains("not written")),
        "{:?}",
        sup.journal()
    );
    // And the next change is written.
    sup.describe(describe(Some("ship it"), None)).await;
    assert_eq!(fake.description().as_deref(), Some("goal: ship it"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nothing_is_described_without_the_claim() {
    let (fake, sup, _dir) = held().await;
    fake.clear_claim();
    wait_until("passive", || !sup.held()).await;
    sup.describe(describe(Some("ship the inbox"), None)).await;
    assert!(fake.descriptions().is_empty(), "{:?}", fake.descriptions());
    assert_eq!(fake.description(), None);
}
