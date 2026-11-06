// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The claim's life from the supervisor's side, against alab-aterm's in-process FAKE aterm
//! (never a real one):
//!
//! - the exit path gives the claim back (`meta unset supervisor holder=…`) and nothing is
//!   claimed after it;
//! - a claim someone else cleared stays given up until the policy moves;
//! - a request connection that stopped serving is replaced, and the claim taken again;
//! - a transient off (a hold, a suspension, a policy read that failed) keeps the proven claim
//!   and acts on nothing; a person's or the config's off gives it back, and the claim is taken
//!   again once the policy reads on; an orderly exit still gives a kept claim back;
//! - the policy is read again at the events that may move it, after lost events, and on the
//!   passive clock while passive (never while acting);
//! - a mail turn that ends passive owes its receipt and report until codex-alab acts again,
//!   typed mail is read on standby, and act records go while the claim is held;
//! - the late echo of codex-alab's own release is no clear; another holder's claim is waited
//!   out at its event, not on the clock; a hold read after a standby lifts the standby.

use std::sync::Arc;
use std::time::Duration;

use codex_alab_aterm::Options as ClientOptions;
use codex_alab_aterm::fake::FakeAterm;
use codex_alab_aterm::fake::Mode;
use codex_alab_autopilot::Supervisor;
use codex_alab_autopilot::SupervisorOptions;
use codex_alab_autopilot::supervisor::Ready;
use codex_alab_core::session::Frame;
use codex_alab_core::session::ProgramState;
use codex_alab_core::session::Shared;
use codex_alab_core::session::now_ms;

struct Harness {
    fake: FakeAterm,
    shared: &'static Shared,
    sup: Arc<Supervisor>,
    _dir: tempfile::TempDir,
}

async fn harness(timeout: Duration) -> Harness {
    harness_with(timeout, None, |_| {}).await
}

/// [`harness`] with the passive clock's period (`None`: its default) and the fake set up by
/// `before` ahead of the supervisor's start.
async fn harness_with(
    timeout: Duration,
    recheck: Option<Duration>,
    before: impl FnOnce(&FakeAterm),
) -> Harness {
    codex_alab_core::activate();
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .unwrap_or_else(|err| panic!("fake aterm: {err}"));
    fake.set_policy_key("human_grace_s", "1");
    before(&fake);
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let endpoint = fake.endpoint();
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(move || Some(endpoint.clone())),
        client: ClientOptions {
            timeout,
            ..ClientOptions::default()
        },
    });
    if let Some(every) = recheck {
        sup.set_policy_recheck(every);
    }
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
    Harness {
        fake,
        shared,
        sup,
        _dir: dir,
    }
}

fn proven(f: &FakeAterm) -> bool {
    f.claim().is_some_and(|c| c.proven_pid.is_some())
}

