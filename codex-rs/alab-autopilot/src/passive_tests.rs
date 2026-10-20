// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Passive spells and what they leave (the review of 4cdc96090), against alab-aterm's
//! in-process FAKE aterm (never a real one):
//!
//! - an off read while the before-every-act checks are in flight stops the act (finding 1);
//! - a point left to a person whose attention a standby or a release cleared is raised again
//!   when codex-alab acts again, and not while a turn of its thread runs (finding 3);
//! - a box answer the TUI already took keeps its record across a standby, and its outcome is
//!   recorded once codex-alab acts again (finding 7);
//! - codex-alab's own yes to the plan box clears the `plan:` attention (inbox finding 8);
//! - codex-alab's own release across a re-confirm is not taken for someone else's clear
//!   (review of fea034d20, finding 3);
//! - a standby while an attention raise or a description write is in flight still ends with
//!   neither standing;
//! - a person's clear an act's checks meet first stays given up, checks cancelled mid-release
//!   included, and a clear a record renewal takes back is a clear whatever the echo count says
//!   (review of 7e7622ff7, findings 1-3);
//! - a description a standby or a release withdrew is written back when acting resumes
//!   (finding 8).

use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use codex_alab_aterm::Options as ClientOptions;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use codex_alab_core::boxes::BoxKind;
use codex_alab_core::boxes::BoxOutcome;
use codex_alab_core::boxes::OpenBox;
use codex_alab_core::session::Frame;
use codex_alab_core::session::ProgramState;
use codex_alab_core::session::Shared;
use codex_alab_core::session::now_ms;
use codex_extension_api::ExtensionData;
use codex_extension_api::NoopExtensionEventSink;
use codex_protocol::ThreadId;

use crate::Autopilot;
use crate::Supervisor;
use crate::SupervisorOptions;
use crate::decide::Timing;
use crate::decide::TurnEnd;
use crate::decide::Wall;
use crate::extension::ThreadRt;
use crate::supervisor::Ready;

struct Harness {
    fake: FakeAterm,
    shared: &'static Shared,
    sup: Arc<Supervisor>,
    autopilot: Arc<Autopilot>,
    rt: Arc<ThreadRt>,
    _store: ExtensionData,
    _dir: tempfile::TempDir,
}

