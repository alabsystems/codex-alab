// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The question overlay, the plan box and the goal box from the supervisor's side: a box the
//! TUI's glue reports in a private shared state is answered (posted, the TUI woken) only under
//! a proven claim of alab-aterm's in-process FAKE aterm, after the grace; the act is recorded
//! once the glue says the box took it; a box that is a person's raises keyed attention and
//! clears it when it closes; an old aterm is never claimed and nothing is posted. The glue's
//! own half (the overlay's submit, the plan box's and the goal box's accept) is tested inside
//! codex-tui.

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
use codex_alab_core::boxes::GoalStop;
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

fn goal_box(stop: GoalStop) -> OpenBox {
    OpenBox {
        id: "goal-1".to_string(),
        kind: BoxKind::Goal {
            rows: vec!["Resume goal".to_string(), "Leave paused".to_string()],
            stop: Some(stop),
        },
        opened_ms: now_ms(),
    }
}

/// The TUI's frame while a box is its active view: a popup is open, which holds every act
/// but the box's own answer back.
fn popup_open(h: &Harness) {
    h.shared.observe(
        &Frame {
            primary_thread: Some("019a0000-0000-7000-8000-0000000000aa"),
            displayed_thread: Some("019a0000-0000-7000-8000-0000000000aa"),
            rollout_path: None,
            cwd: std::path::Path::new("/work"),
            model: "gpt-5",
            codex_home: h._dir.path(),
            embedded: true,
            state: ProgramState::Idle,
            draft_empty: true,
            queued_input: false,
            popup_open: true,
            editor_open: false,
            pending_user_turn: false,
        },
        now_ms(),
    );
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
    // Before the box's own `opened_ms`, so the grace's lower bound holds.
    let opened = std::time::Instant::now();
    h.shared.report_box(question_box());
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

/// UNDER LAND THE OVERLAY GETS LAND'S TEXT (CONTRACT §5 "question overlay", aterm
/// `answer-land@v1`): with the session's Autopilot `land` and aterm's reply carrying
/// `land_question_text`, the same box is answered with that text in its notes, recorded as
/// `answer-land@v1` (an act aterm takes from codex-alab, `acts::is_question_rule`), and the band
/// is told `chose land`. NEGATIVE CONTROL: under Cruise the box takes its recommended option
/// (the test above). MUTANT: `Priority::of` without its Land arm.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn under_land_the_question_box_gets_lands_text_and_rule() {
    const LAND_TEXT: &str = "Decide yourself: land it (aterm's land_question_text).";
    let h = harness(Mode::Current, &[("land_question_text", LAND_TEXT)]).await;
    h.fake.set_status("autopilot", "land");
    assert!(
        h.fake
            .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
            .await
    );
    // A new generation, so the policy and the Autopilot word are read again.
    h.fake.set_policy_key("answer_text", "Pick one.");
    wait_until("the supervisor reads Land and its question text", || {
        h.sup.policy().is_some_and(|p| {
            p.autopilot == codex_alab_autopilot::decide::Mode::Land
                && p.texts.land_question == LAND_TEXT
                && p.answer_text.as_deref() == Some("Pick one.")
        })
    })
    .await;
    h.shared.report_box(question_box());
    wait_until("the answer is posted", || h.shared.box_answer_pending()).await;
    assert_eq!(
        take(&h, "call-1"),
        BoxAnswer::Question {
            id: "call-1".to_string(),
            answers: vec![("cache".to_string(), Choice::Note(LAND_TEXT.to_string()))],
        }
    );
    assert!(
        h.fake
            .wait_for(|f| f.acts().iter().any(|a| a == "answer-land@v1"))
            .await,
        "the act is recorded: {:?}",
        h.sup.journal()
    );
    assert!(
        !h.fake.acts().iter().any(|a| a == "answer-recommended@v1"),
        "{:?}",
        h.fake.acts()
    );
    assert!(
        h.fake
            .wait_for(|f| f
                .stories()
                .contains(&("chose".to_string(), "land".to_string())))
            .await,
        "the band is told the choice: {:?}",
        h.fake.stories()
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

/// "Resume paused goal?" after a resume held every act back ("a popup or modal is open"): over
/// a paused goal, under any policy word, its `Leave paused` is posted after the grace while
/// the popup is open, and it is no act of the task: noted and told in aterm's message log,
/// never `meta set harness_act` (which would move aterm's task face to open) and no story.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_goal_box_over_a_paused_goal_is_left_as_it_was_and_records_no_act() {
    let h = harness(Mode::Current, &[("approve", "none"), ("questions", "ask")]).await;
    assert!(
        h.fake
            .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
            .await
    );
    popup_open(&h);
    // Before the box's own `opened_ms`, so the grace's lower bound holds.
    let opened = std::time::Instant::now();
    h.shared.report_box(goal_box(GoalStop::Paused));
    wait_until("the answer is posted", || h.shared.box_answer_pending()).await;
    assert!(
        opened.elapsed() >= Duration::from_millis(900),
        "not before the grace"
    );
    assert_eq!(
        take(&h, "goal-1"),
        BoxAnswer::Goal {
            id: "goal-1".to_string(),
            row: 1,
        }
    );
    assert!(
        h.fake
            .wait_for(|f| f.notices().iter().any(|n| n.contains("Leave paused")))
            .await,
        "told in aterm's message log: {:?} {:?}",
        h.fake.notices(),
        h.sup.journal()
    );
    // Noted after the notice is sent (`told`).
    wait_until("noted", || {
        h.sup
            .journal()
            .iter()
            .any(|line| line.contains("act goal-leave@v1"))
    })
    .await;
    assert!(
        !h.fake.acts().iter().any(|a| a == "goal-leave@v1"),
        "no act of the task: {:?}",
        h.fake.acts()
    );
    assert!(h.fake.stories().is_empty(), "{:?}", h.fake.stories());
    assert!(h.fake.attention().is_empty(), "{:?}", h.fake.attention());
}

/// Over a usage-limited goal the goal box's `Leave paused` is posted after the grace, as over a
/// paused one: left open, the popup would hold every root thread's acts back, and the goal goes
/// on only through a usage resume, which the box neither owes nor blocks once it is gone. The
/// box raises nothing itself: where the point owes no usage resume, the turn end's own look
/// says a person decides, and says it again after every passive spell (`tests/autopilot.rs`,
/// `a_stopped_goals_attention_is_raised_again_when_acting_resumes`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_goal_box_over_a_usage_limited_goal_is_left_as_it_was_and_raises_nothing() {
    let h = harness(Mode::Current, &[]).await;
    assert!(
        h.fake
            .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
            .await
    );
    popup_open(&h);
    h.shared.report_box(goal_box(GoalStop::UsageLimited));
    wait_until("the answer is posted", || h.shared.box_answer_pending()).await;
    assert_eq!(
        take(&h, "goal-1"),
        BoxAnswer::Goal {
            id: "goal-1".to_string(),
            row: 1,
        }
    );
    wait_until("noted", || {
        h.sup
            .journal()
            .iter()
            .any(|line| line.contains("act goal-leave@v1"))
    })
    .await;
    assert!(h.fake.attention().is_empty(), "{:?}", h.fake.attention());
    assert!(
        !h.fake.acts().iter().any(|a| a == "goal-leave@v1"),
        "no act of the task: {:?}",
        h.fake.acts()
    );
}

/// NEGATIVE CONTROL: over a blocked goal the goal box is a person's: nothing is posted, and
/// keyed attention says why.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_goal_box_over_a_blocked_goal_is_left_to_a_person() {
    let h = harness(Mode::Current, &[]).await;
    assert!(
        h.fake
            .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
            .await
    );
    popup_open(&h);
    h.shared.report_box(goal_box(GoalStop::Blocked));
    assert!(
        h.fake
            .wait_for(|f| f
                .attention()
                .values()
                .any(|v| v.starts_with("goal: left to a person (the goal is blocked")))
            .await,
        "{:?} {:?}",
        h.fake.attention(),
        h.sup.journal()
    );
    assert!(!h.shared.box_answer_pending());
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