fn claims_sent(f: &FakeAterm) -> usize {
    f.requests()
        .iter()
        .filter(|r| r.contains("meta set supervisor codex-alab@"))
        .count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_exit_gives_the_claim_back_and_nothing_claims_after_it() {
    let h = harness(ClientOptions::default().timeout).await;
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
    let shared = h.shared;
    // The exit path calls it from a thread of its own, the record already ended.
    let released =
        tokio::task::spawn_blocking(move || shared.release_claim(Duration::from_secs(2)))
            .await
            .unwrap_or_else(|err| panic!("release: {err}"));
    assert!(released, "{:?}", h.sup.journal());
    assert!(h.fake.claim().is_none());
    let holder = codex_alab_aterm::holder_for(std::process::id());
    assert!(
        h.fake
            .requests()
            .iter()
            .any(|r| r.ends_with(&format!("meta unset supervisor holder={holder}"))),
        "{:?}",
        h.fake.requests()
    );
    assert!(h.sup.closing());
    let sent = claims_sent(&h.fake);
    // A policy move after the exit claims nothing.
    h.fake.set_policy_key("continue_text", "keep at it");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(h.fake.claim().is_none());
    assert_eq!(claims_sent(&h.fake), sent);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_claim_someone_cleared_stays_given_up_until_the_policy_moves() {
    let h = harness(ClientOptions::default().timeout).await;
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
    let sent = claims_sent(&h.fake);
    // A person's `meta unset supervisor`.
    h.fake.clear_claim();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while h.sup.held() {
        assert!(tokio::time::Instant::now() < deadline, "never passive");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        h.fake.claim().is_none(),
        "not taken back: {:?}",
        h.sup.journal()
    );
    assert_eq!(claims_sent(&h.fake), sent);
    // The owner's policy moves: codex-alab may claim again.
    h.fake.set_policy_key("continue_text", "keep at it");
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dead_request_connection_is_replaced_and_the_claim_taken_again() {
    let h = harness(Duration::from_millis(300)).await;
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
    // aterm stops answering `harness policy`: the re-read times out and the connection
    // (and the claim bound to it) ends.
    h.fake.stall("harness");
    h.fake.set_policy_key("continue_text", "keep at it");
    assert!(
        h.fake.wait_for(|f| f.claim().is_none()).await,
        "{:?}",
        h.sup.journal()
    );
    h.fake.unstall("harness");
    assert!(
        h.fake.wait_for(proven).await,
        "claimed again on a new connection: {:?}",
        h.sup.journal()
    );
    // The supervisor counts it held once its status read saw it proven.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while !h.sup.held() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "{:?}",
            h.sup.journal()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        h.sup
            .journal()
            .iter()
            .any(|line| line.contains("stopped serving")),
        "{:?}",
        h.sup.journal()
    );
}

/// Integrations f1: of codex-alab's acts, waits and escalations only a question box's answer is
/// told (`story chose <policy word>` on the claim's connection, as aterm's own approval loop
/// tells a dialog's answer): aterm takes no other story word, and its loop tells no prose
/// answer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_an_answer_is_told_to_aterms_session_log() {
    let h = harness(ClientOptions::default().timeout).await;
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
    // The supervisor marks the claim held after aterm shows it proven (a hang detector).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while !h.sup.held() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "{:?}",
            h.sup.journal()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    h.sup
        .record_act("approve-all@v1", "approved exec once (approve=all)")
        .await;
    h.sup
        .record_act(
            "answer-recommended@v1",
            "selected \"Fast (Recommended)\" (questions=recommended)",
        )
        .await;
    h.sup
        .record_act(
            "prose-long-term@v1",
            "answered a prose question (long-term)",
        )
        .await;
    h.sup
        .record_act("continue@v1", "typed \"keep going\"")
        .await;
    h.sup
        .record_act("context-compact@v1", "compacted a full context")
        .await;
    h.sup
        .record_act("usage-resume@v1", "resumed the goal at the usage reset")
        .await;
    h.sup
        .record_wait("waiting for the usage reset at 14:05")
        .await;
    h.sup
        .attention(
            "question",
            Some("question: names an irreversible act".to_string()),
        )
        .await;
    // Land's instruction and its landing tell nothing either.
    h.sup
        .record_act("land@v1", "typed the land instruction")
        .await;
    h.sup
        .record_act(
            "landed@v1",
            "landed: a clean defense, and the repository holds nothing no remote main has",
        )
        .await;
    let told: Vec<(String, String)> = h.fake.stories();
    let pair = |v: &str, t: &str| (v.to_string(), t.to_string());
    assert_eq!(
        told,
        vec![pair("chose", "recommended")],
        "{:?}",
        h.sup.journal()
    );
    // No story verb aterm refuses was sent: every story request was the answer's.
    let sent: Vec<String> = h
        .fake
        .requests()
        .into_iter()
        .filter(|r| r.contains(" story "))
        .collect();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(h.fake.acts().len(), 8);
}

/// Integrations a: `/ide` inside aterm reads this session's terminal context from aterm while
/// the proven claim is held, on a connection of its own (the claim's stays as it is).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_terminal_context_is_read_under_the_proven_claim() {
    let h = harness(ClientOptions::default().timeout).await;
    h.fake.set_blocks(
        "0",
        &[
            "block 1 complete exit=1 prompt=1 cmd=1 out=2 end=3 cwd=/work cmdline=make",
            "block 2 executing exit=- prompt=3 cmd=3 out=4 end=- cwd=/work cmdline=codex",
        ],
    );
    h.fake.set_selection("0", &["the line I mean"]);
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while !h.sup.held() || h.shared.claim().is_none() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "{:?}",
            h.sup.journal()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(h.shared.context_available());
    let shared = h.shared;
    let context =
        tokio::task::spawn_blocking(move || shared.terminal_context(Duration::from_secs(5)))
            .await
            .unwrap_or_else(|err| panic!("read: {err}"))
            .unwrap_or_else(|| panic!("no context: {:?}", h.sup.journal()));
    assert!(
        context.prompt.contains("the line I mean"),
        "{}",
        context.prompt
    );
    assert!(context.prompt.contains("- `make` exit 1 in /work"));
    assert_eq!(context.summary, "selection 1 line · 1 command");
    // The claim is still held on its own connection.
    assert!(h.sup.held());
    assert!(proven(&h.fake));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_resume_line_written_after_the_claim_renews_it() {
    // aterm reads the record at the claim and at each meta change. End-to-end run
    // 2026-10-16: the rollout came to disk after the claim, so aterm's copy named no line,
    // and a kill -9 before any act was not relaunched.
    let h = harness(ClientOptions::default().timeout).await;
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
    let sent = claims_sent(&h.fake);
    // Negative control: a frame that moves nothing in the record renews nothing.
    h.shared.mark_changed();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(claims_sent(&h.fake), sent, "{:?}", h.fake.requests());
    // The writer wrote the record's first resume line.
    h.shared.note_resume_line();
    assert!(
        h.fake.wait_for(|f| claims_sent(f) == sent + 1).await,
        "{:?}",
        h.fake.requests()
    );
    assert!(proven(&h.fake), "the renewal kept the proof");
    // The same line again is no news.
    h.shared.mark_changed();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(claims_sent(&h.fake), sent + 1);
}

async fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while !done() {
        assert!(tokio::time::Instant::now() < deadline, "never: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn unsets(f: &FakeAterm) -> usize {
    f.requests()
        .iter()
        .filter(|r| r.contains("meta unset supervisor"))
        .count()
}

/// Wait (a hang detector) until the before-every-act checks say go: acting is on.
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
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The claim is kept, still proven, still named by the record, and nothing acts under it.
async fn kept_and_idle(h: &Harness) {
    assert!(proven(&h.fake), "{:?}", h.sup.journal());
    assert!(h.sup.held());
    assert_eq!(unsets(&h.fake), 0, "{:?}", h.fake.requests());
    // The record keeps saying who holds the session (the writer reads this).
    assert_eq!(
        h.shared.claim(),
        Some(codex_alab_aterm::holder_for(std::process::id()))
    );
    assert!(h.sup.policy().is_none(), "no policy to act on");
    assert!(
        matches!(h.sup.ready().await, Ready::Passive(_)),
        "{:?}",
        h.sup.journal()
    );
    // No attention is raised while passive, as none was after a release.
    h.sup
        .attention("turn", Some("turn: core did not start it".to_string()))
        .await;
    assert!(
        !h.fake.attention().contains_key("codex-alab"),
        "{:?}",
        h.fake.attention()
    );
}

/// The person's or the config's offs: each gives the claim back.
const RELEASING: [&str; 5] = [
    "autopilot-off",
    "agent-off",
    "disabled",
    "headless",
    "unhosted",
];

/// CONTRACT §2 as decided 2026-10-17: a TRANSIENT off keeps the proven claim and acts on
/// nothing: a suspension (a failed seamless handoff aterm resumes) and a policy read that
/// failed go on standby, which clears codex-alab's attention; a person's hold keeps the claim
/// and every act waits on `status hold=`. Negative controls: before each off, and after it
/// lifts, the same checks say go, and nothing gave the claim back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_transient_off_keeps_the_proven_claim_and_acts_on_nothing() {
    let h = harness(ClientOptions::default().timeout).await;
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
    // A suspension.
    wait_go(&h.sup).await;
    h.sup
        .attention("turn", Some("turn: left to a person".to_string()))
        .await;
    assert!(
        h.fake.attention().contains_key("codex-alab"),
        "acting raises it: {:?}",
        h.fake.attention()
    );
    h.fake.set_effective(false, "suspended");
    wait_until("suspended", || {
        h.sup.standby().as_deref() == Some("effective=off why=suspended")
    })
    .await;
    assert!(
        h.fake
            .wait_for(|f| !f.attention().contains_key("codex-alab"))
            .await,
        "attention cleared at the off: {:?}",
        h.fake.attention()
    );
    kept_and_idle(&h).await;
    h.fake.set_effective(true, "enabled");
    wait_until("acting again", || h.sup.acting()).await;
    wait_go(&h.sup).await;
    // A policy read aterm cannot answer now.
    h.fake
        .answer_policy_with(Some("ERR internal: the policy cell is busy"));
    h.fake.set_policy_key("continue_text", "keep at it");
    wait_until("the read failed", || {
        h.sup
            .standby()
            .is_some_and(|why| why.starts_with("policy unreadable"))
    })
    .await;
    kept_and_idle(&h).await;
    h.fake.answer_policy_with(None);
    h.fake.set_policy_key("continue_text", "keep going");
    wait_until("acting again", || h.sup.acting()).await;
    wait_go(&h.sup).await;
    assert_eq!(unsets(&h.fake), 0, "{:?}", h.fake.requests());
    // A person's hold keeps the claim too, and every act waits on `status hold=`.
    h.fake.set_status("hold", "1");
    h.fake.set_effective(false, "hold");
    wait_until("the hold is read", || {
        h.sup
            .journal()
            .iter()
            .any(|line| line.contains("why=hold: the claim is kept"))
    })
    .await;
    assert!(proven(&h.fake) && h.sup.held());
    assert!(
        matches!(h.sup.ready().await, Ready::Wait { .. }),
        "{:?}",
        h.sup.journal()
    );
    assert_eq!(unsets(&h.fake), 0, "{:?}", h.fake.requests());
    h.fake.set_status("hold", "0");
    h.fake.set_effective(true, "enabled");
    wait_go(&h.sup).await;
}

/// CONTRACT §2 as decided 2026-10-17: a person's or the config's off gives the claim back
/// (`meta unset supervisor holder=…`), so aterm shows no proven claim (no mail is typed into
/// a hands-off session, no other supervisor meets `ERR busy`), clears codex-alab's attention,
/// and the claim is taken again, proven, once the policy reads on. Negative control: the
/// transient offs above give nothing back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_persons_or_the_configs_off_gives_the_claim_back_until_the_policy_reads_on() {
    let h = harness(ClientOptions::default().timeout).await;
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
    for (n, why) in RELEASING.into_iter().enumerate() {
        wait_go(&h.sup).await;
        h.sup
            .attention("turn", Some("turn: left to a person".to_string()))
            .await;
        assert!(h.fake.attention().contains_key("codex-alab"));
        h.fake.set_effective(false, why);
        assert!(
            h.fake.wait_for(|f| f.claim().is_none()).await,
            "{why}: given back: {:?}",
            h.sup.journal()
        );
        assert_eq!(unsets(&h.fake), n + 1, "{why}: {:?}", h.fake.requests());
        assert!(
            h.fake
                .wait_for(|f| !f.attention().contains_key("codex-alab"))
                .await,
            "{why}: attention cleared"
        );
        wait_until("passive", || !h.sup.held()).await;
        assert_eq!(h.shared.claim(), None, "{why}: the record names no claim");
        assert!(
            matches!(h.sup.ready().await, Ready::Passive(_)),
            "{why}: {:?}",
            h.sup.journal()
        );
        h.fake.set_effective(true, "enabled");
        assert!(
            h.fake.wait_for(proven).await,
            "{why}: claimed again: {:?}",
            h.sup.journal()
        );
    }
    wait_go(&h.sup).await;
}

