// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! aterm's policies and texts, with native codex-alab attribution, so this binary answers
//! a point under FOREVER CRUISE / FOREVER LAND (aterm owner direction 2026-10-19: "we
//! are deprecating the ATERM-TASK-DONE logic and prompts in favor of FOREVER CRUISE or FOREVER
//! LAND"; "Cruise is always looking for more completeness and perfection. Land is trying to
//! wrap it up to end the session cleanly").
//!
//! PROVENANCE: `~/aterm/crates/aterm-agent/src/supervise/policy/turn_end.rs` (the rule ids,
//! `DONE_CHECK`, `DONE_CHECK_REVERSIBLE`, `LAND`, `LAND_NUDGE`, `LAND_ANSWER`, `FINISHED`,
//! `is_finished_line`, `clean_defense`, `land_open_words`, `is_land_rule`,
//! `reversible_clause`, `ack_clause`, `act_words`, `fitted_act`, `Suffix`, `with_rules`,
//! `fit_units`, `FACT_RIDES` and `fact_rides` (here [`with_fact`]; gap 23, aterm main
//! `692e87f835`), `repo_work_suffix` (here [`repo_suffix`]), `DONE_PHRASES`, `says_done` and
//! its readers, `STOP_PHRASES`, `CHOICE_PHRASES`, `DESTRUCTIVE_WORDS`, `OFFER_PHRASES`,
//! `RECOMMEND_PHRASES`, `CHOICE_LEADS`, `classify_said` and its helpers, `REVERSIBLE_ANSWER`,
//! `CARRY_ON`) and `question.rs` (`LONG_TERM_TEXT`, `SHORT_TERM_TEXT`), DIFFED WORD FOR WORD
//! AGAINST aterm main `6ff0b5475a` (2026-10-20), reader by reader and text by text, with the
//! needs reader (`crate::needs`), the strict reading (`crate::open_work`), the repository
//! probe (`crate::repo`) and the three labelled tables (`fixtures/`), whose bodies are
//! aterm's byte for byte. What that diff found ported since: the fences a report's readers
//! skip (aterm `96bd2d4567`), the none clause's UTF-8 offsets (`cee3ea7afb`), a force-push
//! however it is spelled (`a9f89afaa0`, `1548dac3bf`), the probe's main ref and unlanded rule,
//! the repository suffix's cut and bare form, the order of an act's suffixes and its one
//! copy of the standing rules after them, fitted to aterm's verbatim bound (`fitted_act`),
//! the repository named at a Land claim, `says_finished` retired (`448726c354`), and Land's
//! no-repository sentence in [`LAND`] and [`LAND_NUDGE`] (aterm `2b871cf8a1`, `72aa7380fe`).
//! The readers were then diffed item by item against aterm main `66894933d2` (2026-10-24):
//! [`clean_defense`], `land_open_words` and [`says_done`] were main's already, and what the
//! strict reading and the needs reader lacked is ported there (aterm `4cacfb7913`,
//! `ac0c76b137`, `149efe1190`; `2a290f10f7`, `93de0197f9`), with the tables and the blind
//! holdout they read. The probe's unread result (`e32ba59da2`) is also ported:
//! [`Suffix::Repo`] and [`fitted_act`] preserve the warning separately from actual work.
//! Before: the FOREVER CRUISE / FOREVER LAND policy core of aterm `a355109905`, the texts as
//! the spec's §2 gives them, `CARRY_ON` as `448726c354` amended it (it ends on the NEEDS YOU
//! sentence, then aterm `11e4a76a8d`'s restart sentence), and the readers as aterm main
//! `e0da8d68df` had them. [`repo_suffix`] is rule 1 of the spec's §9. The retired words aterm
//! main keeps for the records its older builds wrote
//! (`RETIRED_DONE_CHECKS`, `RETIRED_LANDS`, `RETIRED_LAND_NUDGES`, `RETIRED_LAND_ANSWERS`)
//! have no counterpart here: codex-alab reads its own Land acts by their client id and rule,
//! never by their words, so it keeps no retired copy, not even of the remote-main words it
//! typed before aterm `2b871cf8a1`. Change them only by porting aterm's change. aterm composes its standing rules (`with_rules`): the `harness policy` text keys
//! arrive composed (`Policy::texts`), these copies are the fallback for an aterm that sends
//! none, and [`fitted_act`] splits the rules off a received text ([`split_rules`]) to type
//! an act's suffixes before them as aterm does; an act whose words arrive bare (an answer)
//! takes the least-cut copy the composed texts carry ([`standing_rules`]). [`classify_said`] reads the words without
//! their NEEDS YOU lines, as aterm's one caller of it (`asked_by`) passes them. The per-thread
//! `LastTail` memo aterm wraps the pure readings in is left out (a pure cache, not a
//! behavior), and so are the readers only aterm's screen and record scans use.

use std::borrow::Cow;

use crate::needs;
use crate::open_work::OPEN_NAMED;
use crate::open_work::open_named;
use crate::open_work::open_work_strict;
use crate::repo::RepoRead;
use crate::repo::RepoWork;

/// Label native automatic turns with the binary that sends them. Attribution
/// uses native client IDs, so older transcript labels remain readable.
macro_rules! harness_mark {
    () => {
        "[codex-alab]"
    };
}
pub(crate) use harness_mark;

/// A continuation typed at an ordinary turn end.
pub const RULE_CONTINUE: &str = "continue@v1";
/// A repeated report changes approach instead of ending autonomous work.
pub const FORWARD_RECOVERY: &str = concat!(
    harness_mark!(),
    " Continue autonomously. This approach is not making progress. Decide on a different safe approach and record the decision for another AI to revisit. If this work cannot proceed, defer it with the reason and advance another useful task. Do not ask the person to unblock routine work. Keep moving."
);
/// A single continuation for independently allowed work after a cybersecurity refusal.
/// It never retries the refused action or changes the account's access program.
pub const RULE_POLICY_CONTINUE: &str = "policy-continue@v1";
pub const POLICY_EXHAUSTED: &str = "POLICY-WORK-EXHAUSTED";
pub const POLICY_BOUNDARY: &str = "A cybersecurity policy refused an earlier action. Leave that action excluded: do not retry, rephrase, split, disguise, delegate or seek another model, account or access program to perform it. Continue only clearly allowed, independently authorized work. If no such work remains, explain the limitation, put POLICY-WORK-EXHAUSTED on a line by itself and stop; do not invent a workaround.";
pub const POLICY_CONTINUE: &str = concat!(
    harness_mark!(),
    " Respect the cybersecurity policy refusal and abandon the refused action. ",
    "Do not repeat or rephrase the rejected request, seek a workaround, change model or account, or enroll in or enable Daybreak. ",
    "Preserve existing work and continue only clearly allowed, independently authorized work. ",
    "If no such work remains, explain the limitation, put POLICY-WORK-EXHAUSTED on a line by itself and stop."
);
/// Cruise's defend-and-find-more prompt ([`DONE_CHECK`]), typed where the worker claims it
/// is done. Nothing its yield says ends the task (FOREVER CRUISE); the id is the one older
/// books know the check by.
pub const RULE_DONE_CHECK: &str = "continue-done-check@v1";
/// RETIRED: the act older builds recorded where the worker's [`FINISHED`] ended the task.
/// No end is made by it now; kept so older books read.
pub const RULE_FINISHED: &str = "finished@v1";
/// Land's instruction ([`LAND`]), typed at the first point of a Land episode.
pub const RULE_LAND: &str = "land@v1";
/// A nudge after the land instruction ([`LAND_NUDGE`]): at every free point of the episode
/// until the work is landed, with no bound (FOREVER LAND), paced by the short streak.
pub const RULE_LAND_NUDGE: &str = "land-nudge@v1";
/// The answer a question gets under Land ([`LAND_ANSWER`]).
pub const RULE_LAND_ANSWER: &str = "land-answer@v1";
/// THE LANDING: a clean defense at a land act's yield with the repository clean
/// ([`clean_defense`], `repo::probe`). Recorded as codex-alab's act (`harness_act`), never
/// typed: nothing more is typed into the task until someone else's turn.
pub const RULE_LANDED: &str = "landed@v1";
/// The first public aterm with the landing spelling: `20240ae119` is first contained by
/// `v0.108.0`; `v0.107.0` has no `landed@v1`. Older hosts cannot acknowledge a landing rightly.
pub const LANDED_UNBOUNDED_FROM: (u64, u64) = (0, 108);
/// An unsupported landing leaves the work open, with one update advisory rather than a
/// metadata-only timer that would keep the live upgrade waiting forever.
pub fn landing_needs_newer_aterm(aterm: &str) -> String {
    format!(
        "aterm {aterm} predates landing records ({ATERM_ACT_LEASE}); the work remains open and codex-alab continues"
    )
}
/// THE POINT LEFT TO A PERSON'S OWN EXCHANGE (aterm's `turn_end::RULE_LEFT`, its LEFT row):
/// recorded as codex-alab's act (`harness_act`) once for a point left to the person (Land's
/// opening exchange, or one an Off left: gap 32), never typed, so aterm's face for the proven
/// self-claim reads `theirs` there until someone else's turn (FOREVER LAND §10.4).
pub const RULE_LEFT: &str = "left-to-person@v1";
/// The first aterm that reads [`RULE_LEFT`] as `theirs` and opens it again at the next turn
/// begun past it (0.109.0: aterm 2380aceb85, 8e1292d320): the floor for its unbounded
/// spelling where aterm has no act lease. Before it, aterm read any act but a landing as
/// `open` and logged it as an act taken.
pub const LEFT_UNBOUNDED_FROM: (u64, u64) = (0, 109);
/// The first aterm with the act lease (`meta prepare harness_act`, aterm bf9d401e40), which
/// fences a delayed [`RULE_LEFT`]: the release a person is told to update to.
pub const ATERM_ACT_LEASE: &str = "0.110.0";
/// What a person is told, once per process, where aterm `{aterm}` is too old to show a person's
/// own exchange as theirs: loud, and never blocking (nothing waits on it).
pub fn left_needs_newer_aterm(aterm: &str) -> String {
    format!(
        "aterm {aterm} predates act leases ({ATERM_ACT_LEASE}); the person's exchange remains local, and other work continues"
    )
}
/// `answer_text` (or the priority word's policy text), typed where the worker asked.
pub const RULE_ANSWER: &str = "answer@v1";
/// A prose question answered under `long-term`.
pub const RULE_PROSE_LONG_TERM: &str = "prose-long-term@v1";
/// A prose question answered under `short-term`.
pub const RULE_PROSE_SHORT_TERM: &str = "prose-short-term@v1";
/// A continuation after an API error's wait.
pub const RULE_API_RETRY: &str = "api-retry@v1";
/// A continuation once codex-alab MEASURES the API reachable again (`netwatch`: a fresh
/// verified handshake to the thread's own route), after an API error that never reached it
/// (aterm's `RULE_API_BACK`).
pub const RULE_API_BACK: &str = "api-back@v1";
/// A continuation after a reply the connection cut off.
pub const RULE_API_CUTOFF: &str = "api-cutoff@v1";
/// The continuation at a usage window's reset.
pub const RULE_LIMIT_RESUME: &str = "usage-resume@v1";
/// The compaction on a full context, and the continuation after it.
pub const RULE_COMPACT: &str = "context-compact@v1";
/// A lost login (escalated: Codex signs in outside its TUI).
pub const RULE_LOGIN: &str = "auth-login@v1";
/// A LOST LOGIN'S TURN DELIVERED AGAIN once codex-alab saw the login come back (aterm's
/// `RULE_LOGIN_BACK`): the same sign-in renewed in place, read through codex's own
/// `AuthManager` (`crate::login`), typed once per wall ([`LOGIN_BACK`]).
pub const RULE_LOGIN_BACK: &str = "login-back@v1";
/// A turn the session's host typed (aterm's carry-on after a relaunch).
pub const RULE_HOST_TURN: &str = "harness-turn@v1";
/// THE LIVE UPGRADE'S READINESS NOTICE under aterm's `upgrade_ask = true` ([`upgrade_notice`];
/// amended 2026-10-28 after the parity sweep, gap 27): aterm types its own as a host's turn
/// (`HostTurn::Notice`, booked under its `RULE_HOST_TURN`), which this rule is apart from so
/// that neither the carry-on's chain nor its land act reads it.
pub const RULE_UPGRADE_NOTICE: &str = "upgrade-notice@v1";

