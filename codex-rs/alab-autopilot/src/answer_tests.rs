// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use codex_alab_core::boxes::BoxAnswer;
use codex_alab_core::boxes::BoxKind;
use codex_alab_core::boxes::BoxOption;
use codex_alab_core::boxes::BoxQuestion;
use codex_alab_core::boxes::Choice;
use codex_alab_core::boxes::GoalStop;
use codex_alab_core::boxes::OpenBox;
use pretty_assertions::assert_eq;

use crate::answer::BoxDecision;
use crate::answer::CHOSE_FIRST_SAFE;
use crate::answer::CHOSE_UNMARKED;
use crate::answer::GOAL_LEAVE;
use crate::answer::GOAL_RESUME;
use crate::answer::PLAN_YES;
use crate::answer::RULE_ANSWER_LONG_TERM;
use crate::answer::RULE_ANSWER_RECOMMENDED;
use crate::answer::RULE_ANSWER_SHORT_TERM;
use crate::answer::RULE_GOAL_LEAVE;
use crate::answer::RULE_PLAN;
use crate::answer::bears;
use crate::answer::decide_box;
use crate::answer::declined;
use crate::answer::refuses;
use crate::decide::Policy;
use crate::decide::Questions;
use crate::words;

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
        autopilot: crate::decide::Mode::Cruise,
        texts: crate::decide::Texts::default(),
    }
}

fn q(id: &str, question: &str, options: &[(&str, &str)]) -> BoxQuestion {
    BoxQuestion {
        id: id.to_string(),
        header: String::new(),
        question: question.to_string(),
        options: options
            .iter()
            .map(|(label, description)| BoxOption {
                label: label.to_string(),
                description: description.to_string(),
            })
            .collect(),
        is_other: true,
        is_secret: false,
    }
}

fn question_box(questions: Vec<BoxQuestion>) -> OpenBox {
    OpenBox {
        id: "call-1".to_string(),
        kind: BoxKind::Question {
            thread: "t".to_string(),
            questions,
            blocking: true,
            begun: None,
        },
        opened_ms: 0,
    }
}

fn chosen(decision: &BoxDecision) -> Vec<(String, Choice)> {
    match decision {
        BoxDecision::Answer {
            answer: BoxAnswer::Question { answers, .. },
            ..
        } => answers.clone(),
        other => panic!("expected a question's answer, got {other:?}"),
    }
}

fn said(decision: &BoxDecision) -> &str {
    match decision {
        BoxDecision::Answer { words, .. } => words,
        other => panic!("expected a question's answer, got {other:?}"),
    }
}

fn left(decision: &BoxDecision) -> &str {
    match decision {
        BoxDecision::Leave(why) => why,
        other => panic!("expected the box left to a person, got {other:?}"),
    }
}

#[test]
fn the_recommended_option_answers_and_option_one_when_none_is_marked() {
    let open = question_box(vec![
        q(
            "a",
            "Which cache?",
            &[("Disk", "slow"), ("Memory (Recommended)", "fast")],
        ),
        q("b", "Which name?", &[("cache", ""), ("store", "")]),
    ]);
    let decision = decide_box(&open, &policy());
    assert_eq!(
        chosen(&decision),
        vec![
            ("a".to_string(), Choice::Option(1)),
            ("b".to_string(), Choice::Option(0)),
        ]
    );
    assert!(matches!(
        decision,
        BoxDecision::Answer {
            rule: RULE_ANSWER_RECOMMENDED,
            ..
        }
    ));
}

#[test]
fn a_free_text_question_gets_answer_text_and_a_destructive_one_the_reversible_answer() {
    let open = question_box(vec![q("a", "What should we call it?", &[])]);
    assert_eq!(
        chosen(&decide_box(&open, &policy())),
        vec![(
            "a".to_string(),
            Choice::Note("Pick the recommended option.".to_string())
        )]
    );
    let open = question_box(vec![q("a", "Which branch should I force-push?", &[])]);
    assert_eq!(
        chosen(&decide_box(&open, &policy())),
        vec![(
            "a".to_string(),
            Choice::Note(words::REVERSIBLE_ANSWER.to_string())
        )]
    );
    let no_text = Policy {
        answer_text: None,
        ..policy()
    };
    let open = question_box(vec![q("a", "What should we call it?", &[])]);
    assert!(left(&decide_box(&open, &no_text)).contains("no answer_text"));
}