/// aterm moves `effective=` for an Autopilot word or a hold with no `harness-policy` event: a
/// codex-alab looks again at the `meta` or `hold` event, giving the claim back or claiming
/// again. Negative control: with no event at all, or only a change of the session's words,
/// the quiet move is not seen (the passive clock is at its default 30 s here).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_quiet_off_and_its_lift_are_seen_at_the_meta_or_hold_event_that_tells_them() {
    let h = harness(ClientOptions::default().timeout).await;
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
    h.fake.set_effective(false, "autopilot-off");
    assert!(h.fake.wait_for(|f| f.claim().is_none()).await);
    // The release's own echo (`meta-change field=supervisor value=-`) takes nothing and reads
    // no policy (review of 7e7622ff7, finding 1): wait for it, so the quiet move below is the
    // only one left to see.
    wait_until("the release's echo is met", || {
        h.sup
            .journal()
            .iter()
            .any(|line| line == "a clear met while unclaimed: nothing to take")
    })
    .await;
    // The word turns back on, quietly: nothing is announced, so nothing is seen; nor is it
    // at a change of the session's words, which never move the policy.
    h.fake.set_effective_quietly(true, "enabled");
    h.fake.emit("meta field=attention value=-");
    h.fake.emit("meta field=description value=x");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(h.fake.claim().is_none(), "{:?}", h.sup.journal());
    // The word's own `meta` event is the moment.
    h.fake.emit("meta field=autopilot value=on");
    assert!(
        h.fake.wait_for(proven).await,
        "claimed at the meta event: {:?}",
        h.sup.journal()
    );
    // And a hold's event, for a quiet off and its lift alike.
    h.fake.set_effective_quietly(false, "autopilot-off");
    h.fake.emit("hold 1 reason=owner origin=cli");
    assert!(h.fake.wait_for(|f| f.claim().is_none()).await);
    h.fake.set_effective_quietly(true, "enabled");
    h.fake.emit("hold 0 reason=owner origin=cli");
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
    wait_go(&h.sup).await;
}

/// The owner's ruling of 2026-10-24: the harness's "on" is the whole harness, not Autopilot
/// alone. The word the TUI's chrome reads beside the record's Autopilot follows the session:
/// the acting mode while codex-alab holds the proven claim (a hold keeps it), `Off` while the
/// harness is on and only Autopilot is off (`why=autopilot-off`, the claim given back all the
/// same), whether the session started so or went so, and under a person's hold over that Off
/// (aterm names the hold first; `status` says off), claimed, unclaimed or the claim lost
/// under it; the harness held off by one of aterm's Settings switches (`disabled`,
/// `agent-off`) or with no host (`unhosted`) names its own (`⊘ off`; amended 2026-10-29 after
/// the review of gap 12, finding 3). Negative controls: a suspension, the other release
/// (`headless`) and an unclaimed hold over Land name no word.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_chrome_word_tells_autopilot_off_from_the_harness_off() {
    use codex_alab_core::session::Autopilot;
    use codex_alab_core::session::HarnessWord;
    let state = |h: &Harness| (h.shared.autopilot(), h.shared.harness_word());
    let h = harness(ClientOptions::default().timeout).await;
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
    wait_until("acting under cruise", || {
        state(&h) == (Autopilot::Active, HarnessWord::Cruise)
    })
    .await;
    h.fake.set_effective(false, "suspended");
    wait_until("a suspension names no word", || {
        state(&h) == (Autopilot::Passive, HarnessWord::Unknown)
    })
    .await;
    h.fake.set_status("autopilot", "land");
    h.fake.set_effective(true, "enabled");
    wait_until("acting again, under land", || {
        state(&h) == (Autopilot::Active, HarnessWord::Land)
    })
    .await;
    for why in RELEASING {
        h.fake.set_effective(false, why);
        assert!(
            h.fake.wait_for(|f| f.claim().is_none()).await,
            "{why}: given back: {:?}",
            h.sup.journal()
        );
        // aterm's Settings switches wear `⊘ off` (review of gap 12, finding 3).
        let word = match why {
            "autopilot-off" => HarnessWord::Off,
            "disabled" => HarnessWord::HarnessOff,
            "agent-off" => HarnessWord::AgentOff,
            "unhosted" => HarnessWord::Unhosted,
            _ => HarnessWord::Unknown,
        };
        wait_until(why, || state(&h) == (Autopilot::Passive, word)).await;
        h.fake.set_effective(true, "enabled");
        assert!(
            h.fake.wait_for(proven).await,
            "{why}: {:?}",
            h.sup.journal()
        );
        wait_until("claimed again under land", || {
            state(&h) == (Autopilot::Active, HarnessWord::Land)
        })
        .await;
    }
    h.fake.set_status("hold", "1");
    h.fake.set_effective(false, "hold");
    wait_until("the hold is read", || {
        h.sup
            .journal()
            .iter()
            .any(|line| line.contains("why=hold: the claim is kept"))
    })
    .await;
    assert_eq!(state(&h), (Autopilot::Active, HarnessWord::Land));
    // The person turns Autopilot off under the hold: aterm names the hold first and `status`
    // says off. The claim and the landing's mode are kept; the chrome names the Off.
    h.fake.set_status("autopilot", "off");
    h.fake.emit("meta field=autopilot value=off");
    wait_until("a hold over Off names off", || {
        state(&h) == (Autopilot::Active, HarnessWord::Off)
    })
    .await;
    assert!(h.fake.claim().is_some(), "{:?}", h.sup.journal());
    // The hold lifts over the Off: the claim goes, and the Off stays named.
    h.fake.set_status("hold", "0");
    h.fake.set_effective(false, "autopilot-off");
    assert!(
        h.fake.wait_for(|f| f.claim().is_none()).await,
        "{:?}",
        h.sup.journal()
    );
    wait_until("released under Off", || {
        state(&h) == (Autopilot::Passive, HarnessWord::Off)
    })
    .await;
    // A hold placed over that Off, unclaimed, still names it.
    let passive_holds = |h: &Harness| {
        h.sup
            .journal()
            .iter()
            .filter(|line| line.contains("why=hold: passive"))
            .count()
    };
    let before = passive_holds(&h);
    h.fake.set_status("hold", "1");
    h.fake.set_effective(false, "hold");
    wait_until("the unclaimed hold is read", || passive_holds(&h) > before).await;
    assert_eq!(state(&h), (Autopilot::Passive, HarnessWord::Off));
    // Negative control: a hold over Land, unclaimed, names no word.
    h.fake.set_status("autopilot", "land");
    h.fake.set_effective(false, "hold");
    wait_until("an unclaimed hold over land names none", || {
        state(&h) == (Autopilot::Passive, HarnessWord::Unknown)
    })
    .await;
    h.fake.set_status("hold", "0");
    h.fake.set_effective(true, "enabled");
    wait_go(&h.sup).await;
    wait_until("claimed again under land", || {
        state(&h) == (Autopilot::Active, HarnessWord::Land)
    })
    .await;
    // A claim lost under a person's hold over Off keeps the Off named, though nothing reads
    // the policy again until it moves.
    h.fake.set_status("hold", "1");
    h.fake.set_effective(false, "hold");
    h.fake.set_status("autopilot", "off");
    h.fake.emit("meta field=autopilot value=off");
    wait_until("held over Off again", || {
        state(&h) == (Autopilot::Active, HarnessWord::Off)
    })
    .await;
    h.fake.clear_claim();
    wait_until("the claim is lost", || !h.sup.held()).await;
    wait_until("a loss under the hold over Off keeps it named", || {
        state(&h) == (Autopilot::Passive, HarnessWord::Off)
    })
    .await;

    // aterm's Settings switches and its missing host wear `⊘ off` (review of gap 12, finding
    // 3); a headless instance, which draws no window, names no word (NEGATIVE CONTROL).
    for (why, word) in [
        ("autopilot-off", HarnessWord::Off),
        ("disabled", HarnessWord::HarnessOff),
        ("agent-off", HarnessWord::AgentOff),
        ("unhosted", HarnessWord::Unhosted),
        ("headless", HarnessWord::Unknown),
    ] {
        let h = harness_with(ClientOptions::default().timeout, None, |f| {
            f.set_effective(false, why);
        })
        .await;
        wait_until(why, || state(&h) == (Autopilot::Passive, word)).await;
        assert!(h.fake.claim().is_none(), "{why}: {:?}", h.sup.journal());
    }
}

