// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! WHAT THE THREAD'S REPOSITORY HOLDS THAT NO REMOTE MAIN HAS: the probe behind Land's fourth
//! condition and Cruise's repository suffix (FOREVER CRUISE / FOREVER LAND, aterm owner
//! direction 2026-10-19: "if landing LAND EVERYTHING INTO REMOTE MAIN NO EXCEPTIONS").
//!
//! [`probe`] runs beside the runtime, never in the pure decider and never on a runtime worker:
//! its reads go on on a thread of their own ([`probe_session_in`], aterm's `Probing::ask`), so a
//! synchronous file system call that blocks (a stat that a link leads onto a stalled share) holds
//! that thread alone, and the caller has its answer at [`PROBE_LIMIT`] however long the reads
//! take; every child is killed when the probe is dropped or its limit passes (review of the
//! forever probe, hangs-1). It reads EVERY FOLDER THE SESSION TOUCHED ([`probe_session`]: each
//! folder a command ran in, `cd`ed into or pointed git at ([`command_reach`]), each folder an
//! edit landed in, and the thread's own working directory, `thread_settings_snapshot().cwd`),
//! each repository once: the outermost project a folder's repository is a submodule of (`git
//! rev-parse --show-superproject-working-tree`, at most [`SUPER_CLIMB`] up) and that repository
//! too, as aterm main `801e72779e` reads them (review of the forever probe,
//! land-probe-last-workdir; aterm's D2-4). The thread's own folder's repositories are read
//! whole; a repository only another folder led to holds work only where the session changed it
//! since it began ([`changed_since`]; review of the forever probe at f48a215d7b, R2), so a
//! dependency's checkout it merely looked in holds nothing. It looks for
//!
//! * tracked changes, staged or not (`git status --porcelain=v1 -z`, `XY` other than `??`);
//! * untracked files git does not ignore (`??`);
//! * stash entries made on `HEAD`'s branch or on a branch the session used in that repository ([`Used`]; `git
//!   stash list`, by each entry's `WIP on <branch>:` or `On <branch>:` subject, [`stashes_on`];
//!   on a detached `HEAD`, `(no branch)`): a repository's stash is shared by all its worktrees,
//!   and an entry on a branch the session never used is another session's work;
//! * commits on `HEAD` no remote-tracking ref contains (`git rev-list --count HEAD --not
//!   --remotes`), counted where the repository has a remote-tracking ref or a configured
//!   remote (`git remote`): a remote configured but never fetched from or pushed to lacks every
//!   commit on `HEAD` (review of the forever probe, land-probe-unfetched-remote; aterm's D2-5);
//!   a repository with no remote at all has nowhere to push, and a branch with no commit yet has
//!   nothing to push ([`branch_unborn`]);
//! * commits on `HEAD` the remote's DEFAULT BRANCH does not contain ([`RepoWork::unlanded`]):
//!   `refs/remotes/origin/HEAD` where it is set, else `origin/main`, else `origin/master`; with
//!   none of them the count is unknown and finds nothing. A commit pushed only to a feature
//!   branch is unlanded;
//! * where the repository has a remote, the commits each populated submodule (and each of
//!   theirs) holds at its checkout or at the commit its superproject records that no remote of
//!   that submodule has ([`RepoWork::sub_unpushed`], [`submodules_unpushed`]): a superproject
//!   that pushed its record of one leaves the remote main pointing at a commit nobody can fetch
//!   (review of the forever probe, land-probe-submodule-unpushed; aterm's D2-1);
//! * where the repository has a remote, the commits on the branches the session used
//!   ([`Used`]: those this worktree's own `HEAD` reflog moved from or to since the session's
//!   start, and those its own commands created or checked out in that repository) that neither `HEAD` nor the
//!   remote main has ([`RepoWork::parked`], [`parked_on`]; review of the forever probe,
//!   land-probe-head-only; aterm's D2-3). A branch the session never used, another worktree's
//!   moves and work that predates the session stay out: they are another session's.
//!
//! A completed probe returns `RepoRead::Read(None)` or `RepoRead::Read(Some(work))`: the first
//! repository that holds work, read across the others too (its state, [`RepoWork::state`],
//! hashes every finding's). A deadline, interrupted child, failed or refused read, or more
//! folders than [`PROBE_DIRS`], where no repository holds work, returns [`RepoRead::Unread`],
//! which never proves a clean or unchanged repository. Every read is offline where it can
//! fetch (`GIT_NO_LAZY_FETCH`): nothing is fetched. A verified non-repository, and folders the
//! unattended
//! probe cannot enter under its existing guards, find nothing: a folder macOS guards with
//! a consent dialog, or one that does not resolve over local mounts alone
//! ([`unattended_dir`]: a network share, an automounted volume, or a link onto one).
//! Those folders are never touched. The reads take no optional lock
//! (`GIT_OPTIONAL_LOCKS=0`), disable
//! fsmonitor, hooks and pagers, and pass no inherited environment and no system or global Git
//! configuration; repository-local configuration still applies. Each read's `HOME` is the
//! person's home, for the `~/` a repository's own path values may hold (without it the
//! probe is unread; a failed path expansion must never prove a dirty repository clean), and
//! its `XDG_CONFIG_HOME` points at nothing, so Git's default files under it are not read
//! unchecked. ONE GLOBAL SETTING IS: the person's global excludes file ([`global_excludes`]:
//! `core.excludesFile` from their global configuration and the files it includes, else Git's
//! default `$XDG_CONFIG_HOME/git/ignore`), handed to the status reads alone (`-c
//! core.excludesFile=`), so a file they ignore everywhere (`.DS_Store`) is no untracked work
//! here, as their own `git status` says, and only where the repository sets no
//! `core.excludesFile` of its own ([`own_excludes`]), which Git reads instead of theirs. Only
//! that one value is read from their configuration, by `git config --global --includes --path
//! --get` run in the repository (its conditional includes judged against it), which runs
//! nothing.
//!
//! A finding carries THE WORK'S STATE ([`RepoWork::state`], [`state_of`]): `HEAD`, the
//! branch's stash entries and every changed or untracked file's status, size and modification
//! time, so two probes tell apart work their counts cannot ([`RepoWork::same`]): a landed
//! task's own wake keeps its landing over work it found unchanged (`decide`).
//!
//! PROVENANCE: ported from `~/aterm/crates/aterm-agent/src/harness/repo_work.rs` (aterm
//! `origin/main`, the probe of 4bff4848b4) with the `unlanded` count of the FOREVER CRUISE /
//! FOREVER LAND spec's §9 rule 2, which aterm's port adds beside it; then as aterm main
//! `e0da8d68df` has it: a branch's own stashes and the person's global excludes (d88a7bf289,
//! the review's #6 and #7), the repository's own excludes first and the global read's
//! includes (02cb768483), that read in the repository and `HOME` on every read (fabf0a765a),
//! and the work's state (25cde204a6); then as aterm main `6ff0b5475a` has it, diffed item by
//! item: the main ref a probe reads (`origin/HEAD` where it names a commit, else
//! `origin/main`, `origin/master`), its unlanded count kept as given and named by that ref.
//! The timeout now remains an explicit unread result through the decider and its words
//! (aterm e32ba59da2's rule); this typed port retains codex-alab's existing unavailable-folder policy.
//! The guarded folders are `atpkg::protected`'s. A folder is entered as aterm main 240d2cb43c
//! enters it (`unattended_dir`, `global_excludes`): resolved component by component over the
//! mount table, local mounts alone ([`crate::mounts::resolve_local`], parity review P2; it
//! replaces a lexical check before and after `std::fs::canonicalize`, which followed a link
//! onto a share and could block on a stalled one).
//! The unread result and same-work rule port aterm `e32ba59da2`; failed or interrupted
//! Git children are unread too. Two stricter guards are kept for the reasons their own
//! docs give: the guarded folders read in the file system's own
//! spelling too ([`canonical_outside`]), and `safe.bareRepository=explicit` ([`git_as`]).
//! Then, after the review of the forever probe (owner, 2026-10-19, restated 2026-10-29: "if
//! landing LAND EVERYTHING INTO REMOTE MAIN NO EXCEPTIONS"), as aterm main `b8332c44cf` has it:
//! a submodule's commits on no remote, the session's own branches and their stash entries, the
//! outermost project with the submodule the session worked in, and every commit on `HEAD` where
//! a remote is configured but never fetched; codex-alab reads every folder the session touched
//! where aterm reads its last one, and runs the reads off the runtime as aterm's
//! `Probing::ask` does.

use std::collections::HashSet;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use codex_alab_core::session::UsedBranch;
use codex_alab_core::session::note_branch;

use crate::mounts::Table;
use crate::mounts::resolve_local;

#[path = "repo_cargo_copy.rs"]
mod cargo_copy;

/// How long the whole probe may take ([`probe`]): every call shares it. A probe it cuts is
/// [`RepoRead::Unread`] and holds a landing, so it is a HANG DETECTOR, NEVER A BUDGET (aterm
/// `710345cbf9`): a healthy probe of a large repository on a loaded Mac took 8 to 12 s, and a
/// 10 s limit read it unread every time, holding every landing over it for good.
pub const PROBE_LIMIT: Duration = Duration::from_secs(60);

/// The limit of the start probe over a landing (`ThreadState::wake_began_over`), which the turn
/// waits for before it runs: a hung `git` holds the turn this long at most. A start probe it
/// cuts is [`RepoRead::Unread`], which proves nothing unchanged ([`RepoRead::same`]): the wake
/// keeps its landing only over a repository it leaves clean.
pub const START_PROBE_LIMIT: Duration = Duration::from_secs(10);

/// The most paths a finding names ([`RepoWork::paths`]).
pub const NAMED_PATHS: usize = 3;

/// The most files the work's state is read over ([`RepoWork::state`]): past that it is not
/// read.
pub const STATE_FILES: usize = 4096;

/// The longest path, in characters, a finding names whole: a longer one keeps its last
/// characters after `…`.
const PATH_CHARS: usize = 72;

/// The per-folder services macOS guards under `$HOME` (`atpkg::protected::HOME_FOLDERS`).
const HOME_FOLDERS: &[&str] = &[
    "Documents",
    "Desktop",
    "Downloads",
    "Pictures",
    "Movies",
    "Music",
];

/// The file-provider domains under `~/Library` (`atpkg::protected::LIBRARY_DOMAINS`).
const LIBRARY_DOMAINS: &[(&str, &str)] =
    &[("Library", "Mobile Documents"), ("Library", "CloudStorage")];

/// Roots guarded wherever `$HOME` is (`atpkg::protected::ABSOLUTE_ROOTS`).
const ABSOLUTE_ROOTS: &[&str] = &["/Volumes"];

/// The remote default branches tried after `refs/remotes/origin/HEAD`, in order.
const DEFAULT_BRANCHES: &[&str] = &["refs/remotes/origin/main", "refs/remotes/origin/master"];

/// The most folders one probe reads ([`probe_session`]): past them it is unread. A scope's every
/// folder, the last command's and the thread's own.
pub const PROBE_DIRS: usize = codex_alab_core::session::PROBE_SCOPE_DIRS + 2;

/// The most superprojects [`outermost`] climbs through: past them the read is cut.
const SUPER_CLIMB: usize = 8;

/// The most branches [`Used`] names: the newest.
pub const USED_BRANCHES: usize = 64;

/// The most probe readers alive at once in this process ([`probe_session_in`]): one left reading
/// past its limit (a stat blocked on a stalled share) holds a slot until it ends, and past them a
/// probe is unread at once rather than a thread more. Room for every thread's paced asks over a
/// share stalled for an hour, and for a test binary's parallel probes, whose spurious unread would
/// fail a test that reads clean.
const PROBE_READERS: usize = 64;

/// How long past its limit a probe's caller waits for its reader's own answer (a read cut at
/// the limit answers itself) before it answers unread and leaves the reader behind.
const READER_GRACE: Duration = Duration::from_secs(2);

/// The most paths one `ls-files` read of [`submodule_records`] names.
const RECORD_PATHS: usize = 256;

/// The most of a shallow clone's `shallow` file [`shallow_roots`] reads.
const SHALLOW_BYTES: u64 = 16 * 1024 * 1024;

/// The most of a repository's `FETCH_HEAD` file [`remote_fetch_heads`] reads.
const FETCH_HEAD_BYTES: u64 = 4 * 1024 * 1024;

/// The most configured remotes [`remote_fetch_heads`] reads fetch URLs of.
const FETCH_REMOTES: usize = 32;

/// The most fetched commit tips [`remote_fetch_heads`] resolves.
const FETCH_HEAD_COMMITS: usize = 256;

/// The most commits one read of [`submodules_unpushed`] or [`parked_on`] lists.
const BEYOND_COMMITS: usize = 4096;

/// Where `origin`'s remote-tracking branches are, the remote [`main_ref`] reads.
const ORIGIN_REFS: &str = "refs/remotes/origin/";

/// A completed repository probe, or a cut, interrupted or failed read. An unread
/// repository never supplies evidence for a Land landing or unchanged work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoRead<T = RepoWork> {
    Read(Option<T>),
    Unread,
}

impl<T> Default for RepoRead<T> {
    fn default() -> Self {
        Self::Read(None)
    }
}

impl<T> From<Option<T>> for RepoRead<T> {
    fn from(found: Option<T>) -> Self {
        Self::Read(found)
    }
}

impl<T> RepoRead<T> {
    #[must_use]
    pub fn as_ref(&self) -> RepoRead<&T> {
        match self {
            Self::Read(found) => RepoRead::Read(found.as_ref()),
            Self::Unread => RepoRead::Unread,
        }
    }

    #[must_use]
    pub fn is_clean(&self) -> bool {
        matches!(self, Self::Read(None))
    }

    #[must_use]
    pub fn filter(self, keep: impl FnOnce(&T) -> bool) -> Self {
        match self {
            Self::Read(found) => Self::Read(found.filter(keep)),
            Self::Unread => Self::Unread,
        }
    }
}

impl RepoRead<&RepoWork> {
    #[must_use]
    pub fn cloned(self) -> RepoRead {
        match self {
            Self::Read(found) => RepoRead::Read(found.cloned()),
            Self::Unread => RepoRead::Unread,
        }
    }

    #[must_use]
    pub fn same(a: Self, b: Self) -> bool {
        match (a, b) {
            (Self::Read(a), Self::Read(b)) => RepoWork::same(a, b),
            _ => false,
        }
    }

    #[must_use]
    pub fn summary(self) -> String {
        match self {
            Self::Read(Some(work)) => work.summary(),
            Self::Read(None) => "nothing found".to_string(),
            Self::Unread => "repository read did not finish".to_string(),
        }
    }
}

/// WHAT ONE REPOSITORY HOLDS THAT NO REMOTE MAIN HAS ([`probe`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoWork {
    /// The repository's top-level directory, as `git rev-parse --show-toplevel` names it.
    pub repo: String,
    /// Tracked paths with changes, staged or not.
    pub changed: usize,
    /// Untracked paths git does not ignore (a directory with nothing tracked in it is one).
    pub untracked: usize,
    /// Stash entries.
    pub stashes: usize,
    /// Commits on `HEAD` no remote-tracking ref contains.
    pub unpushed: usize,
    /// Commits on `HEAD` the remote's default branch ([`Self::main_ref`]) does not contain,
    /// pushed elsewhere or not at all; zero where no default branch is known.
    pub unlanded: usize,
    /// The remote default branch [`Self::unlanded`] counts against, as `git` names it short
    /// (`origin/main`); empty where none is known.
    pub main_ref: String,
    /// Commits a populated submodule's checkout, or the commit recorded for it, holds that no
    /// remote of the submodule has, at every level of submodules ([`submodules_unpushed`]): a
    /// pushed record of one nobody can fetch.
    pub sub_unpushed: usize,
    /// The submodules [`Self::sub_unpushed`] counts, the first [`NAMED_PATHS`], each printable.
    pub subs: Vec<String>,
    /// Commits on another branch the session used ([`Used`]), local or `origin`'s of that name,
    /// that neither `HEAD` nor the remote main ([`Self::main_ref`]; with none, any remote) has
    /// ([`parked_on`]).
    pub parked: usize,
    /// The branches [`Self::parked`] counts, the first [`NAMED_PATHS`] by name, each printable.
    pub parked_on: Vec<String>,
    /// The first [`NAMED_PATHS`] changed or untracked paths, each printable ([`printable`]).
    pub paths: Vec<String>,
    /// THE WORK'S STATE, hashed ([`state_of`]): `HEAD`'s commit, the stash entries of its
    /// branch, and every changed or untracked file git does not ignore (an untracked
    /// directory's files each), by its status code, its size and its modification time. Two
    /// findings with the same counts and the same named paths can hold different work (an
    /// edit to a file already changed, a new file in a directory already untracked, a path
    /// past the named ones), and only this tells them apart ([`Self::same`]). `None`: not read
    /// (more than [`STATE_FILES`] files, a read that failed, a finding a test made).
    pub state: Option<u64>,
}

