// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! codex-alab's automatic waits in aterm's Waiting rows (amended 2026-10-27 after the parity
//! sweep, gap 2: a codex tab waiting out a usage limit or an outage looked idle or stuck),
//! against alab-aterm's in-process FAKE aterm (never a real one). A decided wait a person
//! should see goes up as `meta set attention owner=codex-alab-wait kind=wait <words>` once its
//! timer is armed, never under `owner=codex-alab` (the asks' slot) and never with no kind (an
//! ask); a turn's start, a decision that waits for nothing, an act that did not go, a stopped
//! thread, an Off, a standby, a lost claim and the exit take it down; a not-now re-arm and a
//! person's hold keep it; a new claim settles what an earlier image left.

use super::*;
use crate::words;

const WAIT: &str = "codex-alab-wait";
const RESET: &str = "waiting for the usage reset at 15:00";

/// The policy that waits a usage limit out (`limit_wait`; the fake's own policy leaves it off).
const WAITS_LIMITS: &[(&str, &str)] = &[("limit_wait", "true")];

fn wait_up(h: &Harness) -> Option<String> {
    h.fake.attention().get(WAIT).cloned()
}

/// The wait entry's writes so far: its sets, and its clears.
fn wait_writes(h: &Harness) -> (usize, usize) {
    let requests = h.fake.requests();
    let sets = requests
        .iter()
        .filter(|r| r.contains(" meta set attention owner=codex-alab-wait "))
        .count();
    let clears = requests
        .iter()
        .filter(|r| r.ends_with(" meta unset attention owner=codex-alab-wait"))
        .count();
    (sets, clears)
}

/// A usage wall whose reset comes `ahead` from now, or (`None`) came ten minutes ago.
fn usage(ahead: Option<Duration>) -> Wall {
    Wall::Usage {
        reset_at: Some(match ahead {
            Some(ahead) => Instant::now() + ahead,
            None => Instant::now() - Duration::from_secs(600),
        }),
        reset_label: Some("15:00".to_string()),
    }
}

/// The usage reset an hour off: its wait goes up.
async fn raise_reset_wait(h: &Harness) {
    point(h, ended(None, Some(usage(Some(Duration::from_secs(3600))))));
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
    wait_until("the reset's wait is shown", || {
        wait_up(h).as_deref() == Some(RESET)
    })
    .await;
}

async fn evaluate(h: &Harness) {
    Arc::clone(&h.autopilot).evaluate(Arc::clone(&h.rt)).await;
}

/// A WAIT GOES UP AS A WAIT, AND THE ACT'S TURN TAKES IT DOWN: a usage reset an hour off is
/// the exact `owner=codex-alab-wait kind=wait` set, never the asks' `owner=codex-alab` slot,
/// and the turn that starts (codex-alab's typed act, through the same native hook a person's
/// turn starts by) takes it down. NEGATIVE CONTROL: a resume due now waits for nothing and
/// sends no set.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_waits_words_go_up_as_a_wait_and_the_acts_turn_takes_them_down() {
    let h = harness(WAITS_LIMITS).await;
    // NEGATIVE CONTROL first, before any turn runs.
    point(&h, ended(None, Some(usage(None))));
    evaluate(&h).await;
    assert_eq!(wait_writes(&h).0, 0, "{:?}", h.fake.requests());
    assert_eq!(wait_up(&h), None);
    raise_reset_wait(&h).await;
    assert!(h.rt.timer_pending());
    assert!(
        h.fake.requests().contains(&format!(
            "@{} meta set attention owner=codex-alab-wait kind=wait {RESET}",
            h.fake.sid()
        )),
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(
        h.fake.attention_kinds().get(WAIT).map(String::as_str),
        Some("wait")
    );
    assert_eq!(shown(&h), None, "never in the asks' slot");
    let clears = wait_writes(&h).1;
    let session = ExtensionData::new("session");
    let turn = ExtensionData::new("the-acts-turn");
    start_native_turn(&h, &session, &turn).await;
    assert!(!h.rt.timer_pending());
    // Its write goes on a task of its own: core awaits the hook before the turn runs.
    wait_until("the turn's start takes the wait down", || {
        wait_up(&h).is_none()
    })
    .await;
    assert_eq!(wait_writes(&h).1, clears + 1);
}

