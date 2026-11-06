// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! aterm's cooperative schedule (`harness::upgrade::next_step`, aterm main `433715d26d`) on
//! codex-alab's round: one notice at a due upgrade, again every 30 minutes up to four, a rest of
//! 2 h, 4 h, 8 h and then 24 h after each unanswered round, the host's sleep counted, a held
//! clock at a wall, and only the agent's latest reply saying ready lets the re-exec go.

use std::time::Duration;
use std::time::Instant;

use pretty_assertions::assert_eq;

use crate::upgrade_ask::AskStep;
use crate::upgrade_ask::Round;
use crate::upgrade_ask::unanswered_rest;

const TARGET: &str = "/Applications/codex-alab-next";
const HOUR: Duration = Duration::from_secs(3600);
/// aterm's numbers, not the module's: `REASK_S` (1800 s) and `MAX_ASKS` (4).
const REASK: Duration = Duration::from_secs(1800);
const MAX_ASKS: u32 = 4;

fn aimed() -> Round {
    let mut round = Round::default();
    round.aim(TARGET);
    round
}

/// The notice the runtime types: it carries the reference the round offered.
fn notice(round: &mut Round, at: Instant) -> String {
    let marker = round.offer("thread-1");
    let text = crate::words::upgrade_notice(TARGET, "0.1.0", &marker);
    round.noticed(&text, at);
    marker
}

fn waits_until(step: AskStep) -> Instant {
    match step {
        AskStep::Wait { until, .. } => until,
        other => panic!("expected a wait, got {other:?}"),
    }
}

#[test]
fn a_due_upgrade_asks_at_once() {
    let mut round = aimed();
    assert_eq!(round.step(Instant::now(), false), AskStep::Notice);
    assert_eq!(round.asks(), 0);
}

#[test]
fn a_notice_counts_only_with_the_reference_offered() {
    let t0 = Instant::now();
    let mut round = aimed();
    let marker = round.offer("thread-1");
    round.noticed("Continue.", t0);
    assert_eq!(round.asks(), 0, "a text without the reference is no notice");
    assert_eq!(round.step(t0, false), AskStep::Notice);
    let marker_again = round.offer("thread-1");
    assert_eq!(
        marker, marker_again,
        "the same notice is offered the same reference"
    );
    round.noticed(&crate::words::upgrade_notice(TARGET, "0.1.0", &marker), t0);
    assert_eq!(round.asks(), 1);
    assert!(
        marker.starts_with("ATERM-UPGRADE-READY-") && marker.len() == 28,
        "{marker}"
    );
}

/// The notice names the build it moves to, the one running, Codex and its reference, in
/// aterm's sentences; read back as an answer (an agent quoting it) it is no readiness.
#[test]
fn the_notice_carries_its_reference_and_is_no_answer_itself() {
    let marker = "ATERM-UPGRADE-READY-0badc0de";
    let text = crate::words::upgrade_notice(TARGET, "0.164.0", marker);
    assert!(text.starts_with("[codex-alab] Codex /Applications/codex-alab-next is installed; this session runs 0.164.0."), "{text}");
    for part in [
        "codex-alab will restart itself in place",
        "(codex resume, same tab, same flags)",
        crate::words::UPGRADE_STOPPING_POINT,
        crate::words::UPGRADE_READINESS_ANSWER,
        "Notice reference: ATERM-UPGRADE-READY-0badc0de.",
        crate::words::UPGRADE_NOTICE_CLOSE,
    ] {
        assert!(text.contains(part), "{part}: {text}");
    }
    assert!(!crate::upgrade_readiness::says_ready_to(&text, marker));
}

#[test]
fn an_unanswered_notice_repeats_every_half_hour_up_to_four() {
    let t0 = Instant::now();
    let mut round = aimed();
    let mut at = t0;
    for ask in 1..=MAX_ASKS {
        assert_eq!(
            round.step(at, false),
            AskStep::Notice,
            "notice {ask} is due"
        );
        notice(&mut round, at);
        assert_eq!(round.asks(), ask);
        let just_before = at + REASK - Duration::from_secs(1);
        assert_eq!(waits_until(round.step(just_before, false)), at + REASK);
        at += REASK;
    }
    // Half an hour after the fourth notice the round gives up and rests 2 h.
    assert_eq!(waits_until(round.step(at, false)), at + 2 * HOUR);
    assert_eq!(
        waits_until(round.step(at + 2 * HOUR - Duration::from_secs(1), false)),
        at + 2 * HOUR
    );
    assert_eq!(
        round.step(at + 2 * HOUR, false),
        AskStep::Notice,
        "the rest over, it asks again"
    );
    assert_eq!(round.asks(), 0);
}