impl RepoWork {
    /// Whether anything was found.
    #[must_use]
    pub fn found(&self) -> bool {
        self.changed > 0
            || self.untracked > 0
            || self.stashes > 0
            || self.unpushed > 0
            || self.unlanded > 0
            || self.sub_unpushed > 0
            || self.parked > 0
    }

    /// The commits it found on no remote main however it found them: on `HEAD`, in a
    /// submodule, or on a branch the session used (the `unpushed=` aterm's hold names).
    #[must_use]
    pub fn commits_unpushed(&self) -> usize {
        self.unpushed
            .saturating_add(self.sub_unpushed)
            .saturating_add(self.parked)
    }

    /// The finding of the reads' output: `repo` the top level, `status` the NUL-separated
    /// `git status --porcelain -z`, `stash` the stash entries of `HEAD`'s branch, one a line
    /// ([`stashes_on`]), `unpushed` the count of commits no remote-tracking ref contains,
    /// `unlanded` the count of commits the remote default branch `main_ref` (short,
    /// `origin/main`; empty where none is known) does not contain. `None` where nothing was
    /// found.
    #[must_use]
    pub fn of_git(
        repo: &str,
        status: &str,
        stash: &str,
        unpushed: usize,
        unlanded: usize,
        main_ref: &str,
    ) -> Option<Self> {
        let mut work = Self {
            repo: printable(repo.trim_end_matches(['\n', '\r'])),
            stashes: stash.lines().filter(|l| !l.trim().is_empty()).count(),
            unpushed,
            unlanded,
            main_ref: printable(main_ref),
            ..Self::default()
        };
        let mut fields = status.split('\0');
        while let Some(entry) = fields.next() {
            let (Some(code), Some(path)) = (entry.get(..2), entry.get(3..)) else {
                continue;
            };
            if code == "??" {
                work.untracked += 1;
            } else if code != "!!" {
                work.changed += 1;
                // A rename or a copy names its source in the next field.
                if code.starts_with(['R', 'C']) {
                    fields.next();
                }
            } else {
                continue;
            }
            if work.paths.len() < NAMED_PATHS && !path.is_empty() {
                work.paths.push(printable(path));
            }
        }
        work.found().then_some(work)
    }

    /// WHETHER TWO PROBES FOUND THE SAME WORK, nothing in it touched between them: both found
    /// nothing, or both found the same finding and the same known state ([`Self::state`]). A
    /// state not read on either side says nothing of what changed between the two, so the
    /// work is not the same: an edit the counts cannot see is never ruled out.
    #[must_use]
    pub fn same(a: Option<&Self>, b: Option<&Self>) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => a.state.is_some() && a == b,
            _ => false,
        }
    }

    /// The finding in a few words, for the suffix and the journal: `/r: 2 changed, 1
    /// untracked (a.rs, b/, …), 1 stash, 3 unpushed commits, 4 commits not on origin/main`.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.changed > 0 {
            parts.push(format!("{} changed", self.changed));
        }
        if self.untracked > 0 {
            parts.push(format!("{} untracked", self.untracked));
        }
        let mut text = parts.join(", ");
        if !self.paths.is_empty() {
            let more = if self.changed.saturating_add(self.untracked) > self.paths.len() {
                ", …"
            } else {
                ""
            };
            text = format!("{text} ({}{more})", self.paths.join(", "));
        }
        let plural = |n: usize| if n == 1 { "" } else { "s" };
        let mut tail: Vec<String> = Vec::new();
        if self.stashes > 0 {
            tail.push(format!(
                "{} stash{}",
                self.stashes,
                if self.stashes == 1 { "" } else { "es" }
            ));
        }
        if self.unpushed > 0 {
            tail.push(format!(
                "{} unpushed commit{}",
                self.unpushed,
                plural(self.unpushed)
            ));
        }
        if self.sub_unpushed > 0 {
            tail.push(format!(
                "{} unpushed commit{} in {}",
                self.sub_unpushed,
                plural(self.sub_unpushed),
                self.subs.join(", ")
            ));
        }
        if self.unlanded > 0 {
            tail.push(format!(
                "{} commit{} not on {}",
                self.unlanded,
                plural(self.unlanded),
                if self.main_ref.is_empty() {
                    "the remote main"
                } else {
                    &self.main_ref
                }
            ));
        }
        if self.parked > 0 {
            tail.push(format!(
                "{} commit{} on {} not on {}",
                self.parked,
                plural(self.parked),
                self.parked_on.join(", "),
                if self.main_ref.is_empty() {
                    "any remote"
                } else {
                    &self.main_ref
                }
            ));
        }
        let all: Vec<String> = [text]
            .into_iter()
            .filter(|t| !t.is_empty())
            .chain(tail)
            .collect();
        format!("{}: {}", self.repo, all.join(", "))
    }
}

/// `text` with every control character as `?`, and kept to [`PATH_CHARS`] characters (its
/// end, after `…`): a path a typed line names can neither end the line nor fill the prompt.
#[must_use]
pub fn printable(text: &str) -> String {
    let clean: String = text
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect();
    let n = clean.chars().count();
    if n <= PATH_CHARS {
        return clean;
    }
    let tail: String = clean.chars().skip(n - (PATH_CHARS - 1)).collect();
    format!("…{tail}")
}

/// WHAT A SESSION USED BEYOND THE FOLDERS IT IS READ WHOLE IN (aterm's `Used`,
/// [`probe_session`]): the branches it parked work on are these, never another session's, and
/// the other folders it touched are read only where it changed them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Used {
    /// When the session began, in Unix seconds (the thread's first turn, carried across a
    /// re-exec and aterm's relaunch in the record): the branches each worktree's own `HEAD`
    /// reflog moved from or to at or after it are ones it used, and a folder it only touched
    /// holds work only where it changed that folder's repository at or after it
    /// ([`changed_since`]).
    pub since: Option<u64>,
    /// The branches its own commands created or checked out, newest first, each once with the
    /// folder it was named in, at most [`USED_BRANCHES`] ([`command_reach`]): each is read in
    /// that folder's repository alone, and in every repository where its folder cannot be
    /// placed (none, gone, or in no repository).
    pub branches: Vec<UsedBranch>,
    /// The other folders it touched, newest first (each command's and each edit's but the
    /// thread's own; review of the forever probe at f48a215d7b, R2): each repository they are in
    /// holds work only where the session changed it since `since` ([`changed_since`]), so a
    /// checkout it merely looked in (a dependency's, cargo's) holds nothing that predates it.
    pub touched: Vec<PathBuf>,
}

impl Used {
    /// Take in the branches command `argv`, run in folder `cwd`, creates or checks out
    /// ([`command_reach`]; `home` for a `cd ~`): each moves to the front, once, and only the
    /// newest [`USED_BRANCHES`] are kept. Whether anything moved.
    pub fn note_command(
        &mut self,
        argv: &[String],
        cwd: Option<&Path>,
        home: Option<&Path>,
    ) -> bool {
        let mut moved = false;
        for branch in command_reach(argv, cwd, home).branches {
            moved |= note_branch(&mut self.branches, branch, USED_BRANCHES);
        }
        moved
    }
}

/// WHAT A COMMAND REACHES ([`command_reach`]): the folders it moves into or points git at, and
/// the branches its git commands create or check out, each with the folder its git ran in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reach {
    pub folders: Vec<PathBuf>,
    pub branches: Vec<UsedBranch>,
    /// THE WALK WAS CUT: the command reached more than [`REACH_FOLDERS`] folders, or ran a shell
    /// within more than [`SHELL_DEPTH`] shells, so a folder it reached may be missing: the scope
    /// that takes it in lost a folder (its `overflow`), and is never read clean.
    pub cut: bool,
}

/// The most folders one command reaches ([`command_reach`]): as many as a probe scope keeps
/// ([`codex_alab_core::session::PROBE_SCOPE_DIRS`]), so a walk is cut only where the scope that
/// takes its folders in would have lost one anyway. A cut overflows that scope for the rest of
/// its session (an overflowed scope is never read clean), so a lower bound made one ordinary
/// command poison Land for good (review of the forever-persist lane, F1: the first bound, 16,
/// dropped the seventeenth folder it had already placed, and `git -C <worktree> status` over
/// seventeen worktrees in one line held Land until the session ended).
pub(crate) const REACH_FOLDERS: usize = codex_alab_core::session::PROBE_SCOPE_DIRS;

/// WHAT COMMAND `argv`, RUN IN FOLDER `cwd`, REACHES (review of the forever probe at f48a215d7b,
/// R8: a folder reached inside a command, `cd proj && git commit` or `git -C proj commit`, was
/// never read; and the forever probe's follow-up, item e: nor was one another tool was told to
/// work in, `make -C proj`): `argv` read as one simple command, a shell's script (`bash -lc
/// '<script>'`, its word after the options that hold `c`) read as the shell reads it, a shell
/// within it as a subshell. Each `cd` or `pushd` moves the folder the script is in (a subshell's
/// move ends at its `)`, `popd` goes back), `env -C` (`--chdir`) runs its one command in another,
/// and each command runs past its prefixes (`env`, `timeout`, `nice`, `time`, `nohup`,
/// `command`, `exec`, assignments): each folder so reached, at most [`REACH_FOLDERS`] (past
/// them, or past [`SHELL_DEPTH`] shells, the walk is cut, [`Reach::cut`]): a `cd`'s,
/// a git command's past its `-C`s, its `--work-tree`, the folder a `worktree add`, a `clone` or
/// an `init` makes, and the folder another tool is told to work in ([`tool_folders`]: `make`'s,
/// `ninja`'s and `tar`'s `-C`, `cargo`'s `-C` and `--manifest-path` and the folder `cargo new`
/// or `init` makes, `npm`'s `--prefix`, `pnpm`'s `--dir`, `yarn`'s and `bun`'s `--cwd`, `just`'s
/// `-d` and `--justfile`, `go -C`, `uv`'s `--directory` and `--project`, `poetry`'s `-C` and
/// `--project`, `mvn -f`, `gradle -p`); and each branch a git command creates or checks out (a
/// name [`branch_name_ok`] admits) with the folder it ran in. A folder the words cannot place
/// (`cd "$X"`, `cd -` with none before, `~user`) is none, as is every folder after it until one
/// is placed, and a branch named there carries none (read in every repository). `home` is the
/// person's home, for `cd`, `cd ~` and `~/`. STILL UNREAD: a folder a word expands to at run
/// time (a variable, a command's output, a glob), one a program chooses itself (a script's own
/// `cd`, an interpreter's `chdir`, `xargs`, `find -execdir`, `sudo`, `ssh`, a container's `-w`
/// or mount, `eval`, `source`, an alias or a function), a here-document's commands, and the
/// options of tools not named above (`cmake -S`/`-B`, `bazel`, `pip --target`).
#[must_use]
pub fn command_reach(argv: &[String], cwd: Option<&Path>, home: Option<&Path>) -> Reach {
    let mut walk = Walk {
        here: cwd.map(normal),
        before: None,
        pushed: Vec::new(),
        home: home.map(normal),
        reach: Reach::default(),
        seen: HashSet::new(),
        depth: 0,
    };
    let words: Vec<&str> = argv.iter().map(String::as_str).collect();
    walk.command(&words);
    walk.reach
}

/// Whether word `w` names git.
fn is_git(w: &str) -> bool {
    w == "git" || w.ends_with("/git")
}

/// `path` with its `.` and `..` taken lexically, as the shell's `cd` takes them.
fn normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            part => out.push(part),
        }
    }
    out
}

/// [`command_reach`]'s walk through one command's script.
struct Walk {
    /// The folder the script is in now (`None`: one it cannot place).
    here: Option<PathBuf>,
    /// The folder before the last `cd` (`cd -`).
    before: Option<PathBuf>,
    /// `pushd`'s stack.
    pushed: Vec<Option<PathBuf>>,
    home: Option<PathBuf>,
    reach: Reach,
    /// The folders in `reach`, for a constant-time look: a long command line that moves between
    /// the same folders again and again stays cheap to walk.
    seen: HashSet<PathBuf>,
    /// How many shells' scripts this one is within ([`SHELL_DEPTH`] at most).
    depth: usize,
}

/// The most shells within shells [`command_reach`] reads into.
pub(crate) const SHELL_DEPTH: usize = 8;

impl Walk {
    /// Word `w` as a folder from `from`: absolute, or from `from`, with `~` and `~/` from the
    /// home; `None` for anything it cannot place (an expansion, a glob, `~user`).
    fn place(&self, w: &str, from: Option<&Path>) -> Option<PathBuf> {
        if w.contains(['$', '`', '*', '?', '[']) {
            return None;
        }
        let path = if w == "~" {
            self.home.clone()?
        } else if let Some(rest) = w.strip_prefix("~/") {
            self.home.as_deref()?.join(rest)
        } else if w.starts_with('~') {
            return None;
        } else if Path::new(w).is_absolute() {
            PathBuf::from(w)
        } else {
            from?.join(w)
        };
        Some(normal(&path))
    }

    /// Folder `dir` reached; past [`REACH_FOLDERS`] the walk is cut ([`Reach::cut`]).
    fn reached(&mut self, dir: Option<&PathBuf>) {
        if let Some(dir) = dir
            && !self.seen.contains(dir)
        {
            if self.reach.folders.len() < REACH_FOLDERS {
                self.seen.insert(dir.clone());
                self.reach.folders.push(dir.clone());
            } else {
                self.reach.cut = true;
            }
        }
    }

    /// The script moves to `to` (none: a folder it cannot place).
    fn move_to(&mut self, to: Option<PathBuf>) {
        self.before = std::mem::replace(&mut self.here, to);
        let here = self.here.clone();
        self.reached(here.as_ref());
    }