/// THE ACTS AN ATERM FACT MAY RIDE (CONTRACT §5 "aterm fact", gap 23), as a pre-filter only
/// (aterm's `policy::turn_end::FACT_RIDES`, verbatim): only these ask. aterm judges every read
/// itself (`why=rule`, `why=limit`), so a list that drifts can only ask for nothing, never type a
/// fact aterm did not give. Never an answer, a wall's retry, a compaction, a policy continuation,
/// a host's turn or the upgrade's notice.
pub const FACT_RIDES: [&str; 4] = [RULE_CONTINUE, RULE_DONE_CHECK, RULE_LAND, RULE_LAND_NUDGE];

/// `said` (an act's words with its suffixes and standing rules, [`fitted_act`]) with aterm's
/// `rider` after it (aterm's `fact_rides`): ALL OR NOTHING within [`VERBATIM_UNITS`]. `None`
/// where the whole would pass it: the rules are never cut further for it, and the fact is never
/// cut; it stays untold for a later act.
#[must_use]
pub fn with_fact(said: &str, rider: &str) -> Option<String> {
    let whole = format!("{said}{rider}");
    (units(&whole) <= VERBATIM_UNITS).then_some(whole)
}

/// CRUISE'S DEFEND-AND-FIND-MORE PROMPT, typed in place of the continuation where the worker
/// claims it is done ([`says_done`]: a done phrase, an idle report, a retired [`FINISHED`]
/// line, a `Nothing left:` defense). Cruise is always looking for more completeness and
/// correctness: done is never the end, and the worker is asked to land what is open and look
/// harder, or to defend that nothing is left, and keep looking.
pub const DONE_CHECK: &str = concat!(
    harness_mark!(),
    " Autopilot is Cruise: done is never the end. Land every open item, then look harder: \
     review your own work adversarially, widen the checks, fix what they find, close every \
     gap. Decide, record why, defer unavailable work and keep moving without alerts. If you \
     are certain nothing is left, defend it in a few lines starting Nothing left: and keep \
     looking."
);

/// What follows [`DONE_CHECK`] (or a land act) where the words, or a standing need, name an
/// irreversible act (D1): the prompt alone would read as consent to it. aterm's own words.
pub const DONE_CHECK_REVERSIBLE: &str =
    "Approve nothing irreversible: delete, overwrite and force-push nothing.";

/// What Land's instruction and nudge say of files no git repository holds (aterm's
/// `no_repository!`).
macro_rules! no_repository {
    () => {
        "Files no git repository holds land where they are once verified."
    };
}

/// FILES NO REPOSITORY HOLDS LAND WHERE THEY ARE: [`LAND`]'s and [`LAND_NUDGE`]'s sentence for
/// them (aterm's `NO_REPOSITORY`). It names the files, not the folder, so a worker whose folder
/// is no repository still pushes the work it did in one; the probe still reads the thread's
/// repository at a defense ([`repo_suffix`]). It is short so the standing rules keep room on
/// Land's first act with both its clauses ([`fitted_act`]'s bound).
pub const NO_REPOSITORY: &str = no_repository!();

/// What Land types at an episode's first point ([`RULE_LAND`]): land ALL of the work into the
/// remote main branch, no exceptions, and defend it in a `Nothing left:` defense once it is
/// there. Only a clean defense at a land act's yield, with the repository clean, lands the
/// task ([`clean_defense`]). Its [`NO_REPOSITORY`] sentence keeps a worker whose files no
/// repository holds from asking a person for a remote to push to: on 2026-10-24 a real model in
/// a plain folder answered the words without it with a NEEDS YOU for one in 3 runs of 3 (one a
/// live `spawn agent=codex` worker), and with it with a clean `Nothing left:` in 3 of 3.
pub const LAND: &str = concat!(
    harness_mark!(),
    " Autopilot is Land: land ALL of this work into the remote main branch, no exceptions. \
     Finish every open item, verify it, commit, merge and push it, and remove your worktrees \
     and merged branches. Leave nothing unlanded, parked on a branch or for later; chew \
     through every wall with safe, reversible fixes. Start no new work. ",
    no_repository!(),
    " Record unavailable work and the decision to defer it, then land what can proceed. Once it \
     is all on remote main, defend that in a few lines starting Nothing left:"
);

/// What a Land nudge types ([`RULE_LAND_NUDGE`]) at every free point after the instruction
/// until the work is landed: unbounded.
pub const LAND_NUDGE: &str = concat!(
    harness_mark!(),
    " Still landing: get everything onto the remote main branch, no exceptions, with nothing \
     unlanded, parked on a branch or for later. ",
    no_repository!(),
    " Record unavailable work and the decision to defer it, then land what can proceed. Once it \
     is all on remote main, defend that in a few lines starting Nothing left:"
);

/// What a question gets under Land ([`RULE_LAND_ANSWER`]): the option that lands all of the
/// work soonest, never one that starts more or destroys work.
pub const LAND_ANSWER: &str = "Nobody is here to choose: take the option that lands all of this \
                               work on the remote main soonest and start no new work. Pushing \
                               and merging are fine; never force-push or delete work.";

/// THE REPOSITORY SUFFIX (the spec's §9 rule 1; aterm's `repo_work_suffix`): Land's nudge
/// where the probe found work in the thread's repository that no remote main has beside the
/// claim, all of it asked onto the remote main ([`RepoAsk::All`]). `summary` is
/// `RepoWork::summary` (the probe's own counts and the paths git names, never the worker's
/// words), cut to [`REPO_SUMMARY_UNITS`] ([`fit_units`]) as aterm cuts it.
#[must_use]
pub fn repo_suffix(summary: &str) -> String {
    repo_said(&fit_units(summary, REPO_SUMMARY_UNITS), RepoAsk::All)
}

/// CRUISE'S REPOSITORY SUFFIX, the done check's (aterm's `own_repo_work_suffix`, aterm
/// `f9a7dbd4c4`): the finding as [`repo_suffix`] names it, then the agent's own work landed and
/// the rest left as it is ([`RepoAsk::Own`]). The probe reads the whole repository, a person's
/// files and another session's stash included, and Cruise's ask is always the agent's own work
/// (the done check's angle 6 says to leave every one it did not make), so it never says to land
/// all of it.
#[must_use]
pub fn own_repo_suffix(summary: &str) -> String {
    repo_said(&fit_units(summary, REPO_SUMMARY_UNITS), RepoAsk::Own)
}

/// WHAT THE REPOSITORY SUFFIX ASKS OF THE WORK IT NAMES (aterm's `RepoAsk`, aterm
/// `f9a7dbd4c4`: angle 6 contradicted its own act, telling the agent to land another session's
/// stash while it said to leave it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoAsk {
    /// All of it on the remote main: Land's ("if landing LAND EVERYTHING INTO REMOTE MAIN NO
    /// EXCEPTIONS").
    All,
    /// What the agent made, the rest left as it is: Cruise's done check's.
    Own,
}

