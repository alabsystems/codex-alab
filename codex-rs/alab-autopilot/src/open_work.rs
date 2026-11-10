// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! WHAT A WORKER'S REPORT STILL NAMES OPEN (FOREVER CRUISE / FOREVER LAND, aterm owner
//! direction 2026-10-19): Cruise's reading ([`open_work`]), which names the open work a done
//! claim's defend prompt carries as a suffix ([`open_work_suffix`]), and Land's strict reading
//! ([`open_work_strict`]), which no blocker excuses and which counts work not on the remote
//! main branch: a defense that names any of it is no landing (`words::clean_defense`).
//!
//! PROVENANCE: ported verbatim from `~/aterm/crates/aterm-agent/src/supervise/policy/ turn_end.rs`
//! (`OPEN_HEADS` through `LACKING`, Land's word families [`LAND_FAMILIES`] and their readers among
//! them, [`land_open_words`], `open_work_suffix`, `open_named`, `OPEN_NAMED`), diffed item by item
//! against aterm main `66894933d2` (its fences read through [`crate::needs::code_fence`], as
//! aterm's are; the code is main's, the comments are those of the commits ported, not main's later
//! prunes). What that diff found ported since aterm main `6ff0b5475a`: Land's strict reader closing
//! the misses of a blind holdout (`4cacfb7913`: a path's colons and a thousands comma cut no
//! clause; the task's own items skipped, a fix in part, a check only type-checked or not driven end
//! to end, a test missing by its kind or of a claim, a decision or a fix left to a party, a scope
//! excuse and a test that would pass with its fix taken out read open; a test's proof by failing, a
//! run's whole tally, a mechanism retried and a thing kept on purpose read clean), a proof word
//! that names a failing check read as no proof and a vacuous test that needs its fix taken out
//! (`ac0c76b137`), and those proof and clean readings narrowed to their own clause
//! ([`proves_by_failing`], `149efe1190`), with the holdout they were measured on
//! (`fixtures/land-holdout-2026-10-20.tsv`); and the names main gives a change's number, a percent
//! and a count beside none (`CHANGE_NUMBER`, `PERCENT`, `COUNT_BESIDE_NONE`), with its `const fn`s.
//! Before, as aterm main `e0da8d68df` had it: Land's strict reading by word family and clause
//! (`d3e1dd842b`), the ninth review's holes in it closed (`44e4fa7d61`), the tenth and eleventh
//! reviews' read narrow (`0b4c90d4ec`, `500d97d8b3`: a settling rule settles a row only in the
//! row's own clause, by a predicate that lands it; a question read as the statement it asks), and
//! the integration review's leftovers read open (`78d1055bd9`: an ask of the person written without
//! its NEEDS YOU head; `55edc289b5`: a defense that names its own exception; `0e7ef0d916`), failing
//! toward open, with the labelled tables its tests read: Cruise's
//! (`fixtures/open-work-corpus.txt`), Land's (`fixtures/land-defense-table.txt`) and the needs
//! table (`fixtures/needs-table.txt`, [`crate::needs`]'), whose bodies are aterm main
//! `66894933d2`'s byte for byte. A local correction additionally keeps a negative control that
//! failed to perform its check (`failed to fail`) open; ordinary proof by failing still stands.
//! The rows codex-alab once
//! held ahead of aterm's `LAND_STATUS` (the review of 2a9241bbf, finding 7) are aterm's own now,
//! folded into its families and its defense table. The retired token line is left out of a report
//! through [`crate::words::is_finished_line`], and the doc links aterm's module resolves to its own
//! items name codex-alab's. [`land_open_words`] lives here, beside the reader it calls
//! (`open_work_read_as`, private to this module as it is to aterm's), and [`crate::words`]
//! re-exports it. [`open_work_suffix`] names its words through [`open_named`], as aterm's does, for
//! the acts aterm fits to what Claude Code's composer draws as typed ([`crate::words::fitted_act`],
//! which codex-alab ports so it types aterm's words).

use crate::needs;
use crate::words::is_finished_line;
use crate::words::says_done;

/// The heads a report lists open work under, lowercased ([`open_work`]): a
/// sentence that begins with one (Markdown's list, heading, quote and
/// emphasis marks dropped) and ends there, or goes on with `:`, `(`, ` -`,
/// ` –` or ` —`, after at most one of [`OPEN_NOUNS`] (`Remaining work:`,
/// `Not done yet:`), or with a short tail that a colon closes, after `,`,
/// `from` or `for` (`Still open from AU-1:`, `Open items, all out of
/// scope:`, `Follow-ups for the owner:`). A Markdown heading that begins with
/// one is a head whatever follows a comma (`## Not measured, and why`).
/// `Left the config as it was` and `Remaining tests pass` are no head. The
/// challenge names what matched in these words.
pub const OPEN_HEADS: &[&str] = &[
    "still open",
    "still to do",
    "left open",
    "open items",
    "open issues",
    "open work",
    "open tasks",
    "open",
    "to do",
    "todo",
    "todos",
    "left",
    "not done",
    "not finished",
    "unfinished",
    "remaining",
    "what remains",
    "what's left",
    "what is left",
    "follow-up",
    "follow-ups",
    "follow up",
    "follow ups",
    "followup",
    "followups",
    "next step",
    "next steps",
    "outstanding",
    "pending",
    "deferred",
    "skipped",
    "not run",
    "not verified",
    "not tested",
    "untested",
    "not measured",
    "unmeasured",
];

/// The longest tail, in bytes, a head may carry before its colon
/// ([`OPEN_HEADS`]): `Still open from the implementer, and still true:`.
const HEAD_TAIL: usize = 60;

/// The nouns a head may carry before its colon ([`OPEN_HEADS`]).
const OPEN_NOUNS: &[&str] = &[
    "items", "item", "issues", "work", "tasks", "steps", "points", "to do", "yet",
];

/// The words that leave work open wherever they stand in a sentence
/// ([`open_work`]): `the e2e suite was not run`, `Tests: skipped`, `The
/// lexer is still open`, `I did not find the cause`, `Observed, not
/// explained`. A report's `didn't` reads as `did not`, and `not yet run` or
/// `not been run` as `not run`. Not after a word of [`OPEN_NEGATIONS`] in
/// the same clause (`nothing was skipped`), nor `not …` after a modal
/// ([`OPEN_MODALS`]: `do not run it twice`) or `skipped` after `is`, `are`
/// or `be` (`a warm cache is skipped`): those describe; they report nothing
/// undone. Nor inside quotation marks (`the "window still open" check`),
/// where a phrase is mentioned, not reported; nor before a reason that says
/// there was nothing to do (`not run, because no contract was added`).
/// Its `still fails` and kin report a check that is red now (`but one test
/// still fails`), not one that failed and was fixed.
pub const OPEN_STATUS: &[&str] = &[
    "not run",
    "skipped",
    "not done",
    "not verified",
    "not tested",
    "untested",
    "not measured",
    "unmeasured",
    "not re-measured",
    "not re-run",
    "not rerun",
    "not checked",
    "unchecked",
    "not compiled",
    "uncompiled",
    "not reproduced",
    "not validated",
    "not observed",
    "unobserved",
    "not exercised",
    "not explained",
    "unexplained",
    "not fixed",
    "unfixed",
    "not confirmed",
    "unconfirmed",
    "not closed",
    "not identified",
    "unidentified",
    "not passed",
    "still open",
    "remains open",
    "left open",
    "left for",
    "follow-up",
    "follow-ups",
    "needs a re-run",
    "need a re-run",
    "needs re-running",
    "did not measure",
    "did not test",
    "did not verify",
    "did not check",
    "did not look",
    "did not find",
    "did not track",
    "did not investigate",
    "did not fix",
    "did not confirm",
    "still fails",
    "still fail",
    "still failing",
];

/// WHAT LAND READS AS OPEN BESIDE [`OPEN_STATUS`] ([`open_work_strict`]):
/// work not on the remote main branch, wherever it stands in a sentence
/// (`nothing is parked or left for later` is clean). Land lands everything,
/// so `WIP`, `parked` and `left on a branch` are each work still to land.
/// The first of [`LAND_FAMILIES`]; a later day (`for later`) is
/// [`LAND_LATER`].
pub const LAND_STATUS: &[&str] = &[
    "not landed",
    "unlanded",
    "not pushed",
    "unpushed",
    "not merged",
    "unmerged",
    "not committed",
    "uncommitted",
    "parked",
    "wip",
    "work in progress",
    "left on a branch",
    "on its own branch",
    "follow-up",
    "follow-ups",
    "follow up",
    "followup",
    "followups",
    "follow-on",
    "follow-ons",
    "follow on",
    "follow ons",
    "untracked",
    "unstaged",
    "modified files",
    "modified file",
    "without landing",
    "without merging",
    "without pushing",
    "without committing",
    "without being merged",
    "without being applied",
    "without applying",
    // A landing that did not reach the remote (the integration review of
    // 12449ff581): main moved before the push, a dry run, a change staged.
    "moved again before",
    "moved before i pushed",
    "moved before i could",
    "before i could push",
    "dry run",
    "dry-run",
    "--dry-run",
    "staged for commit",
    "staged for",
    "is staged",
    "are staged",
    "staged changes",
    "is modified",
    "are modified",
    "still modified",
    "left modified",
    "shows m",
    "still on disk",
    "left the branch",
    "left the branches",
    "left the worktree",
    "left a branch",
];

/// A LATER DAY ([`LAND_FAMILIES`]): work put off is work still to land —
/// unless the sentence says it happens on its own ([`AUTOMATIC`]: `the
/// nightly job picks it up tomorrow on its own`), which leaves nobody
/// anything to do. Plain `next session` is no row: the corpus uses it as a
/// description (`at its next session spawn`).
pub const LAND_LATER: &[&str] = &[
    "for later",
    "later",
    "tomorrow",
    "tonight",
    "next week",
    "next month",
    "next sprint",
    "next quarter",
    "next round",
    "next pass",
    "next iteration",
    "in the next session",
    "for the next session",
    "a later session",
    "a future session",
    "another session",
    "another day",
    "another time",
    "in the future",
    "future work",
    "eventually",
    "someday",
    "some day",
    "at some point",
    "down the line",
    "for now",
    "for the time being",
    "until later",
    "whenever",
    "scheduled for",
    "planned for",
    "is planned",
    "is scheduled",
    "slated for",
    "are planned",
    "in due course",
    "when time permits",
    "time permitting",
    "pencilled in",
    "penciled in",
    "next release",
    "next restart",
    "next nightly",
    "next deploy",
    "next rebuild",
    "by monday",
    "by tuesday",
    "by wednesday",
    "by thursday",
    "by friday",
    "by the weekend",
    "by end of day",
    "by eod",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "this morning",
    "this afternoon",
    "this evening",
    "in the morning",
    "in the afternoon",
    "in the evening",
    "in a bit",
    "in a while",
    "in a minute",
    "in a few minutes",
    "in a few hours",
    "in a few days",
    "in an hour",
    "shortly",
    "asap",
    "over the weekend",
    "this weekend",
    "next time",
    "next session picks",
    "next session will",
    "next session can",
    "phase 2",
    "phase two",
    "phase ii",
    "post-freeze",
    "after the freeze",
    "next milestone",
    "next drop",
    "set for",
    "penciled for",
    "pencilled for",
    "is in the pipeline",
    "are in the pipeline",
    "still in the pipeline",
    "goes out with",
    "go out with",
    "comes with",
    "come with",
    "lands with",
    "land with",
    "ships with",
    "ship with",
    "when i get a chance",
    "when i'm back",
    "when i am back",
    "bumped to",
    "pushed back",
    "pushed out to",
    "afterwards",
    "afterward",
];

/// The [`LAND_LATER`] rows a past landing in their clause dates instead
/// ([`Reads::Later`]): `merged into origin/main this morning`, `shipped on
/// Monday`, never `the docs follow in the morning`.
const DATED: &[&str] = &[
    "afterwards",
    "afterward",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "this morning",
    "this afternoon",
    "this evening",
    "in the morning",
    "in the afternoon",
    "in the evening",
    "shortly",
];

/// What a [`LAND_LATER`] row ending in `with` must meet to name a later day
/// ([`Reads::Later`]): `the docs land with v1.5`, `goes out with Monday's
/// train`, never `the fix comes with a regression test`.
const LATER_WITH: &[&str] = &[
    "next",
    "release",
    "drop",
    "train",
    "milestone",
    "sprint",
    "version",
    "update",
    "batch",
    "cut",
    "rc",
    "monday's",
    "tuesday's",
    "wednesday's",
    "thursday's",
    "friday's",
];

/// How Land reads one family of its leftover words ([`LandFamily`]).
#[derive(Clone, Copy, Debug)]
enum Reads {
    /// Wherever a row stands, under the common rules ([`land_open`]).
    Anywhere,
    /// Only as a predicate standing alone ([`alone`]): at the end of its
    /// clause or before a preposition (`two items remain`, `remains to be
    /// done`, `stays until the fix ships`), never before a complement (`the
    /// API remains backward compatible`).
    Alone,
    /// Only beside one of these nouns in its stretch of the clause
    /// ([`stretch`]): `CI is running`, `the PR is open`, `needs a review`,
    /// never `the server is running` or `waits until the flush finishes`.
    Beside(&'static [&'static str]),
    /// An exception to the claim ([`excepting`]): `all merged except the
    /// docs`.
    Exception,
    /// A later day ([`LAND_LATER`]), unless it happens on its own.
    Later,
    /// What the worker means to do (`I will`), unless what follows denies
    /// it (`I will start no new work`).
    Intent,
    /// A state kept until something else happens (`the feature is off by
    /// default until it has been tested more`, `coexist until the
    /// switch-over`), never a mechanism that waits ([`MECHANISMS`]: `the
    /// writer waits until the flush finishes`), nor what happens on its own.
    Until,
    /// A clause that ends on a bare negated auxiliary (`the hard part is
    /// done, the polish isn't`).
    Elided,
    /// A need for more work, where a gerund or a thing to add names it next
    /// (`the docs page needs updating`, `need tuning`, `needs a refresh`), or
    /// the worker's own need to do it (`Need to double-check the migration`,
    /// `we need to rerun it`), never `needs nothing` nor what a thing needs
    /// to work (`the user needs to restart`).
    Gerund,
    /// `still` before what is no lasting good ([`STILL_GOOD`]): `the
    /// regression is still there`, `still panics`, `still uses the old
    /// API` — never `the old syntax still parses`.
    Still,
    /// A remote's branch other than main (`origin/feature-x`): work pushed
    /// where no remote main has it.
    Ref,
    /// A count short of the whole (`three of the five call sites`).
    Fraction,
    /// Work the worker did not do (`I didn't port the tests`, `have not
    /// pushed the tag`), never harm it avoided ([`AVOIDED`]: `did not change
    /// the API`, `did not start new work`).
    NotDone,
    /// Something that no longer works ([`WORKS`]: `the README example doesn't
    /// compile any more`, `no longer builds`), never a fault that no longer
    /// happens (`no longer hangs`).
    Lost,
    /// Work that could still be done (`could be improved`, `might be
    /// cleaner`).
    Better,
    /// A concession after a landing claim (`merged, though the help text
    /// needs a refresh`), unless what it concedes is itself settled or
    /// denied ([`conceding`]).
    Concession,
    /// A state of the work wherever it stands (`the docs are in progress`,
    /// `the migration is paused`, `skipping the docs`), unless a control
    /// follows ([`CONTROLS`], `while`: `the field is omitted when empty`),
    /// which describes code, or what it names is an attempt given up for a
    /// better one ([`ATTEMPTS`]: `abandoned the first approach`).
    State,
    /// Only at the end of its clause (`docs: soon`, `docs next`, `the docs
    /// nit aside`, `One thing: the changelog`), never before more words
    /// (`soon after the rebase`) nor after `too`, `as` or `so` (`fired too
    /// soon`).
    End,
    /// `left` at the end of its clause or before `to`, after a count (`one PR
    /// left`, `two small items left`, `two PRs left to merge`): what is
    /// left, counted.
    Count,
    /// `one more` before a thing still to do (`one more PR to merge`), never
    /// `one more time` nor one its clause settles or denies (`one more round
    /// passed`, `one more look found nothing`, `merged after one more round
    /// of review`).
    OneMore,
    /// A sentence that names only what is left (`Only the changelog.`): it
    /// opens with the row and holds no verb ([`verbish`]).
    Only,
    /// A thing named before `after` or `once` with no verb (`docs after the
    /// release`, `docs once the API settles`): work put off to an event,
    /// unless the sentence settles it (`CI after the rebase: green`) or, after
    /// `after`, goes on with its verb (`docs after the rename are correct`).
    Elliptic,
    /// A gap stated as an absence of what a change owes (`no tests`, `There
    /// is no test for the error path`): at the end of its clause or before a
    /// preposition ([`GAP_PREPOSITIONS`]) — never `no tests fail` or `no
    /// tests to add`.
    Absent,
    /// Nobody did a check or a landing step (`no one has run it on
    /// Windows`, `nobody reviewed it`, [`CHECKED`]).
    Nobody,
    /// A landing word a negation denies (`nothing has been merged`, `the
    /// branch was never pushed`, `no PR was merged`, `none of it is on
    /// origin/main`): the landing it would claim did not happen
    /// ([`landing_denied`]).
    Unlanded,
    /// A word for work done that a negation denies with the work for its
    /// subject (`none of the docs are written`, `no tests have been run`):
    /// the work was not done ([`landing_denied`]). Never `nothing is
    /// written to the old path`, which names no work.
    Undone,
    /// Work pushed or merged onto a branch other than main (`pushed to
    /// fix/resize-race`, `merged into develop`, [`onto_branch`]).
    Onto,
    /// Most of a whole claimed landed (`most of the PRs are merged`,
    /// `almost everything is merged`): the rest is not. Never a part that
    /// claims no landing (`most of the diff is tests`).
    Most,
    /// Work given up or undone (`the refactor was dropped`, `the PR was
    /// merged, then reverted`), never a stopgap, an old thing or a bad one
    /// gone ([`disposable`]: `the workaround was dropped`, `reverted the bad
    /// commit`).
    Dropped,
    /// Work on a named branch other than main (`the fix is on
    /// fix/resize-race`, `2 commits on fix/x`, `on the release branch`,
    /// [`on_branch`]), unless the sentence says main has it ([`main_claimed`]).
    OnBranch,
    /// A change request (`PR #12`, `#45`, `the docs PR`) as the subject of a
    /// clause that does not land it (`PR #12 is approved`, `#45 is green and
    /// waiting`, `the blocked PR is unblocked`): an unmerged change, whatever
    /// its state ([`change_open`]). Settled by its landing (`PR #12 is
    /// merged`, `PR #12: merged`), never named in a phrase (`the fix for PR
    /// #12`, `merged as #12`).
    Change,
    /// A part of a whole by its percent (`90% of it is merged`, `about 95%
    /// merged`, `0% coverage`), never one that claims no part (`20% faster`).
    Percent,
    /// A party named as the whole item of a label (`release: owner`,
    /// `tagging: you`): the work is that party's to do.
    Party,
    /// What names only the item before it (`the tag, nothing more`, `the
    /// docs, that's all`): that item is what is left ([`only_item`]).
    OnlyTail,
    /// A none narrowed by what follows it (`nothing important`, `no real
    /// blockers`, `nothing that can't wait`, `none of the remaining items are
    /// urgent`): it concedes the rest ([`qualified_nil`]).
    Qualified,
    /// A count above zero of what a run reports wrong (`211 passed, 3
    /// errors`, `1 panicked`, `2 failures in CI`), never a count of none (`0
    /// errors`) nor one that names a thing's kind (`one error message is
    /// clearer`): its next word ends the clause, joins, or goes on with a
    /// verb or a preposition.
    Tally,
    /// Work shipped without what it owes (`merged without the docs`, `the
    /// release went out without updating the changelog`, [`OWED`]), never
    /// without a fault (`merged without conflicts`).
    Without,
    /// Work approved or reviewed and no more (`the fix is approved`, `the
    /// change is reviewed, approved and queued`): beside a work noun in its
    /// stretch ([`APPROVED_THINGS`]), unless the clause goes on to land it
    /// (`the fix is approved and merged`) or a clause right after with no
    /// subject does (`the fix was reviewed by the owner; merged to
    /// origin/main`).
    Approval,
    /// A check run again until it passed (`passed on rerun`, `green on the
    /// second run`, `retried twice`, `flaked once`): a flake, whatever run
    /// passed after it (a check never retries) — never a landing step the
    /// sentence names before it ([`RETRIED_STEPS`]: `the push succeeded on
    /// the second try`).
    Rerun,
    /// A status said as a label's whole item (`appkit: skip`, `Perf pass:
    /// next session`, `Docs: to do`, a table's `| appkit | ⏭️ skip |`), after
    /// a count above zero (`27 pass, 1 skip`), or, for a skip, opening its
    /// sentence with no verb after it (`SKIP appkit (--no-windows)`): never
    /// the word inside a sentence (`the reader skips quoted rows`).
    Status,
}

impl Reads {
    /// Whether the family's rows are words a clean defense uses all the
    /// time (`no`, `merged`, `after`, `only`), open only in the shape its
    /// reading names: such a row is no other leftover beside an exception
    /// ([`excepting`]: `other than main, no branch exists`).
    const fn common(self) -> bool {
        matches!(
            self,
            Self::End
                | Self::Party
                | Self::OnlyTail
                | Self::Qualified
                | Self::Count
                | Self::OneMore
                | Self::Only
                | Self::Elliptic
                | Self::Absent
                | Self::Nobody
                | Self::Unlanded
                | Self::Undone
                | Self::Onto
                | Self::Most
                | Self::OnBranch
                | Self::Change
                | Self::Percent
                | Self::Tally
                | Self::Without
                | Self::Approval
                | Self::Status
        )
    }
}

/// ONE FAMILY OF WHAT LAND READS AS WORK STILL TO LAND
/// ([`open_work_strict`]): its rows, as whole words in a report's lowercased
/// words, how a row counts there ([`Reads`]), and what settles a row that
/// stands as a description (`the pending PR was merged`, [`settled`]). Every
/// family is load-bearing: the labelled defense table
/// (`fixtures/land-defense-table.txt` beside this file) holds a defense only
/// it reads open.
#[derive(Clone, Copy)]
pub(crate) struct LandFamily {
    /// What the family is, for the defense table's failure message.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "named by the defense table's mutation check")
    )]
    pub(crate) name: &'static str,
    /// Its rows, lowercased; a nudge names the one that matched, or
    /// [`Self::says`].
    rows: &'static [&'static str],
    reads: Reads,
    /// What says a row standing as a description is done: `merged` after
    /// `the pending PR`.
    settled_by: &'static [&'static str],
    /// What a nudge names for any of its rows, where the row alone would
    /// name the opposite (`merged` in `nothing has been merged` is `not
    /// landed`).
    says: Option<&'static str>,
    phrases: Phrases,
}

impl LandFamily {
    const fn new(
        name: &'static str,
        rows: &'static [&'static str],
        reads: Reads,
        settled_by: &'static [&'static str],
    ) -> Self {
        Self {
            name,
            rows,
            reads,
            settled_by,
            says: None,
            phrases: Phrases::of(rows),
        }
    }

    /// The family, its rows named as `says` by a nudge.
    const fn saying(self, says: &'static str) -> Self {
        Self {
            says: Some(says),
            ..self
        }
    }
}

/// What settles a leftover word that stands as a description
/// ([`settled`]): `the pending PR was merged`, `all 3 follow-ups are done
/// and merged`, `the flaky test is fixed`. `pushed` is none: a branch can be
/// pushed and still not landed.
const SETTLED: &[&str] = &[
    "merged",
    "landed",
    "done",
    "fixed",
    "resolved",
    "closed",
    "approved",
    "reviewed",
    "re-enabled",
    "reenabled",
    "enabled",
    "passes",
    "pass",
    "passed",
    "passing",
    "green",
    "removed",
    "deleted",
    "dropped",
    "gone",
    "addressed",
    "finished",
    "completed",
    "complete",
    "stable",
    "deflaked",
    "unblocked",
    "cleared",
    "applied",
    "popped",
    "answered",
    "empty",
    "investigated",
    "explained",
    "succeeded",
    "succeeds",
];

/// WHAT LANDS WORK, and so settles a landing leftover ([`settled`]): `the
/// pending PR was merged`, `the parked lexer has landed`, or a place on the
/// remote main branch ([`settles_at`]: `the follow-up is on remote main`, `the
/// unpushed commits are pushed to origin/main`). FAIL TOWARD OPEN: nothing
/// that leaves the landing to come settles one — `approved`, `reviewed`,
/// `green`, `done`, `rebased`, `enabled` (`auto-merge is enabled`), `applied`
/// (`the stashed change is applied`) or `unblocked` (`the blocked PR is
/// unblocked`).
const LANDS: &[&str] = &[
    "merged",
    "landed",
    "squash-merged",
    "rebase-merged",
    "auto-merged",
    "upstreamed",
];

/// [`LANDS`], and what ships a release a block held (`the release blocked on
/// the key is published`).
const SHIPS: &[&str] = &[
    "merged",
    "landed",
    "squash-merged",
    "rebase-merged",
    "auto-merged",
    "upstreamed",
    "published",
    "released",
    "shipped",
    "deployed",
];

/// What settles a fault, a gap or an item named as a thing ([`settled`]):
/// its being fixed, done or gone (`the TODO in parser.rs is resolved`, `the
/// known issue is fixed`), never its landing (`the TODO is merged` is the
/// marker landed).
const MENDS: &[&str] = &[
    "fixed",
    "resolved",
    "addressed",
    "closed",
    "removed",
    "deleted",
    "corrected",
    "done",
    "gone",
    "cleared",
    "finished",
    "completed",
    "complete",
    "investigated",
    "explained",
    "answered",
];

/// What settles a check's state ([`settled`]): its passing, its fix, its
/// coming back (`the failing test passes`, `the disabled test is
/// re-enabled`). A flaky one takes only its fix ([`DEFLAKED`]).
const CHECKS: &[&str] = &[
    "passes",
    "pass",
    "passed",
    "passing",
    "green",
    "fixed",
    "resolved",
    "succeeded",
    "succeeds",
    "stable",
    "deflaked",
    "re-enabled",
    "reenabled",
    "enabled",
    "removed",
    "deleted",
    "done",
    "finished",
    "completed",
    "complete",
    "investigated",
    "explained",
];

/// What settles a flaky check ([`settled`]): its fix, never a green run
/// (`the flaky test is green` is one run that passed, `stable after 20
/// reruns` twenty).
const DEFLAKED: &[&str] = &["fixed", "resolved", "deflaked", "removed", "deleted"];

/// The rows of the check family a green run does not settle ([`DEFLAKED`]).
const FLAKY: &[&str] = &[
    "flaky",
    "flakes",
    "flaking",
    "intermittently",
    "intermittent",
    "sporadically",
    "unstable",
];

/// What settles a failed landing step or check ([`settled`]): [`CHECKS`] and
/// [`LANDS`] (`the push failed once, then succeeded`).
const STEPS: &[&str] = &[
    "passes",
    "pass",
    "passed",
    "passing",
    "green",
    "fixed",
    "resolved",
    "succeeded",
    "succeeds",
    "merged",
    "landed",
];

/// What settles a review or a question a row waits on, where its noun phrase
/// names one ([`REVIEW_NOUNS`], [`settled`]): `the pending review is done`,
/// `the open question is answered`. Never a change's: `the pending PR is
/// approved` leaves the merge to come.
const REVIEWED: &[&str] = &[
    "approved",
    "reviewed",
    "done",
    "given",
    "answered",
    "received",
    "addressed",
    "resolved",
    "finished",
    "completed",
    "decided",
];

/// What a row's noun phrase names that a review settles ([`REVIEWED`]).
const REVIEW_NOUNS: &[&str] = &[
    "review",
    "reviews",
    "approval",
    "approvals",
    "sign-off",
    "signoff",
    "feedback",
    "question",
    "questions",
    "decision",
    "decisions",
    "answer",
    "comment",
    "comments",
    "thread",
    "threads",
];

/// What a row's noun phrase names that only a landing settles ([`settled`]):
/// a change, whatever else its family takes (`the open PR is closed` leaves
/// it unmerged).
const CHANGE_NOUNS: &[&str] = &[
    "pr", "prs", "mr", "mrs", "request", "requests", "branch", "branches", "commit", "commits",
    "patch", "patches", "stash", "draft",
];

/// What settles an [`OPEN_STATUS`] row named as a thing ([`settled`]):
/// [`CHECKS`] and [`MENDS`] (`the skipped test is re-enabled`, `the
/// unexplained stall is explained`).
const STATUS_BY: &[&str] = &[
    "passes",
    "pass",
    "passed",
    "passing",
    "green",
    "succeeded",
    "succeeds",
    "stable",
    "deflaked",
    "re-enabled",
    "reenabled",
    "enabled",
    "merged",
    "landed",
    "fixed",
    "resolved",
    "addressed",
    "closed",
    "removed",
    "deleted",
    "corrected",
    "done",
    "gone",
    "finished",
    "completed",
    "complete",
    "investigated",
    "explained",
    "answered",
];

/// What settles a stopgap or a part ([`settled`]): only its going away
/// (`the workaround is removed`). `the workaround is merged` is the
/// stopgap landed, still to be replaced.
const RETIRED: &[&str] = &[
    "removed",
    "cleaned",
    "deleted",
    "dropped",
    "reverted",
    "replaced",
    "gone",
    "retired",
    "completed",
    "complete",
    "finished",
];

/// What settles stale words ([`settled`]): `the outdated screenshots were
/// replaced`.
const REFRESHED: &[&str] = &[
    "updated",
    "refreshed",
    "replaced",
    "rewritten",
    "corrected",
    "fixed",
    "removed",
    "deleted",
    "regenerated",
];

/// What a wait is on ([`Reads::Beside`] for the wait verbs): a review, an
/// approval, a decision, a person, a check, a person's or a service's
/// secret (`#secret`, [`noun_in`]: `requires Andrew's token`).
const WAIT_NOUNS: &[&str] = &[
    "review",
    "reviews",
    "reviewer",
    "reviewers",
    "reviewing",
    "approval",
    "approvals",
    "approve",
    "sign-off",
    "signoff",
    "decision",
    "decide",
    "decides",
    "merge",
    "merging",
    "landing",
    "owner",
    "person",
    "someone",
    "somebody",
    "ci",
    "rebase",
    "rebasing",
    "re-run",
    "rerun",
    "cleanup",
    "follow-up",
    "answer",
    "feedback",
    "go-ahead",
    "confirmation",
    "permission",
    "credential",
    "credentials",
    "qa",
    "sign",
    "signs",
    "major",
    "version",
    "upstream",
    "release",
    "vendor",
    "vendors",
    "maintainer",
    "maintainers",
    "team",
    "#secret",
];

/// What a check is ([`Reads::Beside`] for a check's states).
const CHECK_NOUNS: &[&str] = &[
    "ci",
    "build",
    "builds",
    "pipeline",
    "pipelines",
    "check",
    "checks",
    "suite",
    "suites",
    "lane",
    "lanes",
    "workflow",
    "workflows",
    "job",
    "jobs",
    "test",
    "tests",
    "run",
    "runs",
    "benchmark",
    "benchmarks",
    "bench",
    "soak",
    "e2e",
    "snapshot",
    "cell",
    "cells",
    "matrix",
    "lint",
    "lints",
    "warning",
    "warnings",
    "assertion",
    "assertions",
    "threshold",
    "tolerance",
    "timeout",
    "clippy",
    "tippy",
    "linter",
    "rustfmt",
    "fmt",
    "p50",
    "p90",
    "p95",
    "p99",
    "p999",
    "latency",
    "latencies",
    "throughput",
    "perf",
    "performance",
];

/// What stays a branch when it is current with main ([`Reads::Beside`] for
/// `up to date with`): `the branch fix/race is up to date with origin/main`,
/// a branch by its name (`#branch`, [`noun_in`]), never `local main is up to
/// date with origin/main`.
const BRANCH_THINGS: &[&str] = &[
    "branch", "branches", "fork", "worktree", "pr", "prs", "mr", "#branch",
];

/// What can sit on the worker's machine unlanded ([`Reads::Beside`] for
/// `my machine`): `the commit hasn't left my machine`, never `reproduced the
/// hang on my laptop`.
const LOCAL_THINGS: &[&str] = &[
    "commit", "commits", "branch", "branches", "tag", "tags", "change", "changes", "work", "fix",
    "fixes", "patch", "patches", "release", "stash", "worktree", "checkout", "copy",
];

/// What a worker makes that stays on its machine when it is made `locally`
/// ([`Reads::Beside`]): `tagged v1.2.0 locally`, `cut the release locally`,
/// never `tested locally` or `deleted locally`.
const MADE_LOCALLY: &[&str] = &[
    "tagged",
    "tag",
    "tags",
    "committed",
    "merged",
    "cut",
    "released",
    "amended",
    "staged",
];

/// What a fault is ([`Reads::Beside`] for a fault come back): `the outage is
/// back`, `the race recurred`.
const FAULT_NOUNS: &[&str] = &[
    "bug",
    "bugs",
    "outage",
    "crash",
    "crashes",
    "hang",
    "hangs",
    "leak",
    "leaks",
    "race",
    "regression",
    "regressions",
    "flake",
    "flakes",
    "failure",
    "failures",
    "error",
    "errors",
    "problem",
    "problems",
    "issue",
    "issues",
    "panic",
    "panics",
    "deadlock",
    "stall",
    "stalls",
    "glitch",
    "slowdown",
];

/// What a landing step or a check is ([`Reads::Beside`] for `failed`): `the
/// push failed with a 403`, never `the parse failed fast by design` alone.
const STEP_NOUNS: &[&str] = &[
    "push",
    "merge",
    "rebase",
    "commit",
    "upload",
    "deploy",
    "release",
    "publish",
    "install",
    "build",
    "builds",
    "test",
    "tests",
    "check",
    "checks",
    "ci",
    "job",
    "jobs",
    "lane",
    "lanes",
    "run",
    "suite",
    "pipeline",
    "hook",
    "signing",
    "notarization",
];

/// What stays open until someone closes it ([`Reads::Beside`] for
/// `open`): a change under review, an issue, a question, one named by its
/// number (`#n`, [`noun_in`]: `#45 is open`).
const OPEN_NOUNS_LAND: &[&str] = &[
    "pr",
    "prs",
    "request",
    "requests",
    "mr",
    "mrs",
    "issue",
    "issues",
    "ticket",
    "tickets",
    "question",
    "questions",
    "point",
    "points",
    "decision",
    "decisions",
    "thread",
    "threads",
    "comment",
    "comments",
    "discussion",
    "discussions",
    "case",
    "cases",
    "bug",
    "bugs",
    "gap",
    "gaps",
    "problem",
    "problems",
    "item",
    "items",
    "task",
    "tasks",
    "#n",
];