    /// One simple command's `words`, past its prefixes (`env`, whose `-C` or `--chdir` runs it
    /// in that folder, `timeout`, `nice`, `time`, `nohup`, `command`, `builtin`, `exec`, a shell
    /// keyword and assignments): a `cd`, `pushd` or `popd` of the script's own, a git command, a
    /// shell's script, or another tool's folders ([`tool_folders`]).
    fn command(&mut self, words: &[&str]) {
        let mut at = self.here.clone();
        let mut it = words.iter().copied().peekable();
        let mut chdir = false;
        while let Some(w) = it.next_if(|w| prefix_word(w)) {
            match w {
                "env" => {
                    while let Some(o) = it.next_if(|o| o.starts_with('-') || is_assignment(o)) {
                        if o == "--" {
                            break;
                        }
                        if let Some(dir) = option_value(o, &mut it, &["-C", "--chdir"]) {
                            at = dir.and_then(|dir| self.place(dir, at.as_deref()));
                            chdir = true;
                        } else if matches!(o, "-u" | "--unset" | "-S" | "--split-string" | "-P") {
                            it.next();
                        }
                    }
                }
                "timeout" => {
                    while let Some(o) = it.next_if(|o| o.starts_with('-')) {
                        if matches!(o, "-s" | "--signal" | "-k" | "--kill-after") {
                            it.next();
                        }
                    }
                    // Its duration.
                    it.next();
                }
                "nice" => {
                    while let Some(o) = it.next_if(|o| o.starts_with('-')) {
                        if matches!(o, "-n" | "--adjustment") {
                            it.next();
                        }
                    }
                }
                "time" | "command" | "builtin" | "exec" | "nohup" => {
                    while it.next_if(|o| o.starts_with('-')).is_some() {}
                }
                _ => {}
            }
        }
        if chdir {
            self.reached(at.as_ref());
        }
        let Some(first) = it.next() else {
            return;
        };
        let args: Vec<&str> = it.collect();
        match first {
            // A `cd` an `env` runs is a program of its own: the script stays where it is.
            "cd" | "pushd" if !chdir => {
                let mut operands = args
                    .iter()
                    .skip_while(|w| matches!(**w, "-L" | "-P" | "-e" | "-@"));
                let operand = match operands.next() {
                    Some(&"--") => operands.next(),
                    other => other,
                };
                let to = match operand {
                    None if first == "cd" => self.home.clone(),
                    None => None,
                    Some(&"-") => self.before.clone(),
                    Some(w) => self.place(w, self.here.as_deref()),
                };
                if first == "pushd" {
                    self.pushed.push(self.here.clone());
                }
                self.move_to(to);
            }
            "popd" if !chdir => {
                let to = self.pushed.pop().flatten();
                self.move_to(to);
            }
            w if is_git(w) => self.git(at, &args),
            w => {
                let name = Path::new(w)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(w);
                if let Some(script) = shell_script(name, &args) {
                    self.shell(at, script);
                } else {
                    for dir in tool_folders(name, &args, at, |w, from| self.place(w, from)) {
                        self.reached(Some(&dir));
                    }
                }
            }
        }
    }

    /// A shell's `script`, run from folder `at`: walked as a subshell, whose moves end with it;
    /// past [`SHELL_DEPTH`] shells the walk is cut ([`Reach::cut`]).
    fn shell(&mut self, at: Option<PathBuf>, script: &str) {
        if self.depth >= SHELL_DEPTH {
            self.reach.cut = true;
            return;
        }
        let outer = (
            std::mem::replace(&mut self.here, at),
            self.before.take(),
            std::mem::take(&mut self.pushed),
        );
        self.depth += 1;
        let mut subshells: Vec<Option<PathBuf>> = Vec::new();
        for step in shell_steps(script) {
            match step {
                Step::Command(words) => {
                    let words: Vec<&str> = words.iter().map(String::as_str).collect();
                    self.command(&words);
                }
                Step::Open => subshells.push(self.here.clone()),
                Step::Close => {
                    if let Some(here) = subshells.pop() {
                        self.here = here;
                    }
                }
            }
        }
        self.depth -= 1;
        (self.here, self.before, self.pushed) = outer;
    }

    /// A git command with `args` (after `git`), run in folder `at`: its folder past `-C`, its
    /// `--work-tree`, the folder a `worktree add`, a `clone` or an `init` makes, and the branch it
    /// creates or checks out (aterm's `git_branches_in`).
    fn git(&mut self, mut at: Option<PathBuf>, args: &[&str]) {
        let mut own_dir = true;
        let mut it = args.iter().copied().peekable();
        while let Some(w) = it.next_if(|w| w.starts_with('-')) {
            match w {
                "-C" => {
                    let to = it.next();
                    at = to.and_then(|to| {
                        if to.is_empty() {
                            at.clone()
                        } else {
                            self.place(to, at.as_deref())
                        }
                    });
                }
                "--work-tree" => {
                    let tree = it.next().and_then(|t| self.place(t, at.as_deref()));
                    self.reached(tree.as_ref());
                }
                "--git-dir" => {
                    it.next();
                    own_dir = false;
                }
                "-c" | "--namespace" | "--exec-path" | "--config-env" => {
                    it.next();
                }
                w if w.starts_with("--work-tree=") => {
                    let tree = self.place(&w["--work-tree=".len()..], at.as_deref());
                    self.reached(tree.as_ref());
                }
                w if w.starts_with("--git-dir=") => own_dir = false,
                _ => {}
            }
        }
        self.reached(at.as_ref());
        let Some(verb) = it.next() else {
            return;
        };
        let args: Vec<&str> = it.collect();
        let named = match verb {
            "checkout" | "switch" => checkout_branch(verb, &args),
            "worktree" => match args.split_first() {
                Some((&"add", rest)) => {
                    let made = worktree_folder(rest).and_then(|f| self.place(f, at.as_deref()));
                    self.reached(made.as_ref());
                    worktree_branch(rest)
                }
                _ => None,
            },
            "branch" => created_branch(&args),
            "clone" | "init" => {
                let made = made_repository(verb, &args).and_then(|f| self.place(&f, at.as_deref()));
                self.reached(made.as_ref());
                None
            }
            _ => None,
        };
        // A repository named by `--git-dir` is no folder's: read in every repository.
        let dir = at.filter(|_| own_dir);
        if let Some(name) = named.filter(|n| branch_name_ok(n)) {
            self.reach.branches.push(UsedBranch { name, dir });
        }
    }
}

/// Whether word `w` comes before the command it runs ([`Walk::command`]): a prefix program, a
/// shell keyword or an assignment.
fn prefix_word(w: &str) -> bool {
    matches!(
        w,
        "env"
            | "timeout"
            | "nice"
            | "time"
            | "command"
            | "builtin"
            | "exec"
            | "nohup"
            | "{"
            | "}"
            | "!"
            | "if"
            | "then"
            | "elif"
            | "else"
            | "while"
            | "until"
            | "do"
    ) || is_assignment(w)
}

/// Whether word `w` is a shell assignment (`NAME=value`).
fn is_assignment(w: &str) -> bool {
    w.split_once('=').is_some_and(|(k, _)| {
        !k.is_empty()
            && !k.starts_with(|c: char| c.is_ascii_digit())
            && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// Whether word `w` is one of the options `names` that take a value, and that value: the next
/// word of `rest` (`-C dir`, `--chdir dir`), what follows `=` (`--chdir=dir`), or what follows a
/// short option's letter (`-Cdir`). `Some(None)`: the option, with no value left.
fn option_value<'a>(
    w: &'a str,
    rest: &mut impl Iterator<Item = &'a str>,
    names: &[&str],
) -> Option<Option<&'a str>> {
    for name in names {
        if w == *name {
            return Some(rest.next());
        }
        let Some(tail) = w.strip_prefix(name) else {
            continue;
        };
        if name.starts_with("--") {
            if let Some(value) = tail.strip_prefix('=') {
                return Some(Some(value));
            }
        } else if name.len() == 2 && !tail.is_empty() {
            return Some(Some(tail));
        }
    }
    None
}

/// The shells whose `-c` script [`command_reach`] reads.
const SHELLS: [&str; 8] = ["sh", "bash", "zsh", "dash", "ksh", "mksh", "yash", "fish"];

/// The script shell `name` runs with `args`: the first word after its options where one of them
/// holds `c` (`-c`, `-lc`, `-euc`); `-o` and `-O` take a word of their own.
fn shell_script<'a>(name: &str, args: &[&'a str]) -> Option<&'a str> {
    if !SHELLS.contains(&name) {
        return None;
    }
    let mut command = false;
    let mut it = args.iter().copied();
    while let Some(w) = it.next() {
        match w {
            "--" => break,
            "-o" | "+o" | "-O" | "+O" => {
                it.next();
            }
            w if w.starts_with("--") => {}
            w if w.starts_with('-') || w.starts_with('+') => {
                command |= w.starts_with('-') && w.contains('c');
            }
            w => return command.then_some(w),
        }
    }
    command.then(|| it.next()).flatten()
}

/// HOW A TOOL'S FOLDER OPTION NAMES A FOLDER ([`tool_folders`]).
#[derive(Clone, Copy)]
enum Names {
    /// The folder the tool works in from here on (`make -C`, `npm --prefix`): later ones are
    /// from it, and the last is reached.
    Move,
    /// A folder it works on, from where it works (`uv --project`, `gradle -p`).
    Dir,
    /// A file whose folder it works in (`cargo --manifest-path`, `just --justfile`).
    File,
    /// A file whose folder it works in where the word names a `.xml` file, else that folder
    /// (`mvn -f`).
    FileOrDir,
}

/// One folder option of a tool: its spellings, how it names a folder, and whether only before
/// the tool's first operand (`cargo -C`, `go -C`).
type FolderOption = (&'static [&'static str], Names, bool);

/// THE FOLDERS TOOL `name` (a program's file name) IS TOLD TO WORK IN BY `args` (the forever
/// probe's follow-up, item e), run in folder `at`; `place` places a word from a folder
/// ([`Walk::place`]). Each folder option is read up to `--` (the program's own words after it),
/// and for `just`, `yarn`, `bun`, `pnpm`, `uv` and `poetry` up to the operand after which the
/// words are a recipe's or a program's; an option's value is skipped where it would read as an
/// operand. The folder `cargo new` or `cargo init` makes is reached too. None for a tool not named
/// here.
fn tool_folders(
    name: &str,
    args: &[&str],
    at: Option<PathBuf>,
    place: impl Fn(&str, Option<&Path>) -> Option<PathBuf>,
) -> Vec<PathBuf> {
    // (folder options, options that take a value, the operand that ends them: 0 for none)
    let (options, takes, ends): (&[FolderOption], &[&str], usize) = match name {
        "make" | "gmake" | "bmake" => (&[(&["-C", "--directory"], Names::Move, false)], &[], 0),
        "ninja" => (
            &[(&["-C"], Names::Move, false)],
            &["-f", "-j", "-k", "-l", "-d", "-t", "-w"],
            0,
        ),
        "tar" | "gtar" | "bsdtar" => (&[(&["-C", "--directory"], Names::Move, false)], &[], 0),
        "cargo" | "targo" => (
            &[
                (&["-C"], Names::Move, true),
                (&["--manifest-path"], Names::File, false),
            ],
            &[
                "-Z",
                "--config",
                "--color",
                "--name",
                "--vcs",
                "--edition",
                "--registry",
                "-p",
                "--package",
                "-F",
                "--features",
                "-j",
                "--jobs",
                "--target",
                "--target-dir",
                "--profile",
                "--bin",
                "--example",
                "--test",
                "--bench",
                "--message-format",
            ],
            0,
        ),
        "npm" | "npx" => (&[(&["--prefix", "-C"], Names::Move, false)], &[], 0),
        "pnpm" => (
            &[(&["-C", "--dir"], Names::Move, false)],
            &["-F", "--filter"],
            1,
        ),
        "yarn" | "bun" => (&[(&["--cwd"], Names::Move, false)], &[], 1),
        "just" => (
            &[
                (&["-d", "--working-directory"], Names::Dir, false),
                (&["-f", "--justfile"], Names::File, false),
            ],
            &[
                "--shell",
                "--shell-arg",
                "--dotenv-path",
                "--dotenv-filename",
                "--color",
            ],
            1,
        ),
        "go" => (&[(&["-C"], Names::Move, true)], &[], 0),
        "uv" => (
            &[
                (&["--directory"], Names::Move, false),
                (&["--project"], Names::Dir, false),
            ],
            &[
                "--python",
                "-p",
                "--with",
                "--package",
                "--extra",
                "--group",
                "--index",
            ],
            2,
        ),
        "poetry" => (
            &[
                (&["-C", "--directory"], Names::Move, false),
                (&["-P", "--project"], Names::Dir, false),
            ],
            &[],
            2,
        ),
        "mvn" | "mvnw" => (
            &[(&["-f", "--file"], Names::FileOrDir, false)],
            &["-pl", "-P", "-D"],
            0,
        ),
        "gradle" | "gradlew" => (&[(&["-p", "--project-dir"], Names::Dir, false)], &[], 0),
        _ => return Vec::new(),
    };
    let mut base = at;
    let mut moved = false;
    let mut found: Vec<Option<PathBuf>> = Vec::new();
    let mut operands: Vec<&str> = Vec::new();
    let mut it = args.iter().copied();
    'words: while let Some(w) = it.next() {
        if w == "--" || (ends > 0 && operands.len() >= ends) {
            break;
        }
        for (spellings, names, first) in options {
            if *first && !operands.is_empty() {
                continue;
            }
            let Some(value) = option_value(w, &mut it, spellings) else {
                continue;
            };
            let folder = value.and_then(|value| match names {
                Names::Move | Names::Dir => place(value, base.as_deref()),
                Names::File => place(value, base.as_deref())
                    .and_then(|file| file.parent().map(Path::to_path_buf)),
                Names::FileOrDir => place(value, base.as_deref()).and_then(|path| {
                    if value.ends_with(".xml") {
                        path.parent().map(Path::to_path_buf)
                    } else {
                        Some(path)
                    }
                }),
            });
            if matches!(names, Names::Move) {
                base = folder;
                moved = true;
            } else {
                found.push(folder);
            }
            continue 'words;
        }
        if takes.contains(&w) {
            it.next();
        } else if !w.starts_with('-') && !w.starts_with('+') {
            operands.push(w);
        }
    }
    // The folder `cargo new` makes, or `cargo init` makes or fills.
    if matches!(name, "cargo" | "targo")
        && let [verb, path, ..] = operands.as_slice()
        && matches!(*verb, "new" | "init")
    {
        found.push(place(path, base.as_deref()));
    }
    let mut folders = Vec::new();
    for folder in std::iter::once(base.filter(|_| moved))
        .chain(found)
        .flatten()
    {
        if !folders.contains(&folder) {
            folders.push(folder);
        }
    }
    folders
}

/// The folder `git clone <args>` or `git init <args>` makes: `init`'s operand; `clone`'s second,
/// else the "humanish" name git takes from its repository (`file:///b/x.git` makes `x`). None
/// where an `init` names none (it makes its own folder a repository, which git's folder holds).
fn made_repository(verb: &str, args: &[&str]) -> Option<String> {
    let takes: &[&str] = match verb {
        "clone" => &[
            "-o",
            "--origin",
            "-b",
            "--branch",
            "-u",
            "--upload-pack",
            "--reference",
            "--reference-if-able",
            "--separate-git-dir",
            "--depth",
            "--shallow-since",
            "--shallow-exclude",
            "-c",
            "--config",
            "--server-option",
            "-j",
            "--jobs",
            "--filter",
            "--template",
            "--bundle-uri",
            "--ref-format",
            "--revision",
        ],
        _ => &[
            "--template",
            "--separate-git-dir",
            "--object-format",
            "--ref-format",
            "-b",
            "--initial-branch",
        ],
    };
    let mut operands = Vec::new();
    let mut it = args.iter().copied();
    while let Some(w) = it.next() {
        if w == "--" {
            operands.extend(it.by_ref());
            break;
        }
        if takes.contains(&w) {
            it.next();
        } else if !w.starts_with('-') {
            operands.push(w);
        }
    }
    match (verb, operands.as_slice()) {
        ("clone", [_, dir, ..]) | ("init", [dir, ..]) => Some((*dir).to_string()),
        ("clone", [repository]) => {
            let name = repository.trim_end_matches('/');
            let name = name.strip_suffix("/.git").unwrap_or(name);
            let name = name.strip_suffix(".git").unwrap_or(name);
            let name = name.rsplit(['/', ':']).next().unwrap_or(name);
            (!name.is_empty()).then(|| name.to_string())
        }
        _ => None,
    }
}

/// A step of a shell script ([`shell_steps`]): a simple command's words, or a subshell's `(` or
/// `)`.
enum Step {
    Command(Vec<String>),
    Open,
    Close,
}

