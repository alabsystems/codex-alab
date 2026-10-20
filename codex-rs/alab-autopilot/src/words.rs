// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! aterm's words and texts, ported verbatim so codex-alab answers a point exactly as aterm's
//! lane would.
//!
//! PROVENANCE: `~/aterm/crates/aterm-agent/src/supervise/policy/turn_end.rs` (`DONE_CHECK`,
//! `FINISHED`, `says_finished`, `DONE_PHRASES`, `says_done`, `STOP_PHRASES`,
//! `CHOICE_PHRASES`, `DESTRUCTIVE_WORDS`, `OFFER_PHRASES`, `RECOMMEND_PHRASES`,
//! `CHOICE_LEADS`, `classify_said` and its helpers, `REVERSIBLE_ANSWER`, `CARRY_ON`, the
//! rule ids) and `question.rs` (`LONG_TERM_TEXT`, `SHORT_TERM_TEXT`), aterm commit
//! `3f0b07191`. Change them only by porting aterm's change; the per-thread `LastTail` memo
//! aterm wraps the pure readings in is left out (a pure cache, not a behavior).

/// A continuation typed at an ordinary turn end.
pub const RULE_CONTINUE: &str = "continue@v1";
/// The [`DONE_CHECK`] typed where the worker said it is done.
pub const RULE_DONE_CHECK: &str = "continue-done-check@v1";
/// A task the worker ended with [`FINISHED`].
pub const RULE_FINISHED: &str = "finished@v1";
/// `answer_text` (or the priority word's policy text), typed where the worker asked.
pub const RULE_ANSWER: &str = "answer@v1";
/// A prose question answered under `long-term`.
pub const RULE_PROSE_LONG_TERM: &str = "prose-long-term@v1";
/// A prose question answered under `short-term`.
pub const RULE_PROSE_SHORT_TERM: &str = "prose-short-term@v1";
/// A continuation after an API error's wait.
pub const RULE_API_RETRY: &str = "api-retry@v1";
/// A continuation after a reply the connection cut off.
pub const RULE_API_CUTOFF: &str = "api-cutoff@v1";
/// The continuation at a usage window's reset.
pub const RULE_LIMIT_RESUME: &str = "usage-resume@v1";
/// The compaction on a full context, and the continuation after it.
pub const RULE_COMPACT: &str = "context-compact@v1";
/// A lost login (escalated: Codex signs in outside its TUI).
pub const RULE_LOGIN: &str = "auth-login@v1";
/// A turn the session's host typed (aterm's carry-on after a relaunch).
pub const RULE_HOST_TURN: &str = "harness-turn@v1";

/// Typed in place of the continuation when the worker says it is done ([`says_done`]).
/// The reply it asks for is itself a done report.
pub const DONE_CHECK: &str = "If the task is finished and verified, reply with ATERM-TASK-DONE on a line by itself \
     and stop; otherwise continue.";

/// The worker's explicit done token: a last line that is exactly this (Markdown
/// punctuation allowed).
pub const FINISHED: &str = "ATERM-TASK-DONE";

/// Whether the worker's last words end with [`FINISHED`] on a line of its own: the
/// explicit, unambiguous done. A token quoted inside a sentence, or followed by more text,
/// is not it.
#[must_use]
pub fn says_finished(tail: Option<&str>) -> bool {
    tail.and_then(|t| t.lines().rev().find(|l| !l.trim().is_empty()))
        .is_some_and(|last| {
            last.trim_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) == FINISHED
        })
}

/// The words a worker says it is DONE with, lowercased, matched anywhere in its last words
/// ([`says_done`]).
pub const DONE_PHRASES: &[&str] = &[
    "nothing left to do",
    "nothing else to do",
    "nothing more to do",
    "nothing further to do",
    "nothing left to work on",
    "no further work",
    "no remaining work",
    "no more work",
    "all done",
    "all tasks are complete",
    "all tasks complete",
    "everything is done",
    "everything is already done",
    "everything is complete",
    "already complete",
    "work is complete",
    "work is done",
    "task is complete",
    "workflow is finished",
    "workflow is complete",
    "completed successfully",
    "what would you like me to work on",
    "what would you like me to do",
    "what should i work on",
    "what else would you like",
    "let me know if you need anything",
    "let me know if there's anything else",
    // A finished report that asks only what comes next.
    "what's next?",
    "anything else?",
];

