// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use pretty_assertions::assert_eq;

use crate::ActOutcome;
use crate::ClaimOutcome;
use crate::Client;
use crate::Endpoint;
use crate::Error;
use crate::Event;
use crate::Options;
use crate::PolicyAnswer;
use crate::Signal;
use crate::claim_signal;
use crate::fake::FakeAterm;
use crate::fake::Mode;
use crate::policy::Approve;
use crate::policy::ApproveLevel;
use crate::policy::Questions;
use crate::status::HumanInput;
use crate::status::NotReady;
use crate::status::Status;
use crate::wire;

/// Short deadline for the fake: the contract's 5 s is the default, not a test budget.
fn options() -> Options {
    Options {
        timeout: Duration::from_millis(500),
        ..Options::default()
    }
}

async fn fake(mode: Mode) -> (tempfile::TempDir, FakeAterm) {
    let dir = tempfile::tempdir().expect("scratch dir");
    let fake = FakeAterm::start(dir.path(), mode)
        .await
        .expect("fake aterm");
    (dir, fake)
}

async fn client(fake: &FakeAterm) -> Client {
    Client::connect(fake.endpoint(), options())
        .await
        .expect("connect")
}

fn me() -> String {
    crate::holder_for(std::process::id())
}

/// A frame within a minute (a hang detector).
async fn next_event(events: &mut crate::Events) -> Event {
    tokio::time::timeout(Duration::from_secs(60), events.next())
        .await
        .expect("an event")
        .expect("stream open")
}

#[tokio::test]
async fn connects_authenticates_and_reads_whoami_and_status() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    assert_eq!(c.server_version(), "version=0.0.0-fake");
    let who = c.whoami().await.expect("whoami");
    assert_eq!(who.session, fake.sid());
    assert!(who.is_owner());
    let status = c.status().await.expect("status");
    assert_eq!(status.schema, 1);
    assert!(!status.hold);
    assert_eq!(status.hand, None);
    assert_eq!(status.program.as_deref(), Some("codex"));
    assert_eq!(status.get("subject").as_deref(), Some("fake session"));
    assert_eq!(status.supervisor, None);
    assert_eq!(status.supervisor_proven, Some(false));
    // `@self` went out as `@<sid>`.
    assert!(fake.requests().contains(&format!("@{} status", fake.sid())));
}

#[tokio::test]
async fn the_kernel_names_the_process_serving_the_socket() {
    // The fake serves from this very process: the connection's peer is this pid, which is
    // how a lease holder's name is checked against who really serves aterm's socket.
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    assert_eq!(c.server_pid(), Some(std::process::id()));
}

#[tokio::test]
async fn a_wrong_token_is_refused_at_connect() {
    let (dir, fake) = fake(Mode::Current).await;
    let token = std::fs::read_dir(dir.path())
        .expect("dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "token"))
        .expect("token file");
    std::fs::write(&token, "00\n").expect("overwrite token");
    match Client::connect(fake.endpoint(), options()).await {
        Err(Error::Refused(line)) => assert_eq!(line, "ERR auth"),
        Err(e) => panic!("expected ERR auth, got {e}"),
        Ok(_) => panic!("expected ERR auth, got a connection"),
    }
}

#[tokio::test]
async fn the_connection_is_close_on_exec() {
    let (_dir, fake) = fake(Mode::Current).await;
    let stream = tokio::net::UnixStream::connect(&fake.endpoint().sock)
        .await
        .expect("connect");
    crate::conn::set_cloexec(&stream).expect("cloexec");
    use std::os::fd::AsRawFd;
    // SAFETY: F_GETFD on a descriptor the stream owns.
    let flags = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFD) };
    assert!(flags >= 0 && flags & libc::FD_CLOEXEC != 0);
}