/// THE FAMILIES OF LAND'S STRICT READING beyond [`OPEN_STATUS`]
/// ([`open_work_strict`]), in the order a nudge names what matched. Each is
/// a kind of leftover a defense can admit and no repository probe sees: a
/// wait on a person or a check, a later day, a check that could not run, a
/// stopgap, a known issue.
pub(crate) static LAND_FAMILIES: [LandFamily; 68] = [
    LandFamily::new("unlanded", LAND_STATUS, Reads::Anywhere, LANDS),
    LandFamily::new(
        "on a branch",
        &[
            "on a branch",
            "on its branch",
            "on my branch",
            "on a feature branch",
            "on the feature branch",
            "on a side branch",
            "on a separate branch",
            "on a local branch",
            "my local branch",
            "to my branch",
            "to a branch",
            "to a feature branch",
            "to its branch",
            "in a worktree",
            "in my worktree",
            "commits behind",
            "commit behind",
            "commits ahead",
            "commit ahead",
            "ahead of main",
            "ahead of origin",
            "ahead by",
            "behind my branch",
            "on my fork",
            "to my fork",
            "in my fork",
            "on a fork",
            "merged locally",
            "main locally",
            "local main",
            "keeping the worktree",
            "kept the worktree",
            "keep the worktree",
            "keeping the branch",
            "kept the branch",
            "keep the branch",
            "not on main",
            "not on origin",
            "not on the remote",
            "ahead of",
            "into my branch",
            "into my fork",
            "into a branch",
            "into a feature branch",
            "into the feature branch",
            "in my branch",
            "in a branch",
            "in a feature branch",
            "is ahead",
            "are ahead",
            "ahead again",
            "my main",
            "main in the worktree",
            "main in my worktree",
            "main on my machine",
            "main on this machine",
            "main on this mac",
            "main is unchanged",
            "main unchanged",
            "main is untouched",
            "main untouched",
            "left main alone",
            "revision behind",
            "revisions behind",
            "version behind",
            "versions behind",
            "is behind",
            "are behind",
            "still behind",
        ],
        Reads::Anywhere,
        LANDS,
    ),
    LandFamily::new(
        "current with main",
        &[
            "up to date with",
            "up-to-date with",
            "in sync with",
            "current with",
            "rebased onto",
            "rebased on",
        ],
        Reads::Beside(BRANCH_THINGS),
        LANDS,
    )
    .saying("on a branch"),
    LandFamily::new(
        "local",
        &[
            "still local",
            "only local",
            "local only",
            "local-only",
            "committed locally",
            "only locally",
            "is local",
            "are local",
            "away from done",
            "merge away",
            "push away",
            "commit away",
            "created locally",
            "tagged locally",
            "locally tagged",
            "kept locally",
            "sits locally",
            "updated locally",
        ],
        Reads::Anywhere,
        LANDS,
    ),
    LandFamily::new(
        "on my machine",
        &[
            "my machine",
            "my laptop",
            "my computer",
            "my box",
            "my workstation",
            "this machine",
            "this host",
        ],
        Reads::Beside(LOCAL_THINGS),
        LANDS,
    )
    .saying("local"),
    LandFamily::new(
        "made locally",
        &["locally"],
        Reads::Beside(MADE_LOCALLY),
        LANDS,
    )
    .saying("local"),
    LandFamily::new(
        "untouched",
        &["untouched", "not touched", "never touched", "unstarted"],
        Reads::Beside(WORK_ITEMS),
        SHIPS,
    )
    .saying("untouched work"),
    LandFamily::new("without", &["without"], Reads::Without, &[]).saying("work owed"),
    LandFamily::new(
        "approved only",
        &[
            "approved",
            "reviewed",
            "accepted",
            "signed off",
            "lgtm",
            "lgtm'd",
        ],
        Reads::Approval,
        LANDS,
    )
    .saying("approved, not landed"),
    LandFamily::new(
        "stash",
        &["stash", "stashed", "stashes"],
        Reads::Anywhere,
        LANDS,
    ),
    LandFamily::new("draft", &["draft", "drafts"], Reads::Anywhere, LANDS),
    LandFamily::new(
        "left over",
        &[
            "remaining",
            "left over",
            "leftover",
            "leftovers",
            "left to do",
            "left to land",
            "left to be",
            "left to the",
            "left to finish",
            "still to do",
            "still to land",
            "still to be",
            "yet to",
            "still have to",
            "still has to",
            "still need to",
            "still needs to",
            "has to be",
            "have to be",
            "tbd",
            "to be done",
            "to be decided",
            "what remains",
            "thing left",
            "things left",
            "item left",
            "items left",
            "work left",
            "next step",
            "next steps",
            "next task",
            "next tasks",
            "next thing",
            "next item",
            "next items",
            "next job",
            "next change",
            "next pr",
            "next piece",
            "next up",
            "up next",
            "to come",
            "outstanding",
            "should be",
            "must be",
            "needs to be",
            "ought to be",
            "lingering",
            "residual",
            "dangling",
            "stray",
            "last step",
            "final step",
            "last thing",
            "one last",
            "caveat",
            "caveats",
            "as an exercise",
            "for completeness",
            "to go",
            "on my list",
            "on the list",
            "to-do",
            "owed",
            "i owe",
            "still owe",
            "tba",
            "tbc",
            "by hand",
            "manual step",
            "only fixed",
            "only patched",
            "only papered",
        ],
        Reads::Anywhere,
        LANDS,
    ),
    LandFamily::new(
        "remains",
        &[
            "remains",
            "remain",
            "is left",
            "are left",
            "still left",
            "stays",
            "stay",
            "is next",
            "are next",
            "comes next",
            "come next",
            "can wait",
            "will wait",
            "has to wait",
            "yet",
            "mostly",
            "blocked",
            "missing",
        ],
        Reads::Alone,
        &[],
    ),
    LandFamily::new(
        "exception",
        &[
            "except",
            "apart from",
            "aside from",
            "other than",
            "save for",
            "but for",
            "besides",
            "excluding",
            "with the exception of",
            "barring",
            "minus",
            "modulo",
            "one exception",
            "an exception",
            "a single exception",
            "only exception",
            "excepting",
            "excepted",
            "sans",
            "but",
            "bar",
            "save",
            "less",
            "beyond",
            "without",
            "exception",
            "the exception",
            "one gap",
            "the one gap",
            "only gap",
            "the only gap",
            "single gap",
            "not included",
            "not including",
            "not counting",
            "don't count",
            "do not count",
            "excluded",
            "exclusive of",
            "short of",
            "notwithstanding",
            "leaving out",
            "outside of",
            "not the",
            "omission",
            "omissions",
            "one omission",
            "holdout",
            "holdouts",
            "the holdout",
            "one holdout",
            "well, nearly",
            "not withstanding",
            "short one",
            "short by one",
            "one short",
            "give or take",
        ],
        Reads::Exception,
        &[],
    ),
    LandFamily::new(
        "qualified none",
        &[
            "important",
            "big",
            "urgent",
            "blocking",
            "critical",
            "serious",
            "major",
            "real",
            "hard",
            "significant",
            "pressing",
            "must-have",
            "must-haves",
            "showstopper",
            "showstoppers",
            "show-stopper",
            "show-stoppers",
            "that blocks",
            "that matters",
            "that can't wait",
            "that cannot wait",
            "that can not wait",
            "in scope",
            "critical path",
            "today",
            "beyond",
            "worth",
            "of note",
            "noteworthy",
            "substantial",
            "essential",
            "vital",
            "high-priority",
            "high priority",
            "release-blocking",
            "material",
            "meaningful",
            "large",
            "huge",
            "on my side",
            "on my end",
            "from me",
            "for now",
            "yet",
            "here",
            "in this repo",
            "in this pr",
            "for this pr",
        ],
        Reads::Qualified,
        &[],
    )
    .saying("a qualified none"),
    LandFamily::new(
        "a party's",
        &[
            "owner",
            "the owner",
            "owner's",
            "whoever",
            "you",
            "yours",
            "maintainer",
            "maintainers",
            "the maintainer",
            "reviewer",
            "the reviewer",
            "reviewers",
            "human",
            "a human",
        ],
        Reads::Party,
        &[],
    )
    .saying("someone else"),
    LandFamily::new(
        "only that",
        &[
            "nothing more",
            "nothing else",
            "that's all",
            "that is all",
            "that's it",
            "that is it",
        ],
        Reads::OnlyTail,
        &[],
    )
    .saying("only"),
    LandFamily::new(
        "pending",
        &[
            "pending",
            "awaiting",
            "awaits",
            "on hold",
            "up for review",
            "is for review",
            "are for review",
            "sent for review",
            "submitted for review",
            "posted for review",
            "opened for review",
            "pushed for review",
            "under review",
            "in review",
            "out for review",
            "in the merge queue",
            "queued for",
            "unreviewed",
            "unapproved",
            "ready for review",
            "ready to review",
            "ready to merge",
            "ready for merge",
            "ready to land",
            "ready to be merged",
            "ready to be landed",
            "auto-merge",
            "automerge",
            "auto merge",
            "merge train",
            "merge queue",
            "is queued",
            "are queued",
            "on approval",
            "upon approval",
            "merges on",
            "merges once",
            "merges when",
            "merges after",
            "lands once",
            "lands when",
            "lands after",
            "lands on",
            "merging after",
            "merging once",
            "merging when",
            "landing after",
            "landing once",
            "ready to tag",
            "ready to publish",
            "ready to release",
            "ready to ship",
            "ready to push",
            "ready to deploy",
            "for review",
            "for a review",
            "is ready",
            "are ready",
        ],
        Reads::Anywhere,
        LANDS,
    ),
    LandFamily::new(
        "waits on",
        &[
            "needs", "need", "requires", "require", "waiting", "waits", "wait",
        ],
        Reads::Beside(WAIT_NOUNS),
        LANDS,
    ),
    LandFamily::new(
        "blocked",
        &[
            "blocked on",
            "blocked by",
            "is blocked",
            "are blocked",
            "still blocked",
            "remains blocked",
            "stays blocked",
        ],
        Reads::Anywhere,
        SHIPS,
    ),
    LandFamily::new(
        "open change",
        &["open", "opened", "reopened", "filed"],
        Reads::Beside(OPEN_NOUNS_LAND),
        MENDS,
    ),
    LandFamily::new(
        "check",
        &[
            "running",
            "red",
            "failing",
            "queued",
            "cut short",
            "aborted",
            "interrupted",
            "stopped early",
            "killed early",
            "ended early",
            "in flight",
            "flaky",
            "flakes",
            "flaking",
            "disabled",
            "ignored",
            "quarantined",
            "muted",
            "skipped",
            "broken",
            "timing out",
            "timed out",
            "hanging",
            "unstable",
            "yellow",
            "suppressed",
            "silenced",
            "commented out",
            "xfail",
            "fails",
            "fail",
            "intermittently",
            "intermittent",
            "sporadically",
            "amber",
            "turned off",
            "switched off",
            "allow",
            "allowed",
            "relaxed",
            "loosened",
            "weakened",
            "not passing",
            "not green",
            "not succeeding",
            "not succeeded",
            "not stable",
            "not clean",
            "error out",
            "errors out",
            "erroring out",
            "errored out",
            "complains",
            "complain",
            "complaining",
            "regressed",
            "silence",
            "xfailed",
            "deselected",
        ],
        Reads::Beside(CHECK_NOUNS),
        CHECKS,
    ),
    // A CHECK RUN AGAIN UNTIL IT PASSED (the integration review of
    // 12449ff581): AGENTS.md's checks never retry and a flaky test is
    // deleted, so a pass on a rerun is a flake still to fix, however the
    // report words it.
    LandFamily::new(
        "rerun",
        &[
            "on rerun",
            "on re-run",
            "on a rerun",
            "on a re-run",
            "on the rerun",
            "on retry",
            "on a retry",
            "on the retry",
            "after a rerun",
            "after one rerun",
            "after a retry",
            "after one retry",
            "after two retries",
            "after retrying",
            "with a retry",
            "with one retry",
            "one retry",
            "two retries",
            "retried",
            "second run",
            "2nd run",
            "third run",
            "3rd run",
            "second attempt",
            "2nd attempt",
            "third attempt",
            "3rd attempt",
            "second try",
            "2nd try",
            "third try",
            "3rd try",
            "ran it again",
            "ran them again",
            "run it again",
            "running it again",
            "flaked",
            "rerun clean",
            "rerun green",
            "rerun passed",
            "re-run clean",
            "re-run green",
            "reran clean",
            "reran green",
            "re-ran clean",
            "re-ran green",
            "passed this time",
            "green this time",
            "passes this time",
        ],
        Reads::Rerun,
        &[],
    )
    .saying("a rerun"),
    // A STATUS SAID AS A LABEL'S ITEM (the integration review of 12449ff581):
    // a check's skip, a later day, a part, set as a list's or a table's item
    // with no sentence around it.
    LandFamily::new(
        "status item",
        &[
            "skip",
            "skips",
            "named skip",
            "named skips",
            "none run",
            "not read",
            "unread",
            "next session",
            "to do",
            "n/a",
        ],
        Reads::Status,
        &[],
    ),
    LandFamily::new("later", LAND_LATER, Reads::Later, LANDS),
    LandFamily::new("until", &["until", "till"], Reads::Until, &[]),
    LandFamily::new(
        "intent",
        &[
            "i will",
            "i'll",
            "we will",
            "we'll",
            "i am going to",
            "i'm going to",
            "plan to",
            "planning to",
            "intend to",
            "will land",
            "will be landed",
            "will merge",
            "will be merged",
            "will push",
            "will be pushed",
            "will be done",
            "will be fixed",
            "will do",
            "will add",
            "will write",
            "will follow",
            "should still",
            "still should",
            "we should",
            "i should",
            "ought to",
            "would be good to",
            "would be nice to",
            "would be nice",
            "would be good",
            "would help",
            "would be better",
            "nice to have",
            "nice-to-have",
            "worth adding",
            "can be done",
            "could be done",
            "can be added",
            "could be added",
            "we can",
            "we could",
            "i'd like",
            "i would like",
            "would like",
            "second pair of eyes",
            "a second look",
            "another look",
            "a closer look",
            "a final look",
            "whether to",
            "whether we should",
            "if we should",
            "asked the owner",
            "asked the user",
            "i'm leaving",
            "i am leaving",
            "i'm keeping",
            "i am keeping",
            "will be",
            "will happen",
            "will update",
            "will rebuild",
            "will refresh",
            "will regenerate",
            "will redeploy",
            "will deploy",
            "will publish",
            "will roll",
            "will pick",
            "will go",
            "will get",
            "will run",
            "will need",
            "will come",
            "will ship",
            "should i",
            "should we",
            "shall i",
            "shall we",
            "want me to",
            "do you want",
            "let me know",
            "happy to",
            "i can also",
            "if you want",
            "if you'd like",
            "if you like",
            "if needed",
            "if desired",
            "when you are",
            "when you're",
            "let's",
            "let us",
            "worth considering",
            "consider",
        ],
        Reads::Intent,
        &[],
    ),
    LandFamily::new(
        "todo",
        &["todo", "todos", "fixme", "fixmes"],
        Reads::Anywhere,
        MENDS,
    ),
    LandFamily::new(
        "check off in code",
        &["check ignored", "lint allowed"],
        Reads::Anywhere,
        MENDS,
    ),
    LandFamily::new(
        "could not",
        &[
            "could not",
            "couldn't",
            "not able to",
            "unable to",
            "cannot run",
            "cannot test",
            "cannot verify",
            "cannot check",
            "cannot measure",
            "cannot confirm",
            "can't run",
            "can't test",
            "can't verify",
            "can't check",
            "can't measure",
            "can't confirm",
            "did not get to",
            "ran out of",
            "ran out",
            "out of time",
            "ended before",
            "stopped before",
            "unverified",
            "only type-checked",
        ],
        Reads::Anywhere,
        CHECKS,
    ),
    LandFamily::new(
        "not done",
        &[
            "not written",
            "not reviewed",
            "not approved",
            "not released",
            "not deployed",
            "not published",
            "not tagged",
            "not documented",
            "not implemented",
            "not started",
            "not finished",
            "not complete",
            "not completed",
            "not resolved",
            "not addressed",
            "not handled",
            "not updated",
            "not corrected",
            "not driven end to end",
            "not wired",
            "not hooked up",
            "not hooked in",
            "not plugged in",
            "not integrated",
            "not ported",
            "not migrated",
            "not rebased",
            "not signed",
            "not land",
            "not merge",
            "not push",
            "not commit",
            "not all",
            "not removed",
            "not deleted",
            "not cleaned up",
            "not reverted",
            "not dropped",
            "not investigated",
            "not looked at",
            "not looked into",
            "not profiled",
            "not diagnosed",
            "not understood",
            "not root-caused",
            "uninvestigated",
            "did not update",
            "did not add",
            "did not write",
            "did not push",
            "did not merge",
            "did not land",
            "did not finish",
            "did not do",
            "did not address",
            "did not handle",
            "did not document",
            "did not commit",
            "did not remove",
            "did not delete",
            "did not rebase",
            "not covered",
            "uncovered",
            "not tried",
            "untried",
            "not attempted",
            "did not apply",
            "stopped there",
            "gave up",
            "did not have time",
            "had no time",
            "no time for",
            "forgot to",
            "overlooked",
            "did not get into",
            "did not dig into",
            "not get around",
            "unfinished",
            "unaddressed",
            "unresolved",
            "unhandled",
            "unimplemented",
            "unwritten",
            "undocumented",
            "unreleased",
            "unpublished",
            "untagged",
            "unanswered",
            "unsolved",
            "unstarted",
            "never written",
            "never tested",
            "never run",
            "never ran",
            "never reviewed",
            "never documented",
            "never finished",
            "never implemented",
            "never released",
            "never published",
            "never tagged",
            "never verified",
            "never checked",
            "never measured",
            "not so much",
            "left out",
            "missed the merge",
            "missed the release",
            "missed the cut",
            "missed the train",
            "missed the window",
            "missed the deadline",
            "to be written",
            "to be added",
            "to be updated",
            "to be fixed",
            "to be ported",
            "still to write",
        ],
        Reads::Anywhere,
        MENDS,
    ),
    LandFamily::new(
        "partly",
        &[
            "only partly",
            "partly",
            "partially",
            "partial",
            "half done",
            "half-done",
            "half written",
            "half-written",
            "half finished",
            "half-finished",
            "half implemented",
            "half-implemented",
            "half merged",
            "halfway",
            "half-way",
            "half way",
            "mostly done",
            "mostly merged",
            "mostly landed",
            "almost done",
            "nearly done",
            "incomplete",
            "incompletely",
            "not fully",
            "not completely",
            "not entirely",
            "spot-checked",
            "spot-check",
            "spot checked",
            "only checked",
            "only tested",
            "only ran",
            "only verified",
            "only tried",
            "smoke-tested",
            "eyeballed",
            "some of the",
            "a few of the",
            "several of the",
            "half of",
            "half fixed",
            "mostly fixed",
            "fixed only for",
            "a quarter of",
            "a third of",
            "two thirds of",
            "three quarters of",
            "a fraction of",
            "not the real",
            "against a mock",
            "staging only",
            "on staging only",
            "only on staging",
            "only in staging",
            "not zero",
            "only some",
            "only part",
            "done-ish",
            "merged-ish",
            "finished-ish",
            "landed-ish",
            "complete-ish",
            "mostly-done",
        ],
        Reads::Anywhere,
        RETIRED,
    ),
    LandFamily::new(
        "stopgap",
        &[
            "workaround",
            "workarounds",
            "temporary",
            "temporarily",
            "stopgap",
            "stop-gap",
            "hack",
            "hacks",
            "hacky",
            "interim",
            "band-aid",
            "quick fix",
            "old code path",
            "old path",
            "behind a flag",
            "behind a cfg",
            "behind cfg",
            "behind a feature gate",
            "behind a gate",
            "left in place",
            "in place behind",
            "behind a feature flag",
            "behind the flag",
            "off by default",
            "works around",
            "work around",
            "worked around",
            "working around",
            "rather than fixing",
            "instead of fixing",
            "rather than solving",
            "instead of solving",
            "as a fallback",
            "as a backup",
            "rather than deleted",
            "rather than deleting",
            "rather than removed",
            "rather than removing",
            "instead of deleting",
            "instead of removing",
            "placeholder",
            "placeholders",
            "stubbed out",
            "is a stub",
            "just a stub",
            "only a stub",
            "kept around",
            "keep around",
            "keeping around",
        ],
        Reads::Anywhere,
        RETIRED,
    ),
    LandFamily::new(
        "needs doing",
        &[
            "needs",
            "need",
            "requires",
            "require",
            "still needs",
            "wants",
        ],
        Reads::Gerund,
        &[],
    ),
    LandFamily::new("still", &["still"], Reads::Still, &[]),
    LandFamily::new(
        "remote branch",
        &["origin", "upstream", "--no-merged"],
        Reads::Ref,
        LANDS,
    ),
    LandFamily::new("part of a count", &["of", "only"], Reads::Fraction, &[]),
    LandFamily::new(
        "hedge",
        &[
            "not sure",
            "unsure",
            "uncertain",
            "unclear whether",
            "unclear if",
            "not certain",
            "not convinced",
            "not confident",
            "i think",
            "i believe",
            "we think",
            "we believe",
            "i doubt",
            "doubt that",
            "doubtful",
            "i suspect",
            "i guess",
            "i hope",
            "i imagine",
            "probably",
            "should pass",
            "should work",
            "should be fine",
            "likely",
            "hopefully",
            "a guess",
            "guessed",
            "guesswork",
            "i assume",
            "presumably",
            "seems to",
            "appears to",
            "from my laptop",
            "from my machine",
            "tested on my",
            "measured on my",
            "benchmarked on my",
            "only locally",
            "locally only",
            "just locally",
            "only on my",
            "assuming",
            "fairly sure",
            "pretty sure",
            "reasonably sure",
            "quite sure",
            "fairly confident",
            "pretty confident",
            "if i recall",
            "if i remember",
            "iirc",
            "afaict",
            "afaik",
            "as far as i can tell",
            "as far as i know",
            "to my knowledge",
            "looks merged",
            "looks landed",
            "looks done",
            "looks good",
            "looks fine",
            "looks like it",
            "looks like everything",
            "looks like all",
            "it looks like",
            "provided",
            "providing",
            "fingers crossed",
            "should go",
            "should land",
            "should merge",
            "should be merged",
            "should be green",
            "unknown",
            "no idea",
            "no data",
            "not known",
        ],
        Reads::Anywhere,
        &[],
    ),
    LandFamily::new(
        "failed step",
        &[
            "failed",
            "errored",
            "rejected",
            "refused",
            "denied",
            "bounced",
            "conflict",
            "conflicts",
            "conflicted",
            "conflicting",
        ],
        Reads::Beside(STEP_NOUNS),
        STEPS,
    ),
    LandFamily::new(
        "elided",
        &[
            "is not", "are not", "has not", "have not", "was not", "were not", "did not",
            "does not", "do not",
        ],
        Reads::Elided,
        &[],
    ),
    LandFamily::new(
        "did not",
        &["did not", "have not", "has not", "had not", "never got to"],
        Reads::NotDone,
        &[],
    ),
    LandFamily::new(
        "no longer works",
        &[
            "doesn't",
            "does not",
            "don't",
            "do not",
            "no longer",
            "won't",
            "will not",
            "cannot",
            "can't",
        ],
        Reads::Lost,
        &[],
    ),
    LandFamily::new(
        "could be better",
        &["could be", "might be"],
        Reads::Better,
        &[],
    ),
    LandFamily::new(
        "concession",
        &["though", "although", "however", "but", "even so"],
        Reads::Concession,
        &[],
    ),
    LandFamily::new(
        "wrong",
        &[
            "is wrong",
            "are wrong",
            "still wrong",
            "is incorrect",
            "are incorrect",
            "is inaccurate",
            "is misleading",
            "is buggy",
            "is broken",
            "are broken",
            "will break",
            "will fail",
            "would break",
            "only handles",
            "only covers",
            "only supports",
            "only works",
            "is missing",
            "are missing",
            "still missing",
            "lacks",
            "still lacks",
            "lacking",
            "same bug",
            "same problem",
            "same issue",
            "nothing covers",
            "no test covers",
            "no tests cover",
            "says nothing about",
            "does not mention",
            "doesn't mention",
            "no mention of",
            "got worse",
            "gets worse",
            "is worse",
            "are worse",
            "got slower",
            "is slower",
            "are slower",
            "is dirty",
            "are dirty",
            "still dirty",
            "regression in",
            "regressions in",
            "is a regression",
            "there is a regression",
            "introduces a regression",
            "introduced a regression",
            "regressed",
            "main is red",
            "main is broken",
            "main is failing",
            "red on main",
        ],
        Reads::Anywhere,
        MENDS,
    ),
    LandFamily::new(
        "back again",
        &[
            "is back",
            "are back",
            "came back",
            "comes back",
            "is back again",
            "resurfaced",
            "reappeared",
            "recurs",
            "recurred",
            "returned",
            "still reproduces",
        ],
        Reads::Beside(FAULT_NOUNS),
        MENDS,
    ),
    LandFamily::new(
        "stale",
        &[
            "is stale",
            "are stale",
            "still stale",
            "now stale",
            "outdated",
            "out of date",
            "out-of-date",
            "still describes",
            "still mentions",
            "still refers to",
            "still points to",
            "still documents",
        ],
        Reads::Anywhere,
        REFRESHED,
    ),
    LandFamily::new(
        "known issue",
        &[
            "known issue",
            "known issues",
            "known bug",
            "known bugs",
            "known limitation",
            "known limitations",
            "known gap",
            "known gaps",
            "known problem",
            "known problems",
            "known regression",
            "known regressions",
            "known crash",
            "known crashes",
            "known hang",
            "known flake",
            "known failure",
            "known failures",
            "nit",
            "nits",
        ],
        Reads::Anywhere,
        MENDS,
    ),
    LandFamily::new(
        "someone else",
        &[
            "someone else",
            "somebody else",
            "someone will",
            "somebody will",
            "someone should",
            "somebody should",
            "someone can",
            "someone could",
            "someone must",
            "someone needs to",
            "until someone",
            "until somebody",
            "up to the owner",
            "left to the owner",
            "for the owner to",
            "the owner will",
            "the owner must",
            "the owner has to",
            "the owner needs to",
            "the owner's call",
            "up to you",
            "for you to",
            "your call",
            "another agent",
            "another worker",
            "another person",
            "the next agent",
            "the next worker",
            "a later worker",
            "owner's to do",
            "yours to do",
            "the owner's job",
            "the owner's step",
            "the owner's turn",
            "in the owner's hands",
            "in your hands",
            "on my side",
            "on my end",
            "leave to",
            "leave it to",
            "i leave",
            "leaving it to",
            "hand off",
            "handed off",
            "handoff",
            "hand-off",
            "hand-offs",
            "handing off",
            "handing it off",
            "handing over",
            "handed over",
            "you can",
            "you may",
            "you could",
            "you might",
            "feel free",
            "over to you",
            "your court",
            "your move",
            "your turn",
            "on you to",
            "is yours",
            "are yours",
            "yours to",
            "with the owner",
            "with you",
            "sits with",
            "rests with",
            "lies with",
            "has to sign",
            "must sign",
            "needs to sign",
            "to sign off",
            "for sign-off",
            "for signoff",
            "for your review",
            "for your approval",
            "on me",
        ],
        Reads::Anywhere,
        LANDS,
    ),
    // A NEED WRITTEN WITHOUT ITS HEAD (`I need you to publish the crate`,
    // `Owner: please approve the release`, `You'll have to sign the tag`):
    // what only a person can give, asked of them in the defense's own
    // words. No NEEDS YOU line names it ([`needs::needs_you`]), so no need
    // reaches the person's screen, and a defense beside it lands with the
    // ask unanswered. Nothing settles an ask, so it is open until the
    // worker puts it on a NEEDS YOU line, where the nudge sends it.
    LandFamily::new(
        "asks you",
        &[
            "please",
            "kindly",
            "i need you",
            "we need you",
            "need you to",
            "needs you to",
            "need from you",
            "needed from you",
            "need your",
            "needs your",
            "requires you",
            "require you",
            "requires your",
            "require your",
            "you need to",
            "you'll need to",
            "you will need to",
            "you'd need to",
            "you would need to",
            "you'll have to",
            "you will have to",
            "you have to",
            "you must",
            "you should",
            "could you",
            "can you",
            "would you",
            "will you",
            "ask you",
            "asking you",
            "action for you",
            "actions for you",
            "thing for you",
            "things for you",
            "item for you",
            "items for you",
            "task for you",
            "step for you",
            "steps for you",
            "left for you",
            "is for you",
            "are for you",
            "waiting on you",
            "waiting for you",
            "waits on you",
            "waits for you",
            "depends on you",
            "on you",
            "owner action",
            "action needed",
            "action required",
            "needs the owner",
            "requires the owner",
            "require the owner",
            "only a person can",
        ],
        Reads::Anywhere,
        &[],
    ),
    LandFamily::new(
        "deferred",
        &[
            "deferred",
            "postponed",
            "put off",
            "punted",
            "shelved",
            "backlogged",
            "in the backlog",
            "tracked in issue",
            "tracked in an issue",
            "tracked in a ticket",
            "tracked as issue",
            "tracked as an issue",
            "separately",
            "separate pr",
            "separate prs",
            "separate change",
            "separate job",
            "separate task",
            "separate step",
            "separate issue",
            "separate effort",
            "its own pr",
            "another pr",
            "a second pr",
            "second pr",
            "a later pr",
            "subsequent pr",
            "subsequent prs",
            "subsequent change",
            "subsequent changes",
            "subsequent commit",
            "subsequent session",
            "subsequent release",
            "subsequent version",
            "is coming",
            "are coming",
            "coming in",
            "coming soon",
            "in v2",
            "for v2",
            "next version",
            "a future version",
            "a later version",
            "out of scope",
            "outside my scope",
            "beyond scope",
            "beyond the scope",
            "not in scope",
            "another ticket",
            "another issue",
            "a separate ticket",
            "a follow-up ticket",
            "its own ticket",
            "another task",
            "tracked in",
            "tracked by",
            "tracked as",
            "tracked under",
            "tracked separately",
            "tracking issue",
            "on the roadmap",
            "on our roadmap",
            "in the roadmap",
            "can follow",
            "could follow",
            "may follow",
            "might follow",
            "after this lands",
            "once this lands",
            "after this merges",
            "once this merges",
            "after this is merged",
            "once this is merged",
            "second step",
        ],
        Reads::Anywhere,
        LANDS,
    ),
    LandFamily::new(
        "in progress",
        &[
            "in progress",
            "underway",
            "under way",
            "paused",
            "on pause",
            "is in flight",
            "are in flight",
            "still in flight",
            "abandoned",
            "omitted",
            "skipping",
            "ongoing",
            "stalled",
            "stuck",
            "suspended",
            "on the back burner",
            "on ice",
            "about to",
            "wrapping up",
            "almost through",
            "nearly through",
            "going through",
            "going in",
            "merging now",
            "pushing now",
            "landing now",
            "rolling out",
            "being rolled out",
            "being merged",
            "being pushed",
            "in the works",
            "slipped",
            "descoped",
            "de-scoped",
            "fell through",
            "fell behind",
            "fell off",
            "lag behind",
            "lags behind",
            "lagging behind",
            "behind the code",
        ],
        Reads::State,
        MENDS,
    ),
    LandFamily::new(
        "end of a clause",
        &[
            "soon",
            "next",
            "aside",
            "one thing",
            "one more thing",
            "to follow",
            "one item",
            "one small thing",
            "one small item",
            "one last item",
            "a single item",
            "coming",
            "forthcoming",
            "upcoming",
            "nearly",
            "almost",
            "absent",
            "to write",
            "to add",
            "to update",
            "to fix",
            "to port",
            "to land",
            "to merge",
            "to push",
            "to tag",
            "to review",
            "to publish",
            "to release",
            "last bit",
            "the last bit",
            "local",
            "behind",
            "on origin",
            "on the remote",
        ],
        Reads::End,
        &[],
    ),
    LandFamily::new("counted left", &["left"], Reads::Count, &[]),
    LandFamily::new("one more", &["one more"], Reads::OneMore, &[]),
    LandFamily::new("only", &["only", "just", "merely"], Reads::Only, &[]),
    LandFamily::new("after an event", &["after", "once"], Reads::Elliptic, &[]).saying("later"),
    LandFamily::new(
        "absent",
        &[
            "no test",
            "no tests",
            "no unit test",
            "no unit tests",
            "no new test",
            "no new tests",
            "no regression test",
            "no docs",
            "no doc",
            "no documentation",
            "no coverage",
            "no test coverage",
            "no benchmark",
            "no benchmarks",
            "no changelog",
            "no changelog entry",
            "no example",
            "no examples",
            "no readme",
            "no release notes",
            "no release note",
            "no migration",
            "no migrations",
            "zero tests",
            "zero test",
            "0 tests",
            "no test case",
            "no test cases",
            "no integration test",
            "no integration tests",
            "no run",
            "no runs",
            "no result",
            "no results",
            "no numbers",
            "no measurement",
            "no measurements",
        ],
        Reads::Absent,
        &[],
    ),
    LandFamily::new(
        "nobody",
        &["no one", "no-one", "noone", "nobody", "nothing"],
        Reads::Nobody,
        &[],
    ),
    LandFamily::new(
        "landing denied",
        &[
            "merged",
            "landed",
            "pushed",
            "reached",
            "committed",
            "on origin/main",
            "on main",
            "on remote main",
            "on the remote main",
            "in origin/main",
            "on upstream/main",
            "made it",
            "push",
            "merge",
        ],
        Reads::Unlanded,
        &[],
    )
    .saying("not landed"),
    LandFamily::new(
        "work denied",
        &[
            "written",
            "run",
            "tested",
            "done",
            "finished",
            "implemented",
            "documented",
            "reviewed",
        ],
        Reads::Undone,
        &[],
    )
    .saying("not done"),
    LandFamily::new(
        "onto a branch",
        &[
            "pushed",
            "merged",
            "landed",
            "committed",
            "cherry-picked",
            "cherry picked",
            "backported",
            "picked",
        ],
        Reads::Onto,
        &[],
    )
    .saying("to a branch"),
    LandFamily::new(
        "most",
        &[
            "most",
            "most of",
            "half the",
            "the bulk of",
            "the majority of",
            "almost all",
            "almost everything",
            "nearly all",
            "nearly everything",
            "virtually all",
            "practically all",
            "some of it",
            "some of them",
            "part of it",
            "parts of it",
            "half of it",
            "half of them",
            "much of",
            "the majority",
            "largely",
            "virtually everything",
            "practically everything",
            "the better part of",
            "a good part of",
            "pretty much",
            "more or less",
            "essentially",
            "basically",
            "practically",
            "lion's share",
            "the lion's share",
            "tenths",
            "nine tenths",
            "the lion's share of",
            "most commits",
        ],
        Reads::Most,
        &[],
    ),
    LandFamily::new(
        "dropped",
        &[
            "was dropped",
            "were dropped",
            "got dropped",
            "been dropped",
            "is dropped",
            "are dropped",
            "reverted",
            "rolled back",
            "backed out",
            "backed off",
            "scrapped",
            "was cut",
            "were cut",
            "got cut",
            "been cut",
            "cut from",
        ],
        Reads::Dropped,
        &[],
    ),
    LandFamily::new(
        "on a named branch",
        &[
            "on",
            "in",
            "has everything",
            "has it all",
            "has all of it",
            "holds everything",
            "carries everything",
        ],
        Reads::OnBranch,
        &[],
    )
    .saying("on a branch"),
    LandFamily::new(
        "change",
        &[
            "pr",
            "prs",
            "pull request",
            "pull requests",
            "mr",
            "mrs",
            "merge request",
            "merge requests",
            "the second one",
            "the other one",
        ],
        Reads::Change,
        SHIPS,
    )
    .saying("an unmerged change"),
    LandFamily::new(
        "percent",
        &["percent", "per cent", "pct"],
        Reads::Percent,
        &[],
    )
    .saying("a part"),
    LandFamily::new(
        "counted failures",
        &[
            "errors", "error", "failures", "failure", "panicked", "panics", "panic", "crashes",
            "crash", "crashed", "timeouts", "timeout", "warnings", "warning", "xfailed", "xfail",
            "flakes", "flake",
        ],
        Reads::Tally,
        CHECKS,
    ),
];

/// The verbs whose `n't` a report's words spell out ([`ReportLine::of`]):
/// `didn't fix` reads as `did not fix`.
const CONTRACTED: &[&str] = &[
    "did", "was", "were", "has", "have", "had", "is", "are", "does", "do",
];

/// What negates an open item, as its first word, or a status word, before it
/// in its clause ([`open_work`]): `Left: none`, `no check was skipped`.
const OPEN_NEGATIONS: &[&str] = &[
    "none", "nothing", "no", "n/a", "nil", "never", "zero", "0", "without", "neither", "nor",
];

/// The words before `not` that make a status word a description
/// ([`OPEN_STATUS`]). `could` is none of them: `could not run` reports a run
/// that did not happen.
const OPEN_MODALS: &[&str] = &[
    "do", "does", "don't", "doesn't", "must", "should", "shall", "will", "would", "can", "may",
    "might", "need", "needs",
];