/// A TURN'S START NEVER WAITS ON ATERM FOR ITS WAIT (review of f134d2b2cb, finding 4: core
/// awaits the hook before the turn runs, and the clear was written inside it): with the
/// clear held at aterm, the start is done all the same, and the clear lands once aterm
/// answers. NEGATIVE CONTROL: until then the wait still stands at aterm.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turns_start_never_waits_on_aterm_for_its_wait() {
    const CLEAR: &str = "meta unset attention owner=codex-alab-wait";
    // Each call answered within two minutes, so a start that waits on the held clear outlasts
    // the hang detector.
    let h = harness_within(WAITS_LIMITS, Duration::from_secs(120)).await;
    raise_reset_wait(&h).await;
    h.fake.gate(CLEAR);
    let session = ExtensionData::new("session");
    let turn = ExtensionData::new("the-turn");
    tokio::time::timeout(
        Duration::from_secs(60),
        start_native_turn(&h, &session, &turn),
    )
    .await
    .unwrap_or_else(|_| panic!("the start waited on aterm: {:?}", h.sup.journal()));
    wait_until("the clear reaches aterm", || {
        h.fake.gated_requests().iter().any(|r| r.ends_with(CLEAR))
    })
    .await;
    // NEGATIVE CONTROL.
    assert_eq!(wait_up(&h).as_deref(), Some(RESET));
    h.fake.open_gate(CLEAR);
    wait_until("the clear lands", || wait_up(&h).is_none()).await;
}

/// A PERSON'S TURN TAKES THE WAIT DOWN; A NOT-NOW KEEPS IT: the continuation at a person's
/// point waits for the host that just woke to settle, and its wait shows; at the settle's end
/// a draft holds the act back (a not-now re-arm), and the wait still shows, as the act still
/// waits; the person's turn that starts takes it down. NEGATIVE CONTROL: the draft's defer
/// sends no write of the wait.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_persons_turn_takes_the_wait_down_and_a_not_now_keeps_it() {
    use codex_alab_core::wake::Reading;
    use codex_alab_core::wake::Wake;
    let h = harness(WAITS_LIMITS).await;
    let reading = Arc::new(std::sync::Mutex::new(Reading {
        asleep: Duration::from_secs(1000),
        last: Some(Wake {
            at: Instant::now(),
            slept: Duration::from_secs(60),
            seq: 1,
        }),
    }));
    let read = Arc::clone(&reading);
    h.autopilot.set_wake_read(Arc::new(move || {
        Some(
            *read
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }));
    point(&h, ended(Some("Stage 1 is in; stage 2 is next."), None));
    evaluate(&h).await;
    let settle = wait_up(&h).unwrap_or_else(|| panic!("the settle's wait: {:?}", h.sup.journal()));
    assert!(settle.starts_with("the host just woke"), "{settle}");
    // The settle ends (the timer's look, here at once) under a person's draft.
    reading
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .last = None;
    let key = h._store.level_id().to_string();
    h.shared.observe(
        &Frame {
            primary_thread: Some(&key),
            displayed_thread: Some(&key),
            rollout_path: None,
            cwd: std::path::Path::new("/work"),
            model: "gpt-5",
            codex_home: h._dir.path(),
            embedded: true,
            state: ProgramState::Idle,
            draft_empty: false,
            queued_input: false,
            popup_open: false,
            editor_open: false,
            pending_user_turn: false,
        },
        now_ms(),
    );
    let writes = wait_writes(&h);
    evaluate(&h).await;
    assert_eq!(noted(&h, "not now (a draft"), 1, "{:?}", h.sup.journal());
    // A sync now shows what the defer left wanted (review of f134d2b2cb, finding 5: unsynced,
    // a defer that took the wait down locally left the fake's entry standing all the same).
    h.sup.show_waits().await;
    assert_eq!(wait_up(&h), Some(settle), "the act still waits");
    assert_eq!(wait_writes(&h), writes, "the defer writes nothing");
    let session = ExtensionData::new("session");
    let turn = ExtensionData::new("the-persons-turn");
    start_native_turn(&h, &session, &turn).await;
    wait_until("the person's turn takes the wait down", || {
        wait_up(&h).is_none()
    })
    .await;
}