fn policy_reads(f: &FakeAterm) -> usize {
    f.requests()
        .iter()
        .filter(|r| r.ends_with(" harness policy"))
        .count()
}

/// Review of 4cdc96090, finding 5: aterm announces a suspension with no event at all, so
/// while passive codex-alab reads the policy on a clock (30 s; shortened here): a quiet lift
/// of a suspension is seen by it. Negative control: while codex-alab acts the clock is off,
/// and no read is made however long it waits.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_passive_clock_sees_a_quiet_lift_and_never_runs_while_acting() {
    let every = Duration::from_millis(200);
    let h = harness_with(ClientOptions::default().timeout, Some(every), |_| {}).await;
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
    wait_until("acting", || h.sup.acting()).await;
    let reads = policy_reads(&h.fake);
    tokio::time::sleep(every * 5).await;
    assert_eq!(policy_reads(&h.fake), reads, "no clock while acting");
    h.fake.set_effective(false, "suspended");
    wait_until("suspended", || h.sup.standby().is_some()).await;
    h.fake.set_effective_quietly(true, "enabled");
    wait_until("the clock sees the lift", || h.sup.acting()).await;
    kept_by_aterm(&h);
    wait_go(&h.sup).await;
}

/// A session that started with `effective=off` (a quiet word since moved it on) is claimed by
/// the passive clock. Negative control: at the clock's default period, nothing is claimed
/// within the same wait.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_that_started_off_is_claimed_once_the_clock_reads_on() {
    for every in [None, Some(Duration::from_millis(200))] {
        let h = harness_with(ClientOptions::default().timeout, every, |f| {
            f.set_effective(false, "autopilot-off");
        })
        .await;
        wait_until("the off is read", || policy_reads(&h.fake) > 0).await;
        h.fake.set_effective_quietly(true, "enabled");
        if every.is_none() {
            tokio::time::sleep(Duration::from_secs(1)).await;
            assert!(h.fake.claim().is_none(), "{:?}", h.sup.journal());
        } else {
            assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
        }
    }
}

/// Review of 4cdc96090, finding 4: events were lost (`GAP`) and the claim still stands: the
/// policy is read again, as the lost events may have moved it. Negative control: with no gap
/// the quiet move is not seen.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lost_events_read_the_policy_again() {
    let h = harness(ClientOptions::default().timeout).await;
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
    wait_until("acting", || h.sup.acting()).await;
    h.fake.set_effective_quietly(false, "suspended");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(h.sup.acting(), "no gap, no read: {:?}", h.sup.journal());
    h.fake.emit_frame("GAP 1 events-dropped=3");
    wait_until("the gap's read", || {
        h.sup.standby().as_deref() == Some("effective=off why=suspended")
    })
    .await;
    kept_by_aterm(&h);
}

fn mail_turn(id: u64, off: u64) -> codex_alab_core::mail::MailTurn {
    codex_alab_core::mail::MailTurn {
        id,
        off,
        from: "s-lead@n-lab".to_string(),
        kind: "task".to_string(),
        text: format!("Inbox: task @{off}"),
    }
}

/// The TUI's submit path reads typed mail from its own thread, never the runtime's.
async fn read_mail(h: &Harness, off: u64) -> Result<codex_alab_core::mail::InboxMail, String> {
    let shared = h.shared;
    tokio::task::spawn_blocking(move || {
        shared.read_mail(
            codex_alab_core::mail::TriggerKind::Task,
            off,
            Duration::from_secs(5),
        )
    })
    .await
    .unwrap_or_else(|err| panic!("read: {err}"))
}

