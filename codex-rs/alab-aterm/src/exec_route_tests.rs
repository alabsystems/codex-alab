// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::fake::FakeAterm;
use crate::fake::Mode;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::fd::OwnedFd;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::process::Stdio;

/// Every route, each `Off` among them.
fn routes() -> Vec<NoRoute> {
    vec![
        NoRoute::Outside,
        NoRoute::Unreachable,
        NoRoute::NoExecContract,
        NoRoute::Off(Off::Hold),
        NoRoute::Off(Off::AutopilotOff),
        NoRoute::Off(Off::Other("agent-off".into())),
        NoRoute::Off(Off::Other(String::new())),
        NoRoute::RotationOff,
        NoRoute::NotLeader,
        NoRoute::Unproven,
        NoRoute::AnotherLeads,
        NoRoute::NotChatGpt,
        NoRoute::IdentityUnreadable,
        NoRoute::NoRootTurn,
        NoRoute::NotNeeded,
    ]
}

#[test]
fn each_line_is_one_line_that_names_no_label_email_or_path() {
    for route in routes() {
        let line = route.line();
        let body = line
            .strip_prefix(PREFIX)
            .and_then(|rest| rest.strip_suffix('\n'))
            .unwrap_or_else(|| panic!("{line:?}"));
        assert!(!body.is_empty(), "{line:?}");
        assert!(!body.contains(['\n', '\r', '@', '/', '~', '=']), "{line:?}");
        assert!(line.len() < 160, "{line:?}");
    }
    // A host's `why=` word is repeated only when it is short and plain.
    for why in [
        "a/b",
        "x@y.z",
        "Agent-Off",
        "two words",
        &"w".repeat(WHY_MAX + 1),
    ] {
        assert_eq!(Off::of(why), Off::Other(String::new()), "{why}");
    }
    assert_eq!(Off::of("agent-off"), Off::Other("agent-off".into()));
    assert_eq!(Off::of("hold"), Off::Hold);
    assert_eq!(Off::of("autopilot-off"), Off::AutopilotOff);
}

#[test]
fn only_a_route_that_can_still_move_says_the_move_waits() {
    // A hold, an Off, changed keys, the host's not-needed and a run that does not lead (a
    // job `fg` brings to the front) can still move this run at a later read: their lines
    // say the move waits, not that the run waits for its reset. Each is read again, and
    // every route read again can still move.
    for route in routes() {
        let line = route.line();
        let can_still_move = matches!(
            route,
            NoRoute::Off(_) | NoRoute::RotationOff | NoRoute::NotNeeded | NoRoute::NotLeader
        );
        let read_again = route.again().is_some() && route != NoRoute::NoRootTurn;
        assert_eq!(read_again, can_still_move, "{route:?}");
        assert_eq!(
            line.contains("waits for its usage limit to reset"),
            !can_still_move,
            "{line:?}"
        );
        assert_eq!(line.contains("the move waits"), can_still_move, "{line:?}");
    }
    assert!(NoRoute::Off(Off::Hold).line().contains("hold to lift"));
    assert!(
        NoRoute::NotLeader
            .line()
            .contains("waits for this run to lead it")
    );
}

#[test]
fn a_definite_route_stops_the_asking_and_a_hold_waits_for_the_passive_clock() {
    for route in [
        NoRoute::Outside,
        NoRoute::Unreachable,
        NoRoute::NoExecContract,
        NoRoute::Unproven,
        NoRoute::AnotherLeads,
        NoRoute::NotChatGpt,
        NoRoute::IdentityUnreadable,
    ] {
        assert_eq!(route.again(), None, "{route:?}");
    }
    for route in [
        NoRoute::Off(Off::Hold),
        NoRoute::Off(Off::AutopilotOff),
        NoRoute::Off(Off::Other(String::new())),
        NoRoute::RotationOff,
    ] {
        assert_eq!(route.again(), Some(PASSIVE), "{route:?}");
    }
    assert_eq!(NoRoute::NotNeeded.again(), Some(RETRY));
    assert_eq!(NoRoute::NotLeader.again(), Some(RETRY));
    assert_eq!(NoRoute::NoRootTurn.again(), Some(ROOT_RETRY));
}

fn login(account: &str) -> Source {
    Source::login(account, "fixture-user")
}

#[test]
fn the_latch_tells_once_per_wall_and_again_for_a_new_wall() {
    let wall = Wall::new(login("fixture-account"), Some("root-turn".into()));
    let mut latch = Latch::default();
    let first = latch.note(&wall, &NoRoute::NotLeader);
    assert_eq!(first, Some(NoRoute::NotLeader.line()));
    // A retry publishes a new observation of the same wall: the same key, no new line, and
    // another route of the same wall says nothing more either.
    let again = Wall::new(login("fixture-account"), Some("root-turn".into()));
    assert_eq!(latch.note(&again, &NoRoute::NotLeader), None);
    assert_eq!(latch.note(&again, &NoRoute::NotNeeded), None);
    assert!(!latch.settled(&again));
    // Another root turn, or another source, is another wall.
    let turn = Wall::new(login("fixture-account"), Some("next-turn".into()));
    assert_eq!(
        latch.note(&turn, &NoRoute::NotNeeded),
        Some(NoRoute::NotNeeded.line())
    );
    let account = Wall::new(login("other-account"), Some("next-turn".into()));
    assert_eq!(
        latch.note(&account, &NoRoute::NotNeeded),
        Some(NoRoute::NotNeeded.line())
    );
}