/// The steps of shell `text`, quotes and backslashes taken as the shell takes them, its simple
/// commands split at `;`, `&`, `|` and new lines, and each `(` and `)` a step of its own
/// (aterm's `shell_commands`, with the subshells kept).
fn shell_steps(text: &str) -> Vec<Step> {
    let (mut steps, mut words, mut word) = (Vec::new(), Vec::new(), String::new());
    let (mut quote, mut open) = (None::<char>, false);
    let mut chars = text.chars();
    let end =
        |steps: &mut Vec<Step>, words: &mut Vec<String>, word: &mut String, open: &mut bool| {
            if *open {
                words.push(std::mem::take(word));
                *open = false;
            }
            if !words.is_empty() {
                steps.push(Step::Command(std::mem::take(words)));
            }
        };
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => word.extend(chars.next()),
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                open = true;
            }
            (None, '\\') => {
                word.extend(chars.next());
                open = true;
            }
            (None, ';' | '&' | '|' | '\n') => end(&mut steps, &mut words, &mut word, &mut open),
            (None, '(' | ')') => {
                end(&mut steps, &mut words, &mut word, &mut open);
                steps.push(if c == '(' { Step::Open } else { Step::Close });
            }
            (None, c) if c.is_whitespace() => {
                if open {
                    words.push(std::mem::take(&mut word));
                    open = false;
                }
            }
            (None, c) => {
                word.push(c);
                open = true;
            }
        }
    }
    end(&mut steps, &mut words, &mut word, &mut open);
    steps
}

/// The folder `git worktree add <args>` makes: its first word that is no option (none after
/// `--` but the next).
fn worktree_folder<'a>(args: &[&'a str]) -> Option<&'a str> {
    let mut it = args.iter().copied();
    while let Some(w) = it.next() {
        match w {
            "--" => return it.next(),
            "-b" | "-B" | "--reason" => {
                it.next();
            }
            w if w.starts_with('-') => {}
            w => return Some(w),
        }
    }
    None
}

/// The branch `git checkout|switch <args>` creates or moves to: the one a create option names,
/// else its first word that is no option (none after `--`).
fn checkout_branch(verb: &str, args: &[&str]) -> Option<String> {
    let mut it = args.iter();
    let mut first = None;
    while let Some(&w) = it.next() {
        let create = match verb {
            "checkout" => matches!(w, "-b" | "-B" | "--orphan"),
            _ => matches!(w, "-c" | "-C" | "--create" | "--force-create" | "--orphan"),
        };
        if create {
            return it.next().map(|n| (*n).to_string());
        }
        let prefixes: &[&str] = match verb {
            "checkout" => &["-b", "-B", "--orphan="],
            _ => &["-c", "-C", "--create=", "--force-create=", "--orphan="],
        };
        if let Some(name) = prefixes
            .iter()
            .find_map(|p| w.strip_prefix(p))
            .filter(|s| !s.is_empty())
        {
            return Some(name.to_string());
        }
        if matches!(w, "--conflict" | "--pathspec-from-file") {
            it.next();
            continue;
        }
        if w == "--" {
            break;
        }
        if !w.starts_with('-') && first.is_none() {
            first = Some(w.to_string());
        }
    }
    first
}

/// The branch `git worktree add <args>` checks out: `-b`'s or `-B`'s, else the commit named after
/// the folder, else (not `--detach`ed) the folder's name.
fn worktree_branch(args: &[&str]) -> Option<String> {
    let (mut named, mut detached, mut words) = (None, false, Vec::new());
    let mut it = args.iter();
    while let Some(&w) = it.next() {
        match w {
            "--" => {
                words.extend(it.copied());
                break;
            }
            "-b" | "-B" => named = it.next().map(|n| (*n).to_string()),
            "-d" | "--detach" => detached = true,
            "--reason" => {
                it.next();
            }
            w if w.starts_with("-b") || w.starts_with("-B") => {
                named = w.get(2..).map(str::to_string);
            }
            w if w.starts_with('-') => {}
            w => words.push(w),
        }
    }
    named
        .or_else(|| words.get(1).map(|w| (*w).to_string()))
        .or_else(|| {
            words
                .first()
                .filter(|_| !detached)
                .and_then(|path| Path::new(path).file_name())
                .and_then(|n| n.to_str())
                .map(str::to_string)
        })
}

/// The branch `git branch <args>` creates, or renames or copies to: none where it lists,
/// deletes or sets an upstream.
fn created_branch(args: &[&str]) -> Option<String> {
    let mut words = Vec::new();
    let mut moved = false;
    for &w in args {
        match w {
            "-m" | "-M" | "--move" | "-c" | "-C" | "--copy" => moved = true,
            "-d" | "-D" | "--delete" | "-l" | "--list" | "-a" | "--all" | "-r" | "--remotes"
            | "-v" | "-vv" | "--verbose" | "--merged" | "--no-merged" | "--contains"
            | "--no-contains" | "--points-at" | "--show-current" | "--edit-description" | "-u"
            | "--set-upstream-to" | "--unset-upstream" | "--format" | "--sort" | "--column" => {
                return None;
            }
            w if w.starts_with('-') => {
                if [
                    "--set-upstream-to=",
                    "--format=",
                    "--sort=",
                    "--column=",
                    "--merged=",
                    "--no-merged=",
                    "--contains=",
                    "--no-contains=",
                    "--points-at=",
                ]
                .iter()
                .any(|p| w.starts_with(p))
                {
                    return None;
                }
            }
            w => words.push(w),
        }
    }
    let named = if moved { words.last() } else { words.first() };
    named.map(|w| (*w).to_string())
}

/// Whether `name` is a branch name the probe reads as `refs/heads/<name>`: one `git
/// check-ref-format --branch` would take, never an option or a revision expression.
#[must_use]
pub fn branch_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.starts_with(['-', '.', '/'])
        && !name.ends_with(['/', '.'])
        && !name.ends_with(".lock")
        && !name.contains("..")
        && !name.contains("@{")
        && !name.contains("//")
        && name != "@"
        && name != "HEAD"
        && !name
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c))
}

/// Whether `path` lies at or under a root macOS guards with a consent dialog
/// (`atpkg::protected::under_protected_root`): purely lexical, per component.
#[must_use]
pub fn under_protected_root(home: &Path, path: &Path) -> bool {
    if ABSOLUTE_ROOTS
        .iter()
        .any(|root| path.starts_with(Path::new(root)))
    {
        return true;
    }
    let Ok(rest) = path.strip_prefix(home) else {
        return false;
    };
    let mut components = rest.components().filter_map(|component| match component {
        Component::Normal(name) => Some(name),
        _ => None,
    });
    let Some(first) = components.next() else {
        return false;
    };
    if HOME_FOLDERS.iter().any(|folder| first == *folder) {
        return true;
    }
    let Some(second) = components.next() else {
        return false;
    };
    LIBRARY_DOMAINS
        .iter()
        .any(|(lib, domain)| first == *lib && second == *domain)
}

/// `dir` as the probe may enter it, as aterm's `unattended_dir` has it: absolute, no `..`,
/// outside every guarded folder as spelled; then `home` and `dir` resolved over local mounts
/// alone ([`resolve_local`]: a folder on a network share or an automounted volume, or one a
/// link leads onto one, is refused before anything on that mount is touched, so a stalled
/// share never blocks the probe); and the resolved `dir` outside every guarded folder of the
/// resolved `home`, and in the file system's own spelling too ([`canonical_outside`], stricter
/// than aterm). Answers that spelling. `None`: the probe does not run.
pub(crate) fn unattended_dir(dir: &Path, home: &Path, table: &Table) -> Option<PathBuf> {
    if !home.is_absolute()
        || !dir.is_absolute()
        || home
            .components()
            .chain(dir.components())
            .any(|c| c == Component::ParentDir)
        || under_protected_root(home, dir)
    {
        return None;
    }
    let real_home = resolve_local(home, table)?;
    let real = resolve_local(dir, table)?;
    if under_protected_root(&real_home, &real) {
        return None;
    }
    canonical_outside(&real, &real_home)
}

/// STRICTER THAN aterm, KEPT: `real` (a path [`resolve_local`] answered: no link left on it,
/// every folder of it on a local mount, so reading it touches nothing else) in the file
/// system's own spelling (`std::fs::canonicalize`), when that lies outside every guarded folder
/// of `real_home` in its own spelling. macOS's file systems ignore case, and the guarded-folder
/// check compares spellings: `/users/x/documents/p` (a worker's `workdir` may be spelled so)
/// is the guarded `~/Documents/p`, which aterm's check, reading the spelling it was given, lets
/// through to `git` and to the consent dialog the probe must never raise. `None` otherwise.
fn canonical_outside(real: &Path, real_home: &Path) -> Option<PathBuf> {
    let canon = std::fs::canonicalize(real).ok()?;
    let canon_home = std::fs::canonicalize(real_home).ok()?;
    (!under_protected_root(&canon_home, &canon)).then_some(canon)
}

/// A PROBE'S ANSWER AND THE FOLDERS WORTH READING AGAIN ([`probe_session`], [`compact`]): `read`
/// its answer, `kept` the repositories it read, each by its top level (the outermost project
/// and, where a folder was in a submodule of it, that one too), and every folder whose read was
/// cut, in the order given (the touched folders first). A folder that is gone, guarded, off
/// local mounts alone or in no repository is dropped, so a session's set of folders stays its
/// repositories however many folders of theirs it worked in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probed {
    pub read: RepoRead,
    pub kept: Vec<PathBuf>,
}

/// WHAT THE REPOSITORY `dir` IS IN HOLDS THAT NO REMOTE MAIN HAS, by `git`, the whole probe
/// within [`PROBE_LIMIT`] ([`probe_session`] of `dir` alone, by a session that used nothing
/// else). The person's home and `XDG_CONFIG_HOME` are this process's.
pub async fn probe(dir: &Path) -> RepoRead {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return RepoRead::Unread;
    };
    probe_within(dir, &home, PROBE_LIMIT).await
}

/// [`probe`] with the person's `home` (for the guarded folders, every read's `HOME` and their
/// global excludes) and the limit given; `XDG_CONFIG_HOME` is this process's.
pub async fn probe_within(dir: &Path, home: &Path, limit: Duration) -> RepoRead {
    let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    probe_with(dir, home, xdg.as_deref(), limit).await
}

/// [`probe_within`] with the person's `XDG_CONFIG_HOME` given (`None`: unset).
pub async fn probe_with(dir: &Path, home: &Path, xdg: Option<&Path>, limit: Duration) -> RepoRead {
    probe_session_with(&[dir.to_path_buf()], home, xdg, &Used::default(), limit)
        .await
        .read
}

/// THE PROBE OF A SESSION: every folder it touched that is in a repository the probe may enter
/// (`used.touched` first, newest first, then `dirs`, each read whole: the thread's own), each
/// repository once, with the branches it `used`, within one `limit` ([`PROBE_LIMIT`], or
/// [`START_PROBE_LIMIT`] for the start probe a turn waits for). A repository only a touched
/// folder led to holds work only where the session changed it since it began
/// ([`changed_since`]). The first finding is the answer; where none holds work, a read cut or
/// refused anywhere is unread, as are more than [`PROBE_DIRS`] folders.
pub async fn probe_session(dirs: &[PathBuf], used: &Used, limit: Duration) -> Probed {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Probed {
            read: RepoRead::Unread,
            kept: every_folder(dirs, used),
        };
    };
    probe_session_within(dirs, &home, used, limit).await
}

/// [`probe_session`] with the person's `home` and the limit given; `XDG_CONFIG_HOME` is this
/// process's.
pub async fn probe_session_within(
    dirs: &[PathBuf],
    home: &Path,
    used: &Used,
    limit: Duration,
) -> Probed {
    let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    probe_session_with(dirs, home, xdg.as_deref(), used, limit).await
}

/// [`probe_session_within`] with the person's `XDG_CONFIG_HOME` given (`None`: unset), over the
/// machine's mount table as it is now ([`crate::mounts::table`], read without asking any file
/// system).
pub async fn probe_session_with(
    dirs: &[PathBuf],
    home: &Path,
    xdg: Option<&Path>,
    used: &Used,
    limit: Duration,
) -> Probed {
    probe_session_in(dirs, home, xdg, used, limit, crate::mounts::table()).await
}

/// [`probe_session_with`] over the mount table `table` given (a test places folders on mounts of
/// its own).
pub async fn probe_session_in(
    dirs: &[PathBuf],
    home: &Path,
    xdg: Option<&Path>,
    used: &Used,
    limit: Duration,
    table: Table,
) -> Probed {
    Reader {
        dirs: dirs.to_vec(),
        home: home.to_path_buf(),
        xdg: xdg.filter(|x| x.is_absolute()).map(Path::to_path_buf),
        used: used.clone(),
        limit,
        table,
        locate_only: false,
        #[cfg(test)]
        seams: Seams::default(),
    }
    .beside()
    .await
}

/// THE REPOSITORIES `dirs` ARE IN, kept as a probe keeps them ([`Probed::kept`]) without reading
/// them, within `limit`: a scope's compaction between probes (review of the forever probe at
/// f48a215d7b, R1), beside the runtime as a probe's reads are. Every folder, where it was cut
/// or no reader could start; the person's home is this process's.
pub async fn compact(dirs: &[PathBuf], limit: Duration) -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return dirs.to_vec();
    };
    Reader {
        dirs: dirs.to_vec(),
        home,
        xdg: None,
        used: Used::default(),
        limit,
        table: crate::mounts::table(),
        locate_only: true,
        #[cfg(test)]
        seams: Seams::default(),
    }
    .beside()
    .await
    .kept
}

/// [`probe_session_in`] whose reader runs `stall` before each synchronous file system step it
/// takes (a test's stand-in for a stat blocked on a stalled share).
#[cfg(test)]
pub(crate) async fn probe_stalled(
    dirs: &[PathBuf],
    home: &Path,
    limit: Duration,
    stall: Stall,
) -> Probed {
    Reader {
        dirs: dirs.to_vec(),
        home: home.to_path_buf(),
        xdg: None,
        used: Used::default(),
        limit,
        table: crate::mounts::table(),
        locate_only: false,
        seams: Seams {
            stall: Some(stall),
            refuse: None,
        },
    }
    .beside()
    .await
}

/// [`probe_session_with`] whose reads git refuses where `refuse` says so (given each read's
/// folder and arguments): a test's stand-in for a read git refuses (exit 128 with a
/// diagnostic), weighed as one is ([`failed`]).
#[cfg(test)]
pub(crate) async fn probe_refusing(
    dirs: &[PathBuf],
    home: &Path,
    used: &Used,
    refuse: Refuse,
) -> Probed {
    Reader {
        dirs: dirs.to_vec(),
        home: home.to_path_buf(),
        xdg: None,
        used: used.clone(),
        limit: PROBE_LIMIT,
        table: crate::mounts::table(),
        locate_only: false,
        seams: Seams {
            stall: None,
            refuse: Some(refuse),
        },
    }
    .beside()
    .await
}

/// A test's stall ([`probe_stalled`]), run on the reader before each synchronous file system
/// step.
#[cfg(test)]
pub(crate) type Stall = std::sync::Arc<dyn Fn() + Send + Sync>;

/// A test's refusal ([`probe_refusing`]): whether git refuses the read in a folder with these
/// arguments.
#[cfg(test)]
pub(crate) type Refuse = std::sync::Arc<dyn Fn(&Path, &[&str]) -> bool + Send + Sync>;

/// A test's seams in a probe's reads.
#[cfg(test)]
#[derive(Clone, Default)]
struct Seams {
    stall: Option<Stall>,
    refuse: Option<Refuse>,
}

/// Every folder a probe of `dirs` and `used` reads, as [`Probed::kept`] orders them.
fn every_folder(dirs: &[PathBuf], used: &Used) -> Vec<PathBuf> {
    used.touched.iter().chain(dirs).cloned().collect()
}

/// The probe readers alive now ([`PROBE_READERS`]).
static READERS: AtomicUsize = AtomicUsize::new(0);

/// One of the [`PROBE_READERS`] slots, held by a reader's thread until it ends.
struct ReaderSlot;

impl ReaderSlot {
    fn take() -> Option<Self> {
        if READERS.fetch_add(1, Ordering::SeqCst) < PROBE_READERS {
            Some(Self)
        } else {
            READERS.fetch_sub(1, Ordering::SeqCst);
            None
        }
    }
}