/// Review of fea034d20, findings 1, 2 and 5: a mail turn that ends while codex-alab is passive
/// (on standby, or after a release) records its receipt and report as owed, and they go by
/// themselves once it acts again (at the resume, at the new claim); typed mail is read on
/// standby (aterm types the line while it shows the claim proven); an act that went is
/// recorded under the kept claim. Negative controls: while passive nothing is sent; after a
/// release no mail is read and no act is recorded.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_mail_turn_that_ends_passive_is_receipted_and_reported_once_codex_alab_acts() {
    use codex_alab_core::mail::Verdict;
    let h = harness(ClientOptions::default().timeout).await;
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
    h.fake.set_effective(false, "suspended");
    wait_until("suspended", || h.sup.standby().is_some()).await;
    let (id, off) = h
        .fake
        .deliver("s-lead@n-lab", "task", "agent", "Run the tests.");
    let read = read_mail(&h, off).await;
    assert!(
        read.as_ref().is_ok_and(|m| m.id == id),
        "typed mail is read on standby: {read:?}"
    );
    // The turn ends on standby, as the extension's turn end calls these.
    h.sup.hold_receipt(id, Verdict::Handled);
    h.sup
        .receipt(
            mail_turn(id, off),
            Verdict::Handled,
            Some("Tests pass.".to_string()),
        )
        .await;
    assert!(h.fake.receipts().is_empty(), "{:?}", h.fake.receipts());
    assert!(h.fake.posts().is_empty());
    // An act that went before the standby is recorded under the kept claim.
    h.sup
        .record_act("continue@v1", "typed \"keep going\"")
        .await;
    assert_eq!(h.fake.acts(), vec!["continue@v1".to_string()]);
    // Acting again: the owed receipt and report go by themselves.
    h.fake.set_effective(true, "enabled");
    wait_until("the owed receipt and report go at the resume", || {
        h.fake.receipts() == vec![(id, "handled".to_string())] && h.fake.posts().len() == 1
    })
    .await;
    assert_eq!(h.fake.posts()[0].body, "Tests pass.");
    wait_until("the report is recorded as its act", || {
        h.fake.acts().iter().any(|a| a == "mail-report@v1")
    })
    .await;
    // After a release (Autopilot Off): no mail is read, no act is recorded, and a turn that
    // ends owes its receipt and report until the claim is taken again.
    h.fake.set_effective(false, "autopilot-off");
    wait_until("given back", || !h.sup.held() && h.fake.claim().is_none()).await;
    let (id2, off2) = h.fake.deliver("s-lead@n-lab", "task", "agent", "Again.");
    assert_eq!(
        read_mail(&h, off2).await,
        Err("no proven aterm claim".to_string())
    );
    let acts = h.fake.acts().len();
    h.sup
        .record_act("continue@v1", "typed \"keep going\"")
        .await;
    assert_eq!(h.fake.acts().len(), acts, "{:?}", h.fake.acts());
    h.sup
        .receipt(mail_turn(id2, off2), Verdict::Deferred, None)
        .await;
    assert_eq!(h.fake.receipts().len(), 1);
    assert_eq!(h.fake.posts().len(), 1);
    h.fake.set_effective(true, "enabled");
    wait_until("the owed receipt and report go at the new claim", || {
        h.fake.receipts().last() == Some(&(id2, "deferred".to_string()))
            && h.fake.posts().len() == 2
    })
    .await;
    assert!(
        h.fake.posts()[1]
            .body
            .ends_with("(the turn was interrupted or stopped at an error)")
    );
}

/// Review of fea034d20, finding 3: a fast Autopilot off and on, the word's event reaching
/// codex-alab ahead of the `value=-` echo of its own release, so the echo comes after it
/// claimed again. That echo is no clear: `status` still shows the claim proven and its own,
/// and the claim stands. Negative control: a person's clear after it (no release of
/// codex-alab's own due) gives the claim up.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_late_echo_of_its_own_release_is_no_clear() {
    let h = harness(ClientOptions::default().timeout).await;
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
    wait_go(&h.sup).await;
    h.fake.delay_clear_event();
    h.fake.set_effective(false, "autopilot-off");
    assert!(h.fake.wait_for(|f| f.claim().is_none()).await);
    wait_until("passive", || !h.sup.held()).await;
    // Back on: its policy event goes out, then the late echo.
    h.fake.set_effective(true, "enabled");
    let stands = "came after it claimed again: the claim stands";
    let cleared = "the claim was cleared: not taken again";
    wait_until("the late echo is met", || {
        h.sup
            .journal()
            .iter()
            .any(|l| l.contains(stands) || l.contains(cleared))
    })
    .await;
    assert!(
        h.sup.journal().iter().any(|l| l.contains(stands)),
        "{:?}",
        h.sup.journal()
    );
    assert!(proven(&h.fake) && h.sup.held(), "{:?}", h.sup.journal());
    wait_go(&h.sup).await;
    assert_eq!(
        unsets(&h.fake),
        1,
        "only the off's own release: {:?}",
        h.fake.requests()
    );
    // Negative control: a person's clear now is a clear.
    h.fake.clear_claim();
    wait_until("the clear is met", || {
        h.sup.journal().iter().any(|l| l.contains(cleared))
    })
    .await;
    assert!(!h.sup.held());
    assert!(h.fake.claim().is_none(), "{:?}", h.sup.journal());
}

/// Review of fea034d20, finding 6: another holder has the session (`ERR busy`): codex-alab
/// waits for that claim's own release event, and the passive clock (shortened here) asks
/// nothing meanwhile. Negative control: once that claim goes, its event is the moment, and
/// codex-alab claims.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_holders_claim_is_waited_out_at_its_event_not_on_the_clock() {
    let every = Duration::from_millis(200);
    let h = harness_with(ClientOptions::default().timeout, Some(every), |f| {
        f.claim_as("driver@1");
    })
    .await;
    wait_until("busy", || {
        h.sup
            .journal()
            .iter()
            .any(|l| l.contains("driver@1 holds the session"))
    })
    .await;
    let (claims, reads) = (claims_sent(&h.fake), policy_reads(&h.fake));
    tokio::time::sleep(every * 5).await;
    assert_eq!(claims_sent(&h.fake), claims, "{:?}", h.fake.requests());
    assert_eq!(policy_reads(&h.fake), reads, "{:?}", h.fake.requests());
    h.fake.clear_claim();
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
}

/// Review of 7e7622ff7, finding 4: a claim aterm did not prove keeps the passive clock off
/// only while the policy still reads the generation it came back unproven at. The policy moves
/// while aterm suspends the host (that read sees an off, and nothing claims); the suspension
/// then lifts with no event, and the clock sees it and claims, proven now. Negative control:
/// while the policy still reads the unproven claim's generation, the clock reads nothing and
/// nothing is claimed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unproven_claim_keeps_the_clock_off_only_at_its_own_generation() {
    let every = Duration::from_millis(200);
    let h = harness_with(ClientOptions::default().timeout, Some(every), |f| {
        f.set_foreground_leader(None);
    })
    .await;
    wait_until("the claim comes back unproven", || {
        h.sup
            .journal()
            .iter()
            .any(|l| l.contains("aterm did not prove the claim"))
    })
    .await;
    h.fake.set_foreground_leader(Some(std::process::id()));
    let (claims, reads) = (claims_sent(&h.fake), policy_reads(&h.fake));
    tokio::time::sleep(every * 5).await;
    assert_eq!(claims_sent(&h.fake), claims, "{:?}", h.fake.requests());
    assert_eq!(policy_reads(&h.fake), reads, "{:?}", h.fake.requests());
    h.fake.set_effective(false, "suspended");
    wait_until("the move is read", || policy_reads(&h.fake) > reads).await;
    h.fake.set_effective_quietly(true, "enabled");
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
}