#[tokio::test]
async fn claims_only_after_policy_answered_ok_and_on() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    // No policy read yet: nothing is sent.
    let outcome = c.claim().await.expect("claim");
    assert!(matches!(outcome, ClaimOutcome::NotPermitted(_)));
    assert!(
        !fake
            .requests()
            .iter()
            .any(|r| r.contains("meta set supervisor"))
    );
    // Policy on: the claim goes out, is proven, and status says so.
    let PolicyAnswer::Supported(policy) = c.harness_policy().await.expect("policy") else {
        panic!("policy unsupported");
    };
    assert!(policy.effective);
    assert_eq!(
        c.claim().await.expect("claim"),
        ClaimOutcome::Held { changed: true }
    );
    let claim = fake.claim().expect("claim held");
    assert_eq!(claim.holder, me());
    assert_eq!(claim.proven_pid, Some(std::process::id()));
    let status = c.status().await.expect("status");
    assert!(status.proven_holder(&me()));
    // Re-confirm in the tick of an act: same holder, OK.
    assert!(c.reconfirm().await.expect("reconfirm"));
    assert_eq!(
        c.harness_act("continue").await.expect("act"),
        ActOutcome::Recorded
    );
    assert_eq!(fake.acts(), vec!["continue".to_string()]);
}

#[tokio::test]
async fn effective_off_never_claims() {
    let (_dir, fake) = fake(Mode::Current).await;
    fake.set_effective(false, "headless");
    let c = client(&fake).await;
    let PolicyAnswer::Supported(policy) = c.harness_policy().await.expect("policy") else {
        panic!("policy unsupported");
    };
    assert!(!policy.effective);
    assert_eq!(policy.why, "headless");
    assert!(matches!(
        c.claim().await.expect("claim"),
        ClaimOutcome::NotPermitted(_)
    ));
    assert_eq!(fake.claim(), None);
}

#[tokio::test]
async fn an_old_aterm_is_unsupported_and_never_claimed() {
    let (_dir, fake) = fake(Mode::Old).await;
    let c = client(&fake).await;
    assert_eq!(
        c.harness_policy().await.expect("policy"),
        PolicyAnswer::Unsupported("ERR unknown verb (try: help)".to_string())
    );
    assert!(matches!(
        c.claim().await.expect("claim"),
        ClaimOutcome::NotPermitted(_)
    ));
    assert!(
        !fake
            .requests()
            .iter()
            .any(|r| r.contains("meta set supervisor"))
    );
    // Its status has no supervisor_proven, which never reads as proven.
    let status = c.status().await.expect("status");
    assert_eq!(status.supervisor_proven, None);
    assert_eq!(
        c.harness_act("continue").await.expect("act"),
        ActOutcome::NotHeld
    );
}

#[tokio::test]
async fn an_err_policy_is_passive_and_forgets_an_earlier_ok() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    assert!(matches!(
        c.harness_policy().await.expect("policy"),
        PolicyAnswer::Supported(_)
    ));
    fake.answer_policy_with(Some("ERR denied"));
    assert_eq!(
        c.harness_policy().await.expect("policy"),
        PolicyAnswer::Unsupported("ERR denied".to_string())
    );
    assert_eq!(c.policy(), None);
    assert!(matches!(
        c.claim().await.expect("claim"),
        ClaimOutcome::NotPermitted(_)
    ));
}

#[tokio::test]
async fn a_proven_claim_replaces_the_host_and_names_any_other_holder_busy() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    c.harness_policy().await.expect("policy");
    fake.claim_as("aterm-harness@2668");
    assert_eq!(
        c.claim().await.expect("claim"),
        ClaimOutcome::Held { changed: true }
    );
    assert_eq!(fake.claim().map(|c| c.holder), Some(me()));
    c.release().await.expect("release");
    fake.claim_as("driver@lead");
    assert_eq!(
        c.claim().await.expect("claim"),
        ClaimOutcome::Busy {
            holder: "driver@lead".to_string()
        }
    );
    // Our release never clears another holder's claim.
    c.release().await.expect("release");
    assert_eq!(
        fake.claim().map(|c| c.holder),
        Some("driver@lead".to_string())
    );
}

#[tokio::test]
async fn an_unproven_claim_is_given_back_at_once() {
    let (_dir, fake) = fake(Mode::Current).await;
    // Not the foreground leader: aterm takes the claim but does not prove it.
    fake.set_foreground_leader(Some(1));
    let c = client(&fake).await;
    c.harness_policy().await.expect("policy");
    assert_eq!(c.claim().await.expect("claim"), ClaimOutcome::Unproven);
    assert_eq!(fake.claim(), None);
    assert!(!c.holds_claim());

    // A holder naming another pid proves nothing either.
    fake.set_foreground_leader(Some(std::process::id()));
    let forged = Client::connect(
        fake.endpoint(),
        Options {
            holder_pid: std::process::id() + 1,
            ..options()
        },
    )
    .await
    .expect("connect");
    forged.harness_policy().await.expect("policy");
    assert_eq!(forged.claim().await.expect("claim"), ClaimOutcome::Unproven);
    assert_eq!(fake.claim(), None);
}