async fn harness(policy: &[(&str, &str)]) -> Harness {
    codex_alab_core::activate();
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .unwrap_or_else(|err| panic!("fake aterm: {err}"));
    fake.set_policy_key("human_grace_s", "0");
    for (key, value) in policy {
        fake.set_policy_key(key, value);
    }
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let endpoint = fake.endpoint();
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(move || Some(endpoint.clone())),
        client: ClientOptions::default(),
    });
    let autopilot = Autopilot::new(
        Arc::clone(&sup),
        Arc::new(NoopExtensionEventSink),
        None,
        None,
        Timing::default(),
        |_| true,
    );
    let id = ThreadId::new();
    let key = id.to_string();
    let store = ExtensionData::new(key.clone());
    store.insert(ThreadRt::new(id, None, false, Duration::ZERO));
    let rt = store
        .get::<ThreadRt>()
        .unwrap_or_else(|| panic!("inserted"));
    autopilot.keep_thread(&rt);
    autopilot.watch_claims();
    sup.start();
    shared.observe(
        &Frame {
            primary_thread: Some(&key),
            displayed_thread: Some(&key),
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
    shared.note_root_thread(&key);
    let h = Harness {
        fake,
        shared,
        sup,
        autopilot,
        rt,
        _store: store,
        _dir: dir,
    };
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    h
}

/// Wait until `done` holds, at most a minute: a hang detector, never a latency budget.
async fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while !done() {
        assert!(tokio::time::Instant::now() < deadline, "never: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Wait (a hang detector) until the before-every-act checks say go.
async fn wait_go(sup: &Supervisor) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let ready = sup.ready().await;
        if ready == Ready::Go {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "never ready: {ready:?} {:?}",
            sup.journal()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Act again after a passive spell: the policy moves (announced) and reads `effective=on`.
async fn act_again(h: &Harness) {
    h.fake.set_effective(true, "enabled");
    wait_until("acting again", || h.sup.acting()).await;
    wait_go(&h.sup).await;
}

fn shown(h: &Harness) -> Option<String> {
    h.fake.attention().get("codex-alab").cloned()
}

/// Review of 4cdc96090, finding 1: the checks read `status` and re-confirm the claim, and a
/// standby that comes while either is in flight sends nothing and moves no epoch; the checks
/// look for it last, under the lock, and say passive. Negative control: the same gated
/// exchange with codex-alab still acting says go.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_off_while_the_checks_are_in_flight_stops_the_act() {
    let h = harness(&[]).await;
    let client = h
        .sup
        .client()
        .unwrap_or_else(|| panic!("the claim's connection"));
    for (verb, off) in [
        ("status", false),
        ("status", true),
        ("meta", false),
        ("meta", true),
    ] {
        wait_go(&h.sup).await;
        let met = h.fake.gated_requests().len();
        h.fake.gate(verb);
        let sup = Arc::clone(&h.sup);
        let ready = tokio::spawn(async move { sup.ready().await });
        wait_until("the checks meet the gate", || {
            h.fake.gated_requests().len() > met
        })
        .await;
        if off {
            h.sup
                .stand_by(&client, "effective=off why=suspended".to_string())
                .await;
        }
        h.fake.open_gate(verb);
        let ready = tokio::time::timeout(Duration::from_secs(60), ready)
            .await
            .unwrap_or_else(|_| panic!("the checks hung"))
            .unwrap_or_else(|err| panic!("the checks: {err}"));
        if off {
            assert!(
                matches!(&ready, Ready::Passive(why) if why.contains("suspended")),
                "{verb}: {ready:?} {:?}",
                h.sup.journal()
            );
            act_again(&h).await;
        } else {
            assert_eq!(ready, Ready::Go, "{verb}: {:?}", h.sup.journal());
        }
    }
}

fn point(h: &Harness, end: TurnEnd) {
    let timing = Timing::default();
    h.rt.with_state(|s| {
        s.observe_person_turn();
        s.observe_turn_end(end, &timing);
    });
}

fn ended(text: Option<&str>, wall: Option<Wall>) -> TurnEnd {
    TurnEnd {
        at: Instant::now(),
        by_autopilot: false,
        by_person: true,
        text: text.map(str::to_string),
        questions: false,
        worked: Duration::from_secs(600),
        wall,
        aborted: false,
    }
}

/// Review of 4cdc96090, finding 3: a point codex-alab leaves to a person raises attention; a
/// standby or a release clears it; when codex-alab acts again and the point still stands (no
/// turn since), it is raised again. Negative controls: while a turn of the thread runs the
/// older point raises nothing, and a point a turn answered is not raised again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_point_left_to_a_person_is_raised_again_when_acting_resumes() {
    let h = harness(&[("questions", "ask"), ("resume_limits", "false")]).await;
    let cases = [
        ("question", ended(Some("Should I rename the module?"), None)),
        (
            "limit",
            ended(
                None,
                Some(Wall::Usage {
                    reset_at: None,
                    reset_label: None,
                }),
            ),
        ),
        ("login", ended(None, Some(Wall::Login))),
    ];
    for (key, end) in cases {
        for off in ["suspended", "autopilot-off"] {
            point(&h, end.clone());
            Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
            let prefix = format!("{key}: ");
            assert!(
                h.fake
                    .wait_for(|f| f
                        .attention()
                        .get("codex-alab")
                        .is_some_and(|t| t.starts_with(&prefix)))
                    .await,
                "{key}: raised: {:?} {:?}",
                h.fake.attention(),
                h.sup.journal()
            );
            h.fake.set_effective(false, off);
            assert!(
                h.fake
                    .wait_for(|f| !f.attention().contains_key("codex-alab"))
                    .await,
                "{key} {off}: cleared at the off"
            );
            if off == "autopilot-off" {
                wait_until("released", || h.fake.claim().is_none()).await;
            }
            act_again(&h).await;
            assert!(
                h.fake
                    .wait_for(|f| f
                        .attention()
                        .get("codex-alab")
                        .is_some_and(|t| t.starts_with(&prefix)))
                    .await,
                "{key} {off}: raised again: {:?} {:?}",
                h.fake.attention(),
                h.sup.journal()
            );
            assert!(h.rt.state().point.is_some(), "{key}: the point stands");
            // A person's answer: their turn ends with real work, and its end answers the point.
            let timing = Timing::default();
            h.rt.with_state(|s| {
                s.observe_turn_end(ended(Some("Renamed it."), None), &timing);
                s.point = None;
            });
            h.sup.attention(key, None).await;
        }
    }
    // Negative control: a turn of the thread runs; the standing point says nothing then.
    point(&h, ended(Some("Should I rename the module?"), None));
    assert!(h.shared.turn_started(h.rt.key(), "turn-9"));
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(shown(&h), None, "{:?}", h.sup.journal());
    // Its end (a person's, answering it) leaves nothing to raise again either.
    h.shared.turn_ended(h.rt.key(), "turn-9");
    h.rt.with_state(|s| s.point = None);
    h.fake.set_effective(false, "suspended");
    wait_until("passive", || !h.sup.acting()).await;
    act_again(&h).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(shown(&h), None, "{:?}", h.sup.journal());
}

fn question_box(h: &Harness) -> OpenBox {
    OpenBox {
        id: "call-1".to_string(),
        kind: BoxKind::Question {
            thread: h.rt.key().to_string(),
            questions: vec![codex_alab_core::boxes::BoxQuestion {
                id: "cache".to_string(),
                header: "Cache".to_string(),
                question: "Where should the cache live?".to_string(),
                options: vec![
                    codex_alab_core::boxes::BoxOption {
                        label: "On disk".to_string(),
                        description: String::new(),
                    },
                    codex_alab_core::boxes::BoxOption {
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

/// Review of 4cdc96090, finding 7: the glue takes an answer and closes its box, then reports
/// the outcome; a standby between the two keeps the posted answer, and its outcome is
/// recorded once codex-alab acts again. Negative control: an answer the glue never took is
/// withdrawn at the standby, and nothing is recorded for it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_answer_the_tui_took_is_recorded_across_a_standby() {
    // A grace of its own: with none, a posted answer is "not taken" at once.
    let h = harness(&[("human_grace_s", "1")]).await;
    h.shared.report_box(question_box(&h));
    wait_until("the answer is posted", || h.shared.box_answer_pending()).await;
    let _ = h
        .shared
        .take_box_answer("call-1")
        .unwrap_or_else(|| panic!("an answer"));
    h.shared.close_box("call-1");
    h.fake.set_effective(false, "suspended");
    wait_until("passive", || !h.sup.acting()).await;
    h.shared.set_box_outcome(BoxOutcome {
        id: "call-1".to_string(),
        applied: Ok(()),
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !h.fake.acts().iter().any(|a| a == "answer-recommended@v1"),
        "nothing is recorded while passive"
    );
    act_again(&h).await;
    assert!(
        h.fake
            .wait_for(|f| f.acts().iter().any(|a| a == "answer-recommended@v1"))
            .await,
        "recorded once acting again: {:?}",
        h.sup.journal()
    );

    // Negative control: posted, never taken, then a standby: withdrawn, never recorded.
    let acts = h.fake.acts().len();
    let mut open = question_box(&h);
    open.id = "call-2".to_string();
    h.shared.report_box(open);
    wait_until("the answer is posted", || h.shared.box_answer_pending()).await;
    h.fake.set_effective(false, "suspended");
    wait_until("withdrawn", || !h.shared.box_answer_pending()).await;
    h.shared.close_box("call-2");
    act_again(&h).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(h.fake.acts().len(), acts, "{:?}", h.fake.acts());
}

/// Inbox finding 8: under `approve=all` codex-alab answers the plan box itself; once its yes
/// goes in, the `plan:` attention the Plan-mode turn end raised is answered and goes.
/// Negative control: codex-alab's answer to a question box leaves `plan:` standing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn codex_alabs_own_plan_yes_clears_the_plan_attention() {
    // A grace of its own: with none, a posted answer is "not taken" at once.
    let h = harness(&[("human_grace_s", "1")]).await;
    let plan = "plan: Plan mode leaves this turn's end to a person".to_string();
    let glue = |id: &str| {
        let _ = h
            .shared
            .take_box_answer(id)
            .unwrap_or_else(|| panic!("an answer for {id}"));
        h.shared.close_box(id);
        h.shared.set_box_outcome(BoxOutcome {
            id: id.to_string(),
            applied: Ok(()),
        });
    };
    h.sup.attention("plan", Some(plan.clone())).await;
    assert_eq!(shown(&h), Some(plan.clone()));
    h.shared.report_box(question_box(&h));
    wait_until("the answer is posted", || h.shared.box_answer_pending()).await;
    glue("call-1");
    assert!(
        h.fake
            .wait_for(|f| f.acts().iter().any(|a| a == "answer-recommended@v1"))
            .await
    );
    assert_eq!(shown(&h), Some(plan), "a question's answer leaves it");
    h.shared.report_box(plan_box());
    wait_until("the answer is posted", || h.shared.box_answer_pending()).await;
    glue("plan-1");
    assert!(
        h.fake
            .wait_for(|f| f.acts().iter().any(|a| a == "plan-approve@v2"))
            .await
    );
    assert!(
        h.fake
            .wait_for(|f| !f.attention().contains_key("codex-alab"))
            .await,
        "{:?}",
        h.fake.attention()
    );
}

/// Review of fea034d20, finding 3: codex-alab gives the claim back for an Autopilot Off while
/// an act's re-confirm is in flight, and the release's echo moves the epoch the checks watch.
/// The act does not go, and the claim is given back without counting it someone else's clear,
/// so the word's return (told by its `meta` event, no policy move) claims again. Negative
/// control: the same straddle by a person's clear (no release of codex-alab's own) stays
/// given up until the policy moves.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn its_own_release_across_a_reconfirm_is_not_taken_for_a_clear() {
    let h = harness(&[]).await;
    let client = h
        .sup
        .client()
        .unwrap_or_else(|| panic!("the claim's connection"));
    for own in [true, false] {
        wait_go(&h.sup).await;
        h.fake.set_effective_quietly(false, "autopilot-off");
        let met = h.fake.gated_requests().len();
        h.fake.gate("meta");
        let sup = Arc::clone(&h.sup);
        let ready = tokio::spawn(async move { sup.ready().await });
        wait_until("the re-confirm meets the gate", || {
            h.fake
                .gated_requests()
                .iter()
                .skip(met)
                .any(|r| r.contains("meta set supervisor"))
        })
        .await;
        let (epoch, released) = h.sup.epochs();
        if own {
            let sup = Arc::clone(&h.sup);
            let c = client.clone();
            tokio::spawn(async move {
                sup.release_for_off(&c, "effective=off why=autopilot-off".to_string())
                    .await;
            });
            wait_until("the release is sent", || h.sup.epochs().1 > released).await;
            // Its echo reaches the event reader while the re-confirm is still unanswered.
            h.fake.emit("meta field=supervisor value=-");
        } else {
            h.fake.clear_claim();
        }
        wait_until("the epoch moved", || h.sup.epochs().0 > epoch).await;
        h.fake.open_gate("meta");
        let ready = tokio::time::timeout(Duration::from_secs(60), ready)
            .await
            .unwrap_or_else(|_| panic!("the checks hung"))
            .unwrap_or_else(|err| panic!("the checks: {err}"));
        assert!(
            matches!(&ready, Ready::Passive(_)),
            "own={own}: {ready:?} {:?}",
            h.sup.journal()
        );
        wait_until("passive", || !h.sup.held()).await;
        // The word comes back on, told by its `meta` event alone.
        h.fake.set_effective_quietly(true, "enabled");
        h.fake.emit("meta field=autopilot value=on");
        if own {
            wait_until("claimed again at the word's event", || h.sup.acting()).await;
        } else {
            wait_until("stays given up", || {
                h.sup
                    .journal()
                    .iter()
                    .any(|l| l.contains("the claim was cleared: waiting for the policy to move"))
            })
            .await;
            assert!(!h.sup.held(), "{:?}", h.sup.journal());
        }
    }
}

/// A standby that comes while an attention raise is in flight still leaves no attention at
/// aterm: attention syncs run one at a time, so the standby's clear follows the raise and
/// clears it. Before, the clear compared against the shown text the raise had not recorded
/// yet, sent nothing, and the raise landed under the standby (seen under load in
/// `tests/autopilot.rs`, the plan and goal tests, 8 of 18 runs of six binaries at once).
/// Negative control: the same raise with no standby stands.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_standby_while_an_attention_raise_is_in_flight_clears_it() {
    let h = harness(&[]).await;
    let client = h
        .sup
        .client()
        .unwrap_or_else(|| panic!("the claim's connection"));
    let text = "turn: left to a person".to_string();
    for standby in [false, true] {
        wait_go(&h.sup).await;
        let met = h.fake.gated_requests().len();
        h.fake.gate("meta");
        let sup = Arc::clone(&h.sup);
        let raised = text.clone();
        let raise = tokio::spawn(async move { sup.attention("turn", Some(raised)).await });
        wait_until("the raise meets the gate", || {
            h.fake
                .gated_requests()
                .iter()
                .skip(met)
                .any(|r| r.contains("meta set attention"))
        })
        .await;
        let off = standby.then(|| {
            let sup = Arc::clone(&h.sup);
            let c = client.clone();
            tokio::spawn(async move {
                sup.stand_by(&c, "effective=off why=suspended".to_string())
                    .await;
            })
        });
        if standby {
            wait_until("on standby", || h.sup.standby().is_some()).await;
        }
        h.fake.open_gate("meta");
        for task in std::iter::once(raise).chain(off) {
            tokio::time::timeout(Duration::from_secs(60), task)
                .await
                .unwrap_or_else(|_| panic!("hung"))
                .unwrap_or_else(|err| panic!("task: {err}"));
        }
        if standby {
            // At once: the standby's clear came after the raise, and was answered before
            // `stand_by` returned (no later look, such as the passive clock's, is waited for).
            assert!(
                !h.fake.attention().contains_key("codex-alab"),
                "{:?} {:?}",
                h.fake.attention(),
                h.sup.journal()
            );
        } else {
            assert_eq!(shown(&h).as_deref(), Some(text.as_str()));
            h.sup.attention("turn", None).await;
            assert_eq!(shown(&h), None);
        }
    }
}

/// A standby that comes while a description is being written (its `meta` read answered late)
/// still leaves no description standing: the writer finds codex-alab passive once it wrote,
/// and withdraws its own text (the standby found nothing of it to withdraw). Negative
/// control: the same write with no standby stands.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_standby_while_a_description_is_written_withdraws_it() {
    let h = harness(&[]).await;
    let client = h
        .sup
        .client()
        .unwrap_or_else(|| panic!("the claim's connection"));
    for (standby, text) in [
        (false, "goal: ship the inbox"),
        (true, "goal: ship it again"),
    ] {
        wait_go(&h.sup).await;
        let met = h.fake.gated_requests().len();
        h.fake.gate("meta");
        let sup = Arc::clone(&h.sup);
        let write = tokio::spawn(async move { sup.describe(Some(text.to_string())).await });
        wait_until("the write's read meets the gate", || {
            h.fake.gated_requests().len() > met
        })
        .await;
        if standby {
            h.sup
                .stand_by(&client, "effective=off why=suspended".to_string())
                .await;
        }
        h.fake.open_gate("meta");
        tokio::time::timeout(Duration::from_secs(60), write)
            .await
            .unwrap_or_else(|_| panic!("the write hung"))
            .unwrap_or_else(|err| panic!("the write: {err}"));
        if standby {
            assert_eq!(
                h.fake.description(),
                None,
                "{:?} {:?}",
                h.fake.descriptions(),
                h.sup.journal()
            );
        } else {
            assert_eq!(h.fake.description().as_deref(), Some(text));
        }
    }
}

fn claims_sent(h: &Harness) -> usize {
    h.fake
        .requests()
        .iter()
        .filter(|r| r.contains("meta set supervisor codex-alab@"))
        .count()
}

fn noted(h: &Harness, needle: &str) -> usize {
    h.sup
        .journal()
        .iter()
        .filter(|l| l.contains(needle))
        .count()
}

const CLAIMED: &str = "claimed as codex-alab@";
const STAYS_CLEARED: &str = "the claim was cleared: waiting for the policy to move";

/// Let the claim's watch meet the person's clear late: a `meta` event that may move the policy
/// (its read comes first), then the clear's held-back `value=-` (`delay_clear_event`); then
/// wait until it was told one way or the other.
async fn the_watch_meets_it_late(h: &Harness, claimed_before: usize) {
    h.fake.emit("meta field=autopilot value=on");
    wait_until("the watch met the clear", || {
        noted(h, STAYS_CLEARED) > 0 || noted(h, CLAIMED) > claimed_before
    })
    .await;
    wait_until("the clear's own value=- is met", || {
        noted(h, "a clear met while unclaimed: nothing to take") > 0
            || noted(h, "the claim was cleared: not taken again") > 0
    })
    .await;
}

/// Review of 7e7622ff7, finding 1: a person clears the claim, and an act's checks meet the
/// clear before the claim's watch does (`status` shows nobody holding it; the watch is busy,
/// or its event late). The clear stays given up: neither the watch's next policy read
/// (`effective=on`) nor the `value=-` it meets after claims again. Negative control: once the
/// policy moves, codex-alab claims again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_clear_an_acts_checks_meet_first_stays_given_up() {
    let h = harness(&[]).await;
    wait_go(&h.sup).await;
    let (sent, claimed) = (claims_sent(&h), noted(&h, CLAIMED));
    h.fake.delay_clear_event();
    h.fake.clear_claim();
    let ready = h.sup.ready().await;
    assert!(
        matches!(&ready, Ready::Passive(why) if why.contains("no proven claim of ours")),
        "{ready:?} {:?}",
        h.sup.journal()
    );
    the_watch_meets_it_late(&h, claimed).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        claims_sent(&h),
        sent,
        "the person's clear was taken back: {:?}",
        h.sup.journal()
    );
    assert!(h.fake.claim().is_none() && !h.sup.held());
    // The owner's policy moves: codex-alab may claim again.
    h.fake.set_policy_key("continue_text", "keep at it");
    wait_until("claimed again at the policy move", || h.sup.acting()).await;
}

/// Review of 7e7622ff7, finding 3: the approval path gives the before-act checks 1 s, and
/// cancels them past it. Checks cancelled while they give back a claim `status` showed nobody
/// holding (aterm slow to answer the release) still leave codex-alab passive and the person's
/// clear given up: the watch's next `effective=on` read claims nothing. Negative control: the
/// release the cancelled checks started still reaches aterm.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn checks_cancelled_while_they_give_a_claim_back_leave_it_given_up() {
    let h = harness(&[]).await;
    wait_go(&h.sup).await;
    let (sent, claimed) = (claims_sent(&h), noted(&h, CLAIMED));
    let unsets = |f: &FakeAterm| {
        f.requests()
            .iter()
            .filter(|r| r.contains("meta unset supervisor holder="))
            .count()
    };
    let gated_unsets = || {
        h.fake
            .gated_requests()
            .iter()
            .filter(|r| r.contains("meta unset supervisor holder="))
            .count()
    };
    let (before, gated) = (unsets(&h.fake), gated_unsets());
    h.fake.delay_clear_event();
    h.fake.clear_claim();
    h.fake.gate("meta");
    let sup = Arc::clone(&h.sup);
    let checks = tokio::spawn(async move { sup.ready().await });
    wait_until("the checks' release meets the gate", || {
        gated_unsets() > gated
    })
    .await;
    // The caller's budget ran out mid-release.
    checks.abort();
    let _ = checks.await;
    assert!(!h.sup.held(), "{:?}", h.sup.journal());
    h.fake.open_gate("meta");
    assert!(
        h.fake.wait_for(|f| unsets(f) > before).await,
        "the release still went: {:?}",
        h.fake.requests()
    );
    the_watch_meets_it_late(&h, claimed).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        claims_sent(&h),
        sent,
        "the person's clear was taken back: {:?}",
        h.sup.journal()
    );
    assert!(h.fake.claim().is_none() && !h.sup.held());
}

/// Review of 7e7622ff7, finding 2: an echo count no echo consumes (here: the give-back of a
/// stray claim event, aterm holding nothing) must not let a later person's clear pass for an
/// echo. The person clears while the claim's watch renews the claim for the record (a
/// same-holder set, which takes the claim back), so `status` shows it ours when the watch
/// meets the `value=-`. That is a clear: the claim is given up, and the re-taken one given
/// back. Negative control: the renewal with no clear keeps the claim.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_clear_a_renewal_takes_back_is_a_clear_whatever_the_echo_count() {
    let h = harness(&[]).await;
    let holder = codex_alab_aterm::holder_for(std::process::id());
    // A stray event says the claim is ours while codex-alab holds none: it gives it back, and
    // counts that release's echo due, though aterm holds nothing and no echo comes.
    h.fake.clear_claim();
    wait_until("given up", || {
        !h.sup.held() && noted(&h, "the claim was cleared: not taken again") > 0
    })
    .await;
    h.fake
        .emit(&format!("meta field=supervisor value={holder}"));
    wait_until("the stray claim is given back", || {
        noted(&h, "gave back a claim taken while passive") > 0
    })
    .await;
    h.fake.set_policy_key("continue_text", "keep at it");
    wait_until("claimed again", || h.sup.acting()).await;
    // Negative control: a renewal for the record alone keeps the claim.
    let sent = claims_sent(&h);
    h.shared.note_resume_line();
    wait_until("renewed", || claims_sent(&h) > sent).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        h.sup.held() && h.fake.claim().is_some(),
        "{:?}",
        h.sup.journal()
    );
    // The clear across a renewal.
    let met = h.fake.gated_requests().len();
    h.fake.gate("meta");
    h.shared.note_resume_line();
    wait_until("the renewal meets the gate", || {
        h.fake
            .gated_requests()
            .iter()
            .skip(met)
            .any(|r| r.contains("meta set supervisor"))
    })
    .await;
    h.fake.clear_claim();
    h.fake.open_gate("meta");
    wait_until("the clear is met", || {
        noted(&h, "the claim was cleared: not taken again") > 1
            || noted(&h, "came after it claimed again: the claim stands") > 0
    })
    .await;
    assert!(
        noted(&h, "came after it claimed again: the claim stands") == 0,
        "{:?}",
        h.sup.journal()
    );
    wait_until("the re-taken claim is given back", || {
        h.fake.claim().is_none()
    })
    .await;
    assert!(!h.sup.held());
}