/// [`repo_suffix`]'s and [`own_repo_suffix`]'s words around a finding `summary` already cut,
/// asking `ask` of it; the bare head where it is empty (aterm's `repo_said_to`).
fn repo_said(summary: &str, ask: RepoAsk) -> String {
    let ask = match ask {
        RepoAsk::All => REPO_ALL,
        RepoAsk::Own => REPO_OWN,
    };
    if summary.is_empty() {
        return format!("{REPO_HEAD}: {ask}");
    }
    format!("{REPO_HEAD} ({summary}): {ask}")
}

/// The repository suffix's head, before its finding (aterm's `REPO_HEAD`).
const REPO_HEAD: &str = "Your repository still holds work no remote main has";

/// Land's ask of the work the repository suffix names (aterm's `REPO_ALL`).
const REPO_ALL: &str = "commit, merge and push it to the remote main first.";

/// Cruise's ask of it (aterm's `REPO_OWN`).
const REPO_OWN: &str = "land what of it you made first, and leave what you did not make exactly \
                        as it is.";

/// An instruction to the worker to retry its Git read, never a claim of actual open work:
/// Land's ([`RepoAsk::All`]).
pub const REPO_UNREAD: &str = "Your repository could not be read: show its git status yourself \
                               and land what it holds first.";

/// [`REPO_UNREAD`] under Cruise's done check ([`RepoAsk::Own`], aterm's `REPO_UNREAD_OWN`): the
/// agent lands what of it it made.
pub const REPO_UNREAD_OWN: &str = "Your repository could not be read: show its git status \
                                   yourself, land what of it you made first, and leave what you \
                                   did not make exactly as it is.";

/// The most UTF-16 units of a repository's finding [`repo_suffix`] names.
const REPO_SUMMARY_UNITS: usize = 160;

/// `s` whole when it fits in `room` UTF-16 code units; else cut after its last whole word
/// that fits with a `…` after it; empty when none does (aterm's `fit_units`).
pub(crate) fn fit_units(s: &str, room: usize) -> Cow<'_, str> {
    if s.encode_utf16().count() <= room {
        return Cow::Borrowed(s);
    }
    let budget = room.saturating_sub('…'.len_utf16());
    let mut units = 0;
    let mut end = 0;
    for (at, c) in s.char_indices() {
        units += c.len_utf16();
        if units > budget {
            break;
        }
        end = at + c.len_utf8();
    }
    let cut = if s[end..].starts_with(char::is_whitespace) {
        end
    } else {
        s[..end].rfind(char::is_whitespace).unwrap_or(0)
    };
    match s[..cut].trim_end() {
        "" => Cow::Borrowed(""),
        kept => Cow::Owned(format!("{kept}…")),
    }
}

/// RETIRED WORDS: what codex-alab typed as the done check before FOREVER CRUISE / FOREVER
/// LAND. No act types it now; conversations written then still hold it.
pub const RETIRED_DONE_CHECK: &str = "If the task is finished and verified, reply with \
                                      ATERM-TASK-DONE on a line by itself and stop; otherwise \
                                      continue.";

/// THE RETIRED DONE TOKEN: a line that is exactly this (Markdown punctuation allowed).
/// Nothing asks for it any more and it ends nothing in either mode; older primers still teach
/// it, so its readers stay tolerant: a token line is a done claim ([`says_done`]: owed the
/// Cruise prompt or the land nudge). aterm `448726c354` retired `says_finished`, the reader of
/// a last line that is the token; codex-alab's copy goes with it.
pub const FINISHED: &str = "ATERM-TASK-DONE";

/// Whether one line of a report is [`FINISHED`] alone (Markdown punctuation allowed). A
/// quoted line (`> ATERM-TASK-DONE`, or the gutter Claude Code draws a quote in) is someone
/// else's report the worker passed on, not its own claim.
#[must_use]
pub fn is_finished_line(line: &str) -> bool {
    !line.trim_start().starts_with(['>', '▎', '│', '┃'])
        && line.trim_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) == FINISHED
}

/// Whether `rule` is one of Land's three acts ([`RULE_LAND`], [`RULE_LAND_NUDGE`],
/// [`RULE_LAND_ANSWER`]).
#[must_use]
pub fn is_land_rule(rule: &str) -> bool {
    matches!(rule, RULE_LAND | RULE_LAND_NUDGE | RULE_LAND_ANSWER)
}

/// WHETHER THE REPLY `tail` IS A CLEAN DEFENSE, read at a land act's yield: a `Nothing left:`
/// line ([`needs::defense_line`]), no need standing beside it ([`needs::needs_you`]; `NEEDS
/// YOU: nothing` is none), and no open work in Land's strict reading of the rest
/// ([`open_work_strict`] over [`needs::defense_body`]: no blocker excuses an item, and work not
/// on the remote main branch counts). Anything else — the retired token, a done phrase, a
/// defense that names open work or a need — is no landing. The repository probe is the
/// fourth condition (`decide`).
#[must_use]
pub fn clean_defense(tail: Option<&str>) -> bool {
    tail.is_some_and(|t| {
        needs::defense_line(t).is_some()
            && needs::needs_you(t).is_empty()
            && open_work_strict(Some(&needs::defense_body(t))).is_empty()
    })
}

/// What a defense (or a done claim) under Land still names open ([`clean_defense`]'s strict
/// reading), for the next nudge's suffix: aterm's, beside the strict reading it calls
/// ([`crate::open_work::land_open_words`]).
pub use crate::open_work::land_open_words;

/// [`DONE_CHECK_REVERSIBLE`] where `irreversible`, else nothing.
#[must_use]
pub fn reversible_clause(irreversible: bool) -> &'static str {
    if irreversible {
        DONE_CHECK_REVERSIBLE
    } else {
        ""
    }
}

/// [`needs::NEEDS_ACK`] where `ack`, else nothing.
#[must_use]
pub fn ack_clause(ack: bool) -> &'static str {
    if ack { needs::NEEDS_ACK } else { "" }
}

/// AN ACT'S WORDS (aterm's `act_words`): `base`, then each of `after` that is not empty, one
/// space apart, then the standing rules `base` arrived composed with, if any ([`fitted_act`]:
/// aterm types an act's suffixes before its one copy of the rules).
#[must_use]
pub fn act_words(base: &str, after: &[&str]) -> String {
    let parts: Vec<Suffix<'_>> = after.iter().map(|a| Suffix::Fixed(a)).collect();
    fitted_act(base, &parts)
}

/// One suffix of an act ([`fitted_act`]; aterm's `Suffix`).
#[derive(Debug, Clone, Copy)]
pub enum Suffix<'a> {
    /// Typed whole: aterm's fixed words (the reversible clause, the needs hint and ack).
    /// Empty: none.
    Fixed(&'a str),
    /// The open work a claim names (`open_work_suffix`): fewer of its words where the act is
    /// short of room, none where not one fits.
    Open(&'a [&'a str]),
    /// The repository's finding ([`repo_suffix`], [`own_repo_suffix`] by the ask): its summary
    /// cut where the act is short of room, then named no more (the bare head), then left out.
    /// A completed `Read(None)` has no suffix; `Unread` names the unfinished check without
    /// inventing work ([`REPO_UNREAD`], [`REPO_UNREAD_OWN`]).
    Repo(RepoRead<&'a RepoWork>, RepoAsk),
}

/// The UTF-16 units of `s`.
fn units(s: &str) -> usize {
    s.encode_utf16().count()
}

/// The most UTF-16 code units a text aterm types may hold and still be drawn as typed
/// (aterm's `VERBATIM_UNITS`: Claude Code draws a longer one as `[Pasted text #N]`). aterm
/// fits every act to it ([`fitted_act`]), and codex-alab types the words aterm would.
pub const VERBATIM_UNITS: usize = 800;

/// How the standing rules open after a typed text (aterm's `RULES_OPEN`, [`with_rules`]).
const RULES_OPEN: &str = " (standing rules: ";

/// `text` split at the standing rules aterm composed it with (aterm's `with_rules`: the words,
/// [`RULES_OPEN`], the rules, `)`): the words, and the rules where it carries them. The
/// `harness policy` reply sends `done_check_text`, `land_text`, `land_nudge_text`,
/// `land_answer_text` and `continue_text` composed so, and the needs hint and ack bare.
#[must_use]
pub fn split_rules(text: &str) -> (&str, Option<&str>) {
    match text.find(RULES_OPEN) {
        Some(at) if text.ends_with(')') => (
            &text[..at],
            Some(&text[at + RULES_OPEN.len()..text.len() - ')'.len_utf8()]),
        ),
        _ => (text, None),
    }
}