#[test]
fn a_definite_route_settles_its_wall_only() {
    let wall = Wall::new(login("fixture-account"), Some("root-turn".into()));
    let mut latch = Latch::default();
    assert!(!latch.settled(&wall));
    assert!(latch.note(&wall, &NoRoute::Off(Off::Hold)).is_some());
    assert!(!latch.settled(&wall), "a hold is read again");
    // An unproven claim settles the wall: a leader of another terminal claims at most once
    // per wall, and its line was told already.
    assert_eq!(latch.note(&wall, &NoRoute::Unproven), None);
    assert!(latch.settled(&wall));
    let next = Wall::new(login("fixture-account"), Some("next-turn".into()));
    assert!(!latch.settled(&next));
    assert!(latch.note(&next, &NoRoute::NotNeeded).is_some());
    assert!(!latch.settled(&next));
}

#[test]
fn a_walls_source_and_root_name_its_local_route() {
    let root = || Some("root-turn".to_owned());
    assert!(
        Wall::new(login("fixture-account"), root())
            .identity()
            .is_ok()
    );
    assert_eq!(
        Wall::new(Source::NotChatGpt, root()).identity().err(),
        Some(NoRoute::NotChatGpt)
    );
    assert_eq!(
        Wall::new(Source::login("", "fixture-user"), root())
            .identity()
            .err(),
        Some(NoRoute::IdentityUnreadable)
    );
    assert_eq!(
        Wall::new(login("fixture-account"), None).identity().err(),
        Some(NoRoute::NoRootTurn)
    );
    // The source is told first: an API-key run with no root turn is still not ChatGPT.
    assert_eq!(
        Wall::new(Source::NotChatGpt, None).identity().err(),
        Some(NoRoute::NotChatGpt)
    );
}

#[test]
fn a_line_is_written_whole() {
    let mut out = Vec::new();
    let line = NoRoute::Outside.line();
    tell_to(&mut out, &line);
    assert_eq!(out, line.as_bytes());
}

/// The routes aterm's refusals of a run name, and its `ERR denied` to a process prepare.
fn refused_routes() -> Vec<NoRoute> {
    vec![
        NoRoute::NotInTab,
        NoRoute::NotAlabImage,
        NoRoute::NotExec,
        NoRoute::UnsupportedRun,
        NoRoute::NoPoolRow,
        NoRoute::Denied,
    ]
}

/// EACH NEW LINE IS ITS OWN, AND NAMES NO LABEL, EMAIL OR PATH: one line with the prefix, no
/// other route's text, and, since none of these runs moves at this wall, the reset it waits
/// for. None is read again for the wall.
#[test]
fn each_new_line_names_no_label_or_path() {
    let earlier: Vec<String> = routes().iter().map(NoRoute::line).collect();
    let mut told: Vec<String> = Vec::new();
    for route in refused_routes() {
        let line = route.line();
        let body = line
            .strip_prefix(PREFIX)
            .and_then(|rest| rest.strip_suffix('\n'))
            .unwrap_or_else(|| panic!("{line:?}"));
        assert!(!body.is_empty(), "{line:?}");
        assert!(!body.contains(['\n', '\r', '@', '/', '~', '=']), "{line:?}");
        assert!(line.len() < 160, "{line:?}");
        assert!(
            line.contains("waits for its usage limit to reset"),
            "{line:?}"
        );
        assert!(!line.contains("the move waits"), "{line:?}");
        assert_eq!(route.again(), None, "{route:?}");
        assert!(
            !earlier.contains(&line) && !told.contains(&line),
            "{route:?} has another route's line"
        );
        told.push(line);
    }
}

/// A fake host that serves the exec contracts, an admitted leader's client of it, the origin it
/// prepares for, and that prepare's request line.
async fn prepared(dir: &std::path::Path) -> (FakeAterm, Client, crate::rotation::Origin, String) {
    let host = host(dir).await;
    let client = admit(host.endpoint(), true, options(), &ORDINARY)
        .await
        .unwrap();
    let origin = crate::rotation::Origin {
        thread: "00000000-0000-0000-0000-000000000001".into(),
        turn: "00000000-0000-0000-0000-000000000002".into(),
        identity: Identity {
            account: "a".repeat(64),
            user: "b".repeat(64),
        },
    };
    let request = format!(
        "@{} codex-exec-handoff prepare thread={} turn={} source_account={} source_user={}",
        host.sid(),
        origin.thread,
        origin.turn,
        origin.identity.account,
        origin.identity.user
    );
    (host, client, origin, request)
}

/// The route `host` names when it refuses the prepare `request` with `word`, read off the wire as
/// the runtime reads a prepare's answer.
async fn refused_on_the_wire(
    host: &FakeAterm,
    client: &Client,
    origin: &crate::rotation::Origin,
    request: &str,
    word: &str,
) -> Option<NoRoute> {
    host.reply_once(
        request,
        &format!("OK codex-exec-handoff state=refused why={word}\n"),
    );
    let Ok(selection) = account_recovery::prepare_exec(client, origin).await else {
        panic!("{word} was not taken");
    };
    selection.route()
}

