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

#[path = "crash_wait_tests.rs"]
mod crash_wait_tests;

#[path = "quota_need_admission_tests.rs"]
mod quota_need_admission_tests;

#[path = "recovery_wait_timer_custody_tests.rs"]
mod recovery_wait_timer_custody_tests;

#[path = "policy_wake_tests.rs"]
mod policy_wake_tests;

#[path = "status_off_act_tests.rs"]
mod status_off_act_tests;

#[path = "claim_echo_tests.rs"]
mod claim_echo_tests;

#[path = "policy_error_tests.rs"]
mod policy_error_tests;

#[path = "wake_boundary_tests.rs"]
mod wake_boundary_tests;

#[path = "waits_shown_tests.rs"]
mod waits_shown_tests;

#[path = "held_wall_runtime_tests.rs"]
mod held_wall_runtime_tests;

#[path = "held_status_tests.rs"]
mod held_status_tests;

#[path = "land_status_tests.rs"]
mod land_status_tests;

#[path = "task_face_tests.rs"]
mod task_face_tests;

#[path = "landing_publication_tests.rs"]
mod landing_publication_tests;

#[path = "needs_set_tests.rs"]
mod needs_set_tests;

#[path = "login_watch_tests.rs"]
mod login_watch_tests;

#[path = "fact_read_tests.rs"]
mod fact_read_tests;

#[path = "renewal_follow_handoff_tests.rs"]
mod renewal_follow_handoff_tests;

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
    let h = started(policy, |_, _| {}).await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    h
}

/// [`harness`], each call on the claim's connection answered within `timeout`: a test that
/// holds a request at the fake sets a minute, so its hold, not the call's own deadline,
/// decides when the request is answered.
async fn harness_within(policy: &[(&str, &str)], timeout: Duration) -> Harness {
    let client = ClientOptions {
        timeout,
        ..ClientOptions::default()
    };
    let h = started_with(policy, |_, _| {}, true, client).await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    h
}

/// [`harness`], `before` run on the fake and the supervisor just before the supervisor
/// starts, and not waiting for the claim.
async fn started(policy: &[(&str, &str)], before: impl FnOnce(&FakeAterm, &Supervisor)) -> Harness {
    started_with_claim_watch(policy, before, true).await
}

async fn started_with_claim_watch(
    policy: &[(&str, &str)],
    before: impl FnOnce(&FakeAterm, &Supervisor),
    watch: bool,
) -> Harness {
    started_with(policy, before, watch, ClientOptions::default()).await
}

/// The supervisor fixture built with `client`.
async fn started_with(
    policy: &[(&str, &str)],
    before: impl FnOnce(&FakeAterm, &Supervisor),
    watch: bool,
    client: ClientOptions,
) -> Harness {
    started_timed(policy, before, watch, client, Timing::default()).await
}

/// [`started_with`], the autopilot on `timing`.
async fn started_timed(
    policy: &[(&str, &str)],
    before: impl FnOnce(&FakeAterm, &Supervisor),
    watch: bool,
    client: ClientOptions,
    timing: Timing,
) -> Harness {
    started_timed_with_config(policy, before, watch, client, timing, None).await
}

/// A component fixture may supply the actual resolved provider configuration.
async fn started_timed_with_config(
    policy: &[(&str, &str)],
    before: impl FnOnce(&FakeAterm, &Supervisor),
    watch: bool,
    client: ClientOptions,
    timing: Timing,
    config: Option<Arc<codex_core::config::Config>>,
) -> Harness {
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
        client,
    });
    let autopilot = Autopilot::new(Arc::clone(&sup), None, None, timing, |_| true);
    let id = ThreadId::new();
    let key = id.to_string();
    let store = ExtensionData::new(key.clone());
    store.insert(ThreadRt::new(id, config, false, Duration::ZERO));
    let rt = store
        .get::<ThreadRt>()
        .unwrap_or_else(|| panic!("inserted"));
    autopilot.keep_thread(&rt);
    if watch {
        autopilot.watch_claims();
    }
    before(&fake, &sup);
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
    Harness {
        fake,
        shared,
        sup,
        autopilot,
        rt,
        _store: store,
        _dir: dir,
    }
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
    // A hang detector that says where the claim stalled: a gate run under load once met
    // "never: acting again" with nothing to read it by.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while !h.sup.acting() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "never: acting again {:#?}",
            h.sup.journal()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    wait_go(&h.sup).await;
}

fn shown(h: &Harness) -> Option<String> {
    h.fake.attention().get("codex-alab").cloned()
}

/// A killed image can leave its keyed attention in aterm. The next proven holder settles
/// its own key even when this process has never shown anything; another owner's key stays.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_proven_claim_reconciles_own_attention() {
    let h = started(&[], |fake, _| {
        fake.set_attention_as("codex-alab", "stopped: a refusal in the previous image");
        fake.set_attention_as("person", "keep this note");
    })
    .await;
    wait_until("the fresh claim is proven", || h.sup.acting()).await;
    // Also exercise an explicit clear after correction, when the new image's local map
    // and shown value are both empty: that empty state does not prove aterm is empty.
    h.sup.attention("wall", None).await;
    assert_eq!(shown(&h), None);
    assert_eq!(
        h.fake.attention().get("person").map(String::as_str),
        Some("keep this note")
    );
}

/// Without a proven claim, no earlier image's entry is this process's to reconcile.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn own_attention_is_not_reconciled_without_a_proven_claim() {
    for condition in ["other-holder", "unproven", "off"] {
        let h = started(&[], |fake, _| {
            fake.set_attention_as("codex-alab", "stopped: retained by its holder");
            fake.set_attention_as("person", "keep this note");
            match condition {
                "other-holder" => fake.claim_as("another-supervisor"),
                "unproven" => fake.set_foreground_leader(None),
                "off" => fake.set_effective(false, "disabled"),
                _ => unreachable!(),
            }
        })
        .await;
        wait_until("the claim policy was read", || {
            h.fake
                .requests()
                .iter()
                .any(|request| request.ends_with("harness policy"))
        })
        .await;
        h.sup.attention("wall", None).await;
        h.sup
            .attention("wall", Some("stopped: this process has no claim".into()))
            .await;
        assert!(!h.sup.acting(), "{condition}");
        assert_eq!(
            shown(&h).as_deref(),
            Some("stopped: retained by its holder"),
            "{condition}"
        );
        assert_eq!(
            h.fake.attention().get("person").map(String::as_str),
            Some("keep this note"),
            "{condition}"
        );
    }
}

/// A reconnect takes a new claim. Even a known empty state or a desired text equal to the
/// last shown text must replace whatever the connection before left at aterm.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnecting_reconciles_both_empty_and_wanted_own_attention() {
    let h = harness(&[]).await;
    for desired in [None, Some("stopped: the current unresolved refusal")] {
        h.sup.attention("wall", desired.map(str::to_owned)).await;
        h.fake
            .set_attention_as("codex-alab", "stopped: an older image's entry");
        h.fake.set_attention_as("person", "keep this note");
        let claims = noted(&h, CLAIMED);
        h.sup
            .client()
            .unwrap_or_else(|| panic!("claim connection"))
            .close();
        wait_until("the replacement connection proved a new claim", || {
            noted(&h, CLAIMED) > claims && h.sup.acting()
        })
        .await;
        h.sup.attention("wall", desired.map(str::to_owned)).await;
        assert_eq!(shown(&h).as_deref(), desired);
        assert_eq!(
            h.fake.attention().get("person").map(String::as_str),
            Some("keep this note")
        );
    }
    // The unresolved point was corrected in this image.
    h.sup.attention("wall", None).await;
    assert_eq!(shown(&h), None);
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
        earlier: None,
        questions: false,
        worked: Duration::from_secs(600),
        wall,
        aborted: false,
        ran_tools: false,
        restored: false,
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
            h.sup.attention_for(h.rt.key(), key, None).await;
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

    // A person's own exchange also changes aterm's task face. Even if its
    // evaluation is cancelled, an in-flight publication must settle before a
    // new native turn starts; otherwise the delayed write labels running work
    // as left to the person.
    for (cancel, other_root, other_registry) in [
        (false, false, false),
        (true, false, false),
        (true, true, false),
        (true, true, true),
    ] {
        use crate::decide::Mode as Word;
        use codex_extension_api::TurnLifecycleContributor;
        use codex_extension_api::TurnStartInput;
        use codex_protocol::config_types::CollaborationMode;
        use codex_protocol::config_types::ModeKind;
        use codex_protocol::config_types::Settings;
        // The test owns the evaluation rather than racing the initial claim's
        // automatic look, so aborting it really cancels the publishing caller. The exchange
        // opens a Land session: Cruise leaves none to the person (gap 32).
        let h =
            started_with_claim_watch(&[], |fake, _| fake.set_status("autopilot", "land"), false)
                .await;
        wait_until("the claim is proven and acted under", || h.sup.acting()).await;
        word(&h, "land").await;
        h.rt.with_state(|state| {
            let end = TurnEnd {
                worked: Duration::ZERO,
                ..ended(Some("Here is the answer."), None)
            };
            state.ended_under = Some(Word::Land);
            state.observe_person_hand(end.at);
            state.observe_turn_end(end, &Timing::default());
        });
        assert!(h.rt.state().left);
        h.fake.gate("meta set harness_act");
        let autopilot = Arc::clone(&h.autopilot);
        let rt = Arc::clone(&h.rt);
        let mut publication = Some(tokio::spawn(async move { autopilot.evaluate(rt).await }));
        assert!(
            h.fake
                .wait_for(|fake| fake.gated_requests().iter().any(|line| {
                    line.contains("meta set harness_act left-to-person@v1 until_us=")
                }))
                .await,
            "the actual metadata request reached the barrier"
        );
        if cancel {
            let publication = publication.take().expect("evaluation task");
            publication.abort();
            assert!(
                publication
                    .await
                    .expect_err("evaluation cancelled")
                    .is_cancelled()
            );
        }
        let other_id = ThreadId::new();
        let other_store = ExtensionData::new(other_id.to_string());
        other_store.insert(ThreadRt::new(other_id, None, false, Duration::ZERO));
        let accepted_store = if other_root { &other_store } else { &h._store };
        let accepted_rt = accepted_store.get::<ThreadRt>().expect("accepted root");
        let accepting = if other_registry {
            Autopilot::new(Arc::clone(&h.sup), None, None, Timing::default(), |_| true)
        } else {
            Arc::clone(&h.autopilot)
        };
        if accepting.thread_rt(accepted_rt.key()).is_none() {
            accepting.keep_thread(&accepted_rt);
        }
        let policy = h.sup.policy().expect("proven policy");
        assert!(!h.sup.commit_upgrade(&policy, h.shared.upgrade_generation()));
        let session = ExtensionData::new("session");
        let turn = ExtensionData::new("new-native-turn");
        let mode = CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                model: "gpt-5".into(),
                reasoning_effort: None,
                developer_instructions: None,
            },
        };
        let mut start = accepting.on_turn_start(TurnStartInput {
            turn_id: turn.level_id(),
            collaboration_mode: &mode,
            token_usage_at_turn_start: None,
            session_store: &session,
            thread_store: accepted_store,
            turn_store: &turn,
        });
        let polled = std::future::poll_fn(|cx| {
            std::task::Poll::Ready(std::future::Future::poll(start.as_mut(), cx))
        })
        .await;
        assert!(polled.is_pending());
        assert!(!h.shared.turn_running());
        assert!(h.fake.acts().is_empty());
        h.fake.open_gate("meta set harness_act");
        tokio::time::timeout(Duration::from_secs(60), start)
            .await
            .expect("native acceptance hang detector");
        assert!(h.shared.turn_running_on(accepted_rt.key()));
        if let Some(publication) = publication {
            tokio::time::timeout(Duration::from_secs(60), publication)
                .await
                .expect("evaluation hang detector")
                .expect("evaluation task");
        }
        assert_eq!(h.fake.acts(), vec![crate::words::RULE_LEFT.to_string()]);
        Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
        assert_eq!(h.fake.acts(), vec![crate::words::RULE_LEFT.to_string()]);
    }
}

/// A person's own exchange that opens a Land session (Cruise leaves none to the person, and
/// Land only the opening one: gap 32; the fake must read `land`): their turn ended with an
/// answer, at rest, the point left to them.
fn left_point(h: &Harness) {
    h.rt.with_state(|state| {
        let end = TurnEnd {
            worked: Duration::ZERO,
            ..ended(Some("Here is the answer."), None)
        };
        state.ended_under = Some(crate::decide::Mode::Land);
        state.observe_person_hand(end.at);
        state.observe_turn_end(end, &Timing::default());
    });
    assert!(h.rt.state().left, "the point is left to the person");
}

/// The greeting is quiet locally and published once to the host in either mode.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cruise_opening_is_published_as_the_persons_exchange() {
    for (word, mode) in [
        ("cruise", crate::decide::Mode::Cruise),
        ("land", crate::decide::Mode::Land),
    ] {
        let h = started_with_claim_watch(&[], |fake, _| fake.set_status("autopilot", word), false)
            .await;
        wait_until("the claim is proven", || h.sup.acting()).await;
        h.rt.with_state(|state| {
            let end = TurnEnd {
                worked: Duration::ZERO,
                ..ended(Some("Hey—what are we working on?"), None)
            };
            state.ended_under = Some(mode);
            state.observe_person_hand(end.at);
            state.observe_turn_end(end, &Timing::default());
        });
        assert!(h.rt.state().left);
        for _ in 0..2 {
            Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
            assert_eq!(
                h.fake.acts(),
                vec![crate::words::RULE_LEFT.to_string()],
                "{word}"
            );
            assert!(h.rt.state().left, "{word}");
        }
    }
}