/// Review of fea034d20, finding 7: a hold read after a standby lifts the standby: the hold's
/// own rule is the one left (acts wait on `status hold=`), and attention and mail stand under
/// it (CONTRACT §2): the owed receipt goes. Negative control: on standby, before the hold,
/// the receipt waits and no attention is raised.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hold_read_after_a_standby_lifts_the_standby() {
    use codex_alab_core::mail::Verdict;
    let h = harness(ClientOptions::default().timeout).await;
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
    wait_go(&h.sup).await;
    h.fake.set_effective(false, "suspended");
    wait_until("suspended", || h.sup.standby().is_some()).await;
    let (id, off) = h.fake.deliver("s-lead@n-lab", "task", "agent", "go");
    h.sup
        .receipt(
            mail_turn(id, off),
            Verdict::Handled,
            Some("done".to_string()),
        )
        .await;
    h.sup
        .attention("turn", Some("turn: left to a person".to_string()))
        .await;
    assert!(h.fake.receipts().is_empty());
    assert!(!h.fake.attention().contains_key("codex-alab"));
    h.fake.set_status("hold", "1");
    h.fake.set_effective(false, "hold");
    wait_until("the standby is lifted", || {
        h.sup.standby().is_none() && h.sup.acting()
    })
    .await;
    wait_until("the owed receipt goes under the hold", || {
        h.fake.receipts() == vec![(id, "handled".to_string())]
    })
    .await;
    h.sup
        .attention("turn", Some("turn: left to a person".to_string()))
        .await;
    assert!(
        h.fake.attention().contains_key("codex-alab"),
        "{:?}",
        h.fake.attention()
    );
    assert!(
        matches!(h.sup.ready().await, Ready::Wait { .. }),
        "acts wait on the hold: {:?}",
        h.sup.journal()
    );
    assert!(proven(&h.fake));
    assert_eq!(unsets(&h.fake), 0, "{:?}", h.fake.requests());
}

fn kept_by_aterm(h: &Harness) {
    assert!(proven(&h.fake), "{:?}", h.sup.journal());
    assert!(h.sup.held());
    assert_eq!(unsets(&h.fake), 0, "{:?}", h.fake.requests());
}

/// A kept claim (a suspension), and an orderly exit still gives it back (the record removed
/// first, CONTRACT §4), so aterm reads the end as orderly. Negative control: the claim stood
/// up to the exit.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_orderly_exit_while_passive_still_releases_the_claim() {
    let h = harness(ClientOptions::default().timeout).await;
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
    h.fake.set_effective(false, "suspended");
    wait_until("passive", || h.sup.standby().is_some()).await;
    kept_and_idle(&h).await;
    let shared = h.shared;
    let released =
        tokio::task::spawn_blocking(move || shared.release_claim(Duration::from_secs(2)))
            .await
            .unwrap_or_else(|err| panic!("release: {err}"));
    assert!(released, "{:?}", h.sup.journal());
    assert!(h.fake.claim().is_none());
    assert_eq!(unsets(&h.fake), 1, "{:?}", h.fake.requests());
    assert_eq!(h.shared.claim(), None);
    assert!(h.sup.closing());
}

/// Review of 4cdc96090, finding 6: a sign-in screen that never reached its clear left
/// codex-alab's `login:` attention standing; this start is past the sign-in (its primary
/// thread is registered), so its first connection clears it, once, before it claims.
/// The other startup screens' attentions (a folder trust, a model migration, a hooks review)
/// are cleared alike. Negative controls: the look leaves a codex-alab attention that is no
/// startup screen's, a `trust: …` one among them, and the autopilot's own `login: …`
/// attentions, still true (review of fea034d20, finding 4).
/// What the look did is read from the requests before the claim: since 3f850ea70 the proven
/// claim then reconciles codex-alab's own entry (`sync_attention`), so an entry the look left
/// may go moments after the claim, and reading the fake's entries there raced that
/// reconcile (it failed about half its runs alone, as a7c2ff6a3 found for the test below).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_past_the_sign_in_clears_a_standing_login_attention() {
    for (text, cleared) in [
        (
            "login: signed out; codex's sign-in screen waits for a person",
            true,
        ),
        (
            "question: a question box codex-alab leaves to a person",
            false,
        ),
        (
            "login: Codex's sign-in was refused or expired; sign in to Codex again (codex login)",
            false,
        ),
        ("login: Codex asks for a sign-in", false),
        (
            "trust: /work/repo is not trusted yet; codex's folder-trust screen waits for a person (approve=none: every folder is a person's)",
            true,
        ),
        (
            "migration: gpt-5.4 is retired and codex moves to gpt-6.1-sol; codex's model-migration screen waits for a person",
            true,
        ),
        (
            "hooks: 2 hooks for /work/repo need review; codex's hooks-review screen waits for a person",
            true,
        ),
        ("trust: a folder codex-alab was told to leave alone", false),
    ] {
        let h = harness_with(ClientOptions::default().timeout, None, |f| {
            f.set_attention_as("codex-alab", text);
        })
        .await;
        assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
        let requests = h.fake.requests();
        let claim = requests
            .iter()
            .position(|r| r.contains("meta set supervisor"))
            .unwrap_or_else(|| panic!("{text}: no claim: {requests:?}"));
        assert_eq!(
            requests[..claim]
                .iter()
                .any(|r| r.contains("meta unset attention owner=codex-alab")),
            cleared,
            "{text}: the look before the claim: {requests:?}"
        );
        assert_eq!(
            h.sup
                .journal()
                .iter()
                .any(|l| l.contains("cleared a startup screen's attention an earlier start left")),
            cleared,
            "{text}: {:?}",
            h.sup.journal()
        );
        if cleared {
            assert!(
                !h.fake.attention().contains_key("codex-alab"),
                "{text}: {:?}",
                h.sup.journal()
            );
        }
    }
}

/// Review of fea034d20, finding 4: the stale sign-in attention is looked for once a process,
/// whatever the look met: a first look that failed (aterm did not answer `meta`, and the
/// connection went with it) is not made again at the reconnect, which would clear what this
/// process may have raised since. Negative control: the test above, where the first look
/// answers and clears it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_stale_sign_in_attention_is_looked_for_once_a_process() {
    let text = "login: signed out; codex's sign-in screen waits for a person";
    let h = harness_with(Duration::from_millis(300), None, |f| {
        f.set_attention_as("codex-alab", text);
        f.stall("meta");
    })
    .await;
    wait_until("the first look failed", || {
        h.sup
            .journal()
            .iter()
            .any(|l| l.contains("a stale startup screen's attention was not looked for"))
    })
    .await;
    h.fake.unstall("meta");
    assert!(h.fake.wait_for(proven).await, "{:?}", h.sup.journal());
    // The reconnect's session met an answering aterm, and the entry still read exactly the
    // sign-in screen's text: a second look would have cleared it as that screen's own. That
    // a proven claim then settles an entry an earlier image left is the attention
    // reconcile's (sync_attention), which runs or not by whether the claim is held.
    assert!(
        !h.sup
            .journal()
            .iter()
            .any(|l| l.contains("cleared a startup screen's attention an earlier start left")),
        "{:?}",
        h.sup.journal()
    );
}