#[test]
fn an_unmarked_destructive_question_takes_its_one_refusal_or_is_left() {
    let open = question_box(vec![q(
        "a",
        "Should I delete the old migrations?",
        &[("Yes, delete them", ""), ("No, keep them", "")],
    )]);
    assert_eq!(
        chosen(&decide_box(&open, &policy())),
        vec![("a".to_string(), Choice::Option(1))]
    );
    // Negative control: two refusals, or none, and a person answers.
    let open = question_box(vec![q(
        "a",
        "Should I delete the old migrations?",
        &[("Yes", ""), ("Not now", "")],
    )]);
    assert!(left(&decide_box(&open, &policy())).contains("would delete"));
    // A marked recommendation is the model's pick, as aterm takes it.
    let open = question_box(vec![q(
        "a",
        "Should I delete the old migrations?",
        &[
            ("No, keep them", ""),
            ("Yes, delete them (Recommended)", ""),
        ],
    )]);
    assert_eq!(
        chosen(&decide_box(&open, &policy())),
        vec![("a".to_string(), Choice::Option(1))]
    );
}

#[test]
fn the_long_term_word_writes_its_text_and_lets_the_worker_decide() {
    let long = Policy {
        questions: Questions::LongTerm,
        ..policy()
    };
    let open = question_box(vec![q(
        "a",
        "Which cache?",
        &[("Disk", ""), ("Memory (Recommended)", "")],
    )]);
    let decision = decide_box(&open, &long);
    assert_eq!(
        chosen(&decision),
        vec![(
            "a".to_string(),
            Choice::Note(words::LONG_TERM_TEXT.to_string())
        )]
    );
    assert!(matches!(
        decision,
        BoxDecision::Answer {
            rule: RULE_ANSWER_LONG_TERM,
            ..
        }
    ));
}

#[test]
fn ask_off_secret_begun_and_kept_answers_are_left_to_a_person() {
    let open = question_box(vec![q("a", "Which?", &[("A", ""), ("B", "")])]);
    let ask = Policy {
        questions: Questions::Ask,
        ..policy()
    };
    assert!(left(&decide_box(&open, &ask)).contains("`ask`"));
    let off = Policy {
        answer_questions: false,
        ..policy()
    };
    assert!(left(&decide_box(&open, &off)).contains("answer_questions"));

    let mut secret = q("a", "Your token?", &[]);
    secret.is_secret = true;
    assert!(left(&decide_box(&question_box(vec![secret]), &policy())).contains("secret"));

    let mut begun = open;
    if let BoxKind::Question { begun, .. } = &mut begun.kind {
        *begun = Some("a person typed in its notes".to_string());
    }
    assert!(left(&decide_box(&begun, &policy())).contains("a person typed"));

    let kept_recommended = question_box(vec![q(
        "a",
        "Switch models?",
        &[
            ("Switch, and do not ask again (Recommended)", ""),
            ("Stay", ""),
        ],
    )]);
    assert!(left(&decide_box(&kept_recommended, &policy())).contains("kept for every later ask"));

    let kept_persist = question_box(vec![q(
        "a",
        "Save settings?",
        &[("Save (Recommended)", "Persist to settings for later")],
    )]);
    assert!(left(&decide_box(&kept_persist, &policy())).contains("kept for every later ask"));
}