/// Whether the worker's last words say it is done ([`DONE_PHRASES`]) or are exactly `DONE`
/// (punctuation allowed). Title-case `Done.` is a step report.
#[must_use]
pub fn says_done(tail: Option<&str>) -> bool {
    let Some(t) = tail else {
        return false;
    };
    if says_finished(Some(t)) || t.trim_matches(|c: char| !c.is_alphanumeric()) == "DONE" {
        return true;
    }
    let flat = t
        .to_lowercase()
        .replace('\u{2019}', "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    DONE_PHRASES.iter().any(|p| flat.contains(p))
}

/// Phrases a worker asks a person for a decision with: answered ([`RULE_ANSWER`]), never
/// continued; [`REVERSIBLE_ANSWER`] when a destructive act ([`DESTRUCTIVE_WORDS`]) is named.
/// Lowercased, anywhere.
pub const STOP_PHRASES: &[&str] = &[
    "need your decision",
    "needs your decision",
    "need a decision",
    "your decision on",
    "blocked on",
    "i'm blocked",
    "i am blocked",
    "waiting on you",
    "waiting for you",
    "waiting on your",
    "waiting for your",
    // Names the person's authority rather than listing options: a stop, not
    // a choice ([`CHOICE_PHRASES`]).
    "your call",
    "need your input",
    "need your approval",
    "need you to",
    "please confirm",
    "can you confirm",
    "please advise",
    "how would you like me to proceed",
    "cannot proceed without",
    "can't proceed without",
    // The go-ahead, sign-off and review asks, and the
    // ways a worker says it will wait for one.
    "your go-ahead",
    "your go ahead",
    "the go-ahead",
    "a go-ahead",
    "sign off",
    "sign-off",
    "signoff",
    "for your review",
    "ready for review",
    "ready for your review",
    "please review",
    "your approval",
    "needs approval",
    "need approval",
    "how you'd like",
    "how you would like",
    "how would you like",
    "what you'd like me to",
    "let me know how",
    "let me know whether",
    "i'll wait",
    "i will wait",
    "i'll hold off",
    "holding off until",
    "before i proceed",
    "before i continue",
    "before i go ahead",
    "before i run",
    "before i push",
    "before i force",
    "before i delete",
    "before i merge",
    "before i deploy",
    "before i release",
];

/// Phrases a worker asks a person to choose with: a stop, answered ([`RULE_ANSWER`]), only
/// where the phrase asks ([`phrase_asks`]), never in a report.
pub const CHOICE_PHRASES: &[&str] = &[
    "which option",
    "which would you prefer",
    "which do you prefer",
    "let me know which",
];

/// Words that make an offer, stop, choice or question one a person must answer: answered
/// with [`REVERSIBLE_ANSWER`], never continued (`keep going` would read as consent). Whole
/// words after the offer's phrase. Plain `push` is excluded: `push it when green` is an
/// accepted continuation.
pub const DESTRUCTIVE_WORDS: &[&str] = &[
    "force",
    "force-push",
    "--force",
    "delete",
    "deleting",
    "deletion",
    "drop",
    "dropping",
    "remove",
    "removing",
    "removal",
    "rm",
    "deploy",
    "deploying",
    "migrate",
    "migrating",
    "publish",
    "publishing",
    "release",
    "releasing",
    "wipe",
    "purge",
    "reset",
    "overwrite",
    "truncate",
    "destroy",
    "uninstall",
    "revert",
    "rewrite history",
];

/// Words that make a question an offer to go on (it counts as a continuation): `want me to
/// take that on?`.
pub const OFFER_PHRASES: &[&str] = &[
    "want me to",
    "shall i",
    "should i continue",
    "should i proceed",
    "should i keep going",
    "should i go ahead",
    // An offer of more work, not a yes/no about the work done.
    "should i also",
    "would you like me to",
    "next steps:",
    "next step:",
    "i can also",
    "if you'd like, i can",
    "let me know if you'd like",
    "happy to",
];

/// Words that recommend one option: a choice with a recommendation is the worker's to take.
pub const RECOMMEND_PHRASES: &[&str] = &[
    "i recommend",
    "i'd recommend",
    "i would recommend",
    "recommended",
    "my recommendation",
    "i suggest",
    "i'd suggest",
    "i'd go with",
    "i'll go with",
    "i'd pick",
    "i lean",
];

/// Leads that, before an either/or's ` or `, make it a choice the worker would act on.
pub const CHOICE_LEADS: &[&str] = &[
    "should i",
    "shall i",
    "should we",
    "shall we",
    "want me to",
    "do you want",
    "would you like",
    "prefer",
    "which",
];

/// The answer to a question or choice naming an irreversible act ([`Said::Irreversible`]):
/// never "take the option you would recommend" (the recommendation may be the act), and
/// never `answer_text`, which may say so.
pub const REVERSIBLE_ANSWER: &str = "Nobody is here to approve an irreversible step: take the \
                                     option that deletes, overwrites and force-pushes nothing, \
                                     and keep going.";

/// The `long-term` policy text (aterm `question.rs`).
pub const LONG_TERM_TEXT: &str = "Decide yourself: choose the option that best serves long term \
                                  correctness and total value even at the cost of short term \
                                  deliverables. Say which you chose and why in one line and \
                                  continue without asking again. (Answered by the aterm harness \
                                  by policy. The user was not asked.)";

/// The `short-term` policy text (aterm `question.rs`).
pub const SHORT_TERM_TEXT: &str = "Decide yourself: choose the option that gets the current \
                                   deliverable done soonest at acceptable risk. Say which you \
                                   chose and why in one line and continue without asking again. \
                                   (Answered by the aterm harness by policy. The user was not \
                                   asked.)";

/// What every wall act ends with: where to carry on, and not to repeat a step whose result
/// the failure may have eaten.
pub const CARRY_ON: &str = "Carry on from where you stopped; if the result of your last step is \
missing, check whether it ran before you repeat it.";

/// What the worker's last words ask of a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    /// A report, or nothing: the point may be continued.
    Plain,
    /// An offer to go on (`want me to take that on?`): continued.
    Offer,
    /// A question for a person: answered.
    Question,
    /// A decision asked for: answered, with why.
    Stop(String),
    /// A decision that names an irreversible act: answered only with [`REVERSIBLE_ANSWER`].
    Irreversible(String),
}