#[tokio::test]
async fn the_claim_ends_with_the_connection() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    c.harness_policy().await.expect("policy");
    assert_eq!(
        c.claim().await.expect("claim"),
        ClaimOutcome::Held { changed: true }
    );
    drop(c);
    assert!(fake.wait_for(|f| f.claim().is_none()).await);
}

#[tokio::test]
async fn the_watch_goes_passive_on_a_supervisor_change_not_naming_us() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    let mut events = c.subscribe().await.expect("subscribe");
    assert!(fake.wait_for(|f| f.subscribers() == 1).await);
    c.harness_policy().await.expect("policy");
    assert_eq!(
        c.claim().await.expect("claim"),
        ClaimOutcome::Held { changed: true }
    );
    // The subscribe stream is a second connection: the claim survived it.
    assert_eq!(fake.claim().map(|c| c.holder), Some(me()));
    let ours = next_event(&mut events).await;
    assert_eq!(claim_signal(&ours, &me()), Signal::Ours);

    // Someone clears it and another holder takes it.
    fake.claim_as("driver@lead");
    let lost = next_event(&mut events).await;
    assert_eq!(
        claim_signal(&lost, &me()),
        Signal::Lost {
            by: Some("driver@lead".to_string())
        }
    );
    c.note_claim_lost();
    assert!(!c.holds_claim());
    assert_eq!(
        c.harness_act("continue").await.expect("act"),
        ActOutcome::NotHeld
    );
    assert!(!c.reconfirm().await.expect("reconfirm"));
}

#[tokio::test]
async fn a_policy_move_is_announced_and_a_dead_pid_clears_the_claim() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    let mut events = c.subscribe().await.expect("subscribe");
    assert!(fake.wait_for(|f| f.subscribers() == 1).await);
    fake.set_policy_key("continue_text", "carry on");
    assert_eq!(
        claim_signal(&next_event(&mut events).await, &me()),
        Signal::PolicyChanged {
            generation: Some(2)
        }
    );
    let PolicyAnswer::Supported(policy) = c.harness_policy().await.expect("policy") else {
        panic!("policy unsupported");
    };
    assert_eq!(policy.generation, 2);
    assert_eq!(policy.continue_text(), Some("carry on"));
    c.claim().await.expect("claim");
    next_event(&mut events).await; // our own claim
    fake.kill_pid(std::process::id());
    assert_eq!(
        claim_signal(&next_event(&mut events).await, &me()),
        Signal::Lost { by: None }
    );
}

#[tokio::test]
async fn a_timeout_is_unanswered_and_closes_the_connection() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    c.harness_policy().await.expect("policy");
    assert_eq!(
        c.claim().await.expect("claim"),
        ClaimOutcome::Held { changed: true }
    );
    fake.stall("status");
    assert!(matches!(c.status().await, Err(Error::Unanswered)));
    assert!(!c.is_open());
    assert!(!c.holds_claim());
    assert!(matches!(c.whoami().await, Err(Error::Closed)));
    // The server released the claim with the connection.
    assert!(fake.wait_for(|f| f.claim().is_none()).await);
}

#[tokio::test]
async fn a_stalled_policy_read_is_an_error_never_a_claim() {
    let (_dir, fake) = fake(Mode::Current).await;
    fake.stall("harness");
    let c = client(&fake).await;
    assert!(matches!(c.harness_policy().await, Err(Error::Unanswered)));
    assert!(matches!(
        c.claim().await.expect("claim"),
        ClaimOutcome::NotPermitted(_)
    ));
    assert_eq!(fake.claim(), None);
}

#[tokio::test]
async fn attention_and_notices_are_keyed_and_single_line() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    c.set_attention("approval", "a box\nleft\u{202E} open")
        .await
        .expect("attention");
    assert_eq!(
        fake.attention().get("codex-alab").map(String::as_str),
        Some("approval: a box left open")
    );
    c.clear_attention().await.expect("clear");
    assert!(fake.attention().is_empty());
    c.appnotice_harness("continued the turn")
        .await
        .expect("notice");
    assert_eq!(fake.notices(), vec!["continued the turn".to_string()]);
    let long = "x".repeat(400);
    c.set_attention("login", &long).await.expect("attention");
    assert_eq!(fake.attention()["codex-alab"].len(), 200);
}

