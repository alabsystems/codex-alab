// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The question overlay and the plan box from the supervisor's side: a box the TUI's glue
//! reports in a private shared state is answered (posted, the TUI woken) only under a
//! proven claim of alab-aterm's in-process FAKE aterm, after the grace; the act is recorded
//! once the glue says the box took it; a box that is a person's raises keyed attention and
//! clears it when it closes; an old aterm is never claimed and nothing is posted. The glue's
//! own half (the overlay's submit, the plan box's accept) is tested inside codex-tui.

use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::time::Duration;

use codex_alab_aterm::Options as ClientOptions;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use codex_alab_autopilot::Supervisor;
use codex_alab_autopilot::SupervisorOptions;
use codex_alab_core::boxes::BoxAnswer;
use codex_alab_core::boxes::BoxKind;
use codex_alab_core::boxes::BoxOption;
use codex_alab_core::boxes::BoxOutcome;
use codex_alab_core::boxes::BoxQuestion;
use codex_alab_core::boxes::Choice;
use codex_alab_core::boxes::OpenBox;
use codex_alab_core::session::Frame;
use codex_alab_core::session::ProgramState;
use codex_alab_core::session::Shared;
use codex_alab_core::session::now_ms;
use pretty_assertions::assert_eq;

struct Harness {
    fake: FakeAterm,
    shared: &'static Shared,
    sup: Arc<Supervisor>,
    woken: Arc<AtomicU32>,
    _dir: tempfile::TempDir,
}