/// AN ATERM WITHOUT THE ACT LEASE (CX-C C1). aterm before 0.110.0 answers `meta prepare
/// harness_act` with its plain `meta` usage. aterm 0.109.0 reads `left-to-person@v1` as
/// `theirs`: the point is recorded in its unbounded spelling, inside the publication gate,
/// once. An older aterm would read the act as `open`: nothing is sent, and the person is told
/// once in the diagnostic journal, with the unavailable capability. Either way the point is
/// settled: no look is owed again, so no timer stands pending without a paced act and the
/// live upgrade does not wait on it (before, every look found the aterm "unsupported" and
/// owed another in 5 s, for ever). NEGATIVE CONTROLS: a turn on its way meets no unbounded
/// write (that aterm cannot refuse a point past it) and leaves a look owed; aterm 0.110.0 takes
/// the bounded spelling, as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_aterm_without_the_act_lease_gets_the_unbounded_left_or_a_loud_refusal() {
    use crate::words::RULE_LEFT;
    for (release, lease, recorded) in [
        ("0.109.0", false, true),
        ("0.108.0", false, false),
        ("0.0.0-fake", false, false),
        ("0.110.0", true, true),
    ] {
        let h = started_with_claim_watch(
            &[],
            |fake, _| {
                fake.set_status("autopilot", "land");
                fake.set_version(release);
                fake.support_act_deadlines(lease);
            },
            false,
        )
        .await;
        wait_until("the claim is proven and acted under", || h.sup.acting()).await;
        left_point(&h);
        Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
        let sets = || {
            h.fake
                .requests()
                .into_iter()
                .filter(|line| line.contains("meta set harness_act"))
                .collect::<Vec<_>>()
        };
        if recorded {
            assert_eq!(h.fake.acts(), vec![RULE_LEFT.to_string()], "{release}");
            let unbounded = sets()
                .iter()
                .all(|line| line.ends_with("meta set harness_act left-to-person@v1"));
            assert_eq!(unbounded, !lease, "{release}: {:?}", sets());
        } else {
            assert!(h.fake.acts().is_empty(), "{release}");
            assert!(sets().is_empty(), "{release}: nothing sent: {:?}", sets());

            assert!(
                h.sup
                    .journal()
                    .iter()
                    .any(|line| line.contains(&format!("aterm {release} predates act leases"))),
                "{release}: {:?}",
                h.sup.journal()
            );
        }
        // Settled: no look owed, so the live upgrade waits on nothing here.
        assert!(!h.rt.timer_pending(), "{release}: {:?}", h.sup.journal());
        assert!(
            !h.autopilot.upgrade_facts().await.waits_pending,
            "{release}"
        );
        // The same point looked at again sends nothing more and says nothing twice.
        let prepares = h
            .fake
            .requests()
            .iter()
            .filter(|line| line.contains("meta prepare harness_act"))
            .count();
        Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
        assert_eq!(
            h.fake
                .requests()
                .iter()
                .filter(|line| line.contains("meta prepare harness_act"))
                .count(),
            prepares,
            "{release}"
        );

        // A new point on an older aterm says nothing twice either (once per process).
        if !recorded {
            // A new opening point (a message once the session has worked is carried under
            // Land, gap 32: the test opens the session again).
            h.rt.with_state(|state| {
                state.point = None;
                state.past_opening = false;
            });
            tokio::time::sleep(Duration::from_millis(5)).await;
            left_point(&h);
            Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
            assert!(h.fake.acts().is_empty(), "{release}");

            assert!(!h.rt.timer_pending(), "{release}");
        }
    }

    // A person's next turn on its way meets no unbounded write: a look is owed instead.
    let h = started_with_claim_watch(
        &[],
        |fake, _| {
            fake.set_status("autopilot", "land");
            fake.set_version("0.109.0");
            fake.support_act_deadlines(false);
        },
        false,
    )
    .await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    left_point(&h);
    h.shared.observe(
        &Frame {
            primary_thread: Some(h.rt.key()),
            displayed_thread: Some(h.rt.key()),
            rollout_path: None,
            cwd: std::path::Path::new("/work"),
            model: "gpt-5",
            codex_home: h._dir.path(),
            embedded: true,
            state: ProgramState::Idle,
            draft_empty: true,
            queued_input: false,
            popup_open: false,
            editor_open: false,
            pending_user_turn: true,
        },
        now_ms(),
    );
    assert!(h.shared.pending_user_turn());
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert!(h.fake.acts().is_empty(), "{:?}", h.sup.journal());
    assert!(
        !h.fake
            .requests()
            .iter()
            .any(|line| line.contains("meta set harness_act")),
        "{:?}",
        h.fake.requests()
    );
    assert!(
        h.sup
            .journal()
            .iter()
            .any(|line| line.contains("cannot refuse a point past it")),
        "{:?}",
        h.sup.journal()
    );
    assert!(
        h.rt.timer_pending(),
        "a look is owed: {:?}",
        h.sup.journal()
    );
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
    h.sup
        .attention_for(h.rt.key(), "plan", Some(plan.clone()))
        .await;
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
    // The posted answer belongs to the root that showed the plan. A switch before
    // its acknowledgement must clear that root's reason and preserve the new root's.
    let other = ThreadId::new().to_string();
    h.shared.note_root_thread(&other);
    h.shared.observe(
        &Frame {
            primary_thread: Some(&other),
            displayed_thread: Some(&other),
            rollout_path: None,
            cwd: std::path::Path::new("/work"),
            model: "gpt-5",
            codex_home: h._dir.path(),
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
    h.sup
        .attention_for(&other, "plan", Some("plan: the other root's plan".into()))
        .await;
    h.sup
        .attention("plan", Some("plan: an independent session reason".into()))
        .await;
    glue("plan-1");
    assert!(
        h.fake
            .wait_for(|f| f.acts().iter().any(|a| a == "plan-approve@v2"))
            .await
    );
    assert_eq!(
        h.sup.standing_attention().as_deref(),
        Some("plan: an independent session reason")
    );
    h.sup.attention("plan", None).await;
    assert!(
        h.fake
            .wait_for(|f| f.attention().get("codex-alab").map(String::as_str)
                == Some("plan: the other root's plan"))
            .await
    );
    h.sup.attention_for(&other, "plan", None).await;
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
                sup.release_for_off(
                    &c,
                    "effective=off why=autopilot-off".to_string(),
                    codex_alab_core::session::HarnessWord::Off,
                )
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

/// A completion may arrive after core admitted the next native turn. Its own accounting
/// ends, but its old point cannot replace the running turn or withdraw that turn's wall.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_native_completion_cannot_replace_the_newer_turn() {
    use codex_extension_api::TurnAbortInput;
    use codex_extension_api::TurnErrorInput;
    use codex_extension_api::TurnLifecycleContributor;
    use codex_extension_api::TurnStopInput;
    use codex_protocol::items::TurnItem;
    use codex_protocol::items::UserMessageItem;
    use codex_protocol::protocol::TurnAbortReason;
    use codex_protocol::user_input::UserInput;

    for outcome in ["success", "interrupted", "failed"] {
        let h = harness(&[("continue", "false")]).await;
        let session = ExtensionData::new("session");
        let old = ExtensionData::new("old-native-turn");
        let current = ExtensionData::new("new-native-turn");
        start_native_turn(&h, &session, &old).await;
        h.shared.expect_mail_turn(codex_alab_core::mail::MailTurn {
            id: 41,
            off: 77,
            from: format!("@{}", h.fake.endpoint().sid),
            kind: "task".into(),
            text: "the old mail task".into(),
        });
        h.autopilot
            .on_item_completed(
                &h._store,
                &old,
                &TurnItem::UserMessage(UserMessageItem {
                    id: "old-mail-input".into(),
                    client_id: None,
                    content: vec![UserInput::Text {
                        text: "the old mail task".into(),
                        text_elements: Vec::new(),
                    }],
                }),
            )
            .await;
        if outcome == "failed" {
            h.autopilot
                .on_turn_error(TurnErrorInput {
                    turn_id: old.level_id(),
                    error: codex_protocol::protocol::CodexErrorInfo::BioPolicy,
                    error_details: &codex_protocol::error::CodexErrorDetails::Stream(
                        "refusal".into(),
                    ),
                    session_store: &session,
                    thread_store: &h._store,
                    turn_store: &old,
                })
                .await;
        }
        start_native_turn(&h, &session, &current).await;
        h.sup
            .attention_for(h.rt.key(), "wall", Some("new refusal".into()))
            .await;
        let origin = h.rt.state().native_origin;
        if outcome == "interrupted" {
            h.autopilot
                .on_turn_abort(TurnAbortInput {
                    reason: TurnAbortReason::Interrupted,
                    session_store: &session,
                    thread_store: &h._store,
                    turn_store: &old,
                })
                .await;
        } else {
            h.autopilot
                .on_turn_stop(TurnStopInput {
                    session_store: &session,
                    thread_store: &h._store,
                    turn_store: &old,
                })
                .await;
        }
        assert_eq!(h.rt.state().native_origin, origin, "outcome={outcome}");
        assert!(h.rt.state().point_turn_id.is_none());
        assert!(h.shared.last_turn(h.rt.key()).is_none());
        assert!(h.shared.turn_running_on(h.rt.key()));
        assert_eq!(h.sup.standing_attention().as_deref(), Some("new refusal"));
        let verdict = if outcome == "success" {
            "handled"
        } else {
            "deferred"
        };
        wait_until(
            "the late turn's own mail is acknowledged and reported",
            || h.fake.receipts() == vec![(41, verdict.into())] && h.fake.posts().len() == 1,
        )
        .await;

        // Control: the selected native turn still publishes its own completion normally.
        h.autopilot
            .on_turn_stop(TurnStopInput {
                session_store: &session,
                thread_store: &h._store,
                turn_store: &current,
            })
            .await;
        assert_eq!(
            h.rt.state().point_turn_id.as_deref(),
            Some(current.level_id())
        );
        assert_eq!(
            h.shared
                .last_turn(h.rt.key())
                .and_then(|turn| turn.turn_id)
                .as_deref(),
            Some(current.level_id())
        );
        assert!(!h.shared.turn_running_on(h.rt.key()));
        assert_eq!(h.fake.receipts(), vec![(41, verdict.into())]);
        assert_eq!(
            h.fake.posts().len(),
            1,
            "the current turn cannot replay the old report"
        );
    }
}

/// Remote acknowledgement can lag a completion. Local reasons retire at the completion,
/// so a later raise, even with identical words, survives the old acknowledgement.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_delayed_completion_flush_preserves_a_new_same_text_wall() {
    use codex_extension_api::TurnLifecycleContributor;
    use codex_extension_api::TurnStopInput;

    let h = harness(&[("continue", "false")]).await;
    let session = ExtensionData::new("session");
    let turn = ExtensionData::new("completed-native-turn");
    start_native_turn(&h, &session, &turn).await;
    h.sup
        .attention_for(h.rt.key(), "wall", Some("same refusal".into()))
        .await;
    h.sup
        .attention_for(h.rt.key(), "turn", Some("old turn".into()))
        .await;
    h.sup
        .attention_for(h.rt.key(), "login", Some("old login".into()))
        .await;
    h.sup
        .attention("zz-independent", Some("independent ask".into()))
        .await;
    let met = h.fake.gated_requests().len();
    h.fake.gate("meta");
    h.autopilot
        .on_turn_stop(TurnStopInput {
            session_store: &session,
            thread_store: &h._store,
            turn_store: &turn,
        })
        .await;
    wait_until("the old completion's flush reaches the gate", || {
        h.fake.gated_requests().len() > met
    })
    .await;
    assert_eq!(
        h.sup.standing_attention().as_deref(),
        Some("independent ask"),
        "old local reasons must be retired before awaiting aterm"
    );
    let sup = Arc::clone(&h.sup);
    let thread = h.rt.key().to_owned();
    let raised = tokio::spawn(async move {
        sup.attention_for(&thread, "wall", Some("same refusal".into()))
            .await;
    });
    wait_until("the identical newer wall is wanted locally", || {
        h.sup.standing_attention().as_deref() == Some("same refusal")
    })
    .await;
    h.fake.open_gate("meta");
    tokio::time::timeout(Duration::from_secs(60), raised)
        .await
        .expect("raise did not hang")
        .expect("raise task");
    assert_eq!(shown(&h).as_deref(), Some("codex-alab: same refusal"));
    h.sup.attention_for(h.rt.key(), "wall", None).await;
    assert_eq!(
        shown(&h).as_deref(),
        Some("codex-alab: independent ask"),
        "unrelated attention retains its owner"
    );
}

async fn start_native_turn(h: &Harness, session: &ExtensionData, turn: &ExtensionData) {
    use codex_extension_api::TurnLifecycleContributor;
    use codex_extension_api::TurnStartInput;
    use codex_protocol::config_types::CollaborationMode;
    use codex_protocol::config_types::ModeKind;
    use codex_protocol::config_types::Settings;
    let mode = CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "gpt-5".into(),
            reasoning_effort: None,
            developer_instructions: None,
        },
    };
    h.autopilot
        .on_turn_start(TurnStartInput {
            turn_id: turn.level_id(),
            collaboration_mode: &mode,
            token_usage_at_turn_start: None,
            session_store: session,
            thread_store: &h._store,
            turn_store: turn,
        })
        .await;
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

/// One whole turn on the thread `store` holds: started, opened by codex-alab (`ours`) or a
/// person, replied `text`, stopped (the review of 2a9241bbf's `review_turn`).
async fn turn_on(h: &Harness, store: &ExtensionData, id: &str, ours: bool, text: &str) {
    use codex_extension_api::TurnLifecycleContributor;
    use codex_extension_api::TurnStartInput;
    use codex_extension_api::TurnStopInput;
    use codex_protocol::config_types::CollaborationMode;
    use codex_protocol::config_types::ModeKind;
    use codex_protocol::config_types::Settings;
    use codex_protocol::items::AgentMessageContent;
    use codex_protocol::items::AgentMessageItem;
    use codex_protocol::items::TurnItem;
    use codex_protocol::models::MessagePhase;
    let session = ExtensionData::new("session");
    let turn = ExtensionData::new(id);
    let mode = CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "gpt-5".to_string(),
            reasoning_effort: None,
            developer_instructions: None,
        },
    };
    h.autopilot
        .on_turn_start(TurnStartInput {
            turn_id: turn.level_id(),
            collaboration_mode: &mode,
            token_usage_at_turn_start: None,
            session_store: &session,
            thread_store: store,
            turn_store: &turn,
        })
        .await;
    let opened = TurnItem::UserMessage(codex_protocol::items::UserMessageItem {
        id: "user-1".to_string(),
        client_id: ours.then(|| format!("{}1", crate::extension::CLIENT_PREFIX)),
        content: vec![codex_protocol::user_input::UserInput::Text {
            text: "keep going".to_string(),
            text_elements: Vec::new(),
        }],
    });
    h.autopilot.on_item_completed(store, &turn, &opened).await;
    let reply = TurnItem::AgentMessage(AgentMessageItem {
        moderation: None,
        id: "agent-1".to_string(),
        content: vec![AgentMessageContent::Text {
            text: text.to_string(),
        }],
        phase: Some(MessagePhase::FinalAnswer),
        memory_citation: None,
        delivery: None,
        questions: None,
    });
    h.autopilot.on_item_completed(store, &turn, &reply).await;
    h.autopilot
        .on_turn_stop(TurnStopInput {
            session_store: &session,
            thread_store: store,
            turn_store: &turn,
        })
        .await;
}

