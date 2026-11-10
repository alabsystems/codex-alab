// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The box answer (CONTRACT §5 rows "question overlay" and "plan box"): pure, a box the
//! TUI's glue reported and the policy in, a [`BoxDecision`] out. Mirrors aterm's
//! `supervise/policy/question.rs` (`choose_one`, `answer_by_role`, `unmarked_pick`: Codex's
//! question is answered by its `(Recommended)` answer, else [`unmarked_pick`]; an unmarked one
//! whose own header or question names a destructive act by its one refusal, else a person; a
//! kept option 1 is passed over for the first clean option, and an answer kept for every later
//! ask is never taken) and `approval.rs` `plan_pick` (`plan-approve@v2`: the yes that grants
//! no standing mode).
//!
//! - `questions=recommended`: each question's option marked `(Recommended)` (the tool's own
//!   convention, `request_user_input_spec.rs`), else [`unmarked_pick`] (option 1 unless it
//!   does a destructive act, keeps its answer, or approves what it does not say beside an
//!   option that names an act, or another option declines an act). A question with no options
//!   gets `answer_text` in its notes, or [`words::REVERSIBLE_ANSWER`] when it names a
//!   destructive act (as a prose question does).
//! - `long-term`/`short-term`: no option is chosen unless the question is unmarked and names a
//!   destructive act in its question or options (which falls back to the recommended option
//!   picker); otherwise the word's text goes in the notes, so the worker decides (aterm writes
//!   it into the question's free-text row).
//! - Under every word, an UNMARKED question whose own header or question text names a
//!   destructive act ([`words::DESTRUCTIVE_WORDS`]) takes its one refusing option (opening
//!   with `No` or declining the act), or is left to a person; an unmarked question where an
//!   option names an act only to decline it takes the one option that refuses every such act,
//!   or is left to a person.
//! - Autopilot Land (`policy.autopilot`, CONTRACT §3) answers a box under its own priority
//!   over every word that answers (`Priority::of`, aterm `approval_loop.rs`
//!   `dialog_priority`): Land's text (aterm's `land_question_text`, else
//!   [`words::LAND_QUESTION_TEXT`]) goes in the notes as the long-term text does, recorded as
//!   [`RULE_ANSWER_LAND`] (aterm `question.rs` `choose_text` under `QuestionPriority::Land`). A
//!   question with no options gets it too where it names a destructive act, as Land answers a
//!   prose question that names one: Cruise's [`words::REVERSIBLE_ANSWER`] ends `and keep
//!   going`, which is no landing, and Land's text itself rules out a force push and deleted
//!   work. An unmarked question that names a destructive act still takes its one refusal, else
//!   a person, under Land's rule (aterm's `destructive_unmarked`, answered by
//!   `recommended_instead`). `ask` and `answer_questions` off stay a person's under Land: Land
//!   answers only where a word already does.
//! - Left to a person (attention): `ask`, `answer_questions` off, a secret question, a person
//!   who began answering, a `(Recommended)` answer kept for every later ask, or an unmarked
//!   question whose every option names a destructive act, keeps its answer, or approves what
//!   it does not say beside an act.
//! - What aterm's text path keeps a person's beside these has no counterpart on the overlay.
//!   The TUI glue applies a posted answer whole, in one step and its notes with it, so no key of
//!   codex-alab's is ever half sent: a switch of the Autopilot after an answer is posted cannot
//!   leave a priority moved mid-dialog (aterm's latch), and one before it is decided afresh at
//!   the next look. A person's typing since codex-alab's answer is the glue's own `begun` refusal.
//!   The overlay takes a note structurally, so a text gone from its row, a row too far from the
//!   focus, Muse Code's notes field and a preview are no rows here.
//! - The plan box: under `approve=all` only, its `Yes, implement this plan` row while
//!   enabled; never `clear context`.
//! - The goal box ("Resume paused goal?", which a resume opens over a goal that is not
//!   active; left open, it held every act back for good, every root thread's), by why the
//!   goal stopped, under every word: over a paused or a usage-limited goal its `Leave
//!   paused`, which changes nothing (a person's pause stands; a usage resume owed goes once the
//!   box is gone, and where the point owes none the turn end's own look raises `goal:`
//!   attention, `extension::goal_left_to_person`), never `Resume goal`. A blocked goal, and one the
//!   TUI said nothing of, are a person's (`goal: left to a person (...)`), as a blocked goal
//!   is at a turn's end (`extension::goal_left_to_person`). The answer is no act of the task
//!   ([`records_act`]).

