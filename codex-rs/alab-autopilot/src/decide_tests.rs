// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The pure decider, point by point (each rule with its negative control), FOREVER CRUISE /
//! FOREVER LAND's included (aterm owner direction 2026-10-19).

#[path = "go_ahead_decide_tests.rs"]
mod go_ahead_decide_tests;
#[path = "land_held_tests.rs"]
mod land_held_tests;
#[path = "needs_edges_tests.rs"]
mod needs_edges_tests;
#[path = "upgrade_notice_tests.rs"]
mod upgrade_notice_tests;

use std::time::Duration;
use std::time::Instant;

use crate::decide::Act;
use crate::decide::Decision;
use crate::decide::MANAGED_SHORT_TURNS;
use crate::decide::Mode;
use crate::decide::Needs;
use crate::decide::Owed;
use crate::decide::Policy;
use crate::decide::Questions;
use crate::decide::Texts;
use crate::decide::ThreadState;
use crate::decide::Timing;
use crate::decide::TurnEnd;
use crate::decide::Wall;
use crate::decide::decide;
use crate::needs::NEEDS_ACK;
use crate::needs::NEEDS_HINT;
use crate::netwatch::Reach;
use crate::open_work::open_work_suffix;
use crate::repo::RepoRead;
use crate::repo::RepoWork;
use crate::words;
use crate::words::ApiCause;
use crate::words::act_words;
use pretty_assertions::assert_eq;

const MIN: Duration = Duration::from_secs(60);
const NO_JITTER: Duration = Duration::ZERO;

/// A whole-probe operation budget of zero cannot prove an unpushed repository clean.
/// The completed read is the control: both its commit and dirty file are real Git facts.
#[test]
fn an_expired_repository_probe_never_lands_a_clean_defense() {
    let tmp = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let work = crate::repo_tests::repo(tmp.path(), true);
    std::fs::write(work.join("b.txt"), "two\n").unwrap_or_else(|err| panic!("{err}"));
    crate::repo_tests::scratch_git(&work, &["add", "b.txt"]);
    crate::repo_tests::scratch_git(&work, &["commit", "-q", "-m", "two"]);
    std::fs::write(work.join("a.txt"), "changed\n").unwrap_or_else(|err| panic!("{err}"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|err| panic!("{err}"));
    let read = |budget| runtime.block_on(crate::repo::probe_within(&work, tmp.path(), budget));
    let whole = match read(crate::repo::PROBE_LIMIT) {
        RepoRead::Read(Some(work)) => work,
        read => panic!("the complete repository read must find its work: {read:?}"),
    };
    assert_eq!((whole.changed, whole.unpushed, whole.unlanded), (1, 1, 1));
    let now = Instant::now();
    let timing = Timing::default();
    let mut state = ThreadState::default();
    state.observe_mode(Mode::Land);
    state.observe_person_turn();
    state.observe_turn_end(end(now, "Work is in hand.", 3 * MIN), &timing);
    let instruction = decide(&state, &land(), &timing, now, NO_JITTER);
    assert_eq!(
        typed(&instruction).map(|(rule, _)| rule),
        Some(words::RULE_LAND)
    );
    state.acted(&instruction.act, now, &timing);
    let mut at = now;
    for round in 0..3 {
        at += 3 * MIN;
        // Each round the worker works (a tool run), so ruling F holds none of its defenses.
        state.observe_turn_end(
            tooled(ours(
                at,
                "Nothing left: all work is merged and pushed to origin/main.",
                3 * MIN,
            )),
            &timing,
        );
        assert_eq!(
            decide(&state, &land(), &timing, at, NO_JITTER).act,
            Act::Probe
        );
        let expired = read(Duration::ZERO);
        assert_eq!(expired, RepoRead::Unread, "round {round}");
        state.probed(expired);
        let result = decide(&state, &land(), &timing, at, NO_JITTER);
        assert_eq!(
            typed(&result),
            Some((
                words::RULE_LAND_NUDGE,
                format!("{} {}", words::LAND_NUDGE, words::REPO_UNREAD).as_str()
            ))
        );
        assert!(!state.landed);
        state.acted(&result.act, at, &timing);
    }
    // Same real repository, now committed and pushed: the finished read allows landing.
    crate::repo_tests::scratch_git(&work, &["add", "a.txt"]);
    crate::repo_tests::scratch_git(&work, &["commit", "-q", "-m", "three"]);
    crate::repo_tests::scratch_git(&work, &["push", "-q", "origin", "HEAD:main"]);
    at += 3 * MIN;
    state.observe_turn_end(
        ours(
            at,
            "Nothing left: all work is merged and pushed to origin/main.",
            3 * MIN,
        ),
        &timing,
    );
    let clean = read(crate::repo::PROBE_LIMIT);
    assert_eq!(clean, RepoRead::Read(None));
    state.probed(clean);
    assert_eq!(
        decide(&state, &land(), &timing, at, NO_JITTER).act,
        Act::Landed
    );
}

#[test]
fn unread_repository_reads_never_keep_a_landing_by_same_work() {
    let now = Instant::now();
    let timing = Timing::default();
    let at = now + 60 * MIN;
    let known = RepoRead::Read(Some(stated(7)));
    for began in [RepoRead::Unread, known.clone(), RepoRead::Read(None)] {
        for ended in [RepoRead::Unread, known.clone(), RepoRead::Read(None)] {
            let mut state = landed_at(now, &timing);
            state.wake_began_over(began.clone());
            state.observe_turn_end(wake(at, QUIET), &timing);
            assert_eq!(
                decide(&state, &land(), &timing, at, NO_JITTER).act,
                Act::Probe
            );
            state.probed(ended.clone());
            let result = decide(&state, &land(), &timing, at, NO_JITTER);
            let kept = ended.is_clean() || RepoRead::same(began.as_ref(), ended.as_ref());
            assert_eq!(
                state.landed, kept,
                "began={began:?}, ended={ended:?}: {result:?}"
            );
            assert!(!state.judging(), "the read must settle the judgment");
            if kept {
                // Recorded again for aterm, which reopened it at the wake's turn (review of
                // gap 12, finding 1).
                assert_eq!(result.act, Act::Landed);
                assert!(state.reland);
            } else {
                assert!(!state.reland);
                let (rule, text) = typed(&result).expect("Land resumes automatically");
                assert_eq!(rule, words::RULE_LAND_NUDGE);
                assert_eq!(
                    text.contains(words::REPO_UNREAD),
                    matches!(ended, RepoRead::Unread)
                );
                assert_eq!(
                    text.contains("still holds work"),
                    matches!(ended.as_ref(), RepoRead::Read(Some(_)))
                );
                assert_eq!(state.reopened, ended);
            }
        }
    }
    let mut off = landed_at(now, &timing);
    off.park_off();
    off.probed(RepoRead::Unread);
    assert!(!off.landed && !off.judging());
    assert_eq!(
        typed(&decide(&off, &land(), &timing, at, NO_JITTER)).map(|(rule, _)| rule),
        Some(words::RULE_LAND)
    );
}

#[test]
fn unread_repository_reads_guard_carried_and_resumed_prompts() {
    let now = Instant::now();
    let timing = Timing::default();
    for p in [policy(), land()] {
        let mut state = ThreadState {
            task: true,
            owed: Some(Owed {
                native_origin: None,
                rule: if p.autopilot == Mode::Land {
                    words::RULE_LAND_NUDGE
                } else {
                    words::RULE_DONE_CHECK
                },
                text: if p.autopilot == Mode::Land {
                    words::LAND_NUDGE
                } else {
                    words::DONE_CHECK
                }
                .into(),
                mode: p.autopilot,
                not_before: now,
                repo_owed: Some(Vec::new()),
                resume_goal: false,
                landing: false,
            }),
            ..Default::default()
        };
        state.observe_mode(p.autopilot);
        assert_eq!(decide(&state, &p, &timing, now, NO_JITTER).act, Act::Probe);
        state.probed(RepoRead::Unread);
        let result = decide(&state, &p, &timing, now, NO_JITTER);
        let (_, text) = typed(&result).expect("owed act remains automatic");
        let unread = if p.autopilot == Mode::Land {
            words::REPO_UNREAD
        } else {
            words::REPO_UNREAD_OWN
        };
        assert_eq!(text.matches(unread).count(), 1);
        assert!(!text.contains("still holds work"));
    }
    let mut state = ThreadState::default();
    state.observe_resume(true, Some(CLEAN), true);
    state.observe_mode(Mode::Cruise);
    state.probed(RepoRead::Unread);
    let result = decide(&state, &policy(), &timing, now, NO_JITTER);
    assert!(typed(&result).is_some_and(|(_, text)| text.contains(words::REPO_UNREAD_OWN)));
}

#[test]
fn a_cyber_refusal_excludes_the_action_and_continues_independent_work_only_once() {
    let now = Instant::now();
    let timing = Timing::default();
    let mut state = ThreadState {
        task: true,
        ..Default::default()
    };
    state.observe_turn_end(walled(now, Wall::Policy), &timing);
    let first = decide(&state, &policy(), &timing, now, NO_JITTER);
    assert_eq!(
        typed(&first),
        Some((words::RULE_POLICY_CONTINUE, words::POLICY_CONTINUE))
    );
    assert!(
        !crate::decide::is_wall_rule(words::RULE_POLICY_CONTINUE),
        "the refused request gets no retry ladder"
    );
    assert_eq!(
        decide(
            &state,
            &Policy {
                continue_on: false,
                ..policy()
            },
            &timing,
            now,
            NO_JITTER
        )
        .act,
        Act::Nothing
    );
    state.acted(&first.act, now, &timing);
    state.observe_turn_end(
        ours(now + MIN * 4, "Reviewed independent tests.", MIN * 4),
        &timing,
    );
    let next = decide(&state, &policy(), &timing, now + MIN * 4, NO_JITTER);
    assert!(typed(&next).is_some_and(|(_, text)| text.ends_with(words::POLICY_BOUNDARY)));
    state.acted(&next.act, now + MIN * 4, &timing);
    state.observe_turn_end(walled(now + MIN * 8, Wall::Policy), &timing);
    assert!(matches!(
        decide(&state, &policy(), &timing, now + MIN * 8, NO_JITTER).act,
        Act::Attention { .. }
    ));
    state.observe_person_turn();
    state.observe_turn_end(walled(now + MIN * 9, Wall::Policy), &timing);
    assert_eq!(
        typed(&decide(
            &state,
            &policy(),
            &timing,
            now + MIN * 9,
            NO_JITTER
        ))
        .map(|(rule, _)| rule),
        Some(words::RULE_POLICY_CONTINUE)
    );
}

#[test]
fn the_policy_boundary_guards_resumed_acts_and_stops_when_independent_work_is_exhausted() {
    let now = Instant::now();
    let timing = Timing::default();
    let mut state = ThreadState {
        task: true,
        policy_continued: true,
        carry_on: true,
        ..Default::default()
    };
    assert!(
        typed(&decide(&state, &policy(), &timing, now, NO_JITTER))
            .is_some_and(|(_, text)| text.ends_with(words::POLICY_BOUNDARY))
    );
    for mode in [Mode::Cruise, Mode::Land] {
        state.observe_mode(mode);
        state.observe_turn_end(
            ours(
                now,
                &format!("No independent work remains.\n{}", words::POLICY_EXHAUSTED),
                MIN * 3,
            ),
            &timing,
        );
        let p = Policy {
            autopilot: mode,
            ..policy()
        };
        assert!(matches!(
            decide(&state, &p, &timing, now, NO_JITTER).act,
            Act::Attention { .. }
        ));
    }
    let state = ThreadState {
        task: true,
        policy_continued: true,
        carry_on: true,
        resumed_reply: Some(words::POLICY_EXHAUSTED.into()),
        ..Default::default()
    };
    assert!(matches!(
        decide(&state, &policy(), &timing, now, NO_JITTER).act,
        Act::Attention { .. }
    ));
}

#[test]
fn policy_exhaustion_in_an_earlier_assistant_message_stops_the_next_automatic_request() {
    let now = Instant::now();
    let timing = Timing::default();
    let mut state = ThreadState {
        task: true,
        policy_continued: true,
        point: Some(ours(
            now,
            "I have explained the remaining boundary.",
            MIN * 3,
        )),
        ..Default::default()
    };
    assert!(typed(&decide(&state, &policy(), &timing, now, NO_JITTER)).is_some());
    state.point.as_mut().expect("completed turn").earlier = Some(format!(
        "No independently authorized work remains.\n{}",
        words::POLICY_EXHAUSTED
    ));
    assert!(matches!(
        decide(&state, &policy(), &timing, now, NO_JITTER).act,
        Act::Attention { key: "wall", .. }
    ));
    state.point.as_mut().expect("completed turn").earlier = Some(format!(
        "The marker is {} when work is exhausted.",
        words::POLICY_EXHAUSTED
    ));
    assert!(typed(&decide(&state, &policy(), &timing, now, NO_JITTER)).is_some());
}

#[test]
fn exhausted_policy_work_never_overrides_eligibility_or_a_new_terminal_error() {
    let now = Instant::now();
    let timing = Timing::default();
    let base = ThreadState {
        task: true,
        policy_continued: true,
        point: Some(ours(now, words::POLICY_EXHAUSTED, MIN * 3)),
        ..Default::default()
    };
    let off = Policy {
        continue_on: false,
        ..policy()
    };
    assert_eq!(
        decide(&base, &off, &timing, now, NO_JITTER).act,
        Act::Nothing
    );
    for state in [
        ThreadState {
            task: false,
            ..base.clone()
        },
        ThreadState {
            compacting: true,
            ..base.clone()
        },
        ThreadState {
            point: Some(TurnEnd {
                aborted: true,
                ran_tools: false,
                restored: false,
                ..base.point.clone().expect("point")
            }),
            ..base.clone()
        },
    ] {
        assert_eq!(
            decide(&state, &policy(), &timing, now, NO_JITTER).act,
            Act::Nothing
        );
    }
    let mut state = base;
    state.resumed_reply = Some(words::POLICY_EXHAUSTED.into());
    state.observe_turn_end(walled(now, Wall::Login), &timing);
    assert!(matches!(
        decide(&state, &policy(), &timing, now, NO_JITTER).act,
        Act::Attention { key: "login", .. }
    ));
}

#[test]
fn every_automatic_prompt_keeps_the_policy_exclusion_including_an_owed_act() {
    let now = Instant::now();
    let timing = Timing::default();
    let base = ThreadState {
        task: true,
        policy_continued: true,
        point: Some(ours(now, "Reviewed independent tests.", MIN * 3)),
        ..Default::default()
    };
    let owed = Owed {
        native_origin: None,
        rule: words::RULE_CONTINUE,
        text: "finish all remaining work".into(),
        mode: Mode::Cruise,
        not_before: now,
        repo_owed: None,
        resume_goal: false,
        landing: false,
    };
    for (mut state, p) in [
        (base.clone(), policy()),
        (base.clone(), land()),
        (
            ThreadState {
                after_compact: true,
                ..base.clone()
            },
            policy(),
        ),
        (
            ThreadState {
                owed: Some(owed),
                ..base.clone()
            },
            policy(),
        ),
        (
            ThreadState {
                point: Some(walled(
                    now,
                    Wall::Server {
                        what: "a server error".into(),
                        cut_off: false,
                        retry_at: None,
                    },
                )),
                ..base.clone()
            },
            policy(),
        ),
        (
            ThreadState {
                point: Some(walled(
                    now,
                    Wall::Usage {
                        reset_at: Some(now),
                        reset_label: None,
                    },
                )),
                ..base
            },
            policy(),
        ),
    ] {
        state.observe_mode(p.autopilot);
        let decision = decide(&state, &p, &timing, now, NO_JITTER);
        let text = match &decision.act {
            Act::Type { text, .. } | Act::ResumeLimit { text } => text,
            _ => panic!("expected automatic words: {decision:?}"),
        };
        assert!(text.ends_with(words::POLICY_BOUNDARY), "{decision:?}");
        assert_eq!(text.matches(words::POLICY_BOUNDARY).count(), 1);
    }
}

fn policy() -> Policy {
    Policy {
        continue_on: true,
        continue_text: Some("keep going".to_string()),
        continue_per_hour: None,
        approve_all: true,
        answer_questions: true,
        answer_text: Some("Pick the recommended option.".to_string()),
        questions: Questions::Recommended,
        retry_api_errors: true,
        probe_api: true,
        resume_limits: true,
        limit_wait: true,
        compact_on_context_wall: true,
        upgrade: true,
        upgrade_ask: false,
        facts: false,
        human_grace: Duration::from_secs(120),
        autopilot: Mode::Cruise,
        texts: Texts::default(),
    }
}

fn land() -> Policy {
    Policy {
        autopilot: Mode::Land,
        ..policy()
    }
}

/// What the repository probe finds where the tests say it holds work.
fn work() -> RepoWork {
    RepoWork::of_git("/r", "?? new.rs\0", "", 1, 1, "origin/main").unwrap_or_default()
}

fn end(at: Instant, text: &str, worked: Duration) -> TurnEnd {
    TurnEnd {
        at,
        by_autopilot: false,
        by_person: true,
        text: Some(text.to_string()),
        earlier: None,
        questions: false,
        worked,
        wall: None,
        aborted: false,
        ran_tools: false,
        restored: false,
    }
}

fn ours(at: Instant, text: &str, worked: Duration) -> TurnEnd {
    TurnEnd {
        by_autopilot: true,
        by_person: false,
        ..end(at, text, worked)
    }
}

/// `end` with a tool run in it: ruling F's real work, however short ([`TurnEnd::ran_tools`]).
fn tooled(end: TurnEnd) -> TurnEnd {
    TurnEnd {
        ran_tools: true,
        ..end
    }
}

fn walled(at: Instant, wall: Wall) -> TurnEnd {
    TurnEnd {
        wall: Some(wall),
        text: None,
        ..ours(at, "", Duration::from_secs(5))
    }
}

fn typed(d: &Decision) -> Option<(&'static str, &str)> {
    match &d.act {
        Act::Type { rule, text } => Some((rule, text.as_str())),
        _ => None,
    }
}

/// The decision at the point, the mode folded first (as the runtime does) and the repository
/// probe the decider asks for answered with `found`.
fn probed(
    s: &mut ThreadState,
    p: &Policy,
    t: &Timing,
    now: Instant,
    found: Option<RepoWork>,
) -> Decision {
    s.observe_mode(p.autopilot);
    let d = decide(s, p, t, now, NO_JITTER);
    if d.act != Act::Probe {
        return d;
    }
    s.probed(found);
    decide(s, p, t, now, NO_JITTER)
}

/// [`probed`] over a clean repository, then carried out at `now`.
fn run(s: &mut ThreadState, p: &Policy, t: &Timing, now: Instant) -> Decision {
    let d = probed(s, p, t, now, None);
    s.acted(&d.act, now, t);
    d
}

#[test]
fn no_act_before_a_persons_turn_or_a_turn_of_this_process() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    // A resume's idle: no LastTurn here.
    assert_eq!(decide(&s, &policy(), &t, now, NO_JITTER).act, Act::Nothing);
    // A turn ended, but no person ever asked anything (an automatic turn).
    s.observe_turn_end(ours(now, "Done a thing.", 5 * MIN), &t);
    s.task = false;
    assert_eq!(decide(&s, &policy(), &t, now, NO_JITTER).act, Act::Nothing);
    // Negative control: the person's turn opens the task.
    s.observe_turn_end(end(now, "Done a thing.", 5 * MIN), &t);
    assert_eq!(
        typed(&decide(&s, &policy(), &t, now, NO_JITTER)),
        Some((words::RULE_CONTINUE, "keep going"))
    );
}

/// THE CARRY-ON after aterm's relaunch types the mode's act whatever the last words said
/// (FOREVER CRUISE never rests on a done claim): the continuation, or the defend prompt where
/// the resumed reply claimed done (once the repository is probed). NEGATIVE CONTROLS: a plain
/// resume, and a relaunch with no person's turn in the history, type nothing.
#[test]
fn carry_on_after_a_relaunch_whatever_the_last_words_said() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_resume(true, Some("Working through stage 2."), true);
    assert_eq!(
        typed(&decide(&s, &policy(), &t, now, NO_JITTER)),
        Some((words::RULE_HOST_TURN, "keep going"))
    );
    for last in [
        "All landed.\nATERM-TASK-DONE",
        "All done, nothing left to do.",
    ] {
        let mut s = ThreadState::default();
        s.observe_resume(true, Some(last), true);
        assert_eq!(
            decide(&s, &policy(), &t, now, NO_JITTER).act,
            Act::Probe,
            "{last}"
        );
        s.probed(Some(work()));
        assert_eq!(
            typed(&decide(&s, &policy(), &t, now, NO_JITTER)),
            Some((
                words::RULE_HOST_TURN,
                act_words(
                    words::DONE_CHECK,
                    &[&words::own_repo_suffix(&work().summary())]
                )
                .as_str()
            )),
            "{last}"
        );
    }
    // A plain resume (the person ran `codex resume`): nothing until their turn.
    let mut s = ThreadState::default();
    s.observe_resume(true, Some("Working through stage 2."), false);
    assert_eq!(decide(&s, &policy(), &t, now, NO_JITTER).act, Act::Nothing);
    // A relaunch with no person's turn in the history: taskless.
    let mut s = ThreadState::default();
    s.observe_resume(false, Some("Hello."), true);
    assert_eq!(decide(&s, &policy(), &t, now, NO_JITTER).act, Act::Nothing);
}

/// FOREVER CRUISE NEVER ENDS A TASK ON THE WORKER'S WORDS ("Cruise is always looking for more
/// completeness and perfection"): the retired token, a done phrase and a `Nothing left:`
/// defense are each a done claim, owed the defend-and-find-more prompt at every claim, paced
/// by the short streak's back-off (cost control, never a stop). NEGATIVE CONTROL: a report of
/// progress gets the continuation.
#[test]
fn cruise_never_ends_a_task_on_the_workers_words() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Pushed.\nATERM-TASK-DONE", 5 * MIN), &t);
    assert_eq!(decide(&s, &policy(), &t, now, NO_JITTER).act, Act::Probe);
    let d = run(&mut s, &policy(), &t, now);
    assert_eq!(typed(&d), Some((words::RULE_DONE_CHECK, words::DONE_CHECK)));
    assert_eq!(d.not_before, None);
    let mut at = now;
    for (claim, wait) in [
        ("Everything is done.", 2),
        ("All done, nothing left to do.", 4),
        ("ATERM-TASK-DONE", 8),
        (
            "Nothing left: every item is merged and pushed to origin/main.",
            16,
        ),
    ] {
        at += 60 * MIN;
        s.observe_turn_end(ours(at, claim, 3 * MIN), &t);
        let d = probed(&mut s, &policy(), &t, at, None);
        assert_eq!(
            typed(&d),
            Some((words::RULE_DONE_CHECK, words::DONE_CHECK)),
            "{claim}"
        );
        assert_eq!(d.not_before, Some(at + wait * MIN), "{claim}");
        assert!(!s.landed, "{claim}");
        s.acted(&d.act, at + wait * MIN, &t);
    }
    // A report of progress: the continuation.
    s.observe_turn_end(end(at, "Fixed the next bug.", 5 * MIN), &t);
    assert_eq!(
        typed(&decide(&s, &policy(), &t, at, NO_JITTER)),
        Some((words::RULE_CONTINUE, "keep going"))
    );
}

/// THE DEFEND PROMPT CARRIES WHAT IS STILL OPEN: the open work the claim names and what the
/// repository holds that no remote main has, as suffixes; an irreversible act the claim names
/// adds the reversible clause. In aterm's order (`words_act`): the open work, the repository,
/// the reversible clause, the ack, then the act's one copy of the standing rules aterm sent
/// the prompt composed with; the same words where the previous image owed the prompt
/// decided before the repository was read. NEGATIVE CONTROL: nothing open over a clean
/// repository is the prompt alone.
#[test]
fn the_defend_prompt_names_the_open_work_and_the_repository() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_turn_end(
        end(
            now,
            "Committed the parser. Left: the lexer.\nATERM-TASK-DONE",
            5 * MIN,
        ),
        &t,
    );
    let d = probed(&mut s, &policy(), &t, now, Some(work()));
    let open = open_work_suffix(&["left"]).unwrap_or_default();
    // Cruise asks for the agent's own work (aterm's `RepoAsk::Own`, `f9a7dbd4c4`).
    let repo = words::own_repo_suffix(&work().summary());
    assert_eq!(
        typed(&d),
        Some((
            words::RULE_DONE_CHECK,
            act_words(words::DONE_CHECK, &[&open, &repo]).as_str()
        ))
    );
    assert!(repo.contains("1 commit not on origin/main"), "{repo}");
    // The repository before the ack, the standing rules after both.
    let rules = " (standing rules: never push to main)";
    let mut p = policy();
    p.texts.done_check = format!("{}{rules}", words::DONE_CHECK);
    let claim = "All done, nothing left to do.\nNEEDS YOU: the signing key password";
    let want = format!("{} {repo} {NEEDS_ACK}{rules}", words::DONE_CHECK);
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, claim, 5 * MIN), &t);
    assert_eq!(
        typed(&probed(&mut s, &p, &t, now, Some(work()))),
        Some((words::RULE_DONE_CHECK, want.as_str()))
    );
    let decided = format!("{} {NEEDS_ACK}{rules}", words::DONE_CHECK);
    let owed = Owed {
        native_origin: None,
        rule: words::RULE_DONE_CHECK,
        text: decided,
        mode: Mode::Cruise,
        not_before: now,
        repo_owed: Some(Vec::new()),
        resume_goal: false,
        landing: false,
    };
    let mut s = re_execed(claim, Some(owed), false, now);
    assert_eq!(
        typed(&probed(&mut s, &p, &t, now, Some(work()))),
        Some((words::RULE_DONE_CHECK, want.as_str()))
    );
    // NEGATIVE CONTROL.
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "All done, nothing left to do.", 5 * MIN), &t);
    assert_eq!(
        typed(&probed(&mut s, &policy(), &t, now, None)),
        Some((words::RULE_DONE_CHECK, words::DONE_CHECK))
    );
}

/// CRUISE'S REPOSITORY ASK IS THE AGENT'S OWN WORK (aterm's `RepoAsk::Own`, `REPO_OWN` and
/// `REPO_UNREAD_OWN`, aterm `f9a7dbd4c4`: "Angle 6 contradicted its own act ... told the agent
/// to land the lab repository's other-session stash while angle 6 said to leave it"). The probe
/// reads the whole checkout, a person's untracked notes and another session's stash included,
/// and Cruise's done check said "commit, merge and push it to the remote main first" of all of
/// it: the agent pushed the person's file. Under Cruise the found work and the unread probe ask
/// it to land what of it it made and leave the rest exactly as it is, never all of it.
/// NEGATIVE CONTROL: Land's nudge over the same finding, and over the same unread probe, still
/// asks all of it ("if landing LAND EVERYTHING INTO REMOTE MAIN NO EXCEPTIONS").
#[test]
fn cruises_repository_ask_lands_only_what_the_agent_made() {
    const OWN: &str =
        "land what of it you made first, and leave what you did not make exactly as it is.";
    const ALL: &str = "commit, merge and push it to the remote main first.";
    const UNREAD_OWN: &str = "Your repository could not be read: show its git status yourself, \
                              land what of it you made first, and leave what you did not make \
                              exactly as it is.";
    let t = Timing::default();
    let now = Instant::now();
    // A person's untracked notes in the thread's checkout.
    let theirs = RepoWork {
        repo: "/r".to_string(),
        untracked: 1,
        unlanded: 1,
        main_ref: "origin/main".to_string(),
        paths: vec!["notes.md".to_string()],
        ..RepoWork::default()
    };
    let named = format!(
        "Your repository still holds work no remote main has ({}): ",
        theirs.summary()
    );
    let claim = "All tasks are complete.";
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, claim, 5 * MIN), &t);
    let d = probed(&mut s, &policy(), &t, now, Some(theirs.clone()));
    let (rule, text) = typed(&d).unwrap_or_else(|| panic!("the done check: {d:?}"));
    assert_eq!(rule, words::RULE_DONE_CHECK);
    assert!(text.contains(&format!("{named}{OWN}")), "{text}");
    assert!(!text.contains(ALL), "{text}");
    // The unread probe under Cruise.
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, claim, 5 * MIN), &t);
    s.observe_mode(Mode::Cruise);
    assert_eq!(decide(&s, &policy(), &t, now, NO_JITTER).act, Act::Probe);
    s.probed(RepoRead::Unread);
    let d = decide(&s, &policy(), &t, now, NO_JITTER);
    let (_, text) = typed(&d).unwrap_or_else(|| panic!("the done check: {d:?}"));
    assert!(text.contains(UNREAD_OWN), "{text}");
    assert!(!text.contains(words::REPO_UNREAD), "{text}");
    // NEGATIVE CONTROL: Land's nudge asks all of it.
    let p = land();
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Ship the parser.", 5 * MIN), &t);
    assert_eq!(
        typed(&run(&mut s, &p, &t, now)),
        Some((words::RULE_LAND, words::LAND))
    );
    let at = now + 20 * MIN;
    s.observe_turn_end(ours(at, claim, 10 * MIN), &t);
    let due = decide(&s, &p, &t, at, NO_JITTER).not_before.unwrap_or(at);
    let d = probed(&mut s, &p, &t, due, Some(theirs));
    let (rule, text) = typed(&d).unwrap_or_else(|| panic!("the nudge: {d:?}"));
    assert_eq!(rule, words::RULE_LAND_NUDGE);
    assert!(text.contains(&format!("{named}{ALL}")), "{text}");
    assert!(!text.contains(OWN), "{text}");
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Ship the parser.", 5 * MIN), &t);
    run(&mut s, &p, &t, now);
    s.observe_turn_end(ours(at, claim, 10 * MIN), &t);
    s.observe_mode(Mode::Land);
    assert_eq!(decide(&s, &p, &t, due, NO_JITTER).act, Act::Probe);
    s.probed(RepoRead::Unread);
    let (_, text) = typed(&decide(&s, &p, &t, due, NO_JITTER))
        .map(|(rule, text)| (rule, text.to_string()))
        .unwrap_or_else(|| panic!("the unread nudge"));
    assert!(text.contains(words::REPO_UNREAD), "{text}");
    assert!(!text.contains(UNREAD_OWN), "{text}");
}

/// EVERY ACT FITS WHAT THE COMPOSER DRAWS AS TYPED ([`words::VERBATIM_UNITS`], aterm's
/// `every_act_fits_the_verbatim_bound_with_all_its_suffixes`) with all its suffixes at once —
/// the open words (four named), the repository's finding (160 units), the reversible clause,
/// the needs ack — and the standing rules: Cruise's check and Land's nudge, each with and
/// without rules. The findings give way; the act's own words, the reversible clause and the
/// ack are typed whole, and the rules aterm sent are cut again to the room left. NEGATIVE
/// CONTROL: the suffixes joined whole overrun the bound.
#[test]
fn every_act_fits_the_verbatim_bound_with_all_its_suffixes() {
    let t = Timing::default();
    let now = Instant::now();
    let repo = RepoWork {
        repo: format!(
            "/homes/a/Developer/{}",
            "a-rather-long-checkout-name-".repeat(3)
        ),
        changed: 41,
        untracked: 17,
        stashes: 3,
        unpushed: 9,
        paths: (0..12)
            .map(|i| format!("crates/a-long-crate-name-{i}/src/a_rather_long_module_{i}.rs"))
            .collect(),
        ..Default::default()
    };
    let claim = "All done.\nStill open: the lexer.\nLeft: the docs.\nRemaining work: the e2e \
                 suite.\nNext steps: port the reader.\nTODO: wire the flag.\nNEEDS YOU: approve \
                 the force-push to main";
    let rules = "keep each commit small and name the stage it lands ".repeat(8);
    let rules = rules.trim_end();
    for (p, base) in [(policy(), words::DONE_CHECK), (land(), words::LAND_NUDGE)] {
        for rules in [None, Some(rules)] {
            let mut p = p.clone();
            // As aterm's reply sends them: each text composed with the rules, cut to fit.
            let composed = |text: &str| match rules {
                Some(r) => act_words(&format!("{text} (standing rules: {r})"), &[]),
                None => text.to_string(),
            };
            p.texts.done_check = composed(words::DONE_CHECK);
            p.texts.land = composed(words::LAND);
            p.texts.land_nudge = composed(words::LAND_NUDGE);
            let mut s = ThreadState::default();
            s.observe_turn_end(end(now, "Stage 1 landed.", 5 * MIN), &t);
            run(&mut s, &p, &t, now);
            let at = now + 20 * MIN;
            s.observe_turn_end(ours(at, claim, 10 * MIN), &t);
            // The claim's act is paced (a done claim is a short yield): read once due.
            let due = decide(&s, &p, &t, at, NO_JITTER).not_before.unwrap_or(at);
            let d = probed(&mut s, &p, &t, due, Some(repo.clone()));
            let what = format!("{:?} rules={}", p.autopilot, rules.is_some());
            let text = typed(&d)
                .unwrap_or_else(|| panic!("{what}: {d:?}"))
                .1
                .to_string();
            let units = text.encode_utf16().count();
            assert!(
                units <= words::VERBATIM_UNITS,
                "{what}: {units} units: {text}"
            );
            assert!(text.starts_with(base), "{what}: {text}");
            for whole in [words::DONE_CHECK_REVERSIBLE, NEEDS_ACK] {
                assert!(text.contains(whole), "{what}: {whole:?} in {text}");
            }
            // Land asks all of it onto the remote main; Cruise's done check the agent's own
            // work (aterm's `RepoAsk`, `f9a7dbd4c4`).
            let ask = match p.autopilot {
                Mode::Land => "commit, merge and push it to the remote main first",
                Mode::Cruise => "land what of it you made first",
            };
            assert!(text.contains(ask), "{what}: the repository's ask: {text}");
            if let Some(rules) = rules {
                assert!(
                    text.ends_with(&format!("{rules})"))
                        || text.ends_with("…)")
                        || !text.contains("(standing rules:"),
                    "{what}: the rules cut to the room left: {text}"
                );
            }
        }
    }
    // NEGATIVE CONTROL: joined whole.
    let found = crate::open_work::open_work(Some(&crate::needs::without_needs(claim)));
    assert!(found.len() > crate::open_work::OPEN_NAMED, "{found:?}");
    let whole = format!(
        "{} {} {} {} {NEEDS_ACK}",
        words::DONE_CHECK,
        open_work_suffix(&found).unwrap_or_default(),
        words::repo_suffix(&repo.summary()),
        words::DONE_CHECK_REVERSIBLE
    );
    assert!(
        whole.encode_utf16().count() > words::VERBATIM_UNITS,
        "{} units",
        whole.encode_utf16().count()
    );
}

#[test]
fn short_turns_back_off_doubling_and_real_work_ends_the_streak() {
    let t = Timing::default();
    let start = Instant::now();
    let p = policy();
    let mut s = ThreadState::default();
    s.observe_turn_end(end(start, "Stage 1 in.", 5 * MIN), &t);
    assert_eq!(run(&mut s, &p, &t, start).not_before, None);
    // Our continuation's yield is short: the next act waits 2 min from that point.
    let t1 = start + MIN;
    s.observe_turn_end(ours(t1, "Looking.", Duration::from_secs(10)), &t);
    let d = decide(&s, &p, &t, t1, NO_JITTER);
    assert_eq!(d.not_before, Some(t1 + 2 * MIN));
    s.acted(&d.act, t1 + 2 * MIN, &t);
    // A second short yield in a row: 4 min.
    let t2 = t1 + 3 * MIN;
    s.observe_turn_end(ours(t2, "Still looking.", Duration::from_secs(10)), &t);
    let d = decide(&s, &p, &t, t2, NO_JITTER);
    assert_eq!(d.not_before, Some(t2 + 4 * MIN));
    s.acted(&d.act, t2 + 4 * MIN, &t);
    // Negative control: a turn of real work ends the streak.
    let t3 = t2 + 20 * MIN;
    s.observe_turn_end(ours(t3, "Stage 2 in.", 10 * MIN), &t);
    assert_eq!(decide(&s, &p, &t, t3, NO_JITTER).not_before, None);
}

#[test]
fn a_persons_short_turn_backs_the_continue_off() {
    // aterm's `TurnEndState`: someone else's turn under `min_work` is a short turn too, so
    // the continue that answers it waits the 2 min back-off (end-to-end run 2026-10-16:
    // codex-alab typed "keep going" 5 s after a 0.03 s turn).
    let t = Timing::default();
    let start = Instant::now();
    let p = policy();
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(end(start, "Hello.", Duration::from_millis(30)), &t);
    let d = decide(&s, &p, &t, start, NO_JITTER);
    assert_eq!(typed(&d), Some((words::RULE_CONTINUE, "keep going")));
    assert_eq!(d.not_before, Some(start + 2 * MIN));
    // Its yield, short again: 4 min.
    s.acted(&d.act, start + 2 * MIN, &t);
    let t1 = start + 3 * MIN;
    s.observe_turn_end(ours(t1, "Still here.", Duration::from_secs(1)), &t);
    assert_eq!(
        decide(&s, &p, &t, t1, NO_JITTER).not_before,
        Some(t1 + 4 * MIN)
    );
    // Negative control: a person's turn of real work starts no streak.
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(end(start, "Stage 1 in.", 5 * MIN), &t);
    assert_eq!(decide(&s, &p, &t, start, NO_JITTER).not_before, None);
}

/// The most aterm's Waiting row shows of a wait's words (`needs.rs` `ROW_WHAT_MAX`, aterm main
/// `8ffb44c7da`): past it the row is cut, and a clock time at the end is lost.
const WAITING_ROW: usize = 72;

/// `decided`'s wait, due at `at` on the clock `now` reads: `words` and the clock time its act
/// types, the wall clock plus the wait left. The wall clock is read on each side of the
/// decision, which can fall across a minute's turn, so either reading is the time.
fn shows_types_again(
    decided: impl Fn() -> Decision,
    words: &str,
    at: Instant,
    now: Instant,
) -> Decision {
    let clock = || {
        let left = chrono::Duration::from_std(at.saturating_duration_since(now))
            .unwrap_or_else(|_| chrono::Duration::zero());
        format!(
            "{words}{}{}",
            words::TYPES_AGAIN,
            (chrono::Local::now() + left).format("%H:%M")
        )
    };
    let before = clock();
    let d = decided();
    let after = clock();
    assert_eq!(d.not_before, Some(at), "{d:?}");
    let shown = d.shown_wait.clone().unwrap_or_default();
    assert!(
        shown == before || shown == after,
        "{shown:?}, not {before:?} or {after:?}"
    );
    assert!(shown.len() <= WAITING_ROW, "aterm's row cuts {shown:?}");
    d
}

/// THE BACK-OFF AND THE HOUR'S CAP ARE WAITS A PERSON SEES (amended 2026-10-27 after the
/// parity sweep, gap 2: both showed nothing, so a backed-off codex tab looked idle): a
/// continuation a standing short streak backs off shows aterm's words for it with the clock
/// time it types, and one the hour's budget holds names the cap, each whole in aterm's
/// Waiting row (review of f134d2b2cb, findings 2 and 8: the cap's time was cut, and no time
/// was checked); a wake settle that moves the act later keeps the back-off's words without
/// their time; a look past the rung shows none. NEGATIVE CONTROL: with no streak and no cap
/// the act goes at once and shows no wait.
#[test]
fn a_backed_off_or_capped_continuation_shows_its_wait() {
    let t = Timing::default();
    let start = Instant::now();
    let p = policy();
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(end(start, "Hello.", Duration::from_millis(30)), &t);
    assert_eq!(s.short_streak, 1);
    let backoff = words::short_backoff_wait(1, 2 * MIN);
    shows_types_again(
        || decide(&s, &p, &t, start, NO_JITTER),
        &backoff,
        start + 2 * MIN,
        start,
    );
    let past = decide(&s, &p, &t, start + 3 * MIN, NO_JITTER);
    assert_eq!(past.shown_wait, None, "a rung passed: {past:?}");
    // The host woke just before the back-off's end: its settle types later than that time.
    let mut woken = s.clone();
    let woke = start + 2 * MIN - t.wake_settle / 2;
    woken.woke = Some(woke);
    let d = decide(&woken, &p, &t, woke + Duration::from_secs(1), NO_JITTER);
    assert_eq!(d.not_before, Some(woke + t.wake_settle), "{d:?}");
    let shown = d.shown_wait.unwrap_or_default();
    assert!(shown.starts_with(&format!("{backoff}; ")), "{shown:?}");
    assert!(!shown.contains(words::TYPES_AGAIN), "{shown:?}");
    // The longest back-off fits the row too.
    let mut long = s.clone();
    long.short_streak = 17;
    let at = start + t.short_backoff_max;
    let longest = words::short_backoff_wait(17, t.short_backoff_max);
    shows_types_again(
        || decide(&long, &p, &t, start, NO_JITTER),
        &longest,
        at,
        start,
    );
    // Under two minutes the back-off is in seconds, from two on in whole minutes, as aterm
    // says it.
    assert!(words::short_backoff_wait(3, Duration::from_secs(119)).ends_with(" 119 s back-off"));
    assert!(words::short_backoff_wait(3, 2 * MIN).ends_with(" 2 min back-off"));
    let capped = Policy {
        continue_per_hour: Some(1),
        ..policy()
    };
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(end(start, "Stage 1 in.", 5 * MIN), &t);
    s.typed = std::collections::VecDeque::from([start]);
    let now = start + 10 * MIN;
    shows_types_again(
        || decide(&s, &capped, &t, now, NO_JITTER),
        words::HOUR_CAP,
        start + 60 * MIN,
        now,
    );
    // NEGATIVE CONTROL: no streak, no cap.
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(end(start, "Stage 1 in.", 5 * MIN), &t);
    assert_eq!(s.short_streak, 0);
    let d = decide(&s, &p, &t, start, NO_JITTER);
    assert_eq!((d.not_before, d.shown_wait), (None, None), "{:?}", d.why);
}

/// A PERSON'S TURN LEAVES THE SHORT STREAK STANDING (aterm's `TurnEndState`: "the streak
/// stands", someone else's turn moves it by `streak(short, d, false)` at its end and nothing
/// resets it at its start): a short one lengthens it, so the act that answers it backs off
/// past the streak codex-alab's own short yields built; only real work ends it. NEGATIVE
/// CONTROL: a person's turn of real work ends the streak, and the act goes at once.
#[test]
fn a_persons_turn_leaves_the_short_streak_standing() {
    let t = Timing::default();
    let start = Instant::now();
    let p = policy();
    let streak_of_three = || {
        let mut s = ThreadState::default();
        s.observe_turn_end(end(start, "Stage 1 in.", 5 * MIN), &t);
        run(&mut s, &p, &t, start);
        let mut at = start;
        // Each short turn in its own words: the same reply twice would be ruling F's stop.
        for (wait, said) in [
            (2, "Looking at the parser."),
            (4, "Looking at the lexer."),
            (8, "Looking at the tests."),
        ] {
            at += MIN;
            s.observe_turn_end(ours(at, said, Duration::from_secs(10)), &t);
            let d = decide(&s, &p, &t, at, NO_JITTER);
            assert_eq!(d.not_before, Some(at + wait * MIN));
            at += wait * MIN;
            s.acted(&d.act, at, &t);
        }
        (s, at + MIN)
    };
    let (mut s, at) = streak_of_three();
    s.observe_person_turn();
    s.observe_turn_end(end(at, "Noted, carrying on.", Duration::from_secs(20)), &t);
    let d = decide(&s, &p, &t, at, NO_JITTER);
    assert_eq!(typed(&d), Some((words::RULE_CONTINUE, "keep going")));
    assert_eq!(d.not_before, Some(at + 16 * MIN), "the fourth short turn");
    // NEGATIVE CONTROL: a person's turn of real work ends it.
    let (mut s, at) = streak_of_three();
    s.observe_person_turn();
    s.observe_turn_end(end(at, "I fixed the parser.", 5 * MIN), &t);
    assert_eq!(decide(&s, &p, &t, at, NO_JITTER).not_before, None);
}

/// A LEAD'S WORKER IS ANSWERED AT ONCE AT ITS FIRST SHORT DONE CLAIMS AND QUESTIONS (aterm's
/// `waited_on`, `MANAGED_SHORT_TURNS`; parity sweep gap 5, its managed half): where a lead
/// waits on the session, a done claim on the first, second or third short turn in a row probes
/// at once and its prompt goes when the probe is in, and a question is answered at once; the
/// fourth short turn backs off as on any tab. NEGATIVE CONTROLS: the same done claim and the
/// same question back off where no lead waits; a lead's worker's plain continuation (no claim,
/// nothing asked) keeps the ladder.
#[test]
fn a_leads_worker_is_answered_at_once_at_its_first_short_done_claims_and_questions() {
    let t = Timing::default();
    let start = Instant::now();
    let p = policy();
    let done = "All done, nothing left to do.";
    // The `n`th short turn in a row, ending in `end`'s words, under a lead or none.
    let point = |n: u32, end: TurnEnd, managed: bool| {
        let mut s = ThreadState {
            short_streak: n - 1,
            ..ThreadState::default()
        };
        s.observe_person_turn();
        s.observe_turn_end(end, &t);
        assert_eq!(s.short_streak, n);
        s.managed = managed;
        s
    };
    let short = |text: &str| end(start, text, Duration::from_secs(10));
    let backoff = |n: u32| start + 2 * MIN * (1 << (n - 1));
    for n in 1..=MANAGED_SHORT_TURNS {
        let mut s = point(n, short(done), true);
        let d = decide(&s, &p, &t, start, NO_JITTER);
        assert_eq!(
            (&d.act, d.not_before),
            (&Act::Probe, None),
            "claim {n}: {d:?}"
        );
        s.probed_for(s.point_at(), Some(work()));
        let d = decide(&s, &p, &t, start, NO_JITTER);
        assert_eq!(
            typed(&d).map(|(rule, _)| rule),
            Some(words::RULE_DONE_CHECK)
        );
        assert_eq!(d.not_before, None, "claim {n}'s prompt: {d:?}");
        // NEGATIVE CONTROL: no lead waits, so the same claim backs off.
        let d = decide(&point(n, short(done), false), &p, &t, start, NO_JITTER);
        assert_eq!(d.not_before, Some(backoff(n)), "claim {n}, no lead: {d:?}");
    }
    // The fourth short turn backs off under a lead too.
    let d = decide(&point(4, short(done), true), &p, &t, start, NO_JITTER);
    assert_eq!(d.not_before, Some(backoff(4)), "the fourth: {d:?}");
    // A question is answered at once under a lead, and backs off with none.
    let question = || TurnEnd {
        questions: true,
        ..short("Which of the two parsers should I keep?")
    };
    let d = decide(&point(2, question(), true), &p, &t, start, NO_JITTER);
    assert!(typed(&d).is_some(), "{d:?}");
    assert_eq!(d.not_before, None, "a lead's question: {d:?}");
    let d = decide(&point(2, question(), false), &p, &t, start, NO_JITTER);
    assert_eq!(d.not_before, Some(backoff(2)), "no lead's question: {d:?}");
    // NEGATIVE CONTROL: a plain continuation keeps the ladder under a lead.
    let d = decide(
        &point(2, short("Renamed the helper."), true),
        &p,
        &t,
        start,
        NO_JITTER,
    );
    assert_eq!(typed(&d).map(|(rule, _)| rule), Some(words::RULE_CONTINUE));
    assert_eq!(
        d.not_before,
        Some(backoff(2)),
        "a plain continuation: {d:?}"
    );
}

/// LAND'S INSTRUCTION WAITS NO BACK-OFF (aterm's `instruction_due`, v6 §14.10): a switch to
/// Land types the instruction at once over a standing short streak; the nudge after it, Land's
/// answer and the same point under Cruise wait theirs, and `continue_per_hour` still caps the
/// instruction. NEGATIVE CONTROL: the point under Cruise backs off by the same streak.
#[test]
fn lands_instruction_waits_no_back_off_and_the_rest_wait_theirs() {
    let t = Timing::default();
    let start = Instant::now();
    // A person's short turn on a streak of two: the third, which backs an act off 8 min.
    let streak_at = |p: &Policy, text: &str| {
        let mut s = ThreadState {
            short_streak: 2,
            ..ThreadState::default()
        };
        s.observe_mode(p.autopilot);
        s.observe_person_turn();
        s.observe_turn_end(end(start, text, Duration::from_secs(10)), &t);
        assert_eq!(s.short_streak, 3);
        s
    };
    let rule_and_wait = |d: &Decision| (typed(d).map(|(rule, _)| rule), d.not_before);
    let short = "Renamed the helper.";
    let mut s = streak_at(&land(), short);
    let d = decide(&s, &land(), &t, start, NO_JITTER);
    assert_eq!(
        rule_and_wait(&d),
        (Some(words::RULE_LAND), None),
        "Land, its instruction untyped"
    );
    // The nudge, its instruction typed: the streak's next rung (four short turns: 16 min).
    s.acted(&d.act, start, &t);
    let then = start + MIN;
    s.observe_turn_end(ours(then, "Renamed it.", Duration::from_secs(10)), &t);
    assert_eq!(
        rule_and_wait(&decide(&s, &land(), &t, then, NO_JITTER)),
        (Some(words::RULE_LAND_NUDGE), Some(then + 16 * MIN)),
        "the nudge, its instruction typed"
    );
    let s = streak_at(&policy(), short);
    assert_eq!(
        rule_and_wait(&decide(&s, &policy(), &t, start, NO_JITTER)),
        (Some(words::RULE_CONTINUE), Some(start + 8 * MIN)),
        "the same point under Cruise"
    );
    let s = streak_at(&land(), "Should I merge the lexer first?");
    assert_eq!(
        rule_and_wait(&decide(&s, &land(), &t, start, NO_JITTER)),
        (Some(words::RULE_LAND_ANSWER), Some(start + 8 * MIN)),
        "Land's answer, its instruction untyped"
    );
    // The hour's budget holds the instruction as it holds every continuation.
    let capped = Policy {
        continue_per_hour: Some(1),
        ..land()
    };
    let mut s = streak_at(&capped, short);
    s.typed = std::collections::VecDeque::from([start]);
    assert_eq!(
        rule_and_wait(&decide(&s, &capped, &t, start + 10 * MIN, NO_JITTER)),
        (Some(words::RULE_LAND), Some(start + 60 * MIN)),
        "Land's instruction under continue_per_hour"
    );
}

#[test]
fn continue_per_hour_caps_continuations() {
    let t = Timing::default();
    let start = Instant::now();
    let p = Policy {
        continue_per_hour: Some(2),
        ..policy()
    };
    let mut s = ThreadState::default();
    s.observe_turn_end(end(start, "One.", 5 * MIN), &t);
    run(&mut s, &p, &t, start);
    s.observe_turn_end(ours(start + 10 * MIN, "Two.", 5 * MIN), &t);
    run(&mut s, &p, &t, start + 10 * MIN);
    let now = start + 20 * MIN;
    s.observe_turn_end(ours(now, "Three.", 5 * MIN), &t);
    let d = decide(&s, &p, &t, now, NO_JITTER);
    assert_eq!(d.not_before, Some(start + 60 * MIN));
    // No cap: at once. An unreadable cap: nothing is typed.
    assert_eq!(decide(&s, &policy(), &t, now, NO_JITTER).not_before, None);
    let p0 = Policy {
        continue_per_hour: Some(0),
        ..policy()
    };
    assert_eq!(decide(&s, &p0, &t, now, NO_JITTER).act, Act::Nothing);
}

#[test]
fn continue_off_or_without_text_types_nothing() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Stage 1 in.", 5 * MIN), &t);
    let off = Policy {
        continue_on: false,
        ..policy()
    };
    assert_eq!(decide(&s, &off, &t, now, NO_JITTER).act, Act::Nothing);
    let no_text = Policy {
        continue_text: None,
        ..policy()
    };
    assert_eq!(decide(&s, &no_text, &t, now, NO_JITTER).act, Act::Nothing);
}

#[test]
fn prose_questions_are_answered_by_the_policy_words() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Should I rename the module?", 5 * MIN), &t);
    // aterm's NEEDS YOU hint follows the owner's answer, never inside it.
    assert_eq!(
        typed(&decide(&s, &policy(), &t, now, NO_JITTER)),
        Some((
            words::RULE_ANSWER,
            act_words("Pick the recommended option.", &[NEEDS_HINT]).as_str()
        ))
    );
    let long = Policy {
        questions: Questions::LongTerm,
        ..policy()
    };
    assert_eq!(
        typed(&decide(&s, &long, &t, now, NO_JITTER)),
        Some((
            words::RULE_PROSE_LONG_TERM,
            act_words(words::LONG_TERM_TEXT, &[NEEDS_HINT]).as_str()
        ))
    );
    let ask = Policy {
        questions: Questions::Ask,
        ..policy()
    };
    assert!(matches!(
        decide(&s, &ask, &t, now, NO_JITTER).act,
        Act::Attention {
            key: "question",
            ..
        }
    ));
    let off = Policy {
        answer_questions: false,
        ..policy()
    };
    assert!(matches!(
        decide(&s, &off, &t, now, NO_JITTER).act,
        Act::Attention {
            key: "question",
            ..
        }
    ));
    // An async question (`questions: Some(..)`) is a question whatever the text.
    let mut s = ThreadState::default();
    s.observe_turn_end(
        TurnEnd {
            questions: true,
            ..end(now, "Here is where I am.", 5 * MIN)
        },
        &t,
    );
    assert_eq!(
        typed(&decide(&s, &policy(), &t, now, NO_JITTER)).map(|(r, _)| r),
        Some(words::RULE_ANSWER)
    );
}

#[test]
fn a_question_naming_an_irreversible_act_gets_the_reversible_answer() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Want me to force-push the branch?", 5 * MIN), &t);
    let reversible = act_words(words::REVERSIBLE_ANSWER, &[NEEDS_HINT]);
    assert_eq!(
        typed(&decide(&s, &policy(), &t, now, NO_JITTER)),
        Some((words::RULE_ANSWER, reversible.as_str()))
    );
    // Whatever the priority word says, even long-term.
    let long = Policy {
        questions: Questions::LongTerm,
        ..policy()
    };
    assert_eq!(
        typed(&decide(&s, &long, &t, now, NO_JITTER)),
        Some((words::RULE_ANSWER, reversible.as_str()))
    );
}

/// CRUISE'S ANSWERS CARRY THE STANDING RULES, as aterm's `words_act` types them
/// (`act_words(prose_words(cfg), &[NEEDS_HINT, reversible, ack], rules)` and
/// `act_words(REVERSIBLE_ANSWER, &[NEEDS_HINT, ack], rules)`): the answer, its suffixes, then
/// one copy of the rules `harness policy` composed `continue_text` with (aterm sends
/// `answer_text` raw, so the rules come from the continuation). NEGATIVE CONTROL: with no
/// rules standing, the answer is as before.
#[test]
fn cruise_answers_carry_the_standing_rules() {
    let t = Timing::default();
    let now = Instant::now();
    let rules = "Never push to upstream.";
    let ruled = Policy {
        continue_text: Some(format!("keep going (standing rules: {rules})")),
        ..policy()
    };
    let with_rules = |text: String| format!("{text} (standing rules: {rules})");
    let mut s = ThreadState::default();
    s.observe_turn_end(
        end(
            now,
            "Should I use the parser in src/a.rs or write a new one?",
            5 * MIN,
        ),
        &t,
    );
    assert_eq!(
        typed(&decide(&s, &ruled, &t, now, NO_JITTER)),
        Some((
            words::RULE_ANSWER,
            with_rules(act_words("Pick the recommended option.", &[NEEDS_HINT])).as_str()
        ))
    );
    for (questions, rule, text) in [
        (
            Questions::LongTerm,
            words::RULE_PROSE_LONG_TERM,
            words::LONG_TERM_TEXT,
        ),
        (
            Questions::ShortTerm,
            words::RULE_PROSE_SHORT_TERM,
            words::SHORT_TERM_TEXT,
        ),
    ] {
        let p = Policy {
            questions,
            ..ruled.clone()
        };
        assert_eq!(
            typed(&decide(&s, &p, &t, now, NO_JITTER)),
            Some((rule, with_rules(act_words(text, &[NEEDS_HINT])).as_str()))
        );
    }
    // The irreversible question's answer too.
    let mut s = ThreadState::default();
    s.observe_turn_end(
        end(now, "Should I delete the old branch or keep it?", 5 * MIN),
        &t,
    );
    assert_eq!(
        typed(&decide(&s, &ruled, &t, now, NO_JITTER)),
        Some((
            words::RULE_ANSWER,
            with_rules(act_words(words::REVERSIBLE_ANSWER, &[NEEDS_HINT])).as_str()
        ))
    );
    // NEGATIVE CONTROL: no rules stand, none are typed.
    assert_eq!(
        typed(&decide(&s, &policy(), &t, now, NO_JITTER)),
        Some((
            words::RULE_ANSWER,
            act_words(words::REVERSIBLE_ANSWER, &[NEEDS_HINT]).as_str()
        ))
    );
}

/// The policy aterm's `harness policy` reply makes where `rules` stand: `continue_text` (the
/// owner's `owner`) and its four act texts composed with them, each cut to its own room
/// (aterm's `with_rules`), the two suffixes bare.
fn composed(owner: &str, rules: &str) -> Policy {
    let with = |text: &str| words::with_rules(text, Some(rules));
    Policy {
        continue_text: Some(with(owner)),
        texts: Texts {
            done_check: with(words::DONE_CHECK),
            land: with(words::LAND),
            land_nudge: with(words::LAND_NUDGE),
            land_answer: with(words::LAND_ANSWER),
            from_aterm: true,
            ..Texts::default()
        },
        ..policy()
    }
}

/// THE ANSWERS TAKE THE LEAST-CUT COPY OF THE STANDING RULES (aterm's `words_act` types an
/// answer with the rules whole where they fit, `act_words(answer, [hint, …], rules)`): the
/// wire carries the rules only inside its composed texts, each cut to its own room, so an
/// owner's long `continue_text` and Land's instruction hold 377 characters of rules cut, and
/// `land_answer_text`, the shortest of aterm's act texts, holds them whole. Every answer
/// under Cruise (`answer_text`, the policy texts, the reversible answer) types them whole, as
/// aterm does. NEGATIVE CONTROL: the answer composed from `continue_text`'s cut copy, what
/// codex-alab typed before, is not aterm's.
#[test]
fn the_answers_take_the_least_cut_copy_of_the_standing_rules() {
    let t = Timing::default();
    let now = Instant::now();
    let rules = "Never push to upstream; keep every change reversible. ".repeat(7);
    let rules = rules.trim();
    let owner = "keep going ".repeat(45);
    let p = composed(owner.trim(), rules);
    let cut = p
        .continue_text
        .as_deref()
        .and_then(|text| words::split_rules(text).1)
        .unwrap_or_else(|| panic!("continue_text carries the rules"));
    assert!(cut.ends_with('…') && rules.starts_with(cut.trim_end_matches('…')));
    assert_ne!(words::split_rules(&p.texts.land).1, Some(rules));
    assert_eq!(words::split_rules(&p.texts.land_answer).1, Some(rules));
    let mut s = ThreadState::default();
    s.observe_turn_end(
        end(
            now,
            "Should I use the parser in src/a.rs or write a new one?",
            5 * MIN,
        ),
        &t,
    );
    let answer = words::act_words_ruled("Pick the recommended option.", &[NEEDS_HINT], Some(rules));
    assert_eq!(
        typed(&decide(&s, &p, &t, now, NO_JITTER)),
        Some((words::RULE_ANSWER, answer.as_str()))
    );
    for (questions, rule, text) in [
        (
            Questions::LongTerm,
            words::RULE_PROSE_LONG_TERM,
            words::LONG_TERM_TEXT,
        ),
        (
            Questions::ShortTerm,
            words::RULE_PROSE_SHORT_TERM,
            words::SHORT_TERM_TEXT,
        ),
    ] {
        let p = Policy {
            questions,
            ..p.clone()
        };
        assert_eq!(
            typed(&decide(&s, &p, &t, now, NO_JITTER)),
            Some((
                rule,
                words::act_words_ruled(text, &[NEEDS_HINT], Some(rules)).as_str()
            ))
        );
    }
    let mut irreversible = ThreadState::default();
    irreversible.observe_turn_end(
        end(now, "Should I delete the old branch or keep it?", 5 * MIN),
        &t,
    );
    assert_eq!(
        typed(&decide(&irreversible, &p, &t, now, NO_JITTER)),
        Some((
            words::RULE_ANSWER,
            words::act_words_ruled(words::REVERSIBLE_ANSWER, &[NEEDS_HINT], Some(rules)).as_str()
        ))
    );
    // NEGATIVE CONTROL.
    assert_ne!(
        words::act_words_ruled("Pick the recommended option.", &[NEEDS_HINT], Some(cut)),
        answer
    );
}

/// EVERY WALL ACT ENDS ON THE NEEDS YOU SENTENCE (aterm `448726c354`, FOREVER CRUISE /
/// FOREVER LAND: loud, never blocking): a server error's retry, a cut-off reply's and an
/// unreachable API's, under either word, end with [`NEEDS_HINT`], as aterm's wall acts end:
/// what only a person can give goes on its own line and the work goes on. NEGATIVE CONTROL: a
/// wall act worded as it stood before, ending on its carry-on alone, fails the same reading.
#[test]
fn every_wall_act_ends_on_the_needs_you_sentence() {
    let t = Timing::default();
    let now = Instant::now();
    let ends = |text: &str| text.ends_with(NEEDS_HINT);
    for cause in [
        ApiCause::Server,
        ApiCause::CutOff,
        ApiCause::Unreachable,
        ApiCause::Overlong,
    ] {
        let text = words::wall_retry_text("an API error (500)", cause);
        assert!(ends(&text), "{cause:?}: {text}");
    }
    let walls = [
        Wall::Server {
            what: "an API error (500)".to_string(),
            cut_off: false,
            retry_at: None,
        },
        Wall::Server {
            what: "a broken stream".to_string(),
            cut_off: true,
            retry_at: None,
        },
        Wall::Network {
            what: "a network error".to_string(),
        },
        Wall::Overlong {
            what: "an API error (the reply was over its output token maximum)".to_string(),
        },
    ];
    for p in [policy(), land()] {
        for wall in &walls {
            let mut s = ThreadState::default();
            s.observe_person_turn();
            s.observe_turn_end(walled(now, wall.clone()), &t);
            let d = probed(&mut s, &p, &t, now, None);
            let (rule, text) = typed(&d).unwrap_or_else(|| panic!("a wall act: {d:?}"));
            assert!(words::RULE_API_RETRY == rule || words::RULE_API_CUTOFF == rule);
            assert!(ends(text), "{:?} {wall:?}: {text}", p.autopilot);
        }
    }
    // NEGATIVE CONTROL.
    let before = "Codex reported an API error (500). Carry on from where you stopped; if the \
                  result of your last step is missing, check whether it ran before you repeat it.";
    assert!(!ends(before));
}

/// EVERY WALL ACT HANDS THE AGENT ITS OWN RESTART (aterm `11e4a76a8d`: Codex agents asked a
/// person to restart them, a request for what the agent can do itself): aterm's `needs_hint!`
/// ends on `restart_hint!`, so its wall retries (`CARRY_ON`) say it; codex-alab's answers took
/// it off aterm's wire, its wall retries never did. Each wall act typed under Cruise and Land
/// ends on aterm's sentence, word for word. codex-alab's own ack (typed only where aterm sends
/// none; aterm's, which carries the sentence, is typed where it does) stays without it: with it
/// Land's first act passed the 800-unit bound and cut a short standing rule
/// (`words_tests.rs` `lands_first_act_keeps_a_short_standing_rule_beside_both_its_clauses`).
/// NEGATIVE CONTROLS: the 2026-11-01 operating rule's own sentence stays before it (aterm's
/// NEEDS YOU sentence is not revived), and a retry with no restart sentence does not pass.
#[test]
fn every_wall_act_hands_the_agent_its_own_restart() {
    const RESTART: &str =
        "Handle agent restarts yourself: save a handoff; `aterm ctl help restart`.";
    let t = Timing::default();
    let now = Instant::now();
    assert!(words::CARRY_ON.ends_with(RESTART), "{}", words::CARRY_ON);
    assert!(NEEDS_HINT.ends_with(RESTART), "{NEEDS_HINT}");
    assert!(!NEEDS_ACK.contains(RESTART), "{NEEDS_ACK}");
    for cause in [
        ApiCause::Server,
        ApiCause::CutOff,
        ApiCause::Unreachable,
        ApiCause::Overlong,
    ] {
        let text = words::wall_retry_text("an API error (500)", cause);
        assert!(text.ends_with(RESTART), "{cause:?}: {text}");
    }
    let walls = [
        Wall::Server {
            what: "an API error (500)".to_string(),
            cut_off: false,
            retry_at: None,
        },
        Wall::Network {
            what: "a network error".to_string(),
        },
    ];
    for p in [policy(), land()] {
        for wall in &walls {
            let mut s = ThreadState::default();
            s.observe_person_turn();
            s.observe_turn_end(walled(now, wall.clone()), &t);
            let d = probed(&mut s, &p, &t, now, None);
            let (_, text) = typed(&d).unwrap_or_else(|| panic!("a wall act: {d:?}"));
            assert!(
                text.ends_with(RESTART),
                "{:?} {wall:?}: {text}",
                p.autopilot
            );
        }
    }
    // NEGATIVE CONTROLS.
    assert!(
        NEEDS_HINT.starts_with("Decide and record why for later AI review."),
        "{NEEDS_HINT}"
    );
    assert!(!NEEDS_HINT.contains("NEEDS YOU:"), "{NEEDS_HINT}");
    let without = words::CARRY_ON.replace(RESTART, "");
    assert!(!without.trim_end().ends_with(RESTART));
}

/// A WALL'S RETRY CARRIES THE STANDING RULES, as aterm's wall acts do (`type_as`:
/// `with_rules` after `wall_retry_text`): a server error's retry, a cut-off reply's and an
/// unreachable API's, under either word, end with the one copy of the rules aterm composed
/// its texts with, after the NEEDS YOU sentence. NEGATIVE CONTROL: none standing, the retry
/// is aterm's words alone.
#[test]
fn a_walls_retry_carries_the_standing_rules() {
    let t = Timing::default();
    let now = Instant::now();
    let rules = "Never push to upstream.";
    let walls = [
        (
            Wall::Server {
                what: "an API error (500)".to_string(),
                cut_off: false,
                retry_at: None,
            },
            words::wall_retry_text("an API error (500)", ApiCause::Server),
        ),
        (
            Wall::Server {
                what: "an API error (the reply was cut off)".to_string(),
                cut_off: true,
                retry_at: None,
            },
            words::wall_retry_text("an API error (the reply was cut off)", ApiCause::CutOff),
        ),
        (
            Wall::Network {
                what: "an API error (the API could not be reached)".to_string(),
            },
            words::wall_retry_text(
                "an API error (the API could not be reached)",
                ApiCause::Unreachable,
            ),
        ),
        (
            Wall::Overlong {
                what: "an API error (the reply was over its output token maximum)".to_string(),
            },
            words::wall_retry_text(
                "an API error (the reply was over its output token maximum)",
                ApiCause::Overlong,
            ),
        ),
    ];
    let ruled = composed("keep going", rules);
    for (p, with) in [
        (ruled.clone(), Some(rules)),
        (
            Policy {
                autopilot: Mode::Land,
                ..ruled
            },
            Some(rules),
        ),
        (policy(), None),
        (land(), None),
    ] {
        for (wall, words_alone) in &walls {
            let mut s = ThreadState::default();
            s.observe_person_turn();
            s.observe_turn_end(walled(now, wall.clone()), &t);
            let d = probed(&mut s, &p, &t, now, None);
            let (_, text) = typed(&d).unwrap_or_else(|| panic!("a wall act: {d:?}"));
            let want = match with {
                Some(rules) => format!("{words_alone} (standing rules: {rules})"),
                None => words_alone.clone(),
            };
            assert_eq!(text, want, "{:?} {wall:?}", p.autopilot);
        }
    }
}

/// NO RULES STAND WHERE aterm SENDS ITS ACT TEXTS BARE (aterm's `harness policy` sends
/// `done_check_text`, `land_text`, `land_nudge_text` and `land_answer_text` always, composed
/// with the rules only where some stand): an owner's `continue_text` that itself ends `…
/// (standing rules: X)` is the owner's words, not a copy of rules, and neither a wall's retry
/// nor an answer types them, as aterm's `type_as(with_rules(text, None))` types none. Before,
/// the owner's text was split as the one copy there is. NEGATIVE CONTROLS: an aterm that
/// sends no act texts leaves `continue_text`'s copy the one there is, and act texts composed
/// with rules give theirs.
#[test]
fn no_rules_stand_where_aterm_sends_its_act_texts_bare() {
    let t = Timing::default();
    let now = Instant::now();
    let reply = |act_texts: &str| {
        let line = format!(
            "OK gen=1 effective=on why=enabled continue=true \
             continue_text=Keep%20going%20(standing%20rules:%20commit%20often) \
             answer_questions=true questions=recommended answer_text=Pick%20one. \
             retry_api_errors=true continue_per_hour=0{act_texts}"
        );
        match codex_alab_aterm::policy::PolicyAnswer::parse(&line) {
            codex_alab_aterm::policy::PolicyAnswer::Supported(reply) => {
                Policy::from_harness(&reply)
            }
            other => panic!("a supported reply: {other:?}"),
        }
    };
    let server = Wall::Server {
        what: "an API error (500)".to_string(),
        cut_off: false,
        retry_at: None,
    };
    let retry_words = words::wall_retry_text("an API error (500)", ApiCause::Server);
    let typed_at = |p: &Policy| {
        let mut walled_s = ThreadState::default();
        walled_s.observe_person_turn();
        walled_s.observe_turn_end(walled(now, server.clone()), &t);
        let retry = typed(&decide(&walled_s, p, &t, now, NO_JITTER))
            .map(|(_, text)| text.to_string())
            .unwrap_or_else(|| panic!("a retry"));
        let mut asked = ThreadState::default();
        asked.observe_turn_end(
            end(
                now,
                "Should I use the parser in src/a.rs or write a new one?",
                5 * MIN,
            ),
            &t,
        );
        let answer = typed(&decide(&asked, p, &t, now, NO_JITTER))
            .map(|(_, text)| text.to_string())
            .unwrap_or_else(|| panic!("an answer"));
        (retry, answer)
    };
    let bare = reply(
        " done_check_text=DEFEND%20IT land_text=LAND%20IT land_nudge_text=NUDGE%20IT \
         land_answer_text=ANSWER%20IT",
    );
    assert_eq!(
        typed_at(&bare),
        (retry_words.clone(), act_words("Pick one.", &[NEEDS_HINT]))
    );
    // NEGATIVE CONTROL: an aterm that sends no act texts.
    let older = reply("");
    assert_eq!(
        typed_at(&older),
        (
            format!("{retry_words} (standing rules: commit often)"),
            format!(
                "{} (standing rules: commit often)",
                act_words("Pick one.", &[NEEDS_HINT])
            )
        )
    );
    // NEGATIVE CONTROL: act texts composed with rules.
    let ruled = reply(
        " done_check_text=DEFEND%20IT%20(standing%20rules:%20be%20brief) \
         land_text=LAND%20IT%20(standing%20rules:%20be%20brief) \
         land_nudge_text=NUDGE%20IT%20(standing%20rules:%20be%20brief) \
         land_answer_text=ANSWER%20IT%20(standing%20rules:%20be%20brief)",
    );
    assert_eq!(
        typed_at(&ruled).0,
        format!("{retry_words} (standing rules: be brief)")
    );
}

/// CRUISE'S DONE CHECK ASKS aterm'S SIX KINDS OF WORK IN TURN (aterm's `done_check_words(n)`,
/// `TurnEndState::checks`): where the `harness policy` reply sends `done_check_angles` beside its
/// first angle's act, each done claim typed gets the next angle, the seventh the first again,
/// under the standing rules; the count wraps on in order (aterm's `next_checks`); a person's
/// turn, an Off and a run out of Cruise start them again; a count a record carried
/// (`seed_checks`) resumes at the angle after it.
/// NEGATIVE CONTROLS: an older aterm's reply, with no angles, types its act whole at every claim,
/// and so does a reply whose act holds none of the angles it sends.
#[test]
fn the_done_check_asks_aterms_six_angles_in_turn() {
    let t = Timing::default();
    let start = Instant::now();
    let angles = [
        "A1.",
        "A2 two.",
        "A3 three.",
        "A4 four.",
        "A5 five.",
        "A6 six.",
    ];
    let rules = " (standing rules: be brief)";
    let pct = |text: &str| text.replace(' ', "%20").replace('\n', "%0A");
    let policy_of = |done: &str, sent: Option<&str>| {
        let angles = sent.map_or_else(String::new, |a| format!(" done_check_angles={}", pct(a)));
        let line = format!(
            "OK gen=1 effective=on why=enabled continue=true done_check_text={} \
             land_text={} land_nudge_text={} land_answer_text={}{angles}",
            pct(&format!("{done}{rules}")),
            pct(&format!("LAND IT{rules}")),
            pct(&format!("NUDGE IT{rules}")),
            pct(&format!("ANSWER IT{rules}")),
        );
        match codex_alab_aterm::policy::PolicyAnswer::parse(&line) {
            codex_alab_aterm::policy::PolicyAnswer::Supported(reply) => Policy {
                texts: Texts::from_harness(&reply),
                ..policy()
            },
            other => panic!("a supported reply: {other:?}"),
        }
    };
    let check = |angle: &str| format!("CHECK {angle} TAIL.");
    let p = policy_of(&check(angles[0]), Some(&angles.join("\n")));
    let mut day = 0u64;
    let mut claim = |s: &mut ThreadState, p: &Policy| {
        // Each claim says done, so the short streak grows and its act backs off from the turn's
        // end; read 100 days on, past the longest back-off (2 min doubled 16 times, 91 days),
        // every act is due.
        day += 200;
        let at = start + Duration::from_secs(86_400 * day);
        // A worker that works between its claims (a tool run each) meets every angle; the same
        // claim with no work would be ruling F's stop.
        s.observe_turn_end(
            tooled(ours(at, "All done, nothing left to do.", 5 * MIN)),
            &t,
        );
        let read = at + Duration::from_secs(86_400 * 100);
        let d = run(s, p, &t, read);
        assert!(d.not_before.is_none_or(|due| due <= read), "{d:?}");
        match typed(&d) {
            Some((words::RULE_DONE_CHECK, text)) => text.to_string(),
            other => panic!("a done check: {other:?}"),
        }
    };
    let want = |n: usize| format!("{}{rules}", check(angles[n % angles.len()]));
    let mut s = ThreadState::default();
    s.observe_person_turn();
    for n in 0..8 {
        assert_eq!(claim(&mut s, &p), want(n), "claim {n}");
    }
    s.observe_person_turn();
    assert_eq!(
        claim(&mut s, &p),
        want(0),
        "a person's turn starts them again"
    );
    assert_eq!(claim(&mut s, &p), want(1));
    s.park_off();
    assert_eq!(claim(&mut s, &p), want(0), "an Off starts them again");
    s.observe_mode(Mode::Land);
    assert_eq!(
        claim(&mut s, &p),
        want(0),
        "a run out of Cruise starts them again"
    );
    s.checks = 251;
    assert_eq!(claim(&mut s, &p), want(251));
    assert_eq!(
        s.checks, 6,
        "the count wraps to one whole turn, never to 0 or past 251"
    );
    assert_eq!(claim(&mut s, &p), want(252), "the count wraps on in order");
    let mut relaunched = ThreadState::default();
    relaunched.observe_person_turn();
    relaunched.seed_checks(4);
    assert_eq!(
        claim(&mut relaunched, &p),
        want(4),
        "the record's count resumes"
    );
    for seeded in [252u8, 253, 255] {
        let mut s = ThreadState::default();
        s.observe_person_turn();
        s.seed_checks(seeded);
        assert_eq!(claim(&mut s, &p), want(usize::from(seeded)), "{seeded}");
        assert_eq!(claim(&mut s, &p), want(usize::from(seeded) + 1), "{seeded}");
    }

    // THE RELAUNCH'S CARRY-ON over a resumed done claim asks again the check the crash cut (the
    // count's last angle), counting none, so the next claim asks the angle after it.
    // NEGATIVE CONTROL: a count of 0 asks the first, as aterm's relaunch types `done_check_text`.
    for (seeded, again) in [(2u8, 1usize), (0, 0)] {
        let mut s = ThreadState {
            task: true,
            carry_on: true,
            resumed_reply: Some("All done, nothing left to do.".to_string()),
            ..ThreadState::default()
        };
        s.seed_checks(seeded);
        let d = run(&mut s, &p, &t, start);
        assert_eq!(
            typed(&d),
            Some((words::RULE_HOST_TURN, want(again).as_str())),
            "carried on at {seeded}"
        );
        assert_eq!(s.checks, seeded, "the carry-on counts no check");
        assert_eq!(claim(&mut s, &p), want(usize::from(seeded)));
    }

    // THE ANGLE'S OWN ROOM: rules too long for the first angle's act, whose copy aterm cut to
    // that act's room, fit under a shorter angle from the least-cut copy (`land_answer_text`'s),
    // as aterm's `done_check_act(n)` fits them. NEGATIVE CONTROL: the first act's own copy is
    // cut shorter than the rules the shorter angle types.
    let long_rules = "keep every change reversible and small; ".repeat(14);
    let long_rules = long_rules.trim_end();
    let first = format!("A1 {}.", "first ".repeat(40).trim_end());
    let cut_to = |said: &str| words::with_rules(said, Some(long_rules));
    let long_angles = [first.as_str(), "A2.", "A3.", "A4.", "A5.", "A6."];
    let line = format!(
        "OK gen=1 effective=on why=enabled continue=true done_check_text={} \
         land_text={} land_nudge_text={} land_answer_text={} done_check_angles={}",
        pct(&cut_to(&check(&first))),
        pct(&cut_to("LAND IT")),
        pct(&cut_to("NUDGE IT")),
        pct(&cut_to("ANSWER IT")),
        pct(&long_angles.join("\n")),
    );
    let roomy = match codex_alab_aterm::policy::PolicyAnswer::parse(&line) {
        codex_alab_aterm::policy::PolicyAnswer::Supported(reply) => Policy {
            texts: Texts::from_harness(&reply),
            ..policy()
        },
        other => panic!("a supported reply: {other:?}"),
    };
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.checks = 1;
    let second = claim(&mut s, &roomy);
    assert_eq!(
        second,
        cut_to(&check("A2.")),
        "the rules fitted to the angle's own room"
    );
    let first_copy = words::split_rules(&roomy.texts.done_check)
        .1
        .unwrap_or_default();
    let second_copy = words::split_rules(&second).1.unwrap_or_default();
    assert!(
        second_copy.len() > first_copy.len(),
        "CONTROL: the first act's copy is cut shorter: {first_copy:?} vs {second_copy:?}"
    );

    // NEGATIVE CONTROLS.
    for older in [
        policy_of(&check(angles[0]), None),
        policy_of("DEFEND IT", Some(&angles.join("\n"))),
    ] {
        let whole = older.texts.done_check.clone();
        assert!(older.texts.done_angles.is_empty(), "{:?}", older.texts);
        let mut s = ThreadState::default();
        s.observe_person_turn();
        for n in 0..3 {
            assert_eq!(claim(&mut s, &older), whole, "claim {n}");
        }
    }
}

/// A SERVER'S WALL CLIMBS THE RETRY LADDER FROM THE APPEARANCE THAT ANSWERED EACH TRY
/// (aterm's `track_wall`: `if retried { t.since = now }`, the rung counted from `since`):
/// the try at 1 min led to a turn that met the wall again at 2 min, so the next try goes 5 min
/// after that answer, never 5 min after the try (which counted the try's own turn, core's
/// retries included, as time waited).
#[test]
fn server_errors_climb_the_retry_ladder_from_each_tries_answer() {
    let t = Timing::default();
    let start = Instant::now();
    let p = policy();
    let mut s = ThreadState::default();
    s.observe_person_turn();
    let wall = || Wall::Server {
        what: "an API error (500)".to_string(),
        cut_off: false,
        retry_at: None,
    };
    s.observe_turn_end(walled(start, wall()), &t);
    let d = decide(&s, &p, &t, start, NO_JITTER);
    assert_eq!(d.not_before, Some(start + MIN));
    assert_eq!(
        typed(&d).map(|(r, _)| r),
        Some(words::RULE_API_RETRY),
        "{d:?}"
    );
    assert!(d.shown_wait.is_some());
    s.acted(&d.act, start + MIN, &t);
    s.observe_turn_end(walled(start + 2 * MIN, wall()), &t);
    let d = decide(&s, &p, &t, start + 2 * MIN, NO_JITTER);
    assert_eq!(d.not_before, Some(start + 2 * MIN + 5 * MIN));
    // The text names the wall in aterm's words, never the error's own text.
    assert!(
        typed(&d)
            .unwrap()
            .1
            .starts_with("Codex reported an API error (500).")
    );
    // Negative control: retry_api_errors off escalates instead.
    let off = Policy {
        retry_api_errors: false,
        ..policy()
    };
    assert!(matches!(
        decide(&s, &off, &t, start, NO_JITTER).act,
        Act::Attention { key: "api", .. }
    ));
}

#[test]
fn provider_retry_deadlines_survive_redeciding_without_overriding_policy() {
    let t = Timing::default();
    let start = Instant::now();
    let mut s = ThreadState::default();
    s.observe_person_turn();
    let provider_at = start + 10 * MIN;
    s.observe_turn_end(
        walled(
            start,
            Wall::Server {
                what: "an API error (429)".to_string(),
                cut_off: false,
                retry_at: Some(provider_at),
            },
        ),
        &t,
    );
    // Re-reading policy and revisiting the point keep the captured instant: they
    // must neither resume early nor restart the server's ten-minute wait.
    for elapsed in [Duration::ZERO, 3 * MIN, 10 * MIN, 11 * MIN] {
        let d = decide(&s, &policy(), &t, start + elapsed, NO_JITTER);
        assert_eq!(d.not_before, Some(provider_at));
        assert_eq!(typed(&d).map(|(rule, _)| rule), Some(words::RULE_API_RETRY));
    }
    let off = Policy {
        retry_api_errors: false,
        ..policy()
    };
    let d = decide(&s, &off, &t, start, NO_JITTER);
    assert!(matches!(d.act, Act::Attention { key: "api", .. }));
    assert_eq!(d.not_before, None);
}

#[test]
fn short_or_expired_provider_advice_keeps_the_local_retry_ladder() {
    let t = Timing::default();
    let start = Instant::now();
    for retry_at in [None, Some(start), Some(start + MIN / 2)] {
        let mut s = ThreadState::default();
        s.observe_person_turn();
        s.observe_turn_end(
            walled(
                start,
                Wall::Server {
                    what: "an API error (503)".to_string(),
                    cut_off: false,
                    retry_at,
                },
            ),
            &t,
        );
        let d = decide(&s, &policy(), &t, start, NO_JITTER);
        assert_eq!(d.not_before, Some(start + MIN));
        assert_eq!(typed(&d).map(|(rule, _)| rule), Some(words::RULE_API_RETRY));
    }
}

/// A REPLY CUT OFF IS CONTINUED AT ONCE, THEN ON THE NETWORK'S SHORT LADDER (aterm's
/// `(ApiCause::CutOff, _) => attempts.checked_sub(1).map_or(since, |n| rung(&t.net_backoff,
/// n))`): the first try goes at the appearance, and each try after it waits 1, 2, then 5
/// minutes for ever from the appearance that answered the last try, where codex-alab climbed
/// the server's 1/5/15/30/60 min ladder from its first rung. A server's own failure keeps
/// that ladder. NEGATIVE CONTROL: the server's 500 at the same appearances waits 1, 5, 15
/// and 30 minutes. Each cut-off answer ran a tool (work), so ruling F's wall hold, which holds
/// a cut-off answered three times the same with no work (`walls_hold_tests.rs`), leaves the
/// ladder to climb.
#[test]
fn a_cut_off_reply_is_continued_at_once_then_on_the_network_ladder() {
    let t = Timing::default();
    let start = Instant::now();
    let p = policy();
    let run = |cut_off: bool| {
        let wall = Wall::Server {
            what: if cut_off {
                "an API error (the reply was cut off)".to_string()
            } else {
                "an API error (500)".to_string()
            },
            cut_off,
            retry_at: None,
        };
        let mut s = ThreadState::default();
        s.observe_person_turn();
        let mut met = start;
        let mut waits = Vec::new();
        for _ in 0..5 {
            s.observe_turn_end(
                TurnEnd {
                    worked: MIN,
                    ran_tools: true,
                    ..walled(met, wall.clone())
                },
                &t,
            );
            let d = decide(&s, &p, &t, met, NO_JITTER);
            let at = d.not_before.map_or(met, |at| at.max(met));
            waits.push((at - met).as_secs() / 60);
            assert_eq!(
                typed(&d).map(|(rule, _)| rule),
                Some(if cut_off {
                    words::RULE_API_CUTOFF
                } else {
                    words::RULE_API_RETRY
                }),
                "{d:?}"
            );
            s.acted(&d.act, at, &t);
            met = at + MIN;
        }
        waits
    };
    assert_eq!(run(true), [0, 1, 2, 5, 5], "a reply cut off");
    // NEGATIVE CONTROL.
    assert_eq!(run(false), [1, 5, 15, 30, 60], "a server's own failure");
}

/// AN ABORT NO PERSON'S ESC MADE (a replacement, an ended review, a goal's budget, an
/// interrupted point restored from history: no [`ThreadState::observe_esc`]) keeps the task
/// without continuing, retrying or finishing it, a day later too; a later person's turn supplies
/// fresh authority. The Esc's own point is held for the grace and then carried on
/// ([`a_persons_esc_holds_for_the_grace_then_cruise_and_land_carry_on`]).
#[test]
fn an_aborted_turn_keeps_the_task_without_continuing_retrying_or_finishing_it() {
    let t = Timing::default();
    let now = Instant::now();
    for (text, wall) in [
        ("The next step is ready.", None),
        ("ATERM-TASK-DONE", None),
        (
            "",
            Some(Wall::Server {
                what: "an API error (503)".to_string(),
                cut_off: false,
                retry_at: None,
            }),
        ),
        ("", Some(Wall::Context { auto_compact: true })),
    ] {
        let mut s = ThreadState::default();
        s.observe_turn_end(
            TurnEnd {
                aborted: true,
                ran_tools: false,
                restored: false,
                wall,
                ..end(now, text, 5 * MIN)
            },
            &t,
        );
        let d = decide(&s, &policy(), &t, now + 24 * 60 * MIN, NO_JITTER);
        assert_eq!(d.act, Act::Nothing, "{text}: {d:?}");
        assert_eq!(d.not_before, None);
        assert!(s.task, "the interrupted task remains available");
        assert!(!s.landed, "an aborted turn lands nothing");

        // A later person's turn supplies fresh authority for ordinary continuation.
        s.observe_turn_end(end(now, "The task is moving again.", 5 * MIN), &t);
        assert_eq!(
            typed(&decide(&s, &policy(), &t, now, NO_JITTER)),
            Some((words::RULE_CONTINUE, "keep going"))
        );
    }
}

/// An aborted LAND ACT'S YIELD lands nothing: an abort no person's Esc made ended the turn's
/// authority, so its clean defense over a clean repository is neither the landing nor a nudge,
/// a day later too, and the next person's turn opens a new episode with the instruction.
#[test]
fn an_aborted_land_acts_yield_lands_nothing() {
    let t = Timing::default();
    let now = Instant::now();
    let p = land();
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Ship the parser.", 5 * MIN), &t);
    assert_eq!(
        typed(&run(&mut s, &p, &t, now)),
        Some((words::RULE_LAND, words::LAND))
    );
    assert!(s.landing);
    let at = now + 20 * MIN;
    s.observe_turn_end(
        TurnEnd {
            aborted: true,
            ran_tools: false,
            restored: false,
            ..ours(at, CLEAN, 10 * MIN)
        },
        &t,
    );
    let d = run(&mut s, &p, &t, at);
    assert_eq!(d.act, Act::Nothing, "{d:?}");
    assert!(!s.landed, "an aborted yield lands nothing");
    assert_eq!(
        decide(&s, &p, &t, at + 24 * 60 * MIN, NO_JITTER).act,
        Act::Nothing
    );
    let later = at + 30 * MIN;
    s.observe_turn_end(end(later, "Ship the parser.", 5 * MIN), &t);
    assert_eq!(
        typed(&run(&mut s, &p, &t, later)),
        Some((words::RULE_LAND, words::LAND))
    );
}

/// A PERSON'S ESC HOLDS THE POINT FOR `human_grace`, THEN IT IS A TURN END (owner 2026-10-27:
/// "you need to keep working even if the user interrupted a long running job"; aterm's
/// `person_hold`, FOREVER CRUISE and FOREVER LAND): within the grace nothing is typed and the
/// point is decided again where it ends ([`Decision::look_again`]); past it Cruise carries on
/// and Land types its act, the interrupted words read as none (a done claim, a question and a
/// clean defense cut by the Esc claim, ask and land nothing). codex-alab parked such a point
/// until a person's turn. NEGATIVE CONTROLS: an abort no Esc made stays a quiet nothing a day
/// later; an Esc told over a point that was not aborted marks nothing; the next turn end clears
/// the mark.
#[test]
fn a_persons_esc_holds_for_the_grace_then_cruise_and_land_carry_on() {
    let t = Timing::default();
    let now = Instant::now();
    let grace = Duration::from_secs(15);
    let cruise = Policy {
        human_grace: grace,
        ..policy()
    };
    let landing = Policy {
        human_grace: grace,
        ..land()
    };
    let aborted = |end: TurnEnd| TurnEnd {
        aborted: true,
        ..end
    };
    for text in [
        "The next step is ready.",
        "The task is complete.\nATERM-TASK-DONE",
        "Should I delete the old release branch?",
    ] {
        let mut s = ThreadState::default();
        s.observe_turn_end(aborted(end(now, text, 5 * MIN)), &t);
        s.observe_esc(now);
        assert_eq!(s.esc_at, Some(now));
        let held = decide(&s, &cruise, &t, now + Duration::from_secs(5), NO_JITTER);
        assert_eq!(
            (&held.act, held.look_again, held.why.as_str()),
            (&Act::Nothing, Some(now + grace), crate::decide::ESC_HOLD),
            "{text}"
        );
        assert_eq!(
            typed(&decide(&s, &cruise, &t, now + grace, NO_JITTER)),
            Some((words::RULE_CONTINUE, "keep going")),
            "{text}"
        );
        // Under Land the instruction, past the grace.
        let mut s = ThreadState::default();
        s.observe_turn_end(aborted(end(now, text, 5 * MIN)), &t);
        s.observe_esc(now);
        assert_eq!(
            probed(&mut s, &landing, &t, now + Duration::from_secs(5), None).act,
            Act::Nothing
        );
        assert_eq!(
            typed(&probed(&mut s, &landing, &t, now + grace, None)),
            Some((words::RULE_LAND, words::LAND)),
            "{text}"
        );
    }
    // A land act's clean defense the Esc cut lands nothing: Land's nudge past the grace.
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Ship the parser.", 5 * MIN), &t);
    assert_eq!(
        typed(&run(&mut s, &landing, &t, now)),
        Some((words::RULE_LAND, words::LAND))
    );
    let at = now + 20 * MIN;
    s.observe_turn_end(aborted(ours(at, CLEAN, 10 * MIN)), &t);
    s.observe_esc(at);
    assert_eq!(run(&mut s, &landing, &t, at).act, Act::Nothing);
    let d = run(&mut s, &landing, &t, at + grace);
    assert!(
        typed(&d).is_some_and(|(rule, _)| rule == words::RULE_LAND_NUDGE),
        "{d:?}"
    );
    assert!(!s.landed, "a stopped yield lands nothing");
    // NEGATIVE CONTROLS: no Esc told; an Esc over a point not aborted; the next end clears it.
    let mut s = ThreadState::default();
    s.observe_turn_end(aborted(end(now, "The next step is ready.", 5 * MIN)), &t);
    assert_eq!(
        decide(&s, &cruise, &t, now + 24 * 60 * MIN, NO_JITTER).act,
        Act::Nothing
    );
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "The next step is ready.", 5 * MIN), &t);
    s.observe_esc(now);
    assert_eq!(s.esc_at, None);
    s.observe_turn_end(aborted(end(now, "The next step is ready.", 5 * MIN)), &t);
    s.observe_esc(now);
    s.observe_turn_end(
        aborted(end(now + MIN, "The next step is ready.", 5 * MIN)),
        &t,
    );
    assert_eq!(s.esc_at, None);
    assert_eq!(
        decide(&s, &cruise, &t, now + 24 * 60 * MIN, NO_JITTER).act,
        Act::Nothing
    );
}

#[test]
fn a_retry_that_led_to_real_work_starts_the_ladder_over() {
    let t = Timing::default();
    let start = Instant::now();
    let p = policy();
    let mut s = ThreadState::default();
    s.observe_person_turn();
    let wall = || Wall::Server {
        what: "an API error (500)".to_string(),
        cut_off: false,
        retry_at: None,
    };
    s.observe_turn_end(walled(start, wall()), &t);
    let d = decide(&s, &p, &t, start, NO_JITTER);
    s.acted(&d.act, start + MIN, &t);
    // The retry worked for ten minutes before the same wall stood again: a new episode.
    let later = start + 12 * MIN;
    s.observe_turn_end(
        TurnEnd {
            worked: 10 * MIN,
            ..walled(later, wall())
        },
        &t,
    );
    let d = decide(&s, &p, &t, later, NO_JITTER);
    assert_eq!(
        d.not_before,
        Some(later + MIN),
        "the ladder's first step again"
    );
    // Negative control: a retry that did less than the progress climbs on, from its answer.
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(walled(start, wall()), &t);
    let d = decide(&s, &p, &t, start, NO_JITTER);
    s.acted(&d.act, start + MIN, &t);
    s.observe_turn_end(
        TurnEnd {
            worked: 3 * MIN,
            ..walled(start + 5 * MIN, wall())
        },
        &t,
    );
    let d = decide(&s, &p, &t, start + 5 * MIN, NO_JITTER);
    assert_eq!(d.not_before, Some(start + 5 * MIN + 5 * MIN));
}

/// EVERY WALL'S LADDER COUNTS FROM THE APPEARANCE THAT ANSWERED ITS TRY (aterm's `since`,
/// moved only at a retry of its own: `if retried { t.since = now }`), the server's, a
/// cut-off's, an unreached API's, a usage window's that names no reset and a quota's alike:
/// each try's turn met the wall again 3 min after the try, and the next rung is counted from
/// there. A person's or a goal's turn that meets the same wall moves neither the appearance
/// nor the rung, however long it worked (aterm's `anew` asks for the retry). NEGATIVE
/// CONTROLS: counted from the try, each rung came 3 min sooner; a person's long turn read as
/// real work the try led to started the ladder over.
#[test]
fn every_walls_ladder_counts_from_the_appearance_that_answered_its_try() {
    let t = Timing::default();
    let start = Instant::now();
    let p = policy();
    let server = |cut_off| Wall::Server {
        what: "an API error (500)".to_string(),
        cut_off,
        retry_at: None,
    };
    let cases = [
        ("server", server(false), MIN, 5 * MIN),
        // aterm's `(ApiCause::CutOff, _)`: at once, then the network's short ladder.
        ("cut-off", server(true), Duration::ZERO, MIN),
        (
            "network",
            Wall::Network {
                what: "an API error (the API could not be reached)".to_string(),
            },
            MIN,
            2 * MIN,
        ),
        (
            "usage",
            Wall::Usage {
                reset_at: None,
                reset_label: None,
            },
            30 * MIN,
            10 * MIN,
        ),
        ("entitlement", Wall::Entitlement, 30 * MIN, 120 * MIN),
    ];
    for (name, wall, first, second) in cases {
        let mut s = ThreadState::default();
        s.observe_person_turn();
        s.observe_turn_end(walled(start, wall.clone()), &t);
        let d = decide(&s, &p, &t, start, NO_JITTER);
        assert_eq!(d.not_before, Some(start + first), "{name}: {d:?}");
        let tried = start + first;
        s.acted(&d.act, tried, &t);
        let answer = tried + 3 * MIN;
        s.observe_turn_end(
            TurnEnd {
                worked: 3 * MIN,
                ..walled(answer, wall.clone())
            },
            &t,
        );
        let want = answer + second;
        assert_eq!(
            decide(&s, &p, &t, answer, NO_JITTER).not_before,
            Some(want),
            "{name}"
        );
        // NEGATIVE CONTROL: counted from the try, the rung came 3 min sooner.
        assert_ne!(tried + second, want, "{name}");
        for (by_person, who) in [(true, "a person's"), (false, "a goal's")] {
            let mut s = s.clone();
            let met = answer + MIN;
            s.observe_turn_end(
                TurnEnd {
                    by_autopilot: false,
                    by_person,
                    worked: 20 * MIN,
                    ..walled(met, wall.clone())
                },
                &t,
            );
            assert_eq!(
                s.wall.as_ref().map(|w| w.attempts),
                Some(1),
                "{name}: {who}"
            );
            let d = decide(&s, &p, &t, met, NO_JITTER);
            assert_eq!(d.not_before, Some(want), "{name}: {who} turn: {d:?}");
        }
    }
}

/// A TRY ANSWERED SHORT WITH NO WALL WAS TAKEN, AND ITS LADDER GOES (aterm's `retry_taken`: no
/// wall answered the retry, and the track is dropped however little the turn worked): a wall
/// that stands again later starts at the ladder's first rung, from its own appearance, where
/// the kept track sent that wall's try on the second rung counted from the old try.
/// NEGATIVE CONTROL: a try the wall answered climbs on.
#[test]
fn a_try_answered_short_with_no_wall_ends_the_ladder() {
    let t = Timing::default();
    let start = Instant::now();
    let p = policy();
    let wall = || Wall::Server {
        what: "an API error (500)".to_string(),
        cut_off: false,
        retry_at: None,
    };
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(walled(start, wall()), &t);
    let d = decide(&s, &p, &t, start, NO_JITTER);
    s.acted(&d.act, start + MIN, &t);
    // The try's turn ends with no wall, short.
    let answered = start + MIN + Duration::from_secs(20);
    s.observe_turn_end(
        ours(answered, "Retrying the build.", Duration::from_secs(20)),
        &t,
    );
    assert_eq!(s.wall, None, "the try was taken");
    let d = run(&mut s, &p, &t, answered);
    assert_eq!(typed(&d).map(|(rule, _)| rule), Some(words::RULE_CONTINUE));
    let again = answered + 2 * MIN;
    s.observe_turn_end(walled(again, wall()), &t);
    assert_eq!(
        decide(&s, &p, &t, again, NO_JITTER).not_before,
        Some(again + MIN),
        "the first rung, from the new appearance"
    );
    // NEGATIVE CONTROL: the wall answered the try: the second rung.
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(walled(start, wall()), &t);
    let d = decide(&s, &p, &t, start, NO_JITTER);
    s.acted(&d.act, start + MIN, &t);
    s.observe_turn_end(walled(answered, wall()), &t);
    assert_eq!(
        decide(&s, &p, &t, answered, NO_JITTER).not_before,
        Some(answered + 5 * MIN)
    );
}

/// A USAGE WALL NAMING NO RESET, FIRST MET, WAITS THE LONGEST BACK-OFF (aterm's
/// `resume_at_reset`: `(None, None) => back.last()`, 30 min): nothing says when the window
/// opens, and a resume at the first rung's 10 min mostly meets the limit again. NEGATIVE
/// CONTROL: after a resume the limit answered, the first rung (10 min) from that answer; a
/// reset named waits for it.
#[test]
fn a_usage_wall_naming_no_reset_first_waits_the_longest_back_off() {
    let t = Timing::default();
    let start = Instant::now();
    let p = policy();
    let unnamed = || Wall::Usage {
        reset_at: None,
        reset_label: None,
    };
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(walled(start, unnamed()), &t);
    let d = decide(&s, &p, &t, start, NO_JITTER);
    assert!(matches!(d.act, Act::ResumeLimit { .. }), "{d:?}");
    assert_eq!(d.not_before, Some(start + 30 * MIN));
    assert_eq!(d.shown_wait.as_deref(), Some("waiting out the usage limit"));
    let tried = start + 30 * MIN;
    s.acted(&d.act, tried, &t);
    let met = tried + Duration::from_secs(5);
    s.observe_turn_end(walled(met, unnamed()), &t);
    assert_eq!(
        decide(&s, &p, &t, met, NO_JITTER).not_before,
        Some(met + 10 * MIN)
    );
    // NEGATIVE CONTROL: a reset named is waited for, however soon.
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(
        walled(
            start,
            Wall::Usage {
                reset_at: Some(start + 2 * MIN),
                reset_label: None,
            },
        ),
        &t,
    );
    assert_eq!(
        decide(&s, &p, &t, start, NO_JITTER).not_before,
        Some(start + 2 * MIN + t.reset_grace)
    );
}

/// A USAGE RESUME'S SHORT YIELD IS NO SHORT TURN OF A CONTINUATION (aterm's `is_continue_rule`
/// leaves `RULE_LIMIT_RESUME` out, so the streak lengthens only at a continuation's yield): the
/// continuation at the point the resume leaves goes at once, its request moments after the
/// resume's. This is WF5 of the codex-alab proof (2026-10-20: two `usage:continue` model
/// requests 19-30 ms apart after the usage wait): two acts, the resume and the continuation,
/// each typed once, never one act typed twice. NEGATIVE CONTROL: the continuation's own short
/// yield backs the next off 2 min.
#[test]
fn a_usage_resumes_short_yield_is_continued_at_once() {
    let t = Timing::default();
    let start = Instant::now();
    let p = policy();
    let mut s = ThreadState::default();
    s.observe_person_turn();
    let reset = start + 5 * MIN;
    s.observe_turn_end(
        walled(
            start,
            Wall::Usage {
                reset_at: Some(reset),
                reset_label: None,
            },
        ),
        &t,
    );
    let d = decide(&s, &p, &t, start, NO_JITTER);
    assert!(matches!(d.act, Act::ResumeLimit { .. }), "{d:?}");
    let resumed = d.not_before.unwrap_or(start);
    s.acted(&d.act, resumed, &t);
    let ms = Duration::from_millis(15);
    let yielded = resumed + ms;
    s.observe_turn_end(ours(yielded, "Resumed after the reset.", ms), &t);
    assert_eq!(s.short_streak, 0);
    assert_eq!(s.wall, None, "the resume was taken");
    let d = probed(&mut s, &p, &t, yielded, None);
    assert_eq!(typed(&d).map(|(rule, _)| rule), Some(words::RULE_CONTINUE));
    assert_eq!(d.not_before, None, "at once");
    s.acted(&d.act, yielded, &t);
    // NEGATIVE CONTROL: the continuation's own short yield backs the next off.
    let next = yielded + ms;
    // In its own words: the resume's reply said again would be ruling F's stop.
    s.observe_turn_end(ours(next, "Carrying on with the parser.", ms), &t);
    assert_eq!(s.short_streak, 1);
    assert_eq!(
        probed(&mut s, &p, &t, next, None).not_before,
        Some(next + 2 * MIN)
    );
}

/// A WALL'S TRY OWED AT REST IS TYPED BY ATERM'S RELAUNCH AT ITS RUNG (FOREVER CRUISE: a
/// wall's wait is rest, and a crash there left the relaunch typing nothing until a person's
/// turn, where aterm's loop types the wall's next try from the ladder its books carry,
/// `seed_retry`): the record's act reads back as this image's (`owed_of`), is typed at its time
/// with its words, and its yield climbs the carried ladder from the appearance it meets. A
/// usage window's resume goes as the resume ([`Act::ResumeLimit`]) in the words of the mode
/// read when it goes. The wall's own switch off types nothing and RAISES THE WALL'S ATTENTION,
/// as the wall's own point does (`decide_wall`): a try carried past a relaunch is the same
/// wall, and a silent nothing there left the person no word of it. NEGATIVE CONTROLS:
/// `continue` off holds no wall's try, and a paced act owed with `continue` off is a quiet
/// nothing; a paced act owed under another mode is none.
#[test]
fn a_walls_try_owed_at_rest_is_typed_by_the_relaunch_at_its_rung() {
    use codex_alab_core::session::OwedAct;
    let t = Timing::default();
    let now = Instant::now();
    let p = policy();
    let server = || Wall::Server {
        what: "an API error (500)".to_string(),
        cut_off: false,
        retry_at: None,
    };
    let mut old = ThreadState::default();
    old.observe_person_turn();
    old.observe_turn_end(walled(now, server()), &t);
    let d = decide(&old, &p, &t, now, NO_JITTER);
    old.acted(&d.act, now + MIN, &t);
    let answer = now + 3 * MIN;
    old.observe_turn_end(walled(answer, server()), &t);
    let d = decide(&old, &p, &t, answer, NO_JITTER);
    let due = answer + 5 * MIN;
    assert_eq!(d.not_before, Some(due));
    let (rule, text) = typed(&d).unwrap_or_else(|| panic!("a retry: {d:?}"));
    assert_eq!(rule, words::RULE_API_RETRY);
    // The record's act, as the runtime writes it, reads back as this image's.
    let record = OwedAct {
        renewal: None,
        thread: "019a-id".to_string(),
        native_origin: None,
        rule: Some(rule.to_string()),
        text: Some(text.to_string()),
        mode: Some("cruise".to_string()),
        not_before_ms: codex_alab_core::session::now_ms() + 300_000,
        repo_owed: None,
        resume_goal: false,
        landing: false,
        landed: false,
    };
    let read = crate::extension::Autopilot::owed_of(&record)
        .unwrap_or_else(|| panic!("a wall's try is carried"));
    assert_eq!(
        (read.rule, read.text.as_str(), read.resume_goal),
        (rule, text, false)
    );
    let owed = Owed {
        native_origin: None,
        rule,
        text: text.to_string(),
        mode: Mode::Cruise,
        not_before: due,
        repo_owed: None,
        resume_goal: false,
        landing: false,
    };
    let relaunched = |owed: Owed, mode: Mode| {
        let mut s = ThreadState::default();
        s.observe_resume(true, Some("Working."), false);
        s.seed_pace(old.short_streak, old.wall.clone());
        s.observe_owed(Some(owed), false, answer + MIN);
        s.observe_mode(mode);
        s
    };
    let mut s = relaunched(owed.clone(), Mode::Cruise);
    let d = decide(&s, &p, &t, answer + MIN, NO_JITTER);
    assert_eq!(typed(&d), Some((rule, text)));
    assert_eq!(d.not_before, Some(due));
    s.acted(&d.act, due, &t);
    let met = due + 2 * MIN;
    s.observe_turn_end(walled(met, server()), &t);
    assert_eq!(
        decide(&s, &p, &t, met, NO_JITTER).not_before,
        Some(met + 15 * MIN),
        "the third rung, from the appearance that answered the relaunch's try"
    );
    // The wall's own switch off types nothing and raises the wall's attention; `continue`
    // off holds no wall's try.
    let off = Policy {
        retry_api_errors: false,
        ..policy()
    };
    let raised = |d: Decision| match d.act {
        Act::Attention { key, text } => Some((key, text)),
        _ => None,
    };
    let said = raised(decide(
        &relaunched(owed.clone(), Mode::Cruise),
        &off,
        &t,
        due,
        NO_JITTER,
    ));
    assert!(
        said.as_ref()
            .is_some_and(|(key, text)| *key == "api" && text.ends_with("retry_api_errors is off")),
        "{said:?}"
    );
    let paced_off = Owed {
        rule: words::RULE_CONTINUE,
        text: "keep going".to_string(),
        ..owed.clone()
    };
    let quiet_off = Policy {
        continue_on: false,
        ..policy()
    };
    assert_eq!(
        decide(
            &relaunched(paced_off, Mode::Cruise),
            &quiet_off,
            &t,
            due,
            NO_JITTER
        )
        .act,
        Act::Nothing,
        "NEGATIVE CONTROL: a paced act with continue off is a quiet nothing"
    );
    let quiet = Policy {
        continue_on: false,
        ..policy()
    };
    assert_eq!(
        typed(&decide(
            &relaunched(owed.clone(), Mode::Cruise),
            &quiet,
            &t,
            due,
            NO_JITTER
        )),
        Some((rule, text))
    );
    // A wall's try owed under Cruise stays owed under Land; a paced act does not.
    assert_eq!(
        typed(&decide(
            &relaunched(owed.clone(), Mode::Land),
            &land(),
            &t,
            due,
            NO_JITTER
        )),
        Some((rule, text))
    );
    let paced = Owed {
        rule: words::RULE_CONTINUE,
        text: "keep going".to_string(),
        ..owed.clone()
    };
    assert_eq!(relaunched(paced, Mode::Land).owed, None);
    // THE USAGE WINDOW'S RESUME: carried as the resume, in the words of the mode read now.
    let unnamed = Wall::Usage {
        reset_at: None,
        reset_label: None,
    };
    let mut fresh = ThreadState::default();
    fresh.observe_person_turn();
    fresh.observe_mode(Mode::Land);
    fresh.observe_turn_end(walled(now, unnamed), &t);
    let landing = decide(&fresh, &land(), &t, now, NO_JITTER).act;
    assert!(
        matches!(&landing, Act::ResumeLimit { text } if text != "keep going"),
        "{landing:?}"
    );
    let resume = Owed {
        rule: words::RULE_LIMIT_RESUME,
        text: "keep going".to_string(),
        resume_goal: true,
        ..owed
    };
    let d = decide(
        &relaunched(resume.clone(), Mode::Cruise),
        &p,
        &t,
        due,
        NO_JITTER,
    );
    assert_eq!(
        d.act,
        Act::ResumeLimit {
            text: "keep going".to_string()
        }
    );
    let d = decide(
        &relaunched(resume.clone(), Mode::Land),
        &land(),
        &t,
        due,
        NO_JITTER,
    );
    assert_eq!(d.act, landing);
    let held = Policy {
        limit_wait: false,
        ..policy()
    };
    assert_eq!(
        raised(decide(
            &relaunched(resume.clone(), Mode::Cruise),
            &held,
            &t,
            due,
            NO_JITTER
        )),
        Some((
            "limit",
            "limit: the usage limit was reached; resume_limits or limit_wait is off".to_string()
        ))
    );
    // A quota wall's try (typed as a turn) answers to resume_limits alone, and says so.
    let quota = Owed {
        resume_goal: false,
        ..resume
    };
    assert_eq!(
        typed(&decide(
            &relaunched(quota.clone(), Mode::Cruise),
            &held,
            &t,
            due,
            NO_JITTER
        )),
        Some((words::RULE_LIMIT_RESUME, "keep going")),
        "limit_wait holds no quota wall's try"
    );
    let unlimited = Policy {
        resume_limits: false,
        ..policy()
    };
    assert_eq!(
        raised(decide(
            &relaunched(quota, Mode::Cruise),
            &unlimited,
            &t,
            due,
            NO_JITTER
        )),
        Some((
            "limit",
            "limit: this account's quota or plan stops the model; resume_limits is off".to_string()
        ))
    );
}

/// A WALL'S TRY CARRIED PAST A RELAUNCH KEEPS THE LAND EPISODE (parity-3 review, F4): under
/// Land a server wall cuts the turn of Land's instruction, and the retry that waits on its rung
/// delivers that act again (`landing`); the record carries that ([`Owed::landing`]), so the
/// relaunched image types the retry and its yield's clean defense lands, as the image that
/// decided it would have judged, where it typed Land's instruction again over a defense the
/// episode had asked for. NEGATIVE CONTROLS: a try carried with no landing (a record before
/// it, or a wall that cut no land act) starts the episode at the clean defense with the
/// instruction, and the decision's own memory says no landing where the cut act was none.
#[test]
fn a_carried_walls_try_of_a_land_act_lands_its_clean_defense() {
    let t = Timing::default();
    let now = Instant::now();
    let p = land();
    let server = || Wall::Server {
        what: "an API error (500)".to_string(),
        cut_off: false,
        retry_at: None,
    };
    let mut old = ThreadState::default();
    old.observe_person_turn();
    old.observe_mode(Mode::Land);
    old.observe_turn_end(end(now, "Stage 1 is in; stage 2 is next.", 5 * MIN), &t);
    let d = run(&mut old, &p, &t, now);
    assert_eq!(typed(&d).map(|(rule, _)| rule), Some(words::RULE_LAND));
    assert!(old.landing, "the instruction is a land act");
    let cut = now + 10 * MIN;
    old.observe_turn_end(walled(cut, server()), &t);
    let d = decide(&old, &p, &t, cut, NO_JITTER);
    let (rule, text) = typed(&d).unwrap_or_else(|| panic!("a retry: {d:?}"));
    assert_eq!(rule, words::RULE_API_RETRY);
    let due = d.not_before.unwrap_or(cut);
    let owed = crate::extension::owes_of(&d, Mode::Land, due, old.landing, None)
        .unwrap_or_else(|| panic!("the try is carried"));
    assert!(owed.landing, "it delivers the land act again");
    let relaunched = |owed: Owed| {
        let mut s = ThreadState::default();
        s.observe_resume(true, Some("Working."), false);
        s.observe_owed(Some(owed), false, cut + MIN);
        s.observe_mode(Mode::Land);
        s
    };
    let defended = |owed: Owed| {
        let mut s = relaunched(owed);
        let d = decide(&s, &p, &t, due, NO_JITTER);
        assert_eq!(typed(&d), Some((rule, text)), "the carried try");
        s.acted(&d.act, due, &t);
        let yielded = due + 5 * MIN;
        s.observe_turn_end(
            ours(
                yielded,
                "Nothing left: every item is merged and pushed to origin/main.",
                5 * MIN,
            ),
            &t,
        );
        probed(&mut s, &p, &t, yielded, None)
    };
    assert_eq!(defended(owed.clone()).act, Act::Landed);
    // NEGATIVE CONTROLS: carried with no landing, the clean defense starts the episode.
    let unmarked = Owed {
        landing: false,
        ..owed
    };
    assert_eq!(
        typed(&defended(unmarked)).map(|(rule, _)| rule),
        Some(words::RULE_LAND)
    );
    let mut other = ThreadState::default();
    other.observe_person_turn();
    other.observe_mode(Mode::Land);
    other.observe_turn_end(walled(now, server()), &t);
    let d = decide(&other, &p, &t, now, NO_JITTER);
    let at = d.not_before.unwrap_or(now);
    assert!(
        crate::extension::owes_of(&d, Mode::Land, at, other.landing, None)
            .is_some_and(|owed| !owed.landing),
        "a wall that cut no land act: {d:?}"
    );
}

/// THE HOUR'S BUDGET CROSSES A RE-EXEC OR A RELAUNCH (aterm's `seed_acts`): the acts the
/// process before typed within the window count toward `continue_per_hour` here, so the
/// relaunch waits for the oldest to leave the window where that process would have.
/// NEGATIVE CONTROLS: unseeded, at once; acts older than the window count for nothing; a
/// memory that typed acts of its own keeps them.
#[test]
fn the_hours_budget_carried_across_a_relaunch_is_spent_where_it_was_left() {
    let t = Timing::default();
    let start = Instant::now();
    let p = Policy {
        continue_per_hour: Some(2),
        ..policy()
    };
    let now = start + 70 * MIN;
    let relaunched = |acts: &[Instant]| {
        let mut s = ThreadState::default();
        s.observe_resume(true, Some("Working."), false);
        s.seed_acts(acts, now, &t);
        s.observe_turn_end(end(now, "Stage 3.", 5 * MIN), &t);
        probed(&mut s, &p, &t, now, None)
    };
    let d = relaunched(&[start + 30 * MIN, start + 20 * MIN]);
    assert_eq!(typed(&d).map(|(rule, _)| rule), Some(words::RULE_CONTINUE));
    assert_eq!(
        d.not_before,
        Some(start + 80 * MIN),
        "the oldest leaves the window"
    );
    // NEGATIVE CONTROLS: unseeded, or older than the window: at once.
    assert_eq!(relaunched(&[]).not_before, None);
    assert_eq!(relaunched(&[start, start + 5 * MIN]).not_before, None);
    let mut s = ThreadState::default();
    s.typed.push_back(now - MIN);
    s.seed_acts(&[start + 20 * MIN, start + 30 * MIN], now, &t);
    assert_eq!(s.typed.len(), 1, "its own acts kept");
}

/// The wall Codex's incomplete response is, by its reason (`extension::classify`).
fn incomplete(reason: &str) -> Option<Wall> {
    use codex_protocol::error::CodexErr;
    use codex_protocol::error::CodexErrorDetails;
    use codex_protocol::error::ResponseIncompleteError;
    let error = CodexErr::new(CodexErrorDetails::ResponseIncomplete(Box::new(
        ResponseIncompleteError {
            partial_output: Vec::new(),
            reason: reason.to_string(),
            response_id: Some("resp-long".to_string()),
            token_usage: None,
        },
    )));
    crate::extension::classify(&error.to_codex_protocol_error(), error.details(), true)
}

/// A REPLY PAST ITS OUTPUT TOKEN MAXIMUM IS RETRIED AT ONCE IN SMALLER PIECES (aterm's
/// `ApiCause::Overlong`): Codex's incomplete response for `max_output_tokens`, which core
/// never retries, gets `api-cutoff@v1` at once in aterm's words (`wall_retry_text`'s Overlong
/// case: write long files and outputs across several smaller replies), then the server's
/// ladder from each appearance; the long reply each try leads to is the wall's own cause, so
/// its ladder climbs on across that work (aterm's `work_is_its_cause`). NEGATIVE CONTROLS: a
/// server error's ladder starts over after the same work, another reason a response stopped
/// short stays a wall nothing typed gets past, and `retry_api_errors` off leaves it to a
/// person.
#[test]
fn a_reply_past_its_output_token_maximum_is_retried_at_once_in_smaller_pieces() {
    let t = Timing::default();
    let start = Instant::now();
    let p = policy();
    let wall = incomplete("max_output_tokens").unwrap_or_else(|| panic!("a wall"));
    // Each try leads to ten minutes of the long reply, cut off again.
    let tries = |wall: &Wall| {
        let mut s = ThreadState::default();
        s.observe_person_turn();
        let mut at = start;
        let mut decided = Vec::new();
        for _ in 0..3 {
            s.observe_turn_end(
                TurnEnd {
                    worked: 10 * MIN,
                    ..walled(at, wall.clone())
                },
                &t,
            );
            let d = decide(&s, &p, &t, at, NO_JITTER);
            let try_at = d.not_before.unwrap_or(at).max(at);
            decided.push((d.clone(), at));
            s.acted(&d.act, try_at, &t);
            at = try_at + 11 * MIN;
        }
        decided
    };
    let decided = tries(&wall);
    for (d, _) in &decided {
        let (rule, text) = typed(d).expect("a retry");
        assert_eq!(rule, words::RULE_API_CUTOFF);
        assert!(text.contains("across several smaller replies"));
        assert!(!text.contains("NEEDS YOU"));
    }
    let waits: Vec<Option<Duration>> = decided
        .iter()
        .map(|(d, at)| d.not_before.map(|nb| nb.saturating_duration_since(*at)))
        .collect();
    assert_eq!(
        waits,
        [None, Some(MIN), Some(5 * MIN)],
        "at once, then climbing"
    );
    assert!(decided[1].0.shown_wait.is_some() && decided[0].0.shown_wait.is_none());
    // NEGATIVE CONTROLS.
    let server = Wall::Server {
        what: "an API error (500)".to_string(),
        cut_off: false,
        retry_at: None,
    };
    let starts_over: Vec<Option<Duration>> = tries(&server)
        .iter()
        .map(|(d, at)| d.not_before.map(|nb| nb.saturating_duration_since(*at)))
        .collect();
    assert_eq!(starts_over, [Some(MIN), Some(MIN), Some(MIN)]);
    let other = incomplete("some_other_reason").unwrap_or_else(|| panic!("a wall"));
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(walled(start, other), &t);
    assert!(matches!(
        decide(&s, &p, &t, start, NO_JITTER).act,
        Act::Attention { key: "wall", .. }
    ));
    let off = Policy {
        retry_api_errors: false,
        ..policy()
    };
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(walled(start, wall), &t);
    assert!(matches!(
        decide(&s, &off, &t, start, NO_JITTER).act,
        Act::Attention { key: "api", .. }
    ));
}

/// THE OVERLONG LADDER COUNTS FROM THE APPEARANCE THAT ANSWERED codex-alab's TRY (aterm's
/// `track_wall`: the appearance moves only at a retry of its own, `if retried { t.since = now
/// }`): a person's turn, or a goal's, that meets the wall again keeps the ladder's rung and
/// its count, so the next try goes when it was due, never a rung later. Before, every
/// appearance restarted the rung, and the try came 4 minutes after aterm's here. NEGATIVE
/// CONTROL: the appearance that answers codex-alab's own try moves it (the try's turn ends
/// cut off again, and the next rung counts from there).
#[test]
fn the_overlong_ladder_counts_from_the_appearance_that_answered_the_try() {
    let t = Timing::default();
    let start = Instant::now();
    let p = policy();
    let wall = incomplete("max_output_tokens").unwrap_or_else(|| panic!("a wall"));
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(
        TurnEnd {
            worked: 10 * MIN,
            ..walled(start, wall.clone())
        },
        &t,
    );
    let d = decide(&s, &p, &t, start, NO_JITTER);
    assert_eq!(d.not_before, None, "at once");
    s.acted(&d.act, start, &t);
    let t1 = start + 10 * MIN;
    s.observe_turn_end(
        TurnEnd {
            worked: 10 * MIN,
            ..walled(t1, wall.clone())
        },
        &t,
    );
    let d = decide(&s, &p, &t, t1, NO_JITTER);
    assert_eq!(d.not_before, Some(t1 + MIN));
    s.acted(&d.act, t1 + MIN, &t);
    let t2 = t1 + 11 * MIN;
    s.observe_turn_end(
        TurnEnd {
            worked: 10 * MIN,
            ..walled(t2, wall.clone())
        },
        &t,
    );
    // NEGATIVE CONTROL: the try's own answer moved the appearance.
    assert_eq!(
        decide(&s, &p, &t, t2, NO_JITTER).not_before,
        Some(t2 + 5 * MIN)
    );
    for (by_person, who) in [(true, "a person's turn"), (false, "a goal's turn")] {
        let mut s = s.clone();
        let met = t2 + 4 * MIN;
        s.observe_turn_end(
            TurnEnd {
                by_autopilot: false,
                by_person,
                worked: 3 * MIN,
                ..walled(met, wall.clone())
            },
            &t,
        );
        let d = decide(&s, &p, &t, met, NO_JITTER);
        assert_eq!(
            typed(&d).map(|(rule, _)| rule),
            Some(words::RULE_API_CUTOFF),
            "{who}"
        );
        assert_eq!(d.not_before, Some(t2 + 5 * MIN), "{who}: {d:?}");
    }
}

#[test]
fn an_unreached_api_uses_the_network_ladder() {
    let t = Timing::default();
    let start = Instant::now();
    let mut s = ThreadState::default();
    s.observe_person_turn();
    let wall = || Wall::Network {
        what: "an API error (the API could not be reached)".to_string(),
    };
    let mut now = start;
    for step in [1, 2, 5, 5] {
        s.observe_turn_end(walled(now, wall()), &t);
        let d = decide(&s, &policy(), &t, now, NO_JITTER);
        // From the appearance that answered the last try, 30 s after it.
        assert_eq!(d.not_before, Some(now + step * MIN), "step {step}");
        let tried = d.not_before.unwrap_or(now);
        s.acted(&d.act, tried, &t);
        now = tried + Duration::from_secs(30);
    }
}

/// THE API MEASURED REACHABLE AGAIN, OR DOWN (R4, aterm's `(Unreachable, Up)` and
/// `(Unreachable, Down)`): a network wall whose own route the probe measured Up is continued at
/// once under `api-back@v1`, in aterm's words with the standing rules after them; each return
/// the wall met again waits the network ladder's rung by the count of those acts (`ups`) from
/// the appearance that answered the last, typing the retry's words meanwhile (the record carries
/// no measure past the process), and its look at the rung under a fresh Up is `api-back@v1`
/// again. Measured Down, nothing is typed for the hold, 15 minutes from the appearance.
/// NEGATIVE CONTROLS: an unknown reach keeps the ladder by the tries; a server wall under an Up
/// keeps its own ladder and words; with retries off an Up types nothing.
#[test]
fn a_measured_return_is_continued_at_once_and_a_measured_outage_is_held() {
    let t = Timing::default();
    let start = Instant::now();
    let what = "an API error (the API could not be reached)";
    let wall = || Wall::Network {
        what: what.to_string(),
    };
    let rule = |d: &Decision| typed(d).map(|(rule, _)| rule);
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(walled(start, wall()), &t);
    assert_eq!(
        decide(&s, &policy(), &t, start, NO_JITTER).not_before,
        Some(start + MIN),
        "unknown: the ladder"
    );
    s.reach = Reach::Down { since: start };
    let d = decide(&s, &policy(), &t, start, NO_JITTER);
    assert_eq!(rule(&d), Some(words::RULE_API_RETRY));
    assert_eq!(d.not_before, Some(start + 15 * MIN), "down: the hold");
    assert!(d.why.contains("unreachable"), "{d:?}");
    let up_at = start + 3 * MIN;
    s.reach = Reach::Up { since: up_at };
    let d = decide(&s, &policy(), &t, up_at, NO_JITTER);
    assert_eq!(d.not_before, None, "up: at once");
    assert_eq!(
        typed(&d),
        Some((words::RULE_API_BACK, words::api_back_text(what).as_str()))
    );
    assert!(words::api_back_text(what).contains("the API is reachable again now"));
    s.acted(&d.act, up_at, &t);
    assert_eq!(s.wall.as_ref().map(|w| (w.attempts, w.ups)), Some((1, 1)));
    let met = up_at + Duration::from_secs(30);
    s.observe_turn_end(walled(met, wall()), &t);
    let d = decide(&s, &policy(), &t, met, NO_JITTER);
    assert_eq!(d.not_before, Some(met + MIN), "met again: the first rung");
    assert_eq!(
        rule(&d),
        Some(words::RULE_API_RETRY),
        "the wait claims no reach"
    );
    let d = decide(&s, &policy(), &t, met + MIN, NO_JITTER);
    assert_eq!((rule(&d), d.not_before), (Some(words::RULE_API_BACK), None));
    s.acted(&d.act, met + MIN, &t);
    let met2 = met + MIN + Duration::from_secs(30);
    s.observe_turn_end(walled(met2, wall()), &t);
    assert_eq!(
        decide(&s, &policy(), &t, met2, NO_JITTER).not_before,
        Some(met2 + 2 * MIN),
        "the second return met again: the second rung"
    );
    s.reach = Reach::Unknown;
    assert_eq!(
        decide(&s, &policy(), &t, met2, NO_JITTER).not_before,
        Some(met2 + 5 * MIN),
        "unknown again: the ladder by the tries"
    );
    // NEGATIVE CONTROL: a server wall under an Up keeps its own ladder and words.
    let mut server = ThreadState::default();
    server.observe_person_turn();
    server.observe_turn_end(
        walled(
            start,
            Wall::Server {
                what: "an API error (500)".to_string(),
                cut_off: false,
                retry_at: None,
            },
        ),
        &t,
    );
    server.reach = Reach::Up { since: start };
    let d = decide(&server, &policy(), &t, start, NO_JITTER);
    assert_eq!(
        (rule(&d), d.not_before),
        (Some(words::RULE_API_RETRY), Some(start + MIN))
    );
    // NEGATIVE CONTROL: retries off, an Up types nothing.
    let off = Policy {
        retry_api_errors: false,
        ..policy()
    };
    s.reach = Reach::Up { since: met2 };
    assert!(matches!(
        decide(&s, &off, &t, met2, NO_JITTER).act,
        Act::Attention { key: "api", .. }
    ));
}

/// AN API-BACK A CHECK HOLDS BACK IS CARRIED AS THE PLAIN RETRY (parity-3 review, F1): the
/// try typed on a measured return (`api-back@v1`, due at once) waits on a before-every-act
/// check at its point (a person's hold, a draft, a popup), and what the record carries
/// meanwhile is that wall's plain retry (`api-retry@v1` in the retry's words, which claim no
/// measure a relaunch has not taken), due as the try was: a crash at rest under the check
/// hands aterm's relaunch the wall's try, where it carried nothing and the relaunch typed
/// nothing until a person's turn. NEGATIVE CONTROLS: the retry's own wait carries itself, a
/// server wall's retry under an Up carries itself, and an attention carries nothing.
#[test]
fn an_api_back_held_by_a_check_is_carried_as_the_plain_retry() {
    let t = Timing::default();
    let start = Instant::now();
    let what = "an API error (the API could not be reached)";
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(
        walled(
            start,
            Wall::Network {
                what: what.to_string(),
            },
        ),
        &t,
    );
    s.reach = Reach::Down { since: start };
    let retry = decide(&s, &policy(), &t, start, NO_JITTER);
    let (retry_rule, retry_words) =
        typed(&retry).unwrap_or_else(|| panic!("the held wall's retry: {retry:?}"));
    assert_eq!(retry_rule, words::RULE_API_RETRY);
    let up_at = start + 3 * MIN;
    s.reach = Reach::Up { since: up_at };
    let back = decide(&s, &policy(), &t, up_at, NO_JITTER);
    assert_eq!(
        (typed(&back).map(|(rule, _)| rule), back.not_before),
        (Some(words::RULE_API_BACK), None)
    );
    let owed = crate::extension::owes_of(&back, Mode::Cruise, up_at, false, None)
        .unwrap_or_else(|| panic!("the held api-back is carried: {back:?}"));
    assert_eq!(
        (
            owed.rule,
            owed.text.as_str(),
            owed.mode,
            owed.not_before,
            owed.resume_goal
        ),
        (
            words::RULE_API_RETRY,
            retry_words,
            Mode::Cruise,
            up_at,
            false
        )
    );
    // NEGATIVE CONTROLS: the retry carries itself; a server wall's retry under an Up carries
    // itself; an attention carries nothing.
    let at = start + 15 * MIN;
    let carried = crate::extension::owes_of(&retry, Mode::Cruise, at, false, None)
        .unwrap_or_else(|| panic!("the retry is carried"));
    assert_eq!(
        (carried.rule, carried.text.as_str()),
        (retry_rule, retry_words)
    );
    let mut server = ThreadState::default();
    server.observe_person_turn();
    server.observe_turn_end(
        walled(
            start,
            Wall::Server {
                what: "an API error (500)".to_string(),
                cut_off: false,
                retry_at: None,
            },
        ),
        &t,
    );
    server.reach = Reach::Up { since: start };
    let tried = decide(&server, &policy(), &t, start, NO_JITTER);
    let words_of = typed(&tried).map(|(rule, text)| (rule, text.to_string()));
    assert_eq!(
        crate::extension::owes_of(&tried, Mode::Cruise, start + MIN, false, None)
            .map(|owed| (owed.rule, owed.text)),
        words_of
    );
    let off = Policy {
        retry_api_errors: false,
        ..policy()
    };
    let raised = decide(&s, &off, &t, up_at, NO_JITTER);
    assert!(matches!(raised.act, Act::Attention { .. }), "{raised:?}");
    assert_eq!(
        crate::extension::owes_of(&raised, Mode::Cruise, up_at, false, None),
        None
    );
}

#[test]
fn a_usage_window_waits_for_its_reset_then_backs_off() {
    let t = Timing::default();
    let start = Instant::now();
    let reset = start + 90 * MIN;
    let mut s = ThreadState::default();
    s.observe_person_turn();
    let wall = || Wall::Usage {
        reset_at: Some(reset),
        reset_label: Some("14:05".to_string()),
    };
    s.observe_turn_end(walled(start, wall()), &t);
    let jitter = Duration::from_secs(17);
    let d = decide(&s, &policy(), &t, start, jitter);
    assert_eq!(d.not_before, Some(reset + Duration::from_secs(60) + jitter));
    assert!(matches!(d.act, Act::ResumeLimit { .. }));
    assert_eq!(
        d.shown_wait.as_deref(),
        Some("waiting for the usage reset at 14:05")
    );
    let tried = reset + 2 * MIN;
    s.acted(&d.act, tried, &t);
    // The wall again right after the continuation: 10 minutes, then 30.
    s.observe_turn_end(walled(tried, wall()), &t);
    assert_eq!(
        decide(&s, &policy(), &t, tried, jitter).not_before,
        Some(tried + 10 * MIN)
    );
    // Negative control: resume_limits off escalates, types nothing.
    let off = Policy {
        resume_limits: false,
        ..policy()
    };
    assert!(matches!(
        decide(&s, &off, &t, tried, jitter).act,
        Act::Attention { key: "limit", .. }
    ));
}

#[test]
fn fresh_restoration_can_resume_before_reset_without_erasing_retry_pacing() {
    let t = Timing::default();
    let now = Instant::now();
    let reset = now + 7 * 24 * 60 * MIN;
    let mut state = ThreadState::default();
    state.observe_person_turn();
    let wall = || Wall::Usage {
        reset_at: Some(reset),
        reset_label: None,
    };
    state.observe_turn_end(walled(now, wall()), &t);
    state.recovery_not_before = Some(reset);
    // A carried wall without account/bucket evidence retains its original floor.
    assert_eq!(
        decide(&state, &policy(), &t, now, Duration::ZERO).not_before,
        Some(reset + t.reset_grace)
    );
    let restored =
        crate::decide::decide_with_usage(&state, &policy(), &t, now, Duration::ZERO, true);
    assert_eq!(restored.not_before, Some(now));
    assert!(matches!(restored.act, Act::ResumeLimit { .. }));
    state.acted(&restored.act, now, &t);
    state.observe_turn_end(walled(now, wall()), &t);
    let retry = crate::decide::decide_with_usage(&state, &policy(), &t, now, Duration::ZERO, true);
    assert_eq!(retry.not_before, Some(now + 10 * MIN));
    let off = Policy {
        resume_limits: false,
        ..policy()
    };
    assert!(matches!(
        crate::decide::decide_with_usage(&state, &off, &t, now, Duration::ZERO, true).act,
        Act::Attention { .. }
    ));
}

#[test]
fn subscription_restoration_wakes_once_then_preserves_retry_pacing() {
    let t = Timing::default();
    let now = Instant::now();
    let mut state = ThreadState::default();
    state.observe_person_turn();
    state.observe_turn_end(walled(now, Wall::Entitlement), &t);
    state.recovery_not_before = Some(now + 24 * 60 * MIN);
    let restored =
        crate::decide::decide_with_usage(&state, &policy(), &t, now, Duration::ZERO, true);
    assert_eq!(restored.not_before, Some(now));
    state.acted(&restored.act, now, &t);
    state.observe_turn_end(walled(now, Wall::Entitlement), &t);
    let retry = crate::decide::decide_with_usage(&state, &policy(), &t, now, Duration::ZERO, true);
    assert_eq!(retry.not_before, Some(now + t.entitlement_backoff[0]));
}

/// A USAGE WALL MET AGAIN AFTER ITS RESUME WAITS FOR THE RESET IT NAMES NOW (aterm's
/// `resume_at_reset`: the reset plus the grace and the jitter, or the back-off after the
/// continuation that met the wall again, whichever is later). The resume past a session
/// window's reset that meets the weekly limit, which resets days later, waits for that reset,
/// in this process and in aterm's relaunch, whose thread takes up the rung the dead process
/// was on (the record's `pace`, S2): before, attempts past the first read only the back-off,
/// and typed into the standing weekly limit 10 minutes after the resume, then every 30.
/// NEGATIVE CONTROLS: a wall met again that names a reset already past waits out the
/// back-off ([`a_usage_window_waits_for_its_reset_then_backs_off`]), and an unseeded relaunch
/// waits for the named reset as the first appearance does.
#[test]
fn a_usage_wall_met_again_waits_for_the_reset_it_names_now() {
    let t = Timing::default();
    let start = Instant::now();
    let jitter = Duration::from_secs(17);
    let session_reset = start + 5 * MIN;
    let weekly_reset = start + 3 * 24 * 60 * MIN;
    let usage = |reset: Instant| Wall::Usage {
        reset_at: Some(reset),
        reset_label: Some("14:05".to_string()),
    };
    let grace = t.reset_grace;
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(walled(start, usage(session_reset)), &t);
    let d = decide(&s, &policy(), &t, start, jitter);
    assert_eq!(d.not_before, Some(session_reset + grace + jitter));
    let tried = session_reset + grace + jitter;
    s.acted(&d.act, tried, &t);
    // The resume meets the weekly limit at once.
    let met = tried + Duration::from_secs(3);
    s.observe_turn_end(walled(met, usage(weekly_reset)), &t);
    let d = decide(&s, &policy(), &t, met, jitter);
    assert!(matches!(d.act, Act::ResumeLimit { .. }), "{d:?}");
    assert_eq!(d.not_before, Some(weekly_reset + grace + jitter), "{d:?}");
    // aterm's relaunch: the dead process's rung, taken up, meets the weekly limit at its
    // carry-on.
    let pace = s.wall.clone();
    assert_eq!(pace.as_ref().map(|w| w.attempts), Some(1));
    let relaunched = |seeded: bool| {
        let mut r = ThreadState::default();
        r.observe_resume(true, Some("Working on it."), true);
        if seeded {
            r.seed_pace(0, pace.clone());
        }
        let d = probed(&mut r, &policy(), &t, met + MIN, None);
        assert_eq!(typed(&d).map(|(rule, _)| rule), Some(words::RULE_HOST_TURN));
        r.acted(&d.act, met + MIN, &t);
        let again = met + MIN + Duration::from_secs(3);
        r.observe_turn_end(walled(again, usage(weekly_reset)), &t);
        decide(&r, &policy(), &t, again, jitter).not_before
    };
    assert_eq!(relaunched(true), Some(weekly_reset + grace + jitter));
    // NEGATIVE CONTROL: unseeded, the first appearance's wait for the same reset.
    assert_eq!(relaunched(false), Some(weekly_reset + grace + jitter));
}

/// A quota wall's try waits the entitlement back-off and types the blind cleared line (aterm
/// `77161d6432`: it pinned `continue_text` before the one-line resume was ported).
#[test]
fn an_entitlement_wall_is_retried_rarely_and_never_bought() {
    let t = Timing::default();
    let start = Instant::now();
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(walled(start, Wall::Entitlement), &t);
    let d = decide(&s, &policy(), &t, start, NO_JITTER);
    assert_eq!(d.not_before, Some(start + 30 * MIN));
    assert_eq!(
        typed(&d),
        Some((words::RULE_LIMIT_RESUME, words::LIMIT_MAY_HAVE_CLEARED))
    );
}

#[test]
fn a_full_context_is_compacted_once_then_continued() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(walled(now, Wall::Context { auto_compact: true }), &t);
    assert_eq!(run(&mut s, &policy(), &t, now).act, Act::Compact);
    // The compaction's own idle is no turn and no act.
    assert_eq!(decide(&s, &policy(), &t, now, NO_JITTER).act, Act::Nothing);
    s.observe_turn_end(ours(now, "", Duration::from_secs(20)), &t);
    assert_eq!(
        typed(&run(&mut s, &policy(), &t, now)),
        Some((words::RULE_COMPACT, "keep going"))
    );
    // A second wall right after: attention, no second compaction.
    s.observe_turn_end(walled(now, Wall::Context { auto_compact: true }), &t);
    assert!(matches!(
        decide(&s, &policy(), &t, now, NO_JITTER).act,
        Act::Attention { key: "context", .. }
    ));
    // A thread that does not compact, or compact_on_context_wall off: attention.
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(
        walled(
            now,
            Wall::Context {
                auto_compact: false,
            },
        ),
        &t,
    );
    assert!(matches!(
        decide(&s, &policy(), &t, now, NO_JITTER).act,
        Act::Attention { key: "context", .. }
    ));
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(walled(now, Wall::Context { auto_compact: true }), &t);
    let off = Policy {
        compact_on_context_wall: false,
        ..policy()
    };
    assert!(matches!(
        decide(&s, &off, &t, now, NO_JITTER).act,
        Act::Attention { key: "context", .. }
    ));
}

/// UNDER CRUISE THE COMPACTION'S CONTINUATION IS `continue_text` ALONE (aterm's
/// `continue_as(RULE_COMPACT)`, `continuation_text(cfg, rules)`): no reversible clause and no
/// NEEDS YOU ack, and an ack owed stays owed for the next free point's act (aterm's
/// `acted` marks a set acked only by an act typed with `NEEDS_ACK`). NEGATIVE CONTROL: under
/// Land the continuation is Land's words, which carry the reversible clause and the ack
/// (aterm's `land_words`), and they ack the set.
#[test]
fn cruises_compaction_continuation_is_the_continuation_alone() {
    let t = Timing::default();
    let now = Instant::now();
    let need = "Stage 1 is in.\nNEEDS YOU: approval to delete the release branch";
    let compacted = |p: &Policy| {
        let mut s = ThreadState::default();
        s.observe_person_turn();
        s.observe_turn_end(ours(now, need, 10 * MIN), &t);
        assert!(s.needs.ack_owed() && s.needs.destroy());
        // A turn nobody typed meets the full context before the point's act went.
        s.observe_turn_end(walled(now + MIN, Wall::Context { auto_compact: true }), &t);
        assert_eq!(run(&mut s, p, &t, now + MIN).act, Act::Compact);
        s.observe_turn_end(ours(now + 2 * MIN, "", Duration::from_secs(20)), &t);
        let d = run(&mut s, p, &t, now + 2 * MIN);
        (s, d)
    };
    let (s, d) = compacted(&policy());
    assert_eq!(typed(&d), Some((words::RULE_COMPACT, "keep going")));
    assert!(
        s.needs.ack_owed(),
        "the ack waits for the next free point's act"
    );
    // NEGATIVE CONTROL: Land's words carry them, and ack the set.
    let (s, d) = compacted(&land());
    assert_eq!(
        typed(&d),
        Some((
            words::RULE_COMPACT,
            act_words(words::LAND, &[words::DONE_CHECK_REVERSIBLE, NEEDS_ACK]).as_str()
        ))
    );
    assert!(!s.needs.ack_owed());
}

/// A WALL'S GOAL RESUMED IN PLACE OF LAND'S WORDS GIVES NO INSTRUCTION (review of the
/// forever-cruise lane, finding 3): under Land a full context's continuation types Land's words,
/// but where a turn error blocked the goal at that wall (the runtime's `GoalStop::Wall`) the
/// goal is resumed through `GoalService` instead, and no words go. Booked as the try
/// ([`ThreadState::wall_goal_resumed`]: the continuation's rule the last, its answer owed), it
/// leaves Land's episode as it was: no instruction given, no hold begun, no land act in flight,
/// the ack owed; so at the goal's own turn end Land's instruction comes, with the ack.
/// NEGATIVE CONTROL: the same continuation typed marks the instruction given, begins the hold
/// and acks the need.
#[test]
fn a_wall_goal_resumed_in_place_of_lands_words_gives_no_instruction() {
    let t = Timing::default();
    let now = Instant::now();
    let p = land();
    let need = "Stage 1 is in.\nNEEDS YOU: the signing key password";
    let mut s = ThreadState::default();
    s.observe_mode(Mode::Land);
    s.observe_person_turn();
    s.observe_turn_end(ours(now, need, 10 * MIN), &t);
    assert!(s.needs.ack_owed());
    s.observe_turn_end(walled(now + MIN, Wall::Context { auto_compact: true }), &t);
    assert_eq!(run(&mut s, &p, &t, now + MIN).act, Act::Compact);
    s.observe_turn_end(ours(now + 2 * MIN, "", Duration::from_secs(20)), &t);
    let d = probed(&mut s, &p, &t, now + 2 * MIN, None);
    assert_eq!(
        typed(&d),
        Some((
            words::RULE_COMPACT,
            act_words(words::LAND, &[NEEDS_ACK]).as_str()
        ))
    );
    assert!(!s.land.typed, "the episode had no instruction yet");
    let mut typed_at = s.clone();
    s.wall_goal_resumed(&d.act, now + 2 * MIN, &t);
    assert!(!s.land.typed, "no instruction was given");
    assert!(!s.landing, "the goal's turn answers no land act");
    assert!(s.needs.ack_owed(), "the ack stays owed");
    assert_eq!(s.last_rule, Some(words::RULE_COMPACT), "booked as the try");
    s.stamp_land_hold(1_000);
    assert_eq!(s.land_held(), None, "no hold begun");
    // The goal's own turn ends: Land's instruction, with the ack.
    let at = now + 20 * MIN;
    s.observe_turn_end(tooled(ours(at, "Stage 2 is in.", 15 * MIN)), &t);
    assert_eq!(
        typed(&probed(&mut s, &p, &t, at, None)),
        Some((
            words::RULE_LAND,
            act_words(words::LAND, &[NEEDS_ACK]).as_str()
        ))
    );
    // NEGATIVE CONTROL: typed, the continuation gives the instruction.
    typed_at.acted(&d.act, now + 2 * MIN, &t);
    assert!(typed_at.land.typed && typed_at.landing);
    assert!(!typed_at.needs.ack_owed());
    typed_at.stamp_land_hold(1_000);
    assert!(typed_at.land_held().is_some());
}

/// A USAGE-LIMITED GOAL RESUMED IN PLACE OF LAND'S WORDS GIVES NO INSTRUCTION (the
/// forever-persist lane, item b; owner 2026-10-19: FOREVER LAND): under Land the usage window's
/// resume at its reset ([`Act::ResumeLimit`]) carries Land's instruction where the episode had
/// not had it, but where the window stopped a goal the goal is resumed through `GoalService`
/// instead and the words are never typed: the goal's continuation is the upstream template,
/// which names no Land rule and no ack. Booked as the instruction given, the episode nudged at
/// the goal's turn end, its instruction never typed (the nudge's words lean on it) and the ack
/// never given. Booked as the try ([`ThreadState::goal_resumed`]), the episode stands as it was,
/// as a wall's goal resumed leaves it ([`ThreadState::wall_goal_resumed`]): at the goal's own
/// turn end Land's instruction comes, with the ack. NEGATIVE CONTROL: the same resume typed
/// gives the instruction, begins the hold and acks the need.
#[test]
fn a_usage_goal_resumed_in_place_of_lands_words_gives_no_instruction() {
    let t = Timing::default();
    let now = Instant::now();
    let p = land();
    let need = "Stage 1 is in.\nNEEDS YOU: the signing key password";
    let mut s = ThreadState::default();
    s.observe_mode(Mode::Land);
    s.observe_person_turn();
    s.observe_turn_end(ours(now, need, 10 * MIN), &t);
    assert!(s.needs.ack_owed());
    let usage = Wall::Usage {
        reset_at: Some(now + 30 * MIN),
        reset_label: None,
    };
    s.observe_turn_end(walled(now + MIN, usage), &t);
    let d = probed(&mut s, &p, &t, now + MIN, None);
    let want = act_words(words::LAND, &[NEEDS_ACK]);
    assert_eq!(d.act, Act::ResumeLimit { text: want.clone() });
    assert!(!s.land.typed, "the episode had no instruction yet");
    let mut typed_at = s.clone();
    let reset = now + 31 * MIN;
    s.goal_resumed(reset, &t);
    assert!(!s.land.typed, "no instruction was given");
    assert!(!s.landing, "the goal's turn answers no land act");
    assert!(s.needs.ack_owed(), "the ack stays owed");
    assert_eq!(
        s.last_rule,
        Some(words::RULE_LIMIT_RESUME),
        "booked as the try"
    );
    s.stamp_land_hold(1_000);
    assert_eq!(s.land_held(), None, "no hold begun");
    // The goal's own continuation ends (nobody's turn): Land's instruction, with the ack.
    let at = reset + 20 * MIN;
    s.observe_turn_end(
        tooled(TurnEnd {
            by_person: false,
            ..end(at, "Stage 2 is in.", 15 * MIN)
        }),
        &t,
    );
    assert_eq!(
        typed(&probed(&mut s, &p, &t, at, None)),
        Some((words::RULE_LAND, want.as_str()))
    );
    // NEGATIVE CONTROL: typed, the resume gives the instruction.
    typed_at.acted(&d.act, reset, &t);
    assert!(typed_at.land.typed && typed_at.landing);
    assert!(!typed_at.needs.ack_owed());
    typed_at.stamp_land_hold(1_000);
    assert!(typed_at.land_held().is_some());
}

/// A FULL CONTEXT THAT CUTS THE DEFEND PROMPT OR A LAND ACT IS OWED THAT ACT AGAIN (aterm's
/// `cut_text`, `compact_owes` and `compact_owed_text`, ATERM-KI-1016-1): the compaction's
/// summary may drop it, so the compaction's continuation types the cut act's text verbatim
/// under `context-compact@v1`, a wall's retry between them included, and a land act's again
/// is a land act's yield (its clean defense lands). NEGATIVE CONTROL: a compaction that cut
/// the continuation, or a person's turn, continues with the mode's act.
#[test]
fn a_full_context_that_cuts_the_defend_prompt_or_a_land_act_types_it_again() {
    let t = Timing::default();
    let now = Instant::now();
    let context = || Wall::Context { auto_compact: true };
    let compact_then = |s: &mut ThreadState, p: &Policy, at: Instant| {
        s.observe_turn_end(walled(at, context()), &t);
        assert_eq!(run(s, p, &t, at).act, Act::Compact);
        s.observe_turn_end(ours(at + MIN, "", Duration::from_secs(20)), &t);
        run(s, p, &t, at + MIN)
    };
    // Cruise: the defend prompt cut.
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Everything is done.", 5 * MIN), &t);
    let d = run(&mut s, &policy(), &t, now);
    assert_eq!(typed(&d), Some((words::RULE_DONE_CHECK, words::DONE_CHECK)));
    let d = compact_then(&mut s, &policy(), now + MIN);
    assert_eq!(typed(&d), Some((words::RULE_COMPACT, words::DONE_CHECK)));
    // …with a wall's retry between them.
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Everything is done.", 5 * MIN), &t);
    run(&mut s, &policy(), &t, now);
    s.observe_turn_end(
        walled(
            now + MIN,
            Wall::Server {
                what: "a 500".to_string(),
                cut_off: false,
                retry_at: None,
            },
        ),
        &t,
    );
    let d = run(&mut s, &policy(), &t, now + MIN);
    assert_eq!(typed(&d).map(|(r, _)| r), Some(words::RULE_API_RETRY));
    let d = compact_then(&mut s, &policy(), now + 2 * MIN);
    assert_eq!(typed(&d), Some((words::RULE_COMPACT, words::DONE_CHECK)));
    // Land: the instruction cut is typed again (never a nudge in its place), and its yield
    // is the instruction's.
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Started on it.", 5 * MIN), &t);
    let d = run(&mut s, &land(), &t, now);
    assert_eq!(typed(&d), Some((words::RULE_LAND, words::LAND)));
    let d = compact_then(&mut s, &land(), now + MIN);
    assert_eq!(typed(&d), Some((words::RULE_COMPACT, words::LAND)));
    s.observe_turn_end(
        ours(
            now + 3 * MIN,
            "Nothing left: every item is merged and pushed to origin/main.",
            3 * MIN,
        ),
        &t,
    );
    assert_eq!(run(&mut s, &land(), &t, now + 3 * MIN).act, Act::Landed);
    // NEGATIVE CONTROL: the continuation cut.
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Started on it.", 5 * MIN), &t);
    let d = run(&mut s, &policy(), &t, now);
    assert_eq!(typed(&d), Some((words::RULE_CONTINUE, "keep going")));
    let d = compact_then(&mut s, &policy(), now + MIN);
    assert_eq!(typed(&d), Some((words::RULE_COMPACT, "keep going")));
    // NEGATIVE CONTROL: a person's turn walled after the defend prompt went.
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Everything is done.", 5 * MIN), &t);
    run(&mut s, &policy(), &t, now);
    s.observe_turn_end(
        TurnEnd {
            wall: Some(context()),
            text: None,
            ..end(now + MIN, "", Duration::from_secs(5))
        },
        &t,
    );
    assert_eq!(run(&mut s, &policy(), &t, now + MIN).act, Act::Compact);
    s.observe_turn_end(ours(now + 2 * MIN, "", Duration::from_secs(20)), &t);
    let d = run(&mut s, &policy(), &t, now + 2 * MIN);
    assert_eq!(typed(&d), Some((words::RULE_COMPACT, "keep going")));
}

/// THE ACT A WALL CUT IS CARRIED ACROSS A RELAUNCH (B2; aterm's `cut_text`): the defend prompt
/// a wall cut, then the process killed in the wall's wait, its record's pace naming the act
/// (`pace.cut`, with the word it was typed under); the relaunch restores the wall's point, its
/// try delivers the act again, and a full context there has the compaction's continuation type
/// that act verbatim, as the process before would have. Before, the relaunch's compaction
/// typed the plain continuation in its place. NEGATIVE CONTROLS: nothing carried, and an act
/// carried under Land read under Cruise, continue with the mode's act.
#[test]
fn the_act_a_wall_cut_is_carried_across_a_relaunch() {
    let t = Timing::default();
    let now = Instant::now();
    let server = || Wall::Server {
        what: "a 500".to_string(),
        cut_off: false,
        retry_at: None,
    };
    let mut before = ThreadState::default();
    before.observe_turn_end(end(now, "Everything is done.", 5 * MIN), &t);
    run(&mut before, &policy(), &t, now);
    before.observe_turn_end(walled(now + MIN, server()), &t);
    let carried = before
        .cut
        .clone()
        .unwrap_or_else(|| panic!("the wall cut the defend prompt"));
    assert_eq!(carried, words::DONE_CHECK);
    for (seed, want) in [
        (Some(Mode::Cruise), words::DONE_CHECK),
        (None, "keep going"),
        (Some(Mode::Land), "keep going"),
    ] {
        let at = now + 10 * MIN;
        let mut s = ThreadState::default();
        if let Some(mode) = seed {
            s.seed_cut(carried.clone(), mode);
        }
        s.observe_resume(true, None, false);
        // The recovery's point: the wall, met by codex-alab's own act.
        s.observe_turn_end(walled(at, server()), &t);
        let d = run(&mut s, &policy(), &t, at);
        assert_eq!(
            typed(&d).map(|(r, _)| r),
            Some(words::RULE_API_RETRY),
            "{seed:?}"
        );
        s.observe_turn_end(walled(at + MIN, Wall::Context { auto_compact: true }), &t);
        assert_eq!(run(&mut s, &policy(), &t, at + MIN).act, Act::Compact);
        s.observe_turn_end(ours(at + 2 * MIN, "", Duration::from_secs(20)), &t);
        let d = run(&mut s, &policy(), &t, at + 2 * MIN);
        assert_eq!(typed(&d), Some((words::RULE_COMPACT, want)), "{seed:?}");
    }
}

#[test]
fn a_lost_login_types_nothing_and_raises_attention() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(walled(now, Wall::Login), &t);
    let d = decide(&s, &policy(), &t, now, NO_JITTER);
    assert!(
        matches!(d.act, Act::Attention { key: "login", .. }),
        "{d:?}"
    );
    assert!(!d.act.needs_hand());
}

/// THE LOST LOGIN'S LINE (gap 13 stage 1, aterm's `RULE_LOGIN_BACK`): once the runtime reads the
/// login back ([`ThreadState::login`]), the login wall types aterm's line at once with the
/// standing rules after it, under no switch (`retry_api_errors` and `continue` off here, as
/// aterm reads neither), budgeted (the hour's cap holds it, in aterm's words, until the window
/// has room), and no try past the wall: its tries stay none, the hour counts it, and its answer
/// is awaited as a wall try's. NEGATIVE CONTROL: the login not read back keeps today's ask
/// (`a_lost_login_types_nothing_and_raises_attention`).
#[test]
fn a_login_back_types_once() {
    let t = Timing::default();
    let now = Instant::now();
    let rules = "Never push to upstream.";
    let p = Policy {
        continue_on: false,
        continue_text: Some(format!("keep going (standing rules: {rules})")),
        retry_api_errors: false,
        ..policy()
    };
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(walled(now, Wall::Login), &t);
    assert!(
        matches!(
            decide(&s, &p, &t, now, NO_JITTER).act,
            Act::Attention { key: "login", .. }
        ),
        "not read back: the ask"
    );
    s.login.back = true;
    let d = decide(&s, &p, &t, now, NO_JITTER);
    assert_eq!(
        words::LOGIN_BACK,
        "[codex-alab] The expired login has cleared; continue where you stopped."
    );
    let line = format!("{} (standing rules: {rules})", words::LOGIN_BACK);
    assert_eq!(typed(&d), Some((words::RULE_LOGIN_BACK, line.as_str())));
    assert_eq!(d.not_before, None);
    // Budgeted: the hour's cap spent holds it until the window has room, in aterm's words.
    let capped = Policy {
        continue_per_hour: Some(1),
        ..p
    };
    let mut spent = s.clone();
    spent.typed.push_back(now);
    let held = decide(&spent, &capped, &t, now, NO_JITTER);
    assert_eq!(
        typed(&held).map(|(rule, _)| rule),
        Some(words::RULE_LOGIN_BACK)
    );
    assert_eq!(held.not_before, Some(now + t.window));
    assert!(
        held.shown_wait
            .as_deref()
            .is_some_and(|wait| wait.starts_with(words::HOUR_CAP)),
        "{held:?}"
    );
    s.acted(&d.act, now, &t);
    let track = s
        .wall
        .clone()
        .unwrap_or_else(|| panic!("the login's track"));
    assert_eq!((track.attempts, track.back_typed), (0, true));
    assert_eq!(s.typed.len(), 1, "the hour's budget counts it");
    assert_eq!(s.awaited, Some(true), "its answer is a wall try's");
}

/// THE LOST LOGIN'S LINE MET BY THE WALL AGAIN (aterm's `back_typed`): its turn meets the login
/// wall again, and with the login read back once more (another renewal) the ask stands and
/// nothing more is typed for that wall. NEGATIVE CONTROL: a turn that goes through ends the
/// wall's track, and the next lost login's line is typed.
#[test]
fn a_login_back_answered_by_the_wall_types_nothing_more() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(walled(now, Wall::Login), &t);
    s.login.back = true;
    let d = decide(&s, &policy(), &t, now, NO_JITTER);
    assert_eq!(
        typed(&d).map(|(rule, _)| rule),
        Some(words::RULE_LOGIN_BACK)
    );
    s.acted(&d.act, now, &t);
    let at = now + MIN;
    s.observe_turn_end(walled(at, Wall::Login), &t);
    s.login.back = true;
    let d = decide(&s, &policy(), &t, at, NO_JITTER);
    assert!(
        matches!(d.act, Act::Attention { key: "login", .. }),
        "the wall again: {d:?}"
    );
    // NEGATIVE CONTROL: a turn that goes through ends the track; the next wall's line is typed.
    let through = at + MIN;
    s.observe_turn_end(end(through, "Signed in again; carrying on.", 5 * MIN), &t);
    assert_eq!(s.wall, None);
    let again = through + MIN;
    s.observe_turn_end(walled(again, Wall::Login), &t);
    s.login.back = true;
    assert_eq!(
        typed(&decide(&s, &policy(), &t, again, NO_JITTER)).map(|(rule, _)| rule),
        Some(words::RULE_LOGIN_BACK)
    );
}

/// Review of 4cdc96090, finding 3: an attention answers nothing, so its point stands and is
/// decided (and raised) again when codex-alab acts again after a passive spell cleared it.
/// Negative control: an act that answers the point (a typed turn) clears it.
#[test]
fn an_attention_leaves_its_point_standing_and_a_typed_act_answers_it() {
    let t = Timing::default();
    let now = Instant::now();
    let ask = Policy {
        questions: Questions::Ask,
        ..policy()
    };
    for (point, p, key) in [
        (walled(now, Wall::Login), policy(), "login"),
        (
            walled(
                now,
                Wall::Usage {
                    reset_at: None,
                    reset_label: None,
                },
            ),
            Policy {
                resume_limits: false,
                ..policy()
            },
            "limit",
        ),
        (
            end(now, "Should I rename the module?", 5 * MIN),
            ask,
            "question",
        ),
    ] {
        let mut s = ThreadState::default();
        s.observe_person_turn();
        s.observe_turn_end(point, &t);
        let d = run(&mut s, &p, &t, now);
        assert!(
            matches!(d.act, Act::Attention { key: k, .. } if k == key),
            "{key}: {d:?}"
        );
        assert!(s.point.is_some(), "{key}: the point stands");
        assert_eq!(decide(&s, &p, &t, now, NO_JITTER).act, d.act, "{key}");
    }
    // Negative control: a continuation answers its point.
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(end(now, "Stage 1 is in.", 5 * MIN), &t);
    let d = run(&mut s, &policy(), &t, now);
    assert!(typed(&d).is_some(), "{d:?}");
    assert!(s.point.is_none());
}

#[test]
fn only_atermss_own_relaunch_of_a_working_session_is_a_relaunch() {
    use crate::extension::relaunched_by_aterm;
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::LaunchWitness;
    use codex_alab_core::session::Predecessor;
    use std::ffi::OsString;
    let line = [
        "/homes/_owner/.aterm/agents/codex",
        "--yolo",
        "resume",
        "019a-id",
    ];
    let witness = Predecessor {
        pid: 41,
        session_id: "019a-id".to_string(),
        exit: None,
        resume_argv: Some(line.iter().map(|w| (*w).to_string()).collect()),
        status: Some("busy".to_string()),
        updated_at_ms: 1_000_000,
        ..Default::default()
    };
    let args = |words: &[&str]| words.iter().map(OsString::from).collect::<Vec<_>>();
    let started = 1_000_000 + 60_000;
    // aterm's relaunch: its harness's hand on the tab at this start, no person since well
    // before it.
    let aterm = LaunchWitness {
        hand: Some("lease:aterm-harness@812".to_string()),
        human: HumanSeen::At(started - 600_000),
        read_at_ms: started + 400,
        server_pid: Some(812),
        hand_pid: codex_alab_core::session::HandPid::Absent,
    };
    assert!(relaunched_by_aterm(
        &witness,
        &args(&line),
        started,
        Some(&aterm)
    ));
    // aterm's own remedy (SIGTERM), and the terminal going away under it (SIGHUP: aterm's
    // crash, whose restore types this very line).
    for exit in ["signal:15", "signal:1"] {
        assert!(relaunched_by_aterm(
            &Predecessor {
                exit: Some(exit.to_string()),
                ..witness.clone()
            },
            &args(&line),
            started,
            Some(&aterm)
        ));
    }
    // A person's own `codex resume <id>` (a bare name, not aterm's line): no carry-on.
    assert!(!relaunched_by_aterm(
        &witness,
        &args(&["codex", "resume", "019a-id"]),
        started,
        Some(&aterm)
    ));
    // A session that died at rest owed nothing; an orderly or a person's end is no crash.
    for (status, exit) in [
        ("idle", None),
        ("waiting", None),
        ("busy", Some("fatal")),
        ("busy", Some("signal:2")),
        ("idle", Some("signal:1")),
        ("exited", Some("signal:1")),
    ] {
        let p = Predecessor {
            status: Some(status.to_string()),
            exit: exit.map(str::to_string),
            ..witness.clone()
        };
        assert!(
            !relaunched_by_aterm(&p, &args(&line), started, Some(&aterm)),
            "{status} {exit:?}"
        );
    }
    // A witness a day and more old is a later resume, whoever types it.
    let a_day = u64::try_from(crate::extension::RELAUNCH_WINDOW.as_millis()).unwrap_or(0);
    let late = 1_000_000 + a_day + 1;
    assert!(!relaunched_by_aterm(
        &witness,
        &args(&line),
        late,
        Some(&LaunchWitness {
            read_at_ms: late + 400,
            ..aterm
        })
    ));
}

/// F1 (review of 6efbf7518): under the managed link a person's own `codex resume <id>` runs
/// EXACTLY the record's `resumeArgv` (the link execs the package by its absolute path), so
/// the line proves nothing; only what aterm's relaunch alone carries does.
#[test]
fn a_persons_hand_resume_of_the_very_line_is_no_relaunch() {
    use crate::extension::PERSON_LINE_MARGIN;
    use crate::extension::relaunched_by_aterm;
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::LaunchWitness;
    use codex_alab_core::session::Predecessor;
    use std::ffi::OsString;
    let line = [
        "/homes/_owner/.local/share/aterm/dev-builds/artifacts/codex/0.164.0-dev.1/bin/codex",
        "resume",
        "019a-id",
    ];
    let args: Vec<OsString> = line.iter().map(OsString::from).collect();
    let started = 5_000_000;
    let margin = u64::try_from(PERSON_LINE_MARGIN.as_millis()).unwrap_or(0);
    // A tab closed mid-turn (SIGHUP, busy), resumed by hand.
    let closed = Predecessor {
        pid: 41,
        session_id: "019a-id".to_string(),
        exit: Some("signal:1".to_string()),
        resume_argv: Some(line.iter().map(|w| (*w).to_string()).collect()),
        status: Some("busy".to_string()),
        updated_at_ms: started - 60_000,
        ..Default::default()
    };
    // The person typed the line: no hand of aterm's, and their Enter just before the start.
    let person = LaunchWitness {
        hand: None,
        human: HumanSeen::At(started - 80),
        read_at_ms: started + 300,
        server_pid: Some(812),
        hand_pid: codex_alab_core::session::HandPid::Absent,
    };
    for exit in [Some("signal:1"), Some("signal:15"), None] {
        let p = Predecessor {
            exit: exit.map(str::to_string),
            ..closed.clone()
        };
        assert!(
            !relaunched_by_aterm(&p, &args, started, Some(&person)),
            "{exit:?}"
        );
        // No read at all (no aterm reached, no answer): no proof, no carry-on.
        assert!(!relaunched_by_aterm(&p, &args, started, None), "{exit:?}");
    }
    // Each half of the proof is needed. aterm's hand, but a person's keystroke inside the
    // margin before the start (they typed the line under it), or after it, or unknown:
    for human in [
        HumanSeen::At(started - margin + 1),
        HumanSeen::At(started + 100),
        HumanSeen::Unknown,
    ] {
        let w = LaunchWitness {
            hand: Some("lease:aterm-harness@812".to_string()),
            human,
            ..person.clone()
        };
        assert!(
            !relaunched_by_aterm(&closed, &args, started, Some(&w)),
            "{human:?}"
        );
    }
    // A person quiet, but no hand of aterm's: another driver's lease, a turn, or none.
    for hand in [
        None,
        Some("lease:someone-else@9"),
        Some("lease:aterm-harness@"),
        Some("turn:t1:aterm-harness@812"),
        Some("unknown"),
    ] {
        let w = LaunchWitness {
            hand: hand.map(str::to_string),
            human: HumanSeen::Never,
            ..person.clone()
        };
        assert!(
            !relaunched_by_aterm(&closed, &args, started, Some(&w)),
            "{hand:?}"
        );
    }
    // A read made long after the start speaks for nothing of the start.
    let late = LaunchWitness {
        hand: Some("lease:aterm-harness@812".to_string()),
        human: HumanSeen::Never,
        read_at_ms: started + 61_000,
        server_pid: Some(812),
        hand_pid: codex_alab_core::session::HandPid::Absent,
    };
    assert!(!relaunched_by_aterm(&closed, &args, started, Some(&late)));
    // Positive control: the same exit and line, under aterm's hand with the person quiet
    // since before the margin, is aterm's relaunch.
    let aterm = LaunchWitness {
        hand: Some("lease:aterm-harness@812".to_string()),
        human: HumanSeen::At(started - margin),
        read_at_ms: started + 300,
        server_pid: Some(812),
        hand_pid: codex_alab_core::session::HandPid::Absent,
    };
    assert!(relaunched_by_aterm(&closed, &args, started, Some(&aterm)));
}

/// F1b (review of 6efbf7518): a stall's restart carries on once; a continuation that died
/// before a turn ended (its record's `carriedOn` 1) is restarted, but its successor waits for
/// the person, so stall, restart, continue never loops.
#[test]
fn a_carry_on_that_died_before_a_turn_ended_gets_no_second() {
    use crate::extension::MAX_CARRY_ONS;
    use crate::extension::relaunched_by_aterm;
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::LaunchWitness;
    use codex_alab_core::session::Predecessor;
    use std::ffi::OsString;
    let line = ["/abs/bin/codex", "resume", "019a-id"];
    let args: Vec<OsString> = line.iter().map(OsString::from).collect();
    let started = 9_000_000;
    let aterm = LaunchWitness {
        hand: Some("lease:aterm-harness@812".to_string()),
        human: HumanSeen::Never,
        read_at_ms: started + 200,
        server_pid: Some(812),
        hand_pid: codex_alab_core::session::HandPid::Absent,
    };
    let stalled = Predecessor {
        pid: 41,
        session_id: "019a-id".to_string(),
        exit: Some("signal:15".to_string()),
        resume_argv: Some(line.iter().map(|w| (*w).to_string()).collect()),
        status: Some("busy".to_string()),
        updated_at_ms: started - 1_000,
        carried_on: 0,
        needs_off: 0,
        workdir: None,
        owed_act: None,
        recovery: None,
        in_flight_retry: None,
        pace: None,
        aterm_session: None,
    };
    // The first stall's restart: carried on.
    assert!(relaunched_by_aterm(&stalled, &args, started, Some(&aterm)));
    assert_eq!(MAX_CARRY_ONS, 1);
    // The continuation stalled too (and so did any later one), every exit kind alike.
    for exit in [Some("signal:15"), Some("signal:1"), None] {
        for chain in [1, 2] {
            let again = Predecessor {
                exit: exit.map(str::to_string),
                carried_on: chain,
                ..stalled.clone()
            };
            assert!(
                !relaunched_by_aterm(&again, &args, started, Some(&aterm)),
                "{exit:?} {chain}"
            );
        }
    }
}

/// When [`relaunched_start`]'s process started, and a resume a second after it.
const STARTED: u64 = 9_000_000;
const SOON: u64 = STARTED + 1_000;

/// A shared state as aterm's relaunch of `line` leaves it at this process's start: the swept
/// predecessor (`carried_on` its count), and aterm's first read under the harness's lease of
/// the aterm process that served it.
fn relaunched_start(
    line: &[&str],
    carried_on: u32,
    hand_pid: u32,
    server_pid: Option<u32>,
) -> &'static codex_alab_core::session::Shared {
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::Launch;
    use codex_alab_core::session::LaunchWitness;
    use codex_alab_core::session::Predecessor;
    use codex_alab_core::session::Shared;
    let started = STARTED;
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    shared.set_launch(Launch {
        args: line.iter().map(std::ffi::OsString::from).collect(),
        started_at_ms: started,
        ..Default::default()
    });
    shared.note_predecessor(Predecessor {
        pid: 41,
        session_id: line[line.len() - 1].to_string(),
        exit: Some("signal:1".to_string()),
        resume_argv: Some(line.iter().map(|w| (*w).to_string()).collect()),
        status: Some("busy".to_string()),
        updated_at_ms: started - 1_000,
        carried_on,
        needs_off: 0,
        workdir: None,
        owed_act: None,
        recovery: None,
        in_flight_retry: None,
        pace: None,
        aterm_session: None,
    });
    shared.set_swept();
    shared.set_launch_witness(Some(LaunchWitness {
        hand: Some(format!("lease:aterm-harness@{hand_pid}")),
        human: HumanSeen::Never,
        read_at_ms: started + 200,
        server_pid,
        hand_pid: codex_alab_core::session::HandPid::Absent,
    }));
    shared
}

#[test]
fn a_delayed_first_frame_does_not_spend_an_unresolved_bound_relaunch() {
    use crate::extension::Relaunch;
    use crate::extension::relaunch;
    use codex_alab_core::session::HandPid;
    use codex_alab_core::session::Shared;
    let line = ["/abs/bin/codex", "resume", "019a-id"];
    let source = relaunched_start(&line, 0, 812, Some(812));
    let late = Shared::new();
    late.set_launch(source.launch().expect("launch").clone());
    let mut witness = source.launch_witness().expect("witness");
    witness.hand_pid = HandPid::Pid(812);
    late.set_launch_witness(Some(witness));
    assert_eq!(
        relaunch(&late, "019a-id", STARTED + 6_000),
        Relaunch::Nothing
    );
    assert!(
        !late.relaunch_spent(),
        "pending sweep is not a negative verdict"
    );
    late.note_predecessor(source.predecessor("019a-id").expect("predecessor"));
    late.set_swept();
    assert_eq!(
        relaunch(&late, "019a-id", STARTED + 6_000),
        Relaunch::CarryOn
    );
    assert_eq!(
        relaunch(&late, "019a-id", STARTED + 6_001),
        Relaunch::Nothing
    );

    // A process killed at rest retains its exact paced act through the same late sweep.
    let resting = Shared::new();
    resting.set_launch(source.launch().expect("launch").clone());
    resting.set_launch_witness(source.launch_witness().map(|mut witness| {
        witness.hand_pid = HandPid::Pid(812);
        witness
    }));
    assert_eq!(
        relaunch(&resting, "019a-id", STARTED + 6_000),
        Relaunch::Nothing
    );
    assert!(!resting.relaunch_spent());
    let owed = codex_alab_core::session::OwedAct {
        thread: "019a-id".into(),
        rule: Some(words::RULE_DONE_CHECK.into()),
        text: Some("Review the remaining work.".into()),
        mode: Some("cruise".into()),
        not_before_ms: STARTED + 240_000,
        ..Default::default()
    };
    let mut predecessor = source.predecessor("019a-id").expect("predecessor");
    predecessor.status = Some("idle".into());
    predecessor.owed_act = Some(owed.clone());
    resting.note_predecessor(predecessor);
    resting.set_swept();
    assert_eq!(
        relaunch(&resting, "019a-id", STARTED + 6_000),
        Relaunch::Owed(owed)
    );
}

/// Finding 1 (review of 148a9fedb): the relaunch is one fact about this process's start, so
/// it carries on once. `/resume` away from the relaunched thread and back to it re-runs the
/// resume scan with the same predecessor and the same launch read: the second time is no
/// relaunch, and nothing is typed.
#[test]
fn a_resume_away_and_back_in_the_relaunched_process_types_nothing_again() {
    use crate::extension::relaunched;
    let t = Timing::default();
    let now = Instant::now();
    let line = ["/abs/bin/codex", "resume", "019a-id"];
    let shared = relaunched_start(&line, 0, 812, Some(812));
    // The first resume: aterm's relaunch, carried on (the positive control).
    let first = relaunched(shared, "019a-id", SOON);
    assert!(first);
    let mut s = ThreadState::default();
    s.observe_resume(true, Some("Working through stage 2."), first);
    assert_eq!(
        typed(&decide(&s, &policy(), &t, now, NO_JITTER)),
        Some((words::RULE_HOST_TURN, "keep going"))
    );
    // `/resume` to another thread (it has no predecessor) ...
    assert!(!relaunched(shared, "019b-other", SOON));
    // ... and back: a fresh thread state, the same history, the same two facts. Nothing.
    let again = relaunched(shared, "019a-id", SOON);
    assert!(!again, "the relaunch was spent by the first resume");
    let mut s = ThreadState::default();
    s.observe_resume(true, Some("Working through stage 2."), again);
    assert_eq!(decide(&s, &policy(), &t, now, NO_JITTER).act, Act::Nothing);
}

/// AN ACT OWED AT REST SURVIVES A CRASH (FOREVER CRUISE / FOREVER LAND: the back-off's wait
/// is rest, so the record says `idle` while a paced act waits, and aterm's relaunch of an idle
/// session carried nothing on: the act was lost for good, end-to-end run 2026-10-20): the dead
/// process's record carried the act it owed, and aterm's relaunch, proven as for the carry-on,
/// hands it to the first resume, typed as the dead process decided it, at its time. NEGATIVE
/// CONTROLS: a person's own resume of the same line, a `fatal` exit, a record that owed
/// nothing, another thread's act or resume, and a second resume in the process get nothing; a
/// predecessor that died working still gets the carry-on.
#[test]
fn atermss_relaunch_of_a_session_at_rest_hands_it_the_act_it_owed() {
    use crate::extension::Relaunch;
    use crate::extension::relaunch;
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::Launch;
    use codex_alab_core::session::LaunchWitness;
    use codex_alab_core::session::OwedAct;
    use codex_alab_core::session::Predecessor;
    use codex_alab_core::session::Shared;
    let line = ["/abs/bin/codex", "resume", "019a-id"];
    let act = OwedAct {
        renewal: None,
        native_origin: None,
        thread: "019a-id".to_string(),
        rule: Some(words::RULE_DONE_CHECK.to_string()),
        text: Some("Defend it, as decided.".to_string()),
        mode: Some("cruise".to_string()),
        not_before_ms: STARTED + 120_000,
        repo_owed: None,
        resume_goal: false,
        landing: false,
        landed: false,
    };
    let start = |status: &str, exit: Option<&str>, owed: Option<OwedAct>, human: HumanSeen| {
        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        shared.set_launch(Launch {
            args: line.iter().map(std::ffi::OsString::from).collect(),
            started_at_ms: STARTED,
            ..Default::default()
        });
        shared.note_predecessor(Predecessor {
            pid: 41,
            session_id: "019a-id".to_string(),
            exit: exit.map(str::to_string),
            resume_argv: Some(line.iter().map(|w| (*w).to_string()).collect()),
            status: Some(status.to_string()),
            updated_at_ms: STARTED - 1_000,
            owed_act: owed,
            ..Predecessor::default()
        });
        shared.set_swept();
        shared.set_launch_witness(Some(LaunchWitness {
            hand: Some("lease:aterm-harness@812".to_string()),
            human,
            read_at_ms: STARTED + 200,
            server_pid: Some(812),
            hand_pid: codex_alab_core::session::HandPid::Absent,
        }));
        shared
    };
    let shared = start("idle", None, Some(act.clone()), HumanSeen::Never);
    assert_eq!(
        relaunch(shared, "019a-id", SOON),
        Relaunch::Owed(act.clone())
    );
    assert_eq!(
        relaunch(shared, "019a-id", SOON),
        Relaunch::Nothing,
        "spent"
    );
    let other = OwedAct {
        thread: "019b-other".to_string(),
        ..act.clone()
    };
    for (exit, owed, human, why) in [
        (
            None,
            Some(act.clone()),
            HumanSeen::At(STARTED - 1_000),
            "a person typed the line",
        ),
        (Some("fatal"), Some(act.clone()), HumanSeen::Never, "fatal"),
        (None, None, HumanSeen::Never, "nothing owed"),
        (None, Some(other), HumanSeen::Never, "another thread's act"),
    ] {
        let shared = start("idle", exit, owed, human);
        assert_eq!(
            relaunch(shared, "019a-id", SOON),
            Relaunch::Nothing,
            "{why}"
        );
    }
    let shared = start("idle", None, Some(act.clone()), HumanSeen::Never);
    assert_eq!(relaunch(shared, "019b-other", SOON), Relaunch::Nothing);
    let shared = start("busy", None, Some(act.clone()), HumanSeen::Never);
    assert_eq!(
        relaunch(shared, "019a-id", SOON),
        Relaunch::Owed(act.clone())
    );
    for signal in ["signal:1", "signal:15"] {
        let shared = start("busy", Some(signal), Some(act.clone()), HumanSeen::Never);
        assert_eq!(
            relaunch(shared, "019a-id", SOON),
            Relaunch::Owed(act.clone())
        );
    }
    let shared = start("busy", None, None, HumanSeen::Never);
    assert_eq!(relaunch(shared, "019a-id", SOON), Relaunch::CarryOn);
    // A WALL'S TRY waiting at rest is handed over too; a compaction is no act a record carries.
    for (rule, handed) in [
        (words::RULE_API_RETRY, true),
        (words::RULE_API_CUTOFF, true),
        (words::RULE_LIMIT_RESUME, true),
        (words::RULE_COMPACT, false),
    ] {
        let wall = OwedAct {
            rule: Some(rule.to_string()),
            ..act.clone()
        };
        let shared = start("idle", None, Some(wall.clone()), HumanSeen::Never);
        let want = if handed {
            Relaunch::Owed(wall)
        } else {
            Relaunch::Nothing
        };
        assert_eq!(relaunch(shared, "019a-id", SOON), want, "{rule}");
    }
    // The act handed over is typed as decided, at its time.
    let t = Timing::default();
    let now = Instant::now();
    let later = OwedAct {
        not_before_ms: codex_alab_core::session::now_ms() + 120_000,
        ..act
    };
    let owed = crate::extension::Autopilot::owed_of(&later);
    assert!(owed.is_some());
    let mut s = ThreadState::default();
    s.observe_resume(true, Some("Nothing left: it is all on origin/main."), false);
    s.observe_owed(owed, false, now);
    s.observe_mode(Mode::Cruise);
    let d = decide(&s, &policy(), &t, now, NO_JITTER);
    assert_eq!(
        typed(&d),
        Some((words::RULE_DONE_CHECK, "Defend it, as decided."))
    );
    assert!(d.not_before.is_some_and(|at| at > now));
}

/// Finding 1, the second guard: a process whose own count already reached the maximum
/// carries on no more, whatever its predecessor's record says.
#[test]
fn a_process_at_its_own_carry_on_maximum_carries_on_no_more() {
    use crate::extension::MAX_CARRY_ONS;
    use crate::extension::relaunched;
    let line = ["/abs/bin/codex", "resume", "019a-id"];
    let shared = relaunched_start(&line, 0, 812, Some(812));
    shared.set_carried_on(MAX_CARRY_ONS);
    assert!(!relaunched(shared, "019a-id", SOON));
    // Negative control: the same start at a count of zero carries on.
    let shared = relaunched_start(&line, 0, 812, Some(812));
    assert!(relaunched(shared, "019a-id", SOON));
}

/// Finding 4 (review of 148a9fedb): any control-socket client can take a lease named
/// `aterm-harness@<any pid>`; only the lease of the aterm process that served the launch read
/// (the socket's kernel peer) is aterm's, and an unknown server pid proves nothing.
#[test]
fn a_lease_naming_another_pid_than_aterms_own_is_no_relaunch() {
    use crate::extension::aterm_relaunch_seen;
    use crate::extension::relaunched;
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::LaunchWitness;
    let line = ["/abs/bin/codex", "resume", "019a-id"];
    // A forged holder: a lease under the harness's name, for a pid that is not aterm's.
    let forged = relaunched_start(&line, 0, 4040, Some(812));
    assert!(!relaunched(forged, "019a-id", SOON));
    // aterm's own name, but the server's pid could not be told: fail closed.
    let unknown = relaunched_start(&line, 0, 812, None);
    assert!(!relaunched(unknown, "019a-id", SOON));
    // Positive control: the lease names the aterm process that answered.
    let aterm = relaunched_start(&line, 0, 812, Some(812));
    assert!(relaunched(aterm, "019a-id", SOON));
    // The holder's pid must be all of it: no prefix, no suffix, no padding.
    for hand in [
        "lease:aterm-harness@8120",
        "lease:aterm-harness@81",
        "lease:aterm-harness@0812",
        "lease:aterm-harness@812 ",
    ] {
        let w = LaunchWitness {
            hand: Some(hand.to_string()),
            human: HumanSeen::Never,
            read_at_ms: 10,
            server_pid: Some(812),
            hand_pid: codex_alab_core::session::HandPid::Absent,
        };
        assert!(!aterm_relaunch_seen(&w, 0), "{hand}");
    }
}

/// Finding 5 (review of 148a9fedb): a process that declines the carry-on because its
/// predecessor was already in a chain passes the count on (its record keeps it), so a later
/// relaunch of it still declines; it never resets the chain to 0.
#[test]
fn a_declined_carry_on_passes_the_count_on() {
    use crate::extension::MAX_CARRY_ONS;
    use crate::extension::relaunched;
    let line = ["/abs/bin/codex", "resume", "019a-id"];
    let shared = relaunched_start(&line, MAX_CARRY_ONS, 812, Some(812));
    assert_eq!(shared.carried_on(), 0);
    assert!(!relaunched(shared, "019a-id", SOON));
    assert_eq!(shared.carried_on(), MAX_CARRY_ONS, "the count is passed on");
    // Negative control: a carry-on that goes leaves the count for the act to raise.
    let shared = relaunched_start(&line, 0, 812, Some(812));
    assert!(relaunched(shared, "019a-id", SOON));
    assert_eq!(shared.carried_on(), 0);
}

/// Finding 2 (review of cf084e3b7): the process's first resume spends the relaunch whatever
/// it learns after the sweep settles. An empty sweep, or a first resume of another thread,
/// leaves nothing for a later `/resume` back to carry on from.
#[test]
fn the_first_resume_spends_the_relaunch_whatever_it_learns() {
    use crate::extension::relaunched;
    let line = ["/abs/bin/codex", "resume", "019a-id"];
    // The sweep settled empty at the first resume: no predecessor.
    let shared = relaunched_start(&line, 0, 812, Some(812));
    let predecessor = shared
        .predecessor("019a-id")
        .unwrap_or_else(|| panic!("noted"));
    let late = Box::leak(Box::new(codex_alab_core::session::Shared::new()));
    if let Some(launch) = shared.launch() {
        late.set_launch(launch.clone());
    }
    late.set_launch_witness(shared.launch_witness());
    late.set_swept();
    assert!(
        !relaunched(late, "019a-id", SOON),
        "no predecessor known yet"
    );
    // It arrives; the person's later `/resume` back reads it, and must not carry on.
    late.note_predecessor(predecessor);
    late.set_swept();
    assert!(
        !relaunched(late, "019a-id", SOON),
        "the first resume spent it"
    );
    // The first resume was of another thread: spent too.
    let other = relaunched_start(&line, 0, 812, Some(812));
    assert!(!relaunched(other, "019b-other", SOON));
    assert!(!relaunched(other, "019a-id", SOON));
    // Negative control: the same start, resumed first as the relaunched thread, carries on.
    let first = relaunched_start(&line, 0, 812, Some(812));
    assert!(relaunched(first, "019a-id", SOON));
}

/// Finding 2, the second guard: the start's two facts speak for the start only. A resume
/// more than the launch probe's window after it (a person's `/resume` back, hours later, in
/// a process that never resumed before) is no relaunch, and neither is one dated before it.
#[test]
fn a_resume_long_after_the_start_is_no_relaunch() {
    use crate::extension::LAUNCH_PROBE_WINDOW;
    use crate::extension::relaunched;
    let line = ["/abs/bin/codex", "resume", "019a-id"];
    let window = u64::try_from(LAUNCH_PROBE_WINDOW.as_millis()).unwrap_or(u64::MAX);
    for late in [STARTED + window + 1, STARTED + 3 * 3_600_000, STARTED - 1] {
        let shared = relaunched_start(&line, 0, 812, Some(812));
        assert!(!relaunched(shared, "019a-id", late), "{late}");
    }
    // Negative control: within the window, at its very edge too, it carries on.
    for soon in [STARTED, STARTED + window] {
        let shared = relaunched_start(&line, 0, 812, Some(812));
        assert!(relaunched(shared, "019a-id", soon), "{soon}");
    }
}

/// The relaunch lease bound to who holds it (aterm `status hand_pid=`, the kernel's word for
/// the process that took it): a lease spelled `aterm-harness@<aterm pid>` by any other client
/// of the socket is no proof once aterm names its taker, and a taker aterm cannot name proves
/// nothing either. An aterm that prints no `hand_pid=` keeps the name check alone (the
/// negative control: that judgement still takes the forged lease, which is why the token is
/// needed).
#[test]
fn a_lease_forged_under_aterms_name_is_no_relaunch_once_aterm_names_its_taker() {
    use crate::extension::aterm_relaunch_seen;
    use codex_alab_core::session::HandPid;
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::LaunchWitness;
    let started = 7_000_000;
    let read = |hand_pid: HandPid| LaunchWitness {
        hand: Some("lease:aterm-harness@812".to_string()),
        human: HumanSeen::Never,
        read_at_ms: started + 200,
        server_pid: Some(812),
        hand_pid,
    };
    // aterm's own harness took the lease: its taker is the server itself.
    assert!(aterm_relaunch_seen(&read(HandPid::Pid(812)), started));
    // A same-uid client (pid 4242) took a lease under aterm's name.
    assert!(!aterm_relaunch_seen(&read(HandPid::Pid(4242)), started));
    // aterm reports the token but cannot name the taker.
    assert!(!aterm_relaunch_seen(&read(HandPid::Unknown), started));
    // An older aterm prints no token: the name check stands, and it cannot tell the forged
    // lease from aterm's own.
    assert!(aterm_relaunch_seen(&read(HandPid::Absent), started));
    // The taker matching the server never rescues a lease named for another pid.
    let other = LaunchWitness {
        hand: Some("lease:aterm-harness@9".to_string()),
        ..read(HandPid::Pid(812))
    };
    assert!(!aterm_relaunch_seen(&other, started));
    // Nor an unknown server pid.
    let unknown_server = LaunchWitness {
        server_pid: None,
        ..read(HandPid::Pid(812))
    };
    assert!(!aterm_relaunch_seen(&unknown_server, started));
}

/// A clean defense: a `Nothing left:` line, no need, no open work in Land's strict reading.
const CLEAN: &str = "Nothing left: the parser and the lexer are merged and pushed to origin/main, \
                     and the worktrees are removed.";

/// FOREVER LAND: the instruction at the episode's first point, then a nudge at every free
/// point with no bound; a question gets the land answer. NEGATIVE CONTROL: the same points
/// under Cruise get the continuation and the owner's answer.
#[test]
fn land_types_its_instruction_then_nudges_without_bound() {
    let t = Timing::default();
    let now = Instant::now();
    let p = land();
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Stage 1 is in.", 5 * MIN), &t);
    let d = run(&mut s, &p, &t, now);
    assert_eq!(typed(&d), Some((words::RULE_LAND, words::LAND)));
    assert!(s.land.typed && s.landing);
    let mut at = now;
    for n in 1..=12 {
        at += 30 * MIN;
        s.observe_turn_end(
            tooled(ours(
                at,
                "Merged the parser; pushing the lexer next.",
                10 * MIN,
            )),
            &t,
        );
        let d = run(&mut s, &p, &t, at);
        assert_eq!(
            typed(&d),
            Some((words::RULE_LAND_NUDGE, words::LAND_NUDGE)),
            "{n}"
        );
        assert_eq!(d.not_before, None, "{n}: real work backs nothing off");
    }
    assert_eq!(s.land.nudges, 12);
    at += 30 * MIN;
    s.observe_turn_end(ours(at, "Should I rename the module?", 10 * MIN), &t);
    assert_eq!(
        typed(&run(&mut s, &p, &t, at)),
        Some((words::RULE_LAND_ANSWER, words::LAND_ANSWER))
    );
    // NEGATIVE CONTROL: Cruise.
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Stage 1 is in.", 5 * MIN), &t);
    assert_eq!(
        typed(&run(&mut s, &policy(), &t, now)),
        Some((words::RULE_CONTINUE, "keep going"))
    );
    s.observe_turn_end(ours(at, "Should I rename the module?", 10 * MIN), &t);
    assert_eq!(
        typed(&run(&mut s, &policy(), &t, at)).map(|(r, _)| r),
        Some(words::RULE_ANSWER)
    );
}

/// LAND LANDS only at a land act's yield whose reply is a clean defense, over a repository
/// that holds nothing no remote main has: recorded (`landed@v1`), never typed, then nothing
/// until someone else's turn, which opens a new episode. NEGATIVE CONTROLS: the defense on a
/// person's turn (no land act yet) gets the instruction; work in the repository gets the nudge
/// naming it; a defense naming open work, or a need, gets the nudge and no landing (a done
/// claim's nudge names what the repository holds beside it, read once the act is due); a
/// point that claims nothing reads no repository.
#[test]
fn land_lands_a_clean_defense_over_a_clean_repository_only() {
    let t = Timing::default();
    let now = Instant::now();
    let p = land();
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, CLEAN, 5 * MIN), &t);
    assert_eq!(
        typed(&run(&mut s, &p, &t, now)),
        Some((words::RULE_LAND, words::LAND))
    );
    // The yield defends cleanly: the probe first; work found, the nudge names it.
    let at = now + 20 * MIN;
    s.observe_turn_end(ours(at, CLEAN, 10 * MIN), &t);
    s.observe_mode(Mode::Land);
    assert_eq!(decide(&s, &p, &t, at, NO_JITTER).act, Act::Probe);
    let d = probed(&mut s, &p, &t, at, Some(work()));
    let repo = words::repo_suffix(&work().summary());
    assert_eq!(
        typed(&d),
        Some((
            words::RULE_LAND_NUDGE,
            act_words(words::LAND_NUDGE, &[&repo]).as_str()
        ))
    );
    s.acted(&d.act, at, &t);
    assert!(s.landing);
    // Its yield defends cleanly over a clean repository: landed.
    let at = at + 20 * MIN;
    s.observe_turn_end(ours(at, CLEAN, 10 * MIN), &t);
    let d = run(&mut s, &p, &t, at);
    assert_eq!(d.act, Act::Landed);
    assert_eq!(d.act.rule(), words::RULE_LANDED);
    assert!(!d.act.needs_hand());
    assert!(s.landed && !s.landing);
    assert_eq!(decide(&s, &p, &t, at, NO_JITTER).act, Act::Nothing);
    // Someone else's turn opens it again: a new episode's instruction.
    s.observe_turn_end(end(at + MIN, "Also fix the docs typo.", 5 * MIN), &t);
    assert!(!s.landed);
    assert_eq!(
        typed(&run(&mut s, &p, &t, at + MIN)),
        Some((words::RULE_LAND, words::LAND))
    );
    // A defense that names open work: the nudge naming it, and what the repository holds
    // beside the claim (probed first, as aterm reads the repository at every done claim).
    let at = at + 30 * MIN;
    s.observe_turn_end(
        ours(
            at,
            "Nothing left: all merged, but the lexer is not landed.",
            10 * MIN,
        ),
        &t,
    );
    let open = open_work_suffix(&["not landed"]).unwrap_or_default();
    // Paced (a done claim is a short yield): the wait shows the words before the read.
    let d = decide(&s, &p, &t, at, NO_JITTER);
    assert_eq!(
        typed(&d),
        Some((
            words::RULE_LAND_NUDGE,
            act_words(words::LAND_NUDGE, &[&open]).as_str()
        ))
    );
    let due = d.not_before.unwrap_or(at);
    assert_eq!(decide(&s, &p, &t, due, NO_JITTER).act, Act::Probe);
    let d = probed(&mut s, &p, &t, due, Some(work()));
    assert_eq!(
        typed(&d),
        Some((
            words::RULE_LAND_NUDGE,
            act_words(words::LAND_NUDGE, &[&open, &repo]).as_str()
        ))
    );
    s.acted(&d.act, due, &t);
    // NEGATIVE CONTROL: a point that claims nothing reads no repository.
    let at = at + 30 * MIN;
    s.observe_turn_end(ours(at, "Merging the lexer now.", 10 * MIN), &t);
    assert_eq!(
        typed(&decide(&s, &p, &t, at, NO_JITTER)),
        Some((words::RULE_LAND_NUDGE, words::LAND_NUDGE))
    );
    // A need beside the defense: no landing; the nudge carries the ack.
    let at = at + 30 * MIN;
    s.observe_turn_end(
        ours(
            at,
            "Nothing left: merged and pushed.\nNEEDS YOU: the signing key password",
            10 * MIN,
        ),
        &t,
    );
    s.observe_mode(Mode::Land);
    let d = decide(&s, &p, &t, at, NO_JITTER);
    assert_eq!(
        typed(&d),
        Some((
            words::RULE_LAND_NUDGE,
            act_words(words::LAND_NUDGE, &[NEEDS_ACK]).as_str()
        ))
    );
}

/// THE MODE IS THE SESSION'S, folded before each decision: a landed task read under Cruise
/// goes on (Cruise takes nothing as the end: the landing's defense is a done claim, owed the
/// defend prompt at the point the landing kept). NEGATIVE CONTROL: back under Land it is a new
/// episode, its instruction first.
#[test]
fn a_landed_task_read_under_cruise_goes_on() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Stage 1 is in.", 5 * MIN), &t);
    run(&mut s, &land(), &t, now);
    s.observe_turn_end(ours(now + 20 * MIN, CLEAN, 10 * MIN), &t);
    assert_eq!(run(&mut s, &land(), &t, now + 20 * MIN).act, Act::Landed);
    let d = probed(&mut s, &policy(), &t, now + 21 * MIN, None);
    assert!(!s.landed);
    assert_eq!(typed(&d).map(|(r, _)| r), Some(words::RULE_DONE_CHECK));
    // NEGATIVE CONTROL: Land again, a new episode.
    let d = probed(&mut s, &land(), &t, now + 22 * MIN, None);
    assert_eq!(typed(&d), Some((words::RULE_LAND, words::LAND)));
}

/// THE CARRY-ON UNDER LAND types the land instruction, and its yield's clean defense lands.
/// NEGATIVE CONTROL: the same yield without the defense gets the nudge.
#[test]
fn the_carry_on_under_land_lands_its_yields_clean_defense() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_resume(true, Some("Working through stage 2."), true);
    let d = run(&mut s, &land(), &t, now);
    assert_eq!(typed(&d), Some((words::RULE_HOST_TURN, words::LAND)));
    assert!(s.land.typed && s.landing);
    let mut other = s.clone();
    s.observe_turn_end(ours(now + 20 * MIN, CLEAN, 10 * MIN), &t);
    assert_eq!(run(&mut s, &land(), &t, now + 20 * MIN).act, Act::Landed);
    other.observe_turn_end(ours(now + 20 * MIN, "Stage 2 is in.", 10 * MIN), &t);
    assert_eq!(
        typed(&run(&mut other, &land(), &t, now + 20 * MIN)),
        Some((words::RULE_LAND_NUDGE, words::LAND_NUDGE))
    );
}

/// NEEDS YOU NEVER BLOCKS (aterm owner direction 2026-10-19: "if you need something from the
/// user, be LOUDER ABOUT IT ... but NEVER let that block you"): a report that names a need
/// stands it (for the person's screen) and the point's own act goes on, carrying the ack once
/// per new set; a report silent on needs leaves the set; `NEEDS YOU: nothing` clears it.
/// NEGATIVE CONTROLS: the same set again carries no second ack; the question inside the NEEDS
/// YOU line is no question, while the same words outside one are (attention under
/// `questions=ask`).
#[test]
fn a_need_never_blocks_and_its_act_carries_the_ack_once() {
    let t = Timing::default();
    let now = Instant::now();
    let ask = Policy {
        questions: Questions::Ask,
        ..policy()
    };
    let need = "Stage 1 is in.\nNEEDS YOU: should I use the production signing key?";
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, need, 5 * MIN), &t);
    assert_eq!(
        s.needs.standing,
        ["should I use the production signing key?"]
    );
    assert!(s.needs.ack_owed());
    let d = run(&mut s, &ask, &t, now);
    assert_eq!(
        typed(&d),
        Some((
            words::RULE_CONTINUE,
            act_words("keep going", &[NEEDS_ACK]).as_str()
        ))
    );
    assert!(!s.needs.ack_owed());
    let mut at = now + 20 * MIN;
    s.observe_turn_end(ours(at, need, 10 * MIN), &t);
    assert_eq!(
        typed(&run(&mut s, &ask, &t, at)),
        Some((words::RULE_CONTINUE, "keep going"))
    );
    at += 20 * MIN;
    s.observe_turn_end(ours(at, "Stage 2 is in.", 10 * MIN), &t);
    assert_eq!(s.needs.standing.len(), 1, "a silent report leaves the set");
    run(&mut s, &ask, &t, at);
    at += 20 * MIN;
    s.observe_turn_end(
        ours(
            at,
            "NEEDS YOU: the signing key password\nNEEDS YOU: a DNS change",
            10 * MIN,
        ),
        &t,
    );
    assert_eq!(s.needs.standing.len(), 2);
    assert_eq!(
        typed(&run(&mut s, &ask, &t, at)),
        Some((
            words::RULE_CONTINUE,
            act_words("keep going", &[NEEDS_ACK]).as_str()
        ))
    );
    at += 20 * MIN;
    s.observe_turn_end(ours(at, "Stage 3 is in.\nNEEDS YOU: nothing", 10 * MIN), &t);
    assert_eq!(s.needs, Needs::default());
    // NEGATIVE CONTROL: the question outside a NEEDS YOU line.
    let mut s = ThreadState::default();
    s.observe_turn_end(
        end(now, "Should I use the production signing key?", 5 * MIN),
        &t,
    );
    assert!(matches!(
        decide(&s, &ask, &t, now, NO_JITTER).act,
        Act::Attention {
            key: "question",
            ..
        }
    ));
}

/// The needs are someone else's to answer: a person's turn clears them. A need that names an
/// irreversible act makes the act that acks it say to take none. A question's answer carries
/// the hint, then the ack. NEGATIVE CONTROL: once acked, the reversible clause goes with the
/// ack.
#[test]
fn needs_clear_at_a_persons_turn_and_an_irreversible_need_is_never_consented_to() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_turn_end(
        end(
            now,
            "Stage 1 is in.\nNEEDS YOU: approval to force-push main",
            5 * MIN,
        ),
        &t,
    );
    assert_eq!(
        typed(&run(&mut s, &policy(), &t, now)),
        Some((
            words::RULE_CONTINUE,
            act_words("keep going", &[words::DONE_CHECK_REVERSIBLE, NEEDS_ACK]).as_str()
        ))
    );
    let at = now + 20 * MIN;
    s.observe_turn_end(
        ours(
            at,
            "Stage 2 is in.\nNEEDS YOU: approval to force-push main",
            10 * MIN,
        ),
        &t,
    );
    assert_eq!(
        typed(&run(&mut s, &policy(), &t, at)),
        Some((words::RULE_CONTINUE, "keep going"))
    );
    s.observe_person_turn();
    s.observe_turn_end(end(at + MIN, "Looked at it.", 3 * MIN), &t);
    assert_eq!(s.needs, Needs::default());
    let mut s = ThreadState::default();
    s.observe_turn_end(
        end(
            now,
            "Should I rename the module?\nNEEDS YOU: the signing key password",
            5 * MIN,
        ),
        &t,
    );
    assert_eq!(
        typed(&run(&mut s, &policy(), &t, now)),
        Some((
            words::RULE_ANSWER,
            act_words("Pick the recommended option.", &[NEEDS_HINT, NEEDS_ACK]).as_str()
        ))
    );
}

/// A person's turn whose window keystroke aterm's `status human_ms=` read as their message
/// arrived (5 s before the turn's busy work), ended at `at` after `worked` under the Autopilot
/// word `word`, as the runtime sets it before the turn's end ([`ThreadState::ended_under`]:
/// `None` under Off).
fn by_hand(
    s: &mut ThreadState,
    t: &Timing,
    word: Option<Mode>,
    at: Instant,
    text: &str,
    worked: Duration,
) {
    s.ended_under = word;
    s.observe_person_turn();
    s.observe_person_hand(at - worked - Duration::from_secs(5));
    s.observe_turn_end(end(at, text, worked), t);
}

/// A person's turn their hand opened that met a server's wall after a minute's work, ended at
/// `at` under `word` ([`by_hand`]).
fn walled_by_hand(s: &mut ThreadState, t: &Timing, word: Option<Mode>, at: Instant) {
    s.ended_under = word;
    s.observe_person_turn();
    s.observe_person_hand(at - MIN - Duration::from_secs(5));
    s.observe_turn_end(
        TurnEnd {
            wall: Some(Wall::Server {
                what: "a 500".to_string(),
                cut_off: false,
                retry_at: None,
            }),
            text: None,
            ..end(at, "", MIN)
        },
        t,
    );
}

/// UNDER LAND A PERSON'S OWN EXCHANGE IS THEIRS ONLY WHERE IT OPENS THE SESSION (aterm
/// 791977cae8's `under_land_a_persons_message_mid_job_does_not_park_the_work`; the owner,
/// 2026-10-27: "you need to keep working even if the user interrupted a long running job. the
/// user question exception is only for the first message of a session"; aterm's
/// `left_to_person`, `EXCHANGE_WORK`, `PERSON_TURN_SLACK`): the session's first message, a
/// turn their window keystroke opened (aterm's `human_ms` inside the turn and after
/// codex-alab's last point and act) answered within ten minutes, gets nothing, whatever its
/// reply says, until the next turn ends, and a wall's retry that delivers it again keeps it
/// theirs while the answer's work in all stays within it. Once the session has worked (a point
/// this process judged, an Esc'd act, a landing), a person's quick message is a turn end like
/// any other, and Land carries the work on: its instruction, their turn having begun a new
/// episode. NEGATIVE CONTROLS: Off still leaves a message after work to them while it stands
/// (Land read after it carries that message: edges-4), and a retry whose answer took the work
/// past ten minutes is carried.
#[test]
fn under_land_only_the_sessions_opening_exchange_is_the_persons() {
    let t = Timing::default();
    let now = Instant::now();
    let p = land();
    // The session's opening message, answered in 3 min: nothing, a question in the reply
    // included.
    for reply in ["Here is the plan.", "Which file should I start with?"] {
        let mut s = ThreadState::default();
        s.observe_mode(Mode::Land);
        by_hand(&mut s, &t, Some(Mode::Land), now, reply, 3 * MIN);
        assert!(s.left, "{reply}");
        assert_eq!(
            probed(&mut s, &p, &t, now, None).act,
            Act::Nothing,
            "{reply}"
        );
    }
    // Land's act at `at`: its rule, and whether it types the instruction's words.
    let land_act = |s: &mut ThreadState, at: Instant| {
        typed(&probed(s, &p, &t, at, None)).map(|(rule, text)| (rule, text == words::LAND))
    };
    // After a point this process judged (the person's own job, before Land's act went): their
    // quick message is carried.
    let worked = || {
        let mut s = ThreadState::default();
        s.observe_turn_end(end(now, "Stage 1 is in.", 12 * MIN), &t);
        s
    };
    let mut s = worked();
    let at = now + 15 * MIN;
    by_hand(
        &mut s,
        &t,
        Some(Mode::Land),
        at,
        "Here is where it stands.",
        Duration::from_secs(30),
    );
    assert!(!s.left && !s.exchange);
    assert_eq!(land_act(&mut s, at), Some((words::RULE_LAND, true)));
    // After an Esc'd act of Land's: carried.
    let mut s = worked();
    assert_eq!(
        typed(&run(&mut s, &p, &t, now)),
        Some((words::RULE_LAND, words::LAND))
    );
    s.observe_turn_end(
        TurnEnd {
            aborted: true,
            ..ours(now + MIN, "", Duration::from_secs(30))
        },
        &t,
    );
    let at = now + 3 * MIN;
    by_hand(
        &mut s,
        &t,
        Some(Mode::Land),
        at,
        "Yes: stage 2 is half done.",
        Duration::from_secs(20),
    );
    assert!(!s.left);
    assert_eq!(land_act(&mut s, at), Some((words::RULE_LAND, true)));
    // After a landing: their thank-you is carried, Land's instruction of a new episode (where
    // aterm keeps its episode and nudges).
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Stage 1 is in.", 5 * MIN), &t);
    run(&mut s, &p, &t, now);
    s.observe_turn_end(ours(now + 20 * MIN, CLEAN, 10 * MIN), &t);
    assert_eq!(run(&mut s, &p, &t, now + 20 * MIN).act, Act::Landed);
    let at = now + 30 * MIN;
    by_hand(
        &mut s,
        &t,
        Some(Mode::Land),
        at,
        "Glad it helped.",
        Duration::from_secs(20),
    );
    assert!(!s.left && !s.landed);
    assert_eq!(land_act(&mut s, at), Some((words::RULE_LAND, true)));
    // NEGATIVE CONTROL: the same message after work under Off is theirs while the Off stands;
    // Land read after the Off carries it, as it carries one under Land (edges-4).
    let mut s = worked();
    let at = now + 15 * MIN;
    by_hand(
        &mut s,
        &t,
        None,
        at,
        "Here is where it stands.",
        Duration::from_secs(30),
    );
    assert!(s.left);
    assert_eq!(land_act(&mut s, at), Some((words::RULE_LAND, true)));
    assert!(!s.left);
    // A wall's retry delivers their opening exchange again: its answer is theirs while the
    // work in all stays within ten minutes (NEGATIVE CONTROL: past it, carried).
    for (answer, left) in [(2 * MIN, true), (10 * MIN, false)] {
        let mut s = ThreadState::default();
        walled_by_hand(&mut s, &t, Some(Mode::Land), now);
        assert!(s.exchange && !s.left, "{answer:?}");
        let d = run(&mut s, &p, &t, now);
        assert_eq!(typed(&d).map(|(r, _)| r), Some(words::RULE_API_RETRY));
        let at = now + 15 * MIN;
        s.observe_turn_end(ours(at, "Here is the plan.", answer), &t);
        assert_eq!(s.left, left, "{answer:?}");
        let d = probed(&mut s, &p, &t, at, None);
        assert_eq!(d.act == Act::Nothing, left, "{answer:?}");
    }
}

/// CRUISE NEVER PARKS (aterm 126e259836, the owner 2026-10-26): a person's own quick exchange
/// is continued as any turn end is, the session's opening message and one after a done claim
/// included, a question in the reply answered; a wall's retry of it is continued however short
/// its answer; and an exchange Land's rule or an Off left to the person is taken back at
/// Cruise's first decision (aterm's `cruise_takes_exchange`), whatever the word before.
/// NEGATIVE CONTROL: the same left points decided under Land stay theirs.
#[test]
fn cruise_never_leaves_a_persons_own_exchange() {
    let t = Timing::default();
    let now = Instant::now();
    let p = policy();
    // The session's opening message, answered in 3 min, decided with no mode folded: only
    // the turn's end can have left it.
    for (reply, rule) in [
        ("Here is the plan.", words::RULE_CONTINUE),
        ("Which file should I start with?", words::RULE_ANSWER),
    ] {
        let mut s = ThreadState::default();
        by_hand(&mut s, &t, Some(Mode::Cruise), now, reply, 3 * MIN);
        assert!(!s.left && !s.exchange, "{reply}");
        assert_eq!(
            typed(&decide(&s, &p, &t, now, NO_JITTER)).map(|(r, _)| r),
            Some(rule),
            "{reply}"
        );
    }
    // Once the worker said done, a person's short turn is continued too.
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Stage 1 is in.", 12 * MIN), &t);
    run(&mut s, &p, &t, now);
    s.observe_turn_end(ours(now + 20 * MIN, "All done, nothing left.", 5 * MIN), &t);
    assert!(s.claimed);
    let at = now + 40 * MIN;
    by_hand(
        &mut s,
        &t,
        Some(Mode::Cruise),
        at,
        "Thanks! The release notes are up.",
        MIN,
    );
    assert!(!s.left);
    assert_eq!(
        typed(&decide(&s, &p, &t, at, NO_JITTER)).map(|(r, _)| r),
        Some(words::RULE_CONTINUE)
    );
    // A wall's retry of their message: its answer is continued, however short.
    for answer in [2 * MIN, 10 * MIN] {
        let mut s = ThreadState::default();
        walled_by_hand(&mut s, &t, Some(Mode::Cruise), now);
        assert!(!s.exchange, "{answer:?}");
        let d = run(&mut s, &p, &t, now);
        assert_eq!(typed(&d).map(|(r, _)| r), Some(words::RULE_API_RETRY));
        let at = now + 15 * MIN;
        s.observe_turn_end(ours(at, "Here is the plan.", answer), &t);
        assert!(!s.left, "{answer:?}");
        assert_eq!(
            typed(&decide(&s, &p, &t, at, NO_JITTER)).map(|(r, _)| r),
            Some(words::RULE_CONTINUE),
            "{answer:?}"
        );
    }
    // CRUISE TAKES BACK what Land's rule left (the session's opening message) and what an Off
    // left (a message after work, the Off having ended the carry), the latter with Cruise the
    // word folded last, as an Off leaves it.
    let land_left = || {
        let mut s = ThreadState::default();
        s.observe_mode(Mode::Land);
        by_hand(
            &mut s,
            &t,
            Some(Mode::Land),
            now,
            "Here is the plan.",
            3 * MIN,
        );
        (s, now)
    };
    let off_left = || {
        let mut s = ThreadState::default();
        s.observe_turn_end(end(now, "Stage 1 is in.", 12 * MIN), &t);
        run(&mut s, &p, &t, now);
        s.observe_turn_end(ours(now + 20 * MIN, "Stage 2 is in.", 15 * MIN), &t);
        s.park_off();
        let at = now + 40 * MIN;
        by_hand(&mut s, &t, None, at, "Here is where it stands.", 3 * MIN);
        (s, at)
    };
    for (case, (s, at), opens) in [
        ("Land's opening", land_left(), true),
        ("an Off's", off_left(), false),
    ] {
        assert!(s.left, "{case}");
        // Under Land the session's opening exchange stays theirs (NEGATIVE CONTROL), and one an
        // Off left past the opening is Land's to carry (edges-4).
        let mut under_land = s.clone();
        let d = probed(&mut under_land, &land(), &t, at, None);
        if opens {
            assert_eq!(d.act, Act::Nothing, "{case}");
        } else {
            assert_eq!(typed(&d).map(|(r, _)| r), Some(words::RULE_LAND), "{case}");
        }
        assert_eq!(under_land.left, opens, "{case}");
        let mut s = s;
        let d = probed(&mut s, &p, &t, at, None);
        assert!(!s.left && !s.exchange, "{case}");
        assert_eq!(
            typed(&d).map(|(r, _)| r),
            Some(words::RULE_CONTINUE),
            "{case}"
        );
    }
}

/// A PERSON'S HOLD OVER AUTOPILOT OFF TAKES NOTHING BACK (review of gap 32, finding 9): the
/// hold keeps the claim and, in its policy, the word before it (Cruise here), while the word the
/// person chose is Off ([`ThreadState::observe_mode_under`]); an exchange the Off left stays
/// theirs at the look the hold makes, and Land read after it keeps it. NEGATIVE CONTROL: the
/// same look with Cruise the word the supervisor names takes it back.
#[test]
fn a_hold_over_off_takes_no_exchange_back() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_mode(Mode::Cruise);
    by_hand(&mut s, &t, None, now, "Here is where it stands.", 3 * MIN);
    assert!(s.left);
    let mut held = s.clone();
    held.observe_mode_under(Mode::Cruise, true);
    assert!(held.exchange && held.left);
    assert_eq!(probed(&mut held, &land(), &t, now, None).act, Act::Nothing);
    assert!(held.left);
    s.observe_mode_under(Mode::Cruise, false);
    assert!(!s.exchange && !s.left);
}

/// UNDER OFF A PERSON'S OWN EXCHANGE IS THEIRS AS BEFORE (no word the turn ended under,
/// [`ThreadState::ended_under`], as aterm's `Autopilot::Off`): their message, opened by their
/// hand, on a task codex-alab was not carrying (or whose worker had said done), answered within
/// ten minutes, is left to them at the session's opening and after work alike; a Land read
/// after the Off leaves the opening one to them until the next turn ends, and carries one after
/// work (edges-4: the owner 2026-10-27, "the user question exception is only for the first
/// message of a session"). NEGATIVE CONTROLS: a turn no
/// keystroke opened (a control write: `aterm ctl turn`), a keystroke before codex-alab's last
/// act, a job that worked past ten minutes, and a check-in on a task codex-alab carries are
/// each no exchange of theirs, and continued.
#[test]
fn off_leaves_a_persons_own_exchange_as_before() {
    let t = Timing::default();
    let now = Instant::now();
    let p = policy();
    let continued = |s: &ThreadState, at: Instant| {
        typed(&decide(s, &p, &t, at, NO_JITTER)).map(|(r, _)| r) == Some(words::RULE_CONTINUE)
    };
    // The session's opening message, answered in 3 min: theirs, a question in the reply
    // included.
    for reply in ["Here is the plan.", "Which file should I start with?"] {
        let mut s = ThreadState::default();
        by_hand(&mut s, &t, None, now, reply, 3 * MIN);
        assert!(s.left, "{reply}");
        assert_eq!(
            probed(&mut s, &land(), &t, now, None).act,
            Act::Nothing,
            "{reply}"
        );
    }
    // After work, the Off having ended the carry (`park_off`): theirs.
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Stage 1 is in.", 12 * MIN), &t);
    run(&mut s, &p, &t, now);
    s.observe_turn_end(ours(now + 20 * MIN, "Stage 2 is in.", 15 * MIN), &t);
    s.park_off();
    let at = now + 40 * MIN;
    by_hand(
        &mut s,
        &t,
        None,
        at,
        "Here is where it stands.",
        Duration::from_secs(30),
    );
    assert!(s.left);
    let d = probed(&mut s, &land(), &t, at, None);
    assert_eq!(typed(&d).map(|(r, _)| r), Some(words::RULE_LAND), "{d:?}");
    assert!(!s.left);
    // NEGATIVE CONTROL: the same turn by a control write (no keystroke read).
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(end(now, "Here is the plan.", 3 * MIN), &t);
    assert!(!s.left);
    assert!(continued(&s, now));
    // NEGATIVE CONTROL: a job past ten minutes.
    let mut s = ThreadState::default();
    by_hand(&mut s, &t, None, now, "Stage 1 is in.", 12 * MIN);
    assert!(!s.left);
    assert!(continued(&s, now));
    // A task codex-alab carries: a person's short check-in is no exchange of theirs (NEGATIVE
    // CONTROL), but once the worker said done, their short turn is theirs. A carry stands under
    // an Off only where the claim is kept (a person's hold over Off, which parks nothing): a
    // released Off's `park_off` ends it before their turn ends, as the case after work above
    // (review of gap 32, finding 13).
    let carried = || {
        let mut s = ThreadState::default();
        s.observe_turn_end(end(now, "Stage 1 is in.", 12 * MIN), &t);
        run(&mut s, &p, &t, now);
        s.observe_turn_end(ours(now + 20 * MIN, "Stage 2 is in.", 15 * MIN), &t);
        s
    };
    let mut s = carried();
    run(&mut s, &p, &t, now + 20 * MIN);
    let at = now + 40 * MIN;
    by_hand(
        &mut s,
        &t,
        None,
        at,
        "Going fine, carrying on.",
        Duration::from_secs(30),
    );
    assert!(!s.left);
    assert!(continued(&s, at));
    let mut s = carried();
    s.observe_turn_end(ours(now + 21 * MIN, "Everything is done.", 5 * MIN), &t);
    run(&mut s, &p, &t, now + 21 * MIN);
    s.observe_turn_end(ours(now + 30 * MIN, "All done, nothing left.", 5 * MIN), &t);
    let at = now + 40 * MIN;
    by_hand(
        &mut s,
        &t,
        None,
        at,
        "Thanks! The release notes are up.",
        MIN,
    );
    assert!(s.left);
    assert_eq!(decide(&s, &p, &t, at, NO_JITTER).act, Act::Nothing);
    // NEGATIVE CONTROL: a keystroke before codex-alab's last act opened nothing.
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Stage 1 is in.", 12 * MIN), &t);
    s.observe_person_turn();
    s.observe_person_hand(now + MIN);
    run(&mut s, &p, &t, now + 2 * MIN);
    s.observe_turn_end(
        TurnEnd {
            by_person: true,
            by_autopilot: false,
            ..ours(now + 3 * MIN, "Here is the plan.", MIN)
        },
        &t,
    );
    assert!(!s.left);
    assert!(continued(&s, now + 3 * MIN));
}

/// THE NEEDS CLEAR AT A PERSON'S TURN'S POINT, NEVER AT ITS START (aterm's `TurnEndState`:
/// "the person answered what stood" at the point of someone else's turn on someone's prompt
/// that met no wall, the reply then folded): they stay up while the person's turn runs, and
/// through one that ends at a wall (it answered nothing yet); its clean end clears them, and
/// what its reply names stands next. NEGATIVE CONTROL: the clean end of a person's turn whose
/// reply names a need again stands that set, owed its own ack.
#[test]
fn needs_clear_at_a_persons_turns_point_never_at_its_start() {
    let t = Timing::default();
    let now = Instant::now();
    let named = || {
        let mut s = ThreadState::default();
        s.observe_turn_end(
            end(now, "Stage 1 is in.\nNEEDS YOU: the prod API key", 5 * MIN),
            &t,
        );
        run(&mut s, &policy(), &t, now);
        assert_eq!(s.needs.standing, ["the prod API key"]);
        assert!(!s.needs.ack_owed());
        s
    };
    // The person's turn runs: the need stays up.
    let mut s = named();
    s.observe_person_turn();
    assert_eq!(s.needs.standing, ["the prod API key"]);
    // …and through its end at a wall.
    s.observe_turn_end(
        TurnEnd {
            wall: Some(Wall::Usage {
                reset_at: None,
                reset_label: None,
            }),
            text: None,
            ..end(now + MIN, "", Duration::from_secs(5))
        },
        &t,
    );
    assert_eq!(s.needs.standing, ["the prod API key"]);
    // Its clean end clears it.
    let mut s = named();
    s.observe_person_turn();
    s.observe_turn_end(end(now + MIN, "Used the key; stage 2 is in.", 5 * MIN), &t);
    assert_eq!(s.needs, Needs::default());
    // NEGATIVE CONTROL: a reply that names it again stands it, owed its own ack.
    let mut s = named();
    s.observe_person_turn();
    s.observe_turn_end(
        end(
            now + MIN,
            "That key was refused.\nNEEDS YOU: the prod API key",
            5 * MIN,
        ),
        &t,
    );
    assert_eq!(s.needs.standing, ["the prod API key"]);
    assert!(s.needs.ack_owed());
}

/// aterm's own texts win where its `harness policy` reply sends them (`done_check_text`,
/// `land_text`, ...). NEGATIVE CONTROL: the default texts are the verbatim copies.
#[test]
fn atermss_texts_are_typed_where_its_policy_sends_them() {
    let t = Timing::default();
    let now = Instant::now();
    let texts = Texts {
        done_check: "DEFEND IT".to_string(),
        land: "LAND IT".to_string(),
        ..Texts::default()
    };
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "All done, nothing left to do.", 5 * MIN), &t);
    let p = Policy {
        texts: texts.clone(),
        ..policy()
    };
    assert_eq!(
        typed(&probed(&mut s, &p, &t, now, None)),
        Some((words::RULE_DONE_CHECK, "DEFEND IT"))
    );
    let p = Policy { texts, ..land() };
    assert_eq!(
        typed(&probed(&mut s, &p, &t, now, None)),
        Some((words::RULE_LAND, "LAND IT"))
    );
    let d = Texts::default();
    assert_eq!(
        (d.done_check.as_str(), d.land.as_str(), d.needs_ack.as_str()),
        (words::DONE_CHECK, words::LAND, NEEDS_ACK)
    );
}

/// The act the old image's decision `d` owed under `mode` (its rule and words as decided),
/// as the live upgrade's record carries it to the new image.
fn owed_of(d: &Decision, mode: Mode) -> Owed {
    let (rule, text) = typed(d).unwrap_or_else(|| panic!("an act: {d:?}"));
    Owed {
        native_origin: None,
        rule,
        text: text.to_string(),
        mode,
        not_before: d.not_before.unwrap_or_else(|| panic!("paced: {d:?}")),
        repo_owed: d.repo_owed.clone(),
        resume_goal: false,
        landing: false,
    }
}

/// The new image's thread after the re-exec: resumed at `reply` with `owed` carried.
fn re_execed(reply: &str, owed: Option<Owed>, landed: bool, at: Instant) -> ThreadState {
    let mut s = ThreadState::default();
    s.observe_resume(true, Some(reply), false);
    s.observe_owed(owed, landed, at);
    s
}

/// THE ACT THE PREVIOUS IMAGE OWED AT ITS RE-EXEC (the live upgrade under FOREVER CRUISE: a
/// paced act no longer holds it) is typed by this image at its time, AS THE OLD IMAGE DECIDED
/// IT: its rule and its words, its wait shown until then; a landing it carried stays landed
/// under Land. NEGATIVE CONTROLS: a resume that owes nothing types nothing (codex-alab's own re-exec is no
/// relaunch); an owed act on a taskless thread is none.
#[test]
fn the_act_owed_at_the_re_exec_is_typed_at_its_time() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_resume(true, Some("Stage 2 is in."), false);
    assert_eq!(decide(&s, &policy(), &t, now, NO_JITTER).act, Act::Nothing);
    let owed = Owed {
        native_origin: None,
        rule: words::RULE_CONTINUE,
        text: "keep going, as decided".to_string(),
        mode: Mode::Cruise,
        not_before: now + 4 * MIN,
        repo_owed: None,
        resume_goal: false,
        landing: false,
    };
    s.observe_owed(Some(owed), false, now);
    // Its wait shows, though the old image's words for it went with that image (review of
    // f134d2b2cb, finding 7: a back-off carried across showed no Waiting row).
    let d = shows_types_again(
        || decide(&s, &policy(), &t, now, NO_JITTER),
        words::OWED_ACT,
        now + 4 * MIN,
        now,
    );
    assert_eq!(
        typed(&d),
        Some((words::RULE_CONTINUE, "keep going, as decided"))
    );
    assert_eq!(
        decide(&s, &policy(), &t, now + 4 * MIN, NO_JITTER).shown_wait,
        None,
        "due, it waits for nothing"
    );
    s.acted(&d.act, now + 4 * MIN, &t);
    assert_eq!(s.owed, None);
    // A carried landing stays landed under Land.
    let mut s = re_execed(CLEAN, None, true, now);
    assert!(s.landed);
    assert_eq!(probed(&mut s, &land(), &t, now, None).act, Act::Nothing);
    // NEGATIVE CONTROL: taskless.
    let mut s = ThreadState::default();
    s.observe_resume(false, Some("Hello."), false);
    s.observe_owed(
        Some(Owed {
            native_origin: None,
            rule: words::RULE_CONTINUE,
            text: "keep going".to_string(),
            mode: Mode::Cruise,
            not_before: now,
            repo_owed: None,
            resume_goal: false,
            landing: false,
        }),
        false,
        now,
    );
    assert_eq!(decide(&s, &policy(), &t, now, NO_JITTER).act, Act::Nothing);
}

/// AN OWED ANSWER STAYS THE ANSWER THE OLD IMAGE DECIDED (review of 2a9241bbf, findings 1, 8
/// and 16): an irreversible question's reversible answer, decided at a short turn's end and
/// paced, crosses the re-exec as that answer, never the bare continuation the resumed reply
/// alone would give (which reads as consent to the force-push, D1); under Land the land
/// answer ("never force-push or delete work") stays the land answer, never the nudge; and an
/// answer to a question only the old image saw (the turn's structured `questions`) stays an
/// answer. NEGATIVE CONTROL: the same reply read with no owed act is a question again.
#[test]
fn an_owed_answer_stays_the_answer_the_old_image_decided() {
    let t = Timing::default();
    let now = Instant::now();
    let ask = "I can drop the bad commit. Should I force-push main to remove it?";
    for (p, rule, guard) in [
        (policy(), words::RULE_ANSWER, words::REVERSIBLE_ANSWER),
        (land(), words::RULE_LAND_ANSWER, words::LAND_ANSWER),
    ] {
        let mut old = ThreadState::default();
        old.observe_turn_end(end(now, "Start the release.", 5 * MIN), &t);
        run(&mut old, &p, &t, now);
        old.observe_turn_end(ours(now + MIN, ask, Duration::from_secs(30)), &t);
        let d = probed(&mut old, &p, &t, now + MIN, None);
        assert_eq!(typed(&d).map(|(r, _)| r), Some(rule));
        let owed = owed_of(&d, p.autopilot);
        let at = owed.not_before;
        let mut new = re_execed(ask, Some(owed), false, now + 2 * MIN);
        let d = probed(&mut new, &p, &t, at, None);
        let (typed_rule, text) = typed(&d).unwrap_or_else(|| panic!("an act: {d:?}"));
        assert_eq!(typed_rule, rule);
        assert!(text.starts_with(guard), "{text}");
        new.acted(&d.act, at, &t);
        // NEGATIVE CONTROL: no owed act, the reply decided at its point again.
        let mut fresh = ThreadState::default();
        fresh.observe_turn_end(ours(at, ask, 5 * MIN), &t);
        fresh.task = true;
        assert_eq!(
            typed(&probed(&mut fresh, &p, &t, at, None)).map(|(r, _)| r),
            Some(rule)
        );
    }
    // A question only the old image saw (the turn's structured `questions`), in words that ask
    // nothing: the reply alone, decided again, would be continued.
    let p = policy();
    let shown = "Here are the two layouts.";
    let mut old = ThreadState::default();
    old.observe_turn_end(end(now, "Pick a layout.", 5 * MIN), &t);
    run(&mut old, &p, &t, now);
    old.observe_turn_end(
        TurnEnd {
            questions: true,
            ..ours(now + MIN, shown, Duration::from_secs(30))
        },
        &t,
    );
    let d = probed(&mut old, &p, &t, now + MIN, None);
    assert_eq!(typed(&d).map(|(r, _)| r), Some(words::RULE_ANSWER));
    let owed = owed_of(&d, Mode::Cruise);
    let at = owed.not_before;
    let mut new = re_execed(shown, Some(owed), false, now + 2 * MIN);
    assert_eq!(
        typed(&probed(&mut new, &p, &t, at, None)).map(|(r, _)| r),
        Some(words::RULE_ANSWER)
    );
}

/// A NUDGE OWED OVER A DEFENSE NO LAND ACT ASKED FOR NEVER LANDS IT (review of 2a9241bbf,
/// finding 1c): a turn that was neither codex-alab's nor a person's (a goal's continuation)
/// ends on a clean defense; the old image owes the nudge, not a landing, and the new image
/// types that nudge at its time; the nudge's own yield then lands a clean defense. NEGATIVE
/// CONTROL: a defense at the yield of a land act lands.
#[test]
fn a_nudge_owed_over_a_defense_no_land_act_asked_for_never_lands_it() {
    let t = Timing::default();
    let now = Instant::now();
    let mut old = ThreadState::default();
    old.observe_turn_end(end(now, "Land the parser.", 5 * MIN), &t);
    run(&mut old, &land(), &t, now);
    old.observe_turn_end(
        ours(
            now + 20 * MIN,
            "Merged the parser; the lexer next.",
            10 * MIN,
        ),
        &t,
    );
    let d = run(&mut old, &land(), &t, now + 20 * MIN);
    assert_eq!(typed(&d).map(|(r, _)| r), Some(words::RULE_LAND_NUDGE));
    let goal = TurnEnd {
        by_autopilot: false,
        by_person: false,
        ..ours(now + 40 * MIN, CLEAN, Duration::from_secs(40))
    };
    old.observe_turn_end(goal, &t);
    let d = probed(&mut old, &land(), &t, now + 40 * MIN, None);
    assert_eq!(typed(&d).map(|(r, _)| r), Some(words::RULE_LAND_NUDGE));
    let owed = owed_of(&d, Mode::Land);
    let at = owed.not_before;
    let mut new = re_execed(CLEAN, Some(owed), false, now + 41 * MIN);
    let d = probed(&mut new, &land(), &t, at, None);
    assert_eq!(typed(&d), Some((words::RULE_LAND_NUDGE, words::LAND_NUDGE)));
    new.acted(&d.act, at, &t);
    assert!(!new.landed && new.landing);
    // NEGATIVE CONTROL: the nudge's own yield lands its clean defense.
    new.observe_turn_end(ours(at + 20 * MIN, CLEAN, 10 * MIN), &t);
    assert_eq!(
        probed(&mut new, &land(), &t, at + 20 * MIN, None).act,
        Act::Landed
    );
}

/// AN OWED ACT WAITS FOR ITS TIME BEFORE THE REPOSITORY IS READ, as a paced act at a turn end
/// is probed only once due: Cruise's defend prompt, decided without the repository's suffix,
/// reads it when it goes and names what it holds. NEGATIVE CONTROLS: before its time no
/// probe runs; a clean repository adds nothing; aterm's relaunch carry-on, which owes no time,
/// probes at once.
#[test]
fn an_owed_act_waits_for_its_time_before_the_probe() {
    let t = Timing::default();
    let now = Instant::now();
    let repo = words::own_repo_suffix(&work().summary());
    let claim = "All done, nothing left to do.";
    let mut old = ThreadState::default();
    old.observe_turn_end(end(now, "Stage 1.", 5 * MIN), &t);
    run(&mut old, &policy(), &t, now);
    old.observe_turn_end(ours(now + MIN, claim, 5 * MIN), &t);
    let d = probed(&mut old, &policy(), &t, now + MIN, None);
    assert_eq!(typed(&d), Some((words::RULE_DONE_CHECK, words::DONE_CHECK)));
    let owed = owed_of(&d, Mode::Cruise);
    let at = owed.not_before;
    let mut s = re_execed(claim, Some(owed.clone()), false, now + MIN);
    s.observe_mode(Mode::Cruise);
    let d = decide(&s, &policy(), &t, now + MIN, NO_JITTER);
    assert_eq!(typed(&d), Some((words::RULE_DONE_CHECK, words::DONE_CHECK)));
    assert_eq!(d.not_before, Some(at));
    assert_eq!(decide(&s, &policy(), &t, at, NO_JITTER).act, Act::Probe);
    s.probed(Some(work()));
    let d = decide(&s, &policy(), &t, at, NO_JITTER);
    assert_eq!(
        typed(&d),
        Some((
            words::RULE_DONE_CHECK,
            act_words(words::DONE_CHECK, &[&repo]).as_str()
        ))
    );
    assert_eq!(d.not_before, None);
    // NEGATIVE CONTROL: a clean repository.
    let mut s = re_execed(claim, Some(owed), false, now + MIN);
    assert_eq!(
        typed(&probed(&mut s, &policy(), &t, at, None)),
        Some((words::RULE_DONE_CHECK, words::DONE_CHECK))
    );
    // NEGATIVE CONTROL: aterm's relaunch owes no time: the probe at once.
    let mut s = ThreadState::default();
    s.observe_resume(true, Some(claim), true);
    assert_eq!(decide(&s, &policy(), &t, now, NO_JITTER).act, Act::Probe);
}

/// A DONE CLAIM'S ACT DECIDED BEFORE THE PROBE PLACES THE REPOSITORY'S SUFFIX AS THE OLD IMAGE
/// WOULD HAVE (aterm reads the repository at every done claim when its act goes, its
/// `repo_read`): the live upgrade carries the act a done claim owes while it waits on the
/// back-off, decided before the repository was read, with the open work the claim named; the
/// new image probes once it is due and types exactly what the old image would have typed then.
/// Land's nudge as well as Cruise's prompt; where the act is short of room the open words are
/// named fewer before the repository's suffix gives way, as one fitting of them all (aterm's
/// `fitted_act`); a second re-exec before the time carries it on so. NEGATIVE CONTROL: a nudge
/// decided over the probe's finding (a landed task's wake over someone else's work, which no
/// act names) is typed as decided, never read again by a new image that no longer knows
/// whose the work is.
#[test]
fn an_owed_act_decided_before_the_probe_places_the_suffix_as_the_old_image_would() {
    let t = Timing::default();
    let now = Instant::now();
    let big = RepoWork {
        repo: format!(
            "/homes/a/Developer/{}",
            "a-rather-long-checkout-name-".repeat(3)
        ),
        changed: 41,
        untracked: 17,
        stashes: 3,
        unpushed: 9,
        paths: (0..12)
            .map(|i| format!("crates/a-long-crate-name-{i}/src/a_rather_long_module_{i}.rs"))
            .collect(),
        ..Default::default()
    };
    let tight = "All done.\nStill open: the lexer.\nLeft: the docs.\nRemaining work: the e2e \
                 suite.\nNext steps: port the reader.\nTODO: wire the flag.\nNEEDS YOU: approve \
                 the force-push to main";
    let rules = "keep each commit small and name the stage it lands ".repeat(8);
    let rules = rules.trim_end();
    let mut cases = vec![
        (
            land(),
            "Nothing left: all merged, but the lexer is not landed.",
            work(),
            None,
        ),
        (
            policy(),
            "Committed the parser. Left: the lexer.\nATERM-TASK-DONE",
            work(),
            None,
        ),
    ];
    for p in [policy(), land()] {
        for rules in [None, Some(rules)] {
            cases.push((p.clone(), tight, big.clone(), rules));
        }
    }
    for (p, claim, found, rules) in cases {
        let mut p = p;
        let composed = |text: &str| match rules {
            Some(r) => act_words(&format!("{text} (standing rules: {r})"), &[]),
            None => text.to_string(),
        };
        p.texts.done_check = composed(words::DONE_CHECK);
        p.texts.land = composed(words::LAND);
        p.texts.land_nudge = composed(words::LAND_NUDGE);
        let what = format!("{:?} rules={} {claim:?}", p.autopilot, rules.is_some());
        let mut old = ThreadState::default();
        old.observe_turn_end(end(now, "Stage 1 landed.", 5 * MIN), &t);
        run(&mut old, &p, &t, now);
        let at = now + 20 * MIN;
        old.observe_turn_end(ours(at, claim, 10 * MIN), &t);
        old.observe_mode(p.autopilot);
        let d = decide(&old, &p, &t, at, NO_JITTER);
        assert!(
            d.not_before.is_some_and(|due| due > at),
            "{what}: the claim's act is paced: {d:?}"
        );
        let owed = owed_of(&d, p.autopilot);
        let due = owed.not_before;
        // What the old image types once due, the repository read then.
        let want = typed(&probed(&mut old, &p, &t, due, Some(found.clone())))
            .map(|(rule, text)| (rule, text.to_string()))
            .unwrap_or_else(|| panic!("{what}: an act"));
        // Land's nudge asks all of it, Cruise's check the agent's own work, in the old image
        // and the new alike (aterm's `RepoAsk`).
        let ask = match p.autopilot {
            Mode::Land => "commit, merge and push it to the remote main first",
            Mode::Cruise => "land what of it you made first",
        };
        assert!(
            want.1.contains(ask),
            "{what}: the repository's ask: {}",
            want.1
        );
        let mut new = re_execed(claim, Some(owed.clone()), false, at + MIN);
        let got = typed(&probed(&mut new, &p, &t, due, Some(found.clone())))
            .map(|(rule, text)| (rule, text.to_string()));
        assert_eq!(got.as_ref(), Some(&want), "{what}");
        // A second re-exec before the time carries it on.
        let mut new = re_execed(claim, Some(owed), false, at + MIN);
        new.observe_mode(p.autopilot);
        let d = decide(&new, &p, &t, at + MIN, NO_JITTER);
        assert_eq!(d.not_before, Some(due), "{what}");
        let mut newer = re_execed(claim, Some(owed_of(&d, p.autopilot)), false, at + 2 * MIN);
        let got = typed(&probed(&mut newer, &p, &t, due, Some(found)))
            .map(|(rule, text)| (rule, text.to_string()));
        assert_eq!(got.as_ref(), Some(&want), "{what}: carried on");
    }
    // NEGATIVE CONTROL: a nudge decided over the probe's finding.
    let p = land();
    let theirs = stated(7);
    let claim = "Nothing left: all merged, but the lexer is not landed.";
    let wake_at = now + 60 * MIN;
    let mut old = landed_at(now, &t);
    old.wake_began_over(Some(theirs.clone()));
    old.observe_turn_end(wake(wake_at, claim), &t);
    let d = probed(&mut old, &p, &t, wake_at, Some(theirs.clone()));
    assert!(!old.landed, "the wake left work open: {old:?}");
    assert!(d.not_before.is_some_and(|due| due > wake_at), "{d:?}");
    let decided = typed(&d).map(|(rule, text)| (rule, text.to_string()));
    assert_eq!(
        decided.as_ref().map(|(rule, _)| *rule),
        Some(words::RULE_LAND_NUDGE)
    );
    let owed = owed_of(&d, Mode::Land);
    let due = owed.not_before;
    let want = typed(&probed(&mut old, &p, &t, due, Some(theirs)))
        .map(|(rule, text)| (rule, text.to_string()));
    assert_eq!(want, decided);
    assert!(
        want.as_ref()
            .is_some_and(|(_, text)| !text.contains("commit, merge and push")),
        "{want:?}"
    );
    let mut new = re_execed(claim, Some(owed), false, wake_at + MIN);
    new.observe_mode(Mode::Land);
    let got =
        typed(&decide(&new, &p, &t, due, NO_JITTER)).map(|(rule, text)| (rule, text.to_string()));
    assert_eq!(got, want);
}

/// THE PACE A RELAUNCH TAKES UP BACKS OFF AS THE PROCESS BEFORE WOULD HAVE (aterm's
/// `seed_short` and `seed_retry`: a loop takes up its predecessor's streak and ladder): the
/// short streak the record carries is lengthened by the yield of the act the dead process
/// owed (the next act waits 8 min where that process would have, not the first rung's 2), and
/// a server wall met again after a carry-on goes on the rung the dead process was on, counted
/// from the appearance the carry-on's turn met: the dead process's try still owed its answer
/// (aterm's `seed_retry` with `owed`). NEGATIVE CONTROLS: unseeded, the relaunch backs off
/// from the first rung (the 120 s of the parity review's S2) and retries the wall a minute
/// after it; a record from a build that carried neither the appearance nor the answer owed
/// counts from the last try, as that build did; a compaction's attempt is no ladder a process
/// carries; a memory with a streak of its own keeps it.
#[test]
fn a_pace_carried_across_a_relaunch_backs_off_as_the_process_before_would() {
    let t = Timing::default();
    let now = Instant::now();
    let p = policy();
    let claim = "All done, nothing left to do.";
    let mut old = ThreadState::default();
    old.observe_turn_end(end(now, "Stage 1.", 5 * MIN), &t);
    run(&mut old, &p, &t, now);
    let t1 = now + MIN;
    old.observe_turn_end(ours(t1, "Looking.", Duration::from_secs(10)), &t);
    let d = decide(&old, &p, &t, t1, NO_JITTER);
    old.acted(&d.act, t1 + 2 * MIN, &t);
    let t2 = t1 + 3 * MIN;
    old.observe_turn_end(ours(t2, claim, 5 * MIN), &t);
    let d = probed(&mut old, &p, &t, t2, None);
    assert_eq!(d.not_before, Some(t2 + 4 * MIN), "{d:?}");
    let owed = owed_of(&d, Mode::Cruise);
    let due = owed.not_before;
    let pace = (old.short_streak, old.wall.clone());
    assert_eq!(pace, (2, None));
    // The process before, had it lived: the owed act's short yield, then 8 min.
    let yielded = due + MIN;
    let after = |s: &mut ThreadState| {
        let d = probed(s, &p, &t, due, None);
        assert_eq!(
            typed(&d).map(|(rule, _)| rule),
            Some(words::RULE_DONE_CHECK)
        );
        s.acted(&d.act, due, &t);
        s.observe_turn_end(ours(yielded, "Looked again.", Duration::from_secs(10)), &t);
        decide(s, &p, &t, yielded, NO_JITTER).not_before
    };
    let want = after(&mut old);
    assert_eq!(want, Some(yielded + 8 * MIN));
    let relaunched = |seeded: bool| {
        let mut s = ThreadState::default();
        s.observe_resume(true, Some(claim), false);
        if seeded {
            s.seed_pace(pace.0, pace.1.clone());
        }
        s.observe_owed(Some(owed.clone()), false, t2 + MIN);
        s
    };
    assert_eq!(after(&mut relaunched(true)), want);
    // NEGATIVE CONTROL: unseeded, the first rung.
    assert_eq!(after(&mut relaunched(false)), Some(yielded + 2 * MIN));
    // THE LADDER: two retries typed, the second's turn cut by the crash.
    let server = || Wall::Server {
        what: "an API error (500)".to_string(),
        cut_off: false,
        retry_at: None,
    };
    let mut old = ThreadState::default();
    old.observe_turn_end(end(now, "Ship it.", 5 * MIN), &t);
    run(&mut old, &p, &t, now);
    let mut at = now + MIN;
    for _ in 0..2 {
        old.observe_turn_end(walled(at, server()), &t);
        let d = decide(&old, &p, &t, at, NO_JITTER);
        let go = d.not_before.unwrap_or(at);
        old.acted(&d.act, go, &t);
        at = go + MIN;
    }
    let last_try = old.wall.as_ref().and_then(|w| w.last_try);
    assert_eq!(old.wall.as_ref().map(|w| w.attempts), Some(2));
    assert_eq!(old.wall.as_ref().map(|w| w.answer_owed), Some(true));
    let pace = (old.short_streak, old.wall.clone());
    let met = at + 3 * MIN;
    let mut lived = old.clone();
    lived.observe_turn_end(walled(met, server()), &t);
    let want = decide(&lived, &p, &t, met, NO_JITTER).not_before;
    assert_eq!(want, Some(met + 15 * MIN));
    let carried_on = |wall: Option<crate::decide::WallTrack>| {
        let mut s = ThreadState::default();
        s.observe_resume(true, Some("Shipping."), true);
        s.seed_pace(pace.0, wall);
        let d = probed(&mut s, &p, &t, at, None);
        assert_eq!(typed(&d).map(|(rule, _)| rule), Some(words::RULE_HOST_TURN));
        s.acted(&d.act, at, &t);
        s.observe_turn_end(walled(met, server()), &t);
        decide(&s, &p, &t, met, NO_JITTER).not_before
    };
    assert_eq!(carried_on(pace.1.clone()), want);
    // NEGATIVE CONTROL: unseeded, the ladder's first rung from the wall.
    assert_eq!(carried_on(None), Some(met + MIN));
    // NEGATIVE CONTROL: a record from before the appearance was carried counts from the try.
    let legacy = pace.1.as_ref().and_then(|w| {
        crate::decide::WallTrack::carried(
            "server",
            w.first_seen,
            w.attempts,
            w.last_try,
            None,
            false,
        )
    });
    assert_eq!(carried_on(legacy), last_try.map(|at| at + 15 * MIN));
    assert_ne!(last_try.map(|at| at + 15 * MIN), want);
    // NEGATIVE CONTROL: a compaction's attempt is carried by no record.
    let mut compacted = ThreadState::default();
    compacted.observe_turn_end(end(now, "Go.", 5 * MIN), &t);
    compacted.observe_turn_end(walled(now + MIN, Wall::Context { auto_compact: true }), &t);
    let context = compacted.wall.clone();
    assert!(context.is_some());
    assert_eq!(
        context
            .as_ref()
            .and_then(crate::decide::WallTrack::carried_class),
        None
    );
    let mut s = ThreadState::default();
    s.seed_pace(0, context);
    assert_eq!(s.wall, None);
    assert_eq!(
        crate::decide::WallTrack::carried("context", now, 1, None, None, false),
        None
    );
    // NEGATIVE CONTROL: a memory with a streak of its own keeps it.
    let mut s = ThreadState {
        short_streak: 1,
        ..ThreadState::default()
    };
    s.seed_pace(3, None);
    assert_eq!(s.short_streak, 1);
}

/// A LANDING CARRIED ACROSS THE RE-EXEC KEEPS ITS POINT (review of 2a9241bbf, findings 2,
/// 10 and 19): switched to Cruise, it goes on as the same switch without the re-exec does (the
/// defense is a done claim, owed the defend prompt); and an act the old image owed under the
/// other mode is decided again at its point under the new one. NEGATIVE CONTROL: under Land
/// the carried landing types nothing.
#[test]
fn a_carried_landing_or_an_owed_act_of_the_other_mode_is_decided_at_its_point() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = re_execed(CLEAN, None, true, now);
    assert_eq!(probed(&mut s, &land(), &t, now, None).act, Act::Nothing);
    let d = probed(&mut s, &policy(), &t, now + MIN, None);
    assert!(!s.landed);
    assert_eq!(typed(&d), Some((words::RULE_DONE_CHECK, words::DONE_CHECK)));
    // An owed nudge read under Cruise: the reply decided again (a report of progress).
    let owed = Owed {
        native_origin: None,
        rule: words::RULE_LAND_NUDGE,
        text: words::LAND_NUDGE.to_string(),
        mode: Mode::Land,
        not_before: now + 8 * MIN,
        repo_owed: None,
        resume_goal: false,
        landing: false,
    };
    let mut s = re_execed("Merged the parser.", Some(owed.clone()), false, now);
    let d = probed(&mut s, &policy(), &t, now + MIN, None);
    assert_eq!(s.owed, None);
    assert_eq!(typed(&d), Some((words::RULE_CONTINUE, "keep going")));
    // NEGATIVE CONTROL: the same owed act under its own mode.
    let mut s = re_execed("Merged the parser.", Some(owed), false, now);
    let d = probed(&mut s, &land(), &t, now + MIN, None);
    assert_eq!(typed(&d), Some((words::RULE_LAND_NUDGE, words::LAND_NUDGE)));
    assert_eq!(d.not_before, Some(now + 8 * MIN));
}

/// A PROBE ANSWERS ONLY THE POINT IT WAS BEGUN AT: the probe takes up to its limit, and a turn
/// that ends meanwhile is a point of its own, probed again there (never answered with what
/// the repository held before that turn). NEGATIVE CONTROL: the finding for the point in hand
/// is the one the defend prompt names.
#[test]
fn a_probe_answers_only_the_point_it_began_at() {
    let t = Timing::default();
    let now = Instant::now();
    let p = policy();
    let mut s = ThreadState::default();
    s.observe_mode(Mode::Cruise);
    s.observe_turn_end(end(now, "All done, nothing left to do.", 5 * MIN), &t);
    assert_eq!(decide(&s, &p, &t, now, NO_JITTER).act, Act::Probe);
    let begun = s.point_at();
    // A person's turn ends while the probe runs.
    let at = now + Duration::from_secs(8);
    s.observe_turn_end(end(at, "Nothing left to do: the docs are in.", 5 * MIN), &t);
    s.probed_for(begun, Some(work()));
    assert_eq!(decide(&s, &p, &t, at, NO_JITTER).act, Act::Probe);
    s.probed_for(s.point_at(), None);
    assert_eq!(
        typed(&decide(&s, &p, &t, at, NO_JITTER)),
        Some((words::RULE_DONE_CHECK, words::DONE_CHECK))
    );
    // NEGATIVE CONTROL: found for the point in hand, the prompt names it.
    s.probed_for(s.point_at(), Some(work()));
    let repo = words::own_repo_suffix(&work().summary());
    assert_eq!(
        typed(&decide(&s, &p, &t, at, NO_JITTER)),
        Some((
            words::RULE_DONE_CHECK,
            act_words(words::DONE_CHECK, &[&repo]).as_str()
        ))
    );
}

/// The rules whose wait the live upgrade carries: the continuation-type acts and the
/// carry-on (`paced_rule`), and with them a wall's try (`carried_rule`: a retry, a cut-off's
/// retry, a limit's resume). NEGATIVE CONTROLS: a wall's try is no paced act; a compaction,
/// the landing, the retired finish and an unknown word are neither.
#[test]
fn only_a_paced_acts_or_a_walls_tries_rule_is_carried() {
    use crate::decide::carried_rule;
    use crate::decide::paced_rule;
    for rule in [
        words::RULE_API_RETRY,
        words::RULE_API_CUTOFF,
        words::RULE_LIMIT_RESUME,
    ] {
        assert_eq!(carried_rule(rule), Some(rule));
        assert_eq!(paced_rule(rule), None);
    }
    for name in [
        words::RULE_COMPACT,
        words::RULE_LANDED,
        words::RULE_FINISHED,
        "keep going",
        "",
    ] {
        assert_eq!(carried_rule(name), None, "{name}");
    }
    for rule in [
        words::RULE_CONTINUE,
        words::RULE_ANSWER,
        words::RULE_PROSE_LONG_TERM,
        words::RULE_PROSE_SHORT_TERM,
        words::RULE_DONE_CHECK,
        words::RULE_LAND,
        words::RULE_LAND_NUDGE,
        words::RULE_LAND_ANSWER,
        words::RULE_HOST_TURN,
    ] {
        assert_eq!(paced_rule(rule), Some(rule));
        assert_eq!(carried_rule(rule), Some(rule));
    }
    for name in [
        words::RULE_API_RETRY,
        words::RULE_LIMIT_RESUME,
        words::RULE_COMPACT,
        words::RULE_LANDED,
        words::RULE_FINISHED,
        "keep going",
        "",
    ] {
        assert_eq!(paced_rule(name), None, "{name}");
    }
}

/// NO POINT ENDS A CRUISE TASK, and Land lands exactly where every condition holds: a free
/// point of a task (no wall, no compaction), a land act's yield, codex-alab's own turn, a clean
/// defense, and a clean repository. Swept over texts, walls, compactions, taskless threads
/// and land episodes; both verdicts are met.
#[test]
fn landed_only_where_every_condition_holds() {
    let t = Timing::default();
    let now = Instant::now();
    let texts = [
        CLEAN,
        "All landed.\nATERM-TASK-DONE",
        "Nothing left: merged, but the docs are not pushed.",
        "Nothing left: merged.\nNEEDS YOU: the credential",
        "Stage 2 is half done; continuing next.",
    ];
    let mut seen = [0_u32; 2];
    let b = [false, true];
    for text in texts {
        for mode in [Mode::Cruise, Mode::Land] {
            for (task, compacting, after_compact, wall, landing, by_autopilot) in b
                .iter()
                .flat_map(|a| b.iter().map(move |c| (*a, *c)))
                .flat_map(|(a, c)| b.iter().map(move |d| (a, c, *d)))
                .flat_map(|(a, c, d)| b.iter().map(move |e| (a, c, d, *e)))
                .flat_map(|(a, c, d, e)| b.iter().map(move |f| (a, c, d, e, *f)))
                .flat_map(|(a, c, d, e, f)| b.iter().map(move |g| (a, c, d, e, f, *g)))
            {
                let mut s = ThreadState {
                    task,
                    compacting,
                    after_compact,
                    landing,
                    mode: Some(mode),
                    ..ThreadState::default()
                };
                s.land.typed = landing;
                let point = TurnEnd {
                    by_autopilot,
                    by_person: !by_autopilot,
                    ..ours(now, text, 10 * MIN)
                };
                s.point = Some(if wall {
                    TurnEnd {
                        wall: Some(Wall::Server {
                            what: "500".to_string(),
                            cut_off: false,
                            retry_at: None,
                        }),
                        ..point
                    }
                } else {
                    point
                });
                let p = Policy {
                    autopilot: mode,
                    ..policy()
                };
                let lands = probed(&mut s, &p, &t, now, None).act == Act::Landed;
                let want = mode == Mode::Land
                    && task
                    && !compacting
                    && !after_compact
                    && !wall
                    && landing
                    && by_autopilot
                    && words::clean_defense(Some(text));
                assert_eq!(lands, want, "{text:?} {s:?}");
                seen[usize::from(lands)] += 1;
            }
        }
    }
    assert!(
        seen[0] > 0 && seen[1] > 0,
        "both verdicts were met: {seen:?}"
    );
}

/// LAND'S WALLS DELIVER THE LAND ACT AGAIN (review of 2a9241bbf, finding 20): an API retry of
/// the land instruction keeps it a land act, so its yield's clean defense lands; a usage
/// window's resume after a person's turn types the land instruction (never `continue_text`)
/// and opens the episode, so its yield lands too; a compaction's continuation types the land
/// words and keeps the landing. NEGATIVE CONTROLS: the same yields under Cruise get the defend
/// prompt, and a report of progress at the retry's yield gets the nudge.
#[test]
fn land_walls_deliver_the_land_act_again_and_their_yields_land() {
    let t = Timing::default();
    let now = Instant::now();
    let server = || Wall::Server {
        what: "an API error (500)".to_string(),
        cut_off: false,
        retry_at: None,
    };
    // The land instruction's turn hits a server error: the retry, then a clean defense.
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Stage 1 is in.", 5 * MIN), &t);
    assert_eq!(
        typed(&run(&mut s, &land(), &t, now)).map(|(r, _)| r),
        Some(words::RULE_LAND)
    );
    s.observe_turn_end(walled(now + MIN, server()), &t);
    let d = probed(&mut s, &land(), &t, now + MIN, None);
    assert_eq!(typed(&d).map(|(r, _)| r), Some(words::RULE_API_RETRY));
    let at = d.not_before.unwrap_or(now + 2 * MIN);
    s.acted(&d.act, at, &t);
    assert!(s.landing, "a retry delivers the land act again");
    let mut progress = s.clone();
    s.observe_turn_end(ours(at + 20 * MIN, CLEAN, 10 * MIN), &t);
    assert_eq!(
        probed(&mut s, &land(), &t, at + 20 * MIN, None).act,
        Act::Landed
    );
    // NEGATIVE CONTROL: a report of progress at the retry's yield gets the nudge.
    progress.observe_turn_end(ours(at + 20 * MIN, "Stage 2 is in.", 10 * MIN), &t);
    assert_eq!(
        typed(&probed(&mut progress, &land(), &t, at + 20 * MIN, None)),
        Some((words::RULE_LAND_NUDGE, words::LAND_NUDGE))
    );
    // A person's turn ends at a usage window: the resume types the land instruction.
    let reset = now + 30 * MIN;
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(
        TurnEnd {
            by_autopilot: false,
            by_person: true,
            ..walled(
                now,
                Wall::Usage {
                    reset_at: Some(reset),
                    reset_label: None,
                },
            )
        },
        &t,
    );
    let d = probed(&mut s, &land(), &t, now, None);
    assert_eq!(
        d.act,
        Act::ResumeLimit {
            text: words::LAND.to_string()
        }
    );
    let at = d.not_before.unwrap_or(reset);
    s.acted(&d.act, at, &t);
    assert!(s.land.typed && s.landing);
    let mut cruise = s.clone();
    s.observe_turn_end(ours(at + 20 * MIN, CLEAN, 10 * MIN), &t);
    assert_eq!(
        probed(&mut s, &land(), &t, at + 20 * MIN, None).act,
        Act::Landed
    );
    // NEGATIVE CONTROL: the same yield read under Cruise is a done claim.
    cruise.observe_turn_end(ours(at + 20 * MIN, CLEAN, 10 * MIN), &t);
    assert_eq!(
        typed(&probed(&mut cruise, &policy(), &t, at + 20 * MIN, None)).map(|(r, _)| r),
        Some(words::RULE_DONE_CHECK)
    );
    // A compaction under Land: its continuation types the land act the full context cut
    // again (aterm's `compact_owed_text`), and lands after.
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Stage 1 is in.", 5 * MIN), &t);
    run(&mut s, &land(), &t, now);
    s.observe_turn_end(walled(now + MIN, Wall::Context { auto_compact: true }), &t);
    assert_eq!(run(&mut s, &land(), &t, now + MIN).act, Act::Compact);
    s.observe_turn_end(ours(now + 2 * MIN, "", Duration::from_secs(20)), &t);
    let d = run(&mut s, &land(), &t, now + 2 * MIN);
    assert_eq!(typed(&d), Some((words::RULE_COMPACT, words::LAND)));
    assert!(s.landing);
    s.observe_turn_end(ours(now + 30 * MIN, CLEAN, 10 * MIN), &t);
    assert_eq!(
        probed(&mut s, &land(), &t, now + 30 * MIN, None).act,
        Act::Landed
    );
}

/// THE QUOTA WALL UNDER LAND (recheck of 15dbc93c4, finding 6): a person's turn that ends at
/// the account's quota or plan (`Wall::Entitlement`, no reset to wait for) is tried again on
/// the entitlement back-off under `usage-resume@v1` with the land instruction, never
/// `continue_text`; the try opens the episode and is a land act, so a clean defense at its
/// yield lands once the repository is clean, and the nudge naming the repository's work comes
/// where it is not. NEGATIVE CONTROL: the same wall under Cruise types the blind cleared line
/// (aterm `77161d6432`; it typed `continue_text` before), opens no episode and lands nothing:
/// the same defense at its yield gets the defend prompt.
#[test]
fn a_quota_wall_under_land_types_the_land_act_and_its_yield_lands() {
    let t = Timing::default();
    let now = Instant::now();
    let at_quota = |s: &mut ThreadState| {
        s.observe_person_turn();
        s.observe_turn_end(
            TurnEnd {
                by_autopilot: false,
                by_person: true,
                ..walled(now, Wall::Entitlement)
            },
            &t,
        );
    };
    let mut s = ThreadState::default();
    at_quota(&mut s);
    let d = probed(&mut s, &land(), &t, now, None);
    assert_eq!(typed(&d), Some((words::RULE_LIMIT_RESUME, words::LAND)));
    assert_eq!(
        d.not_before,
        Some(now + 30 * MIN),
        "the entitlement back-off"
    );
    let at = now + 30 * MIN;
    s.acted(&d.act, at, &t);
    assert!(s.land.typed && s.landing, "{s:?}");
    s.observe_turn_end(ours(at + 20 * MIN, CLEAN, 10 * MIN), &t);
    let mut dirty = s.clone();
    assert_eq!(run(&mut s, &land(), &t, at + 20 * MIN).act, Act::Landed);
    assert!(s.landed);
    // The repository still holds work: the nudge names it, and nothing lands.
    let d = probed(&mut dirty, &land(), &t, at + 20 * MIN, Some(work()));
    assert_eq!(typed(&d).map(|(r, _)| r), Some(words::RULE_LAND_NUDGE));
    assert!(!dirty.landed);
    // NEGATIVE CONTROL: the same wall under Cruise.
    let mut s = ThreadState::default();
    at_quota(&mut s);
    let d = probed(&mut s, &policy(), &t, now, None);
    assert_eq!(
        typed(&d),
        Some((words::RULE_LIMIT_RESUME, words::LIMIT_MAY_HAVE_CLEARED))
    );
    s.acted(&d.act, at, &t);
    assert!(!s.land.typed && !s.landing, "{s:?}");
    s.observe_turn_end(ours(at + 20 * MIN, CLEAN, 10 * MIN), &t);
    let d = probed(&mut s, &policy(), &t, at + 20 * MIN, None);
    assert_eq!(typed(&d).map(|(r, _)| r), Some(words::RULE_DONE_CHECK));
    assert!(!s.landed);
}

/// CRUISE PROBES WHEN THE ACT IS DUE (review of 2a9241bbf, finding 24): a done claim inside
/// the back-off waits with the defend prompt's words and no probe, so the suffix typed later
/// names what the repository holds then; at its time the probe runs. NEGATIVE CONTROL: a
/// claim with no wait probes at once.
#[test]
fn cruise_probes_the_repository_only_when_the_act_is_due() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_mode(Mode::Cruise);
    s.observe_turn_end(end(now, "Stage 1 is in.", 5 * MIN), &t);
    run(&mut s, &policy(), &t, now);
    let at = now + MIN;
    s.observe_turn_end(ours(at, "All done, nothing left to do.", 5 * MIN), &t);
    let d = decide(&s, &policy(), &t, at, NO_JITTER);
    assert_eq!(typed(&d), Some((words::RULE_DONE_CHECK, words::DONE_CHECK)));
    assert_eq!(d.not_before, Some(at + 2 * MIN));
    assert_eq!(
        decide(&s, &policy(), &t, at + 2 * MIN, NO_JITTER).act,
        Act::Probe
    );
    // NEGATIVE CONTROL: no wait, the probe at once.
    let mut s = ThreadState::default();
    s.observe_mode(Mode::Cruise);
    s.observe_turn_end(end(now, "All done, nothing left to do.", 5 * MIN), &t);
    assert_eq!(decide(&s, &policy(), &t, now, NO_JITTER).act, Act::Probe);
}

/// THE TEXTS ATERM'S `harness policy` REPLY SENDS ARE THE ONES TYPED (decision 6; review of
/// 2a9241bbf, finding 25), read off the reply as the supervisor reads it: Land's instruction,
/// nudge and answer, Cruise's defend prompt, the hint after an answer and the ack. NEGATIVE
/// CONTROL: a key the reply leaves blank (or out) is typed as codex-alab's verbatim copy.
#[test]
fn the_texts_atermss_policy_reply_sends_are_typed() {
    let t = Timing::default();
    let now = Instant::now();
    let codex_alab_aterm::policy::PolicyAnswer::Supported(reply) =
        codex_alab_aterm::policy::PolicyAnswer::parse(
            "OK gen=1 effective=on why=enabled continue=true continue_text=keep%20going \
             answer_questions=true questions=recommended answer_text=Pick%20one. \
             done_check_text=DEFEND%20IT land_text=LAND%20IT land_nudge_text=NUDGE%20IT \
             land_answer_text=ANSWER%20IT needs_hint_text=HINT needs_ack_text=%20 \
             continue_per_hour=0",
        )
    else {
        panic!("a supported reply");
    };
    let cruise = Policy::from_harness(&reply);
    let land = Policy {
        autopilot: Mode::Land,
        ..cruise.clone()
    };
    let need = "Stage 1 is in.\nNEEDS YOU: the signing key password";
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, need, 5 * MIN), &t);
    assert_eq!(
        typed(&run(&mut s, &land, &t, now)),
        Some((
            words::RULE_LAND,
            act_words("LAND IT", &[NEEDS_ACK]).as_str()
        ))
    );
    s.observe_turn_end(ours(now + 20 * MIN, "Stage 2 is in.", 10 * MIN), &t);
    assert_eq!(
        typed(&run(&mut s, &land, &t, now + 20 * MIN)),
        Some((words::RULE_LAND_NUDGE, "NUDGE IT"))
    );
    s.observe_turn_end(
        ours(now + 40 * MIN, "Should I rename the module?", 10 * MIN),
        &t,
    );
    assert_eq!(
        typed(&run(&mut s, &land, &t, now + 40 * MIN)),
        Some((words::RULE_LAND_ANSWER, "ANSWER IT"))
    );
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "All done, nothing left to do.", 5 * MIN), &t);
    assert_eq!(
        typed(&run(&mut s, &cruise, &t, now)),
        Some((words::RULE_DONE_CHECK, "DEFEND IT"))
    );
    s.observe_turn_end(
        ours(now + 20 * MIN, "Should I rename the module?", 10 * MIN),
        &t,
    );
    assert_eq!(
        typed(&run(&mut s, &cruise, &t, now + 20 * MIN)),
        Some((words::RULE_ANSWER, "Pick one. HINT"))
    );
}

/// THE NEEDS' OWN GUARDS (review of 2a9241bbf, findings 15 and 21): a wall's retry carries no
/// ack and acks nothing, so the next act of the point carries it; a `Nothing left:` defense
/// that names no need clears what stood; a turn that is neither a person's nor codex-alab's
/// (a goal's own continuation) answers no need, and its report folds as any. NEGATIVE
/// CONTROLS: a person's turn clears the set; a typed act of the point acks it.
#[test]
fn the_needs_guards_hold() {
    let t = Timing::default();
    let now = Instant::now();
    let named = needs_of("NEEDS YOU: the signing key password");
    // A wall's retry with an ack owed.
    let mut s = ThreadState {
        task: true,
        needs: named.clone(),
        ..ThreadState::default()
    };
    s.observe_turn_end(
        walled(
            now,
            Wall::Server {
                what: "an API error (500)".to_string(),
                cut_off: false,
                retry_at: None,
            },
        ),
        &t,
    );
    let d = probed(&mut s, &policy(), &t, now, None);
    let (rule, text) = typed(&d).unwrap_or_else(|| panic!("the retry: {d:?}"));
    assert_eq!(rule, words::RULE_API_RETRY);
    assert!(!text.contains(NEEDS_ACK), "{text}");
    s.acted(&d.act, now + MIN, &t);
    assert!(s.needs.ack_owed(), "a retry acks nothing");
    s.observe_turn_end(ours(now + 20 * MIN, "Stage 2 is in.", 10 * MIN), &t);
    assert_eq!(
        typed(&run(&mut s, &policy(), &t, now + 20 * MIN)),
        Some((
            words::RULE_CONTINUE,
            act_words("keep going", &[NEEDS_ACK]).as_str()
        ))
    );
    assert!(!s.needs.ack_owed(), "the point's act acked it");
    // A defense that names no need clears the set.
    s.observe_turn_end(ours(now + 40 * MIN, CLEAN, 10 * MIN), &t);
    assert_eq!(s.needs, Needs::default());
    // A goal's continuation answers nothing; its silent report leaves the set.
    let mut s = ThreadState {
        task: true,
        needs: named.clone(),
        ..ThreadState::default()
    };
    let goal = TurnEnd {
        by_autopilot: false,
        by_person: false,
        ..ours(now, "Continuing with the migration tests.", 5 * MIN)
    };
    s.observe_turn_end(goal, &t);
    // The set stands, on the menu bar whole; the reply last read named no need (§15.6).
    assert_eq!(
        s.needs,
        Needs {
            named: false,
            ..named.clone()
        }
    );
    assert_eq!(s.needs.shown(), named.standing);
    // NEGATIVE CONTROL: a person's turn answered it.
    s.observe_turn_end(end(now + MIN, "Here is the password.", 5 * MIN), &t);
    assert_eq!(s.needs, Needs::default());
}

/// The needs a report `text` names, as the decider folds them.
fn needs_of(text: &str) -> Needs {
    let mut needs = Needs::default();
    needs.fold(Some(text));
    needs
}

/// AN OFF PARKS THE THREAD'S MEMORY (review of 2a9241bbf, findings 4 and 12; aterm's
/// `park_off`): the needs it took off the person's screen are cleared, so a later report that
/// names none raises nothing again, and the Land episode ends (Land read after it starts a new
/// one). NEGATIVE CONTROL: a landed task stays landed.
#[test]
fn an_off_parks_the_needs_and_the_episode_and_keeps_the_landing() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_turn_end(
        end(
            now,
            "Stage 1.\nNEEDS YOU: the signing key password",
            5 * MIN,
        ),
        &t,
    );
    run(&mut s, &land(), &t, now);
    assert!(s.land.typed && s.landing && s.needs.key != 0);
    s.park_off();
    assert_eq!(s.needs, Needs::default());
    assert!(!s.land.typed && !s.landing);
    s.observe_turn_end(ours(now + 20 * MIN, "Stage 2 is in.", 10 * MIN), &t);
    assert_eq!(
        s.needs,
        Needs::default(),
        "a silent report raises nothing again"
    );
    assert_eq!(
        typed(&run(&mut s, &land(), &t, now + 20 * MIN)),
        Some((words::RULE_LAND, words::LAND))
    );
    // NEGATIVE CONTROL: a landing survives the park.
    s.observe_turn_end(ours(now + 40 * MIN, CLEAN, 10 * MIN), &t);
    assert_eq!(run(&mut s, &land(), &t, now + 40 * MIN).act, Act::Landed);
    s.park_off();
    assert!(s.landed);
    assert_eq!(
        probed(&mut s, &land(), &t, now + 41 * MIN, None).act,
        Act::Nothing
    );
}

/// THE NEEDS A RESUMED HISTORY LEAVES STANDING ARE RESTORED after aterm's relaunch or the
/// re-exec (review of 2a9241bbf, findings 11 and 18): the next act carries the ack where the
/// history did not ack them. NEGATIVE CONTROLS: an acked set carries no second ack; a taskless
/// thread restores nothing.
#[test]
fn the_needs_a_resumed_history_leaves_are_restored() {
    let t = Timing::default();
    let now = Instant::now();
    let named = needs_of("NEEDS YOU: the prod API key");
    let mut s = ThreadState::default();
    s.observe_resume(true, Some("Widened the tests."), true);
    s.restore_needs(named.clone());
    assert_eq!(
        typed(&decide(&s, &policy(), &t, now, NO_JITTER)),
        Some((
            words::RULE_HOST_TURN,
            act_words("keep going", &[NEEDS_ACK]).as_str()
        ))
    );
    let mut acked = named.clone();
    acked.acked = Some(acked.key);
    let mut s = ThreadState::default();
    s.observe_resume(true, Some("Widened the tests."), true);
    s.restore_needs(acked);
    assert_eq!(
        typed(&decide(&s, &policy(), &t, now, NO_JITTER)),
        Some((words::RULE_HOST_TURN, "keep going"))
    );
    let mut s = ThreadState::default();
    s.observe_resume(false, Some("Hello."), true);
    s.restore_needs(named);
    assert_eq!(s.needs, Needs::default());
}

/// A QUESTION BESIDE A DONE CLAIM IS NO QUESTION (CONTRACT's prose-questions row, scoped
/// after the ninth review of the strict reading, finding 23): a done claim is routed first,
/// as aterm's `land_policy` routes it, so under Land a reply that claims done and asks gets
/// the instruction or a nudge, never the land answer, and under Cruise the defend prompt,
/// never `answer_text`. NEGATIVE CONTROL: the same question with no done claim gets the
/// answer under either word.
#[test]
fn a_question_beside_a_done_claim_gets_the_done_act() {
    let t = Timing::default();
    let now = Instant::now();
    let both = "Everything is done. Should I rename the module?";
    let plain = "Should I rename the module?";
    assert!(words::says_done(Some(both)) && !words::says_done(Some(plain)));
    let at = |p: &Policy, text: &str| {
        let mut s = ThreadState::default();
        s.observe_turn_end(end(now, "Start the release.", 5 * MIN), &t);
        run(&mut s, p, &t, now);
        s.observe_turn_end(ours(now + MIN, "Stage 1 is in.", 10 * MIN), &t);
        run(&mut s, p, &t, now + MIN);
        s.observe_turn_end(ours(now + 2 * MIN, text, 10 * MIN), &t);
        typed(&probed(&mut s, p, &t, now + 2 * MIN, None)).map(|(rule, _)| rule)
    };
    assert_eq!(at(&land(), both), Some(words::RULE_LAND_NUDGE));
    assert_eq!(at(&policy(), both), Some(words::RULE_DONE_CHECK));
    // NEGATIVE CONTROL: no done claim, the question is answered.
    assert_eq!(at(&land(), plain), Some(words::RULE_LAND_ANSWER));
    assert_eq!(at(&policy(), plain), Some(words::RULE_ANSWER));
}

/// LAND ANSWERS EVERY QUESTION WITH THE LAND ANSWER AND THE ACK ONLY (CONTRACT §5 prose
/// questions, amended 2026-10-20; aterm's `land_policy`): one naming an irreversible act
/// included (the land answer itself says never to force-push or delete), whatever the
/// `questions` word, never with Cruise's reversible answer or the NEEDS YOU hint; the ack
/// rides along while a new set of needs stands unacked. NEGATIVE CONTROL: Cruise answers an
/// irreversible question with the reversible answer and the hint, a plain one with the
/// policy's answer and the hint, and leaves one to a person under `questions=ask`.
#[test]
fn land_answers_every_question_with_the_land_answer_and_the_ack_only() {
    let t = Timing::default();
    let now = Instant::now();
    let force = "I can drop the bad commit. Should I force-push main to remove it?";
    let plain = "Should I rename the module?";
    let asked = |p: &Policy, before: &str, text: &str| {
        let mut s = ThreadState::default();
        s.observe_turn_end(end(now, "Start the release.", 5 * MIN), &t);
        run(&mut s, p, &t, now);
        s.observe_turn_end(ours(now + MIN, before, 10 * MIN), &t);
        run(&mut s, p, &t, now + MIN);
        s.observe_turn_end(ours(now + 2 * MIN, text, 10 * MIN), &t);
        probed(&mut s, p, &t, now + 2 * MIN, None)
    };
    let hint = policy().texts.needs_hint;
    let needs_ack = policy().texts.needs_ack;
    for questions in [Questions::Recommended, Questions::Ask] {
        let p = Policy {
            questions,
            ..land()
        };
        for text in [force, plain] {
            let d = asked(&p, "Stage 1 is in.", text);
            assert_eq!(
                typed(&d),
                Some((words::RULE_LAND_ANSWER, words::LAND_ANSWER)),
                "{questions:?}: {text}"
            );
        }
        // A need named beside the question: the ack rides along, nothing else.
        let needing = format!("{force}\nNEEDS YOU: the signing key password");
        let d = asked(&p, "Stage 1 is in.", &needing);
        let (rule, said) = typed(&d).unwrap_or_else(|| panic!("an act: {d:?}"));
        assert_eq!(rule, words::RULE_LAND_ANSWER);
        assert_eq!(said, words::act_words(words::LAND_ANSWER, &[&needs_ack]));
        assert!(!said.contains(&hint), "{said}");
    }
    // NEGATIVE CONTROL: Cruise.
    let (rule, said) = typed(&asked(&policy(), "Stage 1 is in.", force))
        .map(|(r, s)| (r, s.to_string()))
        .unwrap_or_else(|| panic!("an answer"));
    assert_eq!(rule, words::RULE_ANSWER);
    assert!(
        said.starts_with(words::REVERSIBLE_ANSWER) && said.contains(&hint),
        "{said}"
    );
    let (rule, said) = typed(&asked(&policy(), "Stage 1 is in.", plain))
        .map(|(r, s)| (r, s.to_string()))
        .unwrap_or_else(|| panic!("an answer"));
    assert_eq!(rule, words::RULE_ANSWER);
    assert!(
        said.starts_with("Pick the recommended option.") && said.contains(&hint),
        "{said}"
    );
    let ask = Policy {
        questions: Questions::Ask,
        ..policy()
    };
    assert!(matches!(
        asked(&ask, "Stage 1 is in.", plain).act,
        Act::Attention { .. }
    ));
}

/// A task landed under Land at `now` (the instruction typed at a person's turn, its yield's
/// clean defense over a clean repository): `Act::Landed`, its point the defense.
fn landed_at(now: Instant, t: &Timing) -> ThreadState {
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Ship the parser.", 5 * MIN), t);
    assert_eq!(
        typed(&run(&mut s, &land(), t, now)),
        Some((words::RULE_LAND, words::LAND))
    );
    s.observe_turn_end(ours(now + 20 * MIN, CLEAN, 10 * MIN), t);
    assert_eq!(run(&mut s, &land(), t, now + 20 * MIN).act, Act::Landed);
    assert!(s.landed && !s.judging());
    s
}

/// THE LANDED TASK'S OWN WAKE: a turn nobody asked (no `UserMessage` opened it, neither a
/// person's nor codex-alab's: a goal's continuation), replied `text`.
fn wake(at: Instant, text: &str) -> TurnEnd {
    TurnEnd {
        by_autopilot: false,
        by_person: false,
        ..end(at, text, Duration::from_secs(20))
    }
}

/// What the repository probe finds where the work's state was read (`RepoWork::same` compares
/// two findings only where both read it): `state` tells two findings of the same counts apart.
fn stated(state: u64) -> RepoWork {
    RepoWork {
        state: Some(state),
        ..work()
    }
}

const QUIET: &str = "The monitor fired: the deploy finished.";

/// [`probed`] at the end of a landed task's own wake whose judgement keeps the landing: aterm
/// reopened it at the wake's turn, so its `landed@v1` is recorded again, once (review of gap
/// 12, finding 1), and carried out at `now`; what is decided after it.
fn kept(
    s: &mut ThreadState,
    p: &Policy,
    t: &Timing,
    now: Instant,
    found: Option<RepoWork>,
) -> Decision {
    let d = probed(s, p, t, now, found);
    assert_eq!(d.act, Act::Landed, "recorded again: {d:?}");
    assert!(s.landed && s.reland && !s.judging(), "{s:?}");
    s.acted(&d.act, now, t);
    assert!(s.landed && !s.reland, "{s:?}");
    decide(s, p, t, now, NO_JITTER)
}

/// A LANDED TASK'S OWN WAKE KEEPS THE LANDING (aterm main e0da8d68df's `wake_keeps`, NOBODY'S
/// ACT): a turn nobody asked that ends on any reply, on a server error or at a usage window,
/// over a clean repository, its words naming nothing open, leaves the task landed; the
/// decider probes first, records the kept landing again for aterm (which reopened it at the
/// wake's turn: review of gap 12, finding 1), and nothing is typed then or a day later past
/// the reset. The build before reopened every such landing, and a nudge or the wall's ladder
/// typed into landed work. NEGATIVE CONTROL: the same turn opened by a person (a person's act)
/// reopens the task, and records no landing.
#[test]
fn a_landed_tasks_own_wake_keeps_the_landing() {
    let t = Timing::default();
    let now = Instant::now();
    let p = land();
    let wake_at = now + 60 * MIN;
    let usage = Wall::Usage {
        reset_at: Some(wake_at + 60 * MIN),
        reset_label: None,
    };
    let overloaded = Wall::Server {
        what: "an API error (529)".to_string(),
        cut_off: false,
        retry_at: None,
    };
    for (what, wall) in [
        ("a quiet reply", None),
        ("a server error", Some(overloaded)),
        ("a usage window", Some(usage)),
    ] {
        let mut s = landed_at(now, &t);
        assert!(s.wants_start_probe(), "{what}");
        s.observe_turn_end(
            TurnEnd {
                wall: wall.clone(),
                ..wake(wake_at, QUIET)
            },
            &t,
        );
        assert!(s.judging(), "{what}");
        s.observe_mode(p.autopilot);
        assert_eq!(
            decide(&s, &p, &t, wake_at, NO_JITTER).act,
            Act::Probe,
            "{what}"
        );
        assert_eq!(
            kept(&mut s, &p, &t, wake_at, None).act,
            Act::Nothing,
            "{what}"
        );
        assert!(s.landed && !s.judging(), "{what}");
        assert_eq!(
            decide(&s, &p, &t, wake_at + 24 * 60 * MIN, NO_JITTER).act,
            Act::Nothing,
            "{what}: a day later"
        );
        // NEGATIVE CONTROL: a person's turn.
        let mut s = landed_at(now, &t);
        s.observe_turn_end(
            TurnEnd {
                wall,
                by_person: true,
                ..wake(wake_at, QUIET)
            },
            &t,
        );
        assert!(
            !s.landed && !s.judging() && !s.reland,
            "{what}: a person's turn"
        );
        let d = probed(&mut s, &p, &t, wake_at + 24 * 60 * MIN, None);
        assert!(
            matches!(d.act, Act::Type { .. } | Act::ResumeLimit { .. }),
            "{what}: a person's turn: {d:?}"
        );
    }
}

/// A LANDED TASK'S OWN WAKE OVER OPEN WORK REOPENS IT (aterm main e0da8d68df; Land's fourth
/// condition gates a landing whoever's turn it was): a wake ending over work the repository
/// holds that no remote main has, or naming open work or a need in its last words or an
/// earlier message of the turn ([`crate::decide::wake_left_open`]), is no longer landed, and
/// Land's nudge comes, naming the repository's work. NEGATIVE CONTROL: the same wake over a
/// clean repository, its words naming nothing open, stays landed: nothing typed.
#[test]
fn a_landed_tasks_own_wake_over_open_work_gets_the_nudge() {
    let t = Timing::default();
    let now = Instant::now();
    let p = land();
    let wake_at = now + 60 * MIN;
    let repo = words::repo_suffix(&work().summary());
    for (why, ended, found, want) in [
        (
            "a dirty repository",
            wake(wake_at, QUIET),
            Some(work()),
            act_words(words::LAND_NUDGE, &[&repo]),
        ),
        (
            "open words",
            wake(
                wake_at,
                "The monitor fired: the deploy failed, and the fix is still open.",
            ),
            None,
            words::LAND_NUDGE.to_string(),
        ),
        (
            "a need",
            wake(
                wake_at,
                "The monitor fired: the release needs signing.\nNEEDS YOU: the signing key password",
            ),
            None,
            act_words(words::LAND_NUDGE, &[NEEDS_ACK]),
        ),
        (
            "a need said earlier in the wake",
            TurnEnd {
                earlier: Some("NEEDS YOU: the signing key password".to_string()),
                ..wake(wake_at, QUIET)
            },
            None,
            act_words(words::LAND_NUDGE, &[NEEDS_ACK]),
        ),
    ] {
        let mut s = landed_at(now, &t);
        s.observe_turn_end(ended, &t);
        let d = probed(&mut s, &p, &t, wake_at, found);
        assert!(!s.landed && !s.judging(), "{why}: {d:?}");
        assert_eq!(
            typed(&d),
            Some((words::RULE_LAND_NUDGE, want.as_str())),
            "{why}"
        );
    }
    // NEGATIVE CONTROL.
    let mut s = landed_at(now, &t);
    s.observe_turn_end(wake(wake_at, QUIET), &t);
    assert_eq!(kept(&mut s, &p, &t, wake_at, None).act, Act::Nothing);
    assert!(s.landed);
}

/// A LANDED TASK'S OWN WAKE OVER SOMEONE ELSE'S EDITS KEEPS THE LANDING (aterm main
/// e0da8d68df's `wake_began_over`): the work the start probe found as the wake began, found
/// unchanged at its end (the work's state read at both ends), is none of the wake's, so the
/// landing holds and nothing is typed. aterm's recheck of ea564676a3 found the landing
/// reopened there and the worker told to commit the person's edits. NEGATIVE CONTROLS: work
/// that appeared during the wake (the repository clean as it began), work the wake changed
/// (another state), a state neither probe could read, a wake whose start was never probed,
/// and an Off since its start (the person had the session in hand, so the start forgives
/// nothing) reopen it.
#[test]
fn a_landed_tasks_own_wake_over_someone_elses_edits_keeps_the_landing() {
    let t = Timing::default();
    let now = Instant::now();
    let p = land();
    let wake_at = now + 60 * MIN;
    let theirs = stated(7);
    let mut s = landed_at(now, &t);
    s.wake_began_over(Some(theirs.clone()));
    s.observe_turn_end(wake(wake_at, QUIET), &t);
    assert_eq!(
        kept(&mut s, &p, &t, wake_at, Some(theirs.clone())).act,
        Act::Nothing
    );
    assert!(s.landed && !s.judging());
    // NEGATIVE CONTROLS.
    for (why, began, ended, off) in [
        (
            "appeared during the wake",
            Some(None),
            theirs.clone(),
            false,
        ),
        (
            "changed by the wake",
            Some(Some(theirs.clone())),
            stated(8),
            false,
        ),
        ("an unread state", Some(Some(work())), work(), false),
        ("never probed", None, theirs.clone(), false),
        (
            "an Off since its start",
            Some(Some(theirs.clone())),
            theirs,
            true,
        ),
    ] {
        let mut s = landed_at(now, &t);
        if let Some(began) = began {
            s.wake_began_over(began);
        }
        if off {
            s.park_off();
            assert!(!s.wants_start_probe(), "{why}");
        }
        s.observe_turn_end(wake(wake_at, QUIET), &t);
        let d = probed(&mut s, &p, &t, wake_at, Some(ended));
        assert!(!s.landed, "{why}: {d:?}");
        let want = if off {
            words::RULE_LAND
        } else {
            words::RULE_LAND_NUDGE
        };
        assert_eq!(typed(&d).map(|(r, _)| r), Some(want), "{why}: {d:?}");
    }
}

/// A WAKE THAT REOPENS A LANDING OVER SOMEONE ELSE'S EDIT NAMES NONE OF IT (aterm main
/// e0da8d68df's `repo_theirs`): the wake began over the person's edit and ends over the same
/// one, but its words leave work open, so the landing is over and Land's nudge comes, for
/// the wake's words, never for the edit: the nudge names no repository work, so the worker is
/// not told to commit work it did not make; a wake whose words claim done beside a need is
/// read so too (aterm filters the point's finding, whatever the claim). NEGATIVE CONTROL: the
/// repository clean as the wake began, the same finding at its end is the wake's own, and the
/// nudge names it.
#[test]
fn a_wake_that_reopens_a_landing_over_someone_elses_edit_names_none_of_it() {
    let t = Timing::default();
    let now = Instant::now();
    let p = land();
    let wake_at = now + 60 * MIN;
    let theirs = stated(7);
    let open = "The monitor fired: the deploy finished. The docs fix is parked for later.";
    let claim = "The monitor fired: the deploy finished. All done, nothing left to do.\nNEEDS \
                 YOU: the signing key password";
    let repo = words::repo_suffix(&theirs.summary());
    for (why, said, began, named) in [
        ("over their edit", open, Some(theirs.clone()), false),
        (
            "a claim over their edit",
            claim,
            Some(theirs.clone()),
            false,
        ),
        ("work the wake made", open, None, true),
        ("a claim over work the wake made", claim, None, true),
    ] {
        let mut s = landed_at(now, &t);
        s.wake_began_over(began);
        s.observe_turn_end(wake(wake_at, said), &t);
        let d = probed(&mut s, &p, &t, wake_at, Some(theirs.clone()));
        assert!(!s.landed, "{why}: {d:?}");
        let Some((rule, text)) = typed(&d) else {
            panic!("{why}: {d:?}");
        };
        assert_eq!(rule, words::RULE_LAND_NUDGE, "{why}");
        assert_eq!(text.contains(repo.as_str()), named, "{why}: {text}");
    }
}

/// AN OFF AND BACK RE-ARMS LAND OVER THE REPOSITORY AS IT IS (aterm main e0da8d68df, a
/// PERSON'S ACT; its merge review's P6): once an Off stood over a landing
/// ([`ThreadState::park_off`]), this image's or one a re-exec carried, the landing is judged
/// again over a fresh probe before anything is decided over it, and over work found it is
/// over: Land's instruction again (the Off ended the episode). The build before held it on
/// the words alone, so an Off under which the person edited by hand came back landed over
/// their uncommitted work. NEGATIVE CONTROLS: with no Off (a re-exec, nobody's act) the
/// carried landing holds over the same work, no probe asked; after an Off over a clean
/// repository it holds.
#[test]
fn an_off_and_back_re_arms_land_over_the_repository_as_it_is() {
    let t = Timing::default();
    let now = Instant::now();
    let p = land();
    let back = now + 40 * MIN;
    let live = || landed_at(now, &Timing::default());
    let carried = || re_execed(CLEAN, None, true, now);
    for (what, landing) in [
        ("this image's landing", &live as &dyn Fn() -> ThreadState),
        ("a landing the re-exec carried", &carried),
    ] {
        let mut s = landing();
        s.park_off();
        assert!(s.landed && s.judging(), "{what}");
        s.observe_mode(p.autopilot);
        assert_eq!(
            decide(&s, &p, &t, back, NO_JITTER).act,
            Act::Probe,
            "{what}"
        );
        let d = probed(&mut s, &p, &t, back, Some(work()));
        assert_eq!(
            typed(&d),
            Some((words::RULE_LAND, words::LAND)),
            "{what}: {d:?}"
        );
        assert!(!s.landed && !s.judging(), "{what}");
        // NEGATIVE CONTROL: an Off over a clean repository keeps it.
        let mut s = landing();
        s.park_off();
        assert_eq!(
            probed(&mut s, &p, &t, back, None).act,
            Act::Nothing,
            "{what}"
        );
        assert!(s.landed && !s.judging(), "{what}");
    }
    // NEGATIVE CONTROL: no Off, the carried landing holds; nothing is probed.
    let mut s = carried();
    s.observe_mode(p.autopilot);
    assert_eq!(decide(&s, &p, &t, back, NO_JITTER).act, Act::Nothing);
    assert!(s.landed && !s.judging());
}

/// A LANDING STANDS THROUGH A WALL THE TASK'S OWN WAKE MEETS, whether this image landed it or
/// a re-exec carried it (aterm main e0da8d68df, F3 and its merge review's P4): a usage window
/// met by a turn nobody asked, over a clean repository: the task stays landed (its landing
/// recorded again for aterm: review of gap 12, finding 1) and nothing is typed past the reset. Dropped there, the resume was typed into the landed work. NEGATIVE
/// CONTROLS: the same wall over a dirty repository is a turn that may have made the work: the
/// landing is over and the resume is typed at the reset.
#[test]
fn a_landing_stands_through_a_wall_whether_restored_or_live() {
    let t = Timing::default();
    let now = Instant::now();
    let p = land();
    let at = now + 60 * MIN;
    let reset = at + 30 * MIN;
    for (case, restored, found, stands) in [
        ("live", false, None, true),
        ("restored", true, None, true),
        ("live, a dirty repository", false, Some(work()), false),
        ("restored, a dirty repository", true, Some(work()), false),
    ] {
        let mut s = if restored {
            re_execed(CLEAN, None, true, now)
        } else {
            landed_at(now, &t)
        };
        s.observe_turn_end(
            TurnEnd {
                wall: Some(Wall::Usage {
                    reset_at: Some(reset),
                    reset_label: None,
                }),
                text: None,
                ..wake(at, "")
            },
            &t,
        );
        if stands {
            let first = kept(&mut s, &p, &t, at, found);
            let later = decide(&s, &p, &t, reset + 10 * MIN, NO_JITTER);
            assert!(s.landed, "{case}: {first:?}");
            assert_eq!(first.act, Act::Nothing, "{case}");
            assert_eq!(later.act, Act::Nothing, "{case}: past the reset");
        } else {
            let first = probed(&mut s, &p, &t, at, found);
            assert!(!s.landed, "{case}");
            assert!(
                matches!(first.act, Act::ResumeLimit { .. }),
                "{case}: {first:?}"
            );
            assert!(first.not_before.is_some_and(|at| at >= reset), "{case}");
        }
    }
}

/// UNDER LAND A WALL'S RESUME TYPES LAND'S INSTRUCTION WITH THE ACK ONLY WHERE THE EPISODE HAS
/// NOT HAD IT (aterm's `continue_as` since `77161d6432`: `(_, Land) if !land.typed =>
/// land_words`, the instruction the one prompt, never a line and then it): a usage window's
/// resume and a quota wall's try before the instruction type it, with the reversible clause
/// where a need names an irreversible act and the ack while a new set of needs stands unacked,
/// and mark the ack given. Past the instruction each types the one cleared line, which carries
/// no ack (aterm's `cleared_text` with the rules alone), so the ack stays owed for the act after
/// it; this test pinned Land's nudge with the ack there before the owner's 2026-10-25 rule ("a
/// prompt into a turn the agent did not end is spam") was ported. NEGATIVE CONTROL: Cruise's
/// resume types the cleared line too, and the ack stays owed.
#[test]
fn a_wall_resume_under_land_types_lands_instruction_with_the_ack_until_it_was_given() {
    let t = Timing::default();
    let now = Instant::now();
    let usage = Wall::Usage {
        reset_at: Some(now + 30 * MIN),
        reset_label: None,
    };
    let at_wall = |need: &str, wall: Wall, mode: Mode, typed: bool| {
        let mut s = ThreadState {
            task: true,
            needs: needs_of(need),
            mode: Some(mode),
            ..ThreadState::default()
        };
        s.land.typed = typed;
        s.observe_turn_end(walled(now, wall), &t);
        assert!(s.needs.ack_owed());
        s
    };
    let plain = "NEEDS YOU: the signing key password";
    let destroy = "NEEDS YOU: say whether to delete the old release branch";
    assert!(needs_of(destroy).destroy() && !needs_of(plain).destroy());
    for (need, rev) in [(plain, false), (destroy, true)] {
        let want = act_words(words::LAND, &[words::reversible_clause(rev), NEEDS_ACK]);
        let mut s = at_wall(need, usage.clone(), Mode::Land, false);
        let d = probed(&mut s, &land(), &t, now, None);
        assert_eq!(d.act, Act::ResumeLimit { text: want.clone() }, "{need}");
        s.acted(&d.act, now + 31 * MIN, &t);
        assert!(!s.needs.ack_owed(), "{need}: the resume acked it");
        assert!(s.land.typed && s.landing, "{need}");
        let mut s = at_wall(need, Wall::Entitlement, Mode::Land, false);
        let d = probed(&mut s, &land(), &t, now, None);
        assert_eq!(
            typed(&d),
            Some((words::RULE_LIMIT_RESUME, want.as_str())),
            "{need}"
        );
        s.acted(&d.act, now + 31 * MIN, &t);
        assert!(!s.needs.ack_owed(), "{need}: the try acked it");
        // Past the instruction: the one line, the ack owed still.
        let mut s = at_wall(need, usage.clone(), Mode::Land, true);
        let d = probed(&mut s, &land(), &t, now, None);
        assert_eq!(
            d.act,
            Act::ResumeLimit {
                text: words::LIMIT_CLEARED.to_string()
            },
            "{need}"
        );
        s.acted(&d.act, now + 31 * MIN, &t);
        assert!(
            s.needs.ack_owed(),
            "{need}: the cleared line carries no ack"
        );
        assert!(s.land.typed && s.landing, "{need}");
        let mut s = at_wall(need, Wall::Entitlement, Mode::Land, true);
        let d = probed(&mut s, &land(), &t, now, None);
        assert_eq!(
            typed(&d),
            Some((words::RULE_LIMIT_RESUME, words::LIMIT_MAY_HAVE_CLEARED)),
            "{need}"
        );
        s.acted(&d.act, now + 31 * MIN, &t);
        assert!(s.needs.ack_owed(), "{need}: the blind line carries no ack");
    }
    // A goal resumed through `GoalService` typed no words: the ack stays owed and the episode
    // stands as it was, the instruction given before and no land act in flight (this pinned a
    // land act in flight until the forever-persist lane, item b: the goal's continuation is no
    // land act, [`ThreadState::goal_resumed`]).
    let mut s = at_wall(plain, usage.clone(), Mode::Land, true);
    s.goal_resumed(now + 31 * MIN, &t);
    assert!(s.needs.ack_owed() && s.land.typed && !s.landing);
    // NEGATIVE CONTROL: Cruise.
    let mut s = at_wall(plain, usage, Mode::Cruise, true);
    let d = probed(&mut s, &policy(), &t, now, None);
    assert_eq!(
        d.act,
        Act::ResumeLimit {
            text: words::LIMIT_CLEARED.to_string()
        }
    );
    s.acted(&d.act, now + 31 * MIN, &t);
    assert!(s.needs.ack_owed());
}

/// A USAGE LIMIT'S RESUME TYPES ITS ONE CLEARED LINE (aterm `77161d6432`, v6 §14.17.5 slice
/// 12a; owner rule 2026-10-25: "a prompt into a turn the agent did not end is spam"): the limit
/// ended the agent's turn, not the agent, so the resume says the limit has cleared where its
/// reset was named, that it may have where none was or a quota wall's try goes blind, and
/// nothing of Cruise's continuation (which waits for the agent's own next turn end); under Land
/// past the instruction the same line, never the nudge; the standing rules ride after it, as
/// aterm's `with_rules`. codex-alab typed `continue_text` there, as aterm `e0da8d68df` did.
/// NEGATIVE CONTROL: under Land before the instruction the resume types the instruction, the
/// one prompt.
#[test]
fn a_usage_limits_resume_types_its_one_cleared_line() {
    const CLEARED: &str = "[codex-alab] The usage limit has cleared; continue where you stopped.";
    const BLIND: &str =
        "[codex-alab] The usage limit may have cleared; continue where you stopped.";
    let t = Timing::default();
    let now = Instant::now();
    let named = Wall::Usage {
        reset_at: Some(now + 30 * MIN),
        reset_label: None,
    };
    let unnamed = Wall::Usage {
        reset_at: None,
        reset_label: None,
    };
    let at_wall = |wall: Wall, mode: Mode, typed: bool| {
        let mut s = ThreadState {
            task: true,
            mode: Some(mode),
            ..ThreadState::default()
        };
        s.land.typed = typed;
        s.observe_turn_end(walled(now, wall), &t);
        s
    };
    let resumed = |d: &Decision| match &d.act {
        Act::ResumeLimit { text } => text.clone(),
        Act::Type { rule, text } if *rule == words::RULE_LIMIT_RESUME => text.clone(),
        other => panic!("a limit's resume: {other:?}"),
    };
    for (p, typed) in [(policy(), false), (land(), true)] {
        for (wall, want) in [
            (named.clone(), CLEARED),
            (unnamed.clone(), BLIND),
            (Wall::Entitlement, BLIND),
        ] {
            let what = format!("{:?} {wall:?}", p.autopilot);
            let mut s = at_wall(wall, p.autopilot, typed);
            let d = probed(&mut s, &p, &t, now, None);
            let text = resumed(&d);
            assert_eq!(text, want, "{what}");
            assert!(!text.contains("keep going"), "{what}: {text}");
            assert!(!text.contains(words::LAND_NUDGE), "{what}: {text}");
        }
    }
    // The standing rules after it.
    let rules = "never push to upstream";
    let p = composed("keep going", rules);
    let mut s = at_wall(named.clone(), Mode::Cruise, false);
    let d = probed(&mut s, &p, &t, now, None);
    assert_eq!(resumed(&d), format!("{CLEARED} (standing rules: {rules})"));
    // NEGATIVE CONTROL: Land before its instruction types the instruction.
    let mut s = at_wall(named, Mode::Land, false);
    let d = probed(&mut s, &land(), &t, now, None);
    assert_eq!(resumed(&d), words::LAND);
}

/// UNDER LAND A USAGE RESET GOES ON WITH A PERSON'S OWN OPENING EXCHANGE (aterm c4a86d8e97's
/// `exchange_at_wall` arm of `continue_as`; review of the forever-cruise lane, finding 2): the
/// limit cut the session's opening message, which Land leaves to the person, so the resume is
/// the one cleared line, never Land's instruction typed over their request, at the window's
/// reset (`ResumeLimit`) and at a quota wall's try (`Type`) alike; and the line gives no
/// instruction (the episode untyped, no hold begun, no land act in flight), as aterm reads
/// `land_given` off the text. Their request's answer within ten minutes is left to them, as the
/// opening exchange's is; once the work is carried, Land's instruction comes at the agent's own
/// turn end. NEGATIVE CONTROL: before the instruction with no exchange of theirs (a need the
/// work before named standing), the resume is the instruction with the ack, which marks both
/// given.
#[test]
fn under_land_a_usage_reset_goes_on_with_a_persons_opening_exchange() {
    let t = Timing::default();
    let now = Instant::now();
    let p = land();
    let need = "Stage 1 is in.\nNEEDS YOU: the signing key password";
    let cut_at = |wall: Wall, opening: bool| {
        let mut s = ThreadState::default();
        s.observe_mode(Mode::Land);
        if !opening {
            s.observe_turn_end(tooled(ours(now - 20 * MIN, need, 12 * MIN)), &t);
        }
        s.ended_under = Some(Mode::Land);
        s.observe_person_turn();
        s.observe_person_hand(now - MIN - Duration::from_secs(5));
        s.observe_turn_end(
            TurnEnd {
                wall: Some(wall),
                text: None,
                ..end(now, "", MIN)
            },
            &t,
        );
        s
    };
    let resumed = |d: &Decision| match &d.act {
        Act::ResumeLimit { text } => text.clone(),
        Act::Type { rule, text } if *rule == words::RULE_LIMIT_RESUME => text.clone(),
        other => panic!("a limit's resume: {other:?}"),
    };
    for (wall, want) in [
        (
            Wall::Usage {
                reset_at: Some(now + 30 * MIN),
                reset_label: None,
            },
            words::LIMIT_CLEARED,
        ),
        (Wall::Entitlement, words::LIMIT_MAY_HAVE_CLEARED),
    ] {
        let what = format!("{wall:?}");
        let mut s = cut_at(wall.clone(), true);
        assert!(s.exchange_at_wall() && !s.left, "{what}");
        let d = probed(&mut s, &p, &t, now, None);
        assert_eq!(resumed(&d), want, "{what}");
        let at = now + 31 * MIN;
        s.acted(&d.act, at, &t);
        assert!(!s.land.typed, "{what}: no instruction was given");
        assert!(!s.landing, "{what}: the answer is the person's request's");
        s.stamp_land_hold(1_000);
        assert_eq!(s.land_held(), None, "{what}: no hold begun");
        // Their request's answer within ten minutes is theirs.
        let mut quick = s.clone();
        let answered = at + 5 * MIN;
        quick.observe_turn_end(tooled(ours(answered, "Here is the plan.", 3 * MIN)), &t);
        assert!(quick.left, "{what}");
        assert_eq!(
            probed(&mut quick, &p, &t, answered, None).act,
            Act::Nothing,
            "{what}"
        );
        // Carried past it: Land's instruction at the agent's own turn end.
        let answered = at + 15 * MIN;
        s.observe_turn_end(tooled(ours(answered, "Stage 1 is in.", 12 * MIN)), &t);
        assert!(!s.exchange, "{what}");
        let d = probed(&mut s, &p, &t, answered, None);
        assert_eq!(typed(&d), Some((words::RULE_LAND, words::LAND)), "{what}");
        // NEGATIVE CONTROL: no exchange of theirs: the instruction with the ack, both given.
        let mut s = cut_at(wall, false);
        assert!(!s.exchange_at_wall(), "{what}");
        assert!(s.needs.ack_owed(), "{what}: the work before named a need");
        let d = probed(&mut s, &p, &t, now, None);
        assert_eq!(resumed(&d), act_words(words::LAND, &[NEEDS_ACK]), "{what}");
        s.acted(&d.act, at, &t);
        assert!(s.land.typed && s.landing, "{what}");
        assert!(!s.needs.ack_owed(), "{what}");
    }
}

/// THE REPLY IS THE WHOLE TURN (aterm main e0da8d68df's `turn_said`): a need said in an
/// earlier agent message of the turn stands beside its last words' (the needs fold over both),
/// and a clean defense at a land act's yield lands nothing while it does: Land's nudge comes
/// with the ack. NEGATIVE CONTROL: earlier messages that name no need leave the defense to
/// land.
#[test]
fn a_need_said_earlier_in_the_turn_stands_and_keeps_a_defense_from_landing() {
    let t = Timing::default();
    let now = Instant::now();
    let p = land();
    let at = now + 20 * MIN;
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Ship the parser.", 5 * MIN), &t);
    run(&mut s, &p, &t, now);
    let mut clean = s.clone();
    s.observe_turn_end(
        TurnEnd {
            earlier: Some("Pushed the branch.\nNEEDS YOU: the signing key password".to_string()),
            ..ours(at, CLEAN, 10 * MIN)
        },
        &t,
    );
    assert_eq!(
        s.needs.standing,
        vec!["the signing key password".to_string()]
    );
    let d = probed(&mut s, &p, &t, at, None);
    assert!(!s.landed, "{d:?}");
    assert_eq!(
        typed(&d),
        Some((
            words::RULE_LAND_NUDGE,
            act_words(words::LAND_NUDGE, &[NEEDS_ACK]).as_str()
        ))
    );
    // NEGATIVE CONTROL.
    clean.observe_turn_end(
        TurnEnd {
            earlier: Some("Pushed the branch; merging next.".to_string()),
            ..ours(at, CLEAN, 10 * MIN)
        },
        &t,
    );
    assert_eq!(probed(&mut clean, &p, &t, at, None).act, Act::Landed);
}

/// An unfinished repository check must yield a continuation, not a landing.
/// Its next clean-defense yield probes again and can land on a completed read.
#[test]
fn an_unread_repository_keeps_land_moving_until_a_completed_read() {
    let t = Timing::default();
    let now = Instant::now();
    let p = land();
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, CLEAN, 5 * MIN), &t);
    run(&mut s, &p, &t, now);
    let at = now + 20 * MIN;
    s.observe_turn_end(ours(at, CLEAN, 10 * MIN), &t);
    assert_eq!(decide(&s, &p, &t, at, NO_JITTER).act, Act::Probe);
    s.probed(crate::repo::RepoRead::Unread);
    let d = decide(&s, &p, &t, at, NO_JITTER);
    let (rule, text) = typed(&d).unwrap_or_else(|| panic!("no continuation: {d:?}"));
    assert_eq!(rule, words::RULE_LAND_NUDGE);
    assert!(text.contains(words::REPO_UNREAD), "{text}");
    assert!(!text.contains("Your repository still holds work"), "{text}");
    s.acted(&d.act, at, &t);
    assert!(!s.landed);
    let later = at + 20 * MIN;
    s.observe_turn_end(ours(later, CLEAN, 10 * MIN), &t);
    assert_eq!(decide(&s, &p, &t, later, NO_JITTER).act, Act::Probe);
    assert_eq!(run(&mut s, &p, &t, later).act, Act::Landed);
}

/// A timed-out recheck cannot preserve an old landing, even if both ends of
/// the wake timed out. The continuation explicitly names the unread state.
#[test]
fn unread_start_and_end_checks_cannot_preserve_a_landing() {
    let t = Timing::default();
    let now = Instant::now();
    let p = land();
    let mut s = landed_at(now, &t);
    s.wake_began_over(crate::repo::RepoRead::Unread);
    let at = now + 60 * MIN;
    s.observe_turn_end(wake(at, QUIET), &t);
    assert!(s.judging());
    s.probed(crate::repo::RepoRead::Unread);
    assert!(!s.landed && !s.judging());
    let d = decide(&s, &p, &t, at, NO_JITTER);
    let (_, text) = typed(&d).unwrap_or_else(|| panic!("no continuation: {d:?}"));
    assert!(text.contains(words::REPO_UNREAD), "{text}");
}

#[test]
fn cruise_names_an_unread_repository_and_still_continues() {
    let t = Timing::default();
    let now = Instant::now();
    let p = policy();
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Everything is done.", 5 * MIN), &t);
    assert_eq!(decide(&s, &p, &t, now, NO_JITTER).act, Act::Probe);
    s.probed(crate::repo::RepoRead::Unread);
    let d = decide(&s, &p, &t, now, NO_JITTER);
    let (rule, text) = typed(&d).unwrap_or_else(|| panic!("no continuation: {d:?}"));
    assert_eq!(rule, words::RULE_DONE_CHECK);
    // Cruise's unread words ask for the agent's own work (aterm's `REPO_UNREAD_OWN`).
    assert!(text.contains(words::REPO_UNREAD_OWN), "{text}");
    assert!(!text.contains(words::REPO_UNREAD), "{text}");
    assert!(!s.landed);
}

#[test]
fn an_unread_off_recheck_reopens_land() {
    let t = Timing::default();
    let now = Instant::now();
    let p = land();
    let mut s = landed_at(now, &t);
    s.park_off();
    s.observe_mode(Mode::Land);
    assert!(s.judging());
    s.probed(crate::repo::RepoRead::Unread);
    assert!(!s.landed && !s.judging());
    assert_eq!(
        typed(&decide(&s, &p, &t, now + 40 * MIN, NO_JITTER)),
        Some((words::RULE_LAND, words::LAND))
    );
}

#[test]
fn carried_done_claims_keep_an_unread_check_in_their_continuation() {
    let t = Timing::default();
    let now = Instant::now();
    for (p, rule, text, unread) in [
        (
            policy(),
            words::RULE_DONE_CHECK,
            words::DONE_CHECK,
            words::REPO_UNREAD_OWN,
        ),
        (
            land(),
            words::RULE_LAND_NUDGE,
            words::LAND_NUDGE,
            words::REPO_UNREAD,
        ),
    ] {
        let owed = Owed {
            native_origin: None,
            rule,
            text: text.to_string(),
            mode: p.autopilot,
            not_before: now,
            repo_owed: Some(Vec::new()),
            resume_goal: false,
            landing: false,
        };
        let mut s = re_execed(CLEAN, Some(owed), false, now);
        s.observe_mode(p.autopilot);
        assert_eq!(decide(&s, &p, &t, now, NO_JITTER).act, Act::Probe);
        s.probed(crate::repo::RepoRead::Unread);
        let d = decide(&s, &p, &t, now, NO_JITTER);
        let (got, text) = typed(&d).unwrap_or_else(|| panic!("no continuation: {d:?}"));
        assert_eq!(got, rule);
        assert!(text.contains(unread), "{text}");
        assert!(!s.landed);
    }
}

/// The usage wall the sleep tests stand at: its reset `reset` after `start`.
fn slept_at_usage(start: Instant, reset: Duration) -> ThreadState {
    let t = Timing::default();
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(
        walled(
            start,
            Wall::Usage {
                reset_at: Some(start + reset),
                reset_label: Some("12:00".to_string()),
            },
        ),
        &t,
    );
    s
}

/// A RESET THE HOST SLEPT THROUGH IS DUE AT THE WAKE, NOT AS MANY HOURS LATER AS IT SLEPT
/// (2026-10-24): the reset was set on the `Instant` clock, which stops while the host sleeps; a
/// sleep of two hours counted at the next decision moves it back two hours, so the resume is
/// due now. A growth under a second is the two clocks read apart and moves nothing, and the
/// point's first reading only marks it. POSITIVE CONTROL: uncounted, the same decision waits
/// out the reset on the awake clock, 55 min and its grace from now.
#[test]
fn a_reset_the_host_slept_through_is_due_at_the_wake() {
    let t = Timing::default();
    let p = policy();
    let start = Instant::now();
    let base = Duration::from_secs(5000);
    let now = start + 5 * MIN;
    let mut s = slept_at_usage(start, 60 * MIN);
    // POSITIVE CONTROL: no sleep counted.
    let uncounted = decide(&s, &p, &t, now, NO_JITTER);
    assert!(
        matches!(uncounted.act, Act::ResumeLimit { .. }),
        "{uncounted:?}"
    );
    assert_eq!(uncounted.not_before, Some(start + 60 * MIN + t.reset_grace));
    // The point's first reading marks it; a growth under a second moves nothing.
    assert_eq!(s.slept_to(Some(base), start), None);
    assert_eq!(
        s.slept_to(Some(base + Duration::from_millis(400)), start),
        None
    );
    s.net_asked = Some(start);
    // Two hours asleep: the reset moves back by them, and the reach is asked for again.
    assert_eq!(
        s.slept_to(Some(base + 120 * MIN + Duration::from_millis(400)), now),
        Some(120 * MIN)
    );
    assert_eq!(s.net_asked, None, "the wake's network is measured again");
    let d = decide(&s, &p, &t, now, NO_JITTER);
    assert!(matches!(d.act, Act::ResumeLimit { .. }), "{d:?}");
    assert!(
        d.not_before.is_some_and(|at| at <= now),
        "due at the wake: {d:?}"
    );
    // A deadline the clock cannot move back that far is now.
    let mut s = slept_at_usage(start, 60 * MIN);
    s.slept_to(Some(base), start);
    s.slept_to(Some(base + Duration::from_secs(u64::from(u32::MAX))), now);
    let reset = s.point.as_ref().and_then(|point| match point.wall {
        Some(Wall::Usage { reset_at, .. }) => reset_at,
        _ => None,
    });
    assert!(reset.is_some_and(|at| at <= now), "{reset:?}");
}

/// An owed act restored from the record is a real-time wait too. The timer's early
/// wake must not merely re-arm the same uncorrected deadline in the decider.
#[test]
fn a_restored_owed_deadline_counts_sleep_once() {
    let timing = Timing::default();
    let start = Instant::now();
    let base = Duration::from_secs(5000);
    let original = start + 60 * MIN;
    let now = start + 5 * MIN;
    let mut state = ThreadState::default();
    state.observe_resume(true, Some("Independent work is in hand."), false);
    state.observe_owed(
        Some(Owed {
            native_origin: None,
            rule: words::RULE_CONTINUE,
            text: "Continue the independently authorized work.".into(),
            mode: Mode::Cruise,
            not_before: original,
            repo_owed: None,
            resume_goal: false,
            landing: false,
        }),
        false,
        start,
    );
    assert_eq!(state.slept_to(Some(base), start), None);
    assert_eq!(state.slept_to(Some(base), now), None);
    assert_eq!(
        decide(&state, &policy(), &timing, now, NO_JITTER).not_before,
        Some(original),
        "without a sleep the restored full wait stands"
    );
    assert_eq!(state.slept_to(Some(base + 120 * MIN), now), Some(120 * MIN));
    let after = decide(&state, &policy(), &timing, now, NO_JITTER);
    assert_eq!(
        typed(&after),
        Some((
            words::RULE_CONTINUE,
            "Continue the independently authorized work."
        )),
        "the due result must still carry the exact restored act"
    );
    assert!(
        after.not_before.is_none_or(|deadline| deadline <= now),
        "the restored owed act slept through its deadline: {after:?}"
    );
    let deadline = state
        .owed
        .as_ref()
        .expect("the act remains owed")
        .not_before;
    assert_eq!(state.slept_to(Some(base + 120 * MIN), now), None);
    assert_eq!(
        state
            .owed
            .as_ref()
            .expect("the same act remains owed")
            .not_before,
        deadline,
        "one sleep is credited once"
    );
}

/// AFTER A WAKE THE HOST SETTLES BEFORE ANY ACT (aterm's `settle_wake`; 2026-10-24, a resume
/// typed inside a 19 s dark wake with the network down): a resume due at a wake waits until
/// the host has been awake [`Timing::wake_settle`] and this thread's share of the jitter, and
/// says so; a dark wake ends before that, and the wake after it settles again. So does any act
/// typed into the agent (a continuation at a turn end). NEGATIVE CONTROLS: no wake, the resume
/// goes at once; a wake long past, likewise; an attention is never held.
#[test]
fn an_act_waits_for_the_host_to_settle_after_a_wake() {
    let t = Timing::default();
    let p = policy();
    let start = Instant::now();
    let now = start + 70 * MIN;
    let jitter = Duration::from_secs(20);
    let mut s = slept_at_usage(start, 60 * MIN);
    // NEGATIVE CONTROL: no wake.
    let d = decide(&s, &p, &t, now, jitter);
    assert!(d.not_before.is_some_and(|at| at <= now), "{d:?}");
    // A dark wake 5 s ago: the resume waits for the settle and its share.
    let dark = now - Duration::from_secs(5);
    s.woke = Some(dark);
    let d = decide(&s, &p, &t, now, jitter);
    assert!(matches!(d.act, Act::ResumeLimit { .. }), "{d:?}");
    assert_eq!(d.not_before, Some(dark + t.wake_settle + jitter));
    assert!(
        d.shown_wait
            .as_deref()
            .is_some_and(|w| w.contains("the host just woke")),
        "{d:?}"
    );
    // The dark wake ended at 19 s; the next wake (the lid opened) settles again, from it.
    let lid = dark + 25 * MIN;
    s.woke = Some(lid);
    let at = lid + Duration::from_secs(1);
    assert_eq!(
        decide(&s, &p, &t, at, jitter).not_before,
        Some(lid + t.wake_settle + jitter)
    );
    // Settled: as decided.
    let settled = lid + t.wake_settle + jitter;
    let d = decide(&s, &p, &t, settled, jitter);
    assert!(d.not_before.is_some_and(|at| at <= settled), "{d:?}");
    // NEGATIVE CONTROL: a wake long past.
    s.woke = Some(now - 10 * MIN);
    let d = decide(&s, &p, &t, now, jitter);
    assert!(d.not_before.is_some_and(|at| at <= now), "{d:?}");
    // A continuation at a turn end waits for the settle too.
    let mut c = ThreadState::default();
    c.observe_turn_end(ours(now, "Done a thing.", 5 * MIN), &t);
    c.task = false;
    c.observe_turn_end(end(now, "Done a thing.", 5 * MIN), &t);
    let d = decide(&c, &p, &t, now, NO_JITTER);
    assert_eq!(typed(&d), Some((words::RULE_CONTINUE, "keep going")));
    assert_eq!(d.not_before, None, "no wake: at once");
    c.woke = Some(now);
    let d = decide(&c, &p, &t, now, NO_JITTER);
    assert_eq!(typed(&d), Some((words::RULE_CONTINUE, "keep going")));
    assert_eq!(d.not_before, Some(now + t.wake_settle));
    // NEGATIVE CONTROL: an attention is never held.
    let mut a = ThreadState::default();
    a.observe_person_turn();
    a.observe_turn_end(walled(now, Wall::Login), &t);
    a.woke = Some(now);
    let d = decide(&a, &p, &t, now, NO_JITTER);
    assert!(matches!(d.act, Act::Attention { .. }), "{d:?}");
    assert_eq!(d.not_before, None);
}

/// A DUE USAGE RESUME IS NEVER TYPED INTO AN API MEASURED DOWN (2026-10-24): once its reach was
/// asked for ([`ThreadState::net_asked`]), a down measure holds it until the measure comes back
/// up or, at most, the down hold from the ask, and it waits up to [`Timing::probe_wait`] for its
/// first measure. Up, it goes at once; an unknown measure past the probe's wait holds it no
/// longer. NEGATIVE CONTROL: a resume whose reach was never asked for (a route the probe cannot
/// reproduce) goes as decided whatever the reach field says.
#[test]
fn a_due_usage_resume_is_never_typed_into_an_api_measured_down() {
    let t = Timing::default();
    let p = policy();
    let start = Instant::now();
    let now = start + 70 * MIN;
    let asked = now - Duration::from_secs(1);
    let mut s = slept_at_usage(start, 60 * MIN);
    s.net_asked = Some(asked);
    s.reach = Reach::Down { since: asked };
    let d = decide(&s, &p, &t, now, NO_JITTER);
    assert!(matches!(d.act, Act::ResumeLimit { .. }), "{d:?}");
    assert_eq!(d.not_before, Some(asked + t.down_hold));
    assert!(d.why.contains("measured unreachable"), "{d:?}");
    // The measure comes back up: at once.
    s.reach = Reach::Up { since: now };
    let d = decide(&s, &p, &t, now, NO_JITTER);
    assert!(d.not_before.is_some_and(|at| at <= now), "{d:?}");
    // No measure in yet: the probe's wait.
    s.reach = Reach::Unknown;
    assert_eq!(
        decide(&s, &p, &t, now, NO_JITTER).not_before,
        Some(asked + t.probe_wait)
    );
    // Past it, an unknown measure holds nothing.
    let later = asked + t.probe_wait;
    let d = decide(&s, &p, &t, later, NO_JITTER);
    assert!(d.not_before.is_some_and(|at| at <= later), "{d:?}");
    // Down past the hold: one try all the same.
    s.reach = Reach::Down { since: asked };
    let held = asked + t.down_hold;
    let d = decide(&s, &p, &t, held, NO_JITTER);
    assert!(d.not_before.is_some_and(|at| at <= held), "{d:?}");
    // NEGATIVE CONTROL: never asked for.
    s.net_asked = None;
    let d = decide(&s, &p, &t, now, NO_JITTER);
    assert!(d.not_before.is_some_and(|at| at <= now), "{d:?}");
}

/// aterm'S `probe_api` OFF READS NO REACH (gap 30): a network wall under a reach measured
/// before the switch went off keeps the network ladder, Down or Up, as an unknown reach does,
/// and a due usage resume whose reach was asked for goes as decided, neither held for the
/// probe's first measure nor by a down one. NEGATIVE CONTROL: the switch on, the same states
/// hold the wall for the outage and the resume for the measure.
#[test]
fn aterms_probe_api_off_reads_no_reach_and_the_walls_keep_the_ladder() {
    let t = Timing::default();
    let off = Policy {
        probe_api: false,
        ..policy()
    };
    let start = Instant::now();
    let wall = Wall::Network {
        what: "an API error (the API could not be reached)".to_string(),
    };
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(walled(start, wall), &t);
    for reach in [Reach::Down { since: start }, Reach::Up { since: start }] {
        s.reach = reach;
        let d = decide(&s, &off, &t, start, NO_JITTER);
        assert_eq!(
            (typed(&d).map(|(rule, _)| rule), d.not_before),
            (Some(words::RULE_API_RETRY), Some(start + MIN)),
            "{reach:?} off: the ladder"
        );
        assert!(d.why.ends_with("; retrying"), "{d:?}");
    }
    // NEGATIVE CONTROL: on, the outage holds the wall.
    s.reach = Reach::Down { since: start };
    assert_eq!(
        decide(&s, &policy(), &t, start, NO_JITTER).not_before,
        Some(start + t.down_hold),
        "on: the hold"
    );

    let now = start + 70 * MIN;
    let asked = now - Duration::from_secs(1);
    let mut s = slept_at_usage(start, 60 * MIN);
    s.net_asked = Some(asked);
    for reach in [Reach::Unknown, Reach::Down { since: asked }] {
        s.reach = reach;
        let d = decide(&s, &off, &t, now, NO_JITTER);
        assert!(matches!(d.act, Act::ResumeLimit { .. }), "{d:?}");
        assert!(
            d.not_before.is_some_and(|at| at <= now),
            "{reach:?} off: as decided {d:?}"
        );
        // NEGATIVE CONTROL: on, the measure holds it.
        assert!(
            decide(&s, &policy(), &t, now, NO_JITTER)
                .not_before
                .is_some_and(|at| at > now),
            "{reach:?} on: held"
        );
    }
}

#[test]
fn an_owed_answer_honors_question_switches_and_unreadable_hour_cap() {
    let t = Timing::default();
    let now = Instant::now();
    let due = now + 2 * MIN;

    let plain_owed = Owed {
        native_origin: None,
        rule: words::RULE_PROSE_LONG_TERM,
        text: words::LONG_TERM_TEXT.to_string(),
        mode: Mode::Cruise,
        not_before: due,
        repo_owed: None,
        resume_goal: false,
        landing: false,
    };
    let s = re_execed("Which layout should I use?", Some(plain_owed), false, now);

    // answer_questions=false raises question attention even when continue_on=true.
    let no_answer = Policy {
        answer_questions: false,
        ..policy()
    };
    assert_eq!(
        decide(&s, &no_answer, &t, due, NO_JITTER).act,
        Act::Attention {
            key: "question",
            text: "question: the worker asked a question; answer_questions is off".to_string(),
        }
    );

    // questions=ask leaves a non-irreversible Cruise question to a person.
    let ask_person = Policy {
        questions: Questions::Ask,
        ..policy()
    };
    assert_eq!(
        decide(&s, &ask_person, &t, due, NO_JITTER).act,
        Act::Attention {
            key: "question",
            text: "question: the worker asked a question; questions=ask leaves it to a person"
                .to_string(),
        }
    );

    // continue_on=false does NOT suppress an owed question answer when answer_questions=true.
    let no_continue = Policy {
        continue_on: false,
        ..policy()
    };
    assert_eq!(
        typed(&decide(&s, &no_continue, &t, due, NO_JITTER)),
        Some((words::RULE_PROSE_LONG_TERM, words::LONG_TERM_TEXT))
    );

    // An irreversible Cruise answer and a Land answer still go under questions=ask,
    // and use the classified irreversible reason when answer_questions=false.
    let ask_irreversible = "Should I force-push main to remove it?";
    for (mode, rule, text) in [
        (
            Mode::Cruise,
            words::RULE_ANSWER,
            words::REVERSIBLE_ANSWER.to_string(),
        ),
        (
            Mode::Land,
            words::RULE_LAND_ANSWER,
            words::LAND_ANSWER.to_string(),
        ),
    ] {
        let owed = Owed {
            native_origin: None,
            rule,
            text: text.clone(),
            mode,
            not_before: due,
            repo_owed: None,
            resume_goal: false,
            landing: false,
        };
        let s = re_execed(ask_irreversible, Some(owed), false, now);
        let p_ask = Policy {
            autopilot: mode,
            questions: Questions::Ask,
            ..policy()
        };
        assert_eq!(
            typed(&decide(&s, &p_ask, &t, due, NO_JITTER)),
            Some((rule, text.as_str()))
        );
        let p_off = Policy {
            autopilot: mode,
            answer_questions: false,
            ..policy()
        };
        let Act::Attention { key, text: why } = decide(&s, &p_off, &t, due, NO_JITTER).act else {
            panic!("expected attention");
        };
        assert_eq!(key, "question");
        assert!(why.contains("force-push"), "{why}");
        assert!(why.ends_with("; answer_questions is off"), "{why}");
    }

    // continue_per_hour=Some(0) (unreadable cap) types nothing for owed continue-type acts.
    let unreadable_cap = Policy {
        continue_per_hour: Some(0),
        ..policy()
    };
    let d = decide(&s, &unreadable_cap, &t, due, NO_JITTER);
    assert_eq!(d.act, Act::Nothing);
    assert_eq!(d.why, "continue_per_hour unreadable: nothing is typed");
}
