// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The pure decider, point by point (each rule with its negative control).

use std::time::Duration;
use std::time::Instant;

use crate::decide::Act;
use crate::decide::Decision;
use crate::decide::Policy;
use crate::decide::Questions;
use crate::decide::ThreadState;
use crate::decide::Timing;
use crate::decide::TurnEnd;
use crate::decide::Wall;
use crate::decide::decide;
use crate::words;
use pretty_assertions::assert_eq;

const MIN: Duration = Duration::from_secs(60);
const NO_JITTER: Duration = Duration::ZERO;

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
        resume_limits: true,
        limit_wait: true,
        compact_on_context_wall: true,
        upgrade: true,
        human_grace: Duration::from_secs(120),
    }
}

fn end(at: Instant, text: &str, worked: Duration) -> TurnEnd {
    TurnEnd {
        at,
        by_autopilot: false,
        by_person: true,
        text: Some(text.to_string()),
        questions: false,
        worked,
        wall: None,
        aborted: false,
    }
}

fn ours(at: Instant, text: &str, worked: Duration) -> TurnEnd {
    TurnEnd {
        by_autopilot: true,
        by_person: false,
        ..end(at, text, worked)
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

fn run(s: &mut ThreadState, p: &Policy, t: &Timing, now: Instant) -> Decision {
    let d = decide(s, p, t, now, NO_JITTER);
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

#[test]
fn carry_on_only_after_a_relaunch_with_an_unfinished_task() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_resume(true, Some("Working through stage 2."), true);
    assert_eq!(
        typed(&decide(&s, &policy(), &t, now, NO_JITTER)),
        Some((words::RULE_HOST_TURN, "keep going"))
    );
    // A plain resume (the person ran `codex resume`): nothing until their turn.
    let mut s = ThreadState::default();
    s.observe_resume(true, Some("Working through stage 2."), false);
    assert_eq!(decide(&s, &policy(), &t, now, NO_JITTER).act, Act::Nothing);
    // A relaunch whose last reply finished the task or said done: nothing.
    for last in [
        "All landed.\nATERM-TASK-DONE",
        "All done, nothing left to do.",
    ] {
        let mut s = ThreadState::default();
        s.observe_resume(true, Some(last), true);
        assert_eq!(
            decide(&s, &policy(), &t, now, NO_JITTER).act,
            Act::Nothing,
            "{last}"
        );
    }
    // A relaunch with no person's turn in the history: taskless.
    let mut s = ThreadState::default();
    s.observe_resume(false, Some("Hello."), true);
    assert_eq!(decide(&s, &policy(), &t, now, NO_JITTER).act, Act::Nothing);
}

#[test]
fn finished_ends_the_task_until_a_persons_turn() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "Pushed.\nATERM-TASK-DONE", 5 * MIN), &t);
    assert_eq!(run(&mut s, &policy(), &t, now).act, Act::Done);
    assert!(s.done);
    s.observe_turn_end(ours(now, "Nothing more.", MIN), &t);
    assert_eq!(decide(&s, &policy(), &t, now, NO_JITTER).act, Act::Nothing);
    // A person's turn reopens it.
    s.observe_turn_end(end(now, "Fixed the next bug.", 5 * MIN), &t);
    assert_eq!(
        typed(&decide(&s, &policy(), &t, now, NO_JITTER)),
        Some((words::RULE_CONTINUE, "keep going"))
    );
}