/// Retain the old screen entry while the first cleanup command is refused. No policy
/// timer or proven claim may supply its recovery: these starts stay deliberately passive.
async fn refused_startup_attention(mode: &str, clear: bool) -> Harness {
    let mut h = harness_with(
        Duration::from_millis(300),
        Some(Duration::from_secs(3600)),
        |fake| {
            fake.set_attention_as(
                "codex-alab",
                "login: signed out; codex's sign-in screen waits for a person",
            );
            match mode {
                "off" => fake.set_effective(false, "autopilot-off"),
                "unproven" => fake.set_foreground_leader(None),
                "unsupported" => fake.answer_policy_with(Some("ERR unknown verb (try: help)")),
                _ => panic!("unknown startup fixture mode"),
            }
            let command = if clear {
                format!("@{} meta unset attention owner=codex-alab", fake.sid())
            } else {
                format!("@{} meta", fake.sid())
            };
            fake.reply_once(&command, "ERR internal: the metadata cell is busy\n");
        },
    )
    .await;
    h._dir.disable_cleanup(true);
    eprintln!(
        "retained startup attention fixture: {}",
        h._dir.path().display()
    );
    wait_until("the first startup cleanup failed", || {
        h.sup
            .journal()
            .iter()
            .any(|line| line.contains("a stale startup screen's attention was not looked for"))
    })
    .await;
    wait_until("the initial passive policy answered", || {
        policy_reads(&h.fake) > 0
    })
    .await;
    assert!(!h.sup.held() && !h.sup.acting());
    h
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_attention_retry_survives_an_initial_read_refusal_without_a_claim() {
    for mode in ["off", "unproven", "unsupported"] {
        let h = refused_startup_attention(mode, false).await;
        wait_until(
            "startup attention clears after the silent read recovery",
            || !h.fake.attention().contains_key("codex-alab"),
        )
        .await;
        assert!(
            !h.sup.held() && !h.sup.acting(),
            "{mode}: {:?}",
            h.sup.journal()
        );
        assert!(h.fake.acts().is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_attention_retry_survives_an_initial_clear_refusal_without_a_claim() {
    for mode in ["off", "unproven", "unsupported"] {
        let h = refused_startup_attention(mode, true).await;
        wait_until(
            "startup attention clears after the silent clear recovery",
            || !h.fake.attention().contains_key("codex-alab"),
        )
        .await;
        assert!(
            !h.sup.held() && !h.sup.acting(),
            "{mode}: {:?}",
            h.sup.journal()
        );
        assert!(h.fake.acts().is_empty());
    }
}

fn startup_meta_reads(fake: &FakeAterm) -> usize {
    fake.requests()
        .iter()
        .filter(|line| line.ends_with(" meta"))
        .count()
}

async fn startup_unknown_metadata(reply: &str) -> Harness {
    let mut h = harness_with(
        Duration::from_millis(300),
        Some(Duration::from_secs(3600)),
        |fake| {
            fake.set_effective(false, "autopilot-off");
            fake.set_attention_as(
                "codex-alab",
                "login: signed out; codex's sign-in screen waits for a person",
            );
            fake.reply_once(&format!("@{} meta", fake.sid()), reply);
        },
    )
    .await;
    h._dir.disable_cleanup(true);
    eprintln!(
        "retained incomplete startup metadata fixture: {}",
        h._dir.path().display()
    );
    wait_until("the initial Off policy has answered", || {
        policy_reads(&h.fake) > 0
    })
    .await;
    h
}

/// An empty decoded value is not evidence that a new own reason replaced the startup
/// key. The fake still holds the old entry; a later canonical read must recover it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_empty_attention_metadata_retries_while_off() {
    for value in ["", " \t"] {
        let reply = format!(
            "OK attention={} attention_owner=codex-alab supervisor=-\n",
            codex_alab_aterm::wire::pct_encode(value)
        );
        let h = startup_unknown_metadata(&reply).await;
        wait_until(
            "startup cleanup recovers after an empty attention token",
            || !h.fake.attention().contains_key("codex-alab"),
        )
        .await;
        assert!(!h.sup.held() && h.fake.acts().is_empty());
    }
}

/// An absent or empty holder is unknown: a canonical read must precede any clear.
/// The explicit `-` already says there is no holder and permits the initial clear.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_supervisor_metadata_distinguishes_unknown_from_known_empty() {
    let old = codex_alab_aterm::wire::pct_encode(
        "login: signed out; codex's sign-in screen waits for a person",
    );
    for holder in [None, Some(""), Some(" \t"), Some("-")] {
        let field = holder
            .map(|holder| format!(" supervisor={}", codex_alab_aterm::wire::pct_encode(holder)))
            .unwrap_or_default();
        let reply = format!("OK attention={old} attention_owner=codex-alab{field}\n");
        let h = startup_unknown_metadata(&reply).await;
        wait_until("startup cleanup recovers once the holder is known", || {
            !h.fake.attention().contains_key("codex-alab")
        })
        .await;
        let requests = h.fake.requests();
        let clear = requests
            .iter()
            .position(|line| line.ends_with(" meta unset attention owner=codex-alab"))
            .expect("the startup clear was recorded");
        let reads = requests[..clear]
            .iter()
            .filter(|line| line.ends_with(" meta"))
            .count();
        if holder == Some("-") {
            assert_eq!(reads, 1, "a known empty holder: {requests:?}");
        } else {
            assert!(
                reads >= 2,
                "{holder:?}: unknown holder metadata cannot authorize a clear: {requests:?}"
            );
        }
        assert!(!h.sup.held() && h.fake.acts().is_empty());
    }
}

/// The independent retry reads current metadata: a wall reported since startup is never
/// cleared just because the first attempt saw a screen. The publication lane is the barrier.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_attention_retry_leaves_a_fresh_nonstartup_replacement() {
    let h = refused_startup_attention("off", true).await;
    let fresh = "question: a fresh wall belongs to the current session";
    h.fake.set_attention_as("codex-alab", fresh);
    wait_until("the retry has read current metadata", || {
        startup_meta_reads(&h.fake) >= 2
    })
    .await;
    h.sup.attention("barrier", None).await;
    assert_eq!(
        h.fake.attention().get("codex-alab").map(String::as_str),
        Some(fresh)
    );
    assert_eq!(
        h.fake
            .requests()
            .iter()
            .filter(|line| line.ends_with(" meta unset attention owner=codex-alab"))
            .count(),
        1,
        "only the initially refused clear was sent"
    );
    assert!(!h.sup.held() && h.fake.acts().is_empty());
}

/// Another visible owner can hide the old startup key. Its unchanged entry is no reason
/// to clear it or to retire cleanup; when visibility returns silently, the clock finishes it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_attention_retry_recovers_when_other_owner_visibility_returns() {
    let old = "login: signed out; codex's sign-in screen waits for a person";
    let mut h = harness_with(
        Duration::from_millis(300),
        Some(Duration::from_secs(3600)),
        |fake| {
            fake.set_effective(false, "autopilot-off");
            fake.set_attention_as("codex-alab", old);
            fake.set_attention_as("reviewer", "a reviewer is looking");
        },
    )
    .await;
    h._dir.disable_cleanup(true);
    eprintln!(
        "retained hidden startup attention fixture: {}",
        h._dir.path().display()
    );
    wait_until(
        "the hidden key is read again without policy activity",
        || startup_meta_reads(&h.fake) >= 2,
    )
    .await;
    h.sup.attention("barrier", None).await;
    assert_eq!(
        h.fake.attention().get("reviewer").map(String::as_str),
        Some("a reviewer is looking")
    );
    assert_eq!(
        h.fake.attention().get("codex-alab").map(String::as_str),
        Some(old)
    );
    assert!(
        !h.fake
            .requests()
            .iter()
            .any(|line| line.ends_with(" meta unset attention owner=codex-alab"))
    );
    h.fake.clear_attention_quietly("reviewer");
    wait_until("the now-visible startup key clears", || {
        !h.fake.attention().contains_key("codex-alab")
    })
    .await;
    assert!(!h.sup.held() && h.fake.acts().is_empty());
}