/// THE STANDING RULES, AS WHOLE AS ANY TEXT aterm COMPOSED CARRIES THEM, for the acts whose
/// words arrive bare (`answer_text`, the policy texts, the reversible answer, a wall's
/// retry), which aterm types with its rules after them (`words_act`, `type_as`), though the
/// `harness policy` wire has no rules key. Every composed text carries the same rules cut to
/// its own room (aterm's `with_rules`: [`VERBATIM_UNITS`] less its words), so the longest copy
/// is the least cut. `fixed` are aterm's own act texts as its reply sent them
/// (`done_check_text`, `land_text`, `land_nudge_text`, `land_answer_text`; none from an aterm
/// that sends none), whose words never hold the rules' opening, so their split is exact;
/// `owner` is `continue_text`, whose words are the owner's: its copy counts only where it is
/// the fixed texts' copy or one they hold cut ([`is_cut_of`]), else (an owner's text that
/// spells the opening itself) the fixed copy stands. Sent bare, the act texts say no rules
/// stand (aterm sends them always, composed only where rules stand), and an owner's text
/// that ends `… (standing rules: X)` is the owner's words. Only where aterm sent no act text
/// at all is `continue_text`'s copy the one there is.
///
/// EXACT: aterm keeps at most [`RULES_CAP_CHARS`] characters of rules (`RULES_CAP`, then
/// `…`), and `land_answer_text`'s words leave them 599 UTF-16 units, so rules of BMP
/// characters arrive whole; and an act whose words leave no more room than the least-cut
/// copy had gets aterm's own cut of the whole ([`fit_units`] of a cut is the cut of the
/// whole at the smaller room): every wall retry, the reversible answer and the policy texts
/// are longer than `LAND_ANSWER`. Only rules past that room (199 or more characters outside
/// the BMP) under an owner's `answer_text` shorter than 106 units get fewer of their words
/// than aterm types, never others: what aterm cut is not on the wire.
#[must_use]
pub fn standing_rules<'a>(
    fixed: impl IntoIterator<Item = &'a str>,
    owner: Option<&'a str>,
) -> Option<&'a str> {
    let fixed: Vec<&str> = fixed.into_iter().collect();
    let owner = owner.and_then(|t| split_rules(t).1);
    if fixed.is_empty() {
        return owner;
    }
    let fixed = fixed
        .into_iter()
        .filter_map(|t| split_rules(t).1)
        .max_by_key(|r| units(r));
    match (fixed, owner) {
        (Some(f), Some(o)) if is_cut_of(f, o) => Some(o),
        (f, _) => f,
    }
}

/// The most characters of standing rules aterm's loop keeps (`turn_end_loop.rs`'s
/// `RULES_CAP`), before the `…` its cut ends with.
pub const RULES_CAP_CHARS: usize = 400;

/// Whether `cut` is `whole`, or `whole` cut at a word with `…` after ([`fit_units`]).
fn is_cut_of(cut: &str, whole: &str) -> bool {
    cut == whole
        || cut
            .strip_suffix('…')
            .is_some_and(|kept| whole.len() > kept.len() && whole.starts_with(kept))
}

/// `text`, then the standing rules when there are any (aterm's `with_rules`). The rules give
/// way to the text: cut at a word, `…` after, so the whole stays within [`VERBATIM_UNITS`],
/// and left off where not one word of them fits. Rules aterm already cut to fit a shorter
/// text are cut again exactly as aterm cuts the whole ([`fit_units`] of a cut is the cut of
/// the whole at the smaller room).
#[must_use]
pub fn with_rules(text: &str, rules: Option<&str>) -> String {
    let Some(r) = rules.map(str::trim).filter(|r| !r.is_empty()) else {
        return text.to_string();
    };
    let frame = text.encode_utf16().count() + RULES_OPEN.len() + ")".len();
    let r = fit_units(r, VERBATIM_UNITS.saturating_sub(frame));
    if r.is_empty() {
        return text.to_string();
    }
    format!("{text}{RULES_OPEN}{r})")
}