/// One whole turn on the harness's own (root) thread.
async fn turn(h: &Harness, id: &str, ours: bool, text: &str) {
    turn_on(h, &h._store, id, ours, text).await;
}

fn needs_up(h: &Harness) -> Option<String> {
    h.fake.attention().get("needs").cloned()
}

/// Read the session's word as `word` (`status autopilot=`), the policy moved so it is read.
async fn word(h: &Harness, word: &str) {
    use crate::decide::Mode as Word;
    let want = Word::of_word(word).unwrap_or(Word::Cruise);
    h.fake.set_status("autopilot", word);
    h.fake
        .set_policy_key("answer_text", &format!("Pick%20one%20{word}."));
    wait_until("the policy reads the word", || {
        h.sup.policy().is_some_and(|p| p.autopilot == want)
    })
    .await;
}

/// A HOLD KEEPS THE SESSION'S WORD (review of 2a9241bbf, findings 3, 13 and 17): a person's
/// hold over a Land session stores a policy that still reads Land, so a look at the point
/// during it leaves the landing standing. NEGATIVE CONTROL: the same look under a Cruise
/// policy ends the landing (a landed task read under Cruise goes on).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hold_keeps_the_sessions_autopilot_word() {
    use crate::decide::Mode as Word;
    let h = harness(&[]).await;
    word(&h, "land").await;
    let landed = |h: &Harness| {
        h.rt.with_state(|s| {
            s.task = true;
            s.landed = true;
            s.mode = Some(Word::Land);
            s.point = Some(TurnEnd {
                by_autopilot: true,
                by_person: false,
                ..ended(
                    Some("Nothing left: all merged and pushed to origin/main."),
                    None,
                )
            });
        });
    };
    landed(&h);
    h.fake.set_status("hold", "1");
    h.fake.set_effective(false, "hold");
    wait_until("the hold is read", || {
        noted(&h, "the claim is kept, nothing acts") > 0
    })
    .await;
    assert_eq!(
        h.sup.policy().map(|p| p.autopilot),
        Some(Word::Land),
        "{:?}",
        h.sup.journal()
    );
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    let st = h.rt.state();
    assert!(st.landed && st.mode == Some(Word::Land), "{st:?}");
    // NEGATIVE CONTROL: the word Cruise, read at the hold's next read, ends the landing.
    h.fake.set_status("autopilot", "cruise");
    let kept = noted(&h, "the claim is kept, nothing acts");
    h.fake.set_policy_key("answer_text", "Pick%20one.");
    wait_until("the hold is read again", || {
        noted(&h, "the claim is kept, nothing acts") > kept
    })
    .await;
    assert_eq!(h.sup.policy().map(|p| p.autopilot), Some(Word::Cruise));
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert!(!h.rt.state().landed);
}

/// A landed Land task, its point a clean defense at a land act's yield.
fn landed_at_defense(h: &Harness) {
    use crate::decide::Mode as Word;
    h.rt.with_state(|s| {
        s.task = true;
        s.landed = true;
        s.mode = Some(Word::Land);
        s.point = Some(TurnEnd {
            by_autopilot: true,
            by_person: false,
            ..ended(
                Some("Nothing left: all merged and pushed to origin/main."),
                None,
            )
        });
    });
}

/// Hold the session as a person's `hold` does, and wait until the policy read says so.
async fn hold(h: &Harness) {
    let kept = noted(h, "the claim is kept, nothing acts");
    h.fake.set_status("hold", "1");
    h.fake.set_effective(false, "hold");
    wait_until("the hold is read", || {
        noted(h, "the claim is kept, nothing acts") > kept
    })
    .await;
}

/// Read the policy again under the hold (a new generation), and wait until it was read.
async fn reread_hold(h: &Harness, answer: &str) {
    let kept = noted(h, "the claim is kept, nothing acts");
    h.fake.set_policy_key("answer_text", answer);
    wait_until("the hold is read again", || {
        noted(h, "the claim is kept, nothing acts") > kept
    })
    .await;
}

/// A PERSON'S HOLD OVER AUTOPILOT OFF TAKES NO EXCHANGE BACK at the runtime's look (review of
/// gap 32, finding 9): the hold keeps the claim and the policy's word Cruise while the word the
/// supervisor names is Off, so an exchange the Off left stays the person's. NEGATIVE CONTROL:
/// the same look once the hold reads Cruise as the session's word takes this later exchange back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hold_over_autopilot_off_takes_no_exchange_back() {
    use crate::decide::Mode as Word;
    use codex_alab_core::session::HarnessWord;
    let h = harness(&[]).await;
    word(&h, "cruise").await;
    h.fake.set_status("autopilot", "off");
    hold(&h).await;
    assert_eq!(h.sup.policy().map(|p| p.autopilot), Some(Word::Cruise));
    assert_eq!(h.shared.harness_word(), HarnessWord::Off);
    // The person's quick message ends under the hold: no word, as the runtime sets it there.
    h.rt.with_state(|state| {
        state.seed_worked();
        let end = TurnEnd {
            worked: Duration::ZERO,
            ..ended(Some("Here is where it stands."), None)
        };
        state.ended_under = None;
        state.observe_person_hand(end.at);
        state.observe_turn_end(end, &Timing::default());
    });
    assert!(h.rt.state().left, "an Off's exchange is the person's");
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert!(h.rt.state().left, "{:?}", h.sup.journal());
    // NEGATIVE CONTROL: Cruise read under the hold takes it back.
    h.fake.set_status("autopilot", "cruise");
    reread_hold(&h, "Pick%20one%20cruise%20again.").await;
    assert_eq!(h.shared.harness_word(), HarnessWord::Cruise);
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert!(!h.rt.state().left, "{:?}", h.sup.journal());
}

/// A SWITCH TO CRUISE DURING A HOLD IS ACTED ON (recheck of 15dbc93c4, finding 1): the hold's
/// read stores the policy with the session's new word, and every thread looks again under it
/// at once, as the acting path does when the word moves: a landed Land task read under Cruise
/// goes on (its act waits on the hold). Stored silently, the effective read after the hold
/// found the same word and the landing stood for good. NEGATIVE CONTROL: a hold read again
/// with the word and every setting unmoved makes no thread look, and the landing stands (a
/// moved setting has them look, 9df044002).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_switch_to_cruise_during_a_hold_is_acted_on() {
    use crate::decide::Mode as Word;
    let h = harness(&[]).await;
    word(&h, "land").await;
    landed_at_defense(&h);
    hold(&h).await;
    let looks = noted(&h, "the threads look again");
    // NEGATIVE CONTROL first: the word and the settings unmoved (`word` wrote this text).
    reread_hold(&h, "Pick%20one%20land.").await;
    assert_eq!(h.sup.policy().map(|p| p.autopilot), Some(Word::Land));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        noted(&h, "the threads look again"),
        looks,
        "{:?}",
        h.sup.journal()
    );
    assert!(h.rt.state().landed);
    // The switch to Cruise, read under the hold.
    h.fake.set_status("autopilot", "cruise");
    reread_hold(&h, "Pick%20one.").await;
    assert_eq!(h.sup.policy().map(|p| p.autopilot), Some(Word::Cruise));
    assert_eq!(
        noted(
            &h,
            "why=hold autopilot=cruise settings changed: the threads look again"
        ),
        1
    );
    wait_until("the thread looks again under Cruise", || {
        !h.rt.state().landed
    })
    .await;
    assert_eq!(h.rt.state().mode, Some(Word::Cruise));
    // The hold lifts: the word read then is the one already stored, and the thread, which
    // already went on, is not landed again.
    let reread = noted(&h, " re-read");
    h.fake.set_status("hold", "0");
    h.fake.set_effective(true, "enabled");
    wait_until("the hold lifts", || noted(&h, " re-read") > reread).await;
    assert!(!h.rt.state().landed);
}

/// Task reads served so far: one at each decision ([`crate::supervisor::Supervisor::managed`]).
fn task_reads(h: &Harness) -> usize {
    h.fake
        .requests()
        .iter()
        .filter(|r| r.split_whitespace().last() == Some("task"))
        .count()
}

/// Status reads served so far.
fn status_reads(h: &Harness) -> usize {
    h.fake
        .requests()
        .iter()
        .filter(|r| r.split_whitespace().last() == Some("status"))
        .count()
}

/// A WORD ATERM NAMES ONLY AFTER THE CLAIM IS MET AT THE ACT (aterm's `spawn agent=codex`
/// lab leg, 2026-10-23: the worker's codex-alab typed Cruise's `continue@v1` for ten minutes
/// in a Land session). A claim taken while aterm prints `autopilot=-` (its status sweep has
/// not admitted the codex-alab yet) decides under Cruise; aterm then prints `land` with no
/// event. The act's checks meet the word (the word's clock is kept out of the way): no act
/// goes, the policy reads Land, every thread looks again, and the point owes Land's act.
/// Once Land is in force, missing words keep it without reconsideration. Explicit Off
/// withholds acts and releases the claim while preserving Land; a policy read meeting
/// `-` likewise keeps the remembered mode.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_word_aterm_names_after_the_claim_is_met_at_the_act() {
    use crate::decide::Act;
    use crate::decide::Mode as Word;
    use crate::words::RULE_CONTINUE;
    use crate::words::RULE_LAND;
    let h = started_with_claim_watch(
        &[],
        |fake, sup| {
            fake.set_status("autopilot", "-");
            sup.set_word_recheck(Duration::from_secs(3600));
        },
        false,
    )
    .await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    assert_eq!(
        h.sup.policy().map(|p| p.autopilot),
        Some(Word::Cruise),
        "{:?}",
        h.sup.journal()
    );
    h.rt.with_state(|s| {
        s.task = true;
        s.point = Some(ended(Some("Wrote the worker's report."), None));
    });
    assert!(
        matches!(decided_now(&h), Act::Type { rule, .. } if rule == RULE_CONTINUE),
        "{:?}",
        decided_now(&h)
    );
    // aterm admits the codex-alab: the word is printed, with no event.
    h.fake.set_status("autopilot", "land");
    let ready = h.sup.ready().await;
    assert!(
        matches!(&ready, Ready::Wait { why, .. } if why.contains("Autopilot word is land")),
        "{ready:?}"
    );
    assert_eq!(h.sup.policy().map(|p| p.autopilot), Some(Word::Land));
    assert_eq!(
        noted(&h, "status autopilot=land: the threads look again"),
        1,
        "{:?}",
        h.sup.journal()
    );
    assert!(
        matches!(decided_now(&h), Act::Type { rule, .. } if rule == RULE_LAND),
        "{:?}",
        decided_now(&h)
    );
    wait_go(&h.sup).await;
    // A missing word keeps Land without reconsideration. Explicit Off withholds
    // the act and gives back the claim, while the remembered Land mode survives.
    let looks = noted(&h, "the threads look again");
    h.fake.set_status("autopilot", "-");
    assert_eq!(h.sup.ready().await, Ready::Go);
    assert_eq!(h.sup.policy().map(|p| p.autopilot), Some(Word::Land));
    assert_eq!(noted(&h, "the threads look again"), looks);
    h.fake.set_effective_quietly(false, "autopilot-off");
    h.fake.set_status("autopilot", "off");
    assert!(matches!(h.sup.ready().await, Ready::Wait { .. }));
    assert!(!h.sup.acting() && !h.sup.held());
    h.fake.set_status("autopilot", "land");
    act_again(&h).await;
    assert_eq!(h.sup.policy().map(|p| p.autopilot), Some(Word::Land));
    // NEGATIVE CONTROL: a policy read that meets `-` keeps the word in force (its new
    // setting has the threads look again, under Land).
    h.fake.set_status("autopilot", "-");
    let cruise = noted(&h, "autopilot=cruise");
    let reads = noted(&h, "policy gen=");
    h.fake.set_policy_key("answer_text", "Pick%20one.");
    wait_until("the policy is read again", || {
        noted(&h, "policy gen=") > reads
    })
    .await;
    assert_eq!(
        h.sup.policy().map(|p| p.autopilot),
        Some(Word::Land),
        "{:?}",
        h.sup.journal()
    );
    assert_eq!(
        noted(&h, "autopilot=cruise"),
        cruise,
        "{:?}",
        h.sup.journal()
    );
}

/// A WORD ATERM NAMES ONLY AFTER THE CLAIM IS READ ON THE WORD'S CLOCK (review of the act-time
/// read, finding 1): nothing else reads `status` while codex-alab acts on a quiet session, so
/// a landed thread, which owes no act, would never meet a word aterm printed after the claim.
/// While the last read named no word the clock reads again, and the word it meets moves the
/// policy and has every thread look again. NEGATIVE CONTROL: once a read names a word the
/// clock stops, so a word moved later with no event is not polled for.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_word_aterm_names_after_the_claim_is_read_on_the_words_clock() {
    use crate::decide::Mode as Word;
    let h = started(&[], |fake, sup| {
        fake.set_status("autopilot", "-");
        sup.set_word_recheck(Duration::from_millis(20));
    })
    .await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    let reads = status_reads(&h);
    wait_until("the word's clock reads again", || status_reads(&h) > reads).await;
    assert_eq!(h.sup.policy().map(|p| p.autopilot), Some(Word::Cruise));
    // The decisions so far, each reading the session's task (`managed`), the claim's first
    // among them.
    wait_until("the first decision", || task_reads(&h) > 0).await;
    let looks = task_reads(&h);
    h.fake.set_status("autopilot", "land");
    wait_until("the word's clock meets land", || {
        h.sup.policy().is_some_and(|p| p.autopilot == Word::Land)
    })
    .await;
    assert_eq!(
        noted(&h, "status autopilot=land: the threads look again"),
        1,
        "{:?}",
        h.sup.journal()
    );
    // THE LOOK AGAIN'S OWN DECISION reads the person's key (`status human_typed_ms=`, the
    // go-ahead's hour) after the session's task, and may come after the word is met: counted
    // once it has read, so no read of its is taken for the clock's.
    wait_until("the look again reads the person's key", || {
        let requests = h.fake.requests();
        task_reads(&h) > looks
            && requests
                .iter()
                .rposition(|r| r.split_whitespace().last() == Some("task"))
                .is_some_and(|at| {
                    requests[at..]
                        .iter()
                        .any(|r| r.split_whitespace().last() == Some("status"))
                })
    })
    .await;
    // NEGATIVE CONTROL: the word named, the clock stops.
    let reads = status_reads(&h);
    h.fake.set_status("autopilot", "cruise");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(status_reads(&h), reads, "{:?}", h.fake.requests());
    assert_eq!(h.sup.policy().map(|p| p.autopilot), Some(Word::Land));
}