/// What a report's own words leave open, in [`OPEN_HEADS`]' and
/// [`OPEN_STATUS`]' spelling, each once, in the order found: a head with an
/// item that is not negated ([`OPEN_NEGATIONS`]), and a status word outside
/// any head's items that is neither negated, nor a description, nor before a
/// reason that leaves nothing to do ([`open_status`]). WHATEVER BLOCKS IT: an
/// item that names its blocker or a decision is open work all the same (the
/// owner's rule of 2026-10-19: `ATERM-TASK-DONE` never ends a job while any
/// follow-up is left), so `Still open (needs the owner's credential): the
/// release` reads `still open`. Empty where the report names no open work:
/// `Nothing left.`, `No open items.`, `Left: none`, `no check was skipped`,
/// `not run: no contract was added`.
///
/// The report is the worker's last words (its last agent message), one row
/// a line: a row that is no list item and does not begin with a capital
/// continues the row above (its wrap), and the retired token line
/// ([`crate::words::FINISHED`]) is no part of it. A head's inline item ends
/// at its sentence; later sentences are read independently, including after
/// a negated item. Where nothing but
/// a colon follows the head, its items are the lines under it: the first,
/// then each list item, up to the next head or a line that is no list item;
/// the head is open where any of them is not negated. A qualifier in
/// brackets after the head (`Left (none):`) that negates it clears it. Text
/// in backticks and fenced code is never a word of the report; a code block
/// also separates the prose on either side.
///
/// CRUISE'S READING: what the defend prompt ([`crate::words::DONE_CHECK`])
/// names after a done claim ([`open_work_suffix`]). Conservative: a miss
/// costs the suffix, a false find one sentence more in a prompt that is
/// typed anyway. Measured against the labelled reports of three rounds of
/// aterm's own work (`fixtures/open-work-corpus.txt` beside this file), which names each
/// excerpt this reading still gets wrong, and why. Land reads strictly
/// ([`open_work_strict`]).
#[must_use]
pub fn open_work(tail: Option<&str>) -> Vec<&'static str> {
    tail.map_or_else(Vec::new, |t| open_work_read(t, None))
}

/// LAND'S STRICT READING of open work: [`open_work`]'s heads, then each
/// sentence read by [`land_status`] — [`OPEN_STATUS`] and every family of
/// [`LAND_FAMILIES`], where no blocker or decision excuses an item (Land
/// lands everything, no exceptions) and a defense's every leftover counts: a
/// wait on a review, a person or a check, a later day, a check that could
/// not run, a stopgap, an exception to the claim. A defense that names any
/// of it is no landing ([`crate::words::clean_defense`]). Measured against
/// the labelled
/// defense table (`fixtures/land-defense-table.txt` beside this file), both
/// ways: a false landing is Land's worst failure, a false non-landing (an
/// honest clean defense nudged forever) its second.
#[must_use]
pub fn open_work_strict(tail: Option<&str>) -> Vec<&'static str> {
    tail.map_or_else(Vec::new, |t| open_work_read(t, Some(&LAND_FAMILIES)))
}

/// [`open_work`] (`land` none) or Land's strict reading over `tail` with
/// the families `land` ([`open_work_strict`]: all of [`LAND_FAMILIES`]), read
/// as a defense ([`open_work_read_as`]).
fn open_work_read(tail: &str, land: Option<&[LandFamily]>) -> Vec<&'static str> {
    open_work_read_as(tail, land, true)
}

/// [`open_work_read`], `defense` whether `tail` is a defense: there a push
/// with no place leaves the work off main unless the reply says main has it
/// ([`unplaced`]). A done claim with no defense never lands, and its words
/// only name the nudge's suffix, so it names no bare push.
fn open_work_read_as(tail: &str, land: Option<&[LandFamily]>, defense: bool) -> Vec<&'static str> {
    let lines = report_lines(tail, land.is_some());
    // Land's reading: whether the reply says anywhere that main has the work,
    // which leaves a bare push beside it nothing off main ([`unplaced`]).
    let said_main = land.is_some()
        && (!defense
            || lines
                .iter()
                .any(|l| main_named(&l.words) || main_claimed(&l.words)));
    let mut found: Vec<&'static str> = Vec::new();
    let mut i = 0;
    while let Some(line) = lines.get(i) {
        if line.unchecked {
            found.push(UNCHECKED);
        }
        let words = line.words.as_str();
        let starts = sentence_starts(words);
        let end = |k: usize| starts.get(k + 1).copied().unwrap_or(words.len());
        let mut next = i + 1;
        for k in 0..starts.len() {
            let sentence = &words[starts[k]..end(k)];
            let head = open_head(sentence)
                .or_else(|| land.and_then(|_| head_in(sentence, LAND_HEADS)))
                .or_else(|| {
                    (k == 0 && line.heading)
                        .then(|| heading_head(sentence))
                        .flatten()
                });
            let Some((head, rest)) = head else {
                let (line, len) = (&words[starts[k]..], end(k) - starts[k]);
                let word = match land {
                    None => open_status(line, len, &STATUS_PHRASES),
                    Some(families) => {
                        land_status(line, len, &words[..starts[k]], families, said_main)
                    }
                };
                if let Some(word) = word {
                    found.push(word);
                }
                continue;
            };
            // Land clears a head only by an item that is none and says no more
            // ([`nil_list`]): `Remaining: none for this PR` keeps work elsewhere.
            let nil = |item: &str| {
                if land.is_some() {
                    nil_list(item)
                } else {
                    negated(item)
                }
            };
            let (qualifier, inline) = qualified(rest);
            let cleared = nil(qualifier);
            let one_sentence = sentence_starts(inline).len() == 1;
            if inline.is_empty() || (one_sentence && inline.ends_with(':')) {
                // A bare head: the lines under it are its items.
                let mut j = i + 1;
                let mut items: Vec<&str> = Vec::new();
                while let Some(under) = lines.get(j) {
                    if under.block {
                        // Land's reading: a code block right under a bare
                        // head is its item (`Still to do:` over a command).
                        if items.is_empty() {
                            items.push(&under.words);
                        }
                        break;
                    }
                    if under.separated
                        || open_head(&under.words).is_some()
                        || !(under.item || items.is_empty())
                    {
                        break;
                    }
                    items.push(&under.words);
                    j += 1;
                }
                next = j;
                if !cleared && items.iter().any(|item| !nil(item)) {
                    found.push(head);
                }
                if let Some(families) = land {
                    found.extend(
                        items
                            .iter()
                            .filter_map(|item| aside_status(item, families, said_main)),
                    );
                }
                break;
            }
            if !cleared && !nil(inline) {
                found.push(head);
            }
            if let Some(families) = land {
                found.extend(aside_status(inline, families, said_main));
            }
        }
        i = next;
    }
    let mut once: Vec<&'static str> = Vec::with_capacity(found.len());
    for word in found {
        if !once.contains(&word) {
            once.push(word);
        }
    }
    once
}

/// The marks of a task-list item still open ([`ReportLine::of`]): Markdown's
/// `[ ]` and its near spellings, the empty boxes of plain text, and the marks
/// of an item waiting, in progress or failed (`⏳ release notes`). A longer
/// spelling comes before its prefix.
const UNCHECKED_BOXES: &[&str] = &[
    "[ ]", "[  ]", "[]", "[-]", "[~]", "☐", "⬜", "◻️", "◻", "□", "🔲", "⏳", "⌛", "🚧", "❌",
    "✗", "✘", "✕", "✖", "✖️", "⚠️", "⚠",
];

/// The marks of a task-list item done ([`ReportLine::of`]).
const CHECKED_BOXES: &[&str] = &[
    "[x]", "[X]", "[✓]", "[✔]", "[v]", "☑️", "☑", "✅", "✔️", "✔", "✓",
];

/// What Land names an unchecked task-list item by (`- [ ] tag the
/// release`): the Markdown mark of an item still open.
const UNCHECKED: &str = "unchecked box";

/// The word Land's reading puts where a report had code (a span in
/// backticks, or a fenced block): code is never a word of the report, but
/// it is something — a head whose only item is code has an item (`Remaining:
/// `docs/guide.md``). No table holds it.
const CODE: &str = "codespan";

/// Code that marks work left in the source, and the word Land reads it as
/// ([`ReportLine::of`]): `there's a `todo!()` in parser.rs`, ``#[ignore]` is
/// on the resize test`.
const CODE_MARKS: &[(&str, &str)] = &[
    ("todo!(", "todo"),
    ("unimplemented!(", "todo"),
    ("#[ignore", "check ignored"),
    ("continue-on-error", "check ignored"),
    ("allow(", "lint allowed"),
    ("stash@{", "stash"),
    ("--dry-run", "dry run"),
];

/// What Land reads a span of code as (`lower`, lowercased; [`ReportLine::of`]):
/// the word for work it marks ([`code_work`]: `todo!()`, `// FIXME`), a ref or a name it holds alone
/// as itself (a branch, a remote ref, a range, main, a dashed name:
/// `fix/resize-race`, `origin/main..HEAD`, `main`, `feature-x`,
/// [`code_ref`]), else [`CODE`] and the refs among its words (`git push origin
/// fix/x` reads `codespan fix/x`).
fn code_words(raw: &str) -> String {
    let lower = raw.to_lowercase();
    let lower = lower.as_str();
    if let Some(word) = code_work(lower).first() {
        return (*word).to_string();
    }
    // A check's state as the whole span (`skipped`, `SKIP`, `not run`), and
    // a check's line or a repository's state in it ([`output_status`]: `27
    // pass, 1 skip`, ` M Cargo.lock`).
    let whole = lower.trim().trim_matches(|c: char| !c.is_alphanumeric());
    if let Some(word) = SPAN_STATUS
        .iter()
        .find(|(said, _)| *said == whole)
        .map(|(_, word)| *word)
    {
        return word.to_string();
    }
    if let Some(word) = output_status(raw).first() {
        return (*word).to_string();
    }
    let mut tokens = lower.split_whitespace();
    if let (Some(one), None) = (tokens.next(), tokens.next()) {
        // A refspec names where it pushes (`HEAD:main` is main).
        if let Some((_, dst)) = one.split_once(':')
            && matches!(
                dst,
                "main" | "master" | "refs/heads/main" | "refs/heads/master"
            )
        {
            return "main".to_string();
        }
        let named = one.contains(['-', '_'])
            && one
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'));
        if code_ref(one)
            || matches!(one, "main" | "master")
            || BRANCHES.contains(&one)
            || (named && one.starts_with(|c: char| c.is_ascii_alphabetic()))
        {
            return one.to_string();
        }
    }
    let mut out = CODE.to_string();
    for t in lower.split_whitespace().filter(|t| code_ref(t)) {
        out.push(' ');
        out.push_str(t);
        // A range open at its end runs to HEAD (`git log origin/main..`).
        if t.ends_with("..") {
            out.push_str("head");
        }
    }
    out
}

/// The nouns after a span of code that name its words as a kind of thing,
/// not a state ([`ReportLine::of`]): `` its `SKIP` lines ``.
const SPAN_NOUNS: &[&str] = &[
    "line", "lines", "row", "rows", "label", "labels", "word", "words", "status", "marker",
    "markers", "entry", "entries", "prefix", "column", "columns", "case", "cases", "state",
    "states", "outcome", "outcomes", "result", "results", "output", "colour", "color", "text",
];

/// A check's state written as a whole span of code, and the word Land reads
/// it as ([`code_words`]): `` verify appkit: `skipped` ``, `` appkit `SKIP` ``.
const SPAN_STATUS: &[(&str, &str)] = &[
    ("skip", "skipped"),
    ("skips", "skipped"),
    ("skipped", "skipped"),
    ("fail", "check failed"),
    ("failed", "check failed"),
    ("failing", "check failed"),
    ("not run", "not run"),
    ("pending", "pending"),
];

/// The words for what a line of output says is left ([`report_lines`] reads
/// it in a fenced block, [`code_words`] in a span), from `raw` as printed: a
/// check's line that opens with its skip, failure or could-not-run (`SKIP
/// census`, `skip  appkit`, `FAIL tippy`, `NOT RUN ay`), a count of skips or
/// failures above zero (`27 pass, 1 skip`; never `0 failed`, nor an ignored
/// test, which the repository keeps), and `git status`'s own lines: a branch
/// ahead of its upstream (`## main...origin/main [ahead 1]`), an untracked
/// file (`?? docs/x.md`) and a changed one (` M Cargo.lock`).
fn output_status(raw: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    let t = raw.trim();
    let mut tokens = t.split_whitespace();
    let first = tokens
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| matches!(c, '[' | ']' | ':' | '-'));
    let second = tokens.next();
    let named = second.is_some_and(|w| w.starts_with(|c: char| c.is_alphanumeric()));
    if matches!(first, "SKIP" | "SKIPPED") || (first.eq_ignore_ascii_case("skip") && named) {
        out.push("skipped");
    }
    if matches!(first, "FAIL" | "FAILED") || t.contains("test result: FAILED") {
        out.push("check failed");
    }
    if first == "NOT" && second == Some("RUN") {
        out.push("not run");
    }
    let words: Vec<String> = t
        .split_whitespace()
        .map(|w| bare(w).to_lowercase())
        .collect();
    for pair in words.windows(2) {
        if count(&pair[0]).is_some_and(|n| n > 0) {
            match pair[1].as_str() {
                "skip" | "skips" | "skipped" => out.push("skipped"),
                "fail" | "fails" | "failed" | "failures" | "failing" => out.push("check failed"),
                _ => {}
            }
        }
    }
    if t.starts_with("## ") && t.contains("[ahead ") {
        out.push("commits ahead");
    }
    let bytes = raw.trim_end().as_bytes();
    let state = |b: u8| matches!(b, b' ' | b'M' | b'A' | b'D' | b'R' | b'C' | b'U');
    if raw.starts_with("?? ") && raw.len() > 3 {
        out.push("untracked");
    } else if bytes.len() > 3
        && state(bytes[0])
        && state(bytes[1])
        && bytes[..2] != *b"  "
        && bytes[2] == b' '
        && bytes[3] != b' '
        && raw[3..].contains(['.', '/'])
    {
        out.push("modified files");
    }
    out.dedup();
    out
}

/// Whether a word of code names a git ref ([`code_words`]): a range
/// (`main..HEAD`) or a name with a slash (`fix/resize-race`,
/// `origin/main`), never a path to a file (`src/lib.rs`), a rooted or
/// relative path (`/tmp/x`, `./x`) or a URL.
fn code_ref(t: &str) -> bool {
    let plain = t
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'));
    let file = t.rsplit('/').next().is_some_and(|last| {
        last.rsplit_once('.').is_some_and(|(stem, ext)| {
            !stem.is_empty() && ext.starts_with(|c: char| c.is_ascii_alphabetic())
        })
    });
    plain
        && t.starts_with(|c: char| c.is_ascii_alphanumeric())
        && !t.ends_with('/')
        && (t.contains("..") || (t.contains('/') && !file))
}

/// A Markdown table's row of two cells read as a label and its item
/// ([`ReportLine::of`], under Land): `| Open PRs | 0 |` reads `Open PRs: 0`.
fn table_row(t: &str) -> Option<String> {
    let inner = t.strip_prefix('|')?.trim_end().strip_suffix('|')?;
    let cells: Vec<&str> = inner.split('|').map(str::trim).collect();
    match cells.as_slice() {
        [label, item] if !label.is_empty() && !item.is_empty() => Some(format!("{label}: {item}")),
        _ => None,
    }
}

/// The words for work a line of a fenced block marks ([`report_lines`], under
/// Land): its code marks ([`CODE_MARKS`]), and a `TODO` or `FIXME` as a word.
fn code_work(raw: &str) -> Vec<&'static str> {
    let lower = raw.to_lowercase();
    let mut out: Vec<&'static str> = CODE_MARKS
        .iter()
        .filter(|(mark, _)| lower.contains(mark))
        .map(|(_, word)| *word)
        .collect();
    if lower
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| matches!(w, "todo" | "fixme"))
    {
        out.push("todo");
    }
    out
}

/// One logical line of a report ([`report_lines`]).
struct ReportLine {
    /// A code block separates this row from the preceding prose.
    separated: bool,
    /// Land's reading: a fenced code block, read as one [`CODE`] word.
    block: bool,
    /// Land's reading: a task-list item not checked (`- [ ] release tag`).
    unchecked: bool,
    /// It began with a list marker (`- `, `* `, `• `, `1. `, `1) `).
    item: bool,
    /// It is a Markdown heading (`## Not measured`).
    heading: bool,
    /// Its first letter, past the marks, is a capital.
    capital: bool,
    /// Its words, lowercased, with the marks, emphasis and code dropped, a
    /// [`CONTRACTED`] verb's `n't` spelled out, and `yet` or `been` after
    /// `not` dropped. Under Land (`land`), code stands as [`CODE`] or what it
    /// names ([`code_words`]), a task list's box is taken off, at the line's
    /// start or inside it ([`UNCHECKED_BOXES`], [`CHECKED_BOXES`]: an
    /// unchecked one marks the line [`Self::unchecked`]), and a count `N/M`
    /// reads `N of M` (`2/3 PRs merged`).
    words: String,
}

impl ReportLine {
    fn of(raw: &str, land: bool) -> Self {
        let lead = raw.trim_start();
        let marks = lead.trim_start_matches('#');
        // Under Land a heading's marks stand before a space: `#45 is open`
        // names an issue.
        let heading = lead.starts_with('#') && !(land && !marks.starts_with(char::is_whitespace));
        let t = raw.trim().trim_start_matches(|c: char| {
            c == '>' || (c == '#' && heading) || (c == '#' && !land) || c.is_whitespace()
        });
        let marked = ["- ", "* ", "+ ", "• ", "◦ ", "‣ "]
            .iter()
            .find_map(|m| t.strip_prefix(m))
            .or_else(|| {
                let digits = t.bytes().take_while(u8::is_ascii_digit).count();
                let rest = t.get(digits..).filter(|_| (1..=3).contains(&digits))?;
                rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") "))
            });
        let (item, t) = marked.map_or((false, t), |r| (true, r.trim_start()));
        // Under Land a box opens a line with or without a list marker.
        let boxed = UNCHECKED_BOXES.iter().chain(CHECKED_BOXES).find(|b| {
            land && t
                .strip_prefix(**b)
                .is_some_and(|r| r.is_empty() || r.starts_with(' '))
        });
        let mut unchecked = boxed.is_some_and(|b| UNCHECKED_BOXES.contains(b));
        let t = boxed.map_or(t, |b| t[b.len()..].trim_start());
        let cells = land.then(|| table_row(t)).flatten();
        let t = cells.as_deref().unwrap_or(t);
        let capital = t
            .chars()
            .find(|&c| c != '*' && c != '_')
            .is_some_and(char::is_uppercase);
        let mut flat = String::with_capacity(t.len());
        let mut code = false;
        let mut span = String::new();
        for (i, c) in t.char_indices() {
            match c {
                '`' => {
                    if code && land {
                        flat.push(' ');
                        // A state's word named as a kind of thing is no state
                        // (`its `SKIP` lines`, `the `FAIL` rows`).
                        let named = t[i + 1..]
                            .split_whitespace()
                            .next()
                            .map(bare)
                            .is_some_and(|w| SPAN_NOUNS.contains(&w.to_lowercase().as_str()));
                        let words = code_words(&span);
                        if named && SPAN_STATUS.iter().any(|(_, w)| *w == words) {
                            flat.push_str(CODE);
                        } else {
                            flat.push_str(&words);
                        }
                        span.clear();
                    }
                    code = !code;
                    flat.push(' ');
                }
                _ if code => span.push(c),
                '*' => {}
                '’' => flat.push('\''),
                c => flat.extend(c.to_lowercase()),
            }
        }
        if land {
            // A box inside the line (`Nothing left: [ ] docs`, `fix ✔, docs
            // ✘.`) marks it too: after a space, before a space or a mark that
            // ends a clause.
            let mut padded = format!(" {flat} ");
            for b in UNCHECKED_BOXES.iter().chain(CHECKED_BOXES) {
                let mark = format!(" {}", b.to_lowercase());
                let mut from = 0;
                while let Some(found) = padded[from..].find(&mark).map(|i| from + i) {
                    let past = found + mark.len();
                    if padded[past..].starts_with([' ', '.', ',', ';', ':', ')', '!']) {
                        unchecked |= UNCHECKED_BOXES.contains(b);
                        padded.replace_range(found..past, " ");
                        from = found;
                    } else {
                        from = past;
                    }
                }
            }
            // A test attribute written out of code (`marked #[ignore]`).
            flat = padded.replace("#[ignore]", " check ignored ");
        }
        let mut words = String::with_capacity(flat.len());
        let mut after_not = false;
        for token in without_underscore_emphasis(&flat).split_whitespace() {
            if after_not && matches!(token, "yet" | "been") {
                continue;
            }
            if !words.is_empty() {
                words.push(' ');
            }
            let spelled = token.split_once("n't").filter(|(verb, rest)| {
                CONTRACTED.contains(verb) && !rest.contains(char::is_alphanumeric)
            });
            if let Some((verb, rest)) = spelled {
                words.push_str(verb);
                words.push_str(" not");
                words.push_str(rest);
                after_not = rest.is_empty();
            } else if let Some((n, m)) = land.then(|| ratio(token)).flatten() {
                words.push_str(&format!("{n} of {m}"));
                words.push_str(
                    &token[token.trim_end_matches(|c: char| !c.is_ascii_digit()).len()..],
                );
                after_not = false;
            } else {
                words.push_str(token);
                after_not = token == "not";
            }
        }
        Self {
            separated: false,
            block: false,
            unchecked,
            item,
            heading,
            capital,
            words,
        }
    }
}

/// Drop paired underscore emphasis at word boundaries, preserving underscores
/// within identifiers and every unmatched mark.
fn without_underscore_emphasis(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut kept = 0;
    let mut cursor = 0;
    let mut open = None;
    while let Some(offset) = text[cursor..].find('_') {
        let at = cursor + offset;
        let width = text[at..].bytes().take_while(|&b| b == b'_').count();
        cursor = at + width;
        let before = text[..at].chars().next_back();
        let after = text[cursor..].chars().next();
        let closes =
            before.is_some_and(|c| !c.is_whitespace()) && after.is_none_or(|c| !word_char(c));
        if let Some((start, marks)) = open
            && marks == width
            && closes
        {
            out.push_str(&text[kept..start]);
            out.push_str(&text[start + marks..at]);
            kept = cursor;
            open = None;
        } else if open.is_none()
            && before.is_none_or(|c| !word_char(c))
            && after.is_some_and(|c| !c.is_whitespace())
        {
            open = Some((at, width));
        }
    }
    out.push_str(&text[kept..]);
    out
}

/// A count written `N/M` with `N` short of `M` (`2/3`, `3/4.`): its two
/// numbers. Never a date or a path: both sides are digits, and `M` is at most
/// a thousand.
fn ratio(token: &str) -> Option<(u32, u32)> {
    let t = token.trim_end_matches(['.', ',', ';', ':', ')']);
    let (n, m) = t.split_once('/')?;
    let digits = |x: &str| !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit());
    if !(digits(n) && digits(m)) {
        return None;
    }
    let (n, m): (u32, u32) = (n.parse().ok()?, m.parse().ok()?);
    (n < m && m <= 1000).then_some((n, m))
}

/// Whether `raw` is a table's header row, which names its columns (Land's
/// reading, [`report_lines`]): a row of cells (`| PR | State |`) with the
/// table's separator after it (`|---|---|`).
fn table_header(raw: &str, next: Option<&str>) -> bool {
    raw.trim_start().starts_with('|')
        && next.is_some_and(|n| {
            let t = n.trim();
            t.starts_with('|')
                && t.contains('-')
                && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
        })
}

/// A report's logical lines ([`open_work`]): its rows, the retired token line,
/// blank rows and fenced code left out. A noncapital row continues the row
/// above unless a code block separates them; a block also ends a head's items.
/// Under Land (`land`) a fenced block is a line of its own ([`ReportLine::block`]).
fn report_lines(tail: &str, land: bool) -> Vec<ReportLine> {
    let mut out: Vec<ReportLine> = Vec::new();
    let mut fence = None;
    let mut separated = false;
    let lines: Vec<&str> = tail.lines().collect();
    for (i, raw) in lines.iter().copied().enumerate() {
        if let Some((mark, width)) = fence {
            if needs::code_fence(raw)
                .is_some_and(|(m, n, rest)| m == mark && n >= width && rest.trim().is_empty())
            {
                fence = None;
            } else if let Some(block) = out.last_mut().filter(|b| land && b.block) {
                // Land's reading: work a block marks in its code (`TODO:`,
                // `todo!()`, `#[ignore]`) or its output (`SKIP census`, `??
                // docs/x.md`) is a word of its line.
                for word in code_work(raw).into_iter().chain(output_status(raw)) {
                    if !block.words.split(' ').any(|w| w == word) {
                        block.words.push(' ');
                        block.words.push_str(word);
                    }
                }
            }
            continue;
        }
        if let Some((mark, width, _)) = needs::code_fence(raw) {
            fence = Some((mark, width));
            separated = true;
            if land {
                let mut block = ReportLine::of(CODE, true);
                block.block = true;
                block.separated = true;
                out.push(block);
            }
            continue;
        }
        if is_finished_line(raw) || (land && table_header(raw, lines.get(i + 1).copied())) {
            continue;
        }
        let mut line = ReportLine::of(raw, land);
        if line.words.is_empty() && !line.unchecked {
            continue;
        }
        line.separated = std::mem::take(&mut separated);
        match out.last_mut() {
            Some(above) if !line.separated && !line.item && !line.capital => {
                above.unchecked |= line.unchecked;
                above.words.push(' ');
                above.words.push_str(&line.words);
            }
            _ => out.push(line),
        }
    }
    out
}

/// Where each sentence of `words` begins: `0`, then past each `.`, `;`, `!`
/// or `?` a space follows.
fn sentence_starts(words: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (at, c) in words.char_indices() {
        if matches!(c, '.' | ';' | '!' | '?') && words[at + 1..].starts_with(' ') {
            starts.push(at + 2);
        }
    }
    starts.retain(|&s| s < words.len() || s == 0);
    starts
}

/// The head `text` begins with ([`OPEN_HEADS`], the longest that fits) and
/// what follows it: from its colon where a tail stands before that.
fn open_head(text: &str) -> Option<(&'static str, &str)> {
    head_in(text, OPEN_HEADS)
}

/// The heads only Land reads ([`open_work_strict`]): what a report lists
/// under them is work still to land or a question still open, unless the
/// item says there is none (`Blockers: none.`).
const LAND_HEADS: &[&str] = &[
    "blockers",
    "blocker",
    "blocking",
    "known issues",
    "known issue",
    "open questions",
    "unresolved",
    "fixmes",
    "fixme",
    "gaps",
    "possible improvements",
    "potential improvements",
    "further improvements",
    "future improvements",
    "improvement ideas",
    "worth considering",
    "optional",
    "optional follow-up",
    "optional follow-ups",
    "ideas",
    "one idea",
    "an idea",
    "suggestions",
    "suggestion",
    "suggested follow-ups",
    "possible follow-ups",
    "nice to have",
    "nice to haves",
    "nice-to-haves",
    "for later",
    // An ask of the person under a head of its own, no NEEDS YOU line
    // naming it (`Owner: publish the crate`, `For you: approve #45`).
    "owner",
    "for the owner",
    "for you",
    "action for you",
    "actions for you",
    "action needed",
    "action required",
    "ask",
    "asks",
    "one ask",
    "an ask",
    "my ask",
    "one request",
    "a request",
    "your decision",
    "your input",
    "your action",
    "your actions",
    "decision needed",
    "decision required",
    "owner action",
    "owner actions",
    "owner step",
    "owner steps",
    "owner action needed",
    "owner action required",
    "blocking on you",
    // Work put off under a head of its own (`Next session: the perf pass`),
    // and a regression left (`Regression: one extra resize on attach`).
    "next session",
    "next time",
    "later",
    "regression",
    "regressions",
];

/// [`open_head`] over `heads`.
fn head_in<'t>(text: &'t str, heads: &[&'static str]) -> Option<(&'static str, &'t str)> {
    let ends = |rest: &str| {
        rest.is_empty()
            || rest.starts_with([':', '('])
            || [" -", " –", " —", " ("].iter().any(|p| rest.starts_with(p))
    };
    heads
        .iter()
        .filter_map(|&head| {
            let rest = text.strip_prefix(head)?;
            let rest = OPEN_NOUNS
                .iter()
                .find_map(|noun| {
                    rest.strip_prefix(' ')?
                        .strip_prefix(noun)
                        .filter(|r| ends(r))
                })
                .unwrap_or(rest);
            if ends(rest) {
                Some((head, rest))
            } else {
                head_tail(rest).map(|rest| (head, rest))
            }
        })
        .max_by_key(|(head, _)| head.len())
}

/// What follows a head's tail ([`HEAD_TAIL`]), from its colon: `rest` opens
/// with `, `, ` from ` or ` for `.
fn head_tail(rest: &str) -> Option<&str> {
    let tail = [", ", " from ", " for "]
        .iter()
        .find_map(|p| rest.strip_prefix(p))?;
    let colon = tail.find(':').filter(|&at| at <= HEAD_TAIL)?;
    Some(&tail[colon..])
}

/// The head a Markdown heading begins with before a comma, bare whatever
/// follows it (`## Not measured, and why`): the lines under it are its items.
fn heading_head(text: &str) -> Option<(&'static str, &str)> {
    OPEN_HEADS
        .iter()
        .filter(|&&head| text.strip_prefix(head).is_some_and(|r| r.starts_with(',')))
        .max_by_key(|head| head.len())
        .map(|&head| (head, ""))
}

/// A head's rest split into its bracketed qualifier, if any, and the item
/// after the colon or dash.
fn qualified(rest: &str) -> (&str, &str) {
    let rest = rest.trim_start();
    let (qualifier, after) = rest
        .strip_prefix('(')
        .and_then(|r| r.split_once(')'))
        .unwrap_or(("", rest));
    let item = after
        .trim_start_matches(|c: char| matches!(c, ':' | '-' | '–' | '—') || c.is_whitespace())
        .trim_end();
    (qualifier, item)
}

/// A word character for [`phrases_in`]: `skipped-test` and `--skipped` hold
/// no `skipped`.
fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_'
}

/// A table of phrases and the bytes they begin with ([`phrases_in`]).
#[derive(Clone, Copy)]
struct Phrases {
    table: &'static [&'static str],
    first: [bool; 256],
}

impl Phrases {
    const fn of(table: &'static [&'static str]) -> Self {
        let mut first = [false; 256];
        let mut i = 0;
        while i < table.len() {
            first[table[i].as_bytes()[0] as usize] = true;
            i += 1;
        }
        Self { table, first }
    }
}

/// [`OPEN_STATUS`] for [`phrases_in`].
const STATUS_PHRASES: Phrases = Phrases::of(OPEN_STATUS);

/// Each phrase of `phrases` (each begins with a [`word_char`]) that stands in
/// `text` as whole words, with where it begins: one pass over the words of
/// `text`, a word whose first byte begins no phrase passed at one look, and
/// never a substring search per phrase, which cost five times the whole
/// reading on a 15 KB report once the tables grew.
fn phrases_in<'t>(
    text: &'t str,
    phrases: &'t Phrases,
) -> impl Iterator<Item = (usize, &'static str)> + 't {
    let mut before = None;
    text.char_indices()
        .filter_map(move |(at, c)| {
            let starts = word_char(c)
                && !before.is_some_and(word_char)
                && phrases.first[usize::from(text.as_bytes()[at])];
            before = Some(c);
            starts.then_some(at)
        })
        .flat_map(move |at| {
            phrases
                .table
                .iter()
                .copied()
                .filter(move |phrase| {
                    text.as_bytes().get(at) == phrase.as_bytes().first()
                        && text[at..].starts_with(phrase)
                        && !text[at + phrase.len()..]
                            .chars()
                            .next()
                            .is_some_and(word_char)
                })
                .map(move |phrase| (at, phrase))
        })
}

/// Whether an item's first word negates it ([`OPEN_NEGATIONS`]): `none`,
/// `nothing left`, `no open items`.
fn negated(item: &str) -> bool {
    item.trim_start_matches(|c: char| !c.is_alphanumeric())
        .split(|c: char| c.is_whitespace() || matches!(c, '.' | ',' | ';' | ':' | '!' | ')'))
        .next()
        .is_some_and(|first| OPEN_NEGATIONS.contains(&first))
        || item.starts_with("n/a")
}

/// The status word the sentence of its first `len` bytes reports open work
/// with (`phrases`' table, [`OPEN_STATUS`]: Cruise's reading), if one stands
/// there undescribed, unnegated, unquoted and not before a reason that leaves
/// nothing to do; `line` runs on to the end of the sentence's line, where
/// that reason may stand. Land reads a sentence by [`land_status`].
fn open_status(line: &str, len: usize, phrases: &Phrases) -> Option<&'static str> {
    let sentence = &line[..len];
    let stands: Vec<(usize, &'static str)> = phrases_in(sentence, phrases).collect();
    phrases.table.iter().copied().find(|&word| {
        stands.iter().filter(|&&(_, w)| w == word).any(|&(at, _)| {
            let clause = sentence[..at]
                .rsplit([',', ';', ':', '(', ')', '—', '–'])
                .next()
                .unwrap_or("");
            let before: Vec<&str> = clause.split_whitespace().collect();
            let prev = before.last().copied();
            !before.iter().any(|w| OPEN_NEGATIONS.contains(w))
                && !(word.starts_with("not ") && prev.is_some_and(|p| OPEN_MODALS.contains(&p)))
                && !(word == "skipped" && prev.is_some_and(|p| ["is", "are", "be"].contains(&p)))
                && !quoted(sentence, at)
                && !nothing_to_do(&line[at + word.len()..])
        })
    })
}

/// One row of Land's tables standing in a sentence ([`land_status`]).
struct Stand<'f> {
    /// Where it begins in the sentence.
    at: usize,
    word: &'static str,
    /// Its family; `None` for an [`OPEN_STATUS`] row.
    family: Option<&'f LandFamily>,
    /// Its table's place, then its row's: the order a nudge names it in.
    rank: (usize, usize),
    /// Its length in the sentence: its row's, or the token a pattern row
    /// stands for ([`pattern_stands`]: `#45`, `90%`).
    len: usize,
}

impl Stand<'_> {
    const fn end(&self) -> usize {
        self.at + self.len
    }

    fn reads(&self) -> Reads {
        self.family.map_or(Reads::Anywhere, |f| f.reads)
    }

    fn settled_by(&self) -> &'static [&'static str] {
        self.family.map_or(STATUS_BY, |f| f.settled_by)
    }

    /// Whether a place on the remote main settles the row ([`settles_at`]):
    /// a landing leftover (`the follow-up is on remote main`), never a
    /// check, a fault or a marker (`the TODO is on origin/main` is the TODO
    /// landed).
    fn lands_by_place(&self) -> bool {
        match self.family {
            Some(f) => f.settled_by == LANDS || f.settled_by == SHIPS,
            None => matches!(
                self.word,
                "follow-up"
                    | "follow-ups"
                    | "still open"
                    | "remains open"
                    | "left open"
                    | "left for"
            ),
        }
    }
}

/// LAND'S STRICT READING OF ONE SENTENCE (its first `len` bytes of `line`,
/// which runs on to the end of the sentence's line; `prior` is the line
/// before it): the first row of [`OPEN_STATUS`] or of `families`, in table
/// order, that stands there open ([`land_open`]) — else [`QUESTION`] where
/// the sentence asks one: a decision still open, which no landing answers.
fn land_status(
    line: &str,
    len: usize,
    prior: &str,
    families: &[LandFamily],
    said_main: bool,
) -> Option<&'static str> {
    let s = &line[..len];
    let rank =
        |table: &[&str], word: &str| table.iter().position(|w| *w == word).unwrap_or(usize::MAX);
    let mut stands: Vec<Stand<'_>> = phrases_in(s, &STATUS_PHRASES)
        .map(|(at, word)| Stand {
            at,
            word,
            family: None,
            rank: (0, rank(OPEN_STATUS, word)),
            len: word.len(),
        })
        .collect();
    for (k, family) in families.iter().enumerate() {
        stands.extend(phrases_in(s, &family.phrases).map(|(at, word)| Stand {
            at,
            word,
            family: Some(family),
            rank: (k + 1, rank(family.rows, word)),
            len: word.len(),
        }));
        stands.extend(
            pattern_stands(s, family.reads)
                .into_iter()
                .map(|(at, len, word)| Stand {
                    at,
                    word,
                    family: Some(family),
                    rank: (k + 1, 0),
                    len,
                }),
        );
    }
    stands.sort_by_key(|st| st.rank);
    let open = stands
        .iter()
        .find(|st| land_open(line, s, (prior, said_main), st, &stands))
        .map(|st| st.family.and_then(|f| f.says).unwrap_or(st.word));
    if !asks(s) {
        return open;
    }
    // A question a plain yes answers leaves what it states (`Is the stash
    // empty? Yes.` leaves what `the stash is empty.` leaves): its rows are
    // read in that statement.
    let leftover = match stated(s) {
        Some(t) if open.is_some() && reply(&line[len..]) == Some(Answer::Yes) => {
            land_status(&t, t.len(), prior, families, said_main).is_some()
        }
        _ => open.is_some(),
    };
    // A question answered as it asks leaves nothing, its rows included
    // (`Unpushed commits? None.`).
    question_open(s, &line[len..], leftover).then(|| open.unwrap_or(QUESTION))
}