#[test]
fn rests_after_unanswered_rounds_are_two_four_eight_then_twenty_four_hours() {
    assert_eq!(
        (1..=6)
            .map(|streak| unanswered_rest(streak) / 3600)
            .collect::<Vec<_>>(),
        [2, 4, 8, 24, 24, 24].map(Duration::from_secs).to_vec()
    );
    let mut round = aimed();
    let mut at = Instant::now();
    let mut rests = Vec::new();
    for _ in 0..5 {
        for _ in 0..MAX_ASKS {
            assert_eq!(round.step(at, false), AskStep::Notice);
            notice(&mut round, at);
            at += REASK;
        }
        let until = waits_until(round.step(at, false));
        rests.push((until - at).as_secs() / 3600);
        at = until;
    }
    assert_eq!(rests, [2, 4, 8, 24, 24]);
}

#[test]
fn only_an_answer_that_says_ready_lets_it_go() {
    let t0 = Instant::now();
    let mut round = aimed();
    notice(&mut round, t0);
    round.heard(Some("Not yet: the release build is still running."));
    assert!(!round.ready());
    assert!(matches!(round.step(t0, false), AskStep::Wait { .. }));
    round.heard(None);
    assert!(!round.ready());
    round.heard(Some("Everything is saved. I'm ready to restart now."));
    assert_eq!(round.step(t0, false), AskStep::Ready);
}

#[test]
fn the_reference_alone_is_readiness_to_the_latest_notice_only() {
    let t0 = Instant::now();
    let mut round = aimed();
    let first = notice(&mut round, t0);
    let second = notice(&mut round, t0 + REASK);
    assert_ne!(first, second);
    round.heard(Some(&first));
    assert!(
        !round.ready(),
        "an earlier notice's reference does not answer the latest"
    );
    round.heard(Some(&second));
    assert!(round.ready());
}

#[test]
fn a_late_ready_after_the_round_gave_up_still_counts() {
    let mut round = aimed();
    let mut at = Instant::now();
    for _ in 0..MAX_ASKS {
        notice(&mut round, at);
        at += REASK;
    }
    assert!(matches!(round.step(at, false), AskStep::Wait { .. }));
    round.heard(Some("Ready for the restart."));
    assert_eq!(round.step(at + HOUR, false), AskStep::Ready);
}

/// A person's message after a ready answer (aterm's direction) supersedes it, and the round asks
/// again at once.
#[test]
fn a_persons_message_after_ready_supersedes_it_and_asks_again_at_once() {
    let t0 = Instant::now();
    let mut round = aimed();
    notice(&mut round, t0);
    round.heard(Some("Ready for the restart."));
    round.directed();
    assert!(!round.ready());
    assert_eq!(
        round.step(t0 + Duration::from_secs(1), false),
        AskStep::Notice
    );
    notice(&mut round, t0 + Duration::from_secs(1));
    assert_eq!(round.asks(), 2);
    assert!(matches!(
        round.step(t0 + Duration::from_secs(2), false),
        AskStep::Wait { .. }
    ));
}

#[test]
fn a_persons_message_without_readiness_supersedes_nothing() {
    let t0 = Instant::now();
    let mut round = aimed();
    notice(&mut round, t0);
    round.directed();
    assert_eq!(waits_until(round.step(t0, false)), t0 + REASK);
}

#[test]
fn a_superseded_fourth_ready_gives_the_round_up() {
    let mut round = aimed();
    let mut at = Instant::now();
    for _ in 0..MAX_ASKS {
        notice(&mut round, at);
        at += Duration::from_secs(60);
    }
    round.heard(Some("Ready for the restart."));
    round.directed();
    assert_eq!(waits_until(round.step(at, false)), at + 2 * HOUR);
}

#[test]
fn a_notice_that_met_a_wall_is_given_back_and_typed_again() {
    let t0 = Instant::now();
    let mut round = aimed();
    notice(&mut round, t0);
    round.undelivered();
    assert_eq!(round.asks(), 0);
    assert_eq!(
        round.step(t0, false),
        AskStep::Notice,
        "typed again at once"
    );
    notice(&mut round, t0);
    assert_eq!(
        round.asks(),
        1,
        "the given-back notice is not counted twice"
    );
}

#[test]
fn a_held_clock_keeps_the_next_notice_from_coming_due() {
    let t0 = Instant::now();
    let mut round = aimed();
    notice(&mut round, t0);
    // At a wall for two hours: no notice comes due, and no round gives up.
    let walled = t0 + 2 * HOUR;
    assert_eq!(waits_until(round.step(walled, true)), walled + REASK);
    // Off the wall, the half hour runs from where the clock was let go.
    assert_eq!(
        waits_until(round.step(walled + Duration::from_secs(60), false)),
        walled + REASK
    );
    assert_eq!(round.step(walled + REASK, false), AskStep::Notice);
}

#[test]
fn each_due_notice_wakes_one_look() {
    let t0 = Instant::now();
    let mut round = aimed();
    assert!(round.wake_due(t0, false));
    assert!(
        !round.wake_due(t0, false),
        "the same due notice wakes no second look"
    );
    notice(&mut round, t0);
    assert!(
        !round.wake_due(t0, false),
        "a notice in flight wakes nothing"
    );
    assert!(
        round.wake_due(t0 + REASK, false),
        "the next due notice wakes one look"
    );
    assert!(!round.wake_due(t0 + REASK, false));
}