/// A transient refusal at a mode-changing policy event keeps the last word,
/// then retries the unread word without needing another event or a due act.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_word_read_recovers_a_landed_thread_without_another_event() {
    use crate::decide::Mode as Word;
    let h = started(&[], |_, sup| {
        sup.set_word_recheck(Duration::from_millis(20));
    })
    .await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    word(&h, "land").await;
    landed_at_defense(&h);

    h.fake
        .answer_status_with(Some("ERR status temporarily unreadable"));
    h.fake.set_status("autopilot", "cruise");
    let answer = "Retry-after-status-refusal.";
    h.fake.set_policy_key("answer_text", answer);
    wait_until(
        "the changed policy is read despite the refused status",
        || {
            h.sup
                .policy()
                .is_some_and(|p| p.answer_text.as_deref() == Some(answer))
        },
    )
    .await;
    assert_eq!(h.sup.policy().map(|p| p.autopilot), Some(Word::Land));
    assert!(h.rt.state().landed, "the unread word keeps the landing");

    // Restoring this reply emits no event. The landed point owes no act that
    // could perform a readiness check, so only the word's retry can recover it.
    h.fake.answer_status_with(None);
    wait_until("the recovered word is read without another event", || {
        h.sup.policy().is_some_and(|p| p.autopilot == Word::Cruise)
    })
    .await;
    wait_until("the landing ends under the recovered Cruise word", || {
        !h.rt.state().landed
    })
    .await;
}

/// THE HOLD'S WORD WHEN `status` CANNOT BE READ (findings 3, 13 and 17's fallback): a hold
/// read while `status` fails keeps the word read last (Land), never Cruise by default, so the
/// landing stands. NEGATIVE CONTROL: the same hold read once `status` answers again reads the
/// session's word as it stands (Cruise), and the landing ends.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hold_read_while_status_fails_keeps_the_word_read_last() {
    use crate::decide::Mode as Word;
    let h = harness(&[]).await;
    word(&h, "land").await;
    landed_at_defense(&h);
    h.fake.set_status("autopilot", "cruise");
    h.fake.answer_status_with(Some("ERR status unreadable"));
    hold(&h).await;
    assert_eq!(
        h.sup.policy().map(|p| p.autopilot),
        Some(Word::Land),
        "{:?}",
        h.sup.journal()
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    let st = h.rt.state();
    assert!(st.landed && st.mode == Some(Word::Land), "{st:?}");
    // NEGATIVE CONTROL: `status` answers again, and says Cruise.
    h.fake.answer_status_with(None);
    reread_hold(&h, "Pick%20one.").await;
    assert_eq!(h.sup.policy().map(|p| p.autopilot), Some(Word::Cruise));
    wait_until("the landing ends under Cruise", || !h.rt.state().landed).await;
}

/// What the point of a thread decides now under the policy stored.
fn decided_now(h: &Harness) -> crate::decide::Act {
    let policy = h
        .sup
        .policy()
        .unwrap_or_else(|| panic!("no policy: {:?}", h.sup.journal()));
    crate::decide::decide(
        &h.rt.state(),
        &policy,
        &Timing::default(),
        Instant::now(),
        Duration::ZERO,
    )
    .act
}

/// AN OFF READ UNDER A HOLD IS NO MODE (the ninth review of the strict reading, finding 22;
/// a regression from f5514312d): aterm names a hold before an Autopilot Off (`why=hold`
/// while `status autopilot=off`), and the hold's read took that Off for Cruise, so every
/// thread looked again under it and a landed Land task lost its landing. The word in force
/// is kept instead: no thread looks again, the landing stands, and Land picked again within
/// the same hold moves nothing and types nothing (both read with every setting unmoved: a
/// moved setting has the threads look, 9df044002). NEGATIVE CONTROL: Cruise read under the
/// same hold ends the landing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_off_read_under_a_hold_keeps_the_word_and_the_landing() {
    use crate::decide::Mode as Word;
    let h = harness(&[]).await;
    word(&h, "land").await;
    landed_at_defense(&h);
    hold(&h).await;
    let looks = noted(&h, "the threads look again");
    h.fake.set_status("autopilot", "off");
    reread_hold(&h, "Pick%20one%20land.").await;
    assert_eq!(
        h.sup.policy().map(|p| p.autopilot),
        Some(Word::Land),
        "{:?}",
        h.sup.journal()
    );
    // Land picked again within the same hold.
    h.fake.set_status("autopilot", "land");
    reread_hold(&h, "Pick%20one%20land.").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        noted(&h, "the threads look again"),
        looks,
        "{:?}",
        h.sup.journal()
    );
    let st = h.rt.state();
    assert!(st.landed && st.mode == Some(Word::Land), "{st:?}");
    assert!(
        matches!(decided_now(&h), crate::decide::Act::Nothing),
        "{:?}",
        decided_now(&h)
    );
    // NEGATIVE CONTROL: Cruise, read under the same hold.
    h.fake.set_status("autopilot", "cruise");
    reread_hold(&h, "Pick%20one%20cruise.").await;
    assert_eq!(h.sup.policy().map(|p| p.autopilot), Some(Word::Cruise));
    wait_until("the landing ends under Cruise", || !h.rt.state().landed).await;
}

/// AN OFF UNDER A HOLD, THEN ITS RELEASE, KEEPS THE LANDING (finding 22's whole path): the
/// hold lifts with the Off in force (aterm reads `autopilot-off`, a release that parks the
/// threads and keeps the landing), and Land picked again later finds the task landed and
/// types nothing, as the same Off with no hold does (CONTRACT §2: a landing is kept).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_off_under_a_hold_then_its_release_keeps_the_landing() {
    use crate::decide::Mode as Word;
    let h = harness(&[]).await;
    word(&h, "land").await;
    landed_at_defense(&h);
    hold(&h).await;
    h.fake.set_status("autopilot", "off");
    reread_hold(&h, "Pick%20one%20off.").await;
    h.fake.set_status("hold", "0");
    h.fake.set_effective(false, "autopilot-off");
    wait_until("released", || !h.sup.held()).await;
    h.fake.set_status("autopilot", "land");
    h.fake.set_effective(true, "enabled");
    wait_until("claimed again under Land", || {
        h.sup.acting() && h.sup.policy().is_some_and(|p| p.autopilot == Word::Land)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let st = h.rt.state();
    assert!(st.landed && st.mode == Some(Word::Land), "{st:?}");
    assert!(
        matches!(decided_now(&h), crate::decide::Act::Nothing),
        "{:?}",
        decided_now(&h)
    );
}

/// AN OFF ENDS THE NEEDS THE THREAD HELD (review of 2a9241bbf, findings 4 and 12): a need a
/// person's Off took off the screen does not come back after the Off at codex-alab's next
/// turn end that names none, and the record names it as the set the Off took down
/// (`needsOff`, review of the forever autopilot, 2026-10-20), so no later image puts it back.
/// NEGATIVE CONTROL: a report that names it again raises it, and the record names none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_off_ends_the_needs_the_thread_held() {
    let h = harness(&[]).await;
    turn(&h, "t1", false, "Started.").await;
    turn(
        &h,
        "t2",
        true,
        "Stage 2 is in.\nNEEDS YOU: the signing key password",
    )
    .await;
    wait_until("the need is shown", || needs_up(&h).is_some()).await;
    let key = h.sup.needs_key();
    assert_ne!(key, 0);
    assert_eq!(h.shared.needs_off(), 0);
    h.fake.set_effective(false, "autopilot-off");
    wait_until("the off clears it", || {
        needs_up(&h).is_none() && !h.sup.acting()
    })
    .await;
    assert_eq!(h.shared.needs_off(), key);
    act_again(&h).await;
    turn(&h, "t3", true, "Stage 3 is in.").await;
    assert_eq!(h.rt.state().needs, crate::decide::Needs::default());
    assert_eq!(h.sup.needs_key(), 0);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(needs_up(&h), None, "{:?}", h.sup.journal());
    // NEGATIVE CONTROL: named again, it is raised again.
    turn(
        &h,
        "t4",
        true,
        "Stage 4 is in.\nNEEDS YOU: the signing key password",
    )
    .await;
    wait_until("named again, it is shown", || needs_up(&h).is_some()).await;
    assert_eq!(h.shared.needs_off(), 0);
}

/// THE NEEDS WAIT FOR THE CLAIM (review of 2a9241bbf, finding 21 X): a need a report names
/// while codex-alab holds no claim (after an Off) is not put on the person's screen; it is
/// once codex-alab claims again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_need_named_while_released_is_raised_at_the_claim() {
    let h = harness(&[]).await;
    turn(&h, "t1", false, "Started.").await;
    h.fake.set_effective(false, "autopilot-off");
    wait_until("released", || h.fake.claim().is_none() && !h.sup.acting()).await;
    turn(
        &h,
        "t2",
        true,
        "Stage 2 is in.\nNEEDS YOU: the signing key password",
    )
    .await;
    assert_ne!(h.sup.needs_key(), 0, "wanted");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(needs_up(&h), None, "{:?}", h.sup.journal());
    act_again(&h).await;
    wait_until("raised at the claim", || needs_up(&h).is_some()).await;
    assert_eq!(
        needs_up(&h).as_deref(),
        Some("Codex needs you: the signing key password — carrying on")
    );
}

/// THE EXIT TAKES THE NEEDS DOWN (decision 2, which the review of 2a9241bbf's findings 11
/// and 18 rely on: the new image raises them again): nobody is left to carry on beside them
/// once the process leaves.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_exit_takes_the_needs_down() {
    let h = harness(&[]).await;
    turn(
        &h,
        "t1",
        false,
        "Started.\nNEEDS YOU: the signing key password",
    )
    .await;
    wait_until("the need is shown", || needs_up(&h).is_some()).await;
    let shared = h.shared;
    let released =
        tokio::task::spawn_blocking(move || shared.release_claim(Duration::from_secs(60)))
            .await
            .unwrap_or_else(|err| panic!("the release: {err}"));
    assert!(released, "{:?}", h.sup.journal());
    assert!(h.fake.claim().is_none());
    assert_eq!(needs_up(&h), None, "{:?}", h.fake.requests());
}

/// THE EXIT TAKES CODEX-ALAB'S OWN ENTRY DOWN, as it takes the needs down (review of
/// 23c4163306, finding 6): aterm counts the `owner=codex-alab` entry in a task's attention
/// flag, so a reason left up said "needs you" for a process that had gone until the next
/// claim in the session. The point is a sign-in wall's, left to a person. NEGATIVE CONTROL:
/// with the exit's own clear removed the entry stands after the release.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_exit_takes_codex_alabs_own_entry_down() {
    let h = harness(&[]).await;
    point(&h, ended(None, Some(Wall::Login)));
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    wait_until("the wall's reason is shown", || {
        shown(&h).is_some_and(|text| text.starts_with("login: "))
    })
    .await;
    let shared = h.shared;
    let released =
        tokio::task::spawn_blocking(move || shared.release_claim(Duration::from_secs(60)))
            .await
            .unwrap_or_else(|err| panic!("the release: {err}"));
    assert!(released, "{:?}", h.sup.journal());
    assert!(h.fake.claim().is_none());
    assert_eq!(shown(&h), None, "{:?}", h.fake.requests());
}

/// THE EXIT AFTER A NEED STILL IN FLIGHT: aterm applies a set before it answers it, so an exit
/// that read what aterm shows before that answer came left the need up after the process had
/// gone (a full-suite run caught `the_exit_takes_the_needs_down` so, 2026-10-24). The exit
/// waits for the sync in flight and takes the need down. The set is held at aterm, unapplied,
/// until the exit has begun, so the order is the one that run met.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_exit_takes_down_a_need_whose_set_was_still_in_flight() {
    const SET: &str = "meta set attention owner=needs";
    let h = harness_within(&[], Duration::from_secs(60)).await;
    // The claim's own settle of the entry is answered first, so the exit owes the need's
    // clear only to the set in flight (review of 23c4163306, finding 5).
    wait_until("the claim settled the needs entry", || {
        h.fake
            .requests()
            .iter()
            .any(|r| r.ends_with("meta unset attention owner=needs"))
    })
    .await;
    h.fake.gate(SET);
    turn(
        &h,
        "t1",
        false,
        "Started.\nNEEDS YOU: the signing key password",
    )
    .await;
    wait_until("the need's set is in flight", || {
        h.fake.gated_requests().iter().any(|r| r.contains(SET))
    })
    .await;
    assert_eq!(needs_up(&h), None);
    let shared = h.shared;
    let release =
        tokio::task::spawn_blocking(move || shared.release_claim(Duration::from_secs(60)));
    wait_until("the exit began", || !h.sup.held()).await;
    h.fake.open_gate(SET);
    let released = release
        .await
        .unwrap_or_else(|err| panic!("the release: {err}"));
    assert!(released, "{:?}", h.sup.journal());
    assert!(h.fake.claim().is_none());
    assert_eq!(needs_up(&h), None, "{:?}", h.fake.requests());
}

/// The entry a process before this one left up: it crashed, or was killed, so its exit
/// never took its needs down.
const LEFT_ENTRY: &str = "Codex needs you: the signing key password \u{2014} carrying on";