/// The statement a polar question asks ([`land_status`]): `is the stash
/// empty?` states `the stash is empty.`, `stash empty?` states `stash is
/// empty.` — its verb moved after its subject, before the first word of its
/// predicate (a verb, a state or a place on main); `None` for any other
/// question.
fn stated(s: &str) -> Option<String> {
    let t = s.trim_end().strip_suffix('?')?;
    let head = t.rfind(':').map_or(0, |i| i + 1);
    let words: Vec<&str> = t[head..].split_whitespace().collect();
    let state = |w: &str| {
        let w = bare(w);
        verbish(w) || matches!(w, "empty" | "clean" | "gone" | "green" | "red")
    };
    let place = |k: usize| {
        let rest: Vec<&str> = words[k..].iter().map(|w| bare(w)).collect();
        onto_main(&rest)
    };
    let first = bare(words.first()?);
    let (aux, subject, p) = if matches!(first, "is" | "are" | "was" | "were") {
        let p = (2..words.len()).find(|&k| state(words[k]) || place(k))?;
        (first, 1, p)
    } else {
        // No verb: a state or a place after the thing (`stash empty?`, `PR
        // merged?`), its first word a noun whatever else it can be.
        let adjective = |w: &str| {
            let w = bare(w);
            past_form(w) || matches!(w, "empty" | "clean" | "gone" | "green" | "red")
        };
        let p = (1..words.len()).find(|&k| adjective(words[k]) || place(k))?;
        if words[1..p].iter().any(|w| verbish(bare(w))) {
            return None;
        }
        ("is", 0, p)
    };
    Some(format!(
        "{} {} {aux} {}.",
        t[..head].trim_end(),
        words[subject..p].join(" "),
        words[p..].join(" ")
    ))
}

/// The tokens of the sentence `s` a pattern family stands on, beside its
/// rows ([`land_status`]): `(at, len, word)`. A change named by its number
/// ([`Reads::Change`]: `#45`, `!12`, `gh-45`, read as the word `#n`), and a
/// part of a whole by its percent ([`Reads::Percent`]: `90%`, read as `%`).
fn pattern_stands(s: &str, reads: Reads) -> Vec<(usize, usize, &'static str)> {
    if matches!(reads, Reads::Absent) {
        // A test missing by its kind (`no end-to-end test`, `no Linux-level
        // tests`), read as `no test`.
        let words: Vec<(usize, &str)> = words_at(s, 0, s.len()).collect();
        return words
            .windows(3)
            .filter(|w| {
                bare(w[0].1) == "no"
                    && bare(w[1].1).contains('-')
                    && !w[1].1.ends_with([',', ';', ':', '.'])
                    && matches!(bare(w[2].1), "test" | "tests")
            })
            .map(|w| {
                let at = w[0].0 + w[0].1.find("no").unwrap_or(0);
                let (raw, noun) = (w[2].1, bare(w[2].1));
                let end = w[2].0 + raw.find(noun).unwrap_or(0) + noun.len();
                (at, end - at, "no test")
            })
            .collect();
    }
    if !matches!(reads, Reads::Change | Reads::Percent) {
        return Vec::new();
    }
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    words_at(s, 0, s.len())
        .filter_map(|(p, raw)| {
            let lead = raw.len() - raw.trim_start_matches('(').len();
            let t = raw[lead..].trim_end_matches(['.', ',', ';', ':', ')', '!', '?']);
            let at = p + lead;
            match reads {
                Reads::Change => {
                    let number = t
                        .strip_prefix('#')
                        .or_else(|| t.strip_prefix('!'))
                        .or_else(|| t.strip_prefix("gh-"))
                        .or_else(|| {
                            // A change's own page (`https://github.com/o/r/pull/440`).
                            ["/pull/", "/pulls/", "/merge_requests/"]
                                .iter()
                                .find_map(|p| t.split_once(p))
                                .filter(|(site, _)| site.contains("://"))
                                .map(|(_, n)| n.trim_end_matches('/'))
                        });
                    number
                        .filter(|n| digits(n))
                        .map(|_| (at, t.len(), CHANGE_NUMBER))
                }
                Reads::Percent => t
                    .strip_suffix('%')
                    .filter(|n| below_hundred(n))
                    .map(|_| (at, t.len(), PERCENT)),
                _ => None,
            }
        })
        .collect()
}
/// What Land names a question in a defense by ([`land_status`]): `Should I
/// bump the version and publish?` leaves the decision open.
const QUESTION: &str = "a question";

/// What Land names a change by its number by ([`Reads::Change`]: `#45`, `!12`, `gh-45`).
const CHANGE_NUMBER: &str = "#n";

/// What Land names a part of a whole by its percent by ([`Reads::Percent`]: `90%`).
const PERCENT: &str = "%";

/// What Land names a count beside a none by ([`aside_status`]: `Open PRs: none (1 upstream)`).
const COUNT_BESIDE_NONE: &str = "a count beside none";

/// Whether the sentence `s` asks a question: it ends with `?` outside
/// quotation marks (code is never a word of the report).
fn asks(s: &str) -> bool {
    let t = s.trim_end();
    t.ends_with('?') && !quoted(t, t.len() - 1)
}

/// Whether what follows a question (`rest`, the rest of its line) answers
/// it, and how ([`question_open`]): its next sentence, past an `A:` label.
/// `None` where nothing follows on the line.
fn reply(rest: &str) -> Option<Answer> {
    let t = reply_text(rest);
    let end = t.len();
    let words: Vec<&str> = t
        .split_whitespace()
        .map(bare)
        .filter(|w| !w.is_empty())
        .collect();
    let first = *words.first()?;
    // A bare word, alone in its clause: `No.`, `No — the race is fixed`,
    // `Yes, as abc123.`, never `No one has decided.`
    let bare_word = t.strip_prefix(first).is_some_and(|r| {
        r.is_empty()
            || r.starts_with(['.', ',', ';', ':', '!', '—', '–', ')'])
            || [" —", " –", " - "].iter().any(|p| r.starts_with(p))
    });
    let empty = words
        .windows(2)
        .any(|w| matches!(w, ["is" | "are" | "was" | "were", "empty"]));
    Some(
        if bare_word
            && matches!(
                first,
                "yes" | "yep" | "yeah" | "yup" | "correct" | "confirmed" | "affirmative" | "indeed"
            )
        {
            // A yes alone, or with no more than where or as what (`Yes, as
            // abc1234.`, `Yes, on origin/main.`): any other word qualifies
            // it (`Yes, once CI passes`, `Yes, into develop`, `Yes,
            // locally`, `Yes, one`), and a qualified yes asserts nothing.
            // `Yes, and main has the fix`: a yes and main holding the work.
            let main_too = words.get(1) == Some(&"and")
                && main_claimed(&words[2..].join(" "))
                && !words
                    .iter()
                    .any(|w| negation(w) || UNSETTLED.contains(w) || DEFICITS.contains(w));
            if yes_tail(&words[1..]) || main_too {
                Answer::Yes
            } else {
                Answer::Said
            }
        } else if (bare_word
            && matches!(first, "no" | "nope" | "none" | "nothing" | "nil" | "zero"))
            || nil_words(&t[..end])
            || empty
        {
            Answer::No
        } else {
            Answer::Said
        },
    )
}

/// The sentence after a question (`rest`, the rest of its line), past an
/// `A:` label ([`reply`]).
fn reply_text(rest: &str) -> &str {
    let t = rest.trim_start();
    let t = t
        .strip_prefix("a:")
        .or_else(|| t.strip_prefix("answer:"))
        .unwrap_or(t)
        .trim_start();
    &t[..sentence_starts(t).get(1).copied().unwrap_or(t.len())]
}

/// Whether the words after a yes (`tail`) leave it a plain yes ([`reply`]):
/// each one of [`YES_TAIL`] or a commit's sha, or a run that shows it
/// (`Yes, run #4812`), or every one of the checks (`Yes, every lane`).
fn yes_tail(tail: &[&str]) -> bool {
    let mut k = 0;
    while k < tail.len() {
        let w = tail[k];
        let run = matches!(w, "run" | "build" | "job" | "pipeline")
            && tail
                .get(k + 1)
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        let every = matches!(w, "every" | "each")
            && tail
                .get(k + 1)
                .is_some_and(|n| matches!(*n, "lane" | "check" | "job" | "cell" | "one" | "test"));
        if run || every {
            k += 2;
        } else if YES_TAIL.contains(&w) || sha(w) {
            k += 1;
        } else {
            return false;
        }
    }
    true
}

/// The words a yes may carry and stay a plain yes ([`reply`]): where the
/// work is and as what (`Yes, as abc1234 on origin/main.`, `Yes, both.`).
const YES_TAIL: &[&str] = &[
    "as",
    "at",
    "on",
    "in",
    "to",
    "into",
    "the",
    "remote",
    "main",
    "origin/main",
    "upstream/main",
    "both",
    "all",
    "of",
    "them",
    "it",
    "is",
    "are",
    "merged",
    "pushed",
    "and",
];

/// Whether `w` is a commit's short or full hash (`abc1234`): six to forty
/// hex digits, a digit among them.
fn sha(w: &str) -> bool {
    (6..=40).contains(&w.len())
        && w.bytes().all(|b| b.is_ascii_hexdigit())
        && w.bytes().any(|b| b.is_ascii_digit())
}

/// How the sentence after a question answers it ([`reply`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Answer {
    /// A plain yes (`Is CI green? Yes.`, `Yes, as abc1234.`).
    Yes,
    /// A no, or a sentence that says there is none (`Anything unpushed?
    /// `git log origin/main..HEAD` is empty.`).
    No,
    /// Something else said (`Why the delay? The CI queue was slow.`), a
    /// qualified yes among it (`Yes, once CI passes.`).
    Said,
}

/// The words that make a polar question ask after a leftover, whether or
/// not a row of it reads open ([`question_open`]): `Was a changelog entry
/// needed?`, `Is the PR open?` — a yes asserts the leftover, a no denies it.
const DEFICITS: &[&str] = &[
    "need",
    "needs",
    "needed",
    "required",
    "requires",
    "missing",
    "left",
    "pending",
    "open",
    "broken",
    "fail",
    "fails",
    "failed",
    "failing",
    "skipped",
    "flaky",
    "blocked",
    "outstanding",
    "unmerged",
    "unpushed",
    "uncommitted",
    "unlanded",
    "stale",
    "red",
    "deferred",
    "disabled",
    "waiting",
    "owed",
];

/// The deficits a no settles even for a change ([`question_open`]): `Is the
/// docs PR unmerged? No.` says it is merged; `Is the PR open? No.` does not.
const UNLANDED_DEFICITS: &[&str] = &["unmerged", "unpushed", "uncommitted", "unlanded"];

/// WHETHER A QUESTION IN A DEFENSE LEAVES SOMETHING OPEN ([`land_status`]):
/// `s` the question, `rest` the rest of its line, `leftover` whether a row
/// of the question reads open on its own (`Anything unmerged?`). FAIL
/// TOWARD OPEN: a question to the reader always does (`Should I publish?`,
/// `Do you want the docs too?`, `Merge now or after the release?`), and so
/// does one nothing answers on its line. Answered:
///
/// * one about what is left (`Unpushed commits? None.`, `Is anything left?
///   No.`) closes only on a no;
/// * one asking why or how (`Why the delay? The CI queue was slow.`) closes
///   on any answer when it names no leftover itself — an explanation, read
///   on its own; what, which, who, when and where close only on a no (`What
///   failed? The Windows lane.` names the leftover in its answer), save a
///   where the remote main answers and nothing more (`Where is it?
///   origin/main, abc1234.`);
/// * a polar one (`Is it merged?`, `Merged?`, `CI green?`) closes on a plain
///   yes ([`reply`]) only when it asks after a landing or a settled state
///   and names no leftover (`Is it on origin/main? Yes.`) — a change only
///   after its landing or its being gone (`Is the PR approved? Yes.` stays
///   open; `Is the branch deleted? Yes.`, `Stash empty? Yes.` close), its
///   rows read in the statement it asks ([`stated`]) — and on a no
///   only when it asks after a leftover ([`DEFICITS`]: `Is the test still
///   flaky? No.`; a change only by an unlanded word or an answer that lands
///   it). `Will the owner tag it? Yes.` asserts work left, and stays open.
fn question_open(s: &str, rest: &str, leftover: bool) -> bool {
    let q = s.rsplit_once(':').map_or(s, |(_, q)| q);
    let words: Vec<&str> = q
        .split_whitespace()
        .map(bare)
        .filter(|w| !w.is_empty())
        .collect();
    let first = words.first().copied().unwrap_or("");
    let second = words.get(1).copied().unwrap_or("");
    let addressed = words.contains(&"or")
        || first == "shall"
        || words.windows(2).any(|w| {
            matches!(
                w,
                [
                    "should" | "shall" | "may" | "can" | "could" | "would" | "will" | "do" | "did",
                    "i" | "we" | "you"
                ] | ["want", "me"]
                    | ["like", "me"]
            )
        });
    if addressed {
        return true;
    }
    let Some(answer) = reply(rest) else {
        return true;
    };
    let wh = matches!(
        first,
        "what"
            | "what's"
            | "which"
            | "who"
            | "who's"
            | "whom"
            | "whose"
            | "when"
            | "where"
            | "where's"
            | "why"
            | "how"
    );
    let change = words
        .iter()
        .any(|w| CHANGE_NOUNS.contains(w) || w.starts_with('#'))
        || q.contains("pull request");
    let lands = |w: &[&str]| {
        w.iter().any(|x| SHIPS.contains(x) || LANDED.contains(x))
            || (0..w.len()).any(|i| onto_main(&w[i..]))
    };
    let told: Vec<&str> = reply_text(rest)
        .split_whitespace()
        .map(bare)
        .filter(|w| !w.is_empty())
        .collect();
    if wh {
        if second == "about" {
            // `What about the docs? Merged with the fix.`: a thing's state,
            // which only its landing answers (the answer is read on its own).
            return leftover || !lands(&told);
        }
        // `Where is it? origin/main, abc1234.`: a place on the remote main
        // and nothing more answers where.
        let remote = |w: &&str| matches!(*w, "origin/main" | "upstream/main" | "origin/master");
        let placed = matches!(first, "where" | "where's")
            && told.iter().any(remote)
            && told
                .iter()
                .all(|w| remote(w) || YES_TAIL.contains(w) || sha(w));
        return !((matches!(first, "why" | "how") || placed) && !leftover) && answer != Answer::No;
    }
    let settled_word = |w: &&str| SETTLED.contains(w) || matches!(*w, "clean" | "gone" | "empty");
    // A question whether anything is left asks for a no, whatever its verb
    // (`Is anything left? No.`), and so does a thing named with no verb and
    // no state (`Open PRs? None.`, `Stashes? None.`).
    let exists = second == "there"
        || !(words.iter().any(|w| verbish(w) || settled_word(w)) || lands(&words))
        || words.iter().any(|w| {
            matches!(
                *w,
                "any"
                    | "anything"
                    | "anyone"
                    | "anybody"
                    | "something"
                    | "else"
                    | "left"
                    | "remaining"
                    | "outstanding"
            )
        });
    if exists {
        return answer != Answer::No;
    }
    let deficit = words.iter().find(|w| DEFICITS.contains(w));
    match answer {
        Answer::Yes => {
            // A change asked after is settled by its landing, or by its
            // being gone (`Is the branch deleted? Yes.`, `Is the stash
            // empty? Yes.`), never by another state (`Is the PR approved?`).
            let gone = |w: &&str| GONE.contains(w) || matches!(*w, "gone" | "empty");
            leftover
                || deficit.is_some()
                || !(lands(&words)
                    || (!change && words.iter().any(settled_word))
                    || words.iter().any(gone))
        }
        Answer::No => {
            !((leftover || deficit.is_some())
                && (!change
                    || deficit.is_some_and(|d| UNLANDED_DEFICITS.contains(d))
                    || lands(&told)))
        }
        Answer::Said => true,
    }
}