impl Drop for ReaderSlot {
    fn drop(&mut self) {
        READERS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// One probe's reads, owned, for its own thread.
struct Reader {
    dirs: Vec<PathBuf>,
    home: PathBuf,
    xdg: Option<PathBuf>,
    used: Used,
    limit: Duration,
    table: Table,
    /// Only the repositories the folders are in are wanted ([`compact`]).
    locate_only: bool,
    #[cfg(test)]
    seams: Seams,
}

impl Reader {
    /// THE PROBE BESIDE ITS ASKER (aterm's `Probing::ask`; review of the forever probe,
    /// hangs-1): the reads go on on a thread of their own, with a runtime of their own, and a
    /// watcher thread answers with the reader's answer, or unread once [`READER_GRACE`] past the
    /// limit, leaving a reader a synchronous call blocks to end on its own (its children are
    /// killed at the limit by the reads, or once the asker goes). No timer of the asker's
    /// runtime is used, so a runtime whose clock a test pauses waits for the real answer. A
    /// reader that cannot start, more than [`PROBE_READERS`] alive, or one that panicked is
    /// unread.
    async fn beside(self) -> Probed {
        let kept = every_folder(&self.dirs, &self.used);
        let unread = || Probed {
            read: RepoRead::Unread,
            kept: kept.clone(),
        };
        let Some(slot) = ReaderSlot::take() else {
            return unread();
        };
        let (said, answer) = tokio::sync::oneshot::channel();
        let (cancel, cancelled) = tokio::sync::oneshot::channel::<()>();
        let started = std::thread::Builder::new()
            .name("alab-repo-probe".to_string())
            .spawn(move || self.watch(slot, said, cancelled));
        if started.is_err() {
            return unread();
        }
        let found = answer.await;
        drop(cancel);
        found.unwrap_or_else(|_| unread())
    }

    /// The watcher: starts the reader and waits for its answer, at most [`READER_GRACE`] past
    /// the limit.
    fn watch(
        self,
        slot: ReaderSlot,
        said: tokio::sync::oneshot::Sender<Probed>,
        cancelled: tokio::sync::oneshot::Receiver<()>,
    ) {
        let kept = every_folder(&self.dirs, &self.used);
        let wait = self.limit.saturating_add(READER_GRACE);
        let (tell, heard) = std::sync::mpsc::sync_channel(1);
        let reader = std::thread::Builder::new()
            .name("alab-repo-read".to_string())
            .spawn(move || {
                let _slot = slot;
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                let found = runtime.block_on(async move {
                    tokio::select! {
                        biased;
                        _ = cancelled => None,
                        found = self.read() => Some(found),
                    }
                });
                if let Some(found) = found {
                    let _ = tell.send(found);
                }
            });
        let answer = match reader {
            Ok(_) => heard.recv_timeout(wait).ok(),
            Err(_) => None,
        };
        let _ = said.send(answer.unwrap_or(Probed {
            read: RepoRead::Unread,
            kept,
        }));
    }

    /// The reads, on the reader's own runtime.
    async fn read(self) -> Probed {
        let reads = Reads {
            home: self.home,
            xdg: self.xdg,
            table: self.table,
            deadline: (!self.limit.is_zero()).then(|| tokio::time::Instant::now() + self.limit),
            incomplete: AtomicBool::new(false),
            #[cfg(test)]
            seams: self.seams,
        };
        session_reads(&self.dirs, &self.used, &reads, self.locate_only).await
    }
}

/// One repository a probe reads ([`session_reads`]): its top level as git names it, its folder,
/// whether a folder read whole led to it, and the outermost project it is a submodule of, by its
/// place among the probe's repositories.
struct Root {
    top: String,
    root: PathBuf,
    whole: bool,
    outer: Option<usize>,
}

/// [`probe_session`]'s reads: each folder's repositories ([`place`]), then (unless only they are
/// wanted, `locate_only`) each repository once ([`probe_repo`]), with the branches the session
/// named in it and those whose folder cannot be placed, each with a fresh cut under the one
/// deadline; the first finding, its state hashed with every other finding's (unknown where any
/// is, or a read was cut), else unread where any read was cut, else nothing.
async fn session_reads(dirs: &[PathBuf], used: &Used, reads: &Reads, locate_only: bool) -> Probed {
    if dirs.len() + used.touched.len() > PROBE_DIRS {
        return Probed {
            read: RepoRead::Unread,
            kept: every_folder(dirs, used),
        };
    }
    let mut cut = false;
    let mut kept: Vec<PathBuf> = Vec::new();
    let mut roots: Vec<Root> = Vec::new();
    let order = used
        .touched
        .iter()
        .map(|dir| (dir, false))
        .chain(dirs.iter().map(|dir| (dir, true)));
    for (dir, whole) in order {
        reads.incomplete.store(false, Ordering::SeqCst);
        let known = roots.len();
        let at = place(dir, &mut roots, reads).await;
        if reads.incomplete.load(Ordering::SeqCst) {
            cut = true;
            if !kept.contains(dir) {
                kept.push(dir.clone());
            }
        }
        for root in &roots[known..] {
            if !kept.contains(&root.root) {
                kept.push(root.root.clone());
            }
        }
        // A folder read whole reads its repository and each project around it whole.
        let mut up = at.filter(|_| whole);
        while let Some(i) = up {
            roots[i].whole = true;
            up = roots[i].outer;
        }
    }
    if locate_only {
        return Probed {
            read: RepoRead::Unread,
            kept,
        };
    }
    // Each branch the session named, by the repository its folder is in (`None`: every one).
    let mut named: Vec<(Option<usize>, &str)> = Vec::new();
    for branch in &used.branches {
        let at = match &branch.dir {
            Some(dir) => {
                reads.incomplete.store(false, Ordering::SeqCst);
                let at = place(dir, &mut roots, reads).await;
                cut |= reads.incomplete.load(Ordering::SeqCst);
                at
            }
            None => None,
        };
        if let Some(i) = at
            && !kept.contains(&roots[i].root)
        {
            kept.push(roots[i].root.clone());
        }
        named.push((at, branch.name.as_str()));
    }
    let mut cargo_copies = cargo_copy::Memo::default();
    let mut first: Option<RepoWork> = None;
    let mut states: Vec<Option<u64>> = Vec::new();
    for (i, root) in roots.iter().enumerate() {
        let names: Vec<String> = named
            .iter()
            .filter(|(at, _)| at.is_none_or(|at| at == i))
            .map(|(_, name)| (*name).to_string())
            .collect();
        reads.incomplete.store(false, Ordering::SeqCst);
        let found = probe_repo(root, used.since, &names, reads, &mut cargo_copies).await;
        let this_cut = reads.incomplete.load(Ordering::SeqCst);
        cut |= this_cut;
        if let Some(work) = found {
            states.push(work.state.filter(|_| !this_cut));
            if first.is_none() {
                first = Some(work);
            }
        }
    }
    let read = match first {
        Some(mut work) => {
            work.state = if cut || states.iter().any(Option::is_none) {
                None
            } else if let [only] = states.as_slice() {
                *only
            } else {
                let text: String = states
                    .iter()
                    .flatten()
                    .map(|s| format!("{s:016x}\n"))
                    .collect();
                Some(crate::needs::fnv1a_64(text.as_bytes()))
            };
            RepoRead::Read(Some(work))
        }
        None if cut => RepoRead::Unread,
        None => RepoRead::Read(None),
    };
    Probed { read, kept }
}

/// THE REPOSITORY FOLDER `dir` IS IN, among `roots` (each new one, and the outermost project
/// around it, added): one already there it lies in with no `.git` between ([`fold`], so a
/// session that touched a thousand folders of one repository costs one discovery), else as
/// [`locate`] finds it. Its place among `roots`; none for a folder that is gone, guarded, off
/// local mounts alone or in no repository (a refused discovery is cut, [`Reads::incomplete`]).
async fn place(dir: &Path, roots: &mut Vec<Root>, reads: &Reads) -> Option<usize> {
    reads.fs_step();
    let real = unattended_dir(dir, &reads.home, &reads.table)?;
    if let Some(at) = fold(&real, roots, reads) {
        return Some(at);
    }
    let mut outer = None;
    for (top, root) in locate(&real, reads).await {
        let at = match roots.iter().position(|r| r.root == root) {
            Some(at) => at,
            None => {
                roots.push(Root {
                    top,
                    root,
                    whole: false,
                    outer,
                });
                roots.len() - 1
            }
        };
        outer = Some(at);
    }
    outer
}

/// The repository among `roots` folder `real` (as [`unattended_dir`] spelled it) lies in by
/// git's own discovery: the innermost one it is at or under with no `.git` (a file or a folder:
/// a submodule, a linked worktree, a nested clone) in any folder from `real` up to it, each
/// looked at without following a link. None where any look fails otherwise or none is found.
fn fold(real: &Path, roots: &[Root], reads: &Reads) -> Option<usize> {
    let (at, root) = roots
        .iter()
        .enumerate()
        .filter(|(_, r)| real.starts_with(&r.root))
        .max_by_key(|(_, r)| r.root.components().count())?;
    for dir in real.ancestors().take_while(|dir| *dir != root.root) {
        reads.fs_step();
        match std::fs::symlink_metadata(dir.join(".git")) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            _ => return None,
        }
    }
    Some(at)
}

/// THE REPOSITORIES FOLDER `dir` (as [`unattended_dir`] spelled it) IS IN, as the probe reads
/// them: the outermost project its repository is a submodule of ([`outermost`]) and, where that
/// is another, its own, each by its top level and its folder; none for a folder in no
/// repository, or whose repository is guarded or off local mounts (a refused discovery is cut,
/// [`Reads::incomplete`]).
async fn locate(dir: &Path, reads: &Reads) -> Vec<(String, PathBuf)> {
    let Some(top) = git(dir, &["rev-parse", "--show-toplevel"], reads).await else {
        return Vec::new();
    };
    let top = top.trim_end_matches(['\n', '\r']).to_string();
    if top.is_empty() {
        return Vec::new();
    }
    let Some(root) = unattended_dir(Path::new(&top), &reads.home, &reads.table) else {
        return Vec::new();
    };
    let Some((outer_top, outer_root)) = outermost(&top, &root, reads).await else {
        return vec![(top, root)];
    };
    if outer_root == root {
        vec![(top, root)]
    } else {
        vec![(outer_top, outer_root), (top, root)]
    }
}

/// The top level and the folder of the outermost project the repository at top level `top`,
/// folder `root`, is a submodule of (`git rev-parse --show-superproject-working-tree`, at most
/// [`SUPER_CLIMB`] up; itself where it is none), as aterm main `801e72779e` climbs: a session
/// that moved into a submodule works on the project around it too. A project the probe may not
/// enter ends the climb below it. `None`, cut, where a read is refused or cut, answers no
/// absolute folder, or the climb goes on past [`SUPER_CLIMB`].
async fn outermost(top: &str, root: &Path, reads: &Reads) -> Option<(String, PathBuf)> {
    let (mut top, mut root) = (top.to_string(), root.to_path_buf());
    for _ in 0..=SUPER_CLIMB {
        let Some(outer) = git_strict(
            &root,
            &["rev-parse", "--show-superproject-working-tree"],
            reads,
        )
        .await
        else {
            reads.incomplete.store(true, Ordering::SeqCst);
            return None;
        };
        let outer = outer.trim_end_matches(['\n', '\r']);
        if outer.is_empty() {
            return Some((top, root));
        }
        if !Path::new(outer).is_absolute() {
            reads.incomplete.store(true, Ordering::SeqCst);
            return None;
        }
        let Some(outer_root) = unattended_dir(Path::new(outer), &reads.home, &reads.table) else {
            return Some((top, root));
        };
        (top, root) = (outer.to_string(), outer_root);
    }
    reads.incomplete.store(true, Ordering::SeqCst);
    None
}

/// What repository `at` holds that no remote main has, by a session that began at `since` and
/// named the branches `names` in it; where no folder read whole led to it, only where the
/// session changed it since then ([`changed_since`]).
async fn probe_repo(
    at: &Root,
    since: Option<u64>,
    names: &[String],
    reads: &Reads,
    cargo_copies: &mut cargo_copy::Memo,
) -> Option<RepoWork> {
    let (top, root) = (at.top.as_str(), at.root.as_path());
    // One complete factory-copy observation covers its registered graph during this probe.
    // Explicit whole-repository ownership always requires the ordinary landing checks.
    if !at.whole && cargo_copies.derived(root, names, reads).await {
        return None;
    }
    // The repository's own `core.excludesFile` wins over the person's, as in their own `git
    // status`: theirs is handed over only where it sets none.
    let excludes = if own_excludes(root, reads).await {
        None
    } else {
        global_excludes(root, reads).await
    }
    .and_then(|p| p.to_str().map(|p| format!("core.excludesFile={p}")));
    let status_read = |untracked: &'static str| {
        let mut args: Vec<&str> = Vec::new();
        if let Some(flag) = &excludes {
            args.extend(["-c", flag.as_str()]);
        }
        args.extend(["status", "--porcelain=v1", "-z", untracked]);
        args
    };
    let status = git(root, &status_read("--untracked-files=normal"), reads).await?;
    let status = without_lagging_submodules(root, &status, excludes.as_deref(), reads).await;
    let branch = git(root, &["symbolic-ref", "--quiet", "--short", "HEAD"], reads)
        .await
        .map(|b| b.trim().to_string())
        .filter(|b| !b.is_empty());
    let listed = git(root, &["stash", "list", "--format=%gs"], reads)
        .await
        .unwrap_or_default();
    // The branches the session used besides HEAD's: their stash entries are its work too.
    let used_on = used_branches(root, since, names, branch.as_deref(), reads).await;
    let mut stash = stashes_on(&listed, branch.as_deref());
    for name in used_on.iter().flatten() {
        stash.push_str(&stashes_on(&listed, Some(name)));
    }
    let remotes = git(
        root,
        &[
            "for-each-ref",
            "--count=1",
            "--format=%(refname)",
            "refs/remotes",
        ],
        reads,
    )
    .await
    .unwrap_or_default();
    // A remote configured but never fetched from or pushed to has no tracking ref, and every
    // commit on `HEAD` is one it lacks (aterm's D2-5). The read is strict: a refused one is no
    // clean one ([`failed`]).
    let configured = git_strict(root, &["remote"], reads).await;
    let remote =
        !remotes.trim().is_empty() || configured.as_deref().is_some_and(|r| !r.trim().is_empty());
    // A branch with no commit yet has nothing to push or land.
    let born = remote && !branch_unborn(root, reads).await;
    let unpushed = if born {
        git(
            root,
            &["rev-list", "--count", "HEAD", "--not", "--remotes"],
            reads,
        )
        .await
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0)
    } else {
        0
    };
    let main = main_ref(root, reads).await;
    let (unlanded, main_ref) = match main.as_deref().filter(|_| born) {
        Some(full) => git(
            root,
            &["rev-list", "--count", &format!("{full}..HEAD")],
            reads,
        )
        .await
        .and_then(|n| n.trim().parse().ok())
        .map(|n| (n, full.trim_start_matches("refs/remotes/").to_string()))
        .unwrap_or_default(),
        None => (0, String::new()),
    };
    let subs = if remote {
        submodules_unpushed(root, reads).await
    } else {
        Some(Beyond::default())
    };
    let parked = match &used_on {
        Some(names) if remote => parked_on(root, names, main.as_deref(), reads).await,
        Some(_) => Some(Beyond::default()),
        None => None,
    };
    // A read of a superproject's submodules or of the branches the session used that git
    // refused is never a clean reading.
    if subs.is_none() || parked.is_none() {
        reads.incomplete.store(true, Ordering::SeqCst);
    }
    let (subs, parked) = (subs.unwrap_or_default(), parked.unwrap_or_default());
    let mut work =
        RepoWork::of_git(top, &status, &stash, unpushed, unlanded, &main_ref).or_else(|| {
            (subs.count > 0 || parked.count > 0).then(|| RepoWork {
                repo: printable(top.trim_end_matches(['\n', '\r'])),
                main_ref: printable(&main_ref),
                ..RepoWork::default()
            })
        })?;
    (work.sub_unpushed, work.subs) = (subs.count, subs.names);
    (work.parked, work.parked_on) = (parked.count, parked.names);
    // The work's state: every untracked file listed alone (a directory's files each), by the
    // same excludes, with `HEAD`'s commit, and the commits counted beyond `HEAD`.
    let head = git(root, &["rev-parse", "--verify", "--quiet", "HEAD"], reads)
        .await
        .unwrap_or_default();
    let beyond = format!("{}{}", subs.listing, parked.listing);
    let listed = git(root, &status_read("--untracked-files=all"), reads).await;
    // A repository only a touched folder led to: work the session never changed predates it
    // (review of the forever probe at f48a215d7b, R2). The branches it used are its own.
    if !at.whole
        && subs.count == 0
        && parked.count == 0
        && !changed_since(root, since, listed.as_deref(), reads).await
    {
        return None;
    }
    reads.fs_step();
    work.state = listed
        .and_then(|listed| state_of(root, head.trim(), &stash, &listed))
        .map(|state| {
            if beyond.is_empty() {
                state
            } else {
                crate::needs::fnv1a_64(format!("{state:016x}\n{beyond}").as_bytes())
            }
        });
    Some(work)
}