/// Judges the worker's last words ([`Said`]) exactly as aterm's `classify_said` does.
#[must_use]
pub fn classify_said(tail: Option<&str>) -> Said {
    let Some(tail) = tail else {
        return Said::Plain;
    };
    classify_said_read(tail)
}

fn classify_said_read(tail: &str) -> Said {
    let lower = yes_no_asks(&tail.to_lowercase().replace('\u{2019}', "'"));
    let flat = lower.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(p) = STOP_PHRASES.iter().find(|p| flat.contains(*p)) {
        if let Some(w) = destructive_word(&flat) {
            return Said::Irreversible(format!("the worker said \"{p}\" of an act that would {w}"));
        }
        return Said::Stop(format!("the worker said \"{p}\""));
    }
    let recommends = RECOMMEND_PHRASES.iter().any(|p| flat.contains(p));
    let offer_at = OFFER_PHRASES.iter().filter_map(|p| flat.find(p)).min();
    if let Some(at) = offer_at
        && let Some(w) = destructive_word(&flat[at..])
    {
        return Said::Irreversible(format!("the worker offers to {w}"));
    }
    if let Some(w) = questions(&flat).into_iter().find_map(destructive_word) {
        return Said::Irreversible(format!("the worker asks whether to {w}"));
    }
    let offer = offer_at.is_some();
    let lines: Vec<&str> = lower
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let unit_start = lines.iter().rposition(|l| is_option_line(l)).unwrap_or(0);
    let unit = lines[unit_start..].join(" ");
    let unit = without_trailing_aside(&unit).trim_end_matches([')', '"', '*', '`', ' ']);
    let asks = unit.ends_with('?');
    let listed = lines.iter().filter(|l| is_option_line(l)).count();
    let ask = last_sentence(unit);
    let listed_choice = listed >= 2 && asks_to_choose(ask);
    let chooses = asks && (listed_choice || either_or(ask));
    let choice_phrase = CHOICE_PHRASES
        .iter()
        .filter_map(|p| flat.find(p).map(|at| (*p, at)))
        .min_by_key(|&(_, at)| at);
    let phrase_choice = choice_phrase.filter(|&(p, at)| phrase_asks(&flat, &lines, p, at));
    if (chooses || phrase_choice.is_some())
        && let Some(w) = destructive_word(&flat)
    {
        return Said::Irreversible(format!("the worker offers a choice that would {w}"));
    }
    if chooses && !recommends {
        return Said::Stop(if listed_choice {
            "the worker listed options with no recommendation".to_string()
        } else {
            "the worker asks to choose between options".to_string()
        });
    }
    if let Some((p, _)) = phrase_choice {
        return Said::Stop(format!("the worker said \"{p}\""));
    }
    if let Some((_, at)) = choice_phrase
        && let Some(w) = destructive_word(&flat[at..])
    {
        return Said::Irreversible(format!("the worker asks to choose whether to {w}"));
    }
    if !asks {
        return if offer { Said::Offer } else { Said::Plain };
    }
    if offer { Said::Offer } else { Said::Question }
}

/// The yes/no markers a worker closes a question with, lowercased.
const YES_NO_MARKERS: &[&str] = &["(y/n)", "[y/n]", "(yes/no)", "[yes/no]", "(y/n)?", "[y/n]?"];

fn yes_no_asks(text: &str) -> String {
    let mut out = text.to_string();
    for m in YES_NO_MARKERS {
        out = out.replace(m, "?");
    }
    out
}

fn questions(flat: &str) -> Vec<&str> {
    let bytes = flat.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    for (i, b) in bytes.iter().enumerate() {
        let ends = matches!(bytes.get(i + 1), None | Some(b' '));
        if !ends {
            continue;
        }
        match b {
            b'?' => {
                out.push(flat[start..=i].trim());
                start = i + 1;
            }
            b'.' | b'!' | b';' | b':' => start = i + 1,
            _ => {}
        }
    }
    out
}