async fn harness(mode: Mode, policy: &[(&str, &str)]) -> Harness {
    codex_alab_core::activate();
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let fake = FakeAterm::start(dir.path(), mode)
        .await
        .unwrap_or_else(|err| panic!("fake aterm: {err}"));
    fake.set_policy_key("human_grace_s", "1");
    for (key, value) in policy {
        fake.set_policy_key(key, value);
    }
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let woken = Arc::new(AtomicU32::new(0));
    let count = Arc::clone(&woken);
    shared.set_waker(Box::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    }));
    let endpoint = fake.endpoint();
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(move || Some(endpoint.clone())),
        client: ClientOptions::default(),
    });
    sup.start();
    shared.observe(
        &Frame {
            primary_thread: Some("019a0000-0000-7000-8000-0000000000aa"),
            displayed_thread: Some("019a0000-0000-7000-8000-0000000000aa"),
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
    // The autopilot keeps this thread (its `on_thread_start` says so in the codex binary).
    shared.note_root_thread("019a0000-0000-7000-8000-0000000000aa");
    Harness {
        fake,
        shared,
        sup,
        woken,
        _dir: dir,
    }
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

fn question_box() -> OpenBox {
    OpenBox {
        id: "call-1".to_string(),
        kind: BoxKind::Question {
            thread: "019a0000-0000-7000-8000-0000000000aa".to_string(),
            questions: vec![BoxQuestion {
                id: "cache".to_string(),
                header: "Cache".to_string(),
                question: "Where should the cache live?".to_string(),
                options: vec![
                    BoxOption {
                        label: "On disk".to_string(),
                        description: String::new(),
                    },
                    BoxOption {
                        label: "In memory (Recommended)".to_string(),
                        description: String::new(),
                    },
                ],
                is_other: true,
                is_secret: false,
            }],
            blocking: true,
            begun: None,
        },
        opened_ms: now_ms(),
    }
}

fn plan_box() -> OpenBox {
    OpenBox {
        id: "plan-1".to_string(),
        kind: BoxKind::Plan {
            rows: vec![
                ("Yes, implement this plan".to_string(), None),
                ("Yes, clear context and implement".to_string(), None),
                ("No, stay in Plan mode".to_string(), None),
            ],
        },
        opened_ms: now_ms(),
    }
}

/// The glue's half, played: take the answer and say the box took it and closed.
fn take(h: &Harness, id: &str) -> BoxAnswer {
    let answer = h
        .shared
        .take_box_answer(id)
        .unwrap_or_else(|| panic!("an answer for {id}"));
    h.shared.close_box(id);
    h.shared.set_box_outcome(BoxOutcome {
        id: id.to_string(),
        applied: Ok(()),
    });
    answer
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_question_box_is_answered_after_the_grace_and_recorded_once_the_tui_took_it() {
    let h = harness(Mode::Current, &[]).await;
    assert!(
        h.fake
            .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
            .await
    );
    h.shared.report_box(question_box());
    let opened = std::time::Instant::now();
    wait_until("the answer is posted", || h.shared.box_answer_pending()).await;
    assert!(
        opened.elapsed() >= Duration::from_millis(900),
        "not before the grace"
    );
    assert!(h.woken.load(Ordering::SeqCst) >= 1, "the TUI was woken");
    assert_eq!(
        take(&h, "call-1"),
        BoxAnswer::Question {
            id: "call-1".to_string(),
            answers: vec![("cache".to_string(), Choice::Option(1))],
        }
    );
    assert!(
        h.fake
            .wait_for(|f| f.acts().iter().any(|a| a == "answer-recommended@v1"))
            .await,
        "the act is recorded: {:?}",
        h.sup.journal()
    );
    assert!(
        h.fake
            .wait_for(|f| f
                .stories()
                .contains(&("chose".to_string(), "recommended".to_string())))
            .await,
        "the band is told the choice: {:?}",
        h.fake.stories()
    );
    assert!(
        h.fake.attention().is_empty(),
        "no attention for a box it answered"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_plan_box_is_answered_under_approve_all() {
    let h = harness(Mode::Current, &[]).await;
    assert!(
        h.fake
            .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
            .await
    );
    h.shared.report_box(plan_box());
    wait_until("the answer is posted", || h.shared.box_answer_pending()).await;
    assert_eq!(
        take(&h, "plan-1"),
        BoxAnswer::Plan {
            id: "plan-1".to_string(),
            row: 0,
        }
    );
    assert!(
        h.fake
            .wait_for(|f| f.acts().iter().any(|a| a == "plan-approve@v2"))
            .await
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_box_that_is_a_persons_raises_keyed_attention_until_it_closes() {
    let h = harness(Mode::Current, &[("approve", "none")]).await;
    assert!(
        h.fake
            .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
            .await
    );
    h.shared.report_box(plan_box());
    assert!(
        h.fake.wait_for(|f| !f.attention().is_empty()).await,
        "attention is raised: {:?}",
        h.sup.journal()
    );
    let attention = h.fake.attention();
    assert!(
        attention
            .get("codex-alab")
            .is_some_and(|text| text.starts_with("plan: left to a person (approve is not")),
        "{attention:?}"
    );
    assert!(!h.shared.box_answer_pending(), "nothing was posted");
    // The box closes (two frames: the one that drew it, then one that did not).
    let _ = h.shared.end_frame_boxes();
    assert!(h.shared.end_frame_boxes());
    assert!(
        h.fake.wait_for(|f| f.attention().is_empty()).await,
        "the attention clears with the box: {:?}",
        h.fake.attention()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_old_aterm_never_gets_a_box_answered() {
    let h = harness(Mode::Old, &[]).await;
    h.shared.report_box(question_box());
    assert!(
        h.fake
            .wait_for(|f| f.requests().iter().any(|r| r.contains("harness policy")))
            .await
    );
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(!h.shared.box_answer_pending());
    assert_eq!(h.woken.load(Ordering::SeqCst), 0);
    assert!(h.fake.acts().is_empty());
    assert!(h.fake.claim().is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_box_of_a_thread_the_autopilot_does_not_keep_is_left_to_a_person() {
    let h = harness(Mode::Current, &[]).await;
    assert!(
        h.fake
            .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
            .await
    );
    // A sub-agent's question the TUI shows (its thread is no root thread of this TUI).
    let mut open = question_box();
    if let BoxKind::Question { thread, .. } = &mut open.kind {
        *thread = "019a0000-0000-7000-8000-0000000000bb".to_string();
    }
    h.shared.report_box(open);
    assert!(
        h.fake.wait_for(|f| !f.attention().is_empty()).await,
        "attention is raised: {:?}",
        h.sup.journal()
    );
    let attention = h.fake.attention();
    assert!(
        attention
            .get("codex-alab")
            .is_some_and(|text| text.contains("not a root thread")),
        "{attention:?}"
    );
    assert!(!h.shared.box_answer_pending(), "nothing was posted");
}

fn approval_box() -> OpenBox {
    OpenBox {
        id: "approval-call-7".to_string(),
        kind: BoxKind::Approval {
            thread: "019a0000-0000-7000-8000-0000000000aa".to_string(),
            key: "call-7".to_string(),
            what: "exec_command".to_string(),
        },
        opened_ms: now_ms(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_approval_box_the_contributor_cleared_is_approved_once_after_the_grace() {
    // End-to-end run 2026-10-16: a box asked inside the grace was left to a person for good;
    // the contributor now clears it and the box glue reports it, answered after the grace.
    let h = harness(Mode::Current, &[]).await;
    assert!(
        h.fake
            .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
            .await
    );
    h.shared.report_box(approval_box());
    wait_until("the answer is posted", || h.shared.box_answer_pending()).await;
    assert_eq!(
        take(&h, "approval-call-7"),
        BoxAnswer::Approval {
            id: "approval-call-7".to_string(),
        }
    );
    assert!(
        h.fake
            .wait_for(|f| f.acts().iter().any(|a| a == "approve-all@v1"))
            .await,
        "the act is recorded: {:?}",
        h.sup.journal()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_approval_box_is_left_to_a_person_once_approve_is_not_all() {
    // Negative control: the policy moved off `approve=all` after the review.
    let h = harness(Mode::Current, &[("approve", "none")]).await;
    assert!(
        h.fake
            .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
            .await
    );
    h.shared.report_box(approval_box());
    assert!(
        h.fake
            .wait_for(|f| f
                .attention()
                .values()
                .any(|v| v.contains("approve is not all")))
            .await,
        "{:?} {:?}",
        h.fake.attention(),
        h.sup.journal()
    );
    assert!(!h.shared.box_answer_pending());
    assert!(!h.fake.acts().iter().any(|a| a == "approve-all@v1"));
}

/// A frame of the primary thread in `state` (the TUI's observe hook).
fn frame(h: &Harness, state: ProgramState) {
    h.shared.observe(
        &Frame {
            primary_thread: Some("019a0000-0000-7000-8000-0000000000aa"),
            displayed_thread: Some("019a0000-0000-7000-8000-0000000000aa"),
            rollout_path: None,
            cwd: std::path::Path::new("/work"),
            model: "gpt-5",
            codex_home: h._dir.path(),
            embedded: true,
            state,
            draft_empty: true,
            queued_input: false,
            popup_open: false,
            editor_open: false,
            pending_user_turn: false,
        },
        now_ms(),
    );
}

/// The attention audit: an approval the contributor left for an owner's rule shows in the TUI
/// as a box no view reports to codex-alab; past the grace its attention names the rule, and
/// it clears when the box goes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_approval_left_for_an_owners_rule_raises_attention_naming_it() {
    use codex_alab_autopilot::approval::Defer;
    use codex_alab_core::session::BlockedKind;
    let h = harness(Mode::Current, &[]).await;
    wait_until("the claim is held", || h.sup.held()).await;
    h.sup.deferred(Defer::RulePrompt.words());
    frame(&h, ProgramState::Blocked(BlockedKind::Permission));
    assert!(
        h.fake
            .wait_for(|f| f
                .attention()
                .get("codex-alab")
                .is_some_and(|t| t.contains("an execpolicy rule asks about this command")))
            .await,
        "{:?} {:?}",
        h.fake.attention(),
        h.sup.journal()
    );
    assert!(!h.shared.box_answer_pending(), "nothing was answered");
    frame(&h, ProgramState::Working);
    // The next look clears it (a box event or a frame wakes the supervisor).
    frame(&h, ProgramState::Idle);
    assert!(
        h.fake.wait_for(|f| f.attention().is_empty()).await,
        "{:?}",
        h.fake.attention()
    );
}

/// The attention audit: a question naming an irreversible act with no recommended option and
/// no one option that refuses it is a person's, and says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_destructive_question_without_a_refusal_raises_attention() {
    let h = harness(Mode::Current, &[]).await;
    wait_until("the claim is held", || h.sup.held()).await;
    let mut open = question_box();
    if let BoxKind::Question { questions, .. } = &mut open.kind {
        questions[0].question = "Should I delete the production database?".to_string();
        questions[0].options = vec![
            BoxOption {
                label: "Delete it now".to_string(),
                description: String::new(),
            },
            BoxOption {
                label: "Delete it after a backup".to_string(),
                description: String::new(),
            },
        ];
    }
    h.shared.report_box(open);
    assert!(
        h.fake
            .wait_for(|f| f
                .attention()
                .get("codex-alab")
                .is_some_and(|t| t.starts_with("question: left to a person (")))
            .await,
        "{:?} {:?}",
        h.fake.attention(),
        h.sup.journal()
    );
    assert!(!h.shared.box_answer_pending(), "nothing was posted");
}