/// AN ACT'S WORDS FITTED TO [`VERBATIM_UNITS`] (aterm's `fitted_act`): `base` (its standing
/// rules split off, [`split_rules`]) and each [`Suffix::Fixed`] whole, and the findings'
/// suffixes made to fit — the repository's summary cut first, then named no more, then the
/// open words fewer, then none, then the repository's suffix left out — then the standing
/// rules in what room is left ([`with_rules`]).
#[must_use]
pub fn fitted_act(base: &str, parts: &[Suffix<'_>]) -> String {
    let (base, rules) = split_rules(base);
    fitted_act_ruled(base, parts, rules)
}

/// AN ACT'S WORDS UNDER THE STANDING RULES `rules` (aterm's `act_words(base, after, rules)`):
/// `base` as given (no rules split off it: an owner's `answer_text` or aterm's fixed answers
/// arrive bare), each of `after` that is not empty, then `rules` ([`with_rules`]). aterm types
/// its prose answers so (`words_act`), though `harness policy` sends `answer_text` bare: the
/// rules are the least-cut copy its composed texts carry ([`standing_rules`]).
#[must_use]
pub fn act_words_ruled(base: &str, after: &[&str], rules: Option<&str>) -> String {
    let parts: Vec<Suffix<'_>> = after.iter().map(|a| Suffix::Fixed(a)).collect();
    fitted_act_ruled(base, &parts, rules)
}

/// [`fitted_act`] of `base` (taken whole) and `parts`, under the standing rules `rules`.
pub(crate) fn fitted_act_ruled(base: &str, parts: &[Suffix<'_>], rules: Option<&str>) -> String {
    let fixed = units(base)
        + parts
            .iter()
            .map(|p| match p {
                Suffix::Fixed(f) if !f.is_empty() => 1 + units(f),
                _ => 0,
            })
            .sum::<usize>();
    let room = VERBATIM_UNITS.saturating_sub(fixed);
    let found: &[&str] = parts
        .iter()
        .find_map(|p| match p {
            Suffix::Open(f) => Some(*f),
            _ => None,
        })
        .unwrap_or(&[]);
    let repo = parts.iter().find_map(|p| match p {
        Suffix::Repo(r, ask) if !r.is_clean() => Some((*r, *ask)),
        _ => None,
    });
    // The repository's suffix that fits in `room` units, a space before it.
    let repo_in = |room: usize| -> Option<String> {
        let (repo, ask) = repo?;
        let work = match repo {
            RepoRead::Read(Some(work)) => work,
            RepoRead::Read(None) => return None,
            RepoRead::Unread => {
                let unread = match ask {
                    RepoAsk::All => REPO_UNREAD,
                    RepoAsk::Own => REPO_UNREAD_OWN,
                };
                return (units(unread) < room).then(|| unread.to_string());
            }
        };
        let whole = repo_said(&fit_units(&work.summary(), REPO_SUMMARY_UNITS), ask);
        if units(&whole) < room {
            return Some(whole);
        }
        // The words around the summary, and the space before them (the `x` stands in for
        // it).
        let frame = units(&repo_said("x", ask));
        let cut = fit_units(&work.summary(), room.saturating_sub(frame)).into_owned();
        let said = repo_said(&cut, ask);
        (units(&said) < room).then_some(said)
    };
    let mut chosen: (Option<String>, Option<String>) = (None, None);
    for most in (0..=OPEN_NAMED.min(found.len())).rev() {
        let open = (most > 0).then(|| open_named(found, most)).flatten();
        let left = room.saturating_sub(open.as_ref().map_or(0, |o| 1 + units(o)));
        if open.as_ref().is_some_and(|o| 1 + units(o) > room) {
            continue;
        }
        let repo_s = repo_in(left);
        if repo_s.is_some() || repo.is_none() || most == 0 {
            chosen = (open, repo_s);
            break;
        }
    }
    let (open, repo_s) = chosen;
    let mut text = base.to_string();
    for p in parts {
        let said = match p {
            Suffix::Fixed(f) => Some(*f),
            Suffix::Open(_) => open.as_deref(),
            Suffix::Repo(..) => repo_s.as_deref(),
        };
        if let Some(a) = said.filter(|a| !a.is_empty()) {
            text.push(' ');
            text.push_str(a);
        }
    }
    with_rules(&text, rules)
}

/// The words a worker says it is DONE with, lowercased, matched as whole
/// words of its own anywhere in its last words, never inside another word or
/// a code span ([`says_done`], [`whole_words_at`], [`without_code`]): a
/// continuation's yield that ends so is
/// short, whatever it worked. What the live E2E of
/// 2026-09-25 read after its nudges (`The workflow is finished`, `nothing
/// left to do`, `what would you like me to work on?`) and their kin.
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
    // The same reports with `completed`: a phrase reads as whole words
    // ([`whole_words_at`]), so `complete` no longer matches inside them.
    // `already completed` only after a subject of the whole task and a form
    // of `be` (`Everything was already completed.`): bare, `had already
    // completed` reports steps of work still in flight (`The build had
    // already completed step 3 when…`), which names no such subject. A
    // `task …` or `work …` subject is the whole one only with nothing but
    // its determiners before it ([`partial_subject_at`]: `the first task
    // was completed` reports progress).
    "all tasks are completed",
    "all tasks completed",
    "everything is completed",
    "work is completed",
    "task is completed",
    "workflow is completed",
    "everything was completed",
    "everything is already completed",
    "everything was already completed",
    "everything has already been completed",
    "everything had already been completed",
    "everything was already done",
    "all tasks were completed",
    "all tasks are already completed",
    "all tasks were already completed",
    "all tasks have already been completed",
    "all tasks had already been completed",
    "work was completed",
    "work is already completed",
    "work was already completed",
    "work has already been completed",
    "work had already been completed",
    "task was completed",
    "task is already completed",
    "task was already completed",
    "task has already been completed",
    "task had already been completed",
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
    // By family, from the live sessions of 2026-10-02 to 10-15, each
    // continued with `keep going`: the worker says it stops…
    "i'm stopping here",
    "i am stopping here",
    "i'll stop here",
    "i will stop here",
    "i've stopped.",
    "i have stopped.",
    // …hands off to the next session…
    "so the next session can",
    // …or has nothing left that it can do. Never in a sentence that names a
    // wait or work in flight ([`said_done_phrase`]: `everything I can do
    // until the build finishes`, `I'll stop here until CI reports back` are
    // a wait on its own work, whose wake it answers). An empty queue (`all
    // queued work has landed`) is an idle report ([`says_idle`]), and the
    // check's own words echoed back are [`CHECK_ECHO`]: each a claim too.
    "everything i can do is",
    "everything this machine can do",
    "nothing more i can do",
    "nothing else i can do",
];

/// WHETHER THE WORKER'S LAST WORDS CLAIM IT IS DONE: a done phrase
/// ([`DONE_PHRASES`]), a report that nothing of its own is running or queued
/// ([`says_idle`]), the check's words echoed ([`CHECK_ECHO`]), the retired
/// [`FINISHED`] on a line of its own anywhere, words that are exactly `DONE`
/// (punctuation allowed), or a [`needs::NOTHING_LEFT`] defense
/// ([`needs::defense_line`]). Title-case `Done.` is a step report. Read
/// without the report's NEEDS YOU lines ([`needs::without_needs`]). A claim
/// ends nothing: under Cruise it is owed the [`DONE_CHECK`], under Land a
/// land act, and only a clean defense at a land act's yield lands the work
/// ([`clean_defense`]).
#[must_use]
pub fn says_done(tail: Option<&str>) -> bool {
    let Some(t) = tail else {
        return false;
    };
    needs::defense_line(t).is_some() || says_done_read(&needs::without_needs(t))
}

/// The check's own words echoed back: a done claim like any, never more,
/// at the check's yield too, where a worker that is not done echoes them as
/// readily as one that is (`Phase 1 is finished and verified; phase 2 is
/// next`, `Not everything is finished and verified`, `Is the task finished
/// and verified? Not yet`): a claim, owed its act like any. The retired
/// checks' own words (aterm's `RETIRED_DONE_CHECKS`: `nothing of the job is left`,
/// the owner's rule of 2026-10-19; `everything left needs a person`, the
/// primer's definition of finished the check took in the merge of
/// `f09ef3b15`; `If the task is finished and verified, …`), which workers
/// taught by them and the records they wrote still echo.
const CHECK_ECHO: [&str; 3] = [
    "nothing of the job is left",
    "everything left needs a person",
    "finished and verified",
];

/// [`says_done`] on a tail, read afresh.
fn says_done_read(t: &str) -> bool {
    if says_done_outright(t) {
        return true;
    }
    let flat = done_flat(t);
    says_idle(&flat) || CHECK_ECHO.iter().any(|echo| said_as_report(&flat, echo))
}

/// Whether `flat` ([`done_flat`]) says the check's words `echo` as a report
/// of the worker's own state ([`CHECK_ECHO`]): not negated just before them
/// ([`negated_before`]), and not in a clause that makes them a condition
/// ([`conditional_at`]: `I'll tag it once it is finished and verified`, `If
/// the task is finished and verified, …`).
fn said_as_report(flat: &str, echo: &str) -> bool {
    flat.match_indices(echo).any(|(at, _)| {
        !negated_before(flat, at)
            && !conditional_at(flat, at, echo)
            && !partial_subject_at(flat, at, echo)
    })
}

/// What ends a clause for [`conditional_at`] and [`partial_subject_at`].
const CLAUSE_ENDS: &[char] = &[',', ';', ':', '.', '!', '?', '—', '–', '(', ')'];

/// The words of the clause before `at` in `flat`, back to the nearest
/// [`CLAUSE_ENDS`] mark.
fn clause_before(flat: &str, at: usize) -> Vec<&str> {
    flat[..at]
        .rsplit(CLAUSE_ENDS)
        .next()
        .unwrap_or("")
        .split_whitespace()
        .collect()
}

/// Whether the clause `phrase` at `at` in `flat` stands in makes it a
/// CONDITION or a time to come, not a report (the clause runs to the nearest
/// `,` `;` `:` `.` `!` `?` `—` `–` `(` `)` on either side, [`CLAUSE_ENDS`]):
/// - `if`, `when`, `whenever`, `unless` or `once` (not as a count: `once
///   more`, `once again`, `at once`, `more than once`), and `provided`,
///   `providing`, `assuming`, `supposing`, `as soon as` or `as long as`,
///   anywhere among the clause's words outside the phrase itself: `If all
///   tasks are complete, I'll tag the release` and `Everything is done when
///   CI is green` (or `as soon as CI is green`) say nothing is done;
/// - `after`, `before`, `by the time`, `in case` or `whether` only where
///   they LEAD the phrase, before it in its clause (`After all tasks are
///   complete, I'll tag it`, `I'll tag it after it is finished and
///   verified`): after the phrase they date or hedge a report (`All tasks
///   are complete after the rebase`, `… in case you want to ship`) — the
///   review of ATERM-KI-1016-1, KR-DONE-1;
/// - `until` or `till` before the phrase where it waits on work
///   ([`waits_at`]: `the release waits until it is finished and verified`),
///   never before a stop for a person (`Until you review PR #42 there is
///   nothing left to do`); after the phrase [`waits_in`] reads it.
///
/// `When I checked, all tasks were complete` says it in a clause of its own.
/// A phrase whose own words carry one (`let me know if you need anything`)
/// is read past them. A subordinator read where it only dates a report that
/// opens its clause (`Right after the rebase all tasks were complete`) costs
/// one continuation; one missed ends a task whose work goes on.
fn conditional_at(flat: &str, at: usize, phrase: &str) -> bool {
    const IF: &[&str] = &["if", "when", "whenever", "unless", "once"];
    const STATES: &[&[&str]] = &[
        &["provided"],
        &["providing"],
        &["assuming"],
        &["supposing"],
        &["as", "soon", "as"],
        &["as", "long", "as"],
    ];
    const LEADS: &[&[&str]] = &[
        &["after"],
        &["before"],
        &["by", "the", "time"],
        &["in", "case"],
        &["whether"],
    ];
    let after = if phrase.ends_with(CLAUSE_ENDS) {
        ""
    } else {
        flat[at + phrase.len()..]
            .split(CLAUSE_ENDS)
            .next()
            .unwrap_or("")
    };
    // The phrase stands as one word (`\0`, at `p`) between its clause's two
    // halves, so a count's neighbours are read where they stand.
    let mut words = clause_before(flat, at);
    let p = words.len();
    words.push("\0");
    words.extend(after.split_whitespace());
    let word = |j: Option<usize>| j.and_then(|j| words.get(j)).copied().unwrap_or("");
    (0..words.len()).any(|i| {
        let w = words[i];
        let (next, prev, prev2) = (
            word(i.checked_add(1)),
            word(i.checked_sub(1)),
            word(i.checked_sub(2)),
        );
        let count = w == "once"
            && (matches!(next, "more" | "again")
                || prev == "at"
                || (prev == "than" && prev2 == "more"));
        let rest = &words[i..];
        let leads = i < p
            && (LEADS.iter().any(|s| words[i..p].starts_with(s))
                || (matches!(w, "until" | "till") && waits_at(&words, i)));
        (IF.contains(&w) && !count) || STATES.iter().any(|s| rest.starts_with(s)) || leads
    })
}

/// Whether the subject done phrase `phrase` at `at` in `flat` opens with
/// names only PART of the task (the review of ATERM-KI-1016-1, KR-DONE-2):
/// a `task …` or `work …` phrase with anything but determiners of the whole
/// (`the`, `this`, `that`, `my`, `your`, `our`, `all`, `whole`, `entire`)
/// just before it in its clause (`the first task was completed`, `the
/// migration task`, `phase 1's work is done`), or a part named before those
/// (`half the work`, `most of the work`; `all of the work` is the whole);
/// and an `all …` or `everything …` phrase with a part's word just before it
/// (`almost everything is done`, `nearly all tasks are complete`). Other
/// words before the determiners open the clause (`I re-ran the suites and
/// the work is done`). A whole task read as a part costs one continuation; a
/// part read as the whole ends a task whose work goes on.
fn partial_subject_at(flat: &str, at: usize, phrase: &str) -> bool {
    const WHOLE: &[&str] = &[
        "the", "this", "that", "my", "your", "our", "all", "whole", "entire",
    ];
    const PART: &[&str] = &[
        "half",
        "most",
        "some",
        "part",
        "much",
        "almost",
        "nearly",
        "mostly",
        "partly",
        "largely",
        "practically",
        "virtually",
        "of",
    ];
    let noun = phrase.starts_with("task ") || phrase.starts_with("work ");
    if !(noun || phrase.starts_with("all ") || phrase.starts_with("everything ")) {
        return false;
    }
    let before = clause_before(flat, at);
    let mut i = before.len();
    if noun {
        while i > 0 && WHOLE.contains(&before[i - 1]) {
            i -= 1;
        }
        if i == before.len() && i > 0 {
            // A modifier, not a determiner, just before the noun.
            return true;
        }
    }
    match i.checked_sub(1).map(|j| before[j]) {
        Some("of") => i < 2 || before[i - 2] != "all",
        Some(w) => PART.contains(&w),
        None => false,
    }
}

/// The worker's own word that it is done: [`FINISHED`] on a line of its own,
/// exactly `DONE`, or a [`DONE_PHRASES`] phrase said as the end
/// ([`said_done_phrase`]: as whole words outside code ([`whole_words_at`],
/// [`without_code`]), not negated just before it — `This isn't all done`
/// says nothing done — nor in a sentence that names a wait or work in
/// flight, a sentence ending at a list line or heading too
/// ([`done_blocks`]); `what's next?` and `anything else?` only as the last
/// words) —
/// every done report but a first-only one (an idle report, [`says_idle`];
/// the check's words echoed, [`CHECK_ECHO`]). Never a done claim where its
/// own work's wake comes next (aterm's cross-lane review F4: `I'll stop here
/// until CI reports back`).
fn says_done_outright(t: &str) -> bool {
    if t.lines().any(is_finished_line) || t.trim_matches(|c: char| !c.is_alphanumeric()) == "DONE" {
        return true;
    }
    let blocks: Vec<String> = done_blocks(&without_code(t))
        .iter()
        .map(|b| done_flat(b))
        .collect();
    let last = blocks.len().saturating_sub(1);
    DONE_PHRASES.iter().any(|p| {
        blocks
            .iter()
            .enumerate()
            .any(|(i, b)| said_done_phrase(b, p, i == last))
    })
}

/// `t` without its code: each line's inline code spans (`` `…` ``, an
/// unpaired tick's tail kept) and every line of a fenced block (between
/// lines opening with ```` ``` ```` or `~~~`). A quoted log line (`` `Build
/// completed successfully in 0:59:34` ``) is a tool's words, never the
/// worker's own (the decision review's RR3-TE-1). The screen draws no ticks;
/// the conversation's record keeps them.
fn without_code(t: &str) -> String {
    let mut block = None;
    let mut out: Vec<String> = Vec::new();
    for line in t.lines() {
        if needs::fenced_line(line, &mut block) {
            out.push(String::new());
            continue;
        }
        let parts: Vec<&str> = line.split('`').collect();
        let last = parts.len().saturating_sub(1);
        let kept: Vec<&str> = parts
            .iter()
            .enumerate()
            .filter(|&(i, _)| i % 2 == 0 || (last % 2 == 1 && i == last))
            .map(|(_, p)| *p)
            .collect();
        out.push(kept.join(" "));
    }
    out.join("\n")
}

/// A tail cut where a sentence ends with no full stop: before every line
/// that opens a list item or a heading (`-`, `*`, `•`, `#`, `1.`, `1)`, each
/// followed by a space) and at every blank line. A heading or a list line
/// carries no `. `, so read as one text with the next it would make one
/// sentence of both: `All tasks complete` over `- The release workflow is
/// still running in CI` is a finish beside a fact, never a wait (the
/// decision review's RR-DEC-1). But a list the block's own sentence
/// introduces (a line ending `:`) is that sentence's: `All tasks are
/// complete except:` over `- the release workflow, still running in CI`
/// names its wait in its items (the decision review's RR3-TE-3).
fn done_blocks(t: &str) -> Vec<String> {
    let mut blocks: Vec<String> = Vec::new();
    let mut open = false;
    let mut introduced = false;
    for line in t.lines() {
        let l = line.trim();
        if l.is_empty() {
            open = false;
            continue;
        }
        if !open || (opens_item(l) && !introduced) {
            blocks.push(String::new());
            open = true;
            introduced = false;
        }
        // A block was opened above (codex-alab denies `expect`).
        if let Some(b) = blocks.last_mut() {
            b.push(' ');
            b.push_str(l);
        }
        introduced |= l.ends_with(':');
    }
    blocks
}

/// Whether trimmed line `l` opens a list item or a heading: `-`, `*`, `•`,
/// one or more `#`, or a number and `.` or `)`, each followed by a space.
fn opens_item(l: &str) -> bool {
    let rest = if let Some(r) = l.strip_prefix(['-', '*', '•']) {
        r
    } else if l.starts_with('#') {
        l.trim_start_matches('#')
    } else {
        let digits = l.trim_start_matches(|c: char| c.is_ascii_digit());
        if digits.len() == l.len() {
            return false;
        }
        match digits.strip_prefix(['.', ')']) {
            Some(r) => r,
            None => return false,
        }
    };
    rest.starts_with(char::is_whitespace)
}

/// What says work is still in flight or next, read beside a report that
/// would say done: an idle report beside one anywhere in the last words
/// ([`says_idle`]), a done phrase beside one in its own sentence
/// ([`said_done_phrase`]).
const BUSY: &[&str] = &[
    "still running",
    "still going",
    "still working",
    "still in progress",
    "is running:",
    "are running:",
    "next step",
    "is next",
];

/// Words that, in a done phrase's own sentence, make it a WAIT on work in
/// flight (`I'll stop here until CI reports back`), never the end: whole
/// words ([`WAIT_WORDS`]) and phrases ([`WAIT_PHRASES`], and [`BUSY`]).
const WAIT_WORDS: &[&str] = &["until", "till", "once"];

/// See [`WAIT_WORDS`].
const WAIT_PHRASES: &[&str] = &[
    "will notify",
    "notify me",
    "notifies me",
    "waiting on",
    "waiting for",
    "reports back",
    "report back",
];

/// The [`DONE_PHRASES`] that end a report only as its LAST words: `Phase 2
/// is next. What's next? Starting phase 2.` asks nothing.
const ASKS_NEXT: &[&str] = &["what's next?", "anything else?"];

/// Whether `flat` (one of the tail's [`done_blocks`], read by [`done_flat`];
/// `last` when it is the tail's last) says done phrase `phrase` as the END:
/// not negated just before it ([`negated_before`]), not in a sentence that
/// names a wait or work in flight ([`waits_in`]; the sentence only — a
/// stop beside `The worktree removal is still running in the background.`
/// is a stop), and an [`ASKS_NEXT`] phrase only as the tail's last words.
fn said_done_phrase(flat: &str, phrase: &str, last: bool) -> bool {
    flat.match_indices(phrase).any(|(at, _)| {
        if !whole_words_at(flat, at, phrase) {
            return false;
        }
        if negated_before(flat, at)
            || conditional_at(flat, at, phrase)
            || partial_subject_at(flat, at, phrase)
        {
            return false;
        }
        let (start, end) = sentence_of(flat, at, phrase);
        if waits_in(&flat[start..end]) {
            return false;
        }
        !(ASKS_NEXT.contains(&phrase)
            && (!last || flat[at + phrase.len()..].chars().any(char::is_alphanumeric)))
    })
}

/// Whether `phrase` at `at` in `flat` stands as whole words: no letter or
/// digit just before it, nor just after it where it ends in one (the
/// decision review's RR3-TE-1: `already completed` says no `already
/// complete`, `subtask is complete` no `task is complete`, `install done` no
/// `all done`, `no more worktrees` no `no more work`). A phrase that ends in
/// its own mark (`i've stopped.`, `what's next?`) is read up to it.
fn whole_words_at(flat: &str, at: usize, phrase: &str) -> bool {
    let word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    let end = at + phrase.len();
    !word(flat[..at].chars().next_back())
        && !(phrase.ends_with(char::is_alphanumeric) && word(flat[end..].chars().next()))
}

/// The sentence of `flat` the phrase at `at` stands in: from just after the
/// last `. `, `! ` or `? ` before it to the first such end after it (its own
/// closing mark aside), or the text's end. Never split at `;` or `,`
/// (`Everything I can do is queued behind the build, which is still
/// running.`), nor inside `5.9 TiB`.
fn sentence_of(flat: &str, at: usize, phrase: &str) -> (usize, usize) {
    const ENDS: [&str; 3] = [". ", "! ", "? "];
    let start = ENDS
        .iter()
        .filter_map(|e| flat[..at].rfind(e).map(|i| i + e.len()))
        .max()
        .unwrap_or(0);
    let from = at + phrase.trim_end_matches(['.', '?', '!']).len();
    let end = ENDS
        .iter()
        .filter_map(|e| flat[from..].find(e).map(|i| from + i + 1))
        .min()
        .unwrap_or(flat.len());
    (start, end)
}

/// Whether `sentence` names a wait or work in flight ([`WAIT_PHRASES`],
/// [`BUSY`], a [`WAIT_WORDS`] word that waits on work: [`waits_at`]).
fn waits_in(sentence: &str) -> bool {
    if WAIT_PHRASES
        .iter()
        .chain(BUSY)
        .any(|w| sentence.contains(w))
    {
        return true;
    }
    // A hyphenated word is one word (`until the user-space daemon` waits on
    // a daemon, the decision review's RR3-TE-3).
    let words: Vec<&str> = sentence
        .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '-'))
        .filter(|w| !w.is_empty())
        .collect();
    (0..words.len()).any(|i| WAIT_WORDS.contains(&words[i]) && waits_at(&words, i))
}