/// EACH OF ATERM'S REFUSAL WORDS (CONTRACT §1.1, "Wire") NAMES ITS ROUTE AND THAT ROUTE'S CLOCK:
/// one that cannot change for the wall settles it, and an off word or `rotation-off` is read
/// again on the passive clock. `not-needed` keeps C1's 5 s clock, and a move may still come of
/// `waiting` and `selected`.
#[tokio::test]
async fn each_refused_word_names_its_route_and_clock() {
    let dir = tempfile::tempdir().unwrap();
    let (host, client, origin, request) = prepared(dir.path()).await;
    let other = |word: &str| NoRoute::Off(Off::Other(word.into()));
    for (word, route, again) in [
        ("not-in-tab", NoRoute::NotInTab, None),
        ("nested-aterm", NoRoute::NotInTab, None),
        ("not-alab-image", NoRoute::NotAlabImage, None),
        ("not-exec", NoRoute::NotExec, None),
        ("unsupported-run", NoRoute::UnsupportedRun, None),
        ("no-pool-row", NoRoute::NoPoolRow, None),
        ("login-names-another", NoRoute::NoPoolRow, None),
        ("hold", NoRoute::Off(Off::Hold), Some(PASSIVE)),
        (
            "autopilot-off",
            NoRoute::Off(Off::AutopilotOff),
            Some(PASSIVE),
        ),
        ("agent-off", other("agent-off"), Some(PASSIVE)),
        ("disabled", other("disabled"), Some(PASSIVE)),
        ("headless", other("headless"), Some(PASSIVE)),
        ("unhosted", other("unhosted"), Some(PASSIVE)),
        ("rotation-off", NoRoute::RotationOff, Some(PASSIVE)),
    ] {
        assert_eq!(NoRoute::refused(word), Miss::Route(route.clone()), "{word}");
        assert_eq!(route.again(), again, "{word}");
        assert_eq!(
            refused_on_the_wire(&host, &client, &origin, &request, word).await,
            Some(route.clone()),
            "{word}"
        );
        let wall = Wall::new(login("fixture-account"), Some("root-turn".into()));
        let mut latch = Latch::default();
        assert_eq!(latch.note(&wall, &route), Some(route.line()), "{word}");
        assert_eq!(latch.settled(&wall), again.is_none(), "{word}");
    }
    use account_recovery::ExecSelection;
    assert_eq!(ExecSelection::NotNeeded.route(), Some(NoRoute::NotNeeded));
    assert_eq!(NoRoute::NotNeeded.again(), Some(RETRY));
    assert_eq!(ExecSelection::Waiting.route(), None);
    let selected = ExecSelection::Selected {
        destination: Identity {
            account: "c".repeat(64),
            user: "d".repeat(64),
        },
        origin,
    };
    assert_eq!(selected.route(), None);
    assert_eq!(claims(&host), 1, "a refusal sends no other claim");
}