#[tokio::test]
async fn an_old_aterm_has_no_harness_act() {
    let (_dir, fake) = fake(Mode::Old).await;
    let c = client(&fake).await;
    // An old aterm takes the claim but never proves it, so nothing is acted under it.
    c.request(&format!("@{} meta set supervisor {}", fake.sid(), me()))
        .await
        .expect("set");
    assert_eq!(
        c.harness_act("continue").await.expect("act"),
        ActOutcome::NotHeld
    );
    let reply = c
        .request(&format!("@{} meta set harness_act continue", fake.sid()))
        .await
        .expect("act");
    assert!(reply.status.starts_with("ERR unknown meta field"));
}

#[tokio::test]
async fn lines_framing_reads_the_counted_rows() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    let reply = c.request_lines("sessions").await.expect("sessions");
    assert_eq!(reply.status, "OK 1");
    assert_eq!(reply.lines, vec![format!("{} program=codex", fake.sid())]);
    assert!(matches!(
        c.request("bad\nline").await,
        Err(Error::BadRequest(_))
    ));
}

#[tokio::test]
async fn locate_reads_the_graph_entry_and_refuses_odd_sids() {
    let (_dir, fake) = fake(Mode::Current).await;
    let found = crate::locate::locate_in(fake.dir(), &fake.sid()).expect("located");
    assert_eq!(found, fake.endpoint());
    assert_eq!(crate::locate::locate_in(fake.dir(), "s-../../etc"), None);
    assert_eq!(crate::locate::locate_in(fake.dir(), "s-"), None);
    assert_eq!(crate::locate::locate_in(fake.dir(), "s-abcdef"), None);
    let c = Client::connect(found, options()).await.expect("connect");
    assert!(c.whoami().await.is_ok());
}

#[test]
fn token_names_follow_aterm() {
    use crate::locate::token_names_for_sock;
    assert_eq!(
        token_names_for_sock("aterm-2668.sock"),
        vec!["aterm-2668.token"]
    );
    assert_eq!(
        token_names_for_sock("private.sock"),
        vec!["private.sock.token", "aterm.token"]
    );
    assert_eq!(
        token_names_for_sock("aterm"),
        vec!["aterm-sock-aterm.token", "aterm.token"]
    );
}

#[test]
fn pct_fields_decode_as_aterm_writes_them() {
    assert_eq!(
        wire::pct_decode("%E2%97%90%20Codex%2Dalab"),
        "\u{25D0} Codex-alab"
    );
    assert_eq!(wire::pct_decode("100%25"), "100%");
    assert_eq!(wire::pct_decode("50%zz%4"), "50%zz%4");
    assert_eq!(wire::pct_decode("a%0Ab"), "a\nb");
    assert_eq!(
        wire::pct_encode("keep going\n100%"),
        "keep%20going%0A100%25"
    );
    assert_eq!(
        wire::pct_decode(&wire::pct_encode("\u{25D0} x=y")),
        "\u{25D0} x=y"
    );

    let status = Status::parse(
        "schema=1 hold=0 hand=lease:codex-alab@7 input=clear human_ms=130000 \
         supervisor=codex-alab%407 supervisor_proven=1 attention_owner=%2D future=x%20y",
    )
    .expect("status");
    assert_eq!(status.supervisor.as_deref(), Some("codex-alab@7"));
    assert_eq!(status.get("attention_owner").as_deref(), Some("-"));
    assert_eq!(status.get("future").as_deref(), Some("x y"));
    assert_eq!(status.human, HumanInput::Ago(130_000));
    assert_eq!(
        status.ready_for_act("codex-alab@7", Duration::from_secs(120)),
        Ok(())
    );
    assert_eq!(
        status.ready_for_act("codex-alab@7", Duration::from_secs(300)),
        Err(NotReady::Human(HumanInput::Ago(130_000)))
    );

    let PolicyAnswer::Supported(policy) = PolicyAnswer::parse(
        "OK gen=7 effective=on why=enabled approve=safe questions=long-term \
         continue_text=keep%20going%0A%0ARules%3A%20be%20brief answer_text=- \
         human_grace_s=30 trust_roots=~/aterm*,%20/tmp continue_per_hour=4",
    ) else {
        panic!("unsupported");
    };
    assert_eq!(policy.generation, 7);
    assert_eq!(
        policy.continue_text(),
        Some("keep going\n\nRules: be brief")
    );
    assert_eq!(policy.answer_text(), None);
    assert_eq!(policy.approve(), Approve::None);
    // The folder-trust answer reads `safe` as aterm wrote it; the approvals above do not.
    assert_eq!(policy.approve_level(), ApproveLevel::Safe);
    assert_eq!(policy.questions(), Questions::LongTerm);
    assert_eq!(policy.human_grace(), Duration::from_secs(30));
    assert_eq!(policy.trust_roots(), vec!["~/aterm*", "/tmp"]);
    assert_eq!(policy.continue_per_hour(), Some(4));
}

