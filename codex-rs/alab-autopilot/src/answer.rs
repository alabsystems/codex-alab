// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The box answer (CONTRACT §5 rows "question overlay" and "plan box"): pure, a box the
//! TUI's glue reported and the policy in, a [`BoxDecision`] out. Mirrors aterm's
//! `supervise/policy/question.rs` (`answer_by_role`: Codex's question is answered by its
//! `(Recommended)` answer, else the first; an unmarked one that names a destructive act by
//! its one refusal, else a person; an answer kept for every later ask, never) and
//! `approval.rs` `plan_pick` (`plan-approve@v2`: the yes that grants no standing mode).
//!
//! - `questions=recommended`: each question's option marked `(Recommended)` (the tool's own
//!   convention, `request_user_input_spec.rs`), else option 1. A question with no options
//!   gets `answer_text` in its notes, or [`words::REVERSIBLE_ANSWER`] when it names a
//!   destructive act (as a prose question does).
//! - `long-term`/`short-term`: no option is chosen; the word's text goes in the notes, so the
//!   worker decides (aterm writes it into the question's free-text row).
//! - Under every word, an UNMARKED question naming a destructive act ([`words::
//!   DESTRUCTIVE_WORDS`]) takes its one refusing option, or is left to a person.
//! - Left to a person (attention): `ask`, `answer_questions` off, a secret question, a person
//!   who began answering, an answer kept for every later ask.
//! - The plan box: under `approve=all` only, its `Yes, implement this plan` row while
//!   enabled; never `clear context`.

use codex_alab_core::boxes::BoxAnswer;
use codex_alab_core::boxes::BoxKind;
use codex_alab_core::boxes::BoxOption;
use codex_alab_core::boxes::BoxQuestion;
use codex_alab_core::boxes::Choice;
use codex_alab_core::boxes::OpenBox;

use crate::decide::Policy;
use crate::decide::Questions;
use crate::words;

/// aterm's rule ids for a question box's answer (`question.rs`, `approval.rs`).
pub const RULE_ANSWER_RECOMMENDED: &str = "answer-recommended@v1";
pub const RULE_ANSWER_LONG_TERM: &str = "answer-long-term@v1";
pub const RULE_ANSWER_SHORT_TERM: &str = "answer-short-term@v1";
/// The plan box's yes (`approval.rs` `RULE_PLAN`).
pub const RULE_PLAN: &str = "plan-approve@v2";

/// The plan box's row codex-alab accepts (`tui/src/chatwidget/plan_implementation.rs`).
pub const PLAN_YES: &str = "Yes, implement this plan";

/// The label suffix the `request_user_input` tool asks the model to mark its pick with.
const RECOMMENDED: &str = "(recommended)";

/// The words of a standing answer, lowercased (aterm `aterm_phase::prompt::KEPT`).
const KEPT: &[&str] = &[
    "don't ask",
    "do not ask",
    "never ask",
    "keep allowing",
    "always allow",
    "from now on",
    "remember this",
    "without asking",
    "never show",
    "from this project",
];

/// What to do with an open box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoxDecision {
    /// Post this answer (after the before-every-act checks); `rule` and `words` are what the
    /// act is recorded as.
    Answer {
        answer: BoxAnswer,
        rule: &'static str,
        words: String,
    },
    /// A person's: raise attention with this reason.
    Leave(String),
}

/// The decision on `open` under `policy`.
pub fn decide_box(open: &OpenBox, policy: &Policy) -> BoxDecision {
    match &open.kind {
        BoxKind::Question {
            questions, begun, ..
        } => question(&open.id, questions, begun.as_deref(), policy),
        BoxKind::Plan { rows } => plan(&open.id, rows, policy),
        BoxKind::Approval { what, .. } => {
            // The contributor cleared this box against the owner's limits when it was asked;
            // the policy may have moved since.
            if !policy.approve_all {
                return BoxDecision::Leave("approve is not all".to_string());
            }
            BoxDecision::Answer {
                answer: BoxAnswer::Approval {
                    id: open.id.clone(),
                },
                rule: crate::extension::RULE_APPROVE,
                words: format!("approved {what} once (approve=all)"),
            }
        }
    }
}

fn question(
    id: &str,
    questions: &[BoxQuestion],
    begun: Option<&str>,
    policy: &Policy,
) -> BoxDecision {
    if !policy.answer_questions {
        return BoxDecision::Leave("answer_questions is off".to_string());
    }
    if policy.questions == Questions::Ask {
        return BoxDecision::Leave("this session's questions word is `ask`".to_string());
    }
    if let Some(begun) = begun {
        return BoxDecision::Leave(format!("{begun}: theirs to finish"));
    }
    if questions.is_empty() {
        return BoxDecision::Leave("a question box with no question in it".to_string());
    }
    let mut answers = Vec::with_capacity(questions.len());
    let mut said = Vec::with_capacity(questions.len());
    for q in questions {
        match one(q, policy) {
            Ok((choice, words)) => {
                answers.push((q.id.clone(), choice));
                said.push(words);
            }
            Err(why) => return BoxDecision::Leave(why),
        }
    }
    let rule = match policy.questions {
        Questions::LongTerm => RULE_ANSWER_LONG_TERM,
        Questions::ShortTerm => RULE_ANSWER_SHORT_TERM,
        Questions::Recommended | Questions::Ask => RULE_ANSWER_RECOMMENDED,
    };
    BoxDecision::Answer {
        answer: BoxAnswer::Question {
            id: id.to_string(),
            answers,
        },
        rule,
        words: format!("answered the question box: {}", said.join("; ")),
    }
}