/// AN OFF AND A STANDBY TAKE THE WAIT DOWN; A HOLD KEEPS IT: a person's or the config's off
/// (`release_for_off`) and a suspension (`stand_by`) each leave no wait standing, as nothing
/// types when it ends. NEGATIVE CONTROL: a person's hold keeps the claim and the wait (its act
/// waits on the hold).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_off_or_a_standby_takes_the_wait_down_and_a_hold_keeps_it() {
    let h = harness(WAITS_LIMITS).await;
    for off in ["autopilot-off", "suspended"] {
        raise_reset_wait(&h).await;
        h.fake.set_effective(false, off);
        wait_until("the off takes the wait down", || {
            wait_up(&h).is_none() && !h.sup.acting()
        })
        .await;
        if off == "autopilot-off" {
            wait_until("released", || h.fake.claim().is_none()).await;
        }
        act_again(&h).await;
    }
    raise_reset_wait(&h).await;
    hold(&h).await;
    assert!(h.sup.held(), "the hold keeps the claim");
    assert_eq!(wait_up(&h).as_deref(), Some(RESET), "{:?}", h.sup.journal());
}

/// THE EXIT TAKES DOWN CODEX-ALAB'S OWN ENTRY, THE NEEDS AND THE WAIT: nobody is left to type
/// when the wait ends. NEGATIVE CONTROL: another owner's entry stays, and all three stood
/// before the exit.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_exit_takes_its_entry_the_needs_and_the_wait_down() {
    let h = harness(WAITS_LIMITS).await;
    h.fake.set_attention_as("person", "keep this note");
    h.sup
        .attention("api", Some("api: a refusal left to a person".into()))
        .await;
    h.sup.needs(&["the signing key".to_string()], 7).await;
    raise_reset_wait(&h).await;
    assert!(shown(&h).is_some() && needs_up(&h).is_some());
    let shared = h.shared;
    let released =
        tokio::task::spawn_blocking(move || shared.release_claim(Duration::from_secs(60)))
            .await
            .unwrap_or_else(|err| panic!("the release: {err}"));
    assert!(released, "{:?}", h.sup.journal());
    assert_eq!(
        (shown(&h), needs_up(&h), wait_up(&h)),
        (None, None, None),
        "{:?}",
        h.fake.requests()
    );
    assert_eq!(
        h.fake.attention().get("person").map(String::as_str),
        Some("keep this note")
    );
}

/// A NEW CLAIM SETTLES A WAIT AN EARLIER IMAGE LEFT (a SIGKILL never ran its exit): the first
/// sync under the proven claim takes it down, though this process never showed one; another
/// owner's entry stays. NEGATIVE CONTROL: once settled, with no wait standing, a sync sends
/// nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_claim_settles_a_wait_an_earlier_image_left() {
    let h = started(&[], |fake, _| {
        fake.set_attention_as(WAIT, "waiting for the usage reset at 03:00");
        fake.set_attention_as("person", "keep this note");
    })
    .await;
    wait_until("the fresh claim is proven", || h.sup.acting()).await;
    wait_until("the leftover wait is settled", || wait_up(&h).is_none()).await;
    assert_eq!(
        h.fake.attention().get("person").map(String::as_str),
        Some("keep this note")
    );
    // NEGATIVE CONTROL.
    let writes = wait_writes(&h);
    h.sup.show_waits().await;
    h.sup.attention("wall", None).await;
    assert_eq!(wait_writes(&h), writes, "{:?}", h.fake.requests());
}

/// A WAIT NEVER GOES IN THE ASKS' SLOT, AND AN ASK STILL DOES: no request writes
/// `owner=codex-alab` with a wait's words, and every write of the wait owner says
/// `kind=wait`. NEGATIVE CONTROL: the login wall that comes next is an ask, under
/// `owner=codex-alab` with no kind, and its attention takes the wait down (a decision that
/// waits for nothing).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wait_never_goes_in_the_asks_slot() {
    let h = harness(WAITS_LIMITS).await;
    raise_reset_wait(&h).await;
    point(&h, ended(None, Some(Wall::Login)));
    evaluate(&h).await;
    wait_until("the login's ask is shown", || {
        shown(&h).is_some_and(|text| text.starts_with("login: "))
    })
    .await;
    assert_eq!(h.fake.attention_kinds().get("codex-alab"), None, "an ask");
    wait_until("the wait is down", || wait_up(&h).is_none()).await;
    let requests = h.fake.requests();
    let own: Vec<&String> = requests
        .iter()
        .filter(|r| r.contains(" meta set attention owner=codex-alab "))
        .collect();
    assert!(!own.is_empty(), "{requests:?}");
    assert!(own.iter().all(|r| !r.contains("waiting")), "{own:?}");
    assert!(
        requests
            .iter()
            .filter(|r| r.contains(" meta set attention owner=codex-alab-wait "))
            .all(|r| r.contains(" owner=codex-alab-wait kind=wait ")),
        "{requests:?}"
    );
}