/// A real clear can apply while its acknowledgement is lost. A same-text
/// report arriving during that clear must be shown under the next proven claim,
/// even though this process still caches the text it previously acknowledged.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_same_text_need_is_reconciled_after_an_applied_clear_loses_its_reply() {
    const CLEAR: &str = "meta unset attention owner=needs";
    const CLAIM: &str = "meta set supervisor";
    for lose_reply in [false, true] {
        let h = harness(&[]).await;
        let standing = vec!["the deploy token".to_string()];
        h.sup.needs(&standing, 7).await;
        let expected = needs_up(&h).expect("initial need is actually on the server");
        assert_eq!(h.sup.needs_shown(), Some(expected.clone()));
        h.fake.set_attention_as("person", "a reviewer is looking");
        let claims = claims_sent(&h);
        let gated = h.fake.gated_requests().len();
        h.fake.gate(CLEAR);
        if lose_reply {
            h.fake.drop_next_reply(CLEAR);
            h.fake.gate(CLAIM);
        }
        assert!(h.sup.want_needs(&[], 0));
        let clear = {
            let sup = Arc::clone(&h.sup);
            tokio::spawn(async move { sup.show_needs().await })
        };
        wait_until("the real needs clear reaches the server gate", || {
            h.fake.gated_requests().len() > gated
        })
        .await;
        assert!(h.sup.want_needs(&standing, 7));
        h.fake.open_gate(CLEAR);
        tokio::time::timeout(Duration::from_secs(60), clear)
            .await
            .expect("the clear receives its reply or actual socket closure")
            .expect("clear task");
        let applied_clear = needs_up(&h);
        let cached_after_clear = h.sup.needs_shown();
        if lose_reply {
            h.fake.open_gate(CLAIM);
            wait_until("a new request connection proves its claim", || {
                claims_sent(&h) > claims && h.sup.acting()
            })
            .await;
        }
        // This synchronizes with claim-time publication through the production
        // attention permit; no sleep guesses when that reconciliation finished.
        h.sup.show_needs().await;
        let reconciled = needs_up(&h);
        let person = h.fake.attention().get("person").cloned();
        let shared = h.shared;
        let released =
            tokio::task::spawn_blocking(move || shared.release_claim(Duration::from_secs(60)))
                .await
                .expect("scratch supervisor releases before the verdict");
        assert!(released);
        assert_eq!(applied_clear, None, "the server applied its real clear");
        assert_eq!(
            cached_after_clear,
            lose_reply.then(|| expected.clone()),
            "only an acknowledged clear changes this process's cached receipt"
        );
        assert_eq!(person.as_deref(), Some("a reviewer is looking"));
        assert_eq!(
            reconciled,
            Some(expected),
            "lose_reply={lose_reply}: the new claim must reconcile an unknown needs entry"
        );
    }
}

/// A NEEDS ENTRY LEFT BEFORE THE CLAIM COMES DOWN (aterm `448726c354`; recheck of 15dbc93c4,
/// finding 7): a process that crashed left `owner=needs` up, and the new process, which never
/// put it there (its `needs_shown` none), took it for its own none and left it, so the menu
/// bar said "needs you" for good. At its claim, no need standing, it takes the entry down,
/// journaled with the reason; another owner's entry stays. NEGATIVE CONTROLS: a need standing
/// at the claim puts its own text up in the entry's place, nothing taken down first; behind
/// another holder's claim nothing is touched.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_needs_entry_left_before_the_claim_comes_down() {
    use crate::supervisor::NEEDS_LEFT_DOWN;
    let left = |fake: &FakeAterm| {
        fake.set_attention_as("needs", LEFT_ENTRY);
        fake.set_attention_as("someone", "a reviewer is looking");
    };
    let h = started(&[], |fake, _| left(fake)).await;
    wait_until("acting", || h.sup.acting()).await;
    wait_until("the left entry comes down", || needs_up(&h).is_none()).await;
    assert_eq!(noted(&h, NEEDS_LEFT_DOWN), 1, "{:?}", h.sup.journal());
    assert_eq!(
        h.fake.attention().get("someone").map(String::as_str),
        Some("a reviewer is looking")
    );
    // NEGATIVE CONTROL: a need standing at the claim replaces the entry.
    let h = started(&[], |fake, sup| {
        left(fake);
        assert!(sup.want_needs(&["the deploy token".to_string()], 7));
    })
    .await;
    wait_until("acting", || h.sup.acting()).await;
    wait_until("its own need replaces the entry", || {
        needs_up(&h).as_deref() == Some("Codex needs you: the deploy token \u{2014} carrying on")
    })
    .await;
    assert_eq!(noted(&h, "CLEARED needs"), 0, "{:?}", h.sup.journal());
    // NEGATIVE CONTROL: behind another holder's claim, nothing is touched.
    let h = started(&[], |fake, _| {
        left(fake);
        fake.claim_as("manager@1");
    })
    .await;
    wait_until("another holder has the session", || {
        noted(&h, "holds the session; waiting for its release") > 0
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(needs_up(&h).as_deref(), Some(LEFT_ENTRY));
    assert!(
        !h.fake
            .requests()
            .iter()
            .any(|r| r.contains("attention owner=needs")),
        "{:?}",
        h.fake.requests()
    );
}

/// ONLY THE ROOT THREAD'S NEEDS REACH THE PERSON (review of 2a9241bbf, finding 21 U): a side
/// thread the TUI shows (a sub-agent's) names a need, and nothing is raised. NEGATIVE
/// CONTROL: the same words in the root thread's report are.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_side_threads_needs_never_reach_the_person() {
    let h = harness(&[]).await;
    let side_id = ThreadId::new();
    let side_key = side_id.to_string();
    let side = ExtensionData::new(side_key.clone());
    side.insert(ThreadRt::new(side_id, None, false, Duration::ZERO));
    let side_rt = side.get::<ThreadRt>().unwrap_or_else(|| panic!("inserted"));
    h.autopilot.keep_thread(&side_rt);
    h.shared.observe(
        &Frame {
            primary_thread: Some(h.rt.key()),
            displayed_thread: Some(&side_key),
            rollout_path: None,
            cwd: std::path::Path::new("/work"),
            model: "gpt-5",
            codex_home: h._dir.path(),
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
    assert!(h.shared.is_registered(&side_key) && !h.shared.is_root_thread(&side_key));
    let words = "Sub-task done.\nNEEDS YOU: the signing key password";
    turn_on(&h, &side, "s1", false, "Look into it.").await;
    turn_on(&h, &side, "s2", true, words).await;
    assert_eq!(
        side_rt.state().needs.standing.len(),
        1,
        "the side thread holds it"
    );
    assert_eq!(h.sup.needs_key(), 0);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(needs_up(&h), None);
    // NEGATIVE CONTROL: the root thread's report.
    turn(&h, "t1", false, words).await;
    wait_until("the root's need is shown", || needs_up(&h).is_some()).await;
}

/// CODEX-ALAB'S OWN NEED IS NEVER HIDDEN (parity-5, N; FOREVER CRUISE: NEEDS YOU is loud and
/// never blocks): the quota's need goes up in the `owner=needs` entry, first, beside an `api:`
/// reason that stands under `owner=codex-alab`, and beside the worker's own needs, counted
/// with them in aterm's frame (`(+N more)`, aterm's `needs_n=`). Its own wall's end takes only
/// it down, and a person's Off takes it down with the rest, for good: the next needs set after
/// the On is the worker's alone. CONTROL: raised as one of
/// codex-alab's `attention` reasons (what B3 did), it sat behind the `api:` one, the only one
/// aterm shows.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn its_own_need_stands_beside_every_other_and_is_never_hidden() {
    use crate::decide::QUOTA_NEED;
    const API: &str = "api: Codex reported a server error; retry_api_errors is off";
    let quota = format!("Codex needs you: {QUOTA_NEED} \u{2014} carrying on");
    let h = harness(&[]).await;
    turn(&h, "t1", false, "Started.").await;
    h.sup
        .attention_for(h.rt.key(), "api", Some(API.to_string()))
        .await;
    wait_until("the reason is shown", || shown(&h).as_deref() == Some(API)).await;
    // CONTROL: as an attention reason, the need sits behind the `api:` one.
    h.sup
        .attention_for(
            h.rt.key(),
            "limit",
            Some(format!("limit: NEEDS YOU: {QUOTA_NEED}")),
        )
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(shown(&h).as_deref(), Some(API), "hidden there");
    assert_eq!(needs_up(&h), None);
    h.sup.attention_for(h.rt.key(), "limit", None).await;
    // Its own need: in the needs entry, beside the reason.
    h.sup
        .own_need_for(h.rt.key(), "limit", Some(QUOTA_NEED.to_string()))
        .await;
    wait_until("the need is shown", || {
        needs_up(&h).as_deref() == Some(quota.as_str())
    })
    .await;
    assert_eq!(
        shown(&h).as_deref(),
        Some(API),
        "the reason stands beside it"
    );
    assert_eq!(
        h.fake.attention_kinds().get("needs").map(String::as_str),
        Some("ask")
    );
    // A turn that ends with no wall got past the quota: its need goes (and the reason).
    turn(
        &h,
        "t2",
        true,
        "Stage 2 is in.\nNEEDS YOU: the signing key password",
    )
    .await;
    let worker = "Codex needs you: the signing key password \u{2014} carrying on";
    wait_until("the worker's need stands alone", || {
        needs_up(&h).as_deref() == Some(worker) && shown(&h).is_none()
    })
    .await;
    // The quota's wall again: both stand, the quota's first, counted.
    h.sup
        .own_need_for(h.rt.key(), "limit", Some(QUOTA_NEED.to_string()))
        .await;
    let both = format!("Codex needs you: {QUOTA_NEED} (+1 more) \u{2014} carrying on");
    wait_until("both are counted", || {
        needs_up(&h).as_deref() == Some(both.as_str())
    })
    .await;
    // Its own wall's end takes only it down.
    h.sup.own_need_for(h.rt.key(), "limit", None).await;
    wait_until("the worker's need stands alone again", || {
        needs_up(&h).as_deref() == Some(worker)
    })
    .await;
    // A person's Off takes every need down, its own too, and nothing raises it again after.
    h.sup
        .own_need_for(h.rt.key(), "limit", Some(QUOTA_NEED.to_string()))
        .await;
    wait_until("raised again", || {
        needs_up(&h).as_deref() == Some(both.as_str())
    })
    .await;
    h.fake.set_effective(false, "autopilot-off");
    wait_until("the off takes them down", || {
        needs_up(&h).is_none() && !h.sup.acting()
    })
    .await;
    // Not raised while codex-alab does not act.
    h.sup
        .own_need_for(h.rt.key(), "limit", Some(QUOTA_NEED.to_string()))
        .await;
    act_again(&h).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(needs_up(&h), None, "{:?}", h.sup.journal());
    // FOR GOOD (parity-5 review, R3): the next needs set after the On is the worker's alone.
    // Neither the need the Off took down nor the one raised while passive comes back with
    // it (RED with the Off keeping its own needs, or with a passive raise stored).
    h.sup
        .needs(&["the signing key password".to_string()], 7)
        .await;
    wait_until("the worker's need after the On", || needs_up(&h).is_some()).await;
    assert_eq!(
        needs_up(&h).as_deref(),
        Some(worker),
        "{:?}",
        h.sup.journal()
    );
}

/// THE UPGRADE'S EXEC FENCES EVERY LOOK (review of 2a9241bbf, finding 23 O): an evaluation
/// that comes once the TUI was asked to leave raises nothing for its point. NEGATIVE
/// CONTROL: the same point with no exec under way raises its attention.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_look_once_the_upgrade_exec_began_acts_on_nothing() {
    let h = harness(&[("questions", "ask")]).await;
    point(&h, ended(Some("Should I rename the module?"), None));
    h.autopilot.set_upgrade_requested(true);
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert_eq!(noted(&h, "the upgrade's exec is under way; no act"), 1);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(shown(&h), None, "{:?}", h.sup.journal());
    // NEGATIVE CONTROL: no exec under way.
    h.autopilot.set_upgrade_requested(false);
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert!(
        h.fake
            .wait_for(|f| f.attention().contains_key("codex-alab"))
            .await,
        "{:?}",
        h.sup.journal()
    );
}

/// A LANDING ENDS THE CARRY-ON CHAIN (review of 2a9241bbf, finding 22): FOREVER LAND's
/// landing is recorded, and the root thread's chain of carry-ons after relaunches goes back
/// to none, as a person's turn sends it. NEGATIVE CONTROL: a Land point that does not land
/// (no defense: a nudge is owed) leaves the chain.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_landing_ends_the_carry_on_chain() {
    use crate::decide::Mode as Word;
    let h = harness(&[]).await;
    word(&h, "land").await;
    let at = |h: &Harness, text: &str| {
        h.rt.with_state(|s| {
            s.task = true;
            s.mode = Some(Word::Land);
            s.land.typed = true;
            s.landing = true;
            s.point = Some(TurnEnd {
                by_autopilot: true,
                by_person: false,
                ..ended(Some(text), None)
            });
        });
    };
    // NEGATIVE CONTROL first: no defense, no landing.
    h.shared.set_carried_on(2);
    at(&h, "Merged the parser; the docs are next.");
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert!(!h.rt.state().landed);
    assert_eq!(h.shared.carried_on(), 2, "{:?}", h.sup.journal());
    at(&h, "Nothing left: all merged and pushed to origin/main.");
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert!(h.rt.state().landed, "{:?}", h.sup.journal());
    assert_eq!(h.shared.carried_on(), 0);
    assert!(
        h.fake
            .wait_for(|f| f.acts().iter().any(|a| a == "landed@v1"))
            .await,
        "{:?}",
        h.fake.acts()
    );
}