/// Whether the row `st` stands open in the sentence `s` (`line` runs on to
/// the end of its line, `prior` is the line before it) under Land: not quoted, no part of a file's name
/// (`TODO.md`), not opened on the `YOU` of a NEEDS YOU token its clause has for subject
/// (`NEEDS YOU can be lost.`, [`needs::token_you`]: no `you can` asks the person there), not a
/// description (`do not merge`, `a warm cache is skipped`, a past state: `the test that was
/// waiting for review`, `the previously skipped test`, [`described`]), not settled (`the pending PR
/// was merged`, [`settled`]) and not denied (`nothing is pending, deferred
/// or parked`, [`denied`]) — then as its family reads it ([`Reads`]).
/// WHATEVER BLOCKS IT: no blocker, decision, slowness or reason excuses a
/// row here — `not pushed: no remote is set` and `not merged, because no
/// reviewer approved it` are open (a reason that leaves nothing to do,
/// [`nothing_to_do`], is Cruise's reading alone).
fn land_open(
    line: &str,
    s: &str,
    (prior, said_main): (&str, bool),
    st: &Stand<'_>,
    stands: &[Stand<'_>],
) -> bool {
    let (at, end) = (st.at, st.end());
    if quoted(s, at) || in_path(s, at, end) || nil_item(s, at, end) || needs::token_you(s, at) {
        return false;
    }
    if matches!(st.reads(), Reads::Exception) {
        return nothing_but(s, st) || (excepted(s, st) && excepting(s, stands));
    }
    if matches!(st.reads(), Reads::Qualified) {
        return qualified_nil(s, st);
    }
    if matches!(st.reads(), Reads::OnlyTail) {
        return only_item(s, st);
    }
    if matches!(st.reads(), Reads::Party) {
        let rest = &s[st.end()..];
        // A decision or a fix left to a party (`the owner of the reset work
        // has to decide`, `whoever owns that commit should fix it`), never
        // one denied (`no longer has to decide`, `should decide nothing
        // more`).
        let clause: Vec<&str> = rest[..clause_end(rest, 0)]
            .split_whitespace()
            .map(bare)
            .collect();
        let owed = |modal: usize, verb: usize| {
            clause
                .get(verb)
                .is_some_and(|w| matches!(*w, "decide" | "fix" | "choose" | "rule"))
                && !clause[..modal].iter().any(|w| negation(w))
                && !clause
                    .get(verb + 1)
                    .is_some_and(|w| matches!(*w, "nothing" | "none"))
        };
        if (0..clause.len()).any(|i| {
            (matches!(clause[i], "has" | "have" | "needs" | "need")
                && clause.get(i + 1) == Some(&"to")
                && owed(i, i + 2))
                || (matches!(clause[i], "should" | "must") && owed(i, i + 1))
        }) {
            return true;
        }
        // The whole item after a colon: `release: owner`, `tag: the owner`.
        return s[..st.at]
            .trim_end()
            .strip_suffix(':')
            .is_some_and(|label| !label.trim().is_empty())
            && rest[..clause_end(rest, 0)]
                .split_whitespace()
                .map(bare)
                .all(|w| w.is_empty() || matches!(w, "only" | "alone" | "now"));
    }
    // A git word that names a part of the code (`fixed the stash command's
    // help text`) reports no stash.
    if matches!(
        st.word,
        "stash" | "stashes" | "draft" | "drafts" | "wip" | "worktree"
    ) && s[end..]
        .split_whitespace()
        .next()
        .map(|w| bare(w).trim_end_matches("'s"))
        .is_some_and(|w| CODE_PARTS.contains(&w))
    {
        return false;
    }
    // What a row does to nothing leaves nothing: `skipped nothing`.
    if s[end..]
        .split_whitespace()
        .next()
        .map(bare)
        .is_some_and(|w| matches!(w, "nothing" | "none"))
    {
        return false;
    }
    let from = clause_start(s, at);
    let to = clause_end(s, end);
    let before: Vec<&str> = s[from..at].split_whitespace().map(bare).collect();
    let after: Vec<&str> = s[end..to]
        .split_whitespace()
        .map(bare)
        .filter(|w| !w.is_empty())
        .collect();
    let prev = before.last().copied();
    let next = after.first().copied();
    if st.word.starts_with("not ") && prev.is_some_and(|p| OPEN_MODALS.contains(&p)) {
        return false;
    }
    // `unstaged` as the worker's own act on a thing (`immediately unstaged
    // them`), never a state left (`unstaged changes`).
    if st.word == "unstaged" && next.is_some_and(|n| matches!(n, "it" | "them" | "those" | "these"))
    {
        return false;
    }
    // git's own words for a checkout of main (`Your branch is up to date with
    // 'origin/main'`), and a word that names a thing's kind after its
    // determiner (`the provided config`, `an unknown flag`).
    if (matches!(st.word, "up to date with" | "up-to-date with") && before.contains(&"your"))
        || (matches!(st.word, "provided" | "providing" | "unknown")
            && prev.is_some_and(|p| DETERMINERS.contains(&p) || p == "as"))
    {
        return false;
    }
    if matches!(st.word, "could not" | "couldn't") && cannot_happen(&s[end..to]) {
        return false;
    }
    // A test's proof (`mutation runs show they can still fail`), read in its
    // own clause ([`proves_by_failing`]).
    let proof_row = PROOF_ROWS.contains(&st.word)
        || (st.word == "still" && next.is_some_and(|n| PROOF_ROWS.contains(&n)));
    // A control that failed to fail/detect/run did not exercise its check.
    // `failed to pass` still names the expected negative outcome. Keep this
    // subject local: an unrelated UI control is no test noun everywhere else.
    let failed_proof = matches!(st.word, "fail" | "fails" | "failed" | "failing")
        && next == Some("to")
        && !after
            .get(1)
            .is_some_and(|w| matches!(*w, "pass" | "succeed"))
        && (before
            .windows(2)
            .any(|w| matches!(w, ["negative", "control" | "controls"]))
            || before
                .iter()
                .any(|w| matches!(*w, "mutation" | "mutations" | "mutant" | "mutants")));
    if proof_row && !failed_proof && proves_by_failing(s, (from, to), (at, end)) {
        return false;
    }
    // `a warm cache is skipped` describes code; `items 1 and 3 are skipped`
    // reports the task's own items left undone ([`TASK_ITEMS`]).
    if st.family.is_none()
        && st.word == "skipped"
        && prev.is_some_and(|p| ["is", "are", "be"].contains(&p))
        && !before.iter().any(|w| TASK_ITEMS.contains(w))
    {
        return false;
    }
    if described(&before) || behind(&before) {
        return false;
    }
    if matches!(st.reads(), Reads::Unlanded | Reads::Undone) {
        return landing_denied(s, from, at, &after, st, stands);
    }
    if aside_settled(s, st) || also_landed(s, (from, to), st, stands) {
        return false;
    }
    let settles = match past_aside(s, to) {
        Some((joined, to)) => settled(&joined, from, to, st),
        None => settled(s, from, to, st),
    };
    if settles || (st.word != "yet" && denied(s, from, at, st, stands)) {
        return false;
    }
    if failed_proof {
        return true;
    }
    match st.reads() {
        Reads::Anywhere if matches!(st.word, "local main" | "main locally") => {
            !even_with_remote(&after)
        }
        // A term of the code's (`the partial index migration`), and checks
        // run one by one (`tested the parser and the lexer separately`).
        Reads::Anywhere
            if st.word == "partial" && next.is_some_and(|n| PARTIAL_TERMS.contains(&n)) =>
        {
            false
        }
        // Sent for review after a push or as a change (`force-pushed
        // perf7/lab-r9 for review`), never a wait a description names (`the
        // test that was waiting for review feedback is fixed`).
        Reads::Anywhere if matches!(st.word, "for review" | "for a review") => {
            before.iter().any(|w| {
                matches!(
                    *w,
                    "pushed" | "force-pushed" | "sent" | "posted" | "submitted" | "up" | "put"
                ) || CHANGE_NOUNS.contains(w)
                    || branch_name(w)
            })
        }
        Reads::Anywhere
            if st.word == "separately"
                && before.iter().any(|w| TESTED.contains(w))
                && !before.iter().any(|w| negation(w) || UNSETTLED.contains(w)) =>
        {
            false
        }
        Reads::Anywhere
        | Reads::Exception
        | Reads::Party
        | Reads::OnlyTail
        | Reads::Qualified
        | Reads::Unlanded
        | Reads::Undone => true,
        Reads::Alone => {
            alone(&s[end..to])
                && !prev.is_some_and(|p| matches!(p, "main" | "origin/main" | "master"))
        }
        Reads::Beside(nouns) => {
            let (a, b) = stretch(s, from, to, (at, end), st.settled_by());
            let counted = prev.and_then(count).is_some_and(|n| n > 0);
            // A test run's whole tally (`1649 passed, 0 failed, 2 ignored`),
            // with nothing after it but its bracket or the sentence's end:
            // the tests the suite already ignores, nothing this work turned
            // off. A tally without its failures (`211 passed, 1 ignored`), or
            // one that goes on to say which (`3 ignored, the 3 new tests`),
            // stays a skipped check.
            let tally: Vec<&str> = s[..at].split_whitespace().map(bare).collect();
            let tail = s[end..].trim_start();
            let tallied = st.word == "ignored"
                && (tail.starts_with(')') || tail.split_whitespace().all(|w| bare(w).is_empty()))
                && counted
                && tally
                    .windows(2)
                    .any(|w| count(w[0]).is_some() && w[1] == "passed")
                && tally
                    .windows(2)
                    .any(|w| count(w[0]) == Some(0) && w[1] == "failed");
            if tallied {
                return false;
            }
            // A check the worker turned off, named by a pronoun (`I disabled
            // it there`): what else is disabled.
            let turned_off = prev.is_some_and(|p| matches!(p, "i" | "we"))
                && next.is_some_and(|n| matches!(n, "it" | "them"))
                && matches!(
                    st.word,
                    "disabled"
                        | "ignored"
                        | "quarantined"
                        | "muted"
                        | "skipped"
                        | "suppressed"
                        | "silenced"
                        | "turned off"
                        | "switched off"
                        | "commented out"
                );
            // A check turned off in code is the code's (`check ignored`,
            // `lint allowed`, [`CODE_MARKS`]), read by its own family.
            let code_mark = matches!(
                (prev, st.word),
                (Some("check"), "ignored") | (Some("lint"), "allowed")
            );
            !code_mark
                && !next.is_some_and(|n| CONTROLS.contains(&n))
                && (counted
                    || turned_off
                    || nouns_in(s, a, b, nouns)
                    || labelled(s, from, &before, nouns))
        }
        Reads::Later => {
            let with = st.word.ends_with(" with")
                && !after
                    .iter()
                    .find(|w| !matches!(**w, "the" | "a" | "an"))
                    .is_some_and(|w| {
                        LATER_WITH.contains(w)
                            || (w.starts_with('v')
                                && w[1..].starts_with(|c: char| c.is_ascii_digit()))
                            || w.starts_with(|c: char| c.is_ascii_digit())
                    });
            // A past verb in the clause dates it (`merged on Monday`, `the
            // sign-off came this morning`): one undenied before the row, with
            // nothing that puts it off (`moved to Monday`).
            let dated = DATED.contains(&st.word)
                && before.iter().any(|w| {
                    past_form(w) || matches!(*w, "came" | "went" | "ran" | "got" | "was" | "were")
                })
                && !before.iter().enumerate().any(|(i, w)| {
                    // `pushed out` puts it off; `went out` shipped it.
                    let put_off = *w == "out"
                        && i.checked_sub(1).is_some_and(|p| {
                            matches!(
                                before[p],
                                "pushed" | "moved" | "bumped" | "put" | "kicked" | "punted"
                            )
                        });
                    put_off
                        || negation(w)
                        || (UNSETTLED.contains(w) && !(*w == "to" && onto_main(&before[i..])))
                        || matches!(
                            *w,
                            "back"
                                | "until"
                                | "till"
                                | "by"
                                | "for"
                                | "moved"
                                | "bumped"
                                | "rescheduled"
                                | "delayed"
                                | "postponed"
                                | "deferred"
                                | "planned"
                                | "scheduled"
                                | "slated"
                                | "set"
                                | "booked"
                                | "requested"
                                | "asked"
                                | "pencilled"
                                | "penciled"
                        )
                });
            !with
                && !dated
                && !AUTOMATIC.iter().any(|p| s.contains(p))
                && !past_attributive(st, prev, &after)
        }
        Reads::Until => {
            !before
                .iter()
                .any(|w| MECHANISMS.contains(w) && !(WAITS.contains(w) && waits_on_work(&before)))
                && !AUTOMATIC.iter().any(|p| s.contains(p))
        }
        Reads::Elided => next.is_none(),
        Reads::Intent => {
            // `Consider caching the parse.` opens a sentence as a suggestion;
            // elsewhere `consider` reports (`we consider it done`).
            (st.word != "consider" || s[..at].trim().is_empty())
                && !ends_intent(&s[end..], line[s.len()..].trim().is_empty())
                && !after.iter().any(|w| DENIALS.contains(w))
        }
        Reads::Gerund => next.is_some_and(|n| {
            (n.len() > 4 && n.ends_with("ing") && !n.ends_with("thing") && n != "during")
                || matches!(n, "a" | "an" | "another" | "more" | "some")
                // The worker's own need, or one with no subject (`Need to
                // double-check the migration`, `we need to rerun it`).
                || (n == "to"
                    && (s[..at].trim().is_empty()
                        || prev.is_some_and(|p| matches!(p, "i" | "we" | "also" | "just"))))
        }),
        Reads::Still => {
            let rest: Vec<&str> = s[end..to].split_whitespace().map(bare).collect();
            // What the worker kept on purpose (`the old filter is still there
            // on purpose`): a choice, never a fault's word beside it.
            if rest
                .windows(2)
                .any(|w| matches!(w, ["on", "purpose"] | ["by", "design"]))
                && !before.iter().any(|w| FAULT_NOUNS.contains(w))
            {
                return false;
            }
            let (first, second) = (rest.first().copied(), rest.get(1).copied());
            // A test that still catches a fault (`still catches the bug it
            // was written for`), never a check that still flags the work
            // (`the census still catches my new call`).
            if matches!(first, Some("catches" | "catch")) {
                return !rest[1..].iter().any(|w| FAULT_NOUNS.contains(w));
            }
            // A test that would pass with its fix taken out proves nothing
            // (`would still pass if that fix were removed`, `still passes
            // without the fix`), never `still passes when the fix is in`.
            if matches!(first, Some("pass" | "passes"))
                && rest.iter().enumerate().any(|(i, w)| {
                    let later = &rest[i + 1..];
                    (matches!(*w, "if" | "when")
                        && later
                            .iter()
                            .any(|w| matches!(*w, "removed" | "reverted" | "undone")))
                        || (*w == "without"
                            && later
                                .iter()
                                .take(3)
                                .any(|w| matches!(*w, "fix" | "change" | "patch")))
                })
            {
                return true;
            }
            let ok = match rest.get(1..) {
                Some(["with", code]) | Some(["with", "a" | "an", code]) => ok_status(code),
                _ => false,
            };
            // What a comma right after it adds to what still holds qualifies
            // it (`still responds, slowly`, `still loads, but slowly`): any
            // words after a [`STILL_HOLDS`] word, and a phrase with no verb
            // after a [`STILL_GOOD`] one (`still works, the GUI does not`
            // reads its second clause on its own).
            let more = s[to..]
                .strip_prefix(',')
                .filter(|_| rest.len() == 1)
                .map(|r| {
                    let words: Vec<&str> = r[..clause_end(r, 0)]
                        .split_whitespace()
                        .map(bare)
                        .filter(|w| !w.is_empty())
                        .collect();
                    (
                        !words.is_empty(),
                        !words.is_empty() && !words.iter().any(|w| verbish(w)),
                    )
                });
            let (any_more, fragment) = more.unwrap_or((false, false));
            // A fault that still holds is still there (`the regression still
            // holds`, `the bug still applies on Windows`): the head of the
            // subject, before any preposition (`the fix for the race still
            // holds` is the fix).
            let np = before
                .iter()
                .position(|w| PREPOSITIONS.contains(w))
                .unwrap_or(before.len());
            let head = before[..np]
                .iter()
                .rfind(|w| !matches!(**w, "is" | "are" | "was" | "were"));
            let fault = head.is_some_and(|w| FAULT_NOUNS.contains(w));
            // `still accepted: the known crash` lists what is still so.
            fault
                || s[to..].starts_with(':')
                || first.is_none_or(|n| {
                    !((STILL_GOOD.contains(&n) && !fragment)
                        || (STILL_HOLDS.contains(&n)
                            && !any_more
                            && (second.is_none_or(|w| {
                                matches!(w, "for" | "to" | "as" | "on" | "in" | "after" | "from")
                                    || STILL_MANNER.contains(&w)
                            }) || ok))
                        || (matches!(n, "exists" | "exist") && second == Some("as"))
                        || n.starts_with(|c: char| c.is_ascii_digit())
                        || (DETERMINERS.contains(&n) && !second.is_some_and(|w| OLD.contains(&w))))
                })
        }
        Reads::Ref if st.word == "--no-merged" => !lists_none(&s[end..]),
        Reads::Ref => off_main(&s[end..]),
        // A check's reach named as its part (`it is covered by the unit test
        // only`): the clause ends at `only`, never a reach it had (`which was
        // covered by the unit test only`).
        Reads::Fraction if st.word == "only" => {
            (before.is_empty() && only_part(&after))
                || (after.is_empty()
                    && !before.iter().any(|w| matches!(*w, "was" | "were"))
                    && before.iter().any(|w| {
                        matches!(
                            *w,
                            "covered" | "tested" | "exercised" | "checked" | "verified"
                        )
                    }))
        }
        Reads::Fraction => {
            (short_count(&before, &s[end..to]) || leading_part(&before, &s[end..]))
                && !rest_settled(&line[end..])
        }
        Reads::NotDone => next.is_some_and(|n| {
            !AVOIDED.contains(&n)
                && !negation(n)
                && !(STEADY.contains(&n) && !prev.is_some_and(|p| matches!(p, "i" | "we")))
        }),
        Reads::Lost => next.is_some_and(|n| WORKS.contains(&n)),
        Reads::Better => {
            next.is_some_and(|n| (n.len() > 4 && n.ends_with("ed")) || COMPARATIVES.contains(&n))
        }
        Reads::Concession => conceding(&format!("{prior}{}", &s[..at]), &s[end..]),
        Reads::State => {
            !next.is_some_and(|n| CONTROLS.contains(&n) || n == "while")
                && !prev.is_some_and(|p| STOPS.contains(&p))
                && !after.iter().take(3).any(|w| ATTEMPTS.contains(w))
        }
        Reads::End => {
            // A remote that keeps what needs no main (`the branch is deleted
            // locally and on origin`, `the tag is on origin`), and the remote's
            // own main (`merged into main on origin`).
            let kept_off_main = matches!(st.word, "on origin" | "on the remote" | "local")
                && (prev.is_some_and(|p| MAINS.contains(&p))
                    || before.iter().any(|w| {
                        GONE.contains(w)
                            || matches!(*w, "tag" | "tags" | "deleted" | "cleaned")
                            || version(w)
                    }));
            next.is_none()
                && !kept_off_main
                && !prev.is_some_and(|p| matches!(p, "too" | "as" | "so" | "how"))
                // `Aside: …` labels what it sets aside, read on its own.
                && !(st.word == "aside" && before.is_empty() && s[to..].starts_with(':'))
        }
        Reads::Count => {
            (next.is_none() || next == Some("to"))
                && before.iter().rev().take(3).any(|w| counted(w))
        }
        Reads::OneMore => {
            // The thing counted is no verb (`one more pass over the docs`), nor
            // is what a condition after it says (`one more soak run before I'd
            // call it stable`).
            let settles = |w: &&str| {
                ((SETTLED.contains(w) || LANDED.contains(w))
                    && (past_form(w) || matches!(*w, "done" | "green" | "clean")))
                    || negation(w)
            };
            !next.is_some_and(|n| matches!(n, "time" | "times"))
                && !before
                    .iter()
                    .any(|w| SETTLED.contains(w) || LANDED.contains(w) || negation(w))
                && !after
                    .iter()
                    .skip(1)
                    .take_while(|w| {
                        !CONDITIONS.contains(w) && !matches!(**w, "before" | "so" | "to")
                    })
                    .any(settles)
        }
        Reads::Only => {
            (s[..at].trim().is_empty()
                && s.split_whitespace().count() <= 5
                && !s[end..].split_whitespace().map(bare).any(verbish))
                // `no blockers, only polish`: what a none leaves, after it.
                || (before.is_empty()
                    && s[..from].trim_end().ends_with(',')
                    && s[clause_start(s, from.saturating_sub(1))..from]
                        .split_whitespace()
                        .map(bare)
                        .next()
                        .is_some_and(|w| matches!(w, "nothing" | "none" | "no" | "zero")))
        }
        Reads::Elliptic => {
            (1..=3).contains(&before.len())
                && !before.iter().any(|w| {
                    verbish(w)
                        || negation(w)
                        || CUTS.contains(w)
                        || PREPOSITIONS.contains(w)
                        || LEADS.contains(w)
                })
                && !s[end..].split_whitespace().map(bare).any(|w| {
                    SETTLED.contains(&w)
                        || LANDED.contains(&w)
                        || (st.word == "after" && verbish(w))
                })
        }
        // `no test that the error path is logged`: a test of a claim, never
        // `no tests that fail`.
        Reads::Absent => {
            // A test missing by its kind or of a claim, in a gap the sentence
            // puts in the past (`the old code had no end-to-end test`, `there
            // was no test that the error path is logged`), is no gap now.
            let past = prev.is_some_and(|p| matches!(p, "was" | "were" | "had"));
            let kind = st.len != st.word.len();
            (next.is_none_or(|n| GAP_PREPOSITIONS.contains(&n)) && !(kind && past))
                || (next == Some("that")
                    && !past
                    && after.get(1).is_some_and(|w| {
                        matches!(*w, "a" | "an" | "the" | "its" | "each" | "every")
                    }))
        }
        Reads::Nobody => {
            let nothing = st.word == "nothing";
            let filler = |w: &str| {
                matches!(
                    w,
                    "has" | "have" | "had" | "yet" | "ever" | "actually" | "else"
                )
            };
            let Some(k) = after.iter().position(|w| !filler(w)) else {
                return false;
            };
            let w = after[k];
            // `nothing ran on Linux`: no run, read as nobody's; `nothing
            // tests it`: no test of what the sentence names.
            if nothing {
                matches!(
                    w,
                    "ran" | "run" | "tested" | "checked" | "verified" | "measured"
                ) || (matches!(w, "tests" | "covers" | "exercises")
                    && after
                        .get(k + 1)
                        .is_some_and(|o| matches!(*o, "it" | "them" | "this" | "that")))
            } else {
                CHECKED.contains(&w)
            }
        }
        Reads::Onto => {
            // A place said in brackets right after the clause (`all pushed
            // (to b8/reflow)`).
            let bracketed: Vec<&str> = s[end..]
                .trim_start()
                .strip_prefix('(')
                .and_then(|r| r.split_once(')'))
                .map(|(inner, _)| inner.split_whitespace().map(bare).collect())
                .unwrap_or_default();
            ((onto_branch(&after) || (after.is_empty() && onto_branch(&bracketed)))
                && !main_claimed(s))
                || (matches!(st.word, "pushed" | "committed")
                    && unplaced(s, (prior, said_main), &before, &after))
        }
        Reads::Dropped => !disposable(st.word, &before, &after),
        Reads::OnBranch if !matches!(st.word, "on" | "in") => {
            // A branch said to hold it all (`b8/strict-read has everything`).
            prev.is_some_and(|p| {
                (branch_name(p) && !p.split_once('/').is_some_and(|(_, m)| MAINS.contains(&m)))
                    || matches!(p, "branch" | "worktree")
            })
        }
        Reads::OnBranch => {
            on_branch(&after, prev.is_some_and(|p| LANDING_VERBS.contains(&p))) && !main_claimed(s)
        }
        Reads::Change => change_open(s, (from, to), st, &before, &after),
        Reads::Percent => {
            // A rollout counts its part before the percent (`rollout at 50%`).
            let rolling = before.iter().any(|w| {
                matches!(
                    *w,
                    "rollout"
                        | "rolled"
                        | "rolling"
                        | "canary"
                        | "deploy"
                        | "deployed"
                        | "ramp"
                        | "ramped"
                        | "staged"
                )
            });
            // A part as the whole item of a label that names work (`Docs: 80%`).
            let label = s[..at]
                .trim_end()
                .strip_suffix(':')
                .and_then(|l| l.split_whitespace().last())
                .is_some_and(|l| WORK_LABELS.contains(&bare(l)));
            (st.word == "%" || prev.is_some_and(below_hundred))
                && (rolling || percent_part(&s[end..]) || (label && after.is_empty()))
        }
        Reads::Without => after
            .iter()
            .find(|w| !DETERMINERS.contains(w) && !matches!(**w, "any" | "its" | "their"))
            .is_some_and(|w| OWED_WITHOUT.contains(w)),
        Reads::Approval => {
            let (a, b) = stretch(s, from, to, (at, end), LANDS);
            // A clause right after with no subject that lands it (`; merged
            // to origin/main`, `, then merged`).
            let landed_after = {
                let rest = line[to..].trim_start_matches([',', ';', '—', '–', ' ']);
                let words: Vec<&str> = rest[..clause_end(rest, 0)]
                    .split_whitespace()
                    .map(bare)
                    .filter(|w| !w.is_empty())
                    .collect();
                to < line.len()
                    && settles_with(&words, SHIPS, true)
                    && words.first().is_some_and(|w| {
                        SHIPS.contains(w)
                            || LANDING_VERBS.contains(w)
                            || matches!(*w, "then" | "and" | "since" | "now")
                    })
            };
            // `accepted upstream` is the patch landed there.
            let upstream = st.word == "accepted" && next == Some("upstream");
            nouns_in(s, a, b, APPROVED_THINGS) && !landed_after && !upstream
        }
        Reads::Tally => {
            // `a failure on Windows`, `one panic in CI`: a count of one where
            // the clause opens with it or a verb has it (`CI shows a
            // failure`), never inside a phrase (`the fix for a crash`).
            let one = prev.is_some_and(|p| matches!(p, "a" | "an" | "one" | "single"))
                && before.len().checked_sub(2).is_none_or(|i| {
                    matches!(
                        before[i],
                        "has"
                            | "have"
                            | "had"
                            | "shows"
                            | "showed"
                            | "hit"
                            | "hits"
                            | "got"
                            | "saw"
                            | "sees"
                            | "is"
                            | "are"
                            | "was"
                            | "were"
                            | "still"
                            | "remains"
                            | "with"
                            | "there's"
                            | "left"
                    )
                });
            // `errors: 1`: a label counting some.
            let labelled = next.is_none()
                && s[to..].strip_prefix(':').is_some_and(|r| {
                    let mut item = r[..clause_end(r, 0)].split_whitespace().map(bare);
                    item.next().and_then(count).is_some_and(|n| n > 0) && item.next().is_none()
                });
            labelled
                || ((prev.and_then(count).is_some_and(|n| n > 0) || one)
                    && next.is_none_or(|n| {
                        PREPOSITIONS.contains(&n) || JOINS.contains(&n) || verbish(n) || n == "left"
                    }))
        }
        Reads::Rerun => {
            // What the code does again, said in the present of no check
            // (`the refusal is retried`): a mechanism, never a run's flake —
            // in a sentence that names no run's outcome (`the failing step is
            // retried and then passes`, `passes the second time`).
            let mechanism = st.word == "retried"
                && prev.is_some_and(|p| matches!(p, "is" | "are" | "gets" | "get"))
                && !before.iter().any(|w| CHECK_NOUNS.contains(w))
                && !s.split_whitespace().map(bare).any(|w| {
                    matches!(
                        w,
                        "pass"
                            | "passes"
                            | "passed"
                            | "passing"
                            | "green"
                            | "succeeds"
                            | "succeeded"
                            | "fail"
                            | "fails"
                            | "failed"
                            | "failing"
                            | "red"
                            | "second"
                            | "until"
                    )
                });
            !mechanism
                && !s[..at]
                    .split_whitespace()
                    .map(bare)
                    .any(|w| RETRIED_STEPS.contains(&w))
        }
        Reads::Status => {
            // The whole item of a label, past any mark or emoji between
            // (`appkit: ⏭️ skip`).
            let label_text = s[..at]
                .trim_end_matches(|c: char| {
                    c.is_whitespace() || !(c.is_ascii() || c.is_alphanumeric())
                })
                .strip_suffix(':');
            let label = label_text.is_some_and(|l| l.chars().any(char::is_alphanumeric));
            let counted = prev.and_then(count).is_some_and(|n| n > 0);
            let opens = matches!(st.word, "skip" | "skips")
                && s[..at].trim().is_empty()
                && !after.iter().any(|w| verbish(w));
            if st.word == "n/a" {
                // Only a check's item (`| appkit | n/a |`): elsewhere `n/a`
                // says there is nothing (`Open PRs: n/a`).
                label
                    && after.is_empty()
                    && label_text
                        .and_then(|l| l.split_whitespace().last())
                        .is_some_and(|w| CHECK_LABELS.contains(&bare(w)))
            } else {
                (label && after.is_empty()) || counted || opens
            }
        }
        Reads::Most => {
            // The object of a landing (`merged most commits`, `merged the bulk
            // of it`): the rest was not.
            let object = prev.is_some_and(|p| {
                SHIPS.contains(&p) || matches!(p, "pushed" | "committed" | "landed" | "done")
            });
            object
                || !prev.is_some_and(|p| {
                    matches!(p, "the" | "at" | "a" | "an" | "which" | "what" | "whose")
                }) && (after
                    .iter()
                    .any(|w| SETTLED.contains(w) || LANDED.contains(w))
                    || (0..after.len()).any(|i| onto_main(&after[i..])))
        }
    }
}

/// The labels that name a check, whose `n/a` is a check not run
/// ([`Reads::Status`]): `| appkit | n/a |`, `Windowed checks: n/a`.
const CHECK_LABELS: &[&str] = &[
    "appkit", "measure", "verify", "check", "checks", "test", "tests", "lab", "lane", "lanes",
    "fmt", "tippy", "census", "spec", "perf", "cells", "cell", "suite", "e2e",
];

/// The landing steps a retry is no flake of ([`Reads::Rerun`]): `the push
/// failed once on a timeout and succeeded on retry`, `push succeeded on the
/// second try`.
const RETRIED_STEPS: &[&str] = &[
    "push", "pushed", "pushes", "pull", "fetch", "fetched", "clone", "download", "upload",
];

/// Whether `w` reads as a verb or a predicate: one of [`VERBS`], [`AUX`],
/// [`SETTLED`], [`STILL_GOOD`] or [`MORE_VERBS`], or a past form
/// ([`past_form`]).
fn verbish(w: &str) -> bool {
    VERBS.contains(&w)
        || AUX.contains(&w)
        || SETTLED.contains(&w)
        || STILL_GOOD.contains(&w)
        || MORE_VERBS.contains(&w)
        || past_form(w)
}

/// Whether `later` names a thing that came after, in the past, not a day to
/// come ([`Reads::Later`]): after an article, before a noun, in a clause
/// whose verb is past (`a later commit fixed the flake`), never one that is
/// still to come (`a later PR will add the docs`, `the docs come in a later
/// PR`).
fn past_attributive(st: &Stand<'_>, prev: Option<&str>, after: &[&str]) -> bool {
    st.word == "later"
        && prev.is_some_and(|p| matches!(p, "a" | "an" | "the"))
        && after.first().is_some_and(|n| !verbish(n))
        && after
            .get(1)
            .is_some_and(|v| past_form(v) || matches!(*v, "was" | "were" | "had" | "ran"))
}

/// What `still` keeps that is good beside [`STILL_GOOD`] ([`Reads::Still`]),
/// which are no verbs [`verbish`] should read: `the API is still stable`,
/// `existing configs still load`, `the fast path is still taken` — at the
/// end of the clause or before a place or a party (`still responds for
/// existing clients`), never before how (`still responds with errors`).
const STILL_HOLDS: &[&str] = &[
    "stable",
    "responds",
    "respond",
    "load",
    "taken",
    "served",
    "serves",
    "reachable",
    "usable",
    "readable",
    "unchanged",
    "byte-identical",
    "identical",
    "restore",
    "restores",
    "resume",
    "resumes",
    "reconnect",
    "reconnects",
];

/// How a [`STILL_HOLDS`] word may go on and still hold ([`Reads::Still`]):
/// `existing configs still load unchanged`.
const STILL_MANNER: &[&str] = &["unchanged", "fine", "correctly", "normally", "identically"];

/// Whether `w` (after `still responds with`) is a success status
/// ([`Reads::Still`]): `with a 200`, `with 204`, `with ok`.
fn ok_status(w: &str) -> bool {
    matches!(w, "ok" | "success")
        || (w.len() == 3 && w.starts_with('2') && w.bytes().all(|b| b.is_ascii_digit()))
}

/// The present verbs a report says what is with ([`verbish`]): `only the new
/// one exists`.
const MORE_VERBS: &[&str] = &[
    "exists", "exist", "matters", "counts", "differs", "changes", "change", "uses", "use", "calls",
    "touches", "stays", "stay", "lives", "live", "took", "brought", "carried",
];

/// The words ending in `ed` that are no past form ([`past_form`]).
const NOT_PAST: &[&str] = &[
    "need", "embed", "speed", "feed", "seed", "proceed", "succeed", "exceed", "indeed", "hundred",
    "sacred", "naked", "wicked", "kindred",
];

/// Whether `w` is a past form (`changed`, `parked`): longer than three
/// letters, ending in `ed`, and none of [`NOT_PAST`] (`need`, `speed`).
fn past_form(w: &str) -> bool {
    w.len() > 3 && w.ends_with("ed") && !NOT_PAST.contains(&w)
}

/// What a state of the work may name that was given up for a better one
/// ([`Reads::State`]): `abandoned the first approach`.
const ATTEMPTS: &[&str] = &[
    "approach",
    "attempt",
    "attempts",
    "idea",
    "plan",
    "design",
    "experiment",
    "try",
    "prototype",
];

/// The words before a state that make it something stopped
/// ([`Reads::State`]): `stops skipping frames`.
const STOPS: &[&str] = &[
    "stops", "stop", "stopped", "avoids", "avoid", "avoided", "prevents", "prevent",
];

/// Whether `w` counts what is left before `left` ([`Reads::Count`]): a
/// count above zero, or a quantity (`a few`, `some`, `several`).
fn counted(w: &str) -> bool {
    count(w).is_some_and(|n| n > 0)
        || matches!(
            w,
            "a" | "an" | "few" | "some" | "several" | "more" | "many" | "couple" | "only" | "last"
        )
}

/// The words before `after` or `once` that make it no work put off
/// ([`Reads::Elliptic`]): `right after the rebase`, `soon after`.
const LEADS: &[&str] = &[
    "even", "right", "just", "only", "soon", "long", "shortly", "day", "week", "shortly",
];

/// What may follow a missing thing ([`Reads::Absent`]): `no test for the
/// error path`, `no docs yet`.
const GAP_PREPOSITIONS: &[&str] = &[
    "for", "on", "of", "in", "at", "around", "about", "covering", "against", "under", "yet",
];

/// What nobody did that a landing owes ([`Reads::Nobody`]).
const CHECKED: &[&str] = &[
    "run",
    "ran",
    "tested",
    "test",
    "reviewed",
    "review",
    "checked",
    "check",
    "verified",
    "verify",
    "tried",
    "try",
    "looked",
    "measured",
    "confirmed",
    "approved",
    "signed",
    "merged",
    "pushed",
    "landed",
    "released",
    "published",
];

/// The wait verbs among [`MECHANISMS`] ([`waits_on_work`]).
const WAITS: &[&str] = &["waits", "wait", "waiting"];

/// What a report names as work that waits (never code that waits):
/// `the docs wait until the API settles` ([`Reads::Until`]).
const WORK_ITEMS: &[&str] = &[
    "docs",
    "doc",
    "documentation",
    "pr",
    "prs",
    "release",
    "changelog",
    "migration",
    "tag",
    "readme",
    "port",
    "upgrade",
    "cleanup",
    "rest",
    "remainder",
    "rollout",
    "deploy",
];

/// Whether a wait verb in `before` has a work item for its subject: work
/// put off, not a mechanism.
fn waits_on_work(before: &[&str]) -> bool {
    before
        .iter()
        .take_while(|w| !WAITS.contains(w))
        .any(|w| WORK_ITEMS.contains(w))
}

/// Whether `I will` ends the reply with no work meant ([`Reads::Intent`]):
/// the whole rest of its sentence (`rest`) is a sign-off — `stop` or `end`,
/// then at most `here`, `now` or `there` (`I'll stop here.`), `leave it
/// there`, or `wrap up` here or there — and the sentence ends its line
/// (`last`). Anything more is work meant or handed on (`I'll stop here and
/// push`, `I'll stop here for today`, `I'll stop here; you can tag it`), and
/// `pause`, `stand by` and `wrap up` with anything else leave work to come
/// back to (`I'll pause here`, `I'll wrap up the docs`, `I'll wrap up now`,
/// `I'll stand by for your review`). A reason after the sign-off is read for
/// a place on the remote main, undenied (`I'll stop here, since everything
/// is on origin/main`).
fn ends_intent(rest: &str, last: bool) -> bool {
    let words: Vec<&str> = rest
        .split_whitespace()
        .map(bare)
        .filter(|w| !w.is_empty())
        .collect();
    last && match words.as_slice() {
        ["stop" | "end"] => true,
        ["stop" | "end", w] => matches!(*w, "here" | "now" | "there"),
        // A reason that lands it all (`I'll stop here, since everything is
        // on origin/main`).
        [
            "stop" | "end",
            "here" | "now" | "there",
            "since" | "as" | "because",
            reason @ ..,
        ] => {
            (0..reason.len()).any(|i| onto_main(&reason[i..]))
                && !reason
                    .iter()
                    .any(|w| negation(w) || UNSETTLED.contains(w) || DEFICITS.contains(w))
        }
        ["leave", "it" | "things", "there"] | ["leave", "it", "at", "that"] => true,
        ["wrap", "up", w] | ["wrap", "it" | "things", "up", w] => matches!(*w, "here" | "there"),
        _ => false,
    }
}

/// The words a checklist line's item may be when it says none is left
/// ([`nil_item`]): `Uncommitted changes: none`, `Skipped tests: 0`, `Stash:
/// empty`.
const NIL: &[&str] = &["none", "nothing", "n/a", "nil", "zero", "0", "empty", "no"];

/// What may follow a [`NIL`] item and say no more (`none left`, `0 found`,
/// `nothing at all`). Never a word that keeps something elsewhere or to come
/// (`none here`, `none yet`, `none on my side`): such an item reads open.
const NIL_TAIL: &[&str] = &[
    "left",
    "remaining",
    "found",
    "open",
    "pending",
    "outstanding",
    "at",
    "all",
    "whatsoever",
];

/// Whether `item` (an item's words, its end punctuation aside) is a [`NIL`]
/// word with nothing after it but [`NIL_TAIL`] words: `none`, `0`, `nothing
/// at all`, never `0 new, 3 old` or `none in this repo`.
fn nil_words(item: &str) -> bool {
    let mut words = item.split_whitespace().map(bare).filter(|w| !w.is_empty());
    words.next().is_some_and(|w| NIL.contains(&w)) && words.all(|w| NIL_TAIL.contains(&w))
}

/// Whether the row `[at, end)` heads a checklist line whose item says none
/// is left: at most one word before it in its clause and two between it
/// and a colon, and after the colon, to the end of the sentence, a [`NIL`]
/// item ([`nil_words`]: `Open PRs: none`, `Failing tests: 0`), or a list of
/// them each labelled (`Failing tests: 0, skipped tests: 0`). Anything else
/// after the item keeps the row open (`Open PRs: 0 here (1 upstream)`,
/// `Uncommitted changes: none, but 2 untracked files`, `Unpushed commits: 0 —
/// 2 on fix/x`), and `Known gap: no tests for the error path` names a thing.
fn nil_item(s: &str, at: usize, end: usize) -> bool {
    if s[clause_start(s, at)..at].split_whitespace().count() > 1 {
        return false;
    }
    // A colon, or a spaced dash as a list writes one (`Open PRs — none`).
    let colon = s[end..].find(':').map(|c| (end + c, 1));
    let dash = [" — ", " – "]
        .iter()
        .filter_map(|d| s[end..].find(d).map(|c| (end + c, d.len())))
        .min();
    let Some((colon, mark)) = [colon, dash].into_iter().flatten().min() else {
        return false;
    };
    let label = &s[end..colon];
    if label.contains(CLAUSE_MARKS) || label.split_whitespace().count() > 2 {
        return false;
    }
    nil_list(&s[colon + mark..])
}

/// Whether `item` says there is none and no more ([`nil_words`]), alone or
/// with more labelled nones after commas (`none`, `0 left, open PRs: 0`) —
/// never `none, but the docs need a pass` nor `0, FIXMEs: 2` — or a dash. A
/// bracketed aside at its end is read on its own ([`nil_aside`]).
fn nil_list(item: &str) -> bool {
    if nil_aside(item).is_some_and(|(_, inner)| placed_count(inner)) {
        return false;
    }
    let item = nil_aside(item).map_or(item, |(nil, _)| nil);
    let item = item.trim().trim_end_matches(['.', ';', '!', ' ']);
    if matches!(item, "-" | "—" | "–") {
        // A dash for none, as a list or a table writes it (`Open PRs: —`).
        return true;
    }
    let parts: Vec<&str> = item.split([',', ';']).collect();
    // `none added` says nothing of what was there: only with a none left
    // after it (`FIXMEs: none added, none left`).
    let fresh = |part: &str| {
        let words: Vec<&str> = part
            .split_whitespace()
            .map(bare)
            .filter(|w| !w.is_empty())
            .collect();
        matches!(words.as_slice(), [n, rest @ ..] if NIL.contains(n)
            && !rest.is_empty()
            && rest.iter().all(|w| matches!(*w, "added" | "new" | "introduced")))
    };
    let labelled = |part: &str| {
        let split = part
            .split_once(':')
            .or_else(|| part.split_once(" — "))
            .or_else(|| part.split_once(" – "));
        match split {
            Some((label, value)) => {
                label.split_whitespace().count() <= 3
                    && !label.contains(CLAUSE_MARKS)
                    && nil_words(value)
            }
            None => false,
        }
    };
    let first = parts[0];
    (nil_words(first) || (fresh(first) && parts[1..].iter().any(|p| nil_words(p))))
        && parts[1..]
            .iter()
            .all(|part| labelled(part) || nil_words(part) || fresh(part))
}

/// An item and the bracketed aside it ends with ([`nil_list`]): `none
/// (`git status` clean)` is `none` with its evidence.
fn nil_aside(item: &str) -> Option<(&str, &str)> {
    let t = item.trim().trim_end_matches(['.', ';', '!', ' ']);
    t.strip_suffix(')')?.split_once('(')
}

/// Whether the aside of a none (`inner`) counts some kept elsewhere
/// ([`nil_list`], [`aside_status`]): a count above zero alone or before a
/// place or a state ([`ELSEWHERE`]) or a change (`none (1 upstream)`, `0 (2
/// on my fork)`), never one of evidence (`none (3 reruns green)`).
fn placed_count(inner: &str) -> bool {
    let words: Vec<&str> = inner
        .split_whitespace()
        .map(bare)
        .filter(|w| !w.is_empty())
        .collect();
    matches!(words.as_slice(), [n, rest @ ..]
        if (count(n).is_some_and(|c| c > 0) || matches!(*n, "some" | "few"))
            && rest.first().is_none_or(|w| ELSEWHERE.contains(w) || CHANGE_NOUNS.contains(w)))
}

/// Where a count beside a none keeps some ([`placed_count`]): `none (1
/// upstream)`, `0 (2 on my fork)`, never `none (3 reruns green)`.
const ELSEWHERE: &[&str] = &[
    "upstream",
    "elsewhere",
    "on",
    "in",
    "open",
    "pending",
    "left",
    "remaining",
    "outstanding",
    "unmerged",
    "unpushed",
    "of",
    "more",
    "still",
    "here",
    "there",
];

/// What Land reads in the aside of a head's item that is none
/// ([`nil_aside`], [`land_status`]): `Remaining: none (the docs are pending)`
/// is still open.
fn aside_status(item: &str, families: &[LandFamily], said_main: bool) -> Option<&'static str> {
    let (nil, inner) = nil_aside(item)?;
    if !nil_list(nil) {
        return None;
    }
    // A count beside the none is some left elsewhere (`Open PRs: none (1
    // upstream)`).
    if placed_count(inner) {
        return Some(COUNT_BESIDE_NONE);
    }
    land_status(inner, inner.len(), "", families, said_main)
}

/// WHETHER THE ROW STANDS IN A FAULT A FIX REMOVED, beyond [`described`]
/// (`before` it in its clause): a relative clause in the past after a fixing
/// verb and its object (`fixed the bug where tests failed on Windows`,
/// `removed the hack that disabled the test`), never one in the present
/// (`fixed the typo where the PR is still pending`).
///
/// FAIL TOWARD OPEN: nothing else puts a row behind. Not a date (`CI was
/// failing earlier` says nothing of now; `the docs were deferred earlier` is
/// a deferral still standing), not a later clause (`PR #12 was pending
/// review; it is merged now`, `the docs were deferred; the fix is merged
/// now`: another clause may be about another thing), not the task as given
/// (`you asked me to fix the flaky test; the fix is on origin/main`). Each
/// reads open, a deliberate non-landing: the nudge names the word, and the
/// worker restates the settled thing in its own clause (`the PR that was
/// pending review is merged`).
fn behind(before: &[&str]) -> bool {
    before.iter().enumerate().any(|(r, w)| {
        matches!(*w, "where" | "which" | "that" | "when" | "whereby")
            && before[r.saturating_sub(4)..r]
                .iter()
                .any(|v| FIXERS.contains(v))
            && !before[r + 1..].iter().any(|v| PRESENT.contains(v))
    })
}

/// The present finite verbs that keep a relative clause's state standing
/// ([`behind`]): `the typo where the PR is still pending`.
const PRESENT: &[&str] = &[
    "is", "are", "am", "has", "have", "remains", "remain", "stays", "stay", "will", "can", "does",
    "do",
];

/// What fixes a fault named after it ([`behind`]).
const FIXERS: &[&str] = &[
    "fixed",
    "resolved",
    "removed",
    "addressed",
    "closed",
    "eliminated",
    "dropped",
    "deleted",
    "retired",
    "replaced",
    "reverted",
    "rewrote",
    "rewritten",
    "solved",
];

/// What a worker did not do that is harm avoided, no work left
/// ([`Reads::NotDone`]): `did not change the API`, `have not touched the
/// config`, `did not start new work`.
const AVOIDED: &[&str] = &[
    "change",
    "changed",
    "touch",
    "touched",
    "break",
    "broken",
    "broke",
    "alter",
    "altered",
    "modify",
    "modified",
    "affect",
    "affected",
    "need",
    "needed",
    "regress",
    "regressed",
    "fail",
    "failed",
    "introduce",
    "introduced",
    "start",
    "started",
    "expand",
    "expanded",
    "widen",
    "widened",
    "force-push",
    "force-pushed",
    "delete",
    "deleted",
    "leave",
    "left",
    "find",
    "found",
    "see",
    "seen",
    "hit",
    "notice",
    "noticed",
    "add",
    "added",
];

/// What a measured thing did not do, an observation, no work left
/// ([`Reads::NotDone`]) — unless the worker is its subject: `the first
/// prompt did not move` lands, `I did not move the tests` does not.
const STEADY: &[&str] = &[
    "move",
    "moved",
    "budge",
    "budged",
    "shift",
    "shifted",
    "grow",
    "grew",
    "degrade",
    "degraded",
    "crash",
    "crashed",
    "hang",
    "hung",
    "leak",
    "leaked",
    "recur",
    "recurred",
    "reappear",
    "reappeared",
];

/// What should work and no longer does ([`Reads::Lost`]).
const WORKS: &[&str] = &[
    "compile", "compiles", "build", "builds", "work", "works", "pass", "passes", "run", "runs",
    "parse", "parses", "load", "loads", "start", "starts", "handle", "handles", "cover", "covers",
    "support", "supports", "render", "renders", "apply", "applies", "match", "matches", "link",
    "links", "install", "installs", "launch", "launches",
];

/// What work could still be made ([`Reads::Better`]): `could be cleaner`.
const COMPARATIVES: &[&str] = &[
    "better", "cleaner", "faster", "simpler", "nicer", "tidier", "smaller", "safer", "clearer",
];

/// What work owes beside its landing, after `without` ([`Reads::Without`]):
/// `merged without the docs`, `shipped without tagging it`.
const OWED_WITHOUT: &[&str] = &[
    "docs",
    "doc",
    "documentation",
    "changelog",
    "readme",
    "tag",
    "tags",
    "tests",
    "test",
    "coverage",
    "release",
    "notes",
    "migration",
    "port",
    "backport",
    "updating",
    "adding",
    "writing",
    "tagging",
    "porting",
    "testing",
    "publishing",
    "pushing",
    "merging",
    "releasing",
    "documenting",
    "running",
    "rerunning",
    "verifying",
    "reviewing",
    "signing",
    "bumping",
];

/// What approval leaves unlanded ([`Reads::Approval`]): the work itself,
/// never a person's decision (`the owner approved the new dependency`).
const APPROVED_THINGS: &[&str] = &[
    "change",
    "changes",
    "fix",
    "fixes",
    "patch",
    "patches",
    "work",
    "feature",
    "features",
    "refactor",
    "rewrite",
    "migration",
    "port",
    "docs",
    "update",
    "updates",
    "diff",
    "commit",
    "commits",
    "implementation",
];

/// What `partial` names as a term of the code, not a part of the work
/// ([`land_open`]): `a partial index`, `partial application`.
const PARTIAL_TERMS: &[&str] = &[
    "index",
    "indexes",
    "indices",
    "application",
    "function",
    "functions",
    "order",
    "ordering",
    "match",
    "matches",
    "matching",
    "sum",
    "sums",
    "eq",
    "ord",
    "view",
    "views",
    "mock",
    "mocks",
    "template",
    "templates",
    "specialization",
    "key",
    "keys",
    "derivative",
    "evaluation",
    // A terminal's line with no newline yet, and a status line's own state
    // (`the Partial line`).
    "line",
    "lines",
];

/// The verbs that run a check, before `separately` ([`land_open`]): `tested
/// the parser and the lexer separately`.
const TESTED: &[&str] = &[
    "tested",
    "ran",
    "checked",
    "verified",
    "benchmarked",
    "measured",
    "built",
    "profiled",
];

/// What a concession may say a landing cost and leave nothing (`merged,
/// though it took two CI runs`, [`conceding`]): only these words, and at
/// least `took`.
const COSTS: &[&str] = &[
    "it",
    "ci",
    "the",
    "took",
    "a",
    "an",
    "few",
    "couple",
    "of",
    "several",
    "runs",
    "run",
    "tries",
    "try",
    "attempts",
    "retries",
    "reruns",
    "rounds",
    "iterations",
    "while",
    "longer",
    "than",
    "expected",
    "hours",
    "hour",
    "minutes",
    "days",
    "to",
    "go",
    "get",
    "turn",
    "green",
    "pass",
    "lane",
    "job",
    "build",
    "builds",
    "pipeline",
];

/// What claims work landed ([`conceding`]), beside [`SETTLED`].
const LANDED: &[&str] = &[
    "pushed",
    "committed",
    "main",
    "origin/main",
    "upstream/main",
    "shipped",
    "already",
];

/// Whether a concession conceded something left ([`Reads::Concession`]):
/// its line claims a landing `before` it (a [`SETTLED`] or [`LANDED`] word:
/// `merged, though …`, `everything is merged; however, …`), and what
/// follows `after` it, to the end of its sentence, claims none, denies
/// nothing and is no cost ([`COSTS`]) (`merged, though the help text needs a
/// refresh`, never `merged, but the docs were already current`, `merged, but
/// nothing else changed` or `merged, though it took two CI runs`).
fn conceding(before: &str, after: &str) -> bool {
    let claims = |text: &str| {
        text.split_whitespace()
            .map(bare)
            .any(|w| SETTLED.contains(&w) || LANDED.contains(&w))
    };
    let words: Vec<&str> = after
        .split_whitespace()
        .map(bare)
        .filter(|w| !w.is_empty())
        .collect();
    let cost = words.contains(&"took")
        && words
            .iter()
            .all(|w| COSTS.contains(w) || count(w).is_some_and(|n| n > 0));
    claims(before) && !claims(after) && !words.iter().any(|w| negation(w)) && !cost
}

/// What follows a check's state that makes it a control, no state owed
/// ([`Reads::Beside`]): `the test fails without the fix`, `CI is red when
/// the fix is reverted`, `skipped when no GPU is present`.
const CONTROLS: &[&str] = &[
    "without", "if", "when", "unless", "whenever", "against", "before",
];

/// What a thing still is that leaves it to replace ([`Reads::Still`]):
/// `the binary is still the previous build`, never `still the default`.
const OLD: &[&str] = &[
    "old", "previous", "former", "outdated", "stale", "earlier", "last",
];

/// What `still` may say lasts without leaving anything to do
/// ([`Reads::Still`]): `the old syntax still parses`, `everything still
/// passes`.
const STILL_GOOD: &[&str] = &[
    "passes",
    "pass",
    "passed",
    "passing",
    "works",
    "work",
    "working",
    "builds",
    "build",
    "compiles",
    "compile",
    "parses",
    "parse",
    "holds",
    "hold",
    "green",
    "merged",
    "landed",
    "pushed",
    "on",
    "clean",
    "runs",
    "run",
    "fine",
    "correct",
    "valid",
    "intact",
    "supported",
    "matches",
    "match",
    "reads",
    "read",
    "fits",
    "fit",
    "applies",
    "apply",
    "opens",
    "starts",
    "loads",
    "renders",
    "behaves",
    "succeeds",
    "exits",
    "returns",
    "accepts",
    "handles",
    "accepted",
    "compatible",
    "backward",
    "backwards",
    "available",
    "honoured",
    "honored",
    "respected",
    "recognised",
    "recognized",
];

/// Whether `after` a remote's name (`origin`, `upstream`) names work no
/// remote main has ([`Reads::Ref`]): one of its branches other than main
/// (`origin/feature-x`), or a range from main (`origin/main..HEAD`, which
/// lists the commits main lacks) unless its clause says it lists none
/// ([`lists_none`]: `git log origin/main..HEAD shows nothing`) — never
/// `origin/main`, nor a bare `origin`. `--no-merged` (`git branch
/// --no-merged origin/main lists fix/x`) reads as a range.
fn off_main(after: &str) -> bool {
    let Some(name) = after.strip_prefix('/') else {
        return false;
    };
    let token = name
        .split(|c: char| !(word_char(c) || c == '/' || c == '.'))
        .next()
        .unwrap_or("");
    if let Some((left, _)) = token.split_once("..") {
        return matches!(left, "main" | "master") && !lists_none(&name[token.len()..]);
    }
    let name = token.trim_end_matches('.');
    !name.is_empty() && !matches!(name, "main" | "master" | "head")
}

/// Whether what follows a range of commits, to the end of its clause, says
/// it lists none ([`off_main`]): `shows nothing`, `is empty`, `prints no
/// commits`, `returns 0`.
fn lists_none(rest: &str) -> bool {
    rest[..clause_end(rest, 0)]
        .split_whitespace()
        .map(bare)
        .any(|w| matches!(w, "nothing" | "empty" | "none" | "no" | "0" | "zero"))
}