use codex_alab_core::boxes::BoxAnswer;
use codex_alab_core::boxes::BoxKind;
use codex_alab_core::boxes::BoxOption;
use codex_alab_core::boxes::BoxQuestion;
use codex_alab_core::boxes::Choice;
use codex_alab_core::boxes::GoalStop;
use codex_alab_core::boxes::OpenBox;

use crate::decide::Mode;
use crate::decide::Policy;
use crate::decide::Questions;
use crate::words;

/// aterm's rule ids for a question box's answer (`question.rs`, `approval.rs`).
pub const RULE_ANSWER_RECOMMENDED: &str = "answer-recommended@v1";
pub const RULE_ANSWER_LONG_TERM: &str = "answer-long-term@v1";
pub const RULE_ANSWER_SHORT_TERM: &str = "answer-short-term@v1";
/// Autopilot Land's answer (`approval.rs` `RULE_ANSWER_LAND`), over every word that answers.
pub const RULE_ANSWER_LAND: &str = "answer-land@v1";
/// The plan box's yes (`approval.rs` `RULE_PLAN`).
pub const RULE_PLAN: &str = "plan-approve@v2";
/// The goal box's `Leave paused` (codex-alab's own: aterm's Claude Code lane meets no such box).
pub const RULE_GOAL_LEAVE: &str = "goal-leave@v1";

/// Whether an answer recorded under `rule` is an act of the task (`meta set harness_act`):
/// every answer but the goal box's `Leave paused`, which types nothing and moves nothing,
/// where aterm takes any act it does not know as work reopened (`self_claim_act_face`: a
/// landed or `theirs` task would go back to open with nothing running). It is told and noted
/// only.
pub fn records_act(rule: &str) -> bool {
    rule != RULE_GOAL_LEAVE
}

/// The plan box's row codex-alab accepts (`tui/src/chatwidget/plan_implementation.rs`).
pub const PLAN_YES: &str = "Yes, implement this plan";
/// The goal box's rows (`tui/src/chatwidget/goal_menu.rs`, pinned there by the chat glue's
/// test): codex-alab answers only `Leave paused`.
pub use codex_alab_tui::boxes::GOAL_LEAVE;
pub use codex_alab_tui::boxes::GOAL_RESUME;

/// The label suffix the `request_user_input` tool asks the model to mark its pick with.
const RECOMMENDED: &str = "(recommended)";

/// Ledger label when no option was marked and the first was taken (`question.rs` `CHOSE_UNMARKED`).
pub const CHOSE_UNMARKED: &str = "no option is marked (Recommended): option 1";

/// Ledger label when no option was marked and option 1 was passed over for the first option that
/// names no destructive act and keeps no answer (`question.rs` `CHOSE_FIRST_SAFE`).
pub const CHOSE_FIRST_SAFE: &str = "no option is marked (Recommended), and option 1 is passed over";

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
    "persist to settings",
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
        BoxKind::Goal { rows, stop } => goal(&open.id, rows, *stop),
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
    let Some(priority) = Priority::of(policy) else {
        return BoxDecision::Leave("this session's questions word is `ask`".to_string());
    };
    if let Some(begun) = begun {
        return BoxDecision::Leave(format!("{begun}: theirs to finish"));
    }
    if questions.is_empty() {
        return BoxDecision::Leave("a question box with no question in it".to_string());
    }
    let mut answers = Vec::with_capacity(questions.len());
    let mut said = Vec::with_capacity(questions.len());
    for q in questions {
        match one(q, priority, policy) {
            Ok((choice, words)) => {
                answers.push((q.id.clone(), choice));
                said.push(words);
            }
            Err(why) => return BoxDecision::Leave(why),
        }
    }
    BoxDecision::Answer {
        answer: BoxAnswer::Question {
            id: id.to_string(),
            answers,
        },
        rule: priority.rule(),
        words: format!("answered the question box: {}", said.join("; ")),
    }
}

/// THE PRIORITY A QUESTION BOX IS ANSWERED UNDER (aterm `approval_loop.rs` `dialog_priority`):
/// Land's where the session's Autopilot is Land, over every word that answers; else the
/// `questions` word's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Priority {
    Recommended,
    LongTerm,
    ShortTerm,
    Land,
}