/// One question's choice and how it reads in the journal, or why a person answers it.
fn one(q: &BoxQuestion, policy: &Policy) -> Result<(Choice, String), String> {
    let subject = if q.header.trim().is_empty() {
        q.question.trim()
    } else {
        q.header.trim()
    };
    if q.is_secret {
        return Err(format!(
            "a secret question ({subject}): a person answers that"
        ));
    }
    let marked = q
        .options
        .iter()
        .position(|o| o.label.trim().to_lowercase().ends_with(RECOMMENDED));
    if q.options.is_empty() {
        let text = if destructive(q).is_some() {
            words::REVERSIBLE_ANSWER.to_string()
        } else {
            match policy.questions {
                Questions::LongTerm => words::LONG_TERM_TEXT.to_string(),
                Questions::ShortTerm => words::SHORT_TERM_TEXT.to_string(),
                Questions::Recommended | Questions::Ask => match &policy.answer_text {
                    Some(text) => text.clone(),
                    None => {
                        return Err(format!(
                            "a free-text question ({subject}) and no answer_text"
                        ));
                    }
                },
            }
        };
        return Ok((Choice::Note(text), format!("{subject} → notes")));
    }
    if marked.is_none()
        && let Some(w) = destructive(q)
    {
        let refusing: Vec<usize> = q
            .options
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                refuses(&o.label) && words::destructive_word(&o.label.to_lowercase()).is_none()
            })
            .map(|(at, _)| at)
            .collect();
        return match refusing.as_slice() {
            [at] => {
                let o = &q.options[*at];
                kept(o)?;
                Ok((
                    Choice::Option(*at),
                    format!("{subject} → {} (it would {w}: its one refusal)", o.label),
                ))
            }
            _ => Err(format!(
                "a question ({subject}) with no recommended option that would {w}, and no \
                 one option that refuses it: a person answers that"
            )),
        };
    }
    match policy.questions {
        Questions::LongTerm | Questions::ShortTerm => {
            let text = if policy.questions == Questions::LongTerm {
                words::LONG_TERM_TEXT
            } else {
                words::SHORT_TERM_TEXT
            };
            Ok((
                Choice::Note(text.to_string()),
                format!("{subject} → the policy text (the worker decides)"),
            ))
        }
        Questions::Recommended | Questions::Ask => {
            let at = marked.unwrap_or(0);
            let o = &q.options[at];
            kept(o)?;
            let how = if marked.is_some() {
                "its recommended option"
            } else {
                "no option is marked (Recommended): option 1"
            };
            Ok((
                Choice::Option(at),
                format!("{subject} → {} ({how})", o.label),
            ))
        }
    }
}

/// The destructive word a question names in its header, question, labels or descriptions.
fn destructive(q: &BoxQuestion) -> Option<&'static str> {
    let mut text = format!("{}\n{}", q.header, q.question).to_lowercase();
    for o in &q.options {
        text.push('\n');
        text.push_str(&o.label.to_lowercase());
        text.push('\n');
        text.push_str(&o.description.to_lowercase());
    }
    words::destructive_word(&text)
}

/// aterm `question.rs` `refuses`: `No` itself, or `No` then a separator; never a word that
/// starts with it (`None of these`, `Not now`, `Now`).
fn refuses(label: &str) -> bool {
    let l = label.trim().to_lowercase();
    l.strip_prefix("no")
        .is_some_and(|rest| rest.chars().next().is_none_or(|c| !c.is_alphanumeric()))
}

/// aterm `question.rs` `kept`: an answer whose words say it is kept for every later ask.
fn kept(o: &BoxOption) -> Result<(), String> {
    let text = format!("{}\n{}", o.label, o.description)
        .replace('\u{2019}', "'")
        .to_lowercase();
    if KEPT.iter().any(|w| text.contains(w)) {
        Err(format!(
            "a question whose answer `{}` is kept for every later ask: a person answers that",
            o.label
        ))
    } else {
        Ok(())
    }
}

fn plan(id: &str, rows: &[(String, Option<String>)], policy: &Policy) -> BoxDecision {
    if !policy.approve_all {
        return BoxDecision::Leave("approve is not `all`: the plan is a person's".to_string());
    }
    let Some(at) = rows.iter().position(|(label, _)| label == PLAN_YES) else {
        return BoxDecision::Leave(format!("the plan box has no `{PLAN_YES}`"));
    };
    if let Some(why) = &rows[at].1 {
        return BoxDecision::Leave(format!("`{PLAN_YES}` is disabled: {why}"));
    }
    BoxDecision::Answer {
        answer: BoxAnswer::Plan {
            id: id.to_string(),
            row: at,
        },
        rule: RULE_PLAN,
        words: format!("selected \"{PLAN_YES}\" (approve=all)"),
    }
}