/// Whether the [`WAIT_WORDS`] word `words[i]` waits on work in flight. Not
/// where a PERSON comes next (`until you review PR #42`, `until you're
/// back`, `until the owner provides the release credential`): that is a
/// stop for someone, the worker's own word that it is done (the decision
/// review's RR-DEC-1), `the owner's` included. Nor `once` as a count (`once
/// more`, `once again`, `at once`, `more than once`). A hyphenated word is
/// one (`until the user-space daemon` waits on a daemon). `your` is a person
/// only before a person's own act, a hyphenated one by its first part
/// (`until your review`, `until your go-ahead`, `until your sign-off`):
/// `until your CI run finishes` waits on work, and a wrong wait costs one
/// continuation under the back-off where a wrong end strands the work.
fn waits_at(words: &[&str], i: usize) -> bool {
    const PERSON: &[&str] = &["you", "you're", "you've", "someone", "somebody"];
    const PERSON_NOUN: &[&str] = &["owner", "user", "person", "owner's", "user's", "person's"];
    const PERSONS_ACT: &[&str] = &[
        "review",
        "approval",
        "go",
        "decision",
        "reply",
        "answer",
        "input",
        "feedback",
        "sign",
        "confirmation",
        "ok",
        "okay",
        "say",
        "permission",
        "instructions",
        "response",
        "call",
    ];
    let word = |j: Option<usize>| j.and_then(|j| words.get(j)).copied().unwrap_or("");
    let (next, next2) = (word(i.checked_add(1)), word(i.checked_add(2)));
    let (prev, prev2) = (word(i.checked_sub(1)), word(i.checked_sub(2)));
    let person = PERSON.contains(&next)
        || (matches!(next, "the" | "a") && PERSON_NOUN.contains(&next2))
        || (next == "your"
            && next2
                .split('-')
                .next()
                .is_some_and(|w| PERSONS_ACT.contains(&w)));
    let count = words[i] == "once"
        && (matches!(next, "more" | "again")
            || prev == "at"
            || (prev == "than" && prev2 == "more"));
    !(person || count)
}