/// Whether the repository at `root` is on a branch with no commit yet: `HEAD` names a
/// `refs/heads/` ref and no ref of that name names a commit (aterm's `branch_unborn`). A refused
/// read proves nothing ([`Reads::incomplete`]).
async fn branch_unborn(root: &Path, reads: &Reads) -> bool {
    let Some(full) = git(root, &["symbolic-ref", "--quiet", "HEAD"], reads)
        .await
        .map(|full| full.trim().to_string())
        .filter(|full| full.starts_with("refs/heads/"))
    else {
        return false;
    };
    git_strict(
        root,
        &["for-each-ref", "--count=1", "--format=commit", &full],
        reads,
    )
    .await
    .map_or_else(
        || {
            reads.incomplete.store(true, Ordering::SeqCst);
            false
        },
        |refs| refs.trim().is_empty(),
    )
}

/// What [`submodules_unpushed`] or [`parked_on`] counted: the commits, the first
/// [`NAMED_PATHS`] submodules or branches holding them, and their listing for the state.
#[derive(Debug, Default)]
struct Beyond {
    count: usize,
    names: Vec<String>,
    listing: String,
}

/// THE COMMITS EACH POPULATED SUBMODULE of the repository at `root` (a `.gitmodules` path below
/// it holding its own `.git`), and each of theirs in turn, holds at its checkout or at the
/// commit its superproject's index records for it that no remote of that submodule has, git's
/// own check before a push that recurses: a superproject that pushes its record of one leaves
/// the remote main pointing at a commit nobody can fetch (aterm's `submodules_unpushed`, D2-1,
/// R-1, R-4, R-6). A record the submodule never fetched is not read ([`record_available`]); a
/// shallow clone's boundary commits ([`shallow_roots`]) and commits `FETCH_HEAD` records as
/// fetched from a configured remote of that submodule ([`remote_fetch_heads`]) came from a
/// remote. Nothing is fetched. Every submodule is read; each level is a folder below the last,
/// so the walk ends. `None` where a read is refused or cut.
async fn submodules_unpushed(root: &Path, reads: &Reads) -> Option<Beyond> {
    let mut beyond = Beyond::default();
    let mut repos: Vec<(String, PathBuf)> = vec![(String::new(), root.to_path_buf())];
    let max = format!("--max-count={BEYOND_COMMITS}");
    let mut at = 0;
    while let Some((prefix, repo)) = repos.get(at).cloned() {
        at += 1;
        for (path, dir, record) in submodule_records(&repo, reads).await? {
            if repos.iter().any(|(_, read)| *read == dir) {
                continue;
            }
            let mut args = vec!["rev-list", max.as_str(), "HEAD"];
            if let Some(record) = record.as_deref()
                && record_available(&dir, record, reads).await?
            {
                args.push(record);
            }
            args.extend(["--not", "--remotes"]);
            let mut listing = git_strict(&dir, &args, reads).await?;
            let mut commits: Vec<String> = listing
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect();
            if !commits.is_empty() {
                let shallow = shallow_roots(&dir, reads).await?;
                commits.retain(|c| !shallow.contains(c));
                if !commits.is_empty() {
                    let fetched = remote_fetch_heads(&dir, reads).await?;
                    if !fetched.is_empty() {
                        args.extend(fetched.iter().map(String::as_str));
                        listing = git_strict(&dir, &args, reads).await?;
                        commits = listing
                            .lines()
                            .map(str::trim)
                            .filter(|c| !c.is_empty() && !shallow.contains(*c))
                            .map(str::to_string)
                            .collect();
                    }
                }
            }
            let named = format!("{prefix}{path}");
            if !commits.is_empty() {
                beyond.count = beyond.count.saturating_add(commits.len());
                if beyond.names.len() < NAMED_PATHS {
                    beyond.names.push(printable(&named));
                }
                beyond
                    .listing
                    .push_str(&format!("{named}\n{}\n", commits.join("\n")));
            }
            repos.push((format!("{named}/"), dir));
        }
    }
    Some(beyond)
}

/// Whether the index's gitlink `record` is an object this submodule has fetched (`git cat-file
/// -e`): only git's quiet "missing" (exit 1, nothing on stderr) makes it optional; any other
/// refusal leaves the count unread (`None`).
async fn record_available(dir: &Path, record: &str, reads: &Reads) -> Option<bool> {
    if !is_head(record) {
        reads.incomplete.store(true, Ordering::SeqCst);
        return None;
    }
    match git_strict(dir, &["cat-file", "-e", record], reads).await {
        Some(_) => Some(true),
        None if reads.incomplete.load(Ordering::SeqCst) => None,
        None => Some(false),
    }
}

/// The populated submodules of the repository at `root`: each `.gitmodules` path whose folder,
/// below `root` and one the probe may enter, holds its own `.git` (looked at without following
/// a link), with the commit the index records for it. `None` where a read is refused or cut.
async fn submodule_records(
    root: &Path,
    reads: &Reads,
) -> Option<Vec<(String, PathBuf, Option<String>)>> {
    reads.fs_step();
    if !std::fs::symlink_metadata(root.join(".gitmodules")).is_ok_and(|m| m.is_file()) {
        return Some(Vec::new());
    }
    let config = git_strict(
        root,
        &["config", "-z", "--file", ".gitmodules", "--list"],
        reads,
    )
    .await?;
    let mut paths: Vec<(String, PathBuf)> = Vec::new();
    for entry in config.split('\0') {
        let Some((key, path)) = entry.split_once('\n') else {
            continue;
        };
        if !(key.starts_with("submodule.") && key.ends_with(".path"))
            || path.is_empty()
            || Path::new(path)
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            continue;
        }
        let Some(dir) = unattended_dir(&root.join(path), &reads.home, &reads.table) else {
            continue;
        };
        reads.fs_step();
        if dir != root
            && dir.starts_with(root)
            && std::fs::symlink_metadata(dir.join(".git")).is_ok()
            && !paths.iter().any(|(p, d)| p == path || *d == dir)
        {
            paths.push((path.to_string(), dir));
        }
    }
    let mut records: Vec<(String, String)> = Vec::new();
    for chunk in paths.chunks(RECORD_PATHS) {
        let mut args = vec!["--literal-pathspecs", "ls-files", "--stage", "-z", "--"];
        args.extend(chunk.iter().map(|(p, _)| p.as_str()));
        let staged = git_strict(root, &args, reads).await?;
        // `<mode> <commit> <stage>\t<path>`: a gitlink's mode is `160000`.
        records.extend(staged.split('\0').filter_map(|e| {
            let (meta, path) = e.split_once('\t')?;
            let mut meta = meta.split(' ');
            (meta.next()? == "160000").then_some(())?;
            let commit = meta.next().filter(|c| is_head(c))?;
            Some((path.to_string(), commit.to_string()))
        }));
    }
    Some(
        paths
            .into_iter()
            .map(|(path, dir)| {
                let record = records
                    .iter()
                    .find(|(p, _)| *p == path)
                    .map(|(_, c)| c.clone());
                (path, dir, record)
            })
            .collect(),
    )
}

/// The git file at `path` (`rev-parse --git-path`'s answer, joined to `dir`), read-only, where
/// it resolves over local mounts alone ([`resolve_local`]) and is a regular file opened without
/// following a link or blocking; `Ok(None)` where there is no such file, `Err(())` where it
/// lies where the probe never reads (off local mounts alone), which is no clean reading.
fn open_git_file(dir: &Path, at: &str, reads: &Reads) -> Result<Option<std::fs::File>, ()> {
    let path = dir.join(at.trim_end_matches(['\n', '\r']));
    reads.fs_step();
    Ok(
        crate::mounts::resolve_local_or_missing(&path, &reads.table)?
            .and_then(|real| open_regular(&real)),
    )
}

/// The regular file at `path`, read-only, opened without blocking and, on unix, without
/// following a link; `None` where it is none.
fn open_regular(path: &Path) -> Option<std::fs::File> {
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt as _;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)
            .ok()?
    };
    #[cfg(not(unix))]
    let file = {
        if !std::fs::symlink_metadata(path).ok()?.is_file() {
            return None;
        }
        std::fs::File::open(path).ok()?
    };
    file.metadata()
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map(|_| file)
}

/// The first `max` bytes of the git file `at` in `dir` ([`open_git_file`]), lossily as text:
/// empty where there is none, `None` (cut) where it lies off local mounts alone.
fn read_git_file(dir: &Path, at: &str, max: u64, reads: &Reads) -> Option<String> {
    let Ok(file) = open_git_file(dir, at, reads) else {
        reads.incomplete.store(true, Ordering::SeqCst);
        return None;
    };
    let mut bytes = Vec::new();
    if let Some(file) = file {
        let _ = std::io::Read::read_to_end(&mut std::io::Read::take(file, max), &mut bytes);
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The boundary commits of the shallow clone at `dir` (its `shallow` file, the first
/// [`SHALLOW_BYTES`] of it), none where it is no shallow clone: a fetch cut each one's history
/// there, so each came from a remote, though no remote-tracking ref reaches it. `None` where a
/// read is refused or cut.
async fn shallow_roots(dir: &Path, reads: &Reads) -> Option<std::collections::HashSet<String>> {
    let at = git_strict(dir, &["rev-parse", "--git-path", "shallow"], reads).await?;
    let text = read_git_file(dir, &at, SHALLOW_BYTES, reads)?;
    Some(
        text.lines()
            .map(str::trim)
            .filter(|c| is_head(c))
            .map(str::to_string)
            .collect(),
    )
}

/// The commits `FETCH_HEAD` at `dir` (its first [`FETCH_HEAD_BYTES`]) records as fetched from one
/// of `dir`'s configured remotes, resolved to present commits ([`commits_available`]), at most
/// [`FETCH_HEAD_COMMITS`]: `git submodule update --depth N` (`N > 1`) at a commit no
/// remote-tracking ref reaches writes the fetched tip here, while `shallow` holds only the
/// boundary below it. `None` where a read is refused or cut.
async fn remote_fetch_heads(dir: &Path, reads: &Reads) -> Option<Vec<String>> {
    let at = git_strict(dir, &["rev-parse", "--git-path", "FETCH_HEAD"], reads).await?;
    let text = read_git_file(dir, &at, FETCH_HEAD_BYTES, reads)?;
    if text.trim().is_empty() {
        return Some(Vec::new());
    }
    let remotes = git_strict(dir, &["remote"], reads).await?;
    let mut urls: Vec<String> = Vec::new();
    for remote in remotes
        .lines()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .take(FETCH_REMOTES)
    {
        let configured = git_strict(dir, &["remote", "get-url", "--all", remote], reads).await?;
        for raw in configured.lines().map(str::trim).filter(|u| !u.is_empty()) {
            let display = fetch_head_display_url(raw);
            if !display.is_empty() && !urls.contains(&display) {
                urls.push(display);
            }
        }
    }
    if urls.is_empty() {
        return Some(Vec::new());
    }
    let mut shas: Vec<String> = Vec::new();
    for line in text.lines() {
        if shas.len() >= FETCH_HEAD_COMMITS {
            break;
        }
        let Some((sha, rest)) = line.split_once('\t') else {
            continue;
        };
        let Some((merge, note)) = rest.split_once('\t') else {
            continue;
        };
        if !is_head(sha) || !matches!(merge, "" | "not-for-merge") {
            continue;
        }
        let note = note.trim_end_matches('\r');
        let from_remote = urls
            .iter()
            .any(|u| note == u || note.ends_with(&format!("' of {u}")));
        if from_remote && !shas.iter().any(|s| s == sha) {
            shas.push(sha.to_string());
        }
    }
    commits_available(dir, &shas, reads).await
}

/// Those of `shas` that are present commits in `dir` (`cat-file --batch-check` of
/// `<sha>^{commit}`); a missing or non-commit object is skipped, and any other or incomplete
/// answer is `None`.
async fn commits_available(dir: &Path, shas: &[String], reads: &Reads) -> Option<Vec<String>> {
    if shas.is_empty() {
        return Some(Vec::new());
    }
    let mut query = String::new();
    for sha in shas {
        query.push_str(sha);
        query.push_str("^{commit}\n");
    }
    let reply = run_git(
        dir,
        &["cat-file", "--batch-check=%(objectname) %(objecttype)"],
        reads,
        Mode::STRICT,
        Feed::Bytes(query.into_bytes()),
    )
    .await?;
    let lines: Vec<&str> = reply.lines().collect();
    if lines.len() != shas.len() {
        reads.incomplete.store(true, Ordering::SeqCst);
        return None;
    }
    let mut commits: Vec<String> = Vec::new();
    for (sha, line) in shas.iter().zip(lines) {
        let line = line.trim_end_matches('\r');
        if let Some(resolved) = line.strip_suffix(" commit")
            && is_head(resolved)
        {
            if !commits.iter().any(|c| c == resolved) {
                commits.push(resolved.to_string());
            }
        } else if line == format!("{sha}^{{commit}} missing") {
            continue;
        } else {
            reads.incomplete.store(true, Ordering::SeqCst);
            return None;
        }
    }
    Some(commits)
}

/// The anonymized, suffix-trimmed URL git's `store_updated_refs` writes into `FETCH_HEAD` for a
/// configured remote URL `raw` (aterm's `fetch_head_display_url`): `scheme://user@host/...`
/// without `user@`, a trailing `/` stripped once, then a trailing `.git` stripped once.
pub(crate) fn fetch_head_display_url(raw: &str) -> String {
    let anon = match raw.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, path) = rest
                .split_once('/')
                .map_or((rest, None), |(a, p)| (a, Some(p)));
            let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
            match path {
                Some(p) => format!("{scheme}://{host}/{p}"),
                None => format!("{scheme}://{host}"),
            }
        }
        None => raw.to_string(),
    };
    let trimmed = anon.strip_suffix('/').unwrap_or(&anon);
    let stripped = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    if stripped.is_empty() {
        anon
    } else {
        stripped.to_string()
    }
}