/// A FACE THAT HOLDS UNTIL ANOTHER TURN WAITS FOR THE TUI TO TAKE ITS TURN'S END (aterm's
/// codex-alab lab, reland leg, 2026-10-24: a TUI behind under load wrote the person's turn's
/// end and the land turn's begin after `landed@v1`, and aterm, counting a turn past the
/// landing's mark, reopened it). While the TUI shows the thread, `landed@v1` and
/// `left-to-person@v1` are recorded only once its chat widget took the end of the turn they
/// answer, so that turn's begin edge is written first; none is released by the wait's limit
/// (its note). NEGATIVE CONTROLS: with no TUI in the process (no frame waker), with a TUI that
/// shows another thread, and once the TUI switches to another thread mid-wait (its widget no
/// longer takes this thread's end), each is recorded with no wait run out.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_held_face_waits_for_the_tui_to_take_its_turns_end() {
    use crate::decide::Mode as Word;
    use crate::words::RULE_LANDED;
    use crate::words::RULE_LEFT;
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Tui {
        None,
        Books,
        Elsewhere,
        SwitchesAway,
    }
    let show = |h: &Harness, thread: &str| {
        h.shared.observe(
            &Frame {
                primary_thread: Some(h.rt.key()),
                displayed_thread: Some(thread),
                rollout_path: None,
                cwd: std::path::Path::new("/work"),
                model: "gpt-5",
                codex_home: h._dir.path(),
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
    };
    for rule in [RULE_LANDED, RULE_LEFT] {
        for tui in [Tui::None, Tui::Books, Tui::Elsewhere, Tui::SwitchesAway] {
            let h = harness(&[]).await;
            if rule == RULE_LANDED {
                word(&h, "land").await;
                h.rt.with_state(|s| {
                    s.task = true;
                    s.mode = Some(Word::Land);
                    s.land.typed = true;
                    s.landing = true;
                    s.point = Some(TurnEnd {
                        by_autopilot: true,
                        by_person: false,
                        ..ended(
                            Some("Nothing left: all merged and pushed to origin/main."),
                            None,
                        )
                    });
                });
            } else {
                word(&h, "land").await;
                left_point(&h);
            }
            h.shared.turn_started(h.rt.key(), "answered-turn");
            h.shared.turn_ended(h.rt.key(), "answered-turn");
            if tui != Tui::None {
                assert!(h.shared.set_waker(Box::new(|| {})));
            }
            if tui == Tui::Elsewhere {
                show(&h, "another-thread");
            }
            let evaluation = tokio::spawn(Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)));
            if matches!(tui, Tui::Books | Tui::SwitchesAway) {
                tokio::time::sleep(Duration::from_millis(300)).await;
                assert!(
                    h.fake.acts().is_empty(),
                    "{rule} ({tui:?}): recorded before the TUI took its turn's end: {:?}",
                    h.sup.journal()
                );
                if tui == Tui::Books {
                    h.shared.turn_handled_end(h.rt.key(), "answered-turn");
                } else {
                    show(&h, "another-thread");
                }
            }
            tokio::time::timeout(Duration::from_secs(60), evaluation)
                .await
                .expect("evaluation hang detector")
                .expect("evaluation task");
            assert!(
                h.fake
                    .wait_for(|f| f.acts().iter().any(|a| a == rule))
                    .await,
                "{rule} ({tui:?}): {:?}",
                h.sup.journal()
            );
            assert_eq!(h.fake.acts(), vec![rule.to_string()], "{rule} ({tui:?})");
            assert!(
                !h.sup
                    .journal()
                    .iter()
                    .any(|line| line.contains("the TUI took no end of turn")),
                "{rule} ({tui:?}): released by the wait's limit, not its end: {:?}",
                h.sup.journal()
            );
        }
    }
}

/// THE PROBE READS THE FOLDER THE WORKER'S LAST COMMAND RAN IN (review of 2a9241bbf, finding
/// 6): a Land defense at a land act's yield, the worker's last command run in a repository
/// that holds an untracked file, is probed there and does not land. NEGATIVE CONTROL: before
/// any command ran, no folder of the worker's is known (this harness's thread has no folder of
/// its own), nothing is found and the same defense lands.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_probe_reads_the_folder_the_workers_last_command_ran_in() {
    use crate::decide::Mode as Word;
    let h = harness(&[]).await;
    word(&h, "land").await;
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&work).unwrap_or_else(|err| panic!("work: {err}"));
    let init = std::process::Command::new("git")
        .args(["init", "-q", "."])
        .current_dir(&work)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap_or_else(|err| panic!("git init: {err}"));
    assert!(init.status.success(), "{init:?}");
    std::fs::write(work.join("left.txt"), "left\n").unwrap_or_else(|err| panic!("{err}"));
    let at_defense = |h: &Harness| {
        h.rt.with_state(|s| {
            s.task = true;
            s.landed = false;
            s.mode = Some(Word::Land);
            s.land.typed = true;
            s.landing = true;
            s.point = Some(TurnEnd {
                by_autopilot: true,
                by_person: false,
                ..ended(
                    Some("Nothing left: all merged and pushed to origin/main."),
                    None,
                )
            });
        });
    };
    // NEGATIVE CONTROL first: no command ran.
    at_defense(&h);
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert_eq!(noted(&h, "the repository probe: nothing found"), 1);
    assert!(h.rt.state().landed, "{:?}", h.sup.journal());
    crate::rest_tests::command_in(&h.autopilot, &h._store, &work.display().to_string()).await;
    at_defense(&h);
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert_eq!(
        noted(&h, "/work: 1 untracked (left.txt)"),
        1,
        "{:?}",
        h.sup.journal()
    );
    assert!(!h.rt.state().landed);
}

/// The thread is idle after its turn (`on_thread_idle`): its evaluation is spawned.
async fn idle(h: &Harness) {
    use codex_extension_api::ThreadLifecycleContributor;
    let session = ExtensionData::new("session");
    h.autopilot
        .on_thread_idle(codex_extension_api::ThreadIdleInput {
            cause: codex_extension_api::ThreadIdleCause::Completed,
            session_store: &session,
            thread_store: &h._store,
        })
        .await;
}

/// AN EVALUATION QUEUED FOR THE PERMIT HOLDS THE UPGRADE (review of the forever autopilot,
/// 2026-10-20, finding 4): a turn end whose evaluation waits behind the permit the upgrade
/// takes is a point not decided yet, and the upgrade waits for it rather than exec over it
/// (the new image would never decide it). So is a look a claim transition asks for, and the
/// look a not-now's timer asks for once it fired (out of its slot, it is no pending wait).
/// NEGATIVE CONTROL: once its evaluation decided it, the point holds nothing; nor does a
/// not-now's timer still pending (a wait the upgrade already waits for).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_end_queued_for_the_permit_holds_the_upgrade() {
    let h = harness(&[]).await;
    let held_by = |facts: crate::upgrade::UpgradeFacts| match crate::upgrade::check(
        &crate::upgrade::UpgradeFacts {
            unresumable: None,
            ..facts
        },
    ) {
        crate::upgrade::UpgradeCheck::Wait(why) => Some(why),
        _ => None,
    };
    turn(&h, "t1", false, "Started.").await;
    idle(&h).await;
    wait_until("the person's turn end is decided", || !h.rt.undecided()).await;
    let permit = h.rt.hold_eval_when_free().await;
    let looks = h.rt.evaluations();
    turn(&h, "t2", true, "Stage 2 is in; stage 3 is next.").await;
    // From its end to its idle's evaluation, the point is undecided.
    assert!(h.rt.undecided());
    idle(&h).await;
    wait_until("its evaluation is queued", || h.rt.evaluations() > looks).await;
    let facts = h.autopilot.upgrade_facts().await;
    assert_eq!(facts.undecided.as_deref(), Some(h.rt.key()));
    let why = held_by(facts).unwrap_or_else(|| panic!("the upgrade waits"));
    assert!(why.contains("not decided yet"), "{why}");
    drop(permit);
    // NEGATIVE CONTROL: decided.
    wait_until("its evaluation decided it", || !h.rt.undecided()).await;
    let facts = h.autopilot.upgrade_facts().await;
    assert_eq!(facts.undecided, None);
    assert_eq!(held_by(facts), None);
    // A claim transition asks every thread to look again: undecided until it looked (with no
    // paced act owed, which the upgrade would carry instead).
    Autopilot::cancel_timer(&h.rt).await;
    assert!(!h.rt.undecided());
    let permit = h.rt.hold_eval_when_free().await;
    let looks = h.rt.evaluations();
    h.fake.set_effective(false, "autopilot-off");
    wait_until("released", || !h.sup.acting()).await;
    act_again(&h).await;
    wait_until("its look is queued", || h.rt.evaluations() > looks).await;
    assert!(h.rt.undecided());
    drop(permit);
    wait_until("looked again", || !h.rt.undecided()).await;
    // So does a wait that is no paced act's (a not-now) once its timer fired: pending, it holds
    // the upgrade as a wait; fired, its look is queued for the permit.
    Autopilot::cancel_timer(&h.rt).await;
    let permit = h.rt.hold_eval_when_free().await;
    let looks = h.rt.evaluations();
    h.autopilot.schedule(
        &h.rt,
        crate::supervisor::clock() + Duration::from_millis(50),
    );
    assert!(!h.rt.undecided(), "a pending wait is no undecided point");
    wait_until("the timer fired", || h.rt.evaluations() > looks).await;
    assert!(h.rt.undecided());
    assert!(!h.autopilot.upgrade_facts().await.waits_pending);
    drop(permit);
    wait_until("its look decided", || !h.rt.undecided()).await;
}

/// One whole turn NOBODY ASKED on the harness's thread (no `UserMessage` opened it: a goal's
/// continuation), `during` run once it started (before its words), then its agent messages
/// `said` in order, the last marked the final answer.
async fn wake_turn(h: &Harness, id: &str, during: impl FnOnce(), said: &[&str]) {
    use codex_extension_api::TurnLifecycleContributor;
    use codex_extension_api::TurnStartInput;
    use codex_extension_api::TurnStopInput;
    use codex_protocol::config_types::CollaborationMode;
    use codex_protocol::config_types::ModeKind;
    use codex_protocol::config_types::Settings;
    use codex_protocol::items::AgentMessageContent;
    use codex_protocol::items::AgentMessageItem;
    use codex_protocol::items::TurnItem;
    use codex_protocol::models::MessagePhase;
    let session = ExtensionData::new("session");
    let turn = ExtensionData::new(id);
    let mode = CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "gpt-5".to_string(),
            reasoning_effort: None,
            developer_instructions: None,
        },
    };
    h.autopilot
        .on_turn_start(TurnStartInput {
            turn_id: turn.level_id(),
            collaboration_mode: &mode,
            token_usage_at_turn_start: None,
            session_store: &session,
            thread_store: &h._store,
            turn_store: &turn,
        })
        .await;
    during();
    for (i, text) in said.iter().enumerate() {
        let phase = if i + 1 == said.len() {
            MessagePhase::FinalAnswer
        } else {
            MessagePhase::Commentary
        };
        let reply = TurnItem::AgentMessage(AgentMessageItem {
            moderation: None,
            id: format!("agent-{i}"),
            content: vec![AgentMessageContent::Text {
                text: (*text).to_string(),
            }],
            phase: Some(phase),
            memory_citation: None,
            delivery: None,
            questions: None,
        });
        h.autopilot
            .on_item_completed(&h._store, &turn, &reply)
            .await;
    }
    h.autopilot
        .on_turn_stop(TurnStopInput {
            session_store: &session,
            thread_store: &h._store,
            turn_store: &turn,
        })
        .await;
}

/// A scratch repository the worker's last command ran in, holding `left.txt` untracked (a
/// person's edit made after the landing).
async fn edited_repo(h: &Harness) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("tmp: {err}"));
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&work).unwrap_or_else(|err| panic!("work: {err}"));
    let init = std::process::Command::new("git")
        .args(["init", "-q", "."])
        .current_dir(&work)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap_or_else(|err| panic!("git init: {err}"));
    assert!(init.status.success(), "{init:?}");
    std::fs::write(work.join("left.txt"), "left\n").unwrap_or_else(|err| panic!("{err}"));
    crate::rest_tests::command_in(&h.autopilot, &h._store, &work.display().to_string()).await;
    (tmp, work)
}

/// The harness's thread landed under Land: the landing's defense its point.
fn landed_here(h: &Harness) {
    use crate::decide::Mode as Word;
    h.rt.with_state(|s| {
        s.task = true;
        s.landed = true;
        s.landing = false;
        s.rejudge = None;
        s.off_landing = false;
        s.mode = Some(Word::Land);
        s.land.typed = true;
        s.point = Some(TurnEnd {
            by_autopilot: true,
            by_person: false,
            ..ended(
                Some("Nothing left: all merged and pushed to origin/main."),
                None,
            )
        });
    });
}

/// THE LANDED TASK'S OWN WAKE IS JUDGED OVER WHAT THE REPOSITORY HELD AS IT BEGAN (aterm main
/// e0da8d68df's `wake_began_over`, through the runtime): the turn's start probe runs before
/// the turn (core awaits `on_turn_start`), the wake ends on quiet words over the person's
/// untracked edit it began over, and its evaluation keeps the landing. NEGATIVE CONTROLS: a
/// wake that adds work of its own (a file written while it runs) reopens the landing, its
/// nudge naming the work; and so does a wake whose earlier message names a need (the
/// runtime hands the decider the turn's earlier words), the need standing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_landed_tasks_own_wake_is_judged_over_the_repository_it_began_over() {
    let h = harness(&[]).await;
    word(&h, "land").await;
    let (_tmp, work) = edited_repo(&h).await;
    let quiet = "The monitor fired: the deploy finished.";
    landed_here(&h);
    wake_turn(&h, "w1", || {}, &[quiet]).await;
    let began = |h: &Harness| {
        h.sup
            .journal()
            .iter()
            .filter(|l| {
                l.contains("the wake's start probe: ")
                    && l.ends_with("/work: 1 untracked (left.txt)")
            })
            .count()
    };
    assert_eq!(began(&h), 1, "{:?}", h.sup.journal());
    assert!(h.rt.state().judging());
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    let s = h.rt.state();
    assert!(s.landed && !s.judging(), "{:?}", h.sup.journal());
    // NEGATIVE CONTROL: the wake's own work.
    landed_here(&h);
    let written = work.join("wake.txt");
    wake_turn(
        &h,
        "w2",
        || std::fs::write(&written, "wake\n").unwrap_or_else(|err| panic!("{err}")),
        &[quiet],
    )
    .await;
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    let s = h.rt.state();
    assert!(!s.landed, "{:?}", h.sup.journal());
    assert!(
        matches!(s.reopened.as_ref(), crate::repo::RepoRead::Read(Some(w)) if w.untracked == 2),
        "{:?}",
        s.reopened
    );
    std::fs::remove_file(&written).unwrap_or_else(|err| panic!("{err}"));
    // NEGATIVE CONTROL: a need said earlier in the wake.
    landed_here(&h);
    wake_turn(
        &h,
        "w3",
        || {},
        &["NEEDS YOU: the signing key password", quiet],
    )
    .await;
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    let s = h.rt.state();
    assert!(!s.landed, "{:?}", h.sup.journal());
    assert_eq!(s.needs.standing, ["the signing key password"]);
}