/// Whether `flat` ([`done_flat`]) says `phrase` with no negation
/// ([`negated_before`]): `did not fail again; all done` says it, `not yet
/// finished and verified` does not.
fn said_unnegated(flat: &str, phrase: &str) -> bool {
    flat.match_indices(phrase)
        .any(|(at, _)| !negated_before(flat, at))
}

/// Whether a negation (`not`, `cannot`, a `…n't` word) stands among the two
/// words before `at` in `flat`, in the same clause.
fn negated_before(flat: &str, at: usize) -> bool {
    flat[..at]
        .split(' ')
        .rev()
        .filter(|w| !w.is_empty())
        .take(2)
        .take_while(|w| !w.ends_with(['.', ',', ':', ';', '!', '?']))
        .any(|w| w == "not" || w == "cannot" || w.ends_with("n't"))
}

/// A tail as the done readers read it: lowercased, `’` as `'`, one space
/// between words.
fn done_flat(t: &str) -> String {
    t.to_lowercase()
        .replace('’', "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether the worker's last words (`flat`: lowercased, one space between
/// words, as [`says_done`] reads them) say nothing of its own is in flight:
/// `no <word> is` (or `are`) `running` (or `queued`) — `No lander is
/// running, so I'm not re-arming the watch` — and `nothing (else) is
/// queued`, each inside one clause (`Did it pass? No. CI is running again`
/// and `No, it is running the last suite` are an answer and a step), and an
/// empty queue in words (`All queued work has landed`: 2026-10-04 02:08, it
/// was followed by six hours of work). Not where they go on to name what
/// still runs (`still running`, `still going`, `still working`, `still in
/// progress`, a list under `… is running:`) or what comes next (`OD-1 phase
/// 1 is the next step`): that is a wait on work in flight, or work ahead,
/// continued as any (2026-10-07: `nothing is queued to land` over a `Still
/// running:` list of agents). Nor where they
/// stop for a person ([`STOP_PHRASES`]: `What's left needs your decisions:
/// …`): that is an ask, answered as the module header rules (D1), not a done
/// report. An idle report is a done claim, and ends nothing (2026-10-05:
/// the landing reports `OD-1 phase 1 has landed and no lander is running`
/// and, fifteen hours on, `Both branches have landed and no lander is
/// running` came before OD-1 phase 2).
fn says_idle(flat: &str) -> bool {
    const EMPTY_QUEUE: &[&str] = &["all queued work has landed", "all queued work is landed"];
    if BUSY.iter().chain(STOP_PHRASES).any(|b| flat.contains(b)) {
        return false;
    }
    if EMPTY_QUEUE.iter().any(|p| said_unnegated(flat, p)) {
        return true;
    }
    // Each word, and whether a clause ends with it (`no.`, `no,`).
    let words: Vec<(&str, bool)> = flat
        .split(' ')
        .map(|w| {
            let end = &w[w.trim_end_matches(|c: char| !c.is_alphanumeric()).len()..];
            (
                w.trim_matches(|c: char| !c.is_alphanumeric()),
                end.contains(['.', ',', ':', ';', '!', '?']),
            )
        })
        .collect();
    let one_clause = |w: &[(&str, bool)]| w[..w.len() - 1].iter().all(|&(_, ends)| !ends);
    let be = |w: &str| matches!(w, "is" | "are");
    let idle = |w: &str| matches!(w, "running" | "queued");
    let none_of = |w: &[(&str, bool)]| w[0].0 == "no" || (w[0].0 == "nothing" && w[1].0 == "else");
    // `no <word> is running`, `nothing else is queued`…
    let of_one = words
        .windows(4)
        .any(|w| one_clause(w) && none_of(w) && be(w[2].0) && idle(w[3].0));
    // …and `nothing is running`.
    of_one
        || words
            .windows(3)
            .any(|w| one_clause(w) && w[0].0 == "nothing" && be(w[1].0) && idle(w[2].0))
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
/// accepted continuation. Force-push spellings include `--force`, `force-push`, a short `-f`
/// option after `push` (reordered or clustered, as in `push origin main -vf`), and
/// `--force-with-lease`, which overwrites the remote ref as `--force` does when the lease
/// holds.
pub const DESTRUCTIVE_WORDS: &[&str] = &[
    "force",
    "force-push",
    "--force",
    "push -f",
    "force-with-lease",
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
                                  continue without asking again. (Answered by codex-alab autopilot \
                                  by policy. The user was not asked.)";

/// The `short-term` policy text (aterm `question.rs`).
pub const SHORT_TERM_TEXT: &str = "Decide yourself: choose the option that gets the current \
                                   deliverable done soonest at acceptable risk. Say which you \
                                   chose and why in one line and continue without asking again. \
                                   (Answered by codex-alab autopilot by policy. The user was not \
                                   asked.)";

/// Every wall act resumes from the last known result and asks the agent to record
/// unavailable work, decide what can proceed and continue without routine alerts, then to
/// handle its own restart (aterm's `CARRY_ON` over `needs_hint!`, which carries
/// `restart_hint!` since aterm `11e4a76a8d`).
pub const CARRY_ON: &str = concat!(
    "Carry on from where you stopped; if the result of your last step is missing, check \
     whether it ran before you repeat it. ",
    needs::needs_hint!()
);

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

/// Judges the worker's last words ([`Said`]) exactly as aterm's `classify_said` does: read
/// without the report's NEEDS YOU lines ([`needs::without_needs`]), so `NEEDS YOU: approval to
/// publish` is no stop phrase asked of the harness.
#[must_use]
pub fn classify_said(tail: Option<&str>) -> Said {
    let Some(tail) = tail else {
        return Said::Plain;
    };
    classify_said_read(&needs::without_needs(tail))
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

/// Whether `text` names a force-push git's short forms spell: a push whose clause has a short
/// flag cluster with an `f` (`push -f`, `push -uf`, `push origin main -f`), or a refspec forced
/// with `+` (`push origin +main`). PORTED VERBATIM from aterm's `force_push`
/// (`supervise/policy/turn_end.rs`, go-ahead review 4, `a6a1e0d67a`; as at aterm `b938f0e959`):
/// [`destructive_word`] reads both, so the base guard reads `push origin +main` as aterm's does.
/// A short force flag belongs to the push in its clause, not necessarily the next word; a flag
/// keeps its leading dash to tell it from a ref, and a clause stops at command or sentence
/// separators. This remains a conservative prose classifier, not a shell command parser; `-o`
/// consumes the rest of its cluster as a push-option value, so an `f` there is not the force flag.
pub(crate) fn force_push(text: &str) -> bool {
    text.split([';', '|', '&', '\n', '?', '!']).any(|clause| {
        let mut push = false;
        let flagged = clause
            .split(|c: char| !(c.is_alphanumeric() || c == '-'))
            .filter(|word| !word.is_empty())
            .any(|word| match word {
                word if inflects(word, "push") => {
                    push = true;
                    false
                }
                _ => {
                    push && word.strip_prefix('-').is_some_and(|flags| {
                        !flags.starts_with('-')
                            && flags.bytes().take_while(|&b| b != b'o').any(|b| b == b'f')
                    })
                }
            });
        let mut push = false;
        let forced = clause.split_whitespace().any(|token| {
            let token = token.trim_matches(['`', '"', '\'', '(', ')', ',', '.', ':']);
            if token
                .split(|c: char| !(c.is_alphanumeric() || c == '-'))
                .any(|w| inflects(w, "push"))
            {
                push = true;
                return false;
            }
            push && token
                .strip_prefix('+')
                .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_alphabetic()))
        });
        flagged || forced
    })
}

/// The first of [`DESTRUCTIVE_WORDS`] in `text` as a whole word (or phrase), or an
/// inflection of one, returned as its base form.
#[must_use]
pub fn destructive_word(text: &str) -> Option<&'static str> {
    let short_force_push = force_push(text);
    let words: Vec<&str> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '-'))
        .filter(|w| !w.is_empty())
        .map(|w| w.trim_start_matches('-'))
        .collect();
    DESTRUCTIVE_WORDS.iter().copied().find(|d| {
        if *d == "push -f" {
            return short_force_push;
        }
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

/// Why an API wall's request failed, as aterm's `ApiCause` tells the causes apart, where
/// Codex's terminal error tells them (`extension::classify`). aterm's `Config` (a certificate
/// or proxy refusal) and `Request` (a refused request) have no counterpart: Codex names
/// neither apart from a status, and a permanent client refusal is no retry's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiCause {
    /// The server's own failure: a status, an overload, a timeout.
    Server,
    /// A reply the connection cut off.
    CutOff,
    /// The API was never reached.
    Unreachable,
    /// A reply past its output token maximum (Codex's incomplete response,
    /// `max_output_tokens`).
    Overlong,
}

/// aterm's `wall_retry_text` for Codex: the wall named in aterm's own words, never a row of
/// the screen or the error's text (aterm ruling 2026-10-08, H6-HARNESS-1), with what follows
/// from its cause. A reply past its output token maximum is written again the same way it is
/// cut off again, so the words ask for smaller pieces (aterm's `Overlong` case). aterm's
/// `RULE_API_BACK` case is [`api_back_text`].
#[must_use]
pub fn wall_retry_text(what: &str, cause: ApiCause) -> String {
    let who = "Codex";
    match cause {
        ApiCause::CutOff => {
            format!("{who} reported {what}, so your last reply may be incomplete. {CARRY_ON}")
        }
        ApiCause::Unreachable => format!("{who} reported {what}; trying again. {CARRY_ON}"),
        ApiCause::Overlong => format!(
            "{who} reported {what}, so your last reply was cut off. Write long files and outputs \
             across several smaller replies. {CARRY_ON}"
        ),
        ApiCause::Server => format!("{who} reported {what}. {CARRY_ON}"),
    }
}

/// aterm's `wall_retry_text` under `RULE_API_BACK`, for Codex: typed only once codex-alab
/// MEASURED the thread's API route reachable again (`netwatch`, a fresh verified handshake).
#[must_use]
pub fn api_back_text(what: &str) -> String {
    format!("Codex reported {what}, and the API is reachable again now. {CARRY_ON}")
}

/// [`RULE_LOGIN_BACK`]'s words, aterm's `login_back_text` (`cleared_text(Cleared::Login,
/// Clearing::Measured)`): the one line that resumes a turn the lost login ended, never Cruise's
/// or Land's prompt (aterm's owner rule 2026-10-25: a prompt into a turn the agent did not end is
/// spam). Typed with the standing rules after it, as aterm's `type_as` types it.
pub const LOGIN_BACK: &str = concat!(
    harness_mark!(),
    " The expired login has cleared; continue where you stopped."
);

/// HOW A USAGE LIMIT'S CLEARING IS KNOWN (aterm's `Clearing`, [`limit_cleared`]): measured (the
/// reset the wall named passed, or the accounts probe says included usage is back), or tried
/// blind on a back-off where the wall named no reset (a usage window with none, a quota wall).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clearing {
    /// The clearing was seen: *has cleared* ([`LIMIT_CLEARED`]).
    Measured,
    /// The resume is a try on a back-off: *may have cleared* ([`LIMIT_MAY_HAVE_CLEARED`]).
    Blind,
}

/// [`RULE_LIMIT_RESUME`]'s words where the clearing was measured (aterm's
/// `cleared_text(Cleared::UsageLimit, Clearing::Measured)`, [`limit_cleared`]).
pub const LIMIT_CLEARED: &str = concat!(
    harness_mark!(),
    " The usage limit has cleared; continue where you stopped."
);

/// [`RULE_LIMIT_RESUME`]'s words on a blind try (aterm's
/// `cleared_text(Cleared::UsageLimit, Clearing::Blind)`, [`limit_cleared`]).
pub const LIMIT_MAY_HAVE_CLEARED: &str = concat!(
    harness_mark!(),
    " The usage limit may have cleared; continue where you stopped."
);

/// THE ONE LINE A USAGE LIMIT'S RESUME TYPES (aterm's `cleared_text(Cleared::UsageLimit, ..)`,
/// v6 §14.17.5 slice 12a, aterm `77161d6432`): the limit ended the agent's turn, not the agent,
/// so the resume says what cleared and nothing of Cruise's or Land's prompt (aterm's owner rule
/// 2026-10-25: "a prompt into a turn the agent did not end is spam"). Cruise and Land speak at
/// the agent's own next turn end; a prompt the limit cut is the agent's still, and its answer is
/// judged as that prompt's. Typed with the standing rules after it, as aterm's `with_rules`.
#[must_use]
pub const fn limit_cleared(clearing: Clearing) -> &'static str {
    match clearing {
        Clearing::Measured => LIMIT_CLEARED,
        Clearing::Blind => LIMIT_MAY_HAVE_CLEARED,
    }
}