/// The 2026-10-24 hold (`claude-2.1.310-question-reset-in-description.txt`): an unmarked question
/// whose own text asks nothing irreversible, but whose option 1 describes a bug in words holding
/// `reset`, passes over option 1 and takes the first option that names nothing irreversible.
#[test]
fn a_destructive_word_in_one_options_description_never_holds_the_question() {
    let open = question_box(vec![q(
        "target",
        "Which did you mean by \"no no claude code harness, codex-alab\"?",
        &[
            (
                "Both = codex-alab too",
                "Port the usage limit reset fix to codex-alab as well.",
            ),
            (
                "Split = codex-alab first",
                "Land the codex-alab half first.",
            ),
            ("Neither", "Leave both as they are."),
        ],
    )]);
    let d = decide_box(&open, &policy());
    assert_eq!(chosen(&d), vec![("target".to_string(), Choice::Option(1))]);
    assert!(
        said(&d).contains(CHOSE_FIRST_SAFE) && said(&d).contains("option 1 would reset"),
        "{}",
        said(&d)
    );

    let clean = question_box(vec![q(
        "target",
        "Which did you mean by \"no no claude code harness, codex-alab\"?",
        &[
            (
                "Both = codex-alab too",
                "Port the usage limit ended fix to codex-alab as well.",
            ),
            (
                "Split = codex-alab first",
                "Land the codex-alab half first.",
            ),
        ],
    )]);
    let fallback = decide_box(&clean, &policy());
    assert_eq!(
        chosen(&fallback),
        vec![("target".to_string(), Choice::Option(0))]
    );
    assert!(
        said(&fallback).contains(CHOSE_UNMARKED),
        "{}",
        said(&fallback)
    );

    let every = question_box(vec![q(
        "target",
        "Where should the message band sit?",
        &[
            ("Top", "Drops the legacy table."),
            ("Bottom", "Removes the old band."),
            ("Floating", "Deletes the overlay layer."),
        ],
    )]);
    let held = decide_box(&every, &policy());
    assert!(
        left(&held).contains("every option names a destructive act")
            && left(&held).contains("option 1 would drop"),
        "{held:?}"
    );
}

#[test]
fn the_reset_in_a_description_incident_is_answered_under_every_priority() {
    let open = question_box(vec![q(
        "target",
        "Which did you mean by \"no no claude code harness, codex-alab\"?",
        &[
            (
                "Both = codex-alab too",
                "Port the usage limit reset fix to codex-alab as well.",
            ),
            (
                "Split = codex-alab first",
                "Land the codex-alab half first.",
            ),
        ],
    )]);
    for (questions, rule) in [
        (Questions::Recommended, RULE_ANSWER_RECOMMENDED),
        (Questions::LongTerm, RULE_ANSWER_LONG_TERM),
        (Questions::ShortTerm, RULE_ANSWER_SHORT_TERM),
    ] {
        let p = Policy {
            questions,
            ..policy()
        };
        let d = decide_box(&open, &p);
        assert_eq!(
            chosen(&d),
            vec![("target".to_string(), Choice::Option(1))],
            "{questions:?}"
        );
        assert!(
            matches!(d, BoxDecision::Answer { rule: got, .. } if got == rule),
            "{questions:?}: {d:?}"
        );
    }
}

/// The review of 9744af4ee8: an option that names an act only to decline it (`Don't drop the
/// staging table.`) makes the question that act's, so the harness answers with the one option that
/// refuses it, never option 1 (`Yes / Go ahead with it.`).
#[test]
fn an_option_that_declines_an_act_makes_the_question_that_acts() {
    let sibling = question_box(vec![q(
        "a",
        "Do you want to proceed?",
        &[
            ("Yes", "Go ahead with it."),
            ("No", "Don't drop the staging table."),
        ],
    )]);
    let d = decide_box(&sibling, &policy());
    assert_eq!(chosen(&d), vec![("a".to_string(), Choice::Option(1))]);
    assert!(said(&d).contains(&declined("drop")), "{}", said(&d));

    let branches = question_box(vec![q(
        "a",
        "How should I handle the merged branches?",
        &[
            ("As planned", "Go ahead as described above."),
            ("Keep them", "Do not delete any branch; leave them for you."),
        ],
    )]);
    let d = decide_box(&branches, &policy());
    assert_eq!(chosen(&d), vec![("a".to_string(), Choice::Option(1))]);
    assert!(said(&d).contains(&declined("delete")), "{}", said(&d));

    let also_does = question_box(vec![q(
        "a",
        "Do you want to proceed?",
        &[
            ("Yes", "Go ahead with it."),
            ("Instead", "Don't drop the table: truncate it."),
        ],
    )]);
    assert!(
        left(&decide_box(&also_does, &policy()))
            .contains("names `drop` only to decline it, so the others may drop"),
        "an option that declines one act and does another refuses nothing"
    );

    let negation_ends = question_box(vec![q(
        "a",
        "Do you want to proceed?",
        &[("Yes", "Go ahead with it."), ("No", "No, drop it later.")],
    )]);
    assert!(
        left(&decide_box(&negation_ends, &policy()))
            .contains("every option names a destructive act"),
        "a comma ends the negation: option 2 drops, and option 1's `Yes` approves what it does \
         not say beside it"
    );
}