impl Priority {
    /// The priority `policy` answers under, or `None` where its `questions` word is `ask` (a
    /// person's under Land too).
    fn of(policy: &Policy) -> Option<Priority> {
        let word = match policy.questions {
            Questions::Ask => return None,
            Questions::Recommended => Priority::Recommended,
            Questions::LongTerm => Priority::LongTerm,
            Questions::ShortTerm => Priority::ShortTerm,
        };
        Some(if policy.autopilot == Mode::Land {
            Priority::Land
        } else {
            word
        })
    }

    fn rule(self) -> &'static str {
        match self {
            Priority::Recommended => RULE_ANSWER_RECOMMENDED,
            Priority::LongTerm => RULE_ANSWER_LONG_TERM,
            Priority::ShortTerm => RULE_ANSWER_SHORT_TERM,
            Priority::Land => RULE_ANSWER_LAND,
        }
    }

    /// The policy text a priority writes in the notes, so the worker decides, and how the
    /// journal names it; `None` for the recommended word, which chooses an option.
    fn text(self, policy: &Policy) -> Option<(&str, &'static str)> {
        match self {
            Priority::Recommended => None,
            Priority::LongTerm => Some((words::LONG_TERM_TEXT, "the policy text")),
            Priority::ShortTerm => Some((words::SHORT_TERM_TEXT, "the policy text")),
            Priority::Land => Some((&policy.texts.land_question, "the land policy text")),
        }
    }
}

/// One question's choice and how it reads in the journal, or why a person answers it.
fn one(q: &BoxQuestion, priority: Priority, policy: &Policy) -> Result<(Choice, String), String> {
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
        let text = if priority == Priority::Land {
            // Land's text itself rules out a force push and deleted work; Cruise's reversible
            // answer ends `and keep going`, which is no landing.
            policy.texts.land_question.clone()
        } else if destructive(q).is_some() {
            words::REVERSIBLE_ANSWER.to_string()
        } else if let Some((text, _)) = priority.text(policy) {
            text.to_string()
        } else {
            match &policy.answer_text {
                Some(text) => text.clone(),
                None => {
                    return Err(format!(
                        "a free-text question ({subject}) and no answer_text"
                    ));
                }
            }
        };
        return Ok((Choice::Note(text), format!("{subject} → notes")));
    }
    match priority.text(policy) {
        Some((text, named)) if marked.is_some() || destructive(q).is_none() => Ok((
            Choice::Note(text.to_string()),
            format!("{subject} → {named} (the worker decides)"),
        )),
        _ => choose_option(q, subject, marked),
    }
}

fn choose_option(
    q: &BoxQuestion,
    subject: &str,
    marked: Option<usize>,
) -> Result<(Choice, String), String> {
    if let Some(at) = marked {
        let o = &q.options[at];
        kept(o)?;
        return Ok((
            Choice::Option(at),
            format!("{subject} → {} (its recommended option)", o.label),
        ));
    }
    if let Some(w) = asks_to(q) {
        return the_refusal(w, subject, &q.options);
    }
    unmarked_pick(subject, &q.options)
}

/// The destructive word the question's own header or question text names: an act every answer
/// to it bears on (aterm `question.rs` `asks_to`, `answer_by_role`).
fn asks_to(q: &BoxQuestion) -> Option<&'static str> {
    let asked = format!("{}\n{}", q.header, q.question).to_lowercase();
    words::destructive_word(&asked)
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

/// How one option's own words bear on a destructive act ([`words::destructive_word`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Bears {
    /// The first act its words name outside a negation: what taking it would do.
    pub(crate) does: Option<&'static str>,
    /// The acts its words name only under a negation (`Don't drop the staging table`, `Do not
    /// delete any branch`, `nothing gets removed`): acts it declines, which the dialog's other
    /// options may then do.
    pub(crate) declines: Vec<&'static str>,
}

/// Where one clause of an option's words ends: a negation reaches no further.
const CLAUSE_ENDS: &[char] = &[
    '\n', ',', ';', ':', '.', '!', '?', '(', ')', '[', ']', '"', '—', '–',
];

/// Words that negate the act right after them (with any word ending `n't`).
const NEGATORS: &[&str] = &[
    "no", "not", "never", "without", "nothing", "none", "nor", "neither", "dont", "cannot",
];