/// What names the rest of a count ([`rest_settled`]): `the third`, `the
/// others`.
const REMAINDER: &[&str] = &[
    "the other",
    "the others",
    "the rest",
    "the remaining",
    "the last",
    "the second",
    "the third",
    "the fourth",
    "the fifth",
];

/// Whether what follows a part of a count (`after`, to the end of its line)
/// names the rest and settles it ([`Reads::Fraction`]): `two of the three
/// lanes ran here; the third ran in the cross cell and passed`.
fn rest_settled(after: &str) -> bool {
    REMAINDER.iter().any(|r| {
        after.find(r).is_some_and(|at| {
            after[at + r.len()..]
                .split_whitespace()
                .map(bare)
                .any(|w| SETTLED.contains(&w) || LANDED.contains(&w))
        })
    })
}

/// A count as a report writes one: digits, or `one` to `twelve`.
fn count(w: &str) -> Option<u32> {
    const WORDS: [&str; 12] = [
        "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven",
        "twelve",
    ];
    w.parse().ok().or_else(|| {
        WORDS
            .iter()
            .position(|n| *n == w)
            .and_then(|i| u32::try_from(i + 1).ok())
    })
}

/// Whether `of` between the words `before` it and `after` it counts part
/// of a whole ([`Reads::Fraction`]): `three of the five`, `2 of 3`, `2 out of
/// 3` (and `2/3`, which a report's words read `2 of 3`), never `all 3 of the
/// follow-ups`, `every one of the 4 PRs` or `five of five`.
fn short_count(before: &[&str], after: &str) -> bool {
    let mut words = after
        .split_whitespace()
        .map(bare)
        .skip_while(|w| *w == "the");
    let prev = match before {
        // `every one of the 4 PRs` counts them all.
        [.., "every" | "each", "one"] => None,
        [.., n, "out"] => Some(*n),
        [.., n] => Some(*n),
        [] => None,
    };
    let (Some(n), Some(m)) = (prev.and_then(count), words.next().and_then(count)) else {
        return false;
    };
    n < m
}

/// The predicates that say a part of the work is done ([`leading_part`],
/// [`only_part`]): `five are converted`, `only the macOS arm is implemented`.
const PART_DONE: &[&str] = &[
    "done",
    "converted",
    "ported",
    "migrated",
    "implemented",
    "finished",
    "complete",
    "completed",
    "wired",
    "covered",
    "handled",
    "supported",
    "fixed",
    "merged",
    "landed",
    "written",
];

/// Whether a sentence that opens with `of` counts a whole and then a part of
/// it done ([`Reads::Fraction`]): `of the six call sites, five are
/// converted`. `before` is empty (the sentence opens there) and `after` is the
/// sentence past `of`.
fn leading_part(before: &[&str], after: &str) -> bool {
    let Some((whole, part)) = after.split_once(',') else {
        return false;
    };
    let whole = whole
        .split_whitespace()
        .map(bare)
        .find(|w| *w != "the" && *w != "all")
        .and_then(count);
    let part: Vec<&str> = part.split_whitespace().map(bare).collect();
    before.is_empty()
        && whole
            .zip(part.first().and_then(|w| count(w)))
            .is_some_and(|(m, n)| n < m)
        && only_part(&part[1..])
}

/// Whether a clause names its subject's state as a part done ([`leading_part`],
/// and the sentence that opens with `only`: `only the macOS arm is
/// implemented`): an [`AUX`] word, then one of [`PART_DONE`].
fn only_part(after: &[&str]) -> bool {
    after
        .windows(2)
        .take(8)
        .any(|w| AUX.contains(&w[0]) && PART_DONE.contains(&w[1]))
}

/// The marks that end a clause ([`land_open`]), as [`open_status`] reads
/// them.
const CLAUSE_MARKS: [char; 7] = [',', ';', ':', '(', ')', '—', '–'];

/// Whether the character `c` at `i` in `s` ends a clause ([`CLAUSE_MARKS`]):
/// never a number's thousands comma (`4,997`) nor a colon of a path
/// (`hooks::tests::every_hook`), which stand inside one word.
fn clause_mark(s: &str, i: usize, c: char) -> bool {
    let (before, after) = (&s[..i], &s[i + c.len_utf8()..]);
    CLAUSE_MARKS.contains(&c)
        && !(c == ','
            && before.ends_with(|d: char| d.is_ascii_digit())
            && after.starts_with(|d: char| d.is_ascii_digit()))
        && !(c == ':' && (before.ends_with(':') || after.starts_with(':')))
}

/// Where the clause holding `at` begins in `s`: past the last
/// [`CLAUSE_MARKS`] before it ([`clause_mark`]).
fn clause_start(s: &str, at: usize) -> usize {
    s[..at]
        .char_indices()
        .rev()
        .find(|&(i, c)| clause_mark(s, i, c))
        .map_or(0, |(i, c)| i + c.len_utf8())
}

/// Where the clause running on from `from` ends in `s`: at its next
/// [`CLAUSE_MARKS`] ([`clause_mark`]), or the sentence's end.
fn clause_end(s: &str, from: usize) -> usize {
    s[from..]
        .char_indices()
        .find(|&(i, c)| clause_mark(s, from + i, c))
        .map_or(s.len(), |(i, _)| from + i)
}

/// A word of a report without the punctuation around it (`#412` reads
/// `412`, `owner's` stays).
fn bare(w: &str) -> &str {
    w.trim_matches(|c: char| !(c.is_alphanumeric() || matches!(c, '\'' | '-' | '_' | '/')))
}

/// The words of `s[a..b]` and where each begins (a report's words are one
/// space apart, [`ReportLine::of`]).
fn words_at(s: &str, a: usize, b: usize) -> impl Iterator<Item = (usize, &str)> {
    let mut at = a;
    s[a..b]
        .split(' ')
        .map(move |w| {
            let p = at;
            at += w.len() + 1;
            (p, w)
        })
        .filter(|(_, w)| !w.is_empty())
}

/// Whether the row at `[at, end)` is part of a file's name or path
/// (`TODO.md`, `docs/todo`): a name, not a report of anything.
fn in_path(s: &str, at: usize, end: usize) -> bool {
    s[..at].ends_with(['/', '.'])
        || s[end..]
            .strip_prefix('.')
            .is_some_and(|r| r.starts_with(char::is_alphanumeric))
}

/// The words directly before a leftover word that make it a past state
/// ([`described`]): `the previously skipped test now passes`.
const PAST_MARKERS: &[&str] = &[
    "previously",
    "formerly",
    "once",
    "earlier",
    "initially",
    "originally",
];

/// Whether the words `before` a row (in its clause) make it a past state
/// that the rest of the sentence reports on: after a [`PAST_MARKERS`] word,
/// or in a relative clause in the past (`the test that was waiting for review
/// feedback is fixed`, `which had been skipped`). What the main clause then
/// says is read on its own words.
fn described(before: &[&str]) -> bool {
    let back = |k: usize| {
        before
            .len()
            .checked_sub(k)
            .and_then(|i| before.get(i))
            .copied()
    };
    let relative = |k: usize| back(k).is_some_and(|w| matches!(w, "that" | "which" | "who"));
    back(1).is_some_and(|w| PAST_MARKERS.contains(&w))
        || (matches!(back(1), Some("was" | "were" | "had")) && relative(2))
        // What someone said was so (`the case the item said was missing`).
        || (matches!(back(1), Some("was" | "were")) && back(2) == Some("said"))
        || (back(1) == Some("been") && back(2) == Some("had") && relative(3))
}

/// The words that make a following word a thing's name, not its state
/// ([`settled`]): `the pending PR`, `all 3 follow-ups`, `a later commit`.
const DETERMINERS: &[&str] = &[
    "the", "a", "an", "this", "that", "these", "those", "each", "every", "all", "both", "its",
    "my", "our", "their", "his", "her", "your", "any", "some", "one", "two", "three", "four",
    "five", "six", "seven", "eight", "nine", "ten",
];

/// The words before a settling word that leave it undone ([`settled`]):
/// `ready to be merged`, `will be fixed`.
const UNSETTLED: &[&str] = &[
    "to", "be", "will", "would", "can", "could", "should", "must", "may", "might", "shall",
    "needs", "need", "not",
];

/// The words before a settling word that put it in the past, whatever
/// condition follows ([`settled`]): `was merged when CI passed`.
const PAST_AUX: &[&str] = &["was", "were", "got", "had", "been"];

/// The words after a settling word that make it a condition, not a fact
/// ([`settled`]): `is merged once CI passes`.
const CONDITIONS: &[&str] = &["once", "when", "if", "unless", "until"];

/// The words a predicate passes over before the word that settles it
/// ([`stretch`], [`settled`]): `opened and then merged`, `is done and has
/// since landed`.
const AUX: &[&str] = &[
    "is", "was", "are", "were", "has", "have", "had", "been", "got", "now", "then", "since",
    "also", "finally", "already", "just", "all", "both",
];

/// The words that end the stretch of a clause a row's nouns and settling
/// words are read in ([`stretch`]).
const CUTS: &[&str] = &[
    "and", "but", "while", "whereas", "though", "although", "so", "yet", "until", "unless",
    "before", "because",
];

/// The stretch of the clause `[from, to)` that holds the row at `span`: cut
/// at each [`CUTS`] word, save one after the row that goes on with the same
/// predicate to a word of `by` (`the PR was opened and merged`, `all 3
/// follow-ups are done and merged`).
fn stretch(s: &str, from: usize, to: usize, span: (usize, usize), by: &[&str]) -> (usize, usize) {
    let (mut a, mut b) = (from, to);
    for (p, w) in words_at(s, from, to) {
        if !CUTS.contains(&bare(w)) {
            continue;
        }
        let past = p + w.len();
        if past <= span.0 {
            a = past;
        } else if p >= span.1 && !continues(&s[past..to], by) {
            b = p;
            break;
        }
    }
    (a, b)
}

/// Whether `rest` goes on, past [`AUX`] words, to settle what it continues
/// ([`settles_at`]).
fn continues(rest: &str, by: &[&str]) -> bool {
    let words: Vec<&str> = rest.split_whitespace().map(bare).collect();
    settles_at(&words, by, true)
}

/// The verbs that carry work onto a branch ([`settles_at`]): `committed and
/// pushed to origin/main`. Never `rebased` or `squashed`: a branch rebased
/// onto origin/main is still a branch.
const LANDING_VERBS: &[&str] = &[
    "pushed",
    "committed",
    "landed",
    "merged",
    "shipped",
    "squash-merged",
    "rebase-merged",
];

/// Whether `words`, past [`AUX`] words, settle what they follow: a word of
/// `by` (`is merged`), or, for a landing leftover (`place`), a place on the
/// remote main branch right there or after a verb that carries work there
/// or keeps it there ([`onto_main`]: `is on remote main`, `are pushed to
/// origin/main`, `now lives on origin/main`, `took it into origin/main`) —
/// never after another word (`is now nearly on remote main`, `is based on
/// origin/main`). `pushed` alone is none: a branch can be pushed and still
/// not landed.
fn settles_at(words: &[&str], by: &[&str], place: bool) -> bool {
    let rest: Vec<&str> = words
        .iter()
        .copied()
        .skip_while(|w| AUX.contains(w))
        .collect();
    rest.first().is_some_and(|w| by.contains(w))
        || (place
            && (onto_main(&rest)
                || (rest.first().is_some_and(|w| {
                    LANDING_VERBS.contains(w) || matches!(*w, "lives" | "sits" | "went")
                }) && onto_main(&rest[1..]))
                || (rest.first().is_some_and(|w| {
                    LANDING_VERBS.contains(w) || matches!(*w, "took" | "brought" | "carried")
                }) && rest.get(1).is_some_and(|o| matches!(*o, "it" | "them"))
                    && onto_main(&rest[2..]))))
}

/// Whether `words` open with a place on the remote main branch: `on`, `to`,
/// `into`, `onto` or `in` before main (`on remote main`, `to origin/main`,
/// `into the remote main branch`, `on origin main`), or `matches`, `equals`,
/// `at` or `with` before the remote's main (`matches origin/main`, `in sync
/// with origin/main`).
fn onto_main(words: &[&str]) -> bool {
    let Some((&first, rest)) = words.split_first() else {
        return false;
    };
    let lead = rest
        .iter()
        .take_while(|w| matches!(**w, "the" | "remote"))
        .count();
    let remote = rest[..lead].contains(&"remote");
    let mut names = rest[lead..].iter().copied();
    let remote_main = |n: &str| {
        matches!(
            n,
            "origin/main" | "origin/master" | "upstream/main" | "upstream/master"
        )
    };
    match (first, names.next()) {
        ("on" | "to" | "into" | "onto" | "in", Some(n)) => {
            // A repository's main by its owner and name (`to
            // example-org/codex-alab main`).
            let repo = n.split_once('/').is_some_and(|(owner, name)| {
                !owner.is_empty() && !name.is_empty() && !name.contains(['/', '.'])
            });
            remote_main(n)
                || matches!(n, "main" | "master" | "trunk")
                || ((matches!(n, "origin" | "upstream") || repo)
                    && names.next().is_some_and(|m| matches!(m, "main" | "master")))
        }
        ("matches" | "match" | "equals" | "at" | "with", Some(n)) => {
            remote_main(n) || (remote && matches!(n, "main" | "master"))
        }
        _ => false,
    }
}

/// What main does when a sentence says it has the work ([`main_claimed`]).
const HOLDS: &[&str] = &["has", "contains", "includes", "carries", "got", "holds"];

/// Whether the sentence `s` also says main has the work, undenied: a place on
/// the remote main branch ([`onto_main`]) a landing verb carries the work to
/// (`and merged into main`, `merged fix/resize-race into main`: the verb at
/// most three words before, no other verb between) or that the work is on
/// (`is on origin/main`: a finite word right before), or main holding it
/// (`main has the fix`). A backport merged to a release branch is no leftover
/// where main has the fix; `not to main` claims nothing, and neither does a
/// place no landing reaches (`with a PR into origin/main`, `rebased onto
/// origin/main`, `in sync with origin/main`, `based on origin/main`).
fn main_claimed(s: &str) -> bool {
    let words: Vec<&str> = s.split_whitespace().map(bare).collect();
    (0..words.len()).any(|i| {
        let undenied = i
            .checked_sub(1)
            .and_then(|j| words.get(j))
            .is_none_or(|w| *w != "not" && !negation(w));
        let carried = || {
            let prev = i.checked_sub(1).and_then(|j| words.get(j)).copied();
            let back = &words[i.saturating_sub(3)..i];
            // A landing still to come claims nothing (`to be merged into
            // origin/main`, `will merge into main`), nor a change aimed there
            // (`has a PR into origin/main`).
            if back
                .iter()
                .any(|w| UNSETTLED.contains(w) || CHANGE_NOUNS.contains(w) || *w == "date")
            {
                return false;
            }
            prev.is_some_and(|p| AUX.contains(&p) && !matches!(p, "all" | "both"))
                || back
                    .iter()
                    .rev()
                    .take_while(|w| LANDING_VERBS.contains(w) || !verbish(w))
                    .any(|w| LANDING_VERBS.contains(w) || *w == "went")
        };
        undenied
            && ((onto_main(&words[i..]) && carried())
                || (matches!(
                    words[i],
                    "main" | "origin/main" | "upstream/main" | "master"
                ) && words.get(i + 1).is_some_and(|w| HOLDS.contains(w))))
    })
}

/// The names before a branch that a push or a merge names no branch by
/// ([`onto_branch`]): `pushed to origin`, `to the remote`.
const REMOTES: &[&str] = &[
    "the", "remote", "origin", "upstream", "github", "gitlab", "our",
];

/// The names of the main branch ([`onto_branch`]).
const MAINS: &[&str] = &[
    "main",
    "master",
    "trunk",
    "head",
    "default",
    "origin/main",
    "origin/master",
    "upstream/main",
    "upstream/master",
    "origin/head",
];

/// The branch names a repository integrates on beside main ([`onto_branch`]).
const BRANCHES: &[&str] = &[
    "develop",
    "development",
    "dev",
    "staging",
    "next",
    "beta",
    "stable",
    "release",
    "integration",
];

/// Whether what follows a push or a merge (`after`, the rest of its clause)
/// carries the work to a branch other than main ([`Reads::Onto`]): `to`,
/// `into` or `onto` within six words, past a remote's name ([`REMOTES`]),
/// before a branch's name — one with a slash (`fix/resize-race`,
/// `release/1.2`), a dash or an underscore, a known integration branch
/// ([`BRANCHES`]: `merged into develop`), or any word before `branch`. Never
/// main ([`MAINS`]), a count or a determiner (`merged the two helpers into
/// one`), a file (`parse.rs`, `crates.io`), nor a plain word (`pushed the
/// fix to unblock CI`).
fn onto_branch(after: &[&str]) -> bool {
    let Some(k) = after
        .iter()
        .take(6)
        .position(|w| matches!(*w, "to" | "into" | "onto"))
    else {
        return false;
    };
    let mut names = after[k + 1..]
        .iter()
        .copied()
        .skip_while(|w| REMOTES.contains(w));
    let Some(name) = names.next() else {
        return false;
    };
    let then = names.next();
    let file = name
        .rsplit_once('.')
        .is_some_and(|(_, ext)| !ext.is_empty() && ext.bytes().all(|b| b.is_ascii_alphabetic()));
    if MAINS.contains(&name)
        || DETERMINERS.contains(&name)
        || count(name).is_some()
        || file
        || !name.starts_with(char::is_alphabetic)
    {
        return false;
    }
    if then.is_some_and(|w| matches!(w, "main" | "master")) {
        // An integration branch's own main (`pushed to the staging main`)
        // is off main; a repository's is main (`pushed to
        // example-org/codex-alab main`).
        return BRANCHES.contains(&name);
    }
    then == Some("branch")
        || name.contains(['/', '-', '_'])
        || main_grown(name)
        || (BRANCHES.contains(&name) && then.is_none_or(|w| JOINS.contains(&w)))
}

/// Whether `name` is a branch named from main but other than it ([`onto_branch`],
/// [`on_branch`]): `maint`, `mainline`, `main-next`, `master-old`, never
/// `mainly` nor `maintainer`.
fn main_grown(name: &str) -> bool {
    ["main", "master"].iter().any(|m| {
        name.strip_prefix(m).is_some_and(|rest| {
            !rest.is_empty()
                && !matches!(rest, "ly" | "s")
                && !rest.starts_with("tain")
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        })
    })
}

/// The things that may be dropped or reverted with nothing lost, as the head
/// of what is dropped ([`disposable`]): a stopgap, an attempt given up for a
/// better one, a copy, noise (`the workaround was dropped`, `reverted the
/// hotfix`).
const DISPOSABLE: &[&str] = &[
    "workaround",
    "workarounds",
    "hack",
    "hacks",
    "stopgap",
    "stopgaps",
    "stop-gap",
    "band-aid",
    "shim",
    "shims",
    "kludge",
    "approach",
    "attempt",
    "attempts",
    "idea",
    "experiment",
    "experiments",
    "prototype",
    "spike",
    "noise",
    "churn",
    "hotfix",
    "hotfixes",
    "copy",
    "copies",
    "duplicate",
    "duplicates",
];

/// The words right before the head of what is dropped that make it
/// disposable ([`disposable`]): an old, dead, bad or stray thing (`reverted
/// the bad commit`, `the stray debug commit was dropped`). Never a word that
/// names what the thing is about (`the regression test`, `the dependency
/// bump`, `Windows support`) nor when it was (`the previous fix`).
const DISPOSABLE_MODS: &[&str] = &[
    "temporary",
    "interim",
    "hacky",
    "old",
    "outdated",
    "stale",
    "dead",
    "unused",
    "obsolete",
    "legacy",
    "deprecated",
    "redundant",
    "duplicate",
    "duplicated",
    "bad",
    "broken",
    "faulty",
    "buggy",
    "offending",
    "wrong",
    "debug",
    "accidental",
    "unrelated",
    "stray",
    "unintended",
    "bogus",
    "spurious",
    "vendored",
];

/// The rows of [`Reads::Dropped`] that cut a thing, which a release, a tag
/// or a branch takes as its making (`the release was cut from origin/main`).
const CUT_ROWS: &[&str] = &["was cut", "were cut", "got cut", "been cut", "cut from"];

/// What a cut makes ([`CUT_ROWS`]).
const CUT_THINGS: &[&str] = &[
    "release",
    "releases",
    "tag",
    "tags",
    "rc",
    "branch",
    "version",
    "build",
    "candidate",
];

/// WHETHER WHAT A [`Reads::Dropped`] ROW (`word`) DROPS IS DISPOSABLE, read
/// from the row's own noun phrase only: the subject before a passive row
/// (`the workaround was dropped`), from past the clause's last verb, or the
/// object after an active one (`reverted the hotfix`), up to a preposition
/// or a verb. Each of its coordinated things (split at `and`, `or`, `plus`)
/// must be disposable — its head (its last word) one of [`DISPOSABLE`], or
/// the word right before the head one of [`DISPOSABLE_MODS`] — so `the
/// workaround and the docs were dropped` and `reverted the hotfix and the
/// feature` read open. A revert landed again (`reverted and re-landed`) is
/// disposable whatever it reverted.
fn disposable(word: &str, before: &[&str], after: &[&str]) -> bool {
    if after.iter().take(4).any(|w| {
        matches!(
            *w,
            "relanded" | "re-landed" | "remerged" | "re-merged" | "reapplied" | "re-applied"
        )
    }) {
        return true;
    }
    let passive = word.split(' ').next().is_some_and(|w| AUX.contains(&w))
        || CUT_ROWS.contains(&word)
        || before
            .last()
            .is_some_and(|w| matches!(*w, "was" | "were" | "is" | "are" | "got" | "been" | "be"));
    let np: Vec<&str> = if passive {
        let subject = before
            .iter()
            .rposition(|w| verbish(w) && !AUX.contains(w))
            .map_or(before, |i| &before[i + 1..]);
        subject
            .iter()
            .copied()
            .filter(|w| !AUX.contains(w))
            .collect()
    } else {
        after
            .iter()
            .copied()
            .take_while(|w| !PREPOSITIONS.contains(w) && !CUTS.contains(w) || *w == "and")
            .collect()
    };
    let mut things: Vec<Vec<&str>> = vec![Vec::new()];
    for w in np {
        if matches!(w, "and" | "or" | "plus") {
            things.push(Vec::new());
        } else if let Some(thing) = things.last_mut() {
            thing.push(w);
        }
    }
    let things: Vec<Vec<&str>> = things
        .into_iter()
        .map(|t| {
            // A thing ends at its first preposition (`the stash from the
            // experiment`); a verb opens another clause (`and re-landed the
            // fix`).
            t.into_iter()
                .take_while(|w| !PREPOSITIONS.contains(w) && *w != "of")
                .filter(|w| !DETERMINERS.contains(w) && count(w).is_none())
                .collect::<Vec<&str>>()
        })
        .filter(|t| !t.is_empty())
        .take_while(|t| {
            !t.first()
                .is_some_and(|w| verbish(w) && !DISPOSABLE_MODS.contains(w))
        })
        .collect();
    !things.is_empty()
        && things.iter().all(|t| {
            let head = t[t.len() - 1];
            let modifier = t.len().checked_sub(2).map(|i| t[i]);
            DISPOSABLE.contains(&head)
                || modifier.is_some_and(|m| DISPOSABLE_MODS.contains(&m))
                || (CUT_ROWS.contains(&word) && CUT_THINGS.contains(&head))
        })
}

/// The prefixes a branch name opens with before its slash ([`on_branch`]):
/// `fix/x`, `feat/docs`, `release/1.2`.
const BRANCH_PREFIXES: &[&str] = &[
    "fix", "feat", "feature", "features", "bugfix", "hotfix", "release", "releases", "chore",
    "refactor", "wip", "topic", "exp", "dev", "docs", "test", "tmp",
];

/// Whether `name` reads as a branch's name ([`on_branch`]): a prefix and a
/// slash, then a name (`fix/resize-race`, `b8/strict-read`, `release/1.2`) —
/// a known prefix ([`BRANCH_PREFIXES`]), a short code (`b8`), or a dashed or
/// underscored name after it; never `linux/macos`, a path to a file or a URL.
fn branch_name(name: &str) -> bool {
    let Some((prefix, rest)) = name.split_once('/') else {
        return false;
    };
    let code = prefix.len() <= 3
        && prefix.starts_with(char::is_alphabetic)
        && prefix.bytes().skip(1).all(|b| b.is_ascii_digit());
    // A short prefix before a round's name (`rp/r11`, `ab/r39`).
    let round = (1..=3).contains(&prefix.len())
        && prefix.bytes().all(|b| b.is_ascii_lowercase())
        && rest.starts_with(|c: char| c.is_ascii_lowercase())
        && rest.bytes().any(|b| b.is_ascii_digit())
        && rest.bytes().all(|b| b.is_ascii_alphanumeric());
    !rest.is_empty()
        && !name.contains("://")
        && (BRANCH_PREFIXES.contains(&prefix)
            || code
            || round
            || rest.contains(['-', '_'])
            || matches!(prefix, "origin" | "upstream"))
}

/// Whether what follows `on` (`after`, the rest of its clause) names a
/// branch other than main ([`Reads::OnBranch`]): past a remote's name
/// ([`REMOTES`]), a branch's name ([`branch_name`]: `on fix/resize-race`), a
/// integration branch (`on develop`; never `on stable`, a toolchain), or any word before
/// `branch` (`on the release branch`). Never main ([`MAINS`]: `on the main
/// branch`), a file, a count or a determiner (`on a branch` is its own row).
fn on_branch(after: &[&str], landed: bool) -> bool {
    let mut names = after.iter().copied().skip_while(|w| REMOTES.contains(w));
    let Some(name) = names.next() else {
        return false;
    };
    // A possessive names the branch it belongs to (`on origin/main's code`).
    let name = name.strip_suffix("'s").unwrap_or(name);
    let then = names.next();
    let file = name
        .rsplit_once('.')
        .is_some_and(|(_, ext)| !ext.is_empty() && ext.bytes().all(|b| b.is_ascii_alphabetic()));
    // `on branch b8/reflow`: the name follows the word.
    if name == "branch" {
        return then.is_some_and(|n| {
            branch_name(n) || BRANCHES.contains(&n) || (n.contains(['-', '_']) && !n.contains('.'))
        });
    }
    // A fork's main (`landed on fork/main`), never the remote's own.
    if name.split_once('/').is_some_and(|(remote, m)| {
        MAINS.contains(&m) && !REMOTES.contains(&remote) && !MAINS.contains(&name)
    }) {
        return true;
    }
    // Where a landing went (`landed on maint`, `landed on dev`, `landed on
    // feature-x`, `landed on the staging main`).
    if landed
        && !file
        && !MAINS.contains(&name)
        && name.starts_with(char::is_alphabetic)
        && (main_grown(name)
            || (then.is_some_and(|w| matches!(w, "main" | "master")) && BRANCHES.contains(&name))
            || ((BRANCHES.contains(&name) || name.contains(['-', '_']))
                && then.is_none_or(|w| JOINS.contains(&w))))
    {
        return true;
    }
    if MAINS.contains(&name)
        || DETERMINERS.contains(&name)
        || count(name).is_some()
        || file
        || !name.starts_with(char::is_alphabetic)
        || name
            .split_once('/')
            .is_some_and(|(_, m)| MAINS.contains(&m))
    {
        return false;
    }
    then == Some("branch")
        || branch_name(name)
        || (matches!(name, "develop" | "development" | "integration")
            && then.is_none_or(|w| JOINS.contains(&w)))
}

/// The parts of code a git word can name ([`land_open`]): `the stash
/// command`, `the draft parser`.
const CODE_PARTS: &[&str] = &[
    "command",
    "commands",
    "subcommand",
    "flag",
    "flags",
    "option",
    "options",
    "api",
    "module",
    "function",
    "method",
    "code",
    "logic",
    "parser",
    "handler",
    "helper",
    "implementation",
    "type",
    "struct",
    "enum",
    "crate",
    "help",
];

/// The parts of a change request a row can name with its own state (`the PR
/// description is updated`, [`change_open`]): the part is the subject,
/// not the change.
const CHANGE_PARTS: &[&str] = &[
    "description",
    "title",
    "body",
    "template",
    "comment",
    "comments",
    "thread",
    "threads",
    "link",
    "url",
    "number",
    "author",
    "label",
    "labels",
    "summary",
    "text",
];

/// Whether a push or a commit (`before` and `after` the words of its clause
/// around it in the sentence `s`, `prior` the line before it) leaves the work
/// off main ([`Reads::Onto`]): a branch pushed (`pushed the branch
/// feature-x`) unless the sentence says main has it, or a push or a commit
/// with no place (`pushed.`, `pushed to origin`, `everything is committed`)
/// unless the reply says main has the work (`said_main`: `Pushed the fix.
/// Nothing left: it is merged into the remote main.`), the sentence or the
/// line before it claims a landing ([`landing_claimed`]: `merged into main;
/// pushed`), or it carries main or a tag (`main is pushed`, `pushed the tag
/// v1.2`).
fn unplaced(s: &str, (prior, said_main): (&str, bool), before: &[&str], after: &[&str]) -> bool {
    let object = &after[..after.len().min(4)];
    let a_branch =
        |w: &&str| branch_name(w) && !w.split_once('/').is_some_and(|(_, m)| MAINS.contains(&m));
    if (object.iter().any(|w| matches!(*w, "branch" | "branches"))
        && !object.iter().any(|w| MAINS.contains(w)))
        || before.iter().any(a_branch)
    {
        return !main_claimed(s);
    }
    // Main as the push's own subject or object (`main is pushed`, `pushed
    // the main branch`, `pushed it to origin/main`), never a place a change
    // aims at (`has a PR into origin/main`, `to be merged into main`).
    let placed = before.iter().any(|w| MAINS.contains(w))
        || after
            .iter()
            .find(|w| !matches!(**w, "the" | "it" | "them"))
            .is_some_and(|w| MAINS.contains(w))
        || (0..after.len()).any(|k| {
            onto_main(&after[k..])
                && !after[..k]
                    .iter()
                    .any(|w| CHANGE_NOUNS.contains(w) || UNSETTLED.contains(w))
        });
    // A tag needs no branch (`pushed the tag v1.2`, `v1.4.0 is tagged, pushed
    // and published`).
    let tagged = before
        .iter()
        .chain(after)
        .any(|w| matches!(*w, "tag" | "tags") || version(w))
        || s.split_whitespace().next().map(bare).is_some_and(version);
    !placed
        && !tagged
        && !said_main
        && ![s, prior]
            .iter()
            .any(|t| landing_claimed(t) || main_named(t))
}

/// Whether `w` names a version (`v1.4.0`, `1.2.3`): a `v` or a digit, then
/// digits and at least one dot.
fn version(w: &str) -> bool {
    let n = w.strip_prefix('v').unwrap_or(w);
    n.starts_with(|c: char| c.is_ascii_digit())
        && n.contains('.')
        && n.bytes().all(|b| b.is_ascii_digit() || b == b'.')
}

/// Whether `text` names main as where the work is ([`unplaced`]): a place on
/// main ([`onto_main`]: `all on main`, `merged to origin/main`) or main
/// fast-forwarded, with no negation, hedge or rebase in the three words
/// before (`not on main`, `nearly on main`, `rebased it onto origin/main`).
fn main_named(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().map(bare).collect();
    (0..words.len()).any(|i| {
        let back = &words[i.saturating_sub(3)..i];
        !back.iter().any(|w| {
            negation(w)
                || matches!(
                    *w,
                    "not"
                        | "never"
                        | "without"
                        | "before"
                        | "nearly"
                        | "almost"
                        | "rebased"
                        | "rebasing"
                        | "rebase"
                        | "based"
                        | "pr"
                        | "prs"
                        | "mr"
                        | "request"
                        | "requests"
                        | "be"
                        | "to"
                        | "date"
                        | "sync"
                        | "against"
                        | "targeting"
                        | "targets"
                        | "opened"
                        | "will"
                )
        }) && (onto_main(&words[i..])
            || (matches!(words[i], "fast-forwarded" | "ff-merged")
                && words.get(i + 1).is_some_and(|m| MAINS.contains(m))))
    })
}

/// WHETHER A [`Reads::Change`] ROW IS AN UNMERGED CHANGE in its clause
/// (`[from, to)` of `s`, `before` and `after` its words there). FAIL TOWARD
/// OPEN: a change named anywhere reads open (`PR #12 is approved`, `I raised
/// #88`, `posted #88 for review`, `the change is in #88`, `the review on #12
/// is approved`) unless its stretch of the clause lands it ([`stretch`]:
/// `opened a PR and merged it into origin/main`, `the fix for PR #12 is
/// merged`) — the clause's own landing; another clause's never (`the first
/// PR is merged and the second one is approved`) — or it is named, not
/// reported: the object of a landing verb (`merged PR #12`, `squash-merged
/// the docs PR`), a reference right before it with no verb between (`fixes
/// #12`, `see #45`, `via #12`, `merged as #12`; never `two of the PRs` or
/// `no docs changed but PR #12`), a run's number (`run #4812`), a
/// bracketed one with no predicate after the bracket (`fix the race (#12)`,
/// never `the fix (#90) is approved`) or one in apposition (`the pending PR,
/// #12, is merged`), an item of a list a landing verb or a none heads
/// (`merged #11, #12 and #13`, `no open threads, issues or PRs`) or whose
/// shared predicate lands it (`#61, #62 and #63 are all merged`, `both PRs
/// (#61 and #62) are merged`, `#50: approved and merged`, `PR #61 and PR #62
/// are merged`), a part of the change ([`CHANGE_PARTS`]: `the PR description
/// is updated`), one a landing counts in (`everything is merged, PR #160
/// included`), or a number a later stretch lands ([`landed_again`]: `the
/// review on #12 is approved and #12 is merged`).
/// `the second one` and `the other one` are a change only after one is
/// named (`the first PR is merged and the second one is approved`).
fn change_open(
    s: &str,
    (from, to): (usize, usize),
    st: &Stand<'_>,
    before: &[&str],
    after: &[&str],
) -> bool {
    let lead = s[..from].trim_end();
    if matches!(
        st.word,
        "the second one" | "the other one" | "the second" | "the other"
    ) {
        let named = s[..st.at]
            .split_whitespace()
            .map(bare)
            .any(|w| CHANGE_NOUNS[..4].contains(&w) || w.starts_with('#'))
            || s[..st.at].contains("pull request");
        if !named {
            return false;
        }
    }
    // `X included` after a landing names what it holds (`everything is
    // merged, PR #160 included`).
    let included = after.len() <= 2 && after.last() == Some(&"included") && landing_claimed(lead);
    // A bracketed reference (`fix the race (#12)`), not a bracketed subject
    // whose predicate follows (`the fix (#90) is approved`).
    let bracketed = lead.ends_with('(')
        && s[to..].strip_prefix(')').is_none_or(|r| {
            r.split_whitespace()
                .map(bare)
                .find(|w| !w.is_empty())
                .is_none_or(|w| !verbish(w))
        });
    if included
        || bracketed
        || (lead.ends_with(',')
            && before.is_empty()
            && after.is_empty()
            && !s[to..].starts_with(','))
        || after.first().is_some_and(|w| CHANGE_PARTS.contains(w))
    {
        return false;
    }
    // Named by what reports it, or denied, or the object of a landing.
    let names = |words: &[&str]| {
        let landing = words
            .iter()
            .rposition(|w| verbish(w) || CUTS.contains(w))
            .is_some_and(|k| {
                (SHIPS.contains(&words[k])
                    || LANDING_VERBS.contains(&words[k])
                    || words[k] == "rebase-merged")
                    && !words[..k]
                        .iter()
                        .rev()
                        .take(2)
                        .any(|w| UNSETTLED.contains(w) || negation(w))
            });
        landing
            || words.windows(2).any(|w| {
                matches!(w[1], "as" | "in")
                    && (SHIPS.contains(&w[0]) || LANDING_VERBS.contains(&w[0]))
            })
            || words.iter().enumerate().any(|(i, w)| {
                // Only with no verb and no new stretch between it and the
                // change (`see the docs PR`, never `no docs changed but PR
                // #12`).
                !words[i + 1..]
                    .iter()
                    .any(|x| verbish(x) || CUTS.contains(x))
                    && matches!(
                        *w,
                        "no" | "none"
                            | "zero"
                            | "0"
                            | "via"
                            | "see"
                            | "per"
                            | "fixes"
                            | "closes"
                            | "resolves"
                            | "refs"
                            | "addresses"
                            | "supersedes"
                            | "fix"
                            | "close"
                            | "resolve"
                    )
            })
    };
    // A number an issue's close settles (`GitHub closed #45`, `fixed #45`);
    // the change already named (`updated in the same PR`); the change as
    // where a record of it is (`the numbers are in the PR`).
    let closed = st.word == "#n"
        && before
            .iter()
            .rposition(|w| verbish(w))
            .is_some_and(|k| matches!(before[k], "closed" | "fixed" | "resolved" | "addressed"));
    let same = before.last().is_some_and(|w| *w == "same");
    // A run's number (`run #4812`, `CI job #77`) names no change.
    let run = st.word == "#n"
        && before
            .last()
            .is_some_and(|w| matches!(*w, "run" | "build" | "job" | "pipeline" | "workflow"));
    if run {
        return false;
    }
    let record = matches!(before, [.., "in" | "on", "the"])
        && before.iter().any(|w| {
            matches!(
                *w,
                "numbers"
                    | "results"
                    | "details"
                    | "notes"
                    | "logs"
                    | "benchmarks"
                    | "measurements"
                    | "figures"
                    | "writeup"
                    | "write-up"
                    | "rationale"
                    | "discussion"
                    | "context"
            )
        });
    if closed || same || record || names(before) {
        return false;
    }
    // An item of a list with no verb of its own goes with the list's
    // head (`merged #11, #12 and #13`, `no open threads, issues or PRs`).
    let mut f = from;
    while !after.iter().any(|w| verbish(w)) {
        let lead = s[..f].trim_end();
        let Some(comma) = lead.strip_suffix(',').map(str::len) else {
            break;
        };
        let start = clause_start(s, comma);
        let words: Vec<&str> = s[start..comma].split_whitespace().map(bare).collect();
        if names(&words) {
            return false;
        }
        f = start;
    }
    // A list of changes whose shared predicate lands each (`#61, #62 and #63
    // are all merged into origin/main`).
    if !after.iter().any(|w| verbish(w)) && list_landed(s, to) {
        return false;
    }
    let (a, b) = stretch(s, from, to, (st.at, st.end()), SHIPS);
    let words: Vec<&str> = s[a..b]
        .split_whitespace()
        .map(bare)
        .filter(|w| !w.is_empty())
        .collect();
    // Changes coordinated in one subject share its predicate (`PR #61 and
    // PR #62 are merged`, `PR #12 and its follow-up #13 are merged`).
    let shared = before.iter().all(list_item) && {
        let v = after.iter().take_while(|w| list_item(w)).count();
        after[..v].contains(&"and") && settles_with(&after[v..], SHIPS, true)
    };
    !shared && !lands_in(&words) && !landed_again(s, st, b)
}