/// THE UPGRADE WAITS FOR A LANDING'S JUDGEMENT (aterm main e0da8d68df's Off and back, a
/// person's act): a landing an Off stood over owes its judgement over the repository, and
/// the live upgrade must not carry it on the record's word before the probe settled it, so
/// the landed thread reads undecided from the Off (before any look parked it, and once a look
/// under the Off parked it) until an evaluation judged it, though its point was decided
/// before. NEGATIVE CONTROL: with no Off the landed thread holds nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_upgrade_waits_for_a_landings_judgement() {
    let h = harness(&[]).await;
    word(&h, "land").await;
    landed_here(&h);
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert!(h.rt.state().landed);
    // NEGATIVE CONTROL first: no Off.
    assert!(!h.rt.undecided());
    assert_eq!(h.autopilot.upgrade_facts().await.undecided, None);
    h.fake.set_effective(false, "autopilot-off");
    wait_until("released", || !h.sup.acting()).await;
    assert_eq!(
        h.autopilot.upgrade_facts().await.undecided.as_deref(),
        Some(h.rt.key()),
        "an Off no evaluation parked yet"
    );
    // A look under the Off parks the landing, and decides nothing (passive): still owed.
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert!(h.rt.state().judging(), "{:?}", h.sup.journal());
    assert!(h.rt.undecided());
    assert_eq!(
        h.autopilot.upgrade_facts().await.undecided.as_deref(),
        Some(h.rt.key()),
        "parked, not judged"
    );
    let permit = h.rt.hold_eval_when_free().await;
    act_again(&h).await;
    word(&h, "land").await;
    assert_eq!(
        h.autopilot.upgrade_facts().await.undecided.as_deref(),
        Some(h.rt.key())
    );
    let probes = noted(&h, "the repository probe: nothing found");
    drop(permit);
    // An evaluation since the permit came free judged it (the queued look, or this one).
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert!(
        noted(&h, "the repository probe: nothing found") > probes,
        "{:?}",
        h.sup.journal()
    );
    let s = h.rt.state();
    assert!(s.landed && !s.judging(), "{:?}", h.sup.journal());
    assert!(!h.rt.undecided());
    assert_eq!(h.autopilot.upgrade_facts().await.undecided, None);
}

/// AN OFF THAT BEGINS WHILE THE LANDING'S PROBE RUNS SETTLES NOTHING (aterm main
/// e0da8d68df's Off and back): the person's act is judged as it lifts, over the repository as
/// it is then, so a probe that ran across the Off's start is dropped and the landing still
/// owes its judgement (nothing typed, the upgrade still waiting); the next look parks the
/// thread again and judges it. Settled there, a landing judged at the Off's start held over
/// whatever the person then changed under it. NEGATIVE CONTROL: with no Off during it, the
/// probe settles the judgement at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_off_that_begins_while_the_landings_probe_runs_settles_nothing() {
    let h = harness(&[]).await;
    word(&h, "land").await;
    let judging = |h: &Harness| {
        landed_here(h);
        h.rt.with_state(crate::decide::ThreadState::park_off);
        assert!(h.rt.state().judging());
    };
    judging(&h);
    let sup = Arc::clone(&h.sup);
    h.rt.after_probe(move || sup.count_off());
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert_eq!(
        noted(
            &h,
            "an Off came during the landing's probe; judged as it lifts"
        ),
        1,
        "{:?}",
        h.sup.journal()
    );
    assert!(h.rt.state().judging());
    assert!(h.rt.undecided());
    // The next look parks it again and judges it.
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    let s = h.rt.state();
    assert!(s.landed && !s.judging(), "{:?}", h.sup.journal());
    // NEGATIVE CONTROL: no Off during the probe.
    judging(&h);
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    let s = h.rt.state();
    assert!(s.landed && !s.judging(), "{:?}", h.sup.journal());
    assert_eq!(
        noted(
            &h,
            "an Off came during the landing's probe; judged as it lifts"
        ),
        1
    );

    // An Off also invalidates an unlanded point's cached repo probe (`park_off`), and an Off or
    // non-probe new point during a turn-end probe drops that probe without acting on the new
    // point.
    word(&h, "cruise").await;
    h.rt.with_state(|state| {
        state.observe_person_turn();
        let end = ended(Some("All done."), None);
        let at = end.at;
        state.observe_turn_end(end, &Timing::default());
        state.probed_for(Some(at), None);
        assert!(state.repo.is_some());
        state.park_off();
        assert!(
            state.repo.is_none(),
            "park_off invalidates unlanded repo probe too"
        );
    });
    let sup = Arc::clone(&h.sup);
    h.rt.after_probe(move || sup.count_off());
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert!(
        h.rt.state().repo.is_none(),
        "an Off during a turn-end probe drops the finding"
    );
    let rt = Arc::clone(&h.rt);
    h.rt.after_probe(move || {
        rt.with_state(|state| {
            state.observe_turn_end(ended(Some("Still working."), None), &Timing::default());
        });
    });
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert_eq!(
        noted(&h, "a new point came during the probe"),
        1,
        "a non-probe point arriving during a probe is not hijacked: {:?}",
        h.sup.journal()
    );

    // Even if another hook already synced `rt.off_seen` to `sup.offs()` during the probe, or a
    // new turn was accepted while `point_at()` still names the old point, the in-flight probe
    // settles nothing.
    word(&h, "land").await;
    judging(&h);
    let sup = Arc::clone(&h.sup);
    let rt = Arc::clone(&h.rt);
    h.rt.after_probe(move || {
        sup.count_off();
        rt.sync_off_seen(sup.offs());
    });
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert!(
        h.rt.state().judging(),
        "an Off during the probe still settles nothing even after off_seen is synced"
    );
    assert_eq!(
        noted(
            &h,
            "an Off came during the landing's probe; judged as it lifts"
        ),
        2
    );

    judging(&h);
    let rt = Arc::clone(&h.rt);
    h.rt.after_probe(move || {
        rt.note_accepted_turn();
    });
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert!(
        h.rt.state().judging(),
        "a turn accepted during the probe must not let the stale probe settle judging"
    );
    assert_eq!(noted(&h, "a new point came during the probe"), 2);
}

/// A HOLD'S LIFT MET MID-CHECK (parity review P1): an act's `status` read met `hold=1` while
/// an `effective=on` read came (the lift between the two, which the read could not credit to
/// a hold no one had met yet): the checks look again at once, not at their recheck. And the
/// hold the checks met is one the next `effective=on` read finds lifted. NEGATIVE CONTROLS: no
/// read since the checks began is no lift met mid-check, and a read with no hold met or read
/// since the last one finds none lifted.
#[test]
fn a_hold_lift_met_mid_check_is_looked_at_again_at_once() {
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(|| None),
        client: ClientOptions::default(),
    });
    let begun = sup.on_reads();
    assert!(!sup.read_on(1), "no hold met or read yet: none lifted");
    assert!(sup.met_hold(begun), "the lift came while status was read");
    // NEGATIVE CONTROL: checks begun after the last read.
    let _ = sup.read_on(2);
    let begun = sup.on_reads();
    assert!(!sup.met_hold(begun), "no read since the checks began");
    assert!(sup.read_on(3), "the hold the checks met is found lifted");
    assert!(!sup.read_on(4), "nothing met or read since");
}

/// A HOLD ONLY `status` SHOWED IS LOOKED PAST AT ITS LIFT (parity review P1): the checks met
/// `status hold=1` though no policy read said `why=hold` (the hold's own read came after it
/// lifted, or never came), and the `hold 0` event's read, `effective=on` as before, has the
/// thread look again at once. The wait's own recheck is an hour here, so only the lift ends it
/// within the hang detector. NEGATIVE CONTROL: no thread looks again while the hold stands.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hold_only_status_showed_is_looked_past_at_its_lift() {
    let h = harness(&[]).await;
    h.sup.set_recheck(Duration::from_secs(3600));
    point(&h, ended(Some("Stage 1 is in; stage 2 is next."), None));
    assert!(
        matches!(decided_now(&h), crate::decide::Act::Type { .. }),
        "{:?}",
        decided_now(&h)
    );
    h.fake.set_status("hold", "1");
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert_eq!(
        noted(&h, "not now (aterm says not now: Hold)"),
        1,
        "{:?}",
        h.sup.journal()
    );
    let looked = h.rt.evaluations();
    // NEGATIVE CONTROL: the hold stands.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(h.rt.evaluations(), looked, "{:?}", h.sup.journal());
    h.fake.set_status("hold", "0");
    h.fake.emit("hold 0 reason=owner origin=cli");
    wait_until("the thread looks again at the lift", || {
        h.rt.evaluations() > looked
    })
    .await;
    assert_eq!(
        noted(&h, "the hold lifted, the threads look again"),
        1,
        "{:?}",
        h.sup.journal()
    );
}

/// THE UPGRADE THAT MET A HOLD LOOKS AGAIN AT ITS LIFT (parity review R2): the live upgrade's
/// before-every-act checks met `status hold=1`, and the `hold 0` event's `effective=on` read
/// that finds the hold lifted moves the claims watch, which wakes the upgrade's look again as
/// it has every thread look (648d827a8), not the wait's own recheck (an hour here, so only the
/// lift ends it within the hang detector). NEGATIVE CONTROL: the look again is pending while
/// the hold stands.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_upgrade_that_met_a_hold_looks_again_at_its_lift() {
    let h = harness(&[]).await;
    h.sup.set_recheck(Duration::from_secs(3600));
    h.autopilot
        .set_upgrade_facts(Some(crate::upgrade::UpgradeFacts {
            enabled: true,
            other_build: true,
            unconfirmed: None,
            unresumable: None,
            threads: Vec::new(),
            waits_pending: false,
            carried: 0,
            undecided: None,
            left_to_person: None,
        }));
    h.fake.set_status("hold", "1");
    tokio::time::timeout(Duration::from_secs(60), async {
        let deciding = format!("upgrade waits: {} is deciding", h.rt.key());
        let deciding_count = || {
            h.sup
                .journal()
                .iter()
                .filter(|line| line.as_str() == deciding.as_str())
                .count()
        };
        loop {
            let before = deciding_count();
            assert!(!h.autopilot.try_upgrade().await);
            if noted(&h, "upgrade not now (aterm says not now: Hold)") == 1 {
                break;
            }
            assert_eq!(
                deciding_count(),
                before + 1,
                "only an initial claim evaluation may defer this hold check: {:?}",
                h.sup.journal()
            );
            // A busy attempt armed its own short relook. Let that real task finish before
            // the first Hold read arms the hour-long wait whose lift this test exercises.
            while h.autopilot.upgrade_relook_pending() {
                tokio::task::yield_now().await;
            }
        }
    })
    .await
    .expect("the initial claim evaluation and preliminary upgrade relook drain");
    assert_eq!(
        noted(&h, "upgrade not now (aterm says not now: Hold)"),
        1,
        "{:?}",
        h.sup.journal()
    );
    // NEGATIVE CONTROL: the hold stands, and the look again waits.
    assert!(h.autopilot.upgrade_relook_pending());
    h.fake.set_status("hold", "0");
    h.fake.emit("hold 0 reason=owner origin=cli");
    wait_until("the upgrade looks again at the lift", || {
        !h.autopilot.upgrade_relook_pending()
    })
    .await;
    assert_eq!(
        noted(&h, "the hold lifted, the threads look again"),
        1,
        "{:?}",
        h.sup.journal()
    );
}

/// A HOLD WHOSE LIFT NEVER REACHED codex-alab IS READ AGAIN AT aterm'S PARK STEP (parity
/// review R2): with no `hold` event and no `GAP`, nothing tells the acts the hold lifted, so
/// their backstop is the next `status` read, [`crate::supervisor::HOLD_RECHECK`] (aterm's
/// `WAIT_STEP`, the longest its own `park_on_hold` waits between reads), not the minute every
/// other wait with no deadline of its own takes. NEGATIVE CONTROL: another such wait (unread
/// input) still looks again at [`crate::supervisor::RECHECK`].
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_holds_wait_reads_status_again_at_aterms_park_step() {
    use crate::supervisor::HOLD_RECHECK;
    use crate::supervisor::RECHECK;
    let h = harness(&[]).await;
    waits_for(&h, ("hold", "1"), HOLD_RECHECK).await;
    h.fake.set_status("hold", "0");
    // NEGATIVE CONTROL: unread input waits the minute.
    waits_for(&h, ("input", "1"), RECHECK).await;
}

/// The before-every-act checks, with `status` showing `field=raw`, wait `want` from the read.
async fn waits_for(h: &Harness, (field, raw): (&str, &str), want: Duration) {
    use crate::supervisor::clock;
    h.fake.set_status(field, raw);
    let before = clock();
    let ready = h.sup.ready().await;
    let after = clock();
    let Ready::Wait { until, why } = ready else {
        panic!("{field}={raw}: {ready:?}");
    };
    assert!(
        before + want <= until && until <= after + want,
        "{field}={raw} ({why}): {:?} after the read",
        until.saturating_duration_since(before)
    );
}

/// A NETWORK WALL'S WAIT IS MEASURED ONLY WHILE CODEX-ALAB WAITS ON IT (parity-3 review, F6,
/// F7; aterm renews a route's lease only while its loop acts and asks, so its probes stop a
/// lease after a passive spell begins): a thread at a network wall, its route direct (a
/// loopback port nothing listens on) and its look again pending, stands at that route while
/// codex-alab acts with retries on, and its lease lives through the probes; Off, it stands at
/// nothing and its lease lapses; acting again, it stands there again. NEGATIVE CONTROLS:
/// retries off stands at nothing, and so does a point with no look pending (nothing decided to
/// wait on it), however codex-alab acts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_network_walls_wait_is_measured_only_while_codex_alab_waits_on_it() {
    use crate::netwatch::Dest;
    use crate::netwatch::NetTiming;
    use crate::netwatch::Route;
    use crate::netwatch::Waiter;
    let h = harness(&[]).await;
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .unwrap_or_else(|err| panic!("a loopback port: {err}"))
        .port();
    let dest = Dest {
        host: "127.0.0.1".to_string(),
        port,
    };
    let pause = Duration::from_millis(50);
    // A closed port refuses the connect, so no handshake ever asks for the trust.
    assert!(h.autopilot.set_net_probe(
        Arc::new(|| Err(std::io::Error::other("no handshake on a closed port"))),
        NetTiming {
            lease: Duration::from_millis(300),
            down_cadence: [pause; 3],
            steady: pause,
            ..NetTiming::default()
        },
    ));
    point(
        &h,
        ended(
            None,
            Some(Wall::Network {
                what: "an API error (the API could not be reached)".to_string(),
            }),
        ),
    );
    h.rt.set_route(Some(Route::Direct(dest.clone())));
    let later = || Instant::now() + Duration::from_secs(3600);
    h.autopilot.schedule(&h.rt, later());
    assert_eq!(
        h.rt.waits_for(),
        Some(dest.clone()),
        "acting: it waits there"
    );
    let _ = h.autopilot.reach_for(&h.rt);
    // Past the lease (each probe at least `pause` after the last): it lives while it waits.
    let net = h.autopilot.net();
    let probes = net.probes();
    wait_until("probes run past the lease", || net.probes() >= probes + 12).await;
    assert_eq!(net.leases(&dest), 1, "renewed while it waits");
    h.fake.set_effective(false, "autopilot-off");
    wait_until("passive", || !h.sup.acting()).await;
    assert_eq!(h.rt.waits_for(), None, "Off: codex-alab waits on nothing");
    wait_until("the lease lapses", || net.leases(&dest) == 0).await;
    act_again(&h).await;
    h.autopilot.schedule(&h.rt, later());
    assert_eq!(h.rt.waits_for(), Some(dest.clone()), "acting again");
    // NEGATIVE CONTROLS: no look pending; retries off. An evaluation a claim change asked for
    // (`watch_claims`) may still be in flight, holding the thread's permit or arming its look
    // after a cancel (parity-6 review R14: under load it was, and a single cancel read `Some`):
    // cancel until nothing waits on the point, a one-minute hang detector bounding it.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        Autopilot::cancel_timer(&h.rt).await;
        if h.rt.waits_for().is_none() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "nothing waits on the point"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    h.autopilot.schedule(&h.rt, later());
    h.fake.set_policy_key("retry_api_errors", "false");
    wait_until("retries off", || {
        h.sup.policy().is_some_and(|p| !p.retry_api_errors)
    })
    .await;
    assert_eq!(h.rt.waits_for(), None, "retries off");
}