/// Whether `text` is a usage limit's one cleared line ([`limit_cleared`], the standing rules
/// after it), as aterm's `is_cleared_line` reads a typed act: a resume that gives no prompt.
#[must_use]
pub fn is_cleared_line(text: &str) -> bool {
    text.starts_with(LIMIT_CLEARED) || text.starts_with(LIMIT_MAY_HAVE_CLEARED)
}

/// The native upgrade notice's opening, naming this controller and the Codex product.
pub const UPGRADE_ANNOUNCE_HEAD: &str = concat!(harness_mark!(), " Codex ");

/// The notice's ask (aterm's `upgrade::STOPPING_POINT`, verbatim).
pub const UPGRADE_STOPPING_POINT: &str = "Please get to a good stopping point first: let \
     background tasks, subagents or workflows you started finish while they are still making \
     progress (do not cancel them), but stop any background shell or task of yours that only \
     waits for something that has already ended or can never happen (a poll loop on a workflow, \
     agent, job or file that is gone): that wait is not work. A watch or monitor loop you can \
     start again after the resume (a disk or swap watch, a status poll) is not work either: \
     stop it, and note it. Save or commit work in progress, and do not start new long-running \
     work.";

/// What the notice asks the agent to answer (aterm's `upgrade::READINESS_ANSWER`, verbatim).
pub const UPGRADE_READINESS_ANSWER: &str = "Say in ordinary language whether this session is \
    ready to restart now, or explain what must finish first.";

/// The notice's last sentence: codex-alab asks again on the bounded schedule
/// implemented by `crate::upgrade_ask`.
pub const UPGRADE_NOTICE_CLOSE: &str =
    "If you cannot stop now, say why; codex-alab will wait and ask again later.";

/// The prefix of a notice's reference (aterm's `upgrade::READY_PREFIX`); a line that is the
/// reference alone answers the notice as ready (aterm's legacy marker reply).
pub const UPGRADE_READY_PREFIX: &str = "ATERM-UPGRADE-READY-";

/// THE LIVE UPGRADE'S READINESS NOTICE (aterm's `upgrade::prepare_prompt`, aterm main
/// `433715d26d`), sent natively under `upgrade_ask` before the re-exec. It identifies
/// codex-alab as the controller and uses `codex resume` for the same conversation,
/// and the builds, which codex-alab names by the path the launch link names (`to`) and by its
/// own version (`from`), as it reads no version of the build it moves to (aterm's
/// `({source})` after it, a Claude Code install's kind, has no counterpart). `marker` is the
/// notice's reference ([`UPGRADE_READY_PREFIX`]).
#[must_use]
pub fn upgrade_notice(to: &str, from: &str, marker: &str) -> String {
    format!(
        "{UPGRADE_ANNOUNCE_HEAD}{to} is installed; this session runs {from}. To move you onto \
         it, codex-alab will restart itself in place and resume this same conversation (codex \
         resume, same tab, same flags). {UPGRADE_STOPPING_POINT} {UPGRADE_READINESS_ANSWER} \
         Notice reference: {marker}. {UPGRADE_NOTICE_CLOSE}"
    )
}

/// aterm's words for the hour's typing budget spent (`notice::WHY_BUDGET`, aterm main
/// `8ffb44c7da`): the cap that keeps it from typing into a tab in a loop.
pub const HOUR_CAP: &str = "it already typed here as often as an hour allows";

/// THE SHORT-TURN BACK-OFF AS A PERSON SEES IT, in aterm's words (`continue_policy`'s
/// `WaitUntil` why, aterm main `8ffb44c7da`): `<n> short turn(s) in a row: <s> s back-off`,
/// in whole minutes from two minutes on.
#[must_use]
pub fn short_backoff_wait(streak: u32, wait: std::time::Duration) -> String {
    let secs = wait.as_secs();
    if secs < 120 {
        format!("{streak} short turn(s) in a row: {secs} s back-off")
    } else {
        format!(
            "{streak} short turn(s) in a row: {} min back-off",
            secs / 60
        )
    }
}

/// AN ACT THE PREVIOUS IMAGE OWED AS A PERSON SEES ITS WAIT (a live re-exec onto a new build,
/// aterm's relaunch of a session at rest): the back-off or the hour's cap that paced it is the
/// old image's, so its words are not this image's to repeat (review of f134d2b2cb, finding 7:
/// carried across, the wait showed nothing).
pub const OWED_ACT: &str = "the act it owed before its restart";

/// The clause [`types_again_at`] puts after a wait's words, before the clock time (the
/// separator aterm's stop line uses).
pub const TYPES_AGAIN: &str = " \u{b7} types again at ";

/// A wait's `words` with the local clock time its act is typed, `at` on the clock `now` reads
/// (`<words> · types again at HH:MM`). The time is the wall clock now plus the wait left, so a
/// wait counted on the monotonic clock still says the hour a person reads. The hour's cap
/// ([`HOUR_CAP`]) with its time is 72 bytes, the most aterm's Waiting row shows of a wait
/// (`needs.rs` `ROW_WHAT_MAX`), so the time is never cut off (review of f134d2b2cb, finding
/// 2: the cap that named its setting lost it).
#[must_use]
pub fn types_again_at(words: &str, at: std::time::Instant, now: std::time::Instant) -> String {
    let left = chrono::Duration::from_std(at.saturating_duration_since(now))
        .unwrap_or_else(|_| chrono::Duration::zero());
    let clock = (chrono::Local::now() + left).format("%H:%M");
    format!("{words}{TYPES_AGAIN}{clock}")
}

/// A wait's words without the clock time [`types_again_at`] put after them: a later wait that
/// moves the act (the host's wake settle) leaves the words, and their time is no longer when
/// it types.
#[must_use]
pub fn without_clock(shown: &str) -> &str {
    shown
        .split_once(TYPES_AGAIN)
        .map_or(shown, |(words, _)| words)
}