/// THE BRANCHES THE SESSION USED ([`Used`]) besides `HEAD`'s own `head`, in the repository at
/// `root`: those this worktree's own `HEAD` reflog (its last [`LAG_REFLOG`] bytes) moved from
/// or to at or after `since`, then `names` (those its commands named in this repository, and
/// those whose folder could not be placed), each once and at most [`USED_BRANCHES`] (aterm's
/// `used_branches`). Another worktree's moves are in its own reflog, so its branches are never
/// this session's. `None`, cut, where a read is refused or cut, or the reflog lies off local
/// mounts alone.
async fn used_branches(
    root: &Path,
    since: Option<u64>,
    names: &[String],
    head: Option<&str>,
    reads: &Reads,
) -> Option<Vec<String>> {
    let mut moved: Vec<String> = Vec::new();
    if let Some(since) = since {
        let Some(log) = read_reflog(root, "logs/HEAD", reads).await else {
            reads.incomplete.store(true, Ordering::SeqCst);
            return None;
        };
        // No reflog file (none kept, or a `reftable` repository): no move is recorded.
        for line in log.as_deref().unwrap_or_default().lines().rev() {
            // `<old> <new> <who> <when> <zone>\t<why>`
            let Some((_, why)) = line.split_once('\t') else {
                continue;
            };
            if !reflog_time(line).is_some_and(|when| when >= since) {
                continue;
            }
            if let Some((from, to)) = why
                .strip_prefix("checkout: moving from ")
                .and_then(|moved| moved.rsplit_once(" to "))
            {
                moved.extend([to, from].map(str::to_string));
            } else if let Some(back) = why.strip_prefix("rebase (finish): returning to refs/heads/")
            {
                moved.push(back.to_string());
            }
        }
    }
    moved.extend(names.iter().cloned());
    let mut kept: Vec<String> = Vec::new();
    for name in moved {
        if kept.len() < USED_BRANCHES
            && branch_name_ok(&name)
            && Some(name.as_str()) != head
            && !kept.contains(&name)
        {
            kept.push(name);
        }
    }
    Some(kept)
}

/// The last [`LAG_REFLOG`] bytes of the repository at `root`'s reflog `log` (`logs/HEAD`,
/// `logs/refs/stash`), found by `git rev-parse --git-path`, from a whole line: `Some(None)`
/// where there is none, `None` where a read is refused or cut, the file lies off local mounts
/// alone, or it cannot be read.
async fn read_reflog(root: &Path, log: &str, reads: &Reads) -> Option<Option<String>> {
    let at = git_strict(root, &["rev-parse", "--git-path", log], reads).await?;
    let Some(file) = open_git_file(root, &at, reads).ok()? else {
        return Some(None);
    };
    reads.fs_step();
    let mut file = tail_of(file)?;
    let mut bytes = Vec::new();
    reads.fs_step();
    std::io::Read::read_to_end(&mut file, &mut bytes).ok()?;
    Some(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

/// When a reflog `line` (`<old> <new> <who> <when> <zone>`, then `\t<why>` where it has a
/// message) was written, in Unix seconds.
fn reflog_time(line: &str) -> Option<u64> {
    let who = line.split('\t').next()?;
    who.trim_end().rsplit(' ').nth(1)?.parse().ok()
}

/// WHETHER THE SESSION CHANGED THE REPOSITORY AT `root` SINCE IT BEGAN (`since`, Unix seconds),
/// for a repository only a folder it touched led to (review of the forever probe at f48a215d7b,
/// R2: a dependency's checkout it merely looked in held Land for the rest of the session): its
/// `HEAD` reflog or its stash's holds an entry at or after it, or a path `listed` (the `-z`
/// porcelain status with every untracked file listed alone, a rename's both paths) changed at
/// or after it, by its inode's change time, which no tool sets back (for a path with no file,
/// the nearest folder above it that is left, which its removal changed). True wherever it
/// cannot tell: no start, no `HEAD` reflog, a path that is a folder (a submodule or a nested
/// repository, whose own changes are not dated here), more than [`STATE_FILES`] paths, a path
/// it cannot look at, no listing, or a refused read (also cut, [`Reads::incomplete`]).
async fn changed_since(
    root: &Path,
    since: Option<u64>,
    listed: Option<&str>,
    reads: &Reads,
) -> bool {
    let Some(since) = since else {
        return true;
    };
    for (log, absent_moved) in [("logs/HEAD", true), ("logs/refs/stash", false)] {
        match read_reflog(root, log, reads).await {
            None => {
                reads.incomplete.store(true, Ordering::SeqCst);
                return true;
            }
            Some(None) if absent_moved => return true,
            Some(None) => {}
            Some(Some(text)) => {
                if text
                    .lines()
                    .any(|line| reflog_time(line).is_none_or(|when| when >= since))
                {
                    return true;
                }
            }
        }
    }
    let Some(listed) = listed else {
        return true;
    };
    let mut fields = listed.split('\0');
    let mut count = 0usize;
    while let Some(entry) = fields.next() {
        let (Some(code), Some(path)) = (entry.get(..2), entry.get(3..)) else {
            continue;
        };
        let from = if code.starts_with(['R', 'C']) {
            fields.next()
        } else {
            None
        };
        for path in std::iter::once(path).chain(from) {
            count += 1;
            if count > STATE_FILES {
                return true;
            }
            reads.fs_step();
            if path_changed(root, path, since) {
                return true;
            }
        }
    }
    false
}

/// Whether `path` (a status path of the repository at `root`) changed at or after `since` (Unix
/// seconds), by its inode's change time, never following a link: a folder (a submodule or a
/// nested repository) always; for a path with no file, the nearest folder above it, within
/// `root`, that is left; any look that fails otherwise, always.
fn path_changed(root: &Path, path: &str, since: u64) -> bool {
    let at = root.join(path.trim_end_matches('/'));
    let mut look = at.as_path();
    loop {
        match std::fs::symlink_metadata(look) {
            Ok(meta) => return (look == at && meta.is_dir()) || changed_at(&meta) >= since,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => match look.parent() {
                Some(up) if up.starts_with(root) => look = up,
                _ => return true,
            },
            Err(_) => return true,
        }
    }
}

/// When a file's inode last changed, in Unix seconds (its modification time where the system
/// keeps no change time).
fn changed_at(meta: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        u64::try_from(meta.ctime()).unwrap_or(0)
    }
    #[cfg(not(unix))]
    {
        meta.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(u64::MAX, |d| d.as_secs())
    }
}

/// THE COMMITS ON THE BRANCHES `names` that neither `HEAD` nor `main` (the remote main's full
/// ref; with none, any remote-tracking ref) has, and the branches holding them by name: each
/// local branch, and where `main` is `origin`'s, `origin`'s branch of that name, which a session
/// that pushed its branch there and deleted its own still parked its work on (aterm's
/// `parked_on`, D2-3, R-2). A branch since deleted counts nothing. `None` where a read is
/// refused or cut, or git omits reference data with a warning.
async fn parked_on(
    root: &Path,
    names: &[String],
    main: Option<&str>,
    reads: &Reads,
) -> Option<Beyond> {
    if names.is_empty() {
        return Some(Beyond::default());
    }
    let origin = main.is_some_and(|m| m.starts_with(ORIGIN_REFS));
    let mut refs: Vec<(String, &str)> = names
        .iter()
        .map(|n| (format!("refs/heads/{n}"), n.as_str()))
        .collect();
    if origin {
        refs.extend(
            names
                .iter()
                .map(|n| (format!("{ORIGIN_REFS}{n}"), n.as_str())),
        );
    }
    let mut enumerate = vec!["for-each-ref", "--format=%(refname)"];
    enumerate.extend(refs.iter().map(|(name, _)| name.as_str()));
    let existing = git_strict(root, &enumerate, reads).await?;
    refs.retain(|(name, _)| existing.lines().any(|line| line == name));
    if refs.is_empty() {
        return Some(Beyond::default());
    }
    let max = format!("--max-count={BEYOND_COMMITS}");
    let mut args = vec![
        "log",
        "--no-show-signature",
        "--no-color",
        max.as_str(),
        "--format=%H%x09%S",
    ];
    args.extend(refs.iter().map(|(r, _)| r.as_str()));
    args.extend(["--not", main.unwrap_or("--remotes"), "HEAD"]);
    let listing = git_strict(root, &args, reads).await?;
    let mut beyond = Beyond::default();
    let mut on: Vec<&str> = Vec::new();
    for line in listing.lines().filter(|l| !l.trim().is_empty()) {
        beyond.count = beyond.count.saturating_add(1);
        if let Some(&(_, name)) = line
            .split_once('\t')
            .and_then(|(_, source)| refs.iter().find(|(r, _)| r == source))
            && !on.contains(&name)
        {
            on.push(name);
        }
    }
    on.sort_unstable();
    beyond.names = on.into_iter().take(NAMED_PATHS).map(printable).collect();
    beyond.listing = listing;
    Some(beyond)
}

/// THE WORK'S STATE ([`RepoWork::state`]) of the repository at `root`: `head` its `HEAD`
/// commit (empty where it has none), `stash` its branch's stash entries ([`stashes_on`]),
/// `listed` the NUL-separated `git status --porcelain -z --untracked-files=all`, each entry
/// hashed with its status code and the file's size and modification time (`-` for a path with
/// no file, a deletion). `None` past [`STATE_FILES`] entries. Reads no file's contents, and
/// follows no symbolic link.
#[must_use]
pub fn state_of(root: &Path, head: &str, stash: &str, listed: &str) -> Option<u64> {
    let mut text = format!("{head}\n{stash}\n");
    let mut fields = listed.split('\0');
    let mut count = 0usize;
    while let Some(entry) = fields.next() {
        let (Some(code), Some(path)) = (entry.get(..2), entry.get(3..)) else {
            continue;
        };
        if code.starts_with(['R', 'C']) {
            fields.next();
        }
        count += 1;
        if count > STATE_FILES {
            return None;
        }
        let stat = std::fs::symlink_metadata(root.join(path)).ok().map_or_else(
            || "-".to_string(),
            |m| {
                let modified = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos());
                format!("{} {modified}", m.len())
            },
        );
        text.push_str(&format!("{code} {path} {stat}\n"));
    }
    Some(crate::needs::fnv1a_64(text.as_bytes()))
}

/// THE STASH ENTRIES OF ONE BRANCH among `list` (`git stash list --format=%gs`: each entry's
/// subject, one a line), one a line: those Git wrote on `branch` (`WIP on <branch>: …`, or `On
/// <branch>: …` for a message given), `None` for a detached `HEAD` (`(no branch)`). A
/// repository's stash is shared by its worktrees; an entry made on another branch is another
/// worktree's, another session's, work.
#[must_use]
pub fn stashes_on(list: &str, branch: Option<&str>) -> String {
    let name = branch.unwrap_or("(no branch)");
    let (wip, on) = (format!("WIP on {name}: "), format!("On {name}: "));
    list.lines()
        .filter(|l| l.starts_with(&wip) || l.starts_with(&on))
        .map(|l| format!("{l}\n"))
        .collect()
}

/// WHETHER THE REPOSITORY AT `root` SETS ITS OWN `core.excludesFile`: its own configuration
/// (`git config --includes --get`, the system and global files shut out as for every read
/// here, so the local file, a worktree's and what they include answer). Where it does, Git
/// reads that one and never the person's global one ([`global_excludes`]), and the probe hands
/// theirs over not at all: a `-c` would override the repository's own.
async fn own_excludes(root: &Path, reads: &Reads) -> bool {
    git(
        root,
        &["config", "--includes", "--get", "core.excludesFile"],
        reads,
    )
    .await
    .is_some_and(|p| !p.trim().is_empty())
}

/// THE PERSON'S GLOBAL EXCLUDES FILE, which the probe's `git`, run with no global
/// configuration (the module's rules), would not read: `core.excludesFile` as their global
/// configuration sets it, the files it includes followed (`git config --global --includes
/// --path --get`, with their home and `XDG_CONFIG_HOME` and nothing else), else Git's
/// default, `<xdg>/git/ignore` (`<home>/.config/git/ignore` where `xdg` is unset). The read
/// runs in the repository at `root`, so a conditional include (`includeIf "gitdir:…"`,
/// `"onbranch:…"`) is judged against it as in their own `git status` (run from `/`, none
/// matched). Only an absolute file that resolves over local mounts alone ([`resolve_local`])
/// outside the folders macOS guards, as spelled and once resolved (aterm's rule), and in the
/// file system's own spelling too ([`canonical_outside`]); `None` otherwise.
async fn global_excludes(root: &Path, reads: &Reads) -> Option<PathBuf> {
    let home = reads.home.as_path();
    let set = git_as(
        root,
        &[
            "config",
            "--global",
            "--includes",
            "--path",
            "--get",
            "core.excludesFile",
        ],
        reads,
        true,
    )
    .await
    .map(|p| p.trim_end_matches(['\n', '\r']).to_string())
    .filter(|p| !p.is_empty())
    .map(PathBuf::from);
    let path = set.unwrap_or_else(|| {
        reads
            .xdg
            .clone()
            .unwrap_or_else(|| home.join(".config"))
            .join("git")
            .join("ignore")
    });
    if !path.is_absolute()
        || path.components().any(|c| c == Component::ParentDir)
        || under_protected_root(home, &path)
    {
        return None;
    }
    let real = resolve_local(&path, &reads.table)?;
    if under_protected_root(home, &real) {
        return None;
    }
    let real_home = resolve_local(home, &reads.table)?;
    canonical_outside(&real, &real_home).filter(|p| p.is_file())
}

/// THE REMOTE'S DEFAULT BRANCH in the repository at `root`, as a full ref, for
/// [`RepoWork::unlanded`]: `refs/remotes/origin/HEAD`'s target, else
/// `refs/remotes/origin/main`, else `refs/remotes/origin/master` ([`DEFAULT_BRANCHES`]) —
/// the first that names a commit. `None` where none does (the count is unknown). A dangling
/// `origin/HEAD` (its branch renamed on the remote and pruned here, of which git only warns)
/// names none, so the next is tried: one counted against would fail, and so find nothing
/// (review of 2a9241bbf, finding 14).
async fn main_ref(root: &Path, reads: &Reads) -> Option<String> {
    let head = git(
        root,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
        reads,
    )
    .await
    .map(|r| r.trim().to_string())
    .filter(|r| r.starts_with("refs/remotes/"));
    for full in head
        .into_iter()
        .chain(DEFAULT_BRANCHES.iter().map(|b| (*b).to_string()))
    {
        let commit = format!("{full}^{{commit}}");
        if git(root, &["rev-parse", "--verify", "--quiet", &commit], reads)
            .await
            .is_some()
        {
            return Some(full);
        }
    }
    None
}

/// The most submodules [`without_lagging_submodules`] reads one by one: any past them stay.
const LAG_READS: usize = 8;

/// How much of the end of `HEAD`'s reflog [`without_lagging_submodules`] reads: a thousand or
/// more of its moves.
pub(crate) const LAG_REFLOG: u64 = 256 * 1024;