#[test]
fn status_and_policy_fail_closed() {
    // Missing hold/hand/input/human read as "do not act".
    let bare = Status::parse("schema=1").expect("status");
    assert!(bare.hold);
    assert_eq!(
        bare.ready_for_act("codex-alab@7", Duration::ZERO),
        Err(NotReady::Hold)
    );
    let held = Status::parse("schema=1 hold=0 hand=turn:4:lead input=clear human_ms=-").expect("s");
    assert_eq!(
        held.ready_for_act("codex-alab@7", Duration::ZERO),
        Err(NotReady::Hand("turn:4:lead".to_string()))
    );
    let unproven =
        Status::parse("schema=1 hold=0 hand=- input=clear human_ms=- supervisor=codex-alab%407")
            .expect("s");
    assert_eq!(
        unproven.ready_for_act("codex-alab@7", Duration::ZERO),
        Err(NotReady::Unproven)
    );
    let pending = Status::parse("schema=1 hold=0 hand=- input=pending").expect("s");
    assert_eq!(
        pending.ready_for_act("codex-alab@7", Duration::ZERO),
        Err(NotReady::Input("pending".to_string()))
    );
    assert!(Status::parse("schema=2 hold=0").is_err());
    assert!(Status::parse("hold=0").is_err());

    // `hand_pid=`: the lease's kernel-attested taker; absent on an aterm without it, and
    // `-` or anything unreadable names nobody.
    use codex_alab_core::session::HandPid;
    let pid = |line: &str| Status::parse(line).expect("status").hand_pid;
    assert_eq!(pid("schema=1 hand=lease:x"), HandPid::Absent);
    assert_eq!(pid("schema=1 hand=lease:x hand_pid=812"), HandPid::Pid(812));
    for raw in ["-", "0", "x", "-3", "812a", ""] {
        assert_eq!(
            pid(&format!("schema=1 hand=lease:x hand_pid={raw}")),
            HandPid::Unknown,
            "{raw}"
        );
    }

    for line in [
        "OK effective=on",
        "OK gen=x effective=on",
        "ERR denied",
        "OK",
    ] {
        assert!(
            matches!(PolicyAnswer::parse(line), PolicyAnswer::Unsupported(_)),
            "{line}"
        );
    }
    let PolicyAnswer::Supported(odd) = PolicyAnswer::parse("OK gen=1 effective=yes why=-") else {
        panic!("unsupported");
    };
    assert!(!odd.effective);
    assert_eq!(odd.approve_level(), ApproveLevel::None);
    assert_eq!(odd.questions(), Questions::Ask);
    assert_eq!(odd.human_grace(), Duration::from_secs(3600));
    assert_eq!(odd.continue_per_hour(), Some(0));
}

#[test]
fn frames_parse_and_only_supervisor_moves_touch_the_claim() {
    let me = "codex-alab@7";
    let ev = crate::events::parse_frame("EVENT 1 meta field=supervisor value=-", "1").expect("ev");
    assert_eq!(claim_signal(&ev, me), Signal::Lost { by: None });
    let ev = crate::events::parse_frame("EVENT 1 meta field=supervisor value=codex-alab%407", "1")
        .expect("ev");
    assert_eq!(claim_signal(&ev, me), Signal::Ours);
    let ev = crate::events::parse_frame("EVENT 1 meta field=title value=x", "1").expect("ev");
    assert_eq!(claim_signal(&ev, me), Signal::Nothing);
    let ev = crate::events::parse_frame("GAP 1 events-dropped=3", "1").expect("ev");
    assert_eq!(claim_signal(&ev, me), Signal::Lost { by: None });
    assert_eq!(crate::events::parse_frame("EVENT 2 human", "1"), None);
    assert_eq!(crate::events::parse_frame("T 1 123", "1"), None);
    let ev = crate::events::parse_frame("EVENT 1 harness-policy gen=9", "1").expect("ev");
    assert_eq!(
        claim_signal(&ev, me),
        Signal::PolicyChanged {
            generation: Some(9)
        }
    );
    let ev = crate::events::parse_frame("EVENT 1 exited", "1").expect("ev");
    assert_eq!(claim_signal(&ev, me), Signal::Ended);
}