/// Words a negation reaches across to its act (`do not ever delete`, `nothing gets removed`,
/// `don't delete or drop`); any other word between them ends its reach.
const NEGATION_REACHES: &[&str] = &[
    "is", "are", "was", "were", "be", "been", "being", "will", "would", "should", "gets", "get",
    "got", "to", "ever", "any", "anything", "or",
];

fn negates(word: &str) -> bool {
    NEGATORS.contains(&word) || word.ends_with("n't")
}

/// The destructive act that starts at `words[k]`: the word itself, or it and the next where the
/// act is two words (`rewrite history`, `push -f`).
fn act_at(words: &[&str], k: usize) -> Option<&'static str> {
    words::destructive_word(words[k]).or_else(|| {
        let next = words.get(k + 1)?;
        words::destructive_word(next)
            .is_none()
            .then(|| words::destructive_word(&format!("{} {next}", words[k])))
            .flatten()
    })
}

/// Whether the act at `words[k]` is negated: a negator before it, with only
/// [`NEGATION_REACHES`] words and other acts between them.
fn negated(words: &[&str], k: usize) -> bool {
    (0..k)
        .rev()
        .find(|&i| !(NEGATION_REACHES.contains(&words[i]) || act_at(words, i).is_some()))
        .is_some_and(|i| negates(words[i]))
}

/// What one option's own label and description do and decline ([`Bears`]).
pub(crate) fn bears(label: &str, description: &str) -> Bears {
    let text = format!("{label}\n{description}")
        .to_lowercase()
        .replace('\u{2019}', "'");
    let mut read = Bears::default();
    for clause in text.split(CLAUSE_ENDS) {
        let words: Vec<&str> = clause
            .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '-'))
            .map(|w| w.trim_matches('\''))
            .filter(|w| !w.is_empty())
            .collect();
        for k in 0..words.len() {
            let Some(act) = act_at(&words, k) else {
                continue;
            };
            if negated(&words, k) {
                if !read.declines.contains(&act) {
                    read.declines.push(act);
                }
            } else if read.does.is_none() {
                read.does = Some(act);
            }
        }
    }
    if read.does.is_none() && read.declines.is_empty() {
        read.does = words::destructive_word(&text);
    }
    read
}

/// Label openings that approve without saying what: their act is whatever the rest of the dialog
/// names (`Yes`, `Go ahead`, `As planned`, `Both of the above`).
const ASSENTS: &[&str] = &[
    "yes",
    "ok",
    "okay",
    "sure",
    "go ahead",
    "proceed",
    "continue",
    "approve",
    "confirm",
    "accept",
    "do it",
    "sounds good",
    "as planned",
    "as proposed",
    "as described",
    "as above",
    "both",
    "all of",
    "all the above",
];

/// Words anywhere in an option that point at an act named elsewhere.
const POINTS_ELSEWHERE: &[&str] = &[
    "as planned",
    "as proposed",
    "as described",
    "described above",
    "the above",
    "go ahead",
    "proceed with",
    "the plan",
];

/// Whether one option approves what it does not say ([`ASSENTS`], [`POINTS_ELSEWHERE`]).
fn approves_unsaid(label: &str, description: &str) -> bool {
    let l = label.trim().to_lowercase().replace('\u{2019}', "'");
    let opens = ASSENTS.iter().any(|a| {
        l.strip_prefix(a)
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric()))
    });
    let text = format!(
        "{l}\n{}",
        description.to_lowercase().replace('\u{2019}', "'")
    );
    opens || POINTS_ELSEWHERE.iter().any(|p| text.contains(p))
}

struct OptionReading {
    bears: Bears,
    kept: bool,
    refuses: bool,
    unsaid: bool,
}

fn read_options(options: &[BoxOption]) -> Vec<OptionReading> {
    options
        .iter()
        .map(|o| OptionReading {
            bears: bears(&o.label, &o.description),
            kept: kept(o).is_err(),
            refuses: refuses(&o.label),
            unsaid: approves_unsaid(&o.label, &o.description),
        })
        .collect()
}

pub(crate) fn declined(act: &str) -> String {
    format!(
        "no option is marked (Recommended), and an option names `{act}` only to decline it, so \
         the others may {act}: its one refusal"
    )
}