#[test]
fn an_assent_beside_an_act_is_never_taken_blind() {
    let with_act = question_box(vec![q(
        "a",
        "Where should the message band sit?",
        &[
            ("Yes", "Go ahead with it."),
            ("Bottom", "Removes the old band."),
            ("Floating", "An overlay in the corner."),
        ],
    )]);
    let d = decide_box(&with_act, &policy());
    assert_eq!(chosen(&d), vec![("a".to_string(), Choice::Option(2))]);
    assert!(
        said(&d).contains(CHOSE_FIRST_SAFE)
            && said(&d).contains(
                "option 1 approves what it does not say, beside an option that would remove"
            ),
        "{}",
        said(&d)
    );

    let without_act = question_box(vec![q(
        "a",
        "Where should the message band sit?",
        &[
            ("Yes", "Go ahead with it."),
            ("Bottom", "Below the tab strip."),
            ("Floating", "An overlay in the corner."),
        ],
    )]);
    let d = decide_box(&without_act, &policy());
    assert_eq!(chosen(&d), vec![("a".to_string(), Choice::Option(0))]);
    assert!(said(&d).contains(CHOSE_UNMARKED), "{}", said(&d));
}

#[test]
fn a_kept_option_one_is_passed_over_for_the_first_clean_option() {
    let kept_first = question_box(vec![q(
        "a",
        "Switch models?",
        &[
            ("Switch, and do not ask again", ""),
            ("Stay", "For this message only."),
        ],
    )]);
    let d = decide_box(&kept_first, &policy());
    assert_eq!(chosen(&d), vec![("a".to_string(), Choice::Option(1))]);
    assert!(
        said(&d).contains(CHOSE_FIRST_SAFE)
            && said(&d).contains("option 1 keeps its answer for every later ask, so option 2"),
        "{}",
        said(&d)
    );

    let all_kept = question_box(vec![q(
        "a",
        "Switch models?",
        &[
            ("Switch, and do not ask again", ""),
            ("Stay from now on", ""),
        ],
    )]);
    assert!(
        left(&decide_box(&all_kept, &policy()))
            .contains("option 1 keeps its answer for every later ask")
    );
}

#[test]
fn a_question_asking_an_act_is_refused_by_the_option_that_declines_it() {
    let asked = question_box(vec![q(
        "a",
        "Drop the staging table?",
        &[
            ("Yes", "Drop it now."),
            ("Keep it", "Don't drop it; leave the schema."),
        ],
    )]);
    let d = decide_box(&asked, &policy());
    assert_eq!(chosen(&d), vec![("a".to_string(), Choice::Option(1))]);
    assert!(
        said(&d).contains("it would drop: its one refusal"),
        "{}",
        said(&d)
    );

    let does_it = question_box(vec![q(
        "a",
        "Drop the staging table?",
        &[("Yes", "Drop it now."), ("Keep it", "Drop only its rows.")],
    )]);
    assert!(left(&decide_box(&does_it, &policy())).contains("no one option that refuses it"));
}

#[test]
fn an_options_words_are_read_for_what_they_do_and_what_they_decline() {
    for (text, does, declines) in [
        ("Don't drop the staging table", None, &["drop"][..]),
        ("Do not delete any branch", None, &["delete"]),
        ("nothing gets removed", None, &["remove"]),
        ("Never force-push", None, &["force-push"]),
        ("don't delete or drop anything", None, &["delete", "drop"]),
        ("No, delete it", Some("delete"), &[]),
        ("Don't wait, drop it now", Some("drop"), &[]),
        ("Not only delete the branches", Some("delete"), &[]),
        ("Delete without asking", Some("delete"), &[]),
        ("after the usage limit reset", Some("reset"), &[]),
        (
            "Delete the old ones and don't drop the rest",
            Some("delete"),
            &["drop"],
        ),
        ("Keep everything as it is", None, &[]),
    ] {
        let read = bears(text, "");
        assert_eq!(
            (read.does, read.declines.as_slice()),
            (does, declines),
            "{text}"
        );
    }
}