/// A LOST CLAIM TAKES THE WAIT DOWN, HOWEVER IT WENT (review of f134d2b2cb, finding 1: only
/// the claim's watch and a gap took it down): an act's checks that find the claim cleared
/// before its watch does leave no wait standing, as nothing types when it ends. NEGATIVE
/// CONTROL: the same checks under the claim still held keep it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lost_claim_takes_the_wait_down() {
    let h = harness(WAITS_LIMITS).await;
    raise_reset_wait(&h).await;
    // NEGATIVE CONTROL first.
    wait_go(&h.sup).await;
    h.sup.show_waits().await;
    assert_eq!(wait_up(&h).as_deref(), Some(RESET), "{:?}", h.sup.journal());
    // The person clears the claim; the watch meets it late, the checks first.
    h.fake.delay_clear_event();
    h.fake.clear_claim();
    let ready = h.sup.ready().await;
    assert!(
        matches!(&ready, Ready::Passive(why) if why.contains("no proven claim of ours")),
        "{ready:?} {:?}",
        h.sup.journal()
    );
    wait_until("the lost claim takes the wait down", || {
        wait_up(&h).is_none()
    })
    .await;
}

/// A STOPPED THREAD TAKES ITS WAIT DOWN, AND ANOTHER THREAD'S STAYS (review of f134d2b2cb,
/// finding 6: no test reached the stop): the one slot shows the soonest wait, the reset an hour
/// off; the stopped runtime's goes, and the other thread's later wait shows in its place.
/// NEGATIVE CONTROL: the other thread's wait stands through the stop.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stopped_thread_takes_its_wait_down_and_another_threads_stays() {
    use codex_extension_api::ThreadLifecycleContributor;
    use codex_extension_api::ThreadStopInput;
    const LATER: &str = "the other thread's wait";
    let h = harness(WAITS_LIMITS).await;
    assert!(h.sup.want_wait_for(
        "another-thread",
        Some((
            Instant::now() + Duration::from_secs(7200),
            LATER.to_string()
        ))
    ));
    raise_reset_wait(&h).await;
    h.autopilot
        .on_thread_stop(ThreadStopInput {
            session_store: &ExtensionData::new("session"),
            thread_store: &h._store,
        })
        .await;
    wait_until("the other thread's wait shows", || {
        wait_up(&h).as_deref() == Some(LATER)
    })
    .await;
    assert_eq!(
        h.fake.attention_kinds().get(WAIT).map(String::as_str),
        Some("wait")
    );
}

/// A WAIT WHOSE ACT DID NOT GO COMES DOWN (review of f134d2b2cb, finding 6: no test reached an
/// act that did not go), against a real core thread: a continuation backed off shows its wait;
/// due, the thread is in Plan mode, so nothing is typed, and the wait comes down beside the
/// `plan:` ask, as nothing types when it ends. NEGATIVE CONTROL: back in the default mode, the
/// same act goes, and the wait it showed is gone once its turn has run (its turn's start takes
/// it down, and the hour's cap is what waits after it).
#[test]
fn a_wait_whose_act_did_not_go_comes_down() {
    core_test_support::test_codex::run_test_with_large_stack(
        "a wait whose act did not go",
        || async {
            tokio::time::timeout(Duration::from_secs(60), act_not_gone())
                .await
                .unwrap_or_else(|_| panic!("never: the act's outcome"));
            Ok(())
        },
    )
    .unwrap_or_else(|err| panic!("native core test: {err}"));
}