/// Whether `text` is a commit name `git rev-parse --verify` prints: 40 hex digits (SHA-1) or
/// 64 (SHA-256).
fn is_head(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `status` (`git status --porcelain=v1 -z` of the repository at `root`) without each submodule
/// whose checkout only lags its record: nothing edited or untracked in it (as far as the
/// repository's own submodule settings let git see), and its `HEAD` the commit this checkout
/// recorded for it at a point its own `HEAD` moved from or to, in the last [`LAG_REFLOG`] bytes
/// of that reflog.
async fn without_lagging_submodules(
    root: &Path,
    status: &str,
    excludes: Option<&str>,
    reads: &Reads,
) -> String {
    let mut submodules = Vec::new();
    let mut fields = status.split('\0');
    while let Some(entry) = fields.next() {
        let (Some(code), Some(path)) = (entry.get(..2), entry.get(3..)) else {
            continue;
        };
        if code.starts_with(['R', 'C']) {
            fields.next();
        }
        if code == " M" && submodules.len() < LAG_READS && !path.is_empty() && {
            reads.fs_step();
            submodule_candidate(root, path)
        } {
            submodules.push(path);
        }
    }
    if submodules.is_empty() {
        return status.to_string();
    }
    let mut args: Vec<&str> = Vec::new();
    if let Some(flag) = excludes {
        args.extend(["-c", flag]);
    }
    args.extend([
        "status",
        "--porcelain=v2",
        "-z",
        "--untracked-files=normal",
        "--",
    ]);
    args.extend(submodules);
    let Some(described) = git_offline(root, &args, reads, None).await else {
        return status.to_string();
    };
    let mut moved = Vec::new();
    let mut fields = described.split('\0');
    while let Some(entry) = fields.next() {
        if entry.starts_with("2 ") {
            fields.next();
            continue;
        }
        let Some(rest) = entry.strip_prefix("1 ") else {
            continue;
        };
        let parts: Vec<&str> = rest.splitn(8, ' ').collect();
        if let &[".M", "SC..", _, _, _, _, recorded, path] = parts.as_slice() {
            moved.push((path, recorded));
        }
    }
    if moved.is_empty() {
        return status.to_string();
    }
    // A reflog off local mounts alone (a linked worktree's common directory on a share) is
    // never opened: every entry stays.
    let Some(reflog) = git_offline(root, &["rev-parse", "--git-path", "logs/HEAD"], reads, None)
        .await
        .and_then(|at| open_git_file(root, &at, reads).ok().flatten())
        .and_then(tail_of)
    else {
        return status.to_string();
    };
    let mut args = vec![
        "diff-tree",
        "--stdin",
        "-r",
        "--raw",
        "-z",
        "--no-abbrev",
        "--",
    ];
    args.extend(moved.iter().map(|(path, _)| *path));
    let Some(history) = git_offline(root, &args, reads, Some(reflog)).await else {
        return status.to_string();
    };
    let before = records_of(&history);
    let mut lagging = Vec::new();
    for (path, recorded) in moved {
        let Some(head) = git_offline(
            &root.join(path),
            &["rev-parse", "--verify", "--quiet", "HEAD"],
            reads,
            None,
        )
        .await
        else {
            continue;
        };
        let head = head.trim();
        if is_head(head) && head != recorded && before.contains(&(path, head)) {
            lagging.push(path);
        }
    }
    if lagging.is_empty() || reads.incomplete.load(Ordering::SeqCst) {
        return status.to_string();
    }
    let mut kept = String::with_capacity(status.len());
    let mut fields = status.split('\0');
    while let Some(entry) = fields.next() {
        if entry.is_empty() {
            continue;
        }
        let renamed = entry.starts_with(['R', 'C']);
        let gone = entry
            .strip_prefix(" M ")
            .is_some_and(|path| lagging.contains(&path));
        if !gone {
            kept.push_str(entry);
            kept.push('\0');
        }
        if renamed && let Some(from) = fields.next() {
            kept.push_str(from);
            kept.push('\0');
        }
    }
    kept
}

/// WHETHER STATUS ENTRY `path` IN `root` MAY BE A CHECKED-OUT SUBMODULE: a folder that is no
/// link (a gitlink's checkout never is one) holding its own `.git`, each looked at without
/// following a link, so a tracked link retargeted onto a stalled share is never stat'ed through
/// (review of the forever probe, hangs-1).
pub(crate) fn submodule_candidate(root: &Path, path: &str) -> bool {
    let at = root.join(path);
    std::fs::symlink_metadata(&at).is_ok_and(|m| m.is_dir())
        && std::fs::symlink_metadata(at.join(".git")).is_ok()
}

/// The reflog file at `path`, read-only, at the first whole line of its last [`LAG_REFLOG`]
/// bytes; `None` where it is no regular file or cannot be read: the reads behind
/// [`open_git_file`] and [`tail_of`], with no mount table (tests).
#[cfg(test)]
pub(crate) fn reflog_tail(path: &Path) -> Option<std::fs::File> {
    open_regular(path).and_then(tail_of)
}

/// The open regular `file` at the first whole line of its last [`LAG_REFLOG`] bytes; `None`
/// where it cannot be read.
pub(crate) fn tail_of(mut file: std::fs::File) -> Option<std::fs::File> {
    use std::io::Read as _;
    use std::io::Seek as _;
    use std::io::SeekFrom;
    let len = file
        .metadata()
        .ok()
        .filter(std::fs::Metadata::is_file)?
        .len();
    let from = len.saturating_sub(LAG_REFLOG);
    if from > 0 {
        file.seek(SeekFrom::Start(from - 1)).ok()?;
        let mut head = [0_u8; 4096];
        let read = file.read(&mut head).ok()?;
        let newline = head.get(..read)?.iter().position(|&b| b == b'\n')?;
        file.seek(SeekFrom::Start(from + u64::try_from(newline).ok()?))
            .ok()?;
    }
    Some(file)
}

/// The `(path, commit)` records `history` (`git diff-tree --raw -z --no-abbrev`) names.
fn records_of(history: &str) -> std::collections::HashSet<(&str, &str)> {
    let mut records = std::collections::HashSet::new();
    let mut fields = history.split('\0');
    while let Some(field) = fields.next() {
        let Some(meta) = field.trim_start_matches('\n').strip_prefix(':') else {
            continue;
        };
        let &[from_mode, to_mode, from, to, change] =
            meta.split(' ').collect::<Vec<_>>().as_slice()
        else {
            continue;
        };
        let Some(from_path) = fields.next() else {
            break;
        };
        let to_path = if change.starts_with(['R', 'C']) {
            fields.next().unwrap_or(from_path)
        } else {
            from_path
        };
        if from_mode == "160000" {
            records.insert((from_path, from));
        }
        if to_mode == "160000" {
            records.insert((to_path, to));
        }
    }
    records
}

/// What every read of one probe shares ([`git`]): the person's home and `XDG_CONFIG_HOME`, and
/// the mount table its folders are resolved over.
struct Reads {
    /// The person's home: every read's `HOME`, so a `~/` in a path the repository's own
    /// configuration names expands as in their own `git` (with none, Git dies on it at the
    /// first read: `fatal: failed to expand user dir`, and the probe found nothing).
    home: PathBuf,
    /// The person's `XDG_CONFIG_HOME` (absolute), for [`global_excludes`]' read alone.
    xdg: Option<PathBuf>,
    /// The mount table, read once a probe ([`unattended_dir`]).
    table: Table,
    /// Shared deadline across all reads of this probe (`None`: expired at start).
    deadline: Option<tokio::time::Instant>,
    /// A child could not start, was interrupted, or returned an incomplete read.
    incomplete: AtomicBool,
    /// A test's stall before each synchronous file system step ([`probe_stalled`]) and its
    /// refusals ([`probe_refusing`]).
    #[cfg(test)]
    seams: Seams,
}

impl Reads {
    /// Before each synchronous file system step a reader takes: a test's stall
    /// ([`probe_stalled`]); nothing otherwise.
    fn fs_step(&self) {
        #[cfg(test)]
        if let Some(stall) = &self.seams.stall {
            stall();
        }
    }
}

/// How one read runs ([`run_git`]).
#[derive(Clone, Copy)]
struct Mode {
    /// The one read of the person's global configuration ([`global_excludes`]).
    global: bool,
    /// Nothing it reads is fetched (`GIT_NO_LAZY_FETCH`), and a refusal is no cut unless
    /// `strict`.
    offline: bool,
    /// A count or proof the probe's answer rests on: a refusal, or any diagnostic on its
    /// standard error, is cut ([`Reads::incomplete`]), never an absence, but for git's own quiet
    /// absence answers (exit 1 with nothing on standard error of `config`, `symbolic-ref`,
    /// `rev-parse --verify --quiet` and `cat-file -e`).
    strict: bool,
}

impl Mode {
    const PLAIN: Self = Self {
        global: false,
        offline: false,
        strict: false,
    };
    const GLOBAL: Self = Self {
        global: true,
        ..Self::PLAIN
    };
    const OFFLINE: Self = Self {
        offline: true,
        ..Self::PLAIN
    };
    const STRICT: Self = Self {
        offline: true,
        strict: true,
        ..Self::PLAIN
    };
}

/// A read's standard input ([`run_git`]).
enum Feed {
    Null,
    File(std::fs::File),
    Bytes(Vec<u8>),
}

/// `git <args>` in `dir`, with complete UTF-8 stdout and a successful exit; the child is
/// killed when the probe is dropped (its limit passed).
async fn git(dir: &Path, args: &[&str], reads: &Reads) -> Option<String> {
    git_as(dir, args, reads, false).await
}

async fn git_offline(
    dir: &Path,
    args: &[&str],
    reads: &Reads,
    feed: Option<std::fs::File>,
) -> Option<String> {
    run_git(
        dir,
        args,
        reads,
        Mode::OFFLINE,
        feed.map_or(Feed::Null, Feed::File),
    )
    .await
}

/// A strict, offline read ([`Mode::STRICT`]): the counts and proofs a clean reading rests on.
async fn git_strict(dir: &Path, args: &[&str], reads: &Reads) -> Option<String> {
    run_git(dir, args, reads, Mode::STRICT, Feed::Null).await
}

/// Git also calls a corrupt worktree a non-repository. Only accept discovery absence
/// where no ancestor has repository metadata. Each `.git` candidate needs its own local
/// mount admission: a child mount is not covered by its parent's. No final symlink is
/// followed and no metadata contents are opened. An incomplete table proves nothing.
pub(crate) fn no_worktree_metadata(dir: &Path, table: &Table) -> bool {
    if !table.complete {
        return false;
    }
    dir.ancestors().all(|ancestor| {
        let candidate = ancestor.join(".git");
        if !crate::mounts::mount_of(&candidate, table)
            .is_some_and(|mount| mount.local && !mount.automounted)
        {
            return false;
        }
        matches!(
            std::fs::symlink_metadata(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        )
    })
}

/// [`git`], or with `global` the one read of the person's global configuration
/// [`global_excludes`] makes: their `XDG_CONFIG_HOME` set, the global configuration not shut
/// out. Every other read has their `HOME` (for `~/` alone) with the global configuration shut
/// out and `XDG_CONFIG_HOME` pointed at nothing, so Git's own default files under it
/// (`git/ignore`, `git/attributes`) are never read unchecked: the person's default excludes
/// reach the status reads only through [`global_excludes`]' checks.
///
/// STRICTER THAN aterm, KEPT: `-c safe.bareRepository=explicit`, Git's own hardening for a
/// program that runs it unattended in folders it did not choose: the probe reads the folder
/// the worker's last command ran in, which may be inside a clone nobody vetted, and a
/// repository directory planted in a work tree (the embedded bare repository) is refused at
/// discovery, its configuration never read. It changes no answer for an ordinary layout
/// (measured on Git 2.54: a bare repository, a worktree of one, a folder inside a `.git` and
/// a repository's own folder answer alike with and without it; a bare repository has no work
/// tree, so `--show-toplevel` fails either way). The one answer it changes is a folder inside
/// such a planted directory whose configuration names a work tree (`core.bare=false` with
/// `core.worktree`): aterm then probes whatever work tree that configuration names, possibly
/// another repository altogether; here that folder is in no repository, and the probe reads
/// the session's other folders ([`probe_session`]), finding nothing when none is left.
async fn git_as(dir: &Path, args: &[&str], reads: &Reads, global: bool) -> Option<String> {
    let mode = if global { Mode::GLOBAL } else { Mode::PLAIN };
    run_git(dir, args, reads, mode, Feed::Null).await
}

async fn run_git(
    dir: &Path,
    args: &[&str],
    reads: &Reads,
    mode: Mode,
    feed: Feed,
) -> Option<String> {
    if reads.incomplete.load(Ordering::SeqCst) {
        return None;
    }
    // A test's refused read is git's own refusal (exit 128 with a diagnostic), weighed by the
    // same rule ([`failed`]).
    #[cfg(test)]
    if reads
        .seams
        .refuse
        .as_ref()
        .is_some_and(|refuse| refuse(dir, args))
    {
        return failed(dir, args, reads, mode, Some(128), b"fatal: refused\n");
    }
    let Some(deadline) = reads.deadline.filter(|&d| tokio::time::Instant::now() < d) else {
        reads.incomplete.store(true, Ordering::SeqCst);
        return None;
    };
    let Some(path) = std::env::var_os("PATH") else {
        reads.incomplete.store(true, Ordering::SeqCst);
        return None;
    };
    let mut cmd = tokio::process::Command::from(codex_utils_process::background_command("git"));
    cmd.arg("--no-pager")
        .args(["-c", "core.fsmonitor=false"])
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(["-c", "safe.bareRepository=explicit"])
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .env("HOME", &reads.home);
    if mode.global {
        if let Some(xdg) = &reads.xdg {
            cmd.env("XDG_CONFIG_HOME", xdg);
        }
    } else {
        cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("XDG_CONFIG_HOME", "/dev/null");
    }
    if mode.offline {
        cmd.env("GIT_NO_LAZY_FETCH", "1");
    }
    let bytes = match feed {
        Feed::Null => {
            cmd.stdin(Stdio::null());
            None
        }
        Feed::File(file) => {
            cmd.stdin(Stdio::from(file));
            None
        }
        Feed::Bytes(bytes) => {
            cmd.stdin(Stdio::piped());
            Some(bytes)
        }
    };
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let run = async {
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take();
        // The input is written while the output is read, so neither pipe can fill and stall
        // the other; its end closes the child's standard input.
        let write = async move {
            if let (Some(mut stdin), Some(bytes)) = (stdin, bytes) {
                use tokio::io::AsyncWriteExt as _;
                let _ = stdin.write_all(&bytes).await;
            }
        };
        let ((), output) = tokio::join!(write, child.wait_with_output());
        output
    };
    let output = match tokio::time::timeout_at(deadline, run).await {
        Ok(Ok(output)) => output,
        Ok(Err(_)) | Err(_) => {
            reads.incomplete.store(true, Ordering::SeqCst);
            return None;
        }
    };
    if !output.status.success() {
        return failed(dir, args, reads, mode, output.status.code(), &output.stderr);
    }
    // Status can succeed after omitting an unreadable untracked directory. Its
    // C-locale read warning is incomplete evidence, even with empty porcelain. A strict read
    // that said anything on its standard error (a broken ref it skipped) counted short.
    if output
        .stderr
        .split(|byte| *byte == b'\n')
        .any(|line| line.starts_with(b"warning: could not open directory '"))
        || (mode.strict && !output.stderr.iter().all(u8::is_ascii_whitespace))
    {
        reads.incomplete.store(true, Ordering::SeqCst);
        return None;
    }
    match String::from_utf8(output.stdout) {
        Ok(text) => Some(text),
        Err(_) => {
            reads.incomplete.store(true, Ordering::SeqCst);
            None
        }
    }
}

/// A read git ended with a failure (`code`, `None` for a signal, and its standard error):
/// always no answer, cut ([`Reads::incomplete`]) unless it is a quiet absence. A plain offline
/// read's failed exit is no cut; otherwise only git's own absence answers are none: exit 1
/// (with nothing on standard error where `strict`) of `config`, `symbolic-ref`,
/// `rev-parse --verify --quiet` and `cat-file -e`, and a folder discovery finds in no
/// repository.
fn failed(
    dir: &Path,
    args: &[&str],
    reads: &Reads,
    mode: Mode,
    code: Option<i32>,
    stderr: &[u8],
) -> Option<String> {
    if mode.offline && !mode.strict && code.is_some() {
        return None;
    }
    // Provenance reads disable object replacement without changing Git's quiet
    // absence answers. Keep strict exit/status checks after this fixed prefix.
    let args = args.strip_prefix(&["--no-replace-objects"]).unwrap_or(args);
    // These are successful absence checks, not interrupted repository reads.
    let absent = code == Some(1)
        && (!mode.strict || stderr.is_empty())
        && (args.first() == Some(&"config")
            || args.first() == Some(&"symbolic-ref")
            || args.starts_with(&["rev-parse", "--verify", "--quiet"])
            || args.starts_with(&["cat-file", "-e"]));
    let no_repository = args == ["rev-parse", "--show-toplevel"]
        && code == Some(128)
        && ((stderr.starts_with(b"fatal: not a git repository (")
            && no_worktree_metadata(dir, &reads.table))
            || [
                "fatal: this operation must be run in a work tree",
                "fatal: cannot use bare repository",
            ]
            .iter()
            .any(|reason| stderr.starts_with(reason.as_bytes())));
    if !absent && !no_repository {
        reads.incomplete.store(true, Ordering::SeqCst);
    }
    None
}