#[test]
fn another_build_begins_a_new_round() {
    let t0 = Instant::now();
    let mut round = aimed();
    let first = notice(&mut round, t0);
    round.aim(TARGET);
    assert_eq!(round.asks(), 1, "the same build keeps its round");
    round.aim("/Applications/codex-alab-later");
    assert_eq!(round.asks(), 0);
    assert_eq!(round.target(), Some("/Applications/codex-alab-later"));
    assert_eq!(round.step(t0, false), AskStep::Notice);
    let second = round.offer("thread-1");
    assert_ne!(first, second, "a new round's reference differs");
}

/// THE AGENT'S LATEST REPLY DECIDES (aterm's `transcript_has_ready` over `answers`; review of
/// gap 27, finding 1): a reply to codex-alab's own act after the notice answers it in ordinary
/// language as the notice's own reply does, a later reply that does not say ready takes the
/// readiness back with no re-ask before the notice's half hour, and after a person's message
/// only the notice's reference alone says ready. NEGATIVE CONTROL: the same ordinary words
/// after the person's message are no readiness.
#[test]
fn the_agents_latest_reply_answers_the_notice_until_a_person_speaks() {
    let t0 = Instant::now();
    let mut round = aimed();
    let marker = notice(&mut round, t0);
    round.heard(Some("Not yet: the release build is still running."));
    assert!(!round.ready());
    // Codex-alab's continuation, then the agent's reply to it.
    round.heard(Some("The build finished. I'm ready to restart now."));
    assert!(
        round.ready(),
        "a reply to codex-alab's own act answers the notice"
    );
    round.heard(Some("Continuing with stage 2."));
    assert!(!round.ready());
    assert_eq!(
        waits_until(round.step(t0 + Duration::from_secs(60), false)),
        t0 + REASK,
        "no re-ask before the notice's half hour"
    );
    round.heard(Some("Ready for the restart."));
    assert!(round.ready());
    round.directed();
    round.heard(Some("Ready for the restart."));
    assert!(
        !round.ready(),
        "after a person's message ordinary words are no answer"
    );
    round.heard(Some(&format!("Done.\n{marker}")));
    assert!(round.ready(), "the reference alone still is");
}

/// THE HOST'S SLEEP COUNTS (aterm's drive reads unix seconds; review of gap 27, finding 6): a
/// re-ask and a rest the host slept through are due at the wake, though the `Instant` clock
/// stood still. NEGATIVE CONTROL: the same awake minutes with no sleep read wait.
#[test]
fn the_hosts_sleep_counts_toward_the_reask_and_the_rest() {
    let t0 = Instant::now();
    let base = Duration::from_secs(1000);
    let woke = t0 + 5 * Duration::from_secs(60);
    let asked = |slept: Duration| {
        let mut round = aimed();
        round.slept_to(Some(base), t0);
        notice(&mut round, t0);
        round.slept_to(Some(base + slept), woke);
        round.step(woke, false)
    };
    assert_eq!(asked(8 * HOUR), AskStep::Notice);
    assert_eq!(waits_until(asked(Duration::ZERO)), t0 + REASK);
    // A round that gave up rests in real time too.
    let mut round = aimed();
    round.slept_to(Some(base), t0);
    let mut at = t0;
    for _ in 0..MAX_ASKS {
        notice(&mut round, at);
        at += REASK;
    }
    assert_eq!(waits_until(round.step(at, false)), at + 2 * HOUR);
    round.slept_to(Some(base + 3 * HOUR), at + Duration::from_secs(60));
    assert_eq!(
        round.step(at + Duration::from_secs(60), false),
        AskStep::Notice,
        "the rest was slept through"
    );
}

/// A NOTICE IS GIVEN BACK ONLY AT A WALL IT NEVER GOT PAST (aterm's `notice_scan`: a login wall,
/// a usage limit; review of gap 27, finding 3): a login, a usage window and a quota; a server's,
/// the network's, a full context's or a cut reply's wall leaves it standing.
#[test]
fn only_a_login_wall_or_a_usage_limit_withholds_the_notice() {
    use crate::decide::Wall;
    use crate::upgrade_ask::withheld_by;
    for wall in [
        Wall::Login,
        Wall::Usage {
            reset_at: None,
            reset_label: None,
        },
        Wall::Entitlement,
    ] {
        assert!(withheld_by(&wall), "{wall:?}");
    }
    for wall in [
        Wall::Server {
            what: "an API error (503)".into(),
            cut_off: false,
            retry_at: None,
        },
        Wall::Server {
            what: "the reply was cut off".into(),
            cut_off: true,
            retry_at: None,
        },
        Wall::Network {
            what: "no connection".into(),
        },
        Wall::Context { auto_compact: true },
        Wall::Overlong {
            what: "max_output_tokens".into(),
        },
    ] {
        assert!(!withheld_by(&wall), "{wall:?}");
    }
}