async fn act_not_gone() {
    use codex_core::config::Config;
    use codex_protocol::config_types::CollaborationMode;
    use codex_protocol::config_types::ModeKind;
    use codex_protocol::config_types::Settings;
    use codex_protocol::protocol::ThreadSettingsOverrides;
    use core_test_support::responses;
    codex_alab_core::activate();
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .unwrap_or_else(|err| panic!("fake aterm: {err}"));
    fake.set_policy_key("human_grace_s", "0");
    // The act's own turn end waits on the hour's cap, not another act.
    fake.set_policy_key("continue_per_hour", "1");
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let endpoint = fake.endpoint();
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(move || Some(endpoint.clone())),
        client: ClientOptions::default(),
    });
    let mut builder = codex_extension_api::ExtensionRegistryBuilder::<Config>::new();
    let autopilot = Autopilot::new(Arc::clone(&sup), None, None, Timing::default(), |_| true);
    autopilot.register(&mut builder);
    let server = responses::start_mock_server().await;
    let test = core_test_support::test_codex::test_codex()
        .with_extensions(Arc::new(builder.build()))
        .build(&server)
        .await
        .unwrap_or_else(|err| panic!("test codex: {err}"));
    autopilot.bind_thread_manager(Arc::downgrade(&test.thread_manager));
    let rt = test
        .codex
        .thread_extension_data()
        .get::<ThreadRt>()
        .unwrap_or_else(|| panic!("native root runtime"));
    shared.observe(
        &Frame {
            primary_thread: Some(rt.key()),
            displayed_thread: Some(rt.key()),
            rollout_path: None,
            cwd: test.cwd_path(),
            model: "gpt-5",
            codex_home: test.codex_home_path(),
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
    wait_until("the claim is proven and acted under", || sup.acting()).await;
    let mode = |mode| ThreadSettingsOverrides {
        collaboration_mode: Some(CollaborationMode {
            mode,
            settings: Settings {
                model: test.session_configured.model.clone(),
                reasoning_effort: None,
                developer_instructions: None,
            },
        }),
        ..Default::default()
    };
    let wait_up = || fake.attention().get(WAIT).cloned();
    core_test_support::submit_thread_settings(&test.codex, mode(ModeKind::Plan))
        .await
        .unwrap_or_else(|err| panic!("Plan mode: {err}"));
    back_off_then_act(&autopilot, &rt, &fake).await;
    assert!(
        fake.attention()
            .get("codex-alab")
            .is_some_and(|ask| ask.starts_with("plan: Plan mode leaves")),
        "{:?} {:?}",
        fake.attention(),
        sup.journal()
    );
    assert_eq!(wait_up(), None, "{:?}", sup.journal());
    assert!(
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty(),
        "nothing typed"
    );
    // NEGATIVE CONTROL: the default mode, and the act goes.
    core_test_support::submit_thread_settings(&test.codex, mode(ModeKind::Default))
        .await
        .unwrap_or_else(|err| panic!("the default mode: {err}"));
    let typed = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("act"),
            responses::ev_assistant_message("act-msg", "Stage 2 is in."),
            responses::ev_completed("act"),
        ]),
    )
    .await;
    back_off_then_act(&autopilot, &rt, &fake).await;
    wait_until(
        "the act's turn ran and the hour's cap waits after it",
        || {
            typed.requests().len() == 1
                && wait_up().is_some_and(|shown| shown.starts_with(words::HOUR_CAP))
        },
    )
    .await;
    test.codex
        .shutdown_and_wait()
        .await
        .unwrap_or_else(|err| panic!("stop the thread: {err}"));
}

/// The person's point, its continuation backed off by a standing short streak: the wait
/// shows; then the rung is passed, and the act is due.
async fn back_off_then_act(autopilot: &Arc<Autopilot>, rt: &Arc<ThreadRt>, fake: &FakeAterm) {
    rt.with_state(|state| {
        state.observe_person_turn();
        state.observe_turn_end(ended(Some("Stage 1 is in."), None), &Timing::default());
        state.short_streak = 1;
    });
    Arc::clone(autopilot).evaluate(Arc::clone(rt)).await;
    let shown = fake.attention().get(WAIT).cloned().unwrap_or_default();
    assert!(
        shown.starts_with(&words::short_backoff_wait(1, Duration::from_secs(120))),
        "the back-off's wait: {shown:?} {:?}",
        fake.requests()
    );
    rt.with_state(|state| state.short_streak = 0);
    Arc::clone(autopilot).evaluate(Arc::clone(rt)).await;
}