#[test]
fn a_done_reply_gets_the_done_check_once_and_a_second_ends_the_task() {
    let t = Timing::default();
    let now = Instant::now();
    let mut s = ThreadState::default();
    s.observe_turn_end(end(now, "All done, nothing left to do.", 5 * MIN), &t);
    let d = run(&mut s, &policy(), &t, now);
    assert_eq!(typed(&d), Some((words::RULE_DONE_CHECK, words::DONE_CHECK)));
    s.observe_turn_end(ours(now, "Everything is done.", 3 * MIN), &t);
    assert_eq!(decide(&s, &policy(), &t, now, NO_JITTER).act, Act::Done);
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
    assert_eq!(
        typed(&decide(&s, &policy(), &t, now, NO_JITTER)),
        Some((words::RULE_ANSWER, "Pick the recommended option."))
    );
    let long = Policy {
        questions: Questions::LongTerm,
        ..policy()
    };
    assert_eq!(
        typed(&decide(&s, &long, &t, now, NO_JITTER)),
        Some((words::RULE_PROSE_LONG_TERM, words::LONG_TERM_TEXT))
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
    assert_eq!(
        typed(&decide(&s, &policy(), &t, now, NO_JITTER)),
        Some((words::RULE_ANSWER, words::REVERSIBLE_ANSWER))
    );
    // Whatever the priority word says, even long-term.
    let long = Policy {
        questions: Questions::LongTerm,
        ..policy()
    };
    assert_eq!(
        typed(&decide(&s, &long, &t, now, NO_JITTER)),
        Some((words::RULE_ANSWER, words::REVERSIBLE_ANSWER))
    );
}

#[test]
fn server_errors_climb_the_retry_ladder_from_each_try() {
    let t = Timing::default();
    let start = Instant::now();
    let p = policy();
    let mut s = ThreadState::default();
    s.observe_person_turn();
    let wall = || Wall::Server {
        what: "an API error (500)".to_string(),
        cut_off: false,
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
    assert_eq!(d.not_before, Some(start + MIN + 5 * MIN));
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
fn a_retry_that_led_to_real_work_starts_the_ladder_over() {
    let t = Timing::default();
    let start = Instant::now();
    let p = policy();
    let mut s = ThreadState::default();
    s.observe_person_turn();
    let wall = || Wall::Server {
        what: "an API error (500)".to_string(),
        cut_off: false,
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
    // Negative control: a retry that did less than the progress climbs on.
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
    assert_eq!(d.not_before, Some(start + MIN + 5 * MIN));
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
        let since = s.wall.as_ref().and_then(|w| w.last_try).unwrap_or(now);
        assert_eq!(d.not_before, Some(since + step * MIN), "step {step}");
        now = d.not_before.unwrap();
        s.acted(&d.act, now, &t);
    }
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
fn an_entitlement_wall_is_retried_rarely_and_never_bought() {
    let t = Timing::default();
    let start = Instant::now();
    let mut s = ThreadState::default();
    s.observe_person_turn();
    s.observe_turn_end(walled(start, Wall::Entitlement), &t);
    let d = decide(&s, &policy(), &t, start, NO_JITTER);
    assert_eq!(d.not_before, Some(start + 30 * MIN));
    assert_eq!(typed(&d), Some((words::RULE_LIMIT_RESUME, "keep going")));
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
        "/Users/_owner/.aterm/agents/codex",
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
        "/Users/_owner/.local/share/aterm/dev-builds/artifacts/codex/0.164.0-dev.1/bin/codex",
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

/// Finding 1 (review of cf084e3b7): the carry-on chain ends where the decision finishes the
/// task, not where a done phrase appears. A first "completed successfully" gets the done
/// check, and the worker may carry on from it; only the reply after the check, or
/// `ATERM-TASK-DONE`, finishes.
#[test]
fn only_a_finishing_decision_ends_the_task() {
    use crate::decide::ends_task;
    let t = Timing::default();
    let now = Instant::now();
    // A relaunched process: its task from the history, no person's turn in it.
    let mut s = ThreadState::default();
    s.observe_resume(true, Some("Working through stage 2."), false);
    s.observe_turn_end(
        ours(
            now,
            "Build completed successfully; next I'll run the tests.",
            5 * MIN,
        ),
        &t,
    );
    assert!(!ends_task(&s), "a first done phrase is no finish");
    let d = run(&mut s, &policy(), &t, now);
    assert_eq!(typed(&d), Some((words::RULE_DONE_CHECK, words::DONE_CHECK)));
    // The reply after the done check that says done again: finished.
    s.observe_turn_end(ours(now, "All done.", 5 * MIN), &t);
    assert!(ends_task(&s));
    assert_eq!(decide(&s, &policy(), &t, now, NO_JITTER).act, Act::Done);
    // Negative control: the explicit token finishes at once, done check or none.
    let mut s = ThreadState::default();
    s.observe_resume(true, Some("Working through stage 2."), false);
    s.observe_turn_end(ours(now, "All landed.\nATERM-TASK-DONE", 5 * MIN), &t);
    assert!(ends_task(&s));
}

/// `ends_task` is `decide`'s own verdict: across walls, compactions, the done check, a
/// finished task and a taskless thread, it holds exactly where `decide` answers `Act::Done`.
#[test]
fn ends_task_agrees_with_decide_everywhere() {
    use crate::decide::ends_task;
    let t = Timing::default();
    let now = Instant::now();
    let texts = [
        "All landed.\nATERM-TASK-DONE",
        "Build completed successfully; next I'll run the tests.",
        "Anything else?",
        "Stage 2 is half done; continuing next.",
        "DONE",
    ];
    let mut seen = [0_u32; 2];
    for text in texts {
        for (task, checked, done, compacting, after_compact, wall) in every_flag() {
            let mut s = ThreadState {
                task,
                done_checked: checked,
                done,
                compacting,
                after_compact,
                ..ThreadState::default()
            };
            s.point = Some(if wall {
                TurnEnd {
                    text: Some(text.to_string()),
                    ..walled(
                        now,
                        Wall::Server {
                            what: "500".to_string(),
                            cut_off: false,
                        },
                    )
                }
            } else {
                ours(now, text, 5 * MIN)
            });
            let finishes = decide(&s, &policy(), &t, now, NO_JITTER).act == Act::Done;
            assert_eq!(ends_task(&s), finishes, "{text:?} {s:?}");
            seen[usize::from(finishes)] += 1;
        }
    }
    assert!(
        seen[0] > 0 && seen[1] > 0,
        "both verdicts were met: {seen:?}"
    );
}

fn every_flag() -> Vec<(bool, bool, bool, bool, bool, bool)> {
    let b = [false, true];
    let mut out = Vec::new();
    for task in b {
        for checked in b {
            for done in b {
                for compacting in b {
                    for after in b {
                        for wall in b {
                            out.push((task, checked, done, compacting, after, wall));
                        }
                    }
                }
            }
        }
    }
    out
}

/// Finding 2 (review of cf084e3b7): the process's first resume spends the relaunch whatever
/// it learns. A sweep that ran late (no predecessor known yet), or a first resume of another
/// thread, leaves nothing for a later `/resume` back to carry on from.
#[test]
fn the_first_resume_spends_the_relaunch_whatever_it_learns() {
    use crate::extension::relaunched;
    let line = ["/abs/bin/codex", "resume", "019a-id"];
    // The sweep had not finished at the first resume: no predecessor then.
    let shared = relaunched_start(&line, 0, 812, Some(812));
    let predecessor = shared
        .predecessor("019a-id")
        .unwrap_or_else(|| panic!("noted"));
    let late = Box::leak(Box::new(codex_alab_core::session::Shared::new()));
    if let Some(launch) = shared.launch() {
        late.set_launch(launch.clone());
    }
    late.set_launch_witness(shared.launch_witness());
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