#[tokio::test]
async fn nothing_outside_codex_alab_or_aterm() {
    // Upstream's processes never activate the switch: no endpoint, no connection.
    if !codex_alab_core::active() {
        assert_eq!(Endpoint::from_env(), None);
        assert!(
            Client::connect_self()
                .await
                .expect("connect_self")
                .is_none()
        );
    }
}

/// Read-only smoke against the REAL aterm hosting this terminal: `whoami` and `status`
/// only. Never a claim, attention, notice or act. Skipped outside aterm.
#[tokio::test]
async fn real_aterm_read_only_smoke() {
    let Ok(sid) = std::env::var(crate::locate::SELF_SID_ENV) else {
        return;
    };
    let Some(endpoint) =
        crate::locate::control_socket_dir().and_then(|dir| crate::locate::locate_in(&dir, &sid))
    else {
        return;
    };
    let c = Client::connect(endpoint, Options::default())
        .await
        .expect("connect to the real aterm");
    let who = c.whoami().await.expect("whoami");
    assert_eq!(who.session, sid);
    let status = c.status().await.expect("status");
    assert_eq!(status.schema, 1);
    eprintln!(
        "real aterm {}: program={:?} supervisor={:?} proven={:?}",
        c.server_version(),
        status.program,
        status.supervisor,
        status.supervisor_proven
    );
    // An aterm without the proven self-claim prints no supervisor_proven=.
    if status.supervisor_proven.is_none() {
        assert!(!status.proven_holder(c.holder()));
    }
    c.close();
}

async fn held(fake: &FakeAterm) -> Client {
    let c = client(fake).await;
    assert!(matches!(
        c.harness_policy().await.expect("policy"),
        PolicyAnswer::Supported(_)
    ));
    assert_eq!(
        c.claim().await.expect("claim"),
        ClaimOutcome::Held { changed: true }
    );
    c
}

#[tokio::test]
async fn a_story_point_is_told_on_the_claim_connection() {
    let (_dir, fake) = fake(Mode::Current).await;
    let unclaimed = client(&fake).await;
    assert_eq!(
        unclaimed.story("approved", "").await.expect("story"),
        ActOutcome::NotHeld
    );
    let c = held(&fake).await;
    assert_eq!(
        c.story("approved", "").await.expect("story"),
        ActOutcome::Recorded
    );
    assert_eq!(
        c.story("chose", "recommended").await.expect("story"),
        ActOutcome::Recorded
    );
    // One line, cut to aterm's 96 bytes on a character boundary.
    let long = format!("waiting\nfor the usage reset {}", "é".repeat(80));
    assert_eq!(
        c.story("warned", &long).await.expect("story"),
        ActOutcome::Recorded
    );
    let stories = fake.stories();
    assert_eq!(stories[0], ("approved".to_string(), String::new()));
    assert_eq!(stories[1], ("chose".to_string(), "recommended".to_string()));
    assert_eq!(stories[2].0, "warned");
    assert!(stories[2].1.starts_with("waiting for the usage reset é"));
    assert!(stories[2].1.len() <= crate::STORY_MAX_BYTES);
    assert!(
        fake.requests()
            .iter()
            .any(|r| r == &format!("@{} story approved", fake.sid()))
    );
    // A verb outside aterm's closed set is never sent.
    let sent = fake.requests().len();
    assert!(matches!(
        c.story("continued", "").await,
        Err(Error::BadRequest(_))
    ));
    assert_eq!(fake.requests().len(), sent);
}