/// A REFUSAL IN A WORD THIS READER DOES NOT KNOW IS A PASSIVE OFF: never one that stops the asking
/// for the wall, and never asked again on the 5 s clock. Its word is repeated only when it is
/// plain. That holds on the wire too: a word that is not plain (`rotation_off`, in the spelling of
/// aterm's own policy keys, a capital, a word too long, none) is a refusal, never a malformed
/// reply, whose ask the runtime would repeat on the 5 s clock. `denied` is no refusal word: aterm
/// denies with `ERR denied`.
#[tokio::test]
async fn an_unknown_refusal_is_a_passive_off() {
    let wall = Wall::new(login("fixture-account"), Some("root-turn".into()));
    for word in ["a-new-word", "not-in-tabs", "denied"] {
        let Miss::Route(route) = NoRoute::refused(word) else {
            panic!("{word} is no answer");
        };
        assert_eq!(route, NoRoute::Off(Off::Other(word.into())), "{word}");
        assert_eq!(route.again(), Some(PASSIVE), "{word}");
        assert!(route.line().contains("the move waits"), "{word}");
        let mut latch = Latch::default();
        assert!(latch.note(&wall, &route).is_some(), "{word}");
        assert!(!latch.settled(&wall), "{word}");
    }
    let unknown = NoRoute::Off(Off::Other(String::new()));
    assert_eq!(unknown.again(), Some(PASSIVE));
    for word in [
        "",
        "a/b",
        "x@y.z",
        "Not-In-Tab",
        "two words",
        &"w".repeat(WHY_MAX + 1),
    ] {
        assert_eq!(
            NoRoute::refused(word),
            Miss::Route(unknown.clone()),
            "{word:?}"
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let (host, client, origin, request) = prepared(dir.path()).await;
    assert_eq!(
        refused_on_the_wire(&host, &client, &origin, &request, "a-new-word").await,
        Some(NoRoute::Off(Off::Other("a-new-word".into())))
    );
    let secret = "a".repeat(64);
    for word in [
        "rotation_off",
        "not_in_tab",
        "Not-In-Tab",
        "x@y.z",
        "a/b",
        "not-in-tab=1",
        "",
        &"w".repeat(WHY_MAX + 1),
        &format!("{secret}x"),
    ] {
        let route = refused_on_the_wire(&host, &client, &origin, &request, word).await;
        assert_eq!(route, Some(unknown.clone()), "{word:?}");
        let line = unknown.line();
        assert!(
            !line.contains(word) || word.is_empty(),
            "{word:?}: {line:?}"
        );
    }
}

/// A REFUSAL THAT NAMES A SUSPENSION IS NO ANSWER YET, as the host's policy reads it
/// ([`the_hosts_policy_names_the_route`]): aterm answers a suspension `waiting` (CONTRACT §1.1),
/// and a reply that names one anyway is asked again on the 5 s clock, silently, never told as an
/// Off on the passive clock.
#[tokio::test]
async fn a_refused_suspension_is_no_answer_yet() {
    assert_eq!(NoRoute::refused("suspended"), Miss::Transient);
    let dir = tempfile::tempdir().unwrap();
    let (host, client, origin, request) = prepared(dir.path()).await;
    assert_eq!(
        refused_on_the_wire(&host, &client, &origin, &request, "suspended").await,
        None
    );
    host.set_effective(false, "suspended");
    assert_eq!(
        reach(host.endpoint(), options()).await.err(),
        Some(Miss::Transient),
        "the policy's suspension reads the same"
    );
}

/// A RUN ATERM CAN NEVER READ AS A SUPPORTED EXEC RUN (`unsupported-run`: its directory, program
/// or environment is not one aterm can start it again from) STOPS THE ASKING FOR THE WALL, with
/// its own line: it is an exec run, not one that is not.
#[tokio::test]
async fn an_unsupported_run_stops_asking_with_its_own_line() {
    let Miss::Route(route) = NoRoute::refused("unsupported-run") else {
        panic!("unsupported-run is no answer");
    };
    assert_eq!(route, NoRoute::UnsupportedRun);
    assert_eq!(route.again(), None);
    assert_ne!(route.line(), NoRoute::NotExec.line());
    let wall = Wall::new(login("fixture-account"), Some("root-turn".into()));
    let mut latch = Latch::default();
    assert_eq!(latch.note(&wall, &route), Some(route.line()));
    assert!(latch.settled(&wall));
    let dir = tempfile::tempdir().unwrap();
    let (host, client, origin, request) = prepared(dir.path()).await;
    assert_eq!(
        refused_on_the_wire(&host, &client, &origin, &request, "unsupported-run").await,
        Some(NoRoute::UnsupportedRun)
    );
}

/// Whether this process can read files it was denied by their mode: root can, and then a
/// denial cannot be staged by a mode.
fn bypasses_modes() -> bool {
    // SAFETY: geteuid takes no arguments and cannot fail.
    unsafe { libc::geteuid() == 0 }
}

fn set_mode(path: &std::path::Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn only_a_denied_rendezvous_entry_is_unreachable() {
    let dir = tempfile::tempdir().unwrap();
    let graph = dir.path().join("graph");
    std::fs::create_dir(&graph).unwrap();
    let sock = dir.path().join("aterm-1.sock");
    std::fs::write(
        graph.join("t0000000001"),
        format!("sock {}\npid 1\n", sock.display()),
    )
    .unwrap();
    assert_eq!(
        located(crate::locate::read_endpoint(dir.path(), "t0000000001")),
        Ok(Endpoint::explicit(&sock, "t0000000001"))
    );
    // Missing, as across an aterm restart or seamless update and after the tab closed, or
    // naming no socket: no answer yet.
    assert_eq!(
        located(crate::locate::read_endpoint(dir.path(), "t0000000002")),
        Err(Miss::Transient)
    );
    std::fs::write(graph.join("t0000000003"), "pid 1\n").unwrap();
    assert_eq!(
        located(crate::locate::read_endpoint(dir.path(), "t0000000003")),
        Err(Miss::Transient)
    );
    // Denied, as a sandbox denies it: law 2, told and settled.
    let denied = std::io::Error::from_raw_os_error(libc::EPERM);
    assert_eq!(located(Err(denied)), Err(Miss::Route(NoRoute::Unreachable)));
    if !bypasses_modes() {
        set_mode(&graph.join("t0000000001"), 0o000);
        assert_eq!(
            located(crate::locate::read_endpoint(dir.path(), "t0000000001")),
            Err(Miss::Route(NoRoute::Unreachable))
        );
        set_mode(&graph.join("t0000000001"), 0o600);
    }
}

fn options() -> Options {
    Options {
        timeout: Duration::from_millis(500),
        ..Options::default()
    }
}

/// The wall C1 admits a run at: the ordinary bucket's.
const ORDINARY: WallKind = WallKind::Ordinary;

/// A fake aterm that serves both exec contracts and a policy that moves exec runs.
async fn host(dir: &std::path::Path) -> FakeAterm {
    let host = FakeAterm::start(dir, Mode::Current).await.unwrap();
    host.set_policy_key("codex_rotation", "1");
    host.set_policy_key("accounts", "wall");
    host.reply_always(
        &format!("@{} codex-exec-handoff info", host.sid()),
        "OK codex-exec-handoff contract=3\n",
    );
    host.reply_always(
        &format!("@{} codex-exec-handoff tree-info", host.sid()),
        "OK codex-exec-handoff tree-contract=1\n",
    );
    host
}

fn claims(host: &FakeAterm) -> usize {
    host.requests()
        .iter()
        .filter(|line| line.contains("meta set supervisor"))
        .count()
}

#[tokio::test]
async fn a_run_that_does_not_lead_sends_nothing_at_all() {
    let dir = tempfile::tempdir().unwrap();
    let host = host(dir.path()).await;
    let refused = admit(host.endpoint(), false, options(), &ORDINARY).await;
    assert_eq!(refused.err(), Some(Miss::Route(NoRoute::NotLeader)));
    assert_eq!(host.requests(), Vec::<String>::new());
    assert_eq!(claims(&host), 0);
}

#[tokio::test]
async fn a_leader_claims_and_prepares_its_move_as_before() {
    let dir = tempfile::tempdir().unwrap();
    let host = host(dir.path()).await;
    let client = admit(host.endpoint(), true, options(), &ORDINARY)
        .await
        .unwrap();
    assert!(client.holds_claim());
    assert_eq!(claims(&host), 1);
    assert!(host.claim().is_some_and(|claim| claim.proven_pid.is_some()));
    let origin = crate::rotation::Origin {
        thread: "00000000-0000-0000-0000-000000000001".into(),
        turn: "00000000-0000-0000-0000-000000000002".into(),
        identity: Identity {
            account: "a".repeat(64),
            user: "b".repeat(64),
        },
    };
    host.reply_once(
        &format!(
            "@{} codex-exec-handoff prepare thread={} turn={} source_account={} source_user={}",
            host.sid(),
            origin.thread,
            origin.turn,
            origin.identity.account,
            origin.identity.user
        ),
        &format!(
            "OK codex-exec-handoff state=selected thread={} turn={} source_account={} \
             source_user={} target_account={} target_user={}\n",
            origin.thread,
            origin.turn,
            origin.identity.account,
            origin.identity.user,
            "c".repeat(64),
            "d".repeat(64)
        ),
    );
    assert!(matches!(
        account_recovery::prepare_exec(&client, &origin).await,
        Ok(account_recovery::ExecSelection::Selected { .. })
    ));
}

/// A WALL THE HOST'S POLICY DOES NOT LEAVE SENDS NO CLAIM (gap 4, step S3): the policy read
/// already rules the move out, so the tab's claim is not taken for a wall nothing moves. Today's
/// aterm prints no `codex_rotation_refusal=1`, so a leader at a workspace's wall reads the policy
/// and claims nothing; with the word it claims as at an ordinary wall; a model's own limit
/// claims under no policy.
#[tokio::test]
async fn a_leader_claims_only_at_a_wall_its_hosts_policy_leaves() {
    use codex_alab_core::wall_kind::Workspace;
    let dir = tempfile::tempdir().unwrap();
    let host = host(dir.path()).await;
    let credits = WallKind::Workspace(Workspace::MemberCreditsDepleted);
    let model = WallKind::Limit("codex_other".into());
    for wall in [&credits, &model] {
        let refused = admit(host.endpoint(), true, options(), wall).await;
        assert_eq!(
            refused.err(),
            Some(Miss::Route(NoRoute::RotationOff)),
            "{wall:?}"
        );
        assert_eq!(claims(&host), 0, "{wall:?}");
        assert_eq!(host.claim(), None, "{wall:?}");
    }
    assert!(
        host.requests()
            .iter()
            .any(|line| line.contains("harness policy")),
        "the policy is read"
    );
    host.set_policy_key(account_recovery::ROTATION_REFUSAL, "1");
    let refused = admit(host.endpoint(), true, options(), &model).await;
    assert_eq!(refused.err(), Some(Miss::Route(NoRoute::RotationOff)));
    assert_eq!(claims(&host), 0);
    let client = admit(host.endpoint(), true, options(), &credits)
        .await
        .unwrap();
    assert!(client.holds_claim());
    assert_eq!(claims(&host), 1);
}

#[tokio::test]
async fn a_leader_aterm_does_not_prove_is_unproven_and_gives_the_claim_back() {
    let dir = tempfile::tempdir().unwrap();
    let host = host(dir.path()).await;
    host.set_foreground_leader(Some(1));
    let refused = admit(host.endpoint(), true, options(), &ORDINARY).await;
    assert_eq!(refused.err(), Some(Miss::Route(NoRoute::Unproven)));
    assert_eq!(claims(&host), 1);
    assert_eq!(host.claim(), None);
}

#[tokio::test]
async fn a_leader_that_meets_the_tab_leaders_proven_claim_claims_once_per_wall() {
    let dir = tempfile::tempdir().unwrap();
    let host = host(dir.path()).await;
    // A codex-alab TUI holds the tab; this run leads codex's own unified-exec PTY, so aterm
    // answers its claim `ERR busy supervisor=codex-alab@4242`.
    host.claim_proven_by(4242);
    let wall = Wall::new(login("fixture-account"), Some("root-turn".into()));
    let mut latch = Latch::default();
    let mut lines = Vec::new();
    // The controller's asks across one wall: it asks again only while the wall is not
    // settled.
    for _ in 0..3 {
        if latch.settled(&wall) {
            continue;
        }
        let refused = admit(host.endpoint(), true, options(), &ORDINARY).await;
        assert_eq!(refused.err(), Some(Miss::Route(NoRoute::AnotherLeads)));
        lines.extend(latch.note(&wall, &NoRoute::AnotherLeads));
    }
    assert_eq!(claims(&host), 1, "one claim for the wall");
    assert_eq!(lines, vec![NoRoute::AnotherLeads.line()]);
    assert_eq!(
        host.claim().map(|claim| claim.holder),
        Some("codex-alab@4242".to_owned()),
        "the TUI's claim stands"
    );
}

#[tokio::test]
async fn a_leader_that_meets_an_unproven_claim_asks_again_and_moves_once_it_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    let host = host(dir.path()).await;
    // A driver's `ttl=` claim, which lapses at its ttl, or one set without `ttl=` (an older
    // client's), which stands as long as its connection: asked again while it stands.
    host.claim_as("driver-fixture");
    assert_eq!(
        admit(host.endpoint(), true, options(), &ORDINARY)
            .await
            .err(),
        Some(Miss::Transient)
    );
    // A proven claim whose status cannot be read says nothing either.
    host.claim_proven_by(4242);
    host.answer_status_with(Some("ERR busy switching versions retry_ms=100"));
    assert_eq!(
        admit(host.endpoint(), true, options(), &ORDINARY)
            .await
            .err(),
        Some(Miss::Transient)
    );
    host.answer_status_with(None);
    host.clear_claim();
    host.set_foreground_leader(Some(std::process::id()));
    let client = admit(host.endpoint(), true, options(), &ORDINARY)
        .await
        .unwrap();
    assert!(client.holds_claim());
    assert_eq!(claims(&host), 3);
}

#[tokio::test]
async fn a_leader_that_meets_its_own_proven_claim_asks_again_and_moves_once_it_leads() {
    let dir = tempfile::tempdir().unwrap();
    let host = host(dir.path()).await;
    let me = std::process::id();
    // This run's proven claim of its ask before stands, its connection's close not read yet
    // by aterm. The run left the front (a `bg`) after its local check found it leading and
    // before its claim reached aterm, so aterm does not prove the new claim. An unproven claim
    // never takes its holder's proven claim's place: aterm answers `ERR busy` naming this
    // run's own holder, and `status` shows that claim proven. No other program leads the tab,
    // so that is no answer yet, not another leader that settles the wall.
    host.claim_proven_by(me);
    host.set_foreground_leader(Some(1));
    assert_eq!(
        admit(host.endpoint(), true, options(), &ORDINARY)
            .await
            .err(),
        Some(Miss::Transient)
    );
    assert_eq!(
        host.claim().and_then(|claim| claim.proven_pid),
        Some(me),
        "the proven claim stands"
    );
    // Brought back to the front: its next ask is proven and renews the claim.
    host.set_foreground_leader(Some(me));
    let client = admit(host.endpoint(), true, options(), &ORDINARY)
        .await
        .unwrap();
    assert!(client.holds_claim());
    assert_eq!(claims(&host), 2);
}

#[tokio::test]
async fn the_hosts_policy_names_the_route() {
    let dir = tempfile::tempdir().unwrap();
    let host = host(dir.path()).await;
    for (why, expected) in [
        ("hold", Miss::Route(NoRoute::Off(Off::Hold))),
        (
            "autopilot-off",
            Miss::Route(NoRoute::Off(Off::AutopilotOff)),
        ),
        (
            "agent-off",
            Miss::Route(NoRoute::Off(Off::Other("agent-off".into()))),
        ),
        ("suspended", Miss::Transient),
    ] {
        host.set_effective(false, why);
        assert_eq!(
            admit(host.endpoint(), true, options(), &ORDINARY)
                .await
                .err(),
            Some(expected),
            "{why}"
        );
    }
    assert_eq!(claims(&host), 0, "no claim under an off");
    host.set_effective(true, "enabled");
    host.set_policy_key("codex_rotation", "0");
    assert_eq!(
        admit(host.endpoint(), true, options(), &ORDINARY)
            .await
            .err(),
        Some(Miss::Route(NoRoute::RotationOff))
    );
    assert_eq!(claims(&host), 0);
}

#[tokio::test]
async fn a_host_without_the_exec_contracts_has_no_route_and_a_busy_one_no_answer() {
    let dir = tempfile::tempdir().unwrap();
    let host = FakeAterm::start(dir.path(), Mode::Current).await.unwrap();
    host.set_policy_key("codex_rotation", "1");
    host.set_policy_key("accounts", "wall");
    // The fake knows no `codex-exec-handoff` verb, as an aterm before the exec contract.
    assert_eq!(
        reach(host.endpoint(), options()).await.err(),
        Some(Miss::Route(NoRoute::NoExecContract))
    );
    let info = format!("@{} codex-exec-handoff info", host.sid());
    host.reply_once(&info, "OK codex-exec-handoff contract=2\n");
    assert_eq!(
        reach(host.endpoint(), options()).await.err(),
        Some(Miss::Route(NoRoute::NoExecContract))
    );
    host.reply_once(&info, "ERR busy switching versions retry_ms=100\n");
    assert_eq!(
        reach(host.endpoint(), options()).await.err(),
        Some(Miss::Transient)
    );
    host.reply_always(&info, "OK codex-exec-handoff contract=3\n");
    assert_eq!(
        reach(host.endpoint(), options()).await.err(),
        Some(Miss::Route(NoRoute::NoExecContract)),
        "no tree contract"
    );
    host.reply_always(
        &format!("@{} codex-exec-handoff tree-info", host.sid()),
        "OK codex-exec-handoff tree-contract=1\n",
    );
    assert!(reach(host.endpoint(), options()).await.is_ok());
    // An aterm without `harness policy` has no exec contract; a busy or silent one has
    // not answered yet.
    host.answer_policy_with(Some("ERR unknown verb (try: help)"));
    assert_eq!(
        reach(host.endpoint(), options()).await.err(),
        Some(Miss::Route(NoRoute::NoExecContract))
    );
    host.answer_policy_with(Some("ERR busy switching versions retry_ms=100"));
    assert_eq!(
        reach(host.endpoint(), options()).await.err(),
        Some(Miss::Transient)
    );
    host.answer_policy_with(None);
    host.stall("harness");
    assert_eq!(
        reach(host.endpoint(), options()).await.err(),
        Some(Miss::Transient)
    );
    assert_eq!(claims(&host), 0, "reach never claims");
}

/// aterm's refusals when its main thread does not answer `harness policy`
/// (`control_media::main_thread_refusal`, `dialog_refusal_for`; `harness_policy_verb::cmd`
/// sends each as `ERR <why>`).
const MAIN_THREAD_REFUSALS: [&str; 4] = [
    "ERR aterm is showing a dialog; answer it, then retry",
    "ERR main-thread reply dropped",
    "ERR main-thread reply did not arrive within 60s",
    "ERR event loop gone",
];

#[tokio::test]
async fn a_policy_read_the_main_thread_refuses_is_no_answer_and_the_leader_moves_after() {
    let dir = tempfile::tempdir().unwrap();
    let host = host(dir.path()).await;
    for refusal in MAIN_THREAD_REFUSALS {
        host.answer_policy_with(Some(refusal));
        assert_eq!(
            admit(host.endpoint(), true, options(), &ORDINARY)
                .await
                .err(),
            Some(Miss::Transient),
            "{refusal}"
        );
    }
    assert_eq!(claims(&host), 0, "no claim without a policy");
    // The dialog closed: the next ask claims and is admitted, as before C1.
    host.answer_policy_with(None);
    let client = admit(host.endpoint(), true, options(), &ORDINARY)
        .await
        .unwrap();
    assert!(client.holds_claim());
}

#[tokio::test]
async fn only_an_answer_that_names_no_such_request_is_no_exec_contract() {
    let dir = tempfile::tempdir().unwrap();
    let host = host(dir.path()).await;
    for unserved in [
        "ERR unknown verb (try: help)",
        "ERR unknown verb 'harness' (help lists them)",
        "ERR harness has no handler",
        "ERR usage: harness recovery",
        // An `OK` this reader cannot take: an older policy without `gen=` and `effective=`.
        "OK policy",
    ] {
        host.answer_policy_with(Some(unserved));
        assert_eq!(
            reach(host.endpoint(), options()).await.err(),
            Some(Miss::Route(NoRoute::NoExecContract)),
            "{unserved}"
        );
    }
    host.answer_policy_with(None);
    let info = format!("@{} codex-exec-handoff info", host.sid());
    for refusal in MAIN_THREAD_REFUSALS
        .into_iter()
        .chain(["ERR denied", "ERR something new"])
    {
        host.reply_once(&info, &format!("{refusal}\n"));
        assert_eq!(
            reach(host.endpoint(), options()).await.err(),
            Some(Miss::Transient),
            "{refusal}"
        );
    }
    for unserved in [
        "ERR denied native operation authority",
        "ERR codex-exec-handoff has no handler",
        "OK codex-exec-handoff contract=4",
    ] {
        host.reply_once(&info, &format!("{unserved}\n"));
        assert_eq!(
            reach(host.endpoint(), options()).await.err(),
            Some(Miss::Route(NoRoute::NoExecContract)),
            "{unserved}"
        );
    }
    assert!(reach(host.endpoint(), options()).await.is_ok());
}

#[test]
fn not_served_reads_only_the_answers_that_name_no_such_request() {
    for line in [
        "OK",
        "OK codex-exec-handoff contract=2",
        "ERR unknown verb (try: help)",
        "ERR unknown verb 'harness' (help lists them)",
        "ERR harness has no handler",
        "ERR usage: harness policy | recovery",
        "ERR denied native operation authority",
        "ERR codex-exec-handoff unsupported request",
    ] {
        assert!(not_served(line), "{line}");
    }
    for line in MAIN_THREAD_REFUSALS.into_iter().chain([
        "ERR busy switching versions retry_ms=100",
        "ERR control server busy; retry retry_ms=50",
        "ERR auth",
        "ERR denied",
        "ERR no such session 7",
        "",
    ]) {
        assert!(!not_served(line), "{line}");
    }
}

#[tokio::test]
async fn a_socket_that_refuses_or_is_missing_is_no_answer_as_across_an_update() {
    let dir = tempfile::tempdir().unwrap();
    // Missing: the predecessor is gone and its successor has not bound yet.
    let missing = Endpoint::explicit(dir.path().join("aterm-1.sock"), "t0000000001");
    assert_eq!(
        admit(missing, true, options(), &ORDINARY).await.err(),
        Some(Miss::Transient)
    );
    // Refusing: the socket file stays, with no listener behind it.
    let path = dir.path().join("aterm-2.sock");
    drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
    assert_eq!(
        admit(
            Endpoint::explicit(&path, "t0000000002"),
            true,
            options(),
            &ORDINARY
        )
        .await
        .err(),
        Some(Miss::Transient)
    );
}

#[tokio::test]
async fn an_auth_the_successor_refuses_is_no_answer() {
    let dir = tempfile::tempdir().unwrap();
    let host = host(dir.path()).await;
    // The token beside the socket is not the listener's: a successor between its token's
    // rotation and the client's read, as the update handoff orders them.
    let token = host.endpoint().sock.with_extension("token");
    let listener = std::fs::read_to_string(&token).unwrap();
    std::fs::write(&token, "ffffffffffffffff\n").unwrap();
    assert_eq!(
        admit(host.endpoint(), true, options(), &ORDINARY)
            .await
            .err(),
        Some(Miss::Transient)
    );
    std::fs::write(&token, listener).unwrap();
    assert!(
        admit(host.endpoint(), true, options(), &ORDINARY)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn only_a_denied_socket_is_unreachable() {
    if bypasses_modes() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let locked = dir.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    let path = locked.join("aterm-1.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
    // No search permission on its directory: connect is denied (`EACCES`), as a sandbox
    // denies the socket (`EPERM`).
    set_mode(&locked, 0o000);
    let denied = admit(
        Endpoint::explicit(&path, "t0000000001"),
        true,
        options(),
        &ORDINARY,
    )
    .await;
    set_mode(&locked, 0o700);
    assert_eq!(denied.err(), Some(Miss::Route(NoRoute::Unreachable)));
    assert_eq!(NoRoute::Unreachable.again(), None);
}

/// A child of these tests: the case it plays.
const CASE: &str = "CODEX_ALAB_FOREGROUND_CASE";
/// The child's exits: it leads its terminal's foreground group, it does not, or the
/// launcher that was to start it did not lead (the setup failed, so the case proves nothing).
const LEADS: i32 = 40;
const DOES_NOT_LEAD: i32 = 41;
const LAUNCHER_DID_NOT_LEAD: i32 = 42;

/// The test binary again, running only [`foreground_case`] as `case`, with no stdio.
fn child(case: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "exec_route::tests::foreground_case",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CASE, case)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn exit(status: std::process::ExitStatus) -> i32 {
    status
        .code()
        .unwrap_or_else(|| panic!("the child ended by a signal: {status:?}"))
}

#[test]
#[ignore = "run as a child by the foreground tests"]
fn foreground_case() {
    let Ok(case) = std::env::var(CASE) else {
        return;
    };
    match case.as_str() {
        "report" => std::process::exit(if leads_foreground() {
            LEADS
        } else {
            DOES_NOT_LEAD
        }),
        "setpgid" | "member" => {
            // The launcher leads its own terminal, so a child that does not lead it shows that
            // leads_foreground reads the child's place, not a missing terminal.
            if !leads_foreground() {
                std::process::exit(LAUNCHER_DID_NOT_LEAD);
            }
            let mut command = child("report");
            if case == "setpgid" {
                // SAFETY: setpgid is async-signal-safe and touches only the child.
                unsafe {
                    command.pre_exec(|| {
                        if libc::setpgid(0, 0) < 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                        Ok(())
                    });
                }
            }
            std::process::exit(exit(command.status().unwrap()));
        }
        other => panic!("unknown case {other}"),
    }
}

/// Run `case` as the leader of a new terminal of its own: in the child, `setsid`, the
/// PTY's slave made the controlling terminal (`TIOCSCTTY`), and its group put in front
/// (`tcsetpgrp`). The slave is close-on-exec and the child's stdio is `/dev/null`, so once
/// the case runs, only `/dev/tty` reaches its terminal: a [`leads_foreground`] that read
/// stdin, stdout or stderr instead finds no terminal there.
fn in_own_terminal(case: &str) -> i32 {
    let (mut master, mut slave) = (-1, -1);
    // SAFETY: openpty fills two new descriptors on success; the null arguments are optional.
    let opened = unsafe {
        libc::openpty(
            &raw mut master,
            &raw mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(opened, 0, "{}", std::io::Error::last_os_error());
    // SAFETY: openpty succeeded, so both descriptors are open and owned by nothing else.
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    for fd in [&master, &slave] {
        // SAFETY: fcntl on a descriptor owned here, with integer arguments only.
        assert_eq!(
            unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) },
            0
        );
    }
    let mut command = child(case);
    let terminal = slave.as_raw_fd();
    // SAFETY: only async-signal-safe syscalls, on the slave the forked child inherited; it
    // closes at the exec.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0
                || libc::ioctl(terminal, libc::TIOCSCTTY.into(), 0) < 0
                || libc::tcsetpgrp(terminal, libc::getpid()) < 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let status = command.status().unwrap();
    // Both ends stay open until the child is gone, so its terminal never hangs up first.
    drop((master, slave));
    exit(status)
}

#[test]
fn a_terminals_foreground_leader_leads_it() {
    // Its stdio is `/dev/null`, as `codex exec - < prompt.md` or `< /dev/null` typed at the
    // tab's prompt leaves stdin: the terminal is read from `/dev/tty` alone.
    assert_eq!(in_own_terminal("report"), LEADS);
}

#[test]
fn a_setpgid_child_does_not_lead_its_terminal() {
    assert_eq!(in_own_terminal("setpgid"), DOES_NOT_LEAD);
}

#[test]
fn a_member_of_the_foreground_group_does_not_lead_it() {
    assert_eq!(in_own_terminal("member"), DOES_NOT_LEAD);
}

#[test]
fn a_setsid_child_has_no_terminal_to_lead() {
    let mut command = child("report");
    // SAFETY: setsid is async-signal-safe and touches only the child.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    assert_eq!(exit(command.status().unwrap()), DOES_NOT_LEAD);
}