/// Whether the number the change `st` is named by (`#12`) is named again
/// after its stretch (`to`) in a stretch that lands it ([`change_open`]):
/// `the review on #12 is approved and #12 is merged`.
fn landed_again(s: &str, st: &Stand<'_>, to: usize) -> bool {
    let name = &s[st.at..st.end()];
    if st.word != "#n" || !name.starts_with('#') {
        return false;
    }
    let mut from = st.end();
    while let Some(k) = s[from..].find(name).map(|k| from + k) {
        let end = k + name.len();
        from = end;
        if s[end..].starts_with(|c: char| c.is_alphanumeric() || c == '\'' || c == '’') {
            continue;
        }
        let (a, b) = stretch(s, clause_start(s, k), clause_end(s, end), (k, end), SHIPS);
        let words: Vec<&str> = s[a..b]
            .split_whitespace()
            .map(bare)
            .filter(|w| !w.is_empty())
            .collect();
        if k >= to && lands_in(&words) {
            return true;
        }
    }
    false
}

/// A word a list of changes may hold besides the changes ([`list_landed`]):
/// a number, `and`, `its`, `the`, `pr`, `follow-up`.
fn list_item(w: &&str) -> bool {
    w.starts_with('#')
        || w.bytes().all(|b| b.is_ascii_digit())
        || matches!(
            *w,
            "and"
                | "or"
                | "the"
                | "its"
                | "pr"
                | "prs"
                | "mr"
                | "mrs"
                | "pull"
                | "request"
                | "requests"
                | "follow-up"
                | "both"
                | "all"
        )
}

/// Whether `words` (a stretch of a clause) land what they name: a landing
/// word ([`SHIPS`], `rebase-merged`) or a place on the remote main a verb
/// carries it to or keeps it on ([`settles_at`]: `went into origin/main`,
/// `is on origin/main`), undenied and not still to come ([`UNSETTLED`]),
/// before no condition ([`CONDITIONS`]: `is merged once CI passes`).
fn lands_in(words: &[&str]) -> bool {
    (0..words.len()).any(|k| {
        let w = words[k];
        let landing =
            SHIPS.contains(&w) || w == "rebase-merged" || settles_at(&words[k..], &[], true);
        landing
            && !words[..k]
                .iter()
                .rev()
                .take(3)
                .any(|x| UNSETTLED.contains(x) || negation(x) || *x == "without")
            && !words[k..].iter().any(|x| CONDITIONS.contains(x))
    })
}

/// Whether the clauses after the mark at `to` in `s` (a comma, a bracket or a
/// colon) go on with a list of changes and a predicate that lands them all
/// ([`change_open`]): `#61, #62 and #63 are all merged into origin/main`,
/// `both PRs (#61 and #62) are merged`, `#50: approved and merged`.
fn list_landed(s: &str, mut to: usize) -> bool {
    let item = list_item;
    while s[to..].starts_with([',', '(', ')', ':']) {
        let start = to + 1;
        let end = clause_end(s, start);
        let words: Vec<&str> = s[start..end]
            .split_whitespace()
            .map(bare)
            .filter(|w| !w.is_empty())
            .collect();
        match words.iter().position(|w| starts_predicate(w, SHIPS)) {
            Some(v) => {
                return words[..v].iter().all(item) && settles_with(&words[v..], SHIPS, true);
            }
            None if words.iter().all(item) => to = end,
            None => return false,
        }
    }
    false
}

/// Whether `n` is a number below a hundred as a percent writes one
/// ([`Reads::Percent`]): `90`, `99.9`, `ninety`, `ninety-five`, never
/// `100` nor `100.0`.
fn below_hundred(n: &str) -> bool {
    const TENS: [&str; 9] = [
        "ten", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];
    let decimal = !n.is_empty()
        && n.starts_with(|c: char| c.is_ascii_digit())
        && n.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && n.parse::<f64>().is_ok_and(|v| v < 100.0);
    let spelled = !n.starts_with(|c: char| c.is_ascii_digit()) && count(n).is_some();
    decimal
        || spelled
        || TENS.contains(&n)
        || n.split_once('-')
            .is_some_and(|(t, u)| TENS.contains(&t) && count(u).is_some())
}

/// The labels that name work, whose item a percent is the part done of
/// ([`Reads::Percent`]): `Docs: 80%`, `Linux port: 60%`.
const WORK_LABELS: &[&str] = &[
    "docs",
    "documentation",
    "tests",
    "migration",
    "port",
    "porting",
    "conversion",
    "progress",
    "done",
    "completion",
    "implementation",
    "rewrite",
    "refactor",
    "work",
    "readers",
    "lanes",
    "status",
];

/// Whether what follows a percent (`after`) claims a part of a whole
/// ([`Reads::Percent`]): `of` (`90% of it`), a landing or a settling word
/// (`95% merged`), or coverage (`0% coverage`), never another measure (`20%
/// faster`).
fn percent_part(after: &str) -> bool {
    after
        .split_whitespace()
        .map(bare)
        .find(|w| !w.is_empty())
        .is_some_and(|w| {
            w == "of"
                || SETTLED.contains(&w)
                || LANDED.contains(&w)
                || matches!(
                    w,
                    "coverage" | "covered" | "migrated" | "ported" | "tested" | "there"
                )
        })
}

/// What may follow an integration branch's name and leave it the name
/// ([`onto_branch`]): `merged into develop and pushed`, never `merged into
/// release notes`.
const JOINS: &[&str] = &[
    "and", "but", "then", "only", "too", "first", "so", "while", "now", "with", "for", "on",
];

/// WHETHER THE ROW `st` STANDS AS A THING ITS OWN CLAUSE LANDS, read
/// narrowly, since a rule that settles a leftover is the only way to a false
/// landing (FAIL TOWARD OPEN). Within the row's clause (`[from, to)`, cut
/// at [`CUTS`] by [`stretch`]) and by what its family takes
/// ([`LandFamily::settled_by`], narrowed by its noun phrase: [`settlers`]):
///
/// * the row inside its clause's subject (no verb before it), whose
///   predicate, within four words of it, settles it ([`settles_with`]: `the
///   pending PR was merged`, `the TODO in parser.rs is resolved`, `the
///   follow-up is on remote main`, `the stashed change was committed and
///   pushed to origin/main`, `auto-merge landed it`);
/// * the row the object of a settling verb the worker did (`merged the
///   pending PR`, `removed the old workaround`), or a branch the worker
///   deleted after a landing the sentence claims (`merged to origin/main and
///   deleted my local branch`), with nothing after it that undoes it
///   ([`UNDOES`]: `deleted my local branch without merging it`);
/// * the row its clause's predicate, which the same predicate goes on to
///   settle (`the PR was opened and merged`, `the push failed once, then
///   succeeded`);
/// * the row ending a relative clause in the subject, settled by the
///   predicate after it (`the branch I had parked is merged into
///   origin/main`), a predicate that goes on over a list of participles
///   (`is applied, committed and merged into origin/main`), a clause after
///   `that` a check opens (`I double-checked that the stash list is empty`),
///   a thing that exists nowhere now (`the old path no longer exists`), or a
///   branch gone where the sentence lands the work (`the temporary branch is
///   deleted and the work is merged into origin/main`).
///
/// Never another clause or another subject (`the docs were deferred; the
/// fix is merged now`), never a verb that lands nothing (`the pending PR is
/// rebased on origin/main`, `is a pull request into origin/main`, `conflicts
/// with origin/main`), never a predicate after [`UNSETTLED`] (`ready to be
/// merged`) or before a condition ([`CONDITIONS`]: `is merged once CI
/// passes`) unless it is past ([`PAST_AUX`]).
fn settled(s: &str, from: usize, to: usize, st: &Stand<'_>) -> bool {
    if ["not ", "never ", "did not ", "could", "can", "unable"]
        .iter()
        .any(|p| st.word.starts_with(p))
    {
        // A lack negates its own clause: `not all of the comments were
        // addressed` settles nothing.
        return false;
    }
    let (at, end) = (st.at, st.end());
    let mut back: Vec<&str> = s[from..at]
        .split_whitespace()
        .map(bare)
        .filter(|w| !w.is_empty())
        .collect();
    // What the worker checked is so is a clause of its own (`I
    // double-checked that the stash list is empty`).
    if let Some(k) = back.iter().rposition(|w| *w == "that")
        && k > 0
        && CONFIRMED.contains(&back[k - 1])
        && !back[..k]
            .iter()
            .any(|w| negation(w) || UNSETTLED.contains(w))
    {
        back.drain(..=k);
    }
    let whole: Vec<&str> = s[end..to]
        .split_whitespace()
        .map(bare)
        .filter(|w| !w.is_empty())
        .collect();
    let np_end = whole
        .iter()
        .position(|w| starts_predicate(w, &[]))
        .unwrap_or(whole.len());
    let np: Vec<&str> = back.iter().chain(&whole[..np_end]).copied().collect();
    let by = settlers(st, &np);
    if by.is_empty() {
        return false;
    }
    // A change (`the open PR`) lands by its place as well as its merge.
    let place = st.lands_by_place()
        || CHANGE_NOUNS.contains(&st.word)
        || np.iter().any(|w| CHANGE_NOUNS.contains(w));
    if let Some(v) = object_verb(&back, st) {
        let did = worker_did(&back[..v]) && !whole.iter().any(|w| UNDOES.contains(w));
        if did
            && (by.contains(&back[v])
                || (place
                    && GONE.contains(&back[v])
                    && (landing_claimed(s) || merged_first(&whole))))
        {
            return true;
        }
    }
    if matches!(st.reads(), Reads::Change) && np_end == whole.len() && s[to..].starts_with(':') {
        // A change as a label, its state the item after the colon (`PR
        // #12: merged`): only that item settles it.
        let item: Vec<&str> = s[to + 1..clause_end(s, to + 1)]
            .split_whitespace()
            .map(bare)
            .filter(|w| !w.is_empty())
            .collect();
        return settles_once(&item, &by, place)
            && !item
                .iter()
                .any(|w| UNSETTLED.contains(w) || negation(w) || *w == "without");
    }
    let (_, b) = stretch(s, from, to, (at, end), &by);
    let mut fwd: Vec<&str> = s[end..b]
        .split_whitespace()
        .map(bare)
        .filter(|w| !w.is_empty())
        .collect();
    // Two subjects sharing one predicate (`#77 and #78 are both merged`):
    // the predicate after the second is the row's too.
    if whole.first() == Some(&"and") {
        let shared = (2..=4).find(|&k| {
            whole
                .get(k)
                .is_some_and(|w| AUX.contains(w) && !matches!(*w, "all" | "both"))
                && whole[1..k].iter().all(|w| !verbish(w))
        });
        if let Some(k) = shared {
            fwd = whole[k..].to_vec();
        }
    }
    // A list of participles the predicate goes on with (`the stashed change
    // is applied, committed and merged into origin/main`).
    if b == to
        && fwd
            .iter()
            .any(|w| AUX.contains(w) && !matches!(*w, "all" | "both"))
    {
        let mut t = to;
        while s[t..].starts_with(',') {
            let next = clause_end(s, t + 1);
            let words: Vec<&str> = s[t + 1..next]
                .split_whitespace()
                .map(bare)
                .filter(|w| !w.is_empty())
                .collect();
            if !words.first().is_some_and(|w| past_form(w))
                || words.iter().any(|w| negation(w) || UNSETTLED.contains(w))
            {
                break;
            }
            fwd.push("and");
            fwd.extend(words);
            t = next;
        }
    }
    let lead = st.word.split(' ').next().unwrap_or("");
    // A relative clause the row ends, in the subject (`the branch I had
    // parked is merged into origin/main`): the predicate after it is the
    // subject's.
    let relative = past_form(lead)
        && !st.word.contains(' ')
        && back
            .iter()
            .position(|w| matches!(*w, "i" | "we" | "you" | "they"))
            .is_some_and(|p| {
                p >= 2
                    && DETERMINERS.contains(&back[0])
                    && !back[1..p].iter().any(|w| verbish(w))
                    && back[p + 1..]
                        .iter()
                        .all(|w| matches!(*w, "had" | "have" | "has"))
            });
    if relative {
        return whole
            .first()
            .is_some_and(|w| AUX.contains(w) && !matches!(*w, "all" | "both"))
            && !whole
                .iter()
                .any(|w| UNSETTLED.contains(w) || negation(w) || *w == "without")
            && settles_with(&whole, &by, place);
    }
    let named =
        !(AUX.contains(&lead) || FINITE.contains(&lead) || lead == "still" || lead == "only")
            && (back.last().is_none_or(|w| DETERMINERS.contains(w)) || !past_form(lead));
    if back.iter().any(|w| verbish(w)) || !named {
        // The row is its clause's predicate (`the push failed`, `was
        // opened`): only that predicate going on settles it, or, when it
        // was so, a clause with no subject of its own that says what it is
        // now (`CI was failing earlier, now green`).
        let now = back.iter().any(|w| matches!(*w, "was" | "were"))
            && s[to..].strip_prefix(", now ").is_some_and(|r| {
                let words: Vec<&str> = r[..clause_end(r, 0)]
                    .split_whitespace()
                    .map(bare)
                    .filter(|w| !w.is_empty())
                    .collect();
                settles_once(&words, &by, place)
            });
        return now
            || fwd.iter().enumerate().any(|(i, w)| {
                matches!(*w, "and" | "then") && settles_once(&fwd[i + 1..], &by, place)
            });
    }
    // A participle before its noun (`the stashed change is merged`): the
    // noun is no predicate, whatever else it can be.
    let head = usize::from(
        past_form(lead)
            && back.last().is_some_and(|w| DETERMINERS.contains(w))
            && fwd
                .first()
                .is_some_and(|w| !AUX.contains(w) && !by.contains(w) && !LANDING_VERBS.contains(w)),
    );
    let mut starts = fwd
        .iter()
        .enumerate()
        .skip(head)
        .filter(|(_, w)| starts_predicate(w, &by))
        .map(|(k, _)| k);
    // Gone is gone (`the old path no longer exists`); and a branch gone where
    // the sentence lands the work (`the temporary branch is deleted and the
    // work is merged into origin/main`).
    if let ["no", "longer", "exists" | "exist", rest @ ..] = &fwd[head.min(fwd.len())..] {
        // Nowhere, not somewhere (`no longer exists on Windows`).
        return by.contains(&"gone")
            && rest
                .iter()
                .all(|w| matches!(*w, "anymore" | "now" | "at" | "all"));
    }
    let branch = np
        .iter()
        .any(|w| matches!(*w, "branch" | "branches" | "worktree" | "worktrees"));
    let gone = fwd.iter().position(|w| AUX.contains(w)).is_some_and(|v| {
        !fwd[..v].iter().any(|w| verbish(w)) && fwd.get(v + 1).is_some_and(|w| GONE.contains(w))
    });
    if branch && gone && landing_claimed(s) {
        return true;
    }
    let Some(mut k) = starts.next() else {
        return false;
    };
    if fwd
        .first()
        .is_some_and(|w| matches!(*w, "i" | "we" | "you" | "they" | "that" | "which" | "who"))
    {
        // A relative clause in the subject (`the draft I mentioned is
        // merged`): the predicate is the next verb.
        let Some(next) = starts.next() else {
            return false;
        };
        k = next;
    }
    k <= 5
        && !fwd[..k]
            .iter()
            .any(|w| UNSETTLED.contains(w) || negation(w) || *w == "without")
        && (settles_with(&fwd[k..], &by, place) || empty_now(&fwd[k..]))
}

/// The verbs of checking whose `that` opens what was found so ([`settled`]):
/// `I double-checked that the stash list is empty`.
const CONFIRMED: &[&str] = &[
    "checked",
    "double-checked",
    "confirmed",
    "verified",
    "ensured",
    "saw",
];

/// Whether a predicate says its subject holds nothing ([`settled`]): `the
/// stash is empty`, `the merge queue was empty`.
fn empty_now(words: &[&str]) -> bool {
    let skip = words.iter().take_while(|w| AUX.contains(w)).count();
    skip > 0
        && match &words[skip..] {
            ["empty"] => true,
            ["empty", w, ..] => matches!(*w, "and" | "now" | "too" | "again"),
            _ => false,
        }
}

/// The finite verbs a row may open with, which make it its clause's
/// predicate, never a thing its predicate settles ([`settled`]): `is
/// misleading`, `needs review`, `lacks docs`.
const FINITE: &[&str] = &[
    "is", "are", "am", "was", "were", "has", "have", "had", "remains", "remain", "stays", "stay",
    "will", "would", "can", "could", "should", "must", "may", "might", "does", "do", "needs",
    "need", "requires", "require", "lacks", "wants", "waits", "wait", "awaits",
];

/// What a row's own family settles it by, narrowed by the noun phrase it
/// stands in (`np`, [`settled`]): a flaky check only by its fix
/// ([`DEFLAKED`]); a change (a PR, a branch, a commit: [`CHANGE_NOUNS`]) only
/// by a landing, whatever else the family takes (`the open PR is closed`,
/// `the pending PR is approved` land nothing); a review or a question
/// ([`REVIEW_NOUNS`]) also by its answer (`the pending review is done`, `the
/// open question is answered`).
fn settlers(st: &Stand<'_>, np: &[&str]) -> Vec<&'static str> {
    let by = st.settled_by();
    if by.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<&'static str> = if FLAKY.contains(&st.word) {
        DEFLAKED.to_vec()
    } else {
        by.to_vec()
    };
    let change = np.iter().any(|w| CHANGE_NOUNS.contains(w)) || CHANGE_NOUNS.contains(&st.word);
    if st.word == "stray" {
        // A stray thing is junk, a stray commit too: gone is done (`the stray
        // debug print is removed`, `the stray debug commit was dropped`).
        out.extend(["removed", "deleted", "gone", "dropped"]);
    } else if change {
        out = SHIPS.to_vec();
    } else if st.word == "#n"
        || st
            .word
            .split(' ')
            .any(|w| matches!(w, "issue" | "issues" | "ticket" | "tickets"))
    {
        // An issue (`the tracking issue`), or a bare number, which names
        // one as often as a change: its fix or its close settles it (`#45
        // is closed`); a number a change noun names (`PR #45 is closed`) is
        // a change, settled only by landing.
        out.extend(["fixed", "resolved", "closed"]);
    } else {
        if np.iter().any(|w| REVIEW_NOUNS.contains(w)) {
            out.extend(REVIEWED);
        }
        if matches!(st.word, "remaining" | "leftover" | "outstanding" | "paused")
            && np.iter().any(|w| CHECK_NOUNS.contains(w))
        {
            out.extend(CHECKS);
        }
    }
    out
}

/// What may begin a row's predicate ([`settled`]): a finite or passing-over
/// word ([`AUX`], not `all` or `both`), a settling word of `by`, a landing
/// verb, or another verb — never a preposition (`the pending PR on the fork
/// is merged`).
fn starts_predicate(w: &str, by: &[&str]) -> bool {
    (AUX.contains(&w) && !matches!(w, "all" | "both"))
        || by.contains(&w)
        || LANDING_VERBS.contains(&w)
        || (verbish(w) && !PREPOSITIONS.contains(&w) && w != "on")
}

/// Whether a predicate (`words`, from its first word) settles its subject
/// ([`settled`]): at once ([`settles_once`]: `is merged`, `is now on remote
/// main`), or after an `and` or `then` that goes on with the same predicate
/// (`was opened and merged`, `was committed and pushed to origin/main`). A
/// new subject after the `and` is another clause (`is approved and the docs
/// are merged`): its first word settles nothing.
fn settles_with(words: &[&str], by: &[&str], place: bool) -> bool {
    settles_once(words, by, place)
        || words
            .iter()
            .enumerate()
            .any(|(i, w)| matches!(*w, "and" | "then") && settles_once(&words[i + 1..], by, place))
}

/// Whether `words`, past [`AUX`] words, settle what they follow at once
/// ([`settles_at`]), and no condition follows that is not past
/// ([`CONDITIONS`], [`PAST_AUX`]: `is merged once CI passes` settles nothing,
/// `was merged when CI passed` does).
fn settles_once(words: &[&str], by: &[&str], place: bool) -> bool {
    let skip = words.iter().take_while(|w| AUX.contains(w)).count();
    let past = skip
        .checked_sub(1)
        .and_then(|i| words.get(i))
        .is_some_and(|w| PAST_AUX.contains(w));
    settles_at(words, by, place) && (past || !words[skip..].iter().any(|w| CONDITIONS.contains(w)))
}

/// Where the verb stands whose object is the row (`back` the words before
/// it in its clause): right before the row's noun phrase — its determiner
/// and one word past it (`merged the pending PR`, `removed the old
/// workaround`), or the row's own determiner (`deleted my local branch`).
fn object_verb(back: &[&str], st: &Stand<'_>) -> Option<usize> {
    let own = st
        .word
        .split(' ')
        .next()
        .is_some_and(|w| DETERMINERS.contains(&w) || w == "local");
    let determiner = |w: &str| {
        DETERMINERS.contains(&w) || (!w.is_empty() && w.bytes().all(|c| c.is_ascii_digit()))
    };
    let d = match back {
        _ if own => back.len(),
        [.., d] if determiner(d) => back.len() - 1,
        [.., d, w] if determiner(d) && !CUTS.contains(w) && !negation(w) => back.len() - 2,
        _ => return None,
    };
    d.checked_sub(1)
}

/// Whether the words before a verb (`pre`) leave the worker its subject:
/// none, or `I` or `we` with words that pass over (`I then merged`, `we have
/// also removed`), read from the last `and` or `then` that joins it to a verb
/// before (`I merged the pending PR and deleted the stash`) — never a
/// negation (`never merged`) nor another subject.
fn worker_did(pre: &[&str]) -> bool {
    let from = pre
        .iter()
        .rposition(|w| matches!(*w, "and" | "then"))
        .map_or(0, |i| i + 1);
    pre[from..].iter().all(|w| {
        matches!(
            *w,
            "i" | "we"
                | "then"
                | "also"
                | "finally"
                | "just"
                | "so"
                | "have"
                | "had"
                | "has"
                | "now"
                | "already"
                | "since"
        )
    })
}

/// What a branch's going settles when the sentence claims a landing
/// ([`settled`]: `merged to origin/main and deleted my local branch`).
const GONE: &[&str] = &["deleted", "removed", "dropped", "pruned"];

/// Whether the rest of a clause that deletes a branch (`whole`) says it was
/// merged first, and no more ([`settled`]): `deleted my local branch after
/// merging it` — never `after merging it into fix/x`.
fn merged_first(whole: &[&str]) -> bool {
    matches!(
        whole,
        ["after", "merging" | "landing"] | ["after", "merging" | "landing", "it" | "them" | "both"]
    )
}

/// What after a row undoes a verb that would settle it ([`settled`]):
/// `deleted my local branch without merging it`, `before merging`, `dropped
/// the stash instead of applying it`.
const UNDOES: &[&str] = &[
    "without", "before", "instead", "unmerged", "unlanded", "unpushed", "never", "not",
];

/// Whether the sentence `s` claims a landing undenied: a place on the
/// remote main a landing verb names ([`main_claimed`]), or `merged` or
/// `landed` with no negation, `without` or `before` before it.
fn landing_claimed(s: &str) -> bool {
    let words: Vec<&str> = s.split_whitespace().map(bare).collect();
    main_claimed(s)
        || words.iter().enumerate().any(|(i, w)| {
            matches!(*w, "merged" | "landed")
                && !words[..i]
                    .iter()
                    .rev()
                    .take(3)
                    .any(|x| negation(x) || matches!(*x, "not" | "without" | "before"))
        })
}

/// The clause of a row read past what interrupts it before its settling
/// word ([`settled`]), `to` where its clause ends in `s`: an appositive taken
/// out (`the pending PR, #12, is merged`, `the follow-up — the docs fix — is
/// merged`, `the remaining item from the plan (the docs) is merged`: a few
/// words and no verb, or a relative clause, `the PR, which was pending, is
/// merged`, whose own rows are read on their own ([`aside_settled`]; then
/// the clause going on with a verb), or a comma before
/// `then` read through (`the push failed once, then succeeded`). The
/// sentence so joined and where the clause now ends; `None` where nothing
/// interrupts it.
fn past_aside(s: &str, to: usize) -> Option<(String, usize)> {
    let rest = &s[to..];
    for lead in [", then ", ", and then ", ", but then "] {
        if let Some(r) = rest.strip_prefix(lead) {
            let joined = format!("{} then {r}", &s[..to]);
            let end = clause_end(&joined, to + 1);
            return Some((joined, end));
        }
    }
    let mark = rest.chars().next()?;
    let (close, most) = match mark {
        ',' => (',', 4),
        '—' | '–' => (mark, 6),
        '(' => (')', 8),
        _ => return None,
    };
    let inner_at = to + mark.len_utf8();
    let len = s[inner_at..].find(close)?;
    let inner = &s[inner_at..inner_at + len];
    let after = &s[inner_at + len + close.len_utf8()..];
    let words: Vec<&str> = inner.split_whitespace().map(bare).collect();
    let first = after.split_whitespace().map(bare).find(|w| !w.is_empty());
    // A participle about the subject's past (`the PR, queued earlier, is
    // merged`, `the PR (blocked earlier on CI) is merged`, `the PR, as noted,
    // is merged`): its own rows are read on their own ([`aside_settled`]).
    let participle = match words.as_slice() {
        ["as", w] => past_form(w),
        [w, rest @ ..] => {
            past_form(w)
                && !rest
                    .iter()
                    .any(|r| verbish(r) && !PREPOSITIONS.contains(r) && *r != "on")
        }
        [] => false,
    } && !words.iter().any(|w| negation(w) || UNSETTLED.contains(w));
    if words.is_empty()
        || words.len() > most
        || inner.contains(CLAUSE_MARKS)
        || (words.iter().any(|w| verbish(w))
            && !participle
            && !words
                .first()
                .is_some_and(|w| matches!(*w, "which" | "who" | "that")))
        || !first.is_some_and(verbish)
    {
        return None;
    }
    let joined = format!("{}{after}", &s[..to]);
    let end = clause_end(&joined, to);
    Some((joined, end))
}

/// Whether what follows `local main` in its clause (`after`) says it and the
/// remote main are one ([`land_open`]): `local main and origin/main point at
/// the same commit`, `… match`, `… are in sync`, `local main is even with
/// origin/main`. Nothing else: `local main is
/// ahead` is work not landed.
fn even_with_remote(after: &[&str]) -> bool {
    let remote = |m: &str| matches!(m, "origin/main" | "origin/master" | "upstream/main");
    if let [
        "is",
        "even" | "level" | "identical" | "equal",
        "with" | "to",
        m,
    ]
    | ["is", "in", "sync", "with", m]
    | ["is", "at", m] = after
    {
        return remote(m);
    }
    let rest = match after {
        ["and", m, rest @ ..]
            if matches!(*m, "origin/main" | "origin/master" | "upstream/main") =>
        {
            rest
        }
        [verb, ..] if matches!(*verb, "matches" | "equals") => after,
        _ => return false,
    };
    matches!(
        rest,
        ["point" | "are", "at", "the", "same", "commit" | "sha"]
            | ["point", "to", "the", "same", "commit" | "sha"]
            | ["match" | "agree"]
            | ["matches" | "equals", "origin/main" | "upstream/main"]
            | ["are", "in", "sync" | "step"]
            | ["are", "identical" | "even" | "equal"]
    )
}

/// Whether the row `st` stands in a clause (`[from, to)` of `s`) that adds
/// its change to the landing the clause before claims ([`land_open`]):
/// `everything is merged to origin/main, the follow-up refactor too`, `…,
/// the docs PR included`. Narrow: a follow-up or a change, after a comma
/// that ends a landing claim ([`landing_claimed`]), in a clause of at most
/// six words that ends in `too`, `also`, `included` or `as well` and holds
/// no verb and no other row (`…, the docs PR pending too` stays open).
fn also_landed(s: &str, (from, to): (usize, usize), st: &Stand<'_>, stands: &[Stand<'_>]) -> bool {
    let words: Vec<&str> = s[from..to]
        .split_whitespace()
        .map(bare)
        .filter(|w| !w.is_empty())
        .collect();
    let lead = s[..from].trim_end();
    let added = matches!(words.last(), Some(&"too" | &"also" | &"included"))
        || words.ends_with(&["as", "well"]);
    added
        && words.len() <= 6
        && matches!(
            st.word,
            "follow-up"
                | "follow-ups"
                | "followup"
                | "followups"
                | "pr"
                | "prs"
                | "pull request"
                | "pull requests"
                | "mr"
                | "mrs"
                | "#n"
        )
        && lead.ends_with(',')
        && landing_claimed(lead)
        && !words.iter().any(|w| verbish(w) && *w != "included")
        && !stands.iter().any(|o| {
            (from..to).contains(&o.at) && o.at != st.at && !matches!(o.reads(), Reads::Change)
        })
}

/// The words an aside may date its row by ([`aside_settled`]): `pending
/// earlier`, `blocked at first`.
const BEFORE_NOW: &[&str] = &[
    "earlier",
    "before",
    "previously",
    "initially",
    "originally",
    "at",
    "first",
    "once",
    "yesterday",
];

/// WHETHER THE ROW `st` STANDS IN AN ASIDE ABOUT ITS SUBJECT'S PAST and the
/// clause around the aside settles that subject ([`land_open`]): `the PR —
/// pending earlier — is merged`, `the PR (pending earlier) is merged`, `the
/// PR, which was pending, is merged`. Narrow (FAIL TOWARD OPEN): the aside
/// holds the row and no more than a word that dates it ([`BEFORE_NOW`]), or
/// is a relative clause in the past (`which was`, `that had been`); the
/// subject before it is a noun phrase with no verb; and the predicate right
/// after it lands that subject ([`settles_with`]). `the PR, which is still
/// pending, is merged` and `the PR (pending review) is approved` settle
/// nothing.
fn aside_settled(s: &str, st: &Stand<'_>) -> bool {
    let (at, end) = (st.at, st.end());
    let Some(open_at) = s[..at].rfind(CLAUSE_MARKS) else {
        return false;
    };
    let Some(mark) = s[open_at..].chars().next() else {
        return false;
    };
    let close = match mark {
        ',' | '—' | '–' => mark,
        '(' => ')',
        _ => return false,
    };
    let Some(close_at) = s[end..].find(close).map(|c| end + c) else {
        return false;
    };
    let inner: Vec<&str> = s[open_at + mark.len_utf8()..close_at]
        .split_whitespace()
        .map(bare)
        .filter(|w| !w.is_empty())
        .collect();
    let row: Vec<&str> = st.word.split(' ').collect();
    let relative = inner
        .first()
        .is_some_and(|w| matches!(*w, "which" | "who" | "that"));
    let mut i = 0;
    if relative {
        if !inner
            .get(1)
            .is_some_and(|w| matches!(*w, "was" | "were" | "had"))
        {
            return false;
        }
        i = 2;
        if inner.get(i) == Some(&"been") {
            i += 1;
        }
    }
    if inner.get(i..i + row.len()) != Some(row.as_slice()) {
        return false;
    }
    let dated = &inner[i + row.len()..];
    // What dates it, and at most a place or a party it waited on (`blocked
    // earlier on CI`).
    let on: Vec<&str> = dated
        .iter()
        .copied()
        .filter(|w| !BEFORE_NOW.contains(w))
        .collect();
    let waited_on = on.is_empty()
        || (on.len() <= 3
            && matches!(on[0], "on" | "by" | "in" | "for" | "behind")
            && dated.iter().any(|w| BEFORE_NOW.contains(w) && *w != "at")
            && !on
                .iter()
                .any(|w| negation(w) || UNSETTLED.contains(w) || matches!(*w, "still" | "now")));
    if !waited_on || (dated.is_empty() && !relative) {
        return false;
    }
    let from = clause_start(s, open_at);
    let subject: Vec<&str> = s[from..open_at]
        .split_whitespace()
        .map(bare)
        .filter(|w| !w.is_empty())
        .collect();
    if subject.is_empty() || subject.iter().any(|w| verbish(w)) {
        return false;
    }
    let rest = &s[close_at + close.len_utf8()..];
    let pred: Vec<&str> = rest[..clause_end(rest, 0)]
        .split_whitespace()
        .map(bare)
        .filter(|w| !w.is_empty())
        .collect();
    // A row its family settles by nothing (`blocked`) is past where its
    // subject has shipped since (`the release, blocked at first, is
    // published`).
    let by = match settlers(st, &subject) {
        by if by.is_empty() => SHIPS.to_vec(),
        by => by,
    };
    let place = st.lands_by_place() || subject.iter().any(|w| CHANGE_NOUNS.contains(w));
    pred.first().is_some_and(|w| starts_predicate(w, &by))
        && !pred
            .iter()
            .any(|w| UNSETTLED.contains(w) || negation(w) || *w == "without")
        && settles_with(&pred, &by, place)
}

/// Whether a word of `s[a..b]` is one of `nouns` ([`noun_in`]).
fn nouns_in(s: &str, a: usize, b: usize, nouns: &[&str]) -> bool {
    let mut prev = None;
    s[a..b].split_whitespace().any(|raw| {
        let hit = noun_in(prev, raw, nouns);
        prev = Some(raw);
        hit
    })
}

/// The kinds of secret only a person or a service holds ([`noun_in`]).
const SECRETS: &[&str] = &[
    "token",
    "tokens",
    "key",
    "keys",
    "password",
    "passwords",
    "secret",
    "secrets",
    "passphrase",
];