/// An unanswered first read closes its socket. The next connection carries the same
/// pending cleanup without relying on a policy timer or a proven claim.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_attention_retry_survives_a_closed_first_connection_while_off() {
    let mut h = harness_with(
        Duration::from_millis(100),
        Some(Duration::from_secs(3600)),
        |fake| {
            fake.set_effective(false, "autopilot-off");
            fake.set_attention_as(
                "codex-alab",
                "login: signed out; codex's sign-in screen waits for a person",
            );
            fake.stall("meta");
        },
    )
    .await;
    h._dir.disable_cleanup(true);
    eprintln!(
        "retained closed startup attention fixture: {}",
        h._dir.path().display()
    );
    wait_until("the first startup read timed out", || {
        h.sup
            .journal()
            .iter()
            .any(|line| line.contains("a stale startup screen's attention was not looked for"))
    })
    .await;
    let connections = h.fake.connections();
    h.fake.unstall("meta");
    wait_until("the reconnected startup cleanup clears", || {
        !h.fake.attention().contains_key("codex-alab")
    })
    .await;
    assert!(h.fake.connections() > connections);
    assert!(!h.sup.held() && h.fake.acts().is_empty());
}

/// Once a proven claim reconciles the old entry, a same-word current reason belongs to
/// that claim. Neither the pending startup timer nor a later reconnect can erase it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_attention_retry_retires_at_a_claim_and_keeps_current_same_words() {
    let h = refused_startup_attention("off", true).await;
    h.fake.set_effective(true, "enabled");
    wait_until("the first claim acts", || h.sup.acting()).await;
    let fresh = "login: signed out; codex's sign-in screen waits for a person";
    h.sup.attention("current", Some(fresh.into())).await;
    assert_eq!(
        h.fake.attention().get("codex-alab").map(String::as_str),
        Some(fresh)
    );
    let sent = claims_sent(&h.fake);
    h.fake.end_subscriptions();
    wait_until("the claim reconnects", || {
        claims_sent(&h.fake) > sent && h.sup.acting()
    })
    .await;
    h.sup.attention("barrier", None).await;
    assert_eq!(
        h.fake.attention().get("codex-alab").map(String::as_str),
        Some(fresh)
    );
}

/// The launch probe (F1, review of 6efbf7518): the first `status` read, made before the
/// claim, is kept as the start's witness of aterm's relaunch (its harness's hand, the person's
/// last keystroke), and its connection is the one the claim then uses.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_first_status_read_is_kept_as_the_launch_witness() {
    use codex_alab_core::session::HumanSeen;
    codex_alab_core::activate();
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .unwrap_or_else(|err| panic!("fake aterm: {err}"));
    fake.set_policy_key("human_grace_s", "1");
    // aterm's relaunch: its harness's hard lease on the tab, the person quiet for 10 min.
    fake.set_status("hand", "lease:aterm-harness%40812");
    fake.set_status("hand_pid", "812");
    fake.set_status("human_ms", "600000");
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let endpoint = fake.endpoint();
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(move || Some(endpoint.clone())),
        client: ClientOptions::default(),
    });
    let before = now_ms();
    sup.start();
    let probed =
        tokio::task::spawn_blocking(move || shared.wait_launch_probed(Duration::from_secs(60)))
            .await
            .unwrap_or_else(|err| panic!("probe: {err}"));
    assert!(probed, "{:?}", sup.journal());
    let witness = shared
        .launch_witness()
        .unwrap_or_else(|| panic!("no witness: {:?}", sup.journal()));
    assert_eq!(witness.hand.as_deref(), Some("lease:aterm-harness@812"));
    assert_eq!(
        witness.hand_pid,
        codex_alab_core::session::HandPid::Pid(812)
    );
    assert!(witness.read_at_ms >= before);
    assert!(
        matches!(witness.human, HumanSeen::At(at) if at + 600_000 == witness.read_at_ms),
        "{witness:?}"
    );
    // The hand lifts (aterm's relaunch is over): the start's witness stands as read.
    fake.set_status("hand", "-");
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
    assert!(fake.wait_for(proven).await, "{:?}", sup.journal());
    assert_eq!(
        shared.launch_witness().and_then(|w| w.hand),
        Some("lease:aterm-harness@812".to_string())
    );
    // The probe's connection is the one the claim is bound to (the first made); the event
    // stream is the second. Short reads (the mail badge) go on connections of their own.
    assert_eq!(
        fake.claim().and_then(|c| c.conn),
        Some(1),
        "{:?}",
        fake.requests()
    );
    assert!(fake.connections() >= 2, "{:?}", fake.requests());
    // Up to the claim, one request connection (the probe's, kept for the claim) and the event
    // stream's: each connection opens with `version`. (Later ones are tasks' own, such as the
    // mail badge's read right after the claim.)
    let requests = fake.requests();
    let claimed = requests
        .iter()
        .position(|r| r.contains("meta set supervisor codex-alab@"))
        .unwrap_or_else(|| panic!("no claim: {requests:?}"));
    let opened = requests[..claimed]
        .iter()
        .filter(|r| r.as_str() == "version")
        .count();
    assert_eq!(opened, 2, "{requests:?}");
}

/// No aterm reached at the start: the probe is done and witnesses nothing (no carry-on).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unreachable_aterm_leaves_no_launch_witness() {
    codex_alab_core::activate();
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .unwrap_or_else(|err| panic!("fake aterm: {err}"));
    let mut endpoint = fake.endpoint();
    endpoint.sock = dir.path().join("no-such.sock");
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let sup = Supervisor::new(SupervisorOptions {
        shared,
        endpoint: Arc::new(move || Some(endpoint.clone())),
        client: ClientOptions::default(),
    });
    sup.start();
    let probed =
        tokio::task::spawn_blocking(move || shared.wait_launch_probed(Duration::from_secs(60)))
            .await
            .unwrap_or_else(|err| panic!("probe: {err}"));
    assert!(probed, "{:?}", sup.journal());
    assert_eq!(shared.launch_witness(), None);
}

/// THE RECORD IS WRITTEN THROUGH BEFORE THE CLAIM (CX-B B1): aterm reads codex-alab's record
/// at the claim and relaunches a crash only on the line its copy names, and the next start
/// takes the act the record on disk carries. The launch probe answers before any claim, so the
/// claim waits for the record's write-through: what the probe settled (the line, the act its
/// proof lets the record carry) is on disk before aterm reads it. Seen live in parity-3's
/// land8: a SIGKILL 2 ms after the claim beat the coalesced write that would have carried the
/// owed act. NEGATIVE CONTROL (run as a mutant without the write): no write-through comes
/// between the probe and the claim.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_record_is_written_through_before_the_claim() {
    codex_alab_core::activate();
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let fake: &'static FakeAterm = Box::leak(Box::new(
        FakeAterm::start(dir.path(), Mode::Current)
            .await
            .unwrap_or_else(|err| panic!("fake aterm: {err}")),
    ));
    fake.set_policy_key("human_grace_s", "1");
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    // Each write-through: whether the probe had answered, and the claims aterm had been sent.
    let writes: Arc<std::sync::Mutex<Vec<(bool, usize)>>> = Arc::default();
    let seen = Arc::clone(&writes);
    assert!(shared.set_flush(Box::new(move || {
        seen.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((shared.launch_probed(), claims_sent(fake)));
        true
    })));
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
    assert!(fake.wait_for(proven).await, "{:?}", sup.journal());
    let writes = writes
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert!(
        writes.contains(&(true, 0)),
        "written after the probe, before the claim: {writes:?}"
    );
}