/// Journal lines that hold every one of `needles`.
fn noted_all(h: &Harness, needles: &[&str]) -> usize {
    h.sup
        .journal()
        .iter()
        .filter(|l| needles.iter().all(|needle| l.contains(needle)))
        .count()
}

/// A DUE USAGE RESUME WAITS FOR THE API AND FOR THE HOST TO SETTLE (2026-10-24: a resume typed
/// inside a 19 s dark wake with the network down sat in backoff across the sleeps after it, and
/// Esc was the only way on). A thread at a usage wall whose reset has passed, its route direct
/// (a loopback port nothing listens on): the due resume asks for the API's reach and holds for
/// the first measure; the measure (the connect refused: down) holds it, nothing typed, its wait
/// pending and the route still measured. A wake then (two hours asleep): the point's ask is
/// cleared, so the wake's network is measured again, and the resume waits for the host to
/// settle. NEGATIVE CONTROL: a route the probe cannot reproduce is never asked for, and the
/// resume is not held.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_due_usage_resume_waits_for_the_api_and_for_the_host_to_settle() {
    use crate::netwatch::Dest;
    use crate::netwatch::NetTiming;
    use crate::netwatch::Reach;
    use crate::netwatch::Route;
    use crate::netwatch::Waiter;
    use codex_alab_core::wake::Reading;
    use codex_alab_core::wake::Wake;
    let h = harness(&[("limit_wait", "true")]).await;
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .unwrap_or_else(|err| panic!("a loopback port: {err}"))
        .port();
    let dest = Dest {
        host: "127.0.0.1".to_string(),
        port,
    };
    let pause = Duration::from_millis(50);
    assert!(h.autopilot.set_net_probe(
        Arc::new(|| Err(std::io::Error::other("no handshake on a closed port"))),
        NetTiming {
            down_cadence: [pause; 3],
            steady: pause,
            ..NetTiming::default()
        },
    ));
    let base = Duration::from_secs(1000);
    let reading = Arc::new(std::sync::Mutex::new(Reading {
        asleep: base,
        last: None,
    }));
    let read = Arc::clone(&reading);
    h.autopilot.set_wake_read(Arc::new(move || {
        Some(
            *read
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }));
    let usage = || Wall::Usage {
        reset_at: Some(Instant::now() - Duration::from_secs(600)),
        reset_label: Some("12:00".to_string()),
    };
    point(&h, ended(None, Some(usage())));
    h.rt.set_route(Some(Route::Direct(dest.clone())));
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert!(
        h.rt.state().net_asked.is_some(),
        "the due resume asked for the reach: {:?}",
        h.sup.journal()
    );
    assert!(h.rt.timer_pending(), "and waits for its first measure");
    assert_eq!(
        noted_all(&h, &["usage-resume@v1", "measuring the API's reach"]),
        1
    );
    wait_until("the API measured down holds the resume", || {
        matches!(h.rt.state().reach, Reach::Down { .. })
            && noted_all(&h, &["usage-resume@v1", "measured unreachable"]) > 0
    })
    .await;
    assert!(h.rt.timer_pending(), "held, its wait pending");
    assert_eq!(
        h.rt.waits_for(),
        Some(dest.clone()),
        "the route is still measured"
    );
    assert!(
        !h.fake.acts().iter().any(|act| act == "usage-resume@v1"),
        "nothing typed into an API measured down: {:?}",
        h.fake.acts()
    );
    // A wake: two hours asleep. The ask is cleared and the resume settles first.
    let slept = Duration::from_secs(7200);
    *reading
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Reading {
        asleep: base + slept,
        last: Some(Wake {
            at: Instant::now(),
            slept,
            seq: 1,
        }),
    };
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert_eq!(
        noted(&h, "the host slept 7200 s"),
        1,
        "{:?}",
        h.sup.journal()
    );
    assert_eq!(
        h.rt.state().net_asked,
        None,
        "the wake's network is measured again"
    );
    assert_eq!(h.rt.waits_for(), None, "not while the host settles");
    assert_eq!(
        noted_all(&h, &["usage-resume@v1", "the host just woke"]),
        1,
        "{:?}",
        h.sup.journal()
    );
    assert!(h.rt.timer_pending(), "the settle's wait pending");
    assert!(!h.fake.acts().iter().any(|act| act == "usage-resume@v1"));
    // NEGATIVE CONTROL: a route the probe cannot reproduce, and no wake.
    *reading
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Reading {
        asleep: base + slept,
        last: None,
    };
    point(&h, ended(None, Some(usage())));
    h.rt.set_route(Some(Route::Custom("a proxy".to_string())));
    let waits = noted_all(&h, &["wait ", "usage-resume@v1"]);
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert_eq!(h.rt.state().net_asked, None, "never asked for");
    assert_eq!(
        noted_all(&h, &["wait ", "usage-resume@v1"]),
        waits,
        "not held: {:?}",
        h.sup.journal()
    );
}

/// aterm'S `probe_api` OFF MEASURES NOTHING (gap 30; aterm's host asks its probe only with the
/// switch on): a thread at a network wall whose route the probe reproduces (a loopback port
/// nothing listens on), its look again pending, stands at no route; its reach reads unknown, no
/// destination's task starts and no probe runs, and its wall waits the network ladder as an
/// unmeasured route's does. A due usage resume at that route is never asked for and
/// not held. NEGATIVE CONTROL: the switch on, the same thread stands at that route, its probe
/// runs, and the outage it measures holds the wall (today's behaviour). Off again while the
/// lease it took lives, the thread stands at nothing, and until that lease lapses no probe
/// runs but one already past its check when the switch was read.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn aterms_probe_api_off_measures_nothing_and_the_walls_keep_the_ladder() {
    use crate::netwatch::Dest;
    use crate::netwatch::NetTiming;
    use crate::netwatch::Reach;
    use crate::netwatch::Route;
    use crate::netwatch::Waiter;
    use codex_alab_core::wake::Reading;
    let h = harness(&[("probe_api", "false"), ("limit_wait", "true")]).await;
    assert!(
        h.sup.policy().is_some_and(|p| !p.probe_api),
        "the switch is read off"
    );
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .unwrap_or_else(|err| panic!("a loopback port: {err}"))
        .port();
    let dest = Dest {
        host: "127.0.0.1".to_string(),
        port,
    };
    let pause = Duration::from_millis(50);
    assert!(h.autopilot.set_net_probe(
        Arc::new(|| Err(std::io::Error::other("no handshake on a closed port"))),
        NetTiming {
            lease: Duration::from_millis(300),
            down_cadence: [pause; 3],
            steady: pause,
            ..NetTiming::default()
        },
    ));
    // No wake: the host's own sleeps hold nothing here.
    h.autopilot.set_wake_read(Arc::new(|| {
        Some(Reading {
            asleep: Duration::ZERO,
            last: None,
        })
    }));
    let network = || {
        ended(
            None,
            Some(Wall::Network {
                what: "an API error (the API could not be reached)".to_string(),
            }),
        )
    };
    point(&h, network());
    h.rt.set_route(Some(Route::Direct(dest.clone())));
    let later = || Instant::now() + Duration::from_secs(3600);
    h.autopilot.schedule(&h.rt, later());
    assert_eq!(h.rt.waits_for(), None, "off: it stands at no route");
    assert_eq!(h.autopilot.reach_for(&h.rt), Reach::Unknown);
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert_eq!(
        noted_all(
            &h,
            &[
                "s for api-retry@v1",
                "(the API could not be reached); retrying"
            ]
        ),
        1,
        "the ladder: {:?}",
        h.sup.journal()
    );
    let net = h.autopilot.net();
    assert_eq!((net.watched(), net.probes()), (0, 0), "no task, no probe");
    let usage = Wall::Usage {
        reset_at: Some(Instant::now() - Duration::from_secs(600)),
        reset_label: Some("12:00".to_string()),
    };
    point(&h, ended(None, Some(usage)));
    h.rt.set_route(Some(Route::Direct(dest.clone())));
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert_eq!(h.rt.state().net_asked, None, "the resume never asks");
    assert_eq!(
        noted_all(&h, &["wait ", "usage-resume@v1"]),
        0,
        "nor is it held: {:?}",
        h.sup.journal()
    );
    assert_eq!((net.watched(), net.probes()), (0, 0), "still no probe");
    // NEGATIVE CONTROL: the switch on.
    h.fake.set_policy_key("probe_api", "true");
    wait_until("the switch on", || {
        h.sup.policy().is_some_and(|p| p.probe_api)
    })
    .await;
    point(&h, network());
    h.rt.set_route(Some(Route::Direct(dest.clone())));
    h.autopilot.schedule(&h.rt, later());
    assert_eq!(h.rt.waits_for(), Some(dest.clone()), "on: it stands there");
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    wait_until("the probe runs", || net.probes() > 0).await;
    assert_eq!(net.watched(), 1);
    wait_until("the measured outage holds the wall", || {
        noted(
            &h,
            "the API is unreachable; continuing when it is reachable",
        ) > 0
    })
    .await;
    // Off again, its lease living: the next probes run no more.
    h.fake.set_policy_key("probe_api", "false");
    wait_until("the switch off again", || {
        h.sup.policy().is_some_and(|p| !p.probe_api)
    })
    .await;
    let probes = net.probes();
    assert_eq!(h.rt.waits_for(), None, "off: it stands at nothing");
    wait_until("the lease lapses", || net.leases(&dest) == 0).await;
    assert!(
        net.probes() <= probes + 1,
        "{} probes after the switch went off",
        net.probes() - probes
    );
}

/// An old plan's outcome records the applied answer once, but loses authority to
/// clear a same-ID replacement's newer reason when its original runtime stops.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retired_plans_outcome_preserves_the_new_plan_and_its_original_act() {
    use codex_extension_api::ThreadLifecycleContributor;
    use codex_extension_api::ThreadStopInput;
    for already_applied in [false, true] {
        let h = harness(&[("human_grace_s", "1")]).await;
        h.sup
            .attention_for(h.rt.key(), "plan", Some("plan: old runtime".into()))
            .await;
        h.shared.report_box(plan_box());
        wait_until("old plan answer is posted", || {
            h.shared.box_answer_pending()
        })
        .await;
        let glue = || {
            h.shared.take_box_answer("plan-1").expect("old plan answer");
            h.shared.close_box("plan-1");
            h.shared.set_box_outcome(BoxOutcome {
                id: "plan-1".into(),
                applied: Ok(()),
            });
        };
        let gated = h.fake.gated_requests().len();
        if already_applied {
            h.fake.gate("meta set harness_act");
            glue();
            wait_until(
                "the applied old answer's act publication is in flight",
                || h.fake.gated_requests().len() > gated,
            )
            .await;
        }
        h.autopilot
            .on_thread_stop(ThreadStopInput {
                session_store: &ExtensionData::new("session"),
                thread_store: &h._store,
            })
            .await;
        let client =
            h.sup
                .want_attention_for(h.rt.key(), "plan", Some("plan: replacement runtime".into()));
        if already_applied {
            h.fake.open_gate("meta set harness_act");
        } else {
            glue();
        }
        h.sup.publish_attention(client).await;
        assert!(
            h.fake
                .wait_for(|fake| fake
                    .acts()
                    .iter()
                    .filter(|rule| *rule == "plan-approve@v2")
                    .count()
                    == 1)
                .await
        );
        assert_eq!(
            h.sup.standing_attention().as_deref(),
            Some("plan: replacement runtime")
        );
        assert_eq!(shown(&h).as_deref(), Some("plan: replacement runtime"));
        // A duplicate outcome has no Posted left to publish again.
        h.shared.set_box_outcome(BoxOutcome {
            id: "plan-1".into(),
            applied: Ok(()),
        });
        wait_until("the duplicate outcome is seen without a second act", || {
            h.sup
                .journal()
                .iter()
                .any(|line| line.contains("box plan-1 took an answer"))
        })
        .await;
        assert_eq!(
            h.fake
                .acts()
                .iter()
                .filter(|rule| *rule == "plan-approve@v2")
                .count(),
            1
        );
    }
}

/// Repeated Cruise reports schedule recovery without asking for attention.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_repeated_cruise_point_schedules_recovery_without_an_alert() {
    use crate::replies::Kept;
    use crate::replies::Replies;

    let h = started_with_claim_watch(&[], |_, _| {}, false).await;
    wait_until("the claim is proven and acted under", || h.sup.acting()).await;
    h.rt.with_state(|state| {
        let point = TurnEnd {
            by_autopilot: true,
            by_person: false,
            worked: Duration::from_secs(30),
            restored: true,
            ..ended(Some("Still on the parser."), None)
        };
        state.observe_person_turn();
        state.seed_replies(
            Replies::of(&[Kept {
                mark: crate::replies::mark_of(&point),
                answers: 2,
                walled: false,
                tries: 0,
            }]),
            Some(point.at),
        );
        state.observe_turn_end(point, &Timing::default());
    });
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert!(h.rt.timer_pending(), "recovery is paced");

    assert!(h.fake.attention().is_empty());
    assert!(
        h.fake.acts().is_empty(),
        "nothing submitted before recovery is due"
    );
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    assert!(
        h.rt.timer_pending(),
        "another observation keeps recovery scheduled"
    );
}