/// The one option that refuses `w`, the act the question's own header or question text names:
/// it opens with `No` or names `w` only to decline it, and names no act it would do ([`Bears`]).
fn the_refusal(
    w: &'static str,
    subject: &str,
    options: &[BoxOption],
) -> Result<(Choice, String), String> {
    let reads = read_options(options);
    let refusing: Vec<usize> = reads
        .iter()
        .enumerate()
        .filter(|(_, r)| r.bears.does.is_none() && (r.refuses || r.bears.declines.contains(&w)))
        .map(|(at, _)| at)
        .collect();
    match refusing.as_slice() {
        [at] => {
            let o = &options[*at];
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
    }
}

/// The option an unmarked question is answered with, where its own header and question text
/// name no destructive act ([`asks_to`]), and how it was chosen (aterm `question.rs`
/// `unmarked_pick`).
fn unmarked_pick(subject: &str, options: &[BoxOption]) -> Result<(Choice, String), String> {
    let reads = read_options(options);
    let Some(first) = reads.first() else {
        return Err(format!("a question ({subject}) with no option to answer"));
    };
    let mut in_play: Vec<&'static str> = Vec::new();
    for act in reads.iter().flat_map(|r| r.bears.declines.iter().copied()) {
        if !in_play.contains(&act) {
            in_play.push(act);
        }
    }
    if let Some(&act) = in_play.first() {
        let refusing: Vec<usize> = reads
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                r.bears.does.is_none()
                    && !r.kept
                    && (r.refuses || in_play.iter().all(|a| r.bears.declines.contains(a)))
            })
            .map(|(k, _)| k)
            .collect();
        return match refusing.as_slice() {
            [k] => Ok((
                Choice::Option(*k),
                format!("{subject} → {} ({})", options[*k].label, declined(act)),
            )),
            _ => Err(format!(
                "a question ({subject}) with no recommended option where an option names `{act}` \
                 only to decline it, so the others may {act}, and no one option that refuses it: \
                 a person answers that"
            )),
        };
    }
    let named = reads.iter().find_map(|r| r.bears.does);
    let takes =
        |r: &OptionReading| r.bears.does.is_none() && !r.kept && !(named.is_some() && r.unsaid);
    if takes(first) {
        return Ok((
            Choice::Option(0),
            format!("{subject} → {} ({CHOSE_UNMARKED})", options[0].label),
        ));
    }
    let why = match (first.bears.does, named) {
        (Some(w), _) => format!("would {w}"),
        _ if first.kept => "keeps its answer for every later ask".to_string(),
        (None, Some(w)) => {
            format!("approves what it does not say, beside an option that would {w}")
        }
        (None, None) => "cannot be taken".to_string(),
    };
    let Some(k) = reads.iter().position(takes) else {
        return Err(format!(
            "a question ({subject}) with no recommended option whose every option names a \
             destructive act, keeps its answer or approves what it does not say (option 1 \
             {why}), and no one option that refuses it: a person answers that"
        ));
    };
    let how = format!(
        "{CHOSE_FIRST_SAFE}: option 1 {why}, so option {}, the first that names no destructive \
         act and keeps no answer",
        k + 1
    );
    Ok((
        Choice::Option(k),
        format!("{subject} → {} ({how})", options[k].label),
    ))
}

/// aterm `question.rs` `refuses`: `No` itself, or `No` then a separator; never a word that
/// starts with it (`None of these`, `Not now`, `Now`).
pub(crate) fn refuses(label: &str) -> bool {
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

fn goal(id: &str, rows: &[String], stop: Option<GoalStop>) -> BoxDecision {
    let stopped = match stop {
        Some(GoalStop::Paused) => "paused",
        Some(GoalStop::UsageLimited) => "usage-limited",
        Some(GoalStop::Blocked) => {
            return BoxDecision::Leave(
                "the goal is blocked; a person decides how it goes on".to_string(),
            );
        }
        None => {
            return BoxDecision::Leave(
                "the TUI did not say why the goal stopped; a person decides".to_string(),
            );
        }
    };
    let Some(row) = rows.iter().position(|label| label == GOAL_LEAVE) else {
        return BoxDecision::Leave(format!("the goal box has no `{GOAL_LEAVE}`"));
    };
    BoxDecision::Answer {
        answer: BoxAnswer::Goal {
            id: id.to_string(),
            row,
        },
        rule: RULE_GOAL_LEAVE,
        words: format!("selected \"{GOAL_LEAVE}\" over a {stopped} goal: it stays as it was"),
    }
}