/// The words before a [`SECRETS`] word that make it a person's or a
/// service's (`the crates.io token`, `the signing key`).
const SECRET_KINDS: &[&str] = &[
    "crates.io",
    "npm",
    "pypi",
    "api",
    "access",
    "auth",
    "github",
    "gitlab",
    "release",
    "publish",
    "publishing",
    "signing",
    "deploy",
    "personal",
    "apple",
    "notary",
    "ssh",
    "gpg",
    "admin",
];

/// The `'s` words that are no possessive ([`noun_in`]).
const NOT_POSSESSIVE: &[&str] = &[
    "it's", "that's", "what's", "there's", "here's", "let's", "he's", "she's", "who's", "one's",
];

/// Whether the word `raw` (`prev` the word before it) is one of `nouns`: as
/// written (`PR`, the owner's), a change's or an issue's number (`#45`,
/// `!12`) where `nouns` holds `#n`, or a person's or a service's secret
/// (`Andrew's token`, `the crates.io token`) where `nouns` holds `#secret`.
fn noun_in(prev: Option<&str>, raw: &str, nouns: &[&str]) -> bool {
    let w = bare(raw);
    let numbered = || {
        let t = raw.trim_matches(|c: char| matches!(c, ',' | '.' | ':' | ';' | '(' | ')'));
        t.len() > 1 && t.starts_with(['#', '!']) && t[1..].bytes().all(|b| b.is_ascii_digit())
    };
    let secret = || {
        SECRETS.contains(&w)
            && prev.map(bare).is_some_and(|p| {
                (p.ends_with("'s") && !NOT_POSSESSIVE.contains(&p)) || SECRET_KINDS.contains(&p)
            })
    };
    nouns.contains(&w)
        || w.strip_suffix("'s").is_some_and(|w| nouns.contains(&w))
        // A test by its module path (`hooks::tests::every_hook_dialect`).
        || (nouns.contains(&"tests") && (w.contains("tests::") || w.ends_with("::tests")))
        || (nouns.contains(&"#n") && numbered())
        || (nouns.contains(&"#secret") && secret())
        || (nouns.contains(&"#branch")
            && branch_name(w)
            && !w.split_once('/').is_some_and(|(_, m)| MAINS.contains(&m)))
}

/// Whether a row's clause, opened by a colon or a bracket with nothing but
/// [`AUX`] words before the row (`PR #12: open`, `#45 (docs) is open`), or an
/// item after a comma in a labelled list (`CI: green on Linux, red on
/// macOS`), has one of `nouns` in what it labels: the clause before the colon
/// or the bracket.
fn labelled(s: &str, from: usize, before: &[&str], nouns: &[&str]) -> bool {
    if !before.iter().all(|w| AUX.contains(w)) {
        return false;
    }
    let lead = s[..from].trim_end();
    let label_end = if let Some(l) = lead.strip_suffix(':') {
        l.len()
    } else if lead.ends_with(',') {
        // An item of a labelled list (`CI: green on Linux, red on macOS`).
        match lead.rfind(':') {
            Some(colon) if !lead[colon..].contains([';', '(', ')']) => colon,
            _ => return false,
        }
    } else if lead.ends_with(')') {
        match lead.rfind('(') {
            Some(open) => open,
            None => return false,
        }
    } else {
        return false;
    };
    nouns_in(s, clause_start(s, label_end), label_end, nouns)
}

/// A negation as Land reads one ([`denied`]): [`OPEN_NEGATIONS`], and
/// `nobody`.
fn negation(w: &str) -> bool {
    OPEN_NEGATIONS.contains(&w) || matches!(w, "nobody" | "noone" | "no-one")
}

/// The words that end a negation's reach in its clause ([`denied`]):
/// `nothing is pending, but CI is running`.
const CONTRASTS: &[&str] = &[
    "but", "while", "whereas", "though", "although", "yet", "however",
];

/// The words a list of predicates or subjects joins its items with
/// ([`denied`]): `nothing is pending, deferred or parked`.
const GLUE: &[&str] = &[
    "and", "or", "nor", "a", "an", "any", "also", "either", "neither",
];

/// The words that make a clause a sentence of its own, no list item
/// ([`denied`]).
const VERBS: &[&str] = &[
    "is", "are", "was", "were", "be", "been", "has", "have", "had", "will", "would", "can",
    "could", "do", "does", "did", "remains", "remain", "stays", "needs", "awaits", "waits",
];

/// The articles that open a new subject ([`denied`]).
const STRONG: &[&str] = &[
    "the", "this", "that", "these", "those", "its", "my", "our", "their", "his", "her", "your",
];

/// WHETHER A NEGATION DENIES THE ROW `st` in its clause (from `from`): a
/// negation word before it ([`negation`]) within its reach — which ends at a
/// contrast ([`CONTRASTS`]) and at an `and` that opens a new subject
/// (`nothing is pending and CI is running`) — or `not` directly before it
/// (`the tests are not flaky`), or `not … anything` (`did not leave
/// anything for later`). And across commas, a list that continues the
/// clause before: of predicates, whose clause holds nothing but rows and
/// [`GLUE`] (`nothing is pending, deferred or parked`), or of subjects, after
/// a verbless `no …` clause (`no TODO, FIXME or skipped test remains`). Never
/// a checklist's label (a clause a colon closes: `Failing tests: 0, skipped
/// tests: 3`) nor across an item a colon opens (`Unpushed commits: none,
/// unmerged PRs: 1`): an item's value denies nothing beyond its label.
fn denied(s: &str, from: usize, at: usize, st: &Stand<'_>, stands: &[Stand<'_>]) -> bool {
    let covered = |p: usize| stands.iter().any(|o| (o.at..o.end()).contains(&p));
    let before: Vec<(usize, &str)> = words_at(s, from, at).map(|(p, w)| (p, bare(w))).collect();
    let reach = reach(&before, stands);
    let near = &before[reach..];
    let has = |w: &str| near.iter().any(|&(_, x)| x == w);
    if near.iter().any(|&(_, w)| negation(w))
        || (!st.word.starts_with("not ") && near.last().is_some_and(|&(_, w)| w == "not"))
        || (has("not")
            && ["any", "anything", "anyone", "anybody"]
                .iter()
                .any(|w| has(w)))
    {
        return true;
    }
    let to = clause_end(s, st.end());
    if reach > 0 || s[to..].starts_with(':') {
        return false;
    }
    let tail: Vec<(usize, &str)> = words_at(s, st.end(), to)
        .map(|(p, w)| (p, bare(w)))
        .filter(|(_, w)| !w.is_empty())
        .collect();
    let listed =
        |words: &[(usize, &str)]| words.iter().all(|&(p, w)| covered(p) || GLUE.contains(&w));
    let mut cut = from;
    let mut current = before;
    let mut first = true;
    loop {
        if cut == 0 || !s[..cut].ends_with(',') {
            return false;
        }
        let start = clause_start(s, cut - 1);
        if s[..start].trim_end().ends_with(':') {
            return false;
        }
        let previous: Vec<(usize, &str)> = words_at(s, start, cut - 1)
            .map(|(p, w)| (p, bare(w)))
            .collect();
        let predicates = listed(&current) && (!first || listed(&tail));
        let joined = current
            .iter()
            .chain(if first { tail.iter() } else { [].iter() })
            .any(|&(_, w)| matches!(w, "or" | "nor" | "and"));
        let subjects = previous.len() >= 2
            && matches!(previous[0].1, "no" | "neither" | "zero")
            && !previous.iter().any(|&(_, w)| VERBS.contains(&w))
            && joined
            && !current
                .iter()
                .any(|&(_, w)| VERBS.contains(&w) || STRONG.contains(&w) || CONTRASTS.contains(&w));
        if !(predicates || subjects) {
            return false;
        }
        if previous.iter().any(|&(_, w)| negation(w)) {
            return true;
        }
        current = previous;
        cut = start;
        first = false;
    }
}

/// The words past which a negation governs nothing ([`reach`]), beside
/// [`CONTRASTS`]: a clause of its own a cause, a consequence or a condition
/// opens. `there is no reviewer so the docs branch is not merged` negates
/// the reviewer, never the merge.
const REACH_BREAKS: &[&str] = &[
    "so",
    "because",
    "since",
    "as",
    "then",
    "therefore",
    "hence",
    "which",
    "until",
    "unless",
    "when",
    "if",
];

/// Where a negation's reach begins among the words `before` a row, by
/// index ([`denied`]): past the last contrast ([`CONTRASTS`]), clause break
/// ([`REACH_BREAKS`]) or `and` that opens a new subject (`nothing is pending
/// and CI is running`).
fn reach(before: &[(usize, &str)], stands: &[Stand<'_>]) -> usize {
    let covered = |p: usize| stands.iter().any(|o| (o.at..o.end()).contains(&p));
    before
        .iter()
        .enumerate()
        .rev()
        .find(|&(i, &(_, w))| {
            CONTRASTS.contains(&w)
                || REACH_BREAKS.contains(&w)
                || (w == "and"
                    && before
                        .get(i + 1)
                        .is_some_and(|&(p, x)| !(covered(p) || GLUE.contains(&x) || negation(x))))
        })
        .map_or(0, |(i, _)| i + 1)
}

/// What a denied landing names as its subject when `no`, `zero` or
/// `neither` denies it ([`landing_denied`]): `no PR was merged`, `neither
/// change has landed` — never `no secrets were committed`.
const WORK_NOUNS: &[&str] = &[
    "pr", "prs", "mr", "mrs", "change", "changes", "commit", "commits", "branch", "branches",
    "fix", "fixes", "work", "code", "patch", "patches", "push", "merge", "part", "piece",
];

/// The words beside a denied landing that deny no landing of the work
/// ([`landing_denied`]): something else (`nothing else was pushed`, `no new
/// commit since`), an exception (`no branch other than main was pushed`),
/// or a manner (`no PR was merged without review`).
const BESIDE_LANDING: &[&str] = &[
    "else",
    "new",
    "more",
    "other",
    "further",
    "extra",
    "but",
    "except",
    "besides",
    "apart",
    "without",
    "since",
    "after",
    "afterwards",
    "anywhere",
    "elsewhere",
    "outside",
    "than",
];

/// What follows `push` or `merge` that makes it no landing step
/// ([`landing_denied`]): `no merge conflicts`.
const MERGE_THINGS: &[&str] = &[
    "conflict",
    "conflicts",
    "commit",
    "commits",
    "request",
    "requests",
    "queue",
    "train",
    "base",
    "tool",
    "button",
    "strategy",
];

/// WHETHER A NEGATION DENIES THE LANDING WORD `st` ([`Reads::Unlanded`]):
/// a negation within its reach in its clause ([`reach`]) — `nothing`,
/// `none`, `never`, `nobody`, `neither`, `nor`, or `no`, `zero` or `0` with
/// the work for its subject ([`WORK_NOUNS`]) — so the landing it would claim
/// did not happen: `nothing has been merged`, `the branch was never pushed`,
/// `no PR was merged`, `none of it is on origin/main`. A word for work done
/// ([`Reads::Undone`]) takes every negation only with the work or what a
/// change owes for its subject ([`OWED`]: `none of the docs are written`). Never a word after
/// `not` or a modal (`is not merged` is the unlanded row itself, `nothing is
/// waiting to be merged` denies the wait), nor one beside something else or
/// an exception ([`BESIDE_LANDING`]), and `push` or `merge` only right after
/// its negation (`no push was done`, never `no merge conflicts`).
fn landing_denied(
    s: &str,
    from: usize,
    at: usize,
    after: &[&str],
    st: &Stand<'_>,
    stands: &[Stand<'_>],
) -> bool {
    let before: Vec<(usize, &str)> = words_at(s, from, at).map(|(p, w)| (p, bare(w))).collect();
    let near: Vec<&str> = before[reach(&before, stands)..]
        .iter()
        .map(|&(_, w)| w)
        .collect();
    if near.last().is_some_and(|w| UNSETTLED.contains(w))
        || near.iter().chain(after).any(|w| BESIDE_LANDING.contains(w))
    {
        return false;
    }
    if matches!(st.word, "push" | "merge")
        && (!near.last().is_some_and(|w| negation(w))
            || after.first().is_some_and(|w| MERGE_THINGS.contains(w)))
    {
        return false;
    }
    let undone = matches!(st.reads(), Reads::Undone);
    near.iter().enumerate().any(|(i, w)| {
        let work = || {
            near[i + 1..]
                .iter()
                .any(|x| WORK_NOUNS.contains(x) || (undone && OWED.contains(x)))
                || WORK_NOUNS.contains(&st.word)
        };
        let any = matches!(
            *w,
            "nothing" | "none" | "never" | "nobody" | "noone" | "no-one" | "neither" | "nor"
        );
        (any && !undone) || ((any || matches!(*w, "no" | "zero" | "0")) && work())
    })
}

/// What a change owes that a denied word for work done can name as its
/// subject ([`Reads::Undone`]): `none of the docs are written`, `no tests
/// have been run`.
const OWED: &[&str] = &[
    "test",
    "tests",
    "docs",
    "doc",
    "documentation",
    "benchmark",
    "benchmarks",
    "changelog",
    "example",
    "examples",
    "readme",
    "migration",
    "port",
    "suite",
    "check",
    "checks",
    "review",
];

/// What a claim of the whole says before `but` or `bar` makes it an
/// exception ([`excepted`]): `all done but the changelog`.
const WHOLE: &[&str] = &[
    "done",
    "merged",
    "landed",
    "pushed",
    "finished",
    "complete",
    "everything",
    "all",
    "nothing",
    "main",
    "origin/main",
    "master",
];

/// Whether the exception row `st` is one here: `but` only right after a
/// claim of the whole ([`WHOLE`]: `all done but the changelog`, never
/// `merged, but the docs were already current`), and `bar`, `save`, `less`,
/// `beyond` or `without` only there or after a comma (`everything landed,
/// bar the release notes`, `all on main bar the docs`, `all on main, less the
/// docs`, `all on main, without the docs`; never `the status bar`, `save the
/// file`, `takes less time`), each before a thing named ([`DETERMINERS`] or a
/// number); an exception named as one, unless none is (`without exception`,
/// `no exception`). Every other exception row is one wherever it stands.
fn excepted(s: &str, st: &Stand<'_>) -> bool {
    let before = s[..st.at].trim_end();
    let prev = before.split_whitespace().last().map(bare);
    // What a file keeps of main's copy (`resolved as main's text exactly,
    // minus the 1015-10 entry`): a merge's resolution of one file, no
    // exception to the landing.
    let near: Vec<&str> = before.split_whitespace().rev().take(4).map(bare).collect();
    if near.windows(2).any(|w| {
        matches!(w[0], "text" | "file" | "copy") && matches!(w[1], "main's" | "origin/main's")
    }) {
        return false;
    }
    if st.word == "not the" {
        // A landing's own exception after a comma (`merged the fix, not the
        // docs`), never a place it was kept off (`on origin/main, not the
        // fork`) nor a description (`is not the cause`).
        let next = s[st.end()..].split_whitespace().next().map(bare);
        return before.ends_with(',')
            && landing_claimed(before)
            && !next.is_some_and(|n| {
                matches!(
                    n,
                    "branch" | "fork" | "worktree" | "feature" | "side" | "local" | "old" | "stale"
                )
            });
    }
    if matches!(
        st.word,
        "omission" | "omissions" | "holdout" | "holdouts" | "excluded" | "excepted"
    ) && prev.is_some_and(|p| matches!(p, "no" | "nothing" | "none" | "without" | "zero"))
    {
        // `with no omissions`, `nothing excluded`: none is made.
        return false;
    }
    if matches!(st.word, "exception" | "the exception") {
        // Never a noun's modifier (`the exception handler fix`).
        let next = s[st.end()..].split_whitespace().next().map(bare);
        return !prev.is_some_and(|p| matches!(p, "without" | "no" | "any"))
            && next.is_none_or(|n| {
                matches!(
                    n,
                    "" | "of"
                        | "to"
                        | "is"
                        | "was"
                        | "are"
                        | "were"
                        | "being"
                        | "here"
                        | "there"
                        | "remains"
                        | "stays"
                )
            });
    }
    if !matches!(
        st.word,
        "but" | "bar" | "save" | "less" | "beyond" | "without"
    ) {
        return true;
    }
    let next = s[st.end()..].split_whitespace().next().map(bare);
    if st.word == "without" {
        // Only a thing left out after a comma: `all on main, without the
        // docs`, never `ran on main without a crash`.
        return before.ends_with(',') && next.is_some_and(|n| STRONG.contains(&n));
    }
    let whole = prev.is_some_and(|p| WHOLE.contains(&p) && !before.ends_with(','));
    let after_comma = st.word != "but" && before.ends_with(',');
    (whole || after_comma)
        && next.is_some_and(|n| {
            DETERMINERS.contains(&n) || (!n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
        })
}

/// The words a none may pass over before what narrows it ([`qualified_nil`]):
/// `nothing else important`, `none of the remaining items are urgent`.
const NIL_GLUE: &[&str] = &[
    "else",
    "more",
    "really",
    "truly",
    "too",
    "very",
    "particularly",
    "terribly",
    "super",
    "of",
    "on",
    "the",
    "remaining",
    "items",
    "item",
    "issues",
    "issue",
    "left",
    "is",
    "are",
    "was",
    "were",
    "still",
    "open",
    "pending",
    "outstanding",
    "work",
    "tasks",
    "task",
    "things",
    "thing",
    "it",
    "them",
    "that",
    "which",
    "seems",
    "seem",
    "looks",
    "remains",
    "remain",
    "at",
    "all",
];

/// WHETHER A NONE IS NARROWED ([`Reads::Qualified`]): the row `st` follows,
/// in its clause, a none word (`nothing`, `none`, `no`, `zero`) and nothing
/// but [`NIL_GLUE`] — `nothing important`, `no real blockers`, `nothing more
/// today`, `none of the remaining items are urgent` — so the none concedes
/// what it leaves out. Never a word that names a thing's kind (`no major
/// version bump`), nor one after a verb (`no test fails today`).
fn qualified_nil(s: &str, st: &Stand<'_>) -> bool {
    let words: Vec<&str> = s[clause_start(s, st.at)..st.at]
        .split_whitespace()
        .map(bare)
        .filter(|w| !w.is_empty())
        .collect();
    let next = s[st.end()..].split_whitespace().next().map(bare);
    if st.word == "major" && next.is_some_and(|n| matches!(n, "version" | "release" | "bump")) {
        return false;
    }
    words.first().is_some_and(|w| {
        matches!(*w, "nothing" | "none" | "no" | "zero" | "nil")
            || (*w == "not" && words.get(1) == Some(&"much"))
    }) && words.len() <= 7
        && words[1..].iter().all(|w| NIL_GLUE.contains(w))
}

/// WHETHER A CLAUSE NAMES ONLY THE ITEM BEFORE IT ([`Reads::OnlyTail`]): the
/// row opens a clause after a comma (`the tag, nothing more`), and the clause
/// before it is a bare thing — at most five words, no verb, no landing, no
/// place on main and no none (`all on origin/main, nothing else` claims the
/// whole).
fn only_item(s: &str, st: &Stand<'_>) -> bool {
    let from = clause_start(s, st.at);
    if !s[from..st.at].trim().is_empty() {
        return false;
    }
    let Some(lead) = s[..from].trim_end().strip_suffix(',') else {
        return false;
    };
    let words: Vec<&str> = lead[clause_start(lead, lead.len())..]
        .split_whitespace()
        .map(bare)
        .filter(|w| !w.is_empty())
        .collect();
    !words.is_empty()
        && words.len() <= 5
        && !words.iter().any(|w| {
            verbish(w)
                || SHIPS.contains(w)
                || LANDED.contains(w)
                || MAINS.contains(w)
                || negation(w)
                || matches!(*w, "all" | "everything" | "both")
        })
        && !(0..words.len()).any(|i| onto_main(&words[i..]))
}

/// The leftover words a `nothing … but` names ([`nothing_but`]).
const LEFTOVER: &[&str] = &[
    "remains",
    "remain",
    "left",
    "pending",
    "outstanding",
    "open",
    "do",
    "land",
];

/// Whether `but` makes an exception to a denied leftover: a clause that
/// opens `nothing` or `none` and names what is left before it, no comma
/// between (`nothing remains but tagging the release`, `nothing is left but
/// the docs`, `nothing to do but tag the release`): nothing but X is X
/// left. `nothing changed but the docs` names no leftover.
fn nothing_but(s: &str, st: &Stand<'_>) -> bool {
    if st.word != "but" {
        return false;
    }
    let lead = s[clause_start(s, st.at)..st.at].trim_end();
    let words: Vec<&str> = lead.split_whitespace().map(bare).collect();
    let next = s[st.end()..]
        .split_whitespace()
        .map(bare)
        .find(|w| !w.is_empty());
    !lead.ends_with(',')
        && matches!(words.first(), Some(&("nothing" | "none")))
        && words.iter().any(|w| LEFTOVER.contains(w))
        && next.is_some_and(|n| !negation(n))
}

/// What a fault does ([`cannot_happen`]): `the race could not recur`.
const FAULTS: &[&str] = &[
    "recur", "happen", "occur", "deadlock", "race", "crash", "hang", "leak", "break", "fail",
    "regress", "overflow", "panic",
];

/// The rows a test's proof uses for the failure it shows ([`proves_by_failing`]).
const PROOF_ROWS: &[&str] = &[
    "fail",
    "fails",
    "failed",
    "failing",
    "still fail",
    "still fails",
    "still failing",
    "reverted",
];

/// What a fix is when a test's proof takes it out ([`proves_by_failing`]:
/// `fails when the fix is reverted`, `fails if the Contents rule is
/// removed`), never what the test runs beside (`fails if the cache is
/// removed`).
const FIX_NOUNS: &[&str] = &[
    "fix", "fixes", "change", "patch", "commit", "guard", "rule", "check",
];

/// The code before a fix that a test's proof fails on ([`proves_by_failing`]:
/// `fails on the old rule`), never an old host or toolchain (`fails on the
/// old macOS 14 runner`, `on the old Xcode`).
const OLD_CODE: &[&str] = &[
    "code",
    "rule",
    "rules",
    "reader",
    "filter",
    "logic",
    "implementation",
    "parser",
    "guard",
];

/// WHETHER THE FAILURE THE ROW AT `[at, end)` NAMES IS A TEST'S PROOF
/// ([`land_open`]), read in the row's own clause (`[from, to)` of the
/// sentence `s`) and never across a join between the proof and the failure
/// (`and`, `so`, [`CONTRASTS`]):
///
/// * a mutation, a mutant or a negative control before the failure, that
///   makes it fail (`a test that a mutation back to the old filter makes
///   fail`, `each of 10 mutants fails at least one test`, `the negative
///   control gave 5 FAIL`), never a check it names that fails itself (`the
///   mutation test still fails`);
/// * a test seen to fail first (`a failing test that was seen to fail
///   first`);
/// * a test that fails on the code before its fix (`fails on the old rule`,
///   [`OLD_CODE`]), or on main's code where the sentence says it passes
///   after the fix (`failed on origin/main's code … and pass after the
///   fix`);
/// * a test that fails when its fix is taken out (`fails when the fix is
///   reverted`, `fails without the fix`, [`FIX_NOUNS`]), that phrase's
///   `reverted` too — never a revert on its own (`the fix was reverted`).
///
/// Never beside a flake's words anywhere in the sentence (`its negative
/// control failed under load and passed when run alone`, `failed once`).
fn proves_by_failing(s: &str, (from, to): (usize, usize), (at, end): (usize, usize)) -> bool {
    let sentence: Vec<&str> = s
        .split_whitespace()
        .map(bare)
        .filter(|w| !w.is_empty())
        .collect();
    let pair = |a: &str, b: &[&str]| sentence.windows(2).any(|w| w[0] == a && b.contains(&w[1]));
    let flake = sentence.iter().any(|w| {
        matches!(
            *w,
            "flaky"
                | "flake"
                | "flakes"
                | "flaked"
                | "flaking"
                | "intermittent"
                | "intermittently"
                | "sporadically"
                | "rerun"
                | "re-run"
                | "retry"
                | "retried"
                | "isolation"
        )
    }) || pair("under", &["load"])
        || pair("run", &["alone"])
        || pair("ran", &["alone"])
        || ["fail", "fails", "failed", "passed", "passes"]
            .iter()
            .any(|f| pair(f, &["once", "alone"]));
    if flake {
        return false;
    }
    let raw: Vec<(usize, &str)> = words_at(s, from, to).collect();
    let w: Vec<&str> = raw.iter().map(|&(_, r)| bare(r)).collect();
    let fail = |x: &str| matches!(x, "fail" | "fails" | "failed" | "failing");
    let (Some(first), Some(last)) = (
        raw.iter().position(|&(p, r)| p + r.len() > at),
        raw.iter().rposition(|&(p, _)| p < end),
    ) else {
        return false;
    };
    // The failure's own word: the row's last, or the one after a bare
    // `still`.
    let f = if fail(w[last]) || w[last] == "reverted" {
        last
    } else if w.get(last + 1).is_some_and(|n| fail(n)) {
        last + 1
    } else {
        return false;
    };
    let joined = |a: usize, b: usize| {
        w[a.min(b) + 1..a.max(b)]
            .iter()
            .any(|x| CONTRASTS.contains(x) || matches!(*x, "and" | "or" | "so" | "then"))
    };
    let undone = |x: &str| matches!(x, "reverted" | "removed" | "undone");
    let taken_out = (0..w.len()).any(|i| {
        matches!(w[i], "when" | "if" | "once")
            && (i + 1..w.len().min(i + 7)).any(|r| {
                undone(w[r])
                    && w[i + 1..r].iter().any(|x| FIX_NOUNS.contains(x))
                    && (0..i).any(|g| fail(w[g]) && (g == f || r == f) && !joined(g, r))
            })
    }) || matches!(
        w.get(f + 1..),
        Some(["without", "the" | "its" | "that" | "this", n, ..]) if FIX_NOUNS.contains(n)
    ) || matches!(
        w.get(f + 1..),
        Some(["with", "the" | "its" | "that" | "this", n, u, ..])
            if FIX_NOUNS.contains(n) && undone(u)
    );
    if w[f] == "reverted" {
        return taken_out;
    }
    // A proof word naming a check that fails itself (`the mutation test
    // still fails`), never one that makes a test fail (`a mutation check that
    // makes its test fail`).
    let check = |i: usize| {
        w.get(i + 1).is_some_and(|n| {
            matches!(
                *n,
                "test" | "tests" | "job" | "jobs" | "suite" | "check" | "checks" | "lane"
            )
        }) && w.iter().skip(i + 2).take(2).any(|x| fail(x))
    };
    let agent = (0..first).any(|i| {
        let noun = matches!(w[i], "mutation" | "mutations" | "mutant" | "mutants")
            || (matches!(w[i], "control" | "controls") && i > 0 && w[i - 1] == "negative");
        noun && !check(i) && !joined(i, first)
    });
    let seen = (0..w.len().saturating_sub(2)).any(|i| {
        matches!(w[i], "seen" | "shown" | "watched")
            && w[i + 1] == "to"
            && w[i + 2] == "fail"
            && (i + 2 == f || (f < i && !joined(f, i)))
    });
    let after_fix = sentence.windows(4).any(|x| {
        matches!(x[0], "pass" | "passes")
            && matches!(x[1], "after" | "with")
            && matches!(x[2], "the" | "its" | "this")
            && x[3] == "fix"
    });
    let on_old = match w.get(f + 1..) {
        Some(["on" | "against", "the", old, code, ..]) => {
            matches!(
                *old,
                "old" | "unfixed" | "original" | "previous" | "pre-fix"
            ) && OLD_CODE.contains(code)
        }
        Some(["on" | "against", "main's" | "origin/main's", "code", ..]) => after_fix,
        _ => false,
    };
    agent || seen || on_old || taken_out
}

/// What names one of the task's own items, which a report of it skipped
/// leaves undone (`Item 7 is skipped`, [`land_open`]), where `a frame is
/// skipped` describes code.
const TASK_ITEMS: &[&str] = &["item", "items", "finding", "findings"];

/// Whether what follows `could not` (`after`, the rest of its clause) says
/// a fault cannot happen, a description of the fix and no check that could
/// not be made: `the race could not recur in 500 runs`.
fn cannot_happen(after: &str) -> bool {
    let mut words = after.split_whitespace().map(bare);
    words.next().is_some_and(|w| FAULTS.contains(&w))
}

/// What a negation passes over without denying anything ([`excepting`]):
/// `none of it`, `nothing else`.
const IDLE: &[&str] = &["else", "more", "other", "at", "all", "of", "it", "them", ""];

/// WHETHER AN EXCEPTION OPENS WORK in the sentence `s` (`all merged except
/// the docs`, `everything is on main, save for the migration`): always,
/// unless the sentence denies a plain verb (`aside from those three nothing
/// was touched`, `other than main, no branch exists`) and holds no other
/// row. A bare `none, apart from the migration` denies nothing, nor does a
/// negation in a relative clause (`apart from a glitch that nobody will
/// notice`), and
/// `nothing remains except the tag` keeps the tag: an exception to a denied
/// leftover is that leftover.
fn excepting(s: &str, stands: &[Stand<'_>]) -> bool {
    if stands
        .iter()
        .any(|st| !matches!(st.reads(), Reads::Exception) && !st.reads().common())
    {
        return true;
    }
    !s.split(CLAUSE_MARKS).any(|clause| {
        let words: Vec<&str> = clause.split_whitespace().map(bare).collect();
        words.iter().enumerate().any(|(i, w)| {
            negation(w)
                && *w != "without"
                && !words[..i]
                    .iter()
                    .any(|x| matches!(*x, "that" | "which" | "who"))
                && words[i + 1..].iter().any(|x| !IDLE.contains(x))
        })
    })
}

/// The words a predicate standing alone may meet next ([`alone`]): `remains
/// in parser.rs`, `remains to be done`, `stays until the fix ships`.
const PREPOSITIONS: &[&str] = &[
    "in", "on", "for", "to", "with", "at", "from", "until", "till", "after", "before", "behind",
    "over", "under",
];

/// Whether a [`Reads::Alone`] row stands alone, `after` it the rest of its
/// clause: nothing, or a preposition ([`PREPOSITIONS`]) — never a complement
/// (`remains backward compatible`, `remains the default`).
fn alone(after: &str) -> bool {
    after
        .split_whitespace()
        .map(bare)
        .find(|w| !w.is_empty())
        .is_none_or(|w| PREPOSITIONS.contains(&w))
}

/// What waits as a mechanism, describing code ([`Reads::Until`]): `the
/// writer waits until the flush finishes`, `the lock holds until release`.
const MECHANISMS: &[&str] = &[
    "waits", "wait", "waiting", "blocks", "block", "blocking", "retries", "retry", "spins", "spin",
    "loops", "loop", "sleeps", "sleep", "polls", "poll", "holds", "hold", "buffers", "queues",
    "reads", "scrolls", "runs", "repeats", "backs",
];

/// What says work happens with nobody doing it ([`Reads::Later`]): `the
/// nightly job picks it up tomorrow on its own`.
const AUTOMATIC: &[&str] = &[
    "on its own",
    "on their own",
    "by itself",
    "by themselves",
    "automatically",
    "unattended",
];

/// What denies an intent after it ([`Reads::Intent`]): `I will start no new
/// work`.
const DENIALS: &[&str] = &["no", "not", "nothing", "never", "none", "nobody"];

/// Whether `at` stands inside quotation marks in `text`.
fn quoted(text: &str, at: usize) -> bool {
    let before = &text[..at];
    before.matches('"').count() % 2 == 1 || before.rfind('“') > before.rfind('”')
}

/// Whether what follows a status word gives a reason that leaves nothing to
/// do: one that opens with a negation after a colon, semicolon, bracket or
/// full stop (`not run: no contract was added`, `not run. No contract was
/// added.`), or a `because`, `since` or `as` clause that holds one or
/// `already` (`not run, because the diff adds no contract`, `not done,
/// because main already has it`). Never one that says only that the work
/// takes long ([`ONLY_SLOW`]: `not run, because there was no time`), nor
/// one that names what is lacking ([`LACKING`]: `not reproduced: no mount is
/// available`, `not measured (no windows allowed)`): a negated blocker is
/// still a blocker, and the work it blocks is open (the owner's rule of
/// 2026-10-19).
fn nothing_to_do(after: &str) -> bool {
    let after = after.trim_start();
    let explained = after.starts_with([':', ';', '(', '.']);
    let reason = after.trim_start_matches(|c: char| {
        matches!(c, ',' | ':' | ';' | '(' | '.') || c.is_whitespace()
    });
    let because = ["because ", "since ", "as "]
        .iter()
        .find_map(|w| reason.strip_prefix(w));
    let clause = because
        .unwrap_or(reason)
        .split([',', ';', '.'])
        .next()
        .unwrap_or("");
    if reason_words(clause).any(|w| ONLY_SLOW.contains(&w) || LACKING.contains(&w)) {
        return false;
    }
    if because.is_some() {
        return reason_words(clause).any(|w| OPEN_NEGATIONS.contains(&w))
            || clause
                .split(|c: char| !word_char(c))
                .any(|w| w == "already");
    }
    explained
        && reason_words(reason)
            .next()
            .is_some_and(|w| OPEN_NEGATIONS.contains(&w))
}

/// The words of a reason ([`nothing_to_do`]), `n/a` one of them.
fn reason_words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| c.is_whitespace() || matches!(c, ',' | '.' | ';' | ':' | '(' | ')'))
        .filter(|w| !w.is_empty())
}

/// What a reason says when it says only that the work takes long
/// ([`nothing_to_do`]): the owner's rule takes no slowness for a blocker, so
/// `not run, because there was no time` leaves the run to do.
const ONLY_SLOW: &[&str] = &["time", "slow", "long", "minutes", "hours"];

/// What a negated reason names when it names a lack, not an absence of work
/// ([`nothing_to_do`]): `no mount is available`, `no windows allowed`, `no
/// GPU on this host`. `no contract was added` names none, and leaves
/// nothing to do.
const LACKING: &[&str] = &[
    "available",
    "allowed",
    "permitted",
    "installed",
    "here",
    "host",
    "machine",
    "access",
    "window",
    "windows",
    "mount",
    "gpu",
    "credential",
    "credentials",
    "memory",
    "ram",
    "disk",
];

/// What a defense (or a done claim) under Land still names open (`words::clean_defense`'s
/// strict reading), for the next nudge's suffix ([`open_work_suffix`]); empty where the reply
/// holds neither (a plain report of progress names its own next steps). A done claim with no
/// defense line is read without the bare-push rule ([`open_work_read_as`]): it never lands, so
/// its words name only the suffix, the same at a run loop's first point as at a later one.
#[must_use]
pub fn land_open_words(tail: Option<&str>) -> Vec<&'static str> {
    match tail {
        Some(t) if needs::defense_line(t).is_some() => {
            open_work_strict(Some(&needs::defense_body(t)))
        }
        Some(t) if says_done(Some(t)) => {
            open_work_read_as(&needs::without_needs(t), Some(&LAND_FAMILIES), false)
        }
        _ => Vec::new(),
    }
}

/// The most of [`open_work`]'s words the open-work suffix names ([`open_work_suffix`]): the
/// act stays one short line however many heads a report uses.
pub const OPEN_NAMED: usize = 4;

/// THE OPEN-WORK SUFFIX (the defend prompt and the Land nudge where the claim or the defense
/// named open work): the words [`open_work`] (or [`open_work_strict`]) found, in its own
/// tables' spelling and never the worker's (no act types the worker's words back: aterm's
/// ruling of 2026-10-08), at most [`OPEN_NAMED`] of them, `…` after the rest. `None` where
/// nothing was found.
#[must_use]
pub fn open_work_suffix(found: &[&str]) -> Option<String> {
    open_named(found, OPEN_NAMED)
}

/// [`open_work_suffix`] naming at most `most` of `found` (at least one): aterm's `open_named`,
/// through which its act fitting names fewer ([`crate::words::fitted_act`]).
#[must_use]
pub fn open_named(found: &[&str], most: usize) -> Option<String> {
    let most = most.max(1);
    let named = match found.get(..most) {
        _ if found.is_empty() => return None,
        Some(first) if found.len() > most => format!("{}, …", first.join(", ")),
        _ => found.join(", "),
    };
    Some(format!(
        "Your report still names open work ({named}): finish and land it first."
    ))
}

#[cfg(test)]
#[path = "open_work_land_tests.rs"]
mod land_reading;