fn without_trailing_aside(unit: &str) -> &str {
    let mut t = unit.trim_end();
    while t.ends_with(')') {
        let Some(open) = t.rfind('(') else {
            break;
        };
        let before = t[..open].trim_end();
        if !before.ends_with(['?', '.', '!']) {
            break;
        }
        t = before;
    }
    t
}

fn last_sentence(text: &str) -> &str {
    let body = text.trim_end_matches('?');
    let mut start = 0;
    let bytes = body.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if matches!(b, b'.' | b'!' | b'?' | b':' | b';') && bytes.get(i + 1) == Some(&b' ') {
            start = i + 1;
        }
    }
    text[start..].trim()
}

fn either_or(ask: &str) -> bool {
    ask.find(" or ")
        .is_some_and(|at| CHOICE_LEADS.iter().any(|l| ask[..at].contains(l)))
}

fn asks_to_choose(ask: &str) -> bool {
    let words: Vec<&str> = ask
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .collect();
    either_or(ask)
        || CHOICE_LEADS.iter().any(|l| ask.contains(l))
        || words.iter().any(|w| {
            matches!(
                *w,
                "which" | "option" | "options" | "choose" | "pick" | "either" | "or"
            )
        })
}

fn phrase_asks(flat: &str, lines: &[&str], p: &str, at: usize) -> bool {
    let rest = &flat[at..];
    let bytes = rest.as_bytes();
    let end = bytes.iter().enumerate().position(|(i, b)| {
        matches!(b, b'.' | b'!' | b'?')
            && matches!(bytes.get(i + 1), None | Some(b' '))
            && !(*b == b'.' && i > 0 && bytes[i - 1].is_ascii_digit())
    });
    let asks_here = end.is_none_or(|i| bytes[i] == b'?' || i + 1 == bytes.len());
    let introduces = lines
        .iter()
        .position(|l| l.contains(p))
        .is_some_and(|k| lines.get(k + 1).is_some_and(|l| is_option_line(l)));
    asks_here || introduces
}

/// The first of [`DESTRUCTIVE_WORDS`] in `text` as a whole word (or phrase), or an
/// inflection of one, returned as its base form.
#[must_use]
pub fn destructive_word(text: &str) -> Option<&'static str> {
    let words: Vec<&str> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '-'))
        .filter(|w| !w.is_empty())
        .map(|w| w.trim_start_matches('-'))
        .collect();
    DESTRUCTIVE_WORDS.iter().copied().find(|d| {
        let want: Vec<&str> = d.split(' ').map(|w| w.trim_start_matches('-')).collect();
        words.windows(want.len()).any(|win| {
            win.iter()
                .zip(want.iter())
                .all(|(w, base)| inflects(w, base))
        })
    })
}

fn inflects(word: &str, base: &str) -> bool {
    if word == base {
        return true;
    }
    let Some(rest) = word.strip_prefix(base) else {
        let stem = base.strip_suffix('e').unwrap_or(base);
        return (stem.len() < base.len()
            && word
                .strip_prefix(stem)
                .is_some_and(|rest| matches!(rest, "ing" | "ed" | "ten")))
            || (base.ends_with("write") && word == format!("{}ote", &base[..base.len() - 3]));
    };
    if matches!(rest, "s" | "es" | "d" | "ed" | "ing" | "ten") {
        return true;
    }
    let last = base.chars().last().unwrap_or(' ');
    rest.strip_prefix(last)
        .is_some_and(|rest| matches!(rest, "ed" | "ing" | "en"))
}

fn is_option_line(line: &str) -> bool {
    let t = line.trim_start();
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && t[digits..].starts_with(['.', ')']) {
        return true;
    }
    let mut cs = t.chars();
    if let (Some(a), Some(b)) = (cs.next(), cs.next())
        && a.is_ascii_lowercase()
        && b == ')'
    {
        return true;
    }
    t.starts_with("(a)")
        || t.starts_with("(b)")
        || t.starts_with("- ")
        || t.starts_with("• ")
        || t.starts_with("option ")
}

/// aterm's `wall_retry_text` for Codex: the wall named in aterm's own words, never a row of
/// the screen or the error's text (aterm ruling 2026-10-08, H6-HARNESS-1).
#[must_use]
pub fn wall_retry_text(what: &str, cut_off: bool, unreachable: bool) -> String {
    let who = "Codex";
    if cut_off {
        format!("{who} reported {what}, so your last reply may be incomplete. {CARRY_ON}")
    } else if unreachable {
        format!("{who} reported {what}; trying again. {CARRY_ON}")
    } else {
        format!("{who} reported {what}. {CARRY_ON}")
    }
}