/// Review of 7e7622ff7, finding 8: a standby (or a release) withdraws the description, and
/// acting again writes it back, though no turn starts and no plan step moves. Negative
/// control: a thread with nothing to say writes nothing at either.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_description_a_standby_withdrew_is_written_back_when_acting_resumes() {
    let h = harness(&[]).await;
    wait_go(&h.sup).await;
    h.fake.set_effective(false, "suspended");
    wait_until("passive", || !h.sup.acting()).await;
    act_again(&h).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        h.fake.descriptions().is_empty(),
        "{:?}",
        h.fake.descriptions()
    );
    h.rt.set_plan_step(Some("write the inbox"));
    let text = crate::describe::describe(None, Some("write the inbox"));
    assert!(text.is_some());
    h.autopilot.refresh_description(&h.rt).await;
    assert_eq!(h.fake.description(), text);
    for off in ["suspended", "autopilot-off"] {
        h.fake.set_effective(false, off);
        assert!(
            h.fake.wait_for(|f| f.description().is_none()).await,
            "{off}: withdrawn"
        );
        act_again(&h).await;
        assert!(
            h.fake.wait_for(|f| f.description() == text).await,
            "{off}: written back: {:?} {:?}",
            h.fake.descriptions(),
            h.sup.journal()
        );
    }
}
