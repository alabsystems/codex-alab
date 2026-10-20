// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use codex_alab_core::boxes::BoxAnswer;
use codex_alab_core::boxes::BoxKind;
use codex_alab_core::boxes::BoxOption;
use codex_alab_core::boxes::BoxQuestion;
use codex_alab_core::boxes::Choice;
use codex_alab_core::boxes::OpenBox;
use pretty_assertions::assert_eq;

use crate::answer::BoxDecision;
use crate::answer::PLAN_YES;
use crate::answer::RULE_ANSWER_LONG_TERM;
use crate::answer::RULE_ANSWER_RECOMMENDED;
use crate::answer::RULE_PLAN;
use crate::answer::decide_box;
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
        resume_limits: true,
        limit_wait: true,
        compact_on_context_wall: true,
        upgrade: true,
        human_grace: Duration::from_secs(120),
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

    let kept = question_box(vec![q(
        "a",
        "Switch models?",
        &[("Switch, and do not ask again", ""), ("Stay", "")],
    )]);
    assert!(left(&decide_box(&kept, &policy())).contains("kept for every later ask"));
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