#[tokio::test]
async fn an_old_aterm_has_no_story_and_the_client_says_so() {
    let (_dir, fake) = fake(Mode::Old).await;
    let c = client(&fake).await;
    let reply = c
        .request(&format!("@{} story approved", fake.sid()))
        .await
        .expect("story");
    assert!(reply.status.starts_with("ERR unknown verb"));
    // And unclaimed (an old aterm never proves a claim), nothing is told.
    assert_eq!(
        c.story("approved", "").await.expect("story"),
        ActOutcome::NotHeld
    );
}

#[tokio::test]
async fn a_typed_trigger_reads_its_row_and_body_without_listing_anything() {
    use codex_alab_core::mail::TriggerKind;
    let (_dir, fake) = fake(Mode::Current).await;
    fake.deliver("h-andrew", "note", "human", "first");
    let (id, off) = fake.deliver(
        "s-lead@n-lab",
        "task",
        "agent",
        "Run the tests.\nThen push.\n",
    );
    let c = client(&fake).await;
    let mail = crate::mail::read(&c, TriggerKind::Task, off, &|_| None)
        .await
        .expect("mail");
    assert_eq!(mail.id, id);
    assert_eq!(mail.off, off);
    assert_eq!(mail.from, "s-lead@n-lab");
    assert_eq!(mail.kind, "task");
    assert_eq!(mail.trust, "agent");
    assert_eq!(mail.body, "Run the tests.\nThen push.\n");
    assert_eq!(mail.cut_from, None);
    // Read by peek and by id: no `inbox get @<off>` (which may park for the bridge), and
    // nothing marked seen.
    let reqs = fake.requests();
    assert!(
        reqs.iter().any(|r| r.ends_with("inbox --peek --meta")),
        "{reqs:?}"
    );
    assert!(
        reqs.iter().any(|r| r.ends_with(&format!("inbox get {id}"))),
        "{reqs:?}"
    );
    assert!(!reqs.iter().any(|r| r.contains("inbox get @")), "{reqs:?}");
    assert!(fake.receipts().is_empty());
    // Negative controls: the line names another kind, or an offset that is no row here.
    assert!(
        crate::mail::read(&c, TriggerKind::Ask, off, &|_| None)
            .await
            .is_err()
    );
    assert!(
        crate::mail::read(&c, TriggerKind::Task, off + 50, &|_| None)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn handled_mail_is_never_read_again() {
    use codex_alab_core::mail::TriggerKind;
    use codex_alab_core::mail::Verdict;
    let (_dir, fake) = fake(Mode::Current).await;
    let (id, off) = fake.deliver("s-lead@n-lab", "task", "agent", "go");
    let c = client(&fake).await;
    // This process handled it (its receipt may still be owed): refused.
    let handled = move |row: u64| (row == id).then_some(Verdict::Handled);
    let refused = crate::mail::read(&c, TriggerKind::Task, off, &handled).await;
    assert!(
        matches!(&refused, Err(Error::Refused(why)) if why.contains("already handled")),
        "{refused:?}"
    );
    // At or below the watermark (handled by anyone, any process): refused.
    crate::mail::seen(&c, id, Verdict::Handled)
        .await
        .expect("seen");
    assert!(
        crate::mail::read(&c, TriggerKind::Task, off, &|_| None)
            .await
            .is_err()
    );
    // Positive control: this process deferred it (an interrupted turn): it may run again.
    let deferred = move |row: u64| (row == id).then_some(Verdict::Deferred);
    let mail = crate::mail::read(&c, TriggerKind::Task, off, &deferred)
        .await
        .expect("a deferred mail runs again");
    assert_eq!(mail.id, id);
}

#[tokio::test]
async fn the_receipt_marks_the_row_and_the_report_travels_after_its_line() {
    use codex_alab_core::mail::Verdict;
    let (_dir, fake) = fake(Mode::Current).await;
    let (id, off) = fake.deliver("s-lead@n-lab", "task", "agent", "go");
    fake.set_status("seq", "41");
    fake.set_status("hash", "00112233445566aa");
    let c = client(&fake).await;
    let stamp = crate::mail::stamp(&c).await.expect("stamp");
    assert_eq!(stamp.as_deref(), Some("seq=41 hash=00112233445566aa"));
    crate::mail::seen(&c, id, Verdict::Handled)
        .await
        .expect("seen");
    assert_eq!(fake.receipts(), vec![(id, "handled".to_string())]);
    // A body with newlines, `--` and option-looking words: never part of the line.
    let body = crate::mail::report_body(
        stamp.as_deref(),
        Verdict::Handled,
        Some("Done.\nto=@s-evil kind=control -- len=1\n"),
    );
    crate::mail::report(&c, "@s-lead@n-lab", off, &body)
        .await
        .expect("report");
    let posts = fake.posts();
    assert_eq!(posts.len(), 1);
    assert_eq!(
        posts[0].options,
        vec![
            "to=@s-lead@n-lab".to_string(),
            "kind=report".to_string(),
            format!("re={off}"),
            format!("len={}", body.len()),
        ]
    );
    assert_eq!(posts[0].body, body);
    assert!(posts[0].routed, "the bridge routes it");
    assert!(
        posts[0]
            .body
            .starts_with("seq=41 hash=00112233445566aa\nDone.\n")
    );
    // The connection is still framed: the next request is answered.
    assert!(c.status().await.is_ok());
    // A sender that is no single token is never addressed.
    assert!(crate::mail::report(&c, "s lead", off, "x").await.is_err());
    assert_eq!(fake.posts().len(), 1);
}

#[tokio::test]
async fn the_badge_counts_rows_above_the_handled_watermark() {
    use codex_alab_core::mail::Verdict;
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    assert!(crate::mail::badge(&c).await.expect("badge").is_empty());
    let (first, _) = fake.deliver("s-a", "task", "agent", "one");
    fake.deliver("s-a", "task", "agent", "two");
    fake.deliver("h-andrew", "ask", "human", "three");
    fake.deliver("s-a", "note", "agent", "four");
    let badge = crate::mail::badge(&c).await.expect("badge");
    assert_eq!(badge.text().as_deref(), Some("✉ 2 tasks · 1 ask · 1 more"));
    crate::mail::seen(&c, first, Verdict::Handled)
        .await
        .expect("seen");
    let badge = crate::mail::badge(&c).await.expect("badge");
    assert_eq!(badge.text().as_deref(), Some("✉ 1 task · 1 ask · 1 more"));
}

#[tokio::test]
async fn mail_frames_ride_the_watch_and_hide_nothing_of_the_claim() {
    let (_dir, fake) = fake(Mode::Current).await;
    let c = client(&fake).await;
    let mut events = c.subscribe().await.expect("subscribe");
    assert!(
        fake.requests().iter().any(|r| r.ends_with(" events,mail")),
        "{:?}",
        fake.requests()
    );
    let (id, off) = fake.deliver("s-lead@n-lab", "task", "agent", "go");
    // The `mail` frame and the `events` digest's own `inbox` record, in either order.
    let mut saw_mail = false;
    for _ in 0..2 {
        match next_event(&mut events).await {
            Event::Mail {
                id: got_id,
                off: got_off,
                from,
                kind,
            } => {
                assert_eq!((got_id, got_off), (Some(id), Some(off)));
                assert_eq!(from.as_deref(), Some("s-lead@n-lab"));
                assert_eq!(kind.as_deref(), Some("task"));
                saw_mail = true;
            }
            Event::Other(line) => assert!(line.contains(" inbox "), "{line}"),
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(saw_mail);
    let me = "codex-alab@7";
    for line in [
        "GAP 1 mail-dropped=4",
        "EVENT 1 inbox-seen 3 off=9 floor=- seen=3",
    ] {
        let ev = crate::events::parse_frame(line, "1").expect("ev");
        assert_eq!(claim_signal(&ev, me), Signal::Nothing, "{line}");
    }
    assert_eq!(
        crate::events::parse_frame("GAP 1 mail-dropped=4", "1"),
        Some(Event::MailGap)
    );
    // Negative control: an events gap still hides what the claim's watch must see.
    let ev = crate::events::parse_frame("GAP 1 events-dropped=4", "1").expect("ev");
    assert_eq!(claim_signal(&ev, me), Signal::Lost { by: None });
}

#[tokio::test]
async fn an_aterm_without_the_mail_stream_still_gets_the_claims_watch() {
    let (_dir, fake) = fake(Mode::Current).await;
    fake.refuse_mail_stream();
    let c = client(&fake).await;
    let _events = c.subscribe().await.expect("subscribe");
    let reqs = fake.requests();
    assert!(reqs.iter().any(|r| r.ends_with(" events,mail")), "{reqs:?}");
    assert!(reqs.iter().any(|r| r.ends_with(" events")), "{reqs:?}");
    assert_eq!(fake.subscribers(), 1);
}