#[test]
fn a_refusal_is_no_and_never_a_word_that_starts_with_it() {
    for label in [
        "No",
        "no",
        "No, keep it",
        "No — leave it",
        "No (skip)",
        "No.",
    ] {
        assert!(refuses(label), "{label}");
    }
    for label in ["None of the above", "Not now", "Now", "Node", "Yes", ""] {
        assert!(!refuses(label), "{label}");
    }
}

fn plan_box(yes_disabled: Option<&str>) -> OpenBox {
    OpenBox {
        id: "plan-1".to_string(),
        kind: BoxKind::Plan {
            rows: vec![
                (PLAN_YES.to_string(), yes_disabled.map(str::to_string)),
                ("Yes, clear context and implement".to_string(), None),
                ("No, stay in Plan mode".to_string(), None),
            ],
        },
        opened_ms: 0,
    }
}

#[test]
fn the_plan_box_gets_its_yes_only_under_approve_all_and_only_while_enabled() {
    assert_eq!(
        decide_box(&plan_box(None), &policy()),
        BoxDecision::Answer {
            answer: BoxAnswer::Plan {
                id: "plan-1".to_string(),
                row: 0,
            },
            rule: RULE_PLAN,
            words: format!("selected \"{PLAN_YES}\" (approve=all)"),
        }
    );
    let none = Policy {
        approve_all: false,
        ..policy()
    };
    assert!(left(&decide_box(&plan_box(None), &none)).contains("approve"));
    assert!(
        left(&decide_box(
            &plan_box(Some("Default mode unavailable")),
            &policy()
        ))
        .contains("Default mode unavailable")
    );
}

fn goal_box(rows: &[&str], stop: Option<GoalStop>) -> OpenBox {
    OpenBox {
        id: "goal-1".to_string(),
        kind: BoxKind::Goal {
            rows: rows.iter().map(ToString::to_string).collect(),
            stop,
        },
        opened_ms: 0,
    }
}

/// Over a paused or a usage-limited goal the goal box gets its `Leave paused` under every
/// word (Cruise or Land, every switch off), never `Resume goal`: left open it held every root
/// thread's acts back, the usage window's resume included. A blocked goal, and one the TUI
/// said nothing of, are a person's. NEGATIVE CONTROL: a goal box without the row that changes
/// nothing is a person's too.
#[test]
fn the_goal_box_is_left_as_it_was_by_why_the_goal_stopped() {
    let rows = [GOAL_RESUME, GOAL_LEAVE];
    let words = [
        policy(),
        Policy {
            autopilot: crate::decide::Mode::Land,
            ..policy()
        },
        Policy {
            approve_all: false,
            answer_questions: false,
            questions: Questions::Ask,
            continue_on: false,
            resume_limits: false,
            limit_wait: false,
            ..policy()
        },
    ];
    for (stop, stopped) in [
        (GoalStop::Paused, "paused"),
        (GoalStop::UsageLimited, "usage-limited"),
    ] {
        for policy in &words {
            assert_eq!(
                decide_box(&goal_box(&rows, Some(stop)), policy),
                BoxDecision::Answer {
                    answer: BoxAnswer::Goal {
                        id: "goal-1".to_string(),
                        row: 1,
                    },
                    rule: RULE_GOAL_LEAVE,
                    words: format!(
                        "selected \"{GOAL_LEAVE}\" over a {stopped} goal: it stays as it was"
                    ),
                }
            );
        }
    }
    for policy in &words {
        assert!(
            left(&decide_box(
                &goal_box(&rows, Some(GoalStop::Blocked)),
                policy
            ))
            .contains("the goal is blocked")
        );
        assert!(left(&decide_box(&goal_box(&rows, None), policy)).contains("did not say"));
    }
    assert!(
        left(&decide_box(
            &goal_box(&[GOAL_RESUME], Some(GoalStop::Paused)),
            &policy()
        ))
        .contains(GOAL_LEAVE)
    );
    assert!(!crate::answer::records_act(RULE_GOAL_LEAVE));
    assert!(crate::answer::records_act(RULE_PLAN));
}
