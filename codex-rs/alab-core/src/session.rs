// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The one in-process state codex-alab's pieces share (CONTRACT §4-§5).
//!
//! The TUI hooks (`codex-rs/alab-tui`) write it: the launch snapshot taken first thing in
//! `main`, what each frame shows (threads, cwd, model, program state, the draft, the
//! person's queued input and the background terminals its footer counts), the human-input
//! stamp, and the exit's signal or exec request.
//! The session-record writer and, later, the autopilot read it. It is one `static` in one
//! crate, so the TUI and the in-process app-server extension see the same copy (tui-life §6).
//! Shared state performs no I/O; post-commit capture has a separate owned operation.

use crate::boxes::BoxAnswer;
use crate::boxes::BoxOutcome;
use crate::boxes::BoxSlot;
use crate::boxes::OpenBox;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

/// Upstream's line between an `/ide` context and the person's request
/// (`tui/src/ide_context/prompt.rs`, `PROMPT_REQUEST_BEGIN`).
const PROMPT_REQUEST_MARKER: &str = "\n## My request for Codex:\n";

/// Maximum UTF-8 bytes of the primary thread's active goal objective kept in the session record.
pub const GOAL_MAX_BYTES: usize = 8192;

/// Milliseconds since the Unix epoch (0 if the clock is before it).
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// A process's identity, timezone- and locale-free, as upstream's daemon pidfile keeps it
/// (`app-server-daemon/src/backend/pid_identity.rs`). `execve` keeps every field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessIdentity {
    Linux {
        boot_id: String,
        start_ticks: u64,
    },
    MacOs {
        boot_id: String,
        unique_id: u64,
        start_seconds: u64,
        start_microseconds: u64,
    },
}

/// What `main` saw before anything changed it: arg0 rewrites `PATH` and `.env` is merged
/// into the environment right after, so a relaunch or re-exec builds from this.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Launch {
    pub args: Vec<OsString>,
    pub vars: Vec<(OsString, OsString)>,
    pub cwd: Option<PathBuf>,
    pub current_exe: Option<PathBuf>,
    /// The (device, inode) of the file `current_exe` named at launch: the build this image
    /// runs, however its path is later re-pointed or replaced (the live upgrade compares it).
    pub exe_id: Option<(u64, u64)>,
    /// `args[0]` as the launch `PATH` resolved it (never canonicalized: atpkg's twin or
    /// reroute path, so a relaunch runs the build that path names then).
    pub argv0: Option<PathBuf>,
    /// The admitted atpkg route when its shim had to exec the physical image directly.
    /// This may be missing until atpkg repairs it; it never changes the running image.
    pub managed_argv0: Option<PathBuf>,
    pub pid: u32,
    pub identity: Option<ProcessIdentity>,
    /// The start time in `ps -o lstart` form, as Claude Code's records carry it.
    pub proc_start: Option<String>,
    pub started_at_ms: u64,
}

impl Launch {
    /// The route followed for update monitoring and resuming, independent of the physical
    /// launch/image identity retained in `argv0`, `current_exe` and `exe_id`.
    pub fn upgrade_path(&self) -> Option<&Path> {
        self.managed_argv0.as_deref().or(self.argv0.as_deref())
    }

    /// A launch-time environment variable.
    pub fn var(&self, name: &str) -> Option<&OsStr> {
        self.vars
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_os_str())
    }

    /// The aterm session this process was launched in ([`ATERM_SESSION_ENV`]), if any.
    pub fn aterm_session(&self) -> Option<&str> {
        self.var(ATERM_SESSION_ENV)
            .and_then(OsStr::to_str)
            .filter(|sid| !sid.trim().is_empty())
    }
}

/// The environment variable naming the aterm session (`s-<hex>`) a process runs in, which
/// aterm sets for every program in a tab (never `ATERM_SESSION_ID`, a nested aterm's
/// preminted identity): the key of aterm's own books for that tab.
pub const ATERM_SESSION_ENV: &str = "ATERM_PARENT_SESSION_ID";

/// WHETHER A DEAD PROCESS'S RECORD IS FROM THIS PROCESS'S OWN ATERM SESSION: `theirs` (its
/// `atermSession`) is the session `launch` runs in. aterm's loop keeps a tab's pace in that
/// session's books (`short_streak_in`, `seed_retry` and `seed_acts` read the ledger by `sid`)
/// and takes it up at every start in that tab, however old and however the worker before
/// ended. So a record from the same session hands its pace on just as aterm's books do. A
/// record from another tab, or with no session (outside aterm, or an older build), hands it
/// on only where aterm's relaunch typed this process's line (CONTRACT §4 `pace`).
pub fn same_aterm_session(theirs: Option<&str>, launch: &Launch) -> bool {
    theirs.is_some_and(|theirs| launch.aterm_session() == Some(theirs))
}

/// Why a box blocks the displayed thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockedKind {
    Permission,
    Question,
    Auth,
}

/// The displayed thread's program state, as OSC 7501 reports it. `Unknown` until a frame
/// says otherwise; a reader treats it as busy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProgramState {
    #[default]
    Unknown,
    Idle,
    Working,
    Blocked(BlockedKind),
    Done,
    Error,
}

/// One frame's facts, borrowed from the TUI (the `observe` hook builds it).
#[derive(Clone, Copy, Debug)]
pub struct Frame<'a> {
    pub primary_thread: Option<&'a str>,
    pub displayed_thread: Option<&'a str>,
    /// The displayed thread's rollout file (it may not exist yet).
    pub rollout_path: Option<&'a Path>,
    pub cwd: &'a Path,
    pub model: &'a str,
    pub codex_home: &'a Path,
    pub embedded: bool,
    pub state: ProgramState,
    pub draft_empty: bool,
    /// Queued, pending-steer or rejected-steer input the TUI holds.
    pub queued_input: bool,
    /// A modal or popup of the bottom pane is open.
    pub popup_open: bool,
    /// The external editor (Ctrl+G) was asked for or runs: the TUI draws no frame while a
    /// person writes in it, so this frame's reading stands until it closes.
    pub editor_open: bool,
    /// A user turn was submitted and has not started.
    pub pending_user_turn: bool,
}

/// What the autopilot will know of a turn this process ran and saw end (filled later).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LastTurn {
    pub turn_id: Option<String>,
    pub ended_at_ms: u64,
    pub last_agent_message: Option<String>,
    pub by_autopilot: bool,
}

/// A thread the TUI registered (primary or displayed at some frame).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThreadFacts {
    pub rollout_path: Option<PathBuf>,
    pub last_turn: Option<LastTurn>,
}

/// The latest frame's facts, kept under one lock.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub primary_thread: Option<String>,
    pub displayed_thread: Option<String>,
    pub cwd: Option<PathBuf>,
    pub model: Option<String>,
    pub codex_home: Option<PathBuf>,
    pub embedded: bool,
    pub state: ProgramState,
    /// When `state` last changed (ms since the epoch).
    pub state_since_ms: u64,
    pub threads: BTreeMap<String, ThreadFacts>,
}

impl View {
    /// The primary thread's facts, if it is known.
    pub fn primary(&self) -> Option<&ThreadFacts> {
        self.threads.get(self.primary_thread.as_deref()?)
    }
}

/// codex-alab's autopilot, as the record reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Autopilot {
    #[default]
    Off,
    Passive,
    Active,
}

/// The Autopilot word aterm's harness runs this session under, as the supervisor last knew
/// it, for the TUI's chrome and the turn's end's exchange (`decide::ThreadState::ended_under`,
/// gap 32; the record carries [`Autopilot`]): with the claim held,
/// `Cruise` or `Land`; `Off` is the whole harness on with Autopilot off (`harness policy`
/// `why=autopilot-off`, `status autopilot=off`, or that status under a person's hold);
/// `HarnessOff` and `AgentOff` are the harness itself off by one of aterm's Settings switches,
/// and `Unhosted` aterm with no harness host, which aterm's chip wears as `⊘ off` (its
/// `MasterOff`, `AgentOff` and `Unhosted`; amended 2026-10-29 after the review of gap 12,
/// finding 3: a codex tab showed no Autopilot face there); `Unknown` is any other passive
/// reason, or nothing read yet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum HarnessWord {
    #[default]
    Unknown,
    Cruise,
    Land,
    Off,
    /// The harness off in aterm's Settings for every session (`why=disabled`).
    HarnessOff,
    /// aterm's switch for codex-alab sessions off, the harness on (`why=agent-off`).
    AgentOff,
    /// aterm runs no harness host (`why=unhosted`).
    Unhosted,
}

/// THE TASK'S FACE ON CODEX-ALAB'S LINE (aterm's `TaskFace`, `claude_lights.rs`; gap 12 of the
/// parity sweep, 2026-10-29): a landing (`✓ landed`), a point left to a person (`⤓ quiet`), or
/// neither, as the thread's decider holds it ([`Shared::set_task_face`]). The TUI reads it under
/// the Land word only, as aterm's chip does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TaskFace {
    #[default]
    Open,
    Landed,
    Theirs,
}

/// The [`Autopilot`] in the low bits of [`Shared`]'s one Autopilot cell, the [`HarnessWord`]
/// above them: the supervisor publishes the two in one store and the TUI reads them in one
/// load ([`Shared::harness`]), so a frame never sees one half of a change.
const AUTOPILOT_BITS: u8 = 0b11;
const WORD_SHIFT: u32 = 2;

impl Autopilot {
    fn bits(self) -> u8 {
        match self {
            Autopilot::Off => 0,
            Autopilot::Passive => 1,
            Autopilot::Active => 2,
        }
    }

    fn from_cell(cell: u8) -> Self {
        match cell & AUTOPILOT_BITS {
            1 => Autopilot::Passive,
            2 => Autopilot::Active,
            _ => Autopilot::Off,
        }
    }
}

impl HarnessWord {
    fn bits(self) -> u8 {
        (self as u8) << WORD_SHIFT
    }

    fn from_cell(cell: u8) -> Self {
        match cell >> WORD_SHIFT {
            1 => HarnessWord::Cruise,
            2 => HarnessWord::Land,
            3 => HarnessWord::Off,
            4 => HarnessWord::HarnessOff,
            5 => HarnessWord::AgentOff,
            6 => HarnessWord::Unhosted,
            _ => HarnessWord::Unknown,
        }
    }
}

/// An unresolved provider failure, kept across a witnessed crash or live re-exec.
/// Deadlines use epoch milliseconds so restarting a process never restarts a wait.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recovery {
    pub thread: String,
    /// The exact failed turn; a newer history point makes this intent obsolete.
    pub turn_id: String,
    pub point_at_ms: u64,
    pub not_before_ms: u64,
    pub by_autopilot: bool,
    pub by_person: bool,
    pub wall: RecoveryWall,
}

/// A provider retry still inside a native turn. This is only a pacing fact: it cannot
/// authorize a continuation, waive a manual stop, or extend the crash carry-on chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InFlightRetry {
    pub thread: String,
    pub turn_id: String,
    pub not_before_ms: u64,
}

/// At most one crash carry-on without a person's turn or real work between.
pub const MAX_CARRY_ONS: u32 = 1;

/// Only failures the autopilot can re-decide under the current policy are carried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryWall {
    Network {
        what: String,
        /// THE ROUTE WAS MEASURED (B2): the process that met the wall measured its route (a
        /// direct one, its own failure no TLS or proxy refusal), so the process after it reads
        /// the route again and measures it while the wait stands: a measured return ends the
        /// hold a measured outage decided, where read blind the wait ran out the hold.
        measured: bool,
    },
    Overlong {
        what: String,
    },
    Entitlement {
        quota: Option<crate::quota::Scope>,
    },
    Server {
        what: String,
        cut_off: bool,
        retry_at_ms: Option<u64>,
    },
    Usage {
        reset_at_ms: Option<u64>,
        reset_label: Option<String>,
        quota: Option<crate::quota::Scope>,
    },
}

/// What the previous image of this pid left in its record (a re-exec), read once at start.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inherited {
    pub exec_generation: u64,
    pub status: Option<String>,
    pub session_id: Option<String>,
    /// The autopilot's act the previous image owed, or its landing ([`OwedAct`]).
    pub owed_act: Option<OwedAct>,
    /// An unresolved provider failure; it is re-decided, never replayed as prompt text.
    pub recovery: Option<Recovery>,
    pub in_flight_retry: Option<InFlightRetry>,
    /// The key of the NEEDS YOU set an Off took off the person's screen in the previous
    /// image, which no report raised again before its re-exec ([`Shared::needs_off`]; `0`:
    /// none).
    pub needs_off: u64,
    /// The folder its primary thread's worker last ran a command in ([`Shared::workdir`]).
    pub workdir: Option<PathBuf>,
}

/// WHAT THE REPOSITORY PROBE READS FOR A THREAD (CONTRACT §4 `probeScope`, added 2026-11-05
/// after the review of the forever probe): the folders its worker touched (each folder a
/// command ran in, `cd`ed into or pointed git at, and each folder an edit landed in, newest last,
/// the probe's repositories once it compacts them), whether it lost one it could not keep (more
/// than [`PROBE_SCOPE_DIRS`] at once, or one a record could not name), when its session began
/// (the thread's first turn, Unix seconds), and the branches its commands created or checked out
/// with the folder each was named in (newest first). The record carries the primary thread's, so
/// the image after a re-exec, or a relaunch after a crash, reads what the worker touched before
/// it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProbeScope {
    pub dirs: Vec<PathBuf>,
    pub overflow: bool,
    pub since: Option<u64>,
    pub branches: Vec<UsedBranch>,
}

/// A BRANCH A COMMAND OF THE SESSION CREATED OR CHECKED OUT ([`ProbeScope::branches`]): its name
/// and the folder the git command that named it ran in (its `cd`s and `-C`s taken), whose
/// repository alone the probe reads it in, so a name the session used in one repository never
/// reads another repository's branch of that name (review of the forever probe at f48a215d7b,
/// R3). `None` where that folder could not be placed (a `cd $VAR`): read in every repository the
/// probe reads, as a branch whose folder is gone is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsedBranch {
    pub name: String,
    pub dir: Option<PathBuf>,
}

impl UsedBranch {
    /// Branch `name`, named in folder `dir`.
    #[must_use]
    pub fn new(name: &str, dir: Option<&Path>) -> Self {
        Self {
            name: name.to_owned(),
            dir: dir.map(Path::to_path_buf),
        }
    }
}

/// `branch` taken into `branches` (newest first): it moves to the front, once, and only the
/// newest `max` are kept. Whether anything moved.
pub fn note_branch(branches: &mut Vec<UsedBranch>, branch: UsedBranch, max: usize) -> bool {
    if branches.first() == Some(&branch) {
        return false;
    }
    branches.retain(|kept| *kept != branch);
    branches.insert(0, branch);
    branches.truncate(max);
    true
}

/// The most folders a [`ProbeScope`] keeps: a new one past them sets its `overflow`. The scope is
/// compacted to its repositories long before ([`PROBE_SCOPE_COMPACT_AT`]), so only a session that
/// touched this many repositories, or this many folders faster than a compaction runs, reaches
/// it (review of the forever probe at f48a215d7b, R1: 128 folders of one clean repository
/// between two probes overflowed the scope, and Land could never land again).
pub const PROBE_SCOPE_DIRS: usize = 512;

/// The folders past which the autopilot compacts a scope to its repositories at once, without
/// waiting for a probe (R1).
pub const PROBE_SCOPE_COMPACT_AT: usize = 64;

/// The most branches a [`ProbeScope`] keeps: the newest.
pub const PROBE_SCOPE_BRANCHES: usize = 64;

impl ProbeScope {
    /// Folder `dir` touched: it moves to the newest end, once; a new one past
    /// [`PROBE_SCOPE_DIRS`] sets `overflow` instead. Whether the scope moved.
    pub fn touch(&mut self, dir: &Path) -> bool {
        if self.dirs.last().map(PathBuf::as_path) == Some(dir) {
            return false;
        }
        if let Some(at) = self.dirs.iter().position(|d| d == dir) {
            let kept = self.dirs.remove(at);
            self.dirs.push(kept);
            return true;
        }
        if self.dirs.len() >= PROBE_SCOPE_DIRS {
            let moved = !self.overflow;
            self.overflow = true;
            return moved;
        }
        self.dirs.push(dir.to_path_buf());
        true
    }

    /// Branch `branch` named by a command ([`note_branch`], at most [`PROBE_SCOPE_BRANCHES`]).
    /// Whether the scope moved.
    pub fn name_branch(&mut self, branch: UsedBranch) -> bool {
        note_branch(&mut self.branches, branch, PROBE_SCOPE_BRANCHES)
    }

    /// `other`'s folders, branches and start taken in beside this one's (the earlier start,
    /// every folder and branch of both, an overflow of either): a scope carried across a
    /// re-exec or a relaunch, met with the one this image began. Whether the scope moved.
    pub fn merge(&mut self, other: &Self) -> bool {
        let before = self.clone();
        for dir in other.dirs.iter().rev() {
            if !self.dirs.contains(dir) {
                if self.dirs.len() >= PROBE_SCOPE_DIRS {
                    self.overflow = true;
                } else {
                    self.dirs.insert(0, dir.clone());
                }
            }
        }
        self.overflow |= other.overflow;
        self.since = match (self.since, other.since) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        for branch in &other.branches {
            if !self.branches.contains(branch) && self.branches.len() < PROBE_SCOPE_BRANCHES {
                self.branches.push(branch.clone());
            }
        }
        *self != before
    }
}

/// The native attempt and lifecycle boundary an owed decision was made after. These facts
/// bind an existing handoff; they never authorize an action by themselves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeOrigin {
    pub turn_id: String,
    pub phase: NativePhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativePhase {
    Running,
    Complete,
    Interrupted,
    Aborted,
}

/// THE ACT THE AUTOPILOT OWED AT THE LIVE UPGRADE (FOREVER CRUISE / FOREVER LAND, aterm owner
/// direction 2026-10-19): under Cruise a paced act is almost always pending, so it no longer
/// holds the re-exec; the image that leaves writes the act it owed (and a Land landing) into
/// its record ([`Shared::set_owed_act`]), and the new image types it at its time
/// ([`Inherited::owed_act`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OwedAct {
    /// The root thread it was owed on.
    pub thread: String,
    /// The exact native point this decision observed. Absent in legacy records.
    pub native_origin: Option<NativeOrigin>,
    /// Account recovery intent. It is inert until the native host proves its
    /// committed receipt and the new AuthManager proves the selected identity.
    pub renewal: Option<AccountHandoff>,
    /// The rule it would have been typed under (`None`: no act owed, only `landed`).
    pub rule: Option<String>,
    /// The words the old image decided, exactly (the act is typed as decided, never derived
    /// again from the resumed reply alone: review of 2a9241bbf, findings 1, 8 and 16).
    pub text: Option<String>,
    /// The Autopilot word it was decided under (`cruise`, `land`).
    pub mode: Option<String>,
    /// Not before then (ms since the epoch; 0: at once).
    pub not_before_ms: u64,
    /// THE REPOSITORY'S SUFFIX OWED: a done claim's act decided before the repository was read
    /// (Cruise's defend prompt, Land's nudge) carries the open work the claim named, and the
    /// new image reads the repository once the act is due and fits the words again with what
    /// it holds, as the old image would have. `None`: the words are typed as decided.
    pub repo_owed: Option<Vec<String>>,
    /// THE USAGE WINDOW'S RESUME: the act resumes a usage-limited goal first, else types the
    /// words (a `usage-resume@v1` act under this flag; without it, a quota's try typed as a
    /// turn).
    pub resume_goal: bool,
    /// A WALL'S TRY THAT DELIVERS A LAND ACT AGAIN (the act the wall cut was one): the episode
    /// had its instruction, and the try's yield is a land act's, whose clean defense lands.
    pub landing: bool,
    /// The thread had landed under Land: nothing is typed until someone else's turn.
    pub landed: bool,
}

pub use crate::account_handoff::AccountHandoff;

/// The signals aterm may relaunch a session after (CONTRACT §4): SIGHUP, the terminal gone
/// under aterm's own crash (its restore relaunches the session), and SIGTERM, aterm's stall or
/// memory remedy. Any other signal (a person's SIGINT) ends the session in order. POSIX fixes
/// both numbers on every Unix codex-alab runs on.
pub const RELAUNCH_SIGNALS: [i32; 2] = [1, 15];

/// Whether a record's `exit` leaves aterm a relaunch to type: none written (a panic, SIGKILL,
/// power loss) or `signal:<n>` for a signal of [`RELAUNCH_SIGNALS`]. A `fatal` exit and any
/// other signal are orderly ends.
pub fn exit_relaunchable(exit: Option<&str>) -> bool {
    exit.is_none_or(|exit| {
        RELAUNCH_SIGNALS
            .iter()
            .any(|signal| exit.strip_prefix("signal:") == Some(signal.to_string().as_str()))
    })
}

/// How long a dead process's record can still be a relaunch aterm owes: one written longer
/// before a start is a person's later resume, never aterm's relaunch (the carry-on's window),
/// and the start-up sweep removes it. A younger one is left for the process it names.
pub const RELAUNCH_HORIZON: Duration = Duration::from_secs(24 * 3600);

/// THE PACE A THREAD'S AUTOPILOT KEEPS ACROSS A RE-EXEC OR A RELAUNCH, as aterm's loop keeps
/// it across its own restarts (`seed_short`: the short streak the last act was typed after;
/// `seed_retry`, `seed_usage_attempt`: the rung of a wall's ladder): the record carries the
/// primary thread's (`pace`), so the image after a live upgrade, or aterm's relaunch after a
/// crash, backs off as the process before would have. Started from nothing, a relaunch backed
/// a worker that yields at once off the first rung (2 min) where the process before was on
/// the fourth (16 min), and retried a wall at the ladder's first rung.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pace {
    /// Short turns of codex-alab's continuations in a row.
    pub short_streak: u32,
    /// The wall whose ladder the thread is on, where its class's ladder outlives a process.
    pub wall: Option<WallMark>,
    /// When each act `continue_per_hour` counts was typed within its window (ms since the
    /// epoch; aterm's `seed_acts`): the next process spends the hour's budget where this one
    /// left it.
    pub typed_ms: Vec<u64>,
    /// THE ACT A WALL CUT (aterm's `cut_text`): the defend prompt's or the land act's words a
    /// wall's tries deliver again, which a full context's compaction then types again
    /// verbatim; carried so the process after a crash or a re-exec delivers the same act
    /// (B2: a relaunch's compaction typed the plain continuation in its place).
    pub cut: Option<CutAct>,
    /// THE DONE CHECKS THIS CRUISE RUN TYPED (aterm's `seed_checks`): the process after asks
    /// the angle after them, not the first again.
    pub checks: u8,
    /// RULING F'S REPLIES, the oldest first (aterm's `Carried::said`, its ledger note's
    /// `<mark>:<answers>[w<tries>]`): the image after a re-exec holds the same reply as this
    /// one would have. aterm's relaunch takes none up (its `launched_since_books`).
    pub replies: Vec<KeptReply>,
    /// The no-work answers in a row (aterm's note's `<run> in a row`).
    pub replies_run: u32,
    /// When the newest answer counted came (ms since the epoch; aterm's `seed_replies` `ago`).
    pub replies_at_ms: Option<u64>,
    /// THE RESET THE NEWEST ANSWER COUNTED SHOWED (ms since the epoch; aterm's `said_reset_ms`
    /// and `seed_replies_reset`): past it, a wall's replies may be answered again and the run
    /// starts afresh in the image after a re-exec as in this one.
    pub replies_reset_ms: Option<u64>,
    /// THE POINT RULING F HOLDS (its reply's words): the image after a re-exec stands at it
    /// again, so an Off and back types there as it would have in this one. A held point owes
    /// no act, so the record's owed act names none (parity review of the stage-1 port,
    /// finding 1).
    pub replies_point: Option<ReplyPoint>,
    /// LAND'S HOLD STANDING (aterm's `HoldStart`, which its loop seeds from the session's
    /// books at every start, `seed_land_hold`): when the act that began it was typed and the
    /// nudges typed since, so the image after a re-exec, or aterm's relaunch after a crash,
    /// says the same landing (`since`, `nudges`) the process before said (review of gap 11,
    /// finding 1: the new image's first claim took the hold down, and the next land act began
    /// it anew, its `since` and nudges started over).
    pub land_hold: Option<LandHold>,
    /// THE PERSON'S HOUR STANDING (aterm's `go-ahead-hour@v1`, which its loop books where the
    /// hour runs from the end of the turn their key came in and seeds at every start,
    /// `take_go_ahead_hour`, `seed_go_ahead_hour`): when the hour the person's last key holds
    /// the owner's standing go-ahead for began (ms since the epoch), so the image after a
    /// re-exec, or aterm's relaunch after a crash, holds it for as long as this one would have.
    /// The window keeps the key across both, never the end of a long turn the key came in
    /// (review of the go-ahead port, findings 3 and 9: a re-exec timed that hour from the key,
    /// as much too early as the turn was long).
    pub go_ahead_hour_ms: Option<u64>,
    /// THE GO-AHEAD'S COUNT ON THE STANDING PARK (aterm's `GoAheads`): the image after a re-exec
    /// bounds the same park as this one would have (review of the go-ahead port, finding 3: a
    /// re-exec at a held park let one more go-ahead through the same reply's hold). aterm's
    /// relaunch takes none up, as it takes no reply up.
    pub go_ahead: Option<GoAheadCount>,
    /// THE POINT LEFT TO THE PERSON (aterm's LEFT record, which its next loop applies again): the
    /// image after a re-exec stands at it again, still theirs under Land, so a later switch to
    /// Cruise takes it back and continues there as this one would have (edges-5: the live
    /// upgrade moved a left session with no notice, and the new image stood at no point).
    pub left_point: Option<LeftPoint>,
    /// A PERSON'S ESC STANDING OVER THE POINT (the autopilot's `esc_at`, aterm's `person_hold`):
    /// when a person's Esc stopped the point's turn, and that turn, so the image after a re-exec,
    /// or aterm's relaunch after a crash, holds the point for the same grace from the same Esc
    /// and then carries on, as this one would have (the forever-persist lane, item a: the Esc
    /// lived in memory only, so the image after stood at an interrupted point no person's Esc
    /// had made, parked for good, and a relaunch after a signal typed its carry-on at once over
    /// the person's Esc).
    pub esc: Option<EscHold>,
    /// THE GOAL A TURN ERROR BLOCKED AT THE WALL THE POINT STANDS AT (the autopilot's
    /// `wall_goal_after`): the wall's try resumes it in the image after a re-exec and in aterm's
    /// relaunch as in this one (item a: it lived in memory only, so the relaunch's try met the
    /// blocked goal as a person's stop and parked for good).
    pub wall_goal: Option<WallGoal>,
}

/// A PERSON'S ESC AS THE RECORD CARRIES IT ([`Pace::esc`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EscHold {
    /// When the Esc stopped the turn (ms since the epoch).
    pub at_ms: u64,
    /// The native turn it stopped: the image after takes the Esc up at that turn's abort alone
    /// (none is carried for a point whose turn this process could not name).
    pub turn: String,
}

/// THE GOAL A WALL'S TRY RESUMES, AS THE RECORD CARRIES IT ([`Pace::wall_goal`]): its identity
/// as the wall left it (the autopilot's `same_wall_goal` reads it against the store), the native
/// turn whose turn error blocked it, and how many of a person's stops of the thread's goal this
/// process had seen when it was bound ([`Shared::person_goal_stops`]), so a person's stop since
/// makes it no wall's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WallGoal {
    pub goal_id: String,
    pub objective: String,
    pub token_budget: Option<i64>,
    /// The native turn whose turn error blocked it: the image after a re-exec or a relaunch reads
    /// the thread's history from that turn's start for a person's goal set, which upstream
    /// records there, and binds no goal a person stopped (review of the forever-persist lane,
    /// F2: a stop the record writer had not yet written when the process died).
    pub turn: String,
    /// [`Shared::person_goal_stops`] when it was bound: this process's count. The record carries
    /// none, so a carried goal reads as bound at none, where the image after begins its count, and
    /// a person's stop in either image takes the goal out of the pace ([`Shared::set_pace`],
    /// [`Shared::person_goal_set`]).
    pub person_stops: u64,
}

/// THE POINT LEFT TO THE PERSON AS THE RECORD CARRIES IT ([`Pace::left_point`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LeftPoint {
    /// The point's words.
    pub point: ReplyPoint,
    /// Its exchange opened the session: Land keeps only such an exchange theirs.
    pub opens: bool,
    /// The exchange did tool work, including its wall retries: Cruise carries it after re-exec.
    pub ran_tools: bool,
    /// A question box occurred anywhere in the exchange; `point.questions` is the latest turn.
    pub had_questions: bool,
}

/// THE GO-AHEAD'S COUNT AS THE RECORD CARRIES IT ([`Pace::go_ahead`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GoAheadCount {
    /// The go-aheads typed on the standing park.
    pub acts: u32,
    /// When the first of them went since the agent last moved past its park (ms since the
    /// epoch).
    pub first_ms: Option<u64>,
}

/// LAND'S HOLD AS THE RECORD CARRIES IT ([`Pace::land_hold`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LandHold {
    /// When the act that began it was typed (ms since the epoch).
    pub since_ms: u64,
    /// The nudges typed since that act.
    pub nudges: u32,
}

/// THE WORDS OF THE POINT RULING F HOLDS ([`Pace::replies_point`]): its last and earlier words
/// and whether it asked, which its reply's mark is read from again.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplyPoint {
    /// The turn's last words.
    pub text: Option<String>,
    /// The turn's earlier words.
    pub earlier: Option<String>,
    /// The turn asked the person questions.
    pub questions: bool,
}

/// One reply ruling F keeps, as the record carries it ([`Pace::replies`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeptReply {
    /// Its mark (FNV-1a/64 of its words, as the autopilot reads them).
    pub mark: u64,
    /// The no-work answers that settled to it.
    pub answers: u32,
    /// A wall said it.
    pub walled: bool,
    /// The answers to the wall's own tries.
    pub tries: u32,
}

impl Pace {
    /// Nothing to carry: no short streak, no ladder, no act within the hour's budget, no act
    /// a wall cut, no reply kept, no Land hold standing, no person's hour, no go-ahead counted,
    /// no point left to the person, no person's Esc standing and no goal a wall's try resumes.
    pub fn at_rest(&self) -> bool {
        self.short_streak == 0
            && self.wall.is_none()
            && self.typed_ms.is_empty()
            && self.cut.is_none()
            && self.checks == 0
            && self.replies.is_empty()
            && self.replies_run == 0
            && self.replies_at_ms.is_none()
            && self.replies_reset_ms.is_none()
            && self.replies_point.is_none()
            && self.land_hold.is_none()
            && self.go_ahead_hour_ms.is_none()
            && self.go_ahead.is_none()
            && self.left_point.is_none()
            && self.esc.is_none()
            && self.wall_goal.is_none()
    }
}

/// The act a wall cut, as the record carries it ([`Pace::cut`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CutAct {
    /// Its words, exactly as typed.
    pub text: String,
    /// The Autopilot word it was typed under (`cruise`, `land`): read under another, it is
    /// none.
    pub mode: String,
}

/// A wall's ladder as the record carries it ([`Pace::wall`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WallMark {
    /// The wall's class word (`server`, `network`, `usage`, `entitlement`).
    pub class: String,
    /// How often codex-alab tried past it.
    pub attempts: u32,
    /// When it first stood (ms since the epoch).
    pub first_seen_ms: u64,
    /// When codex-alab last tried past it (ms since the epoch).
    pub last_try_ms: Option<u64>,
    /// The appearance the ladder counts from (ms since the epoch; aterm's `since`): the first,
    /// or the one that answered the last try. `None` from a build that carried none: counted
    /// from the last try then.
    pub since_ms: Option<u64>,
    /// The last try still owes its answer (the process died in its turn): the first turn the
    /// next process sees end answers it (aterm's `seed_retry` with `owed`).
    pub answer_owed: bool,
}

/// A dead process's record that this process's start swept away: the crash witness of a
/// session aterm may have relaunched as this process (the autopilot's carry-on reads it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Predecessor {
    pub pid: u32,
    pub session_id: String,
    /// The record's `exit` (`signal:<n>`, `fatal`), `None` when it was never ended (a
    /// panic, SIGKILL or power loss).
    pub exit: Option<String>,
    /// The record's `resumeArgv`: the very line aterm's crash relaunch types.
    pub resume_argv: Option<Vec<String>>,
    /// The record's `status` word (`busy`, `idle`, `waiting`, ...).
    pub status: Option<String>,
    /// The record's `updatedAt` (ms since the epoch).
    pub updated_at_ms: u64,
    /// The record's `carriedOn`: how many relaunch carry-ons in a row led to it with no
    /// person's turn and no real work since ([`Shared::set_carried_on`]); 0 for a record that
    /// has none.
    pub carried_on: u32,
    /// The record's `needsOff`: the key of the NEEDS YOU set an Off took off the person's
    /// screen in the dead process, which no report raised again as its record was last
    /// written ([`Shared::needs_off`]); 0 for a record that has none.
    pub needs_off: u64,
    /// The record's `workdir`: the folder its primary thread's worker last ran a command in.
    pub workdir: Option<PathBuf>,
    /// The record's `owed`: the paced act the dead process owed as its record was last
    /// written ([`Shared::set_owed_act`]: kept while the act waits, so a crash at rest leaves
    /// it to aterm's relaunch), `None` for a record that has none.
    pub owed_act: Option<OwedAct>,
    /// The unresolved provider failure the dead process was waiting to recover from.
    pub recovery: Option<Recovery>,
    pub in_flight_retry: Option<InFlightRetry>,
    /// The record's `pace`: its primary thread's short streak, wall ladder, counted acts and
    /// cut act. The start takes it up only where [`same_aterm_session`] holds or aterm's
    /// relaunch typed this process's line.
    pub pace: Option<Pace>,
    /// The record's `atermSession`: the aterm session the dead process ran in.
    pub aterm_session: Option<String>,
}

/// When a person last gave aterm's session input, as one `status` read saw it (`human_ms=`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HumanSeen {
    /// `-`: no person has, since the session started (or since an update carried it).
    Never,
    /// At this time (ms since the epoch: the read's time less `human_ms`).
    At(u64),
    /// Missing or unreadable: it cannot be told.
    Unknown,
}

/// What aterm's `@self status` said at this process's start, before anything of codex-alab's
/// acted (the aterm client's first read, CONTRACT §4): the proof of aterm's crash relaunch.
/// aterm types that relaunch line under its harness's hard lease on the tab (`hand=lease:
/// aterm-harness@<pid>`), held from before the line to the new process's first idle; a
/// person's own `codex resume <id>` comes with no such hand and with the person's keystroke
/// just before it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchWitness {
    /// `hand=` decoded, `None` for `-`.
    pub hand: Option<String>,
    /// When a person last gave input.
    pub human: HumanSeen,
    /// When the read was made (ms since the epoch).
    pub read_at_ms: u64,
    /// The pid of the process that answered the read: the control socket's kernel peer
    /// (`LOCAL_PEEREPID`, `SO_PEERCRED`), `None` when it cannot be told. aterm's harness
    /// names its hand after the aterm process that serves the socket, so only a hand naming
    /// this pid is aterm's own; any client of the socket can take a lease under any name.
    pub server_pid: Option<u32>,
    /// `hand_pid=`: the kernel's word for who took the lease the hand names.
    pub hand_pid: HandPid,
}

/// How long before launch a person's input can have typed the process's command line.
pub const PERSON_LINE_MARGIN: Duration = Duration::from_secs(5);

/// The last launch-probe read that can still attest to the process's start.
pub const LAUNCH_PROBE_WINDOW: Duration = Duration::from_secs(60);

impl LaunchWitness {
    /// Whether the first status read proves aterm's own crash relaunch, rather than a
    /// person's resume: its kernel-attested harness lease, a quiet person and a timely read.
    pub fn proves_relaunch(&self, started_at_ms: u64) -> bool {
        const ATERM_HAND: &str = "lease:aterm-harness@";
        let margin = u64::try_from(PERSON_LINE_MARGIN.as_millis()).unwrap_or(u64::MAX);
        let window = u64::try_from(LAUNCH_PROBE_WINDOW.as_millis()).unwrap_or(u64::MAX);
        let aterms_hand = match (self.hand.as_deref(), self.server_pid) {
            (Some(hand), Some(server)) => {
                hand.strip_prefix(ATERM_HAND) == Some(server.to_string().as_str())
                    && match self.hand_pid {
                        HandPid::Absent => true,
                        HandPid::Pid(taker) => taker == server,
                        HandPid::Unknown => false,
                    }
            }
            _ => false,
        };
        let person_quiet = match self.human {
            HumanSeen::Never => true,
            HumanSeen::At(at) => at.saturating_add(margin) <= started_at_ms,
            HumanSeen::Unknown => false,
        };
        aterms_hand
            && person_quiet
            && self.read_at_ms >= started_at_ms
            && self.read_at_ms - started_at_ms <= window
    }
}

/// What `status hand_pid=` said: the kernel-attested pid of the process that took the drive
/// lease `hand=` names (aterm reads it off the taking connection's peer at the acquire,
/// never off the holder's text). An aterm from before the token prints none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandPid {
    /// No `hand_pid=` token: an aterm that does not report it.
    Absent,
    /// `hand_pid=-` or an unreadable value: no live lease, or a taker it cannot name.
    Unknown,
    /// The taker's pid.
    Pid(u32),
}

impl HandPid {
    /// The token's raw value as `status` prints it (`None` when absent).
    pub fn parse(raw: Option<&str>) -> HandPid {
        match raw {
            None => HandPid::Absent,
            Some(raw) => (!raw.is_empty()
                && !raw.starts_with('0')
                && raw.bytes().all(|b| b.is_ascii_digit()))
            .then(|| raw.parse::<u32>().ok())
            .flatten()
            .filter(|pid| *pid > 0)
            .map_or(HandPid::Unknown, HandPid::Pid),
        }
    }
}

/// Releases codex-alab's claim on its aterm session within the given time (the exit path
/// calls it once the record is ended); `true` when nothing is held any more.
pub type ReleaseHook = Box<dyn Fn(Duration) -> bool + Send + Sync>;

/// Terminal context aterm gave for one user turn (`/ide` inside aterm): the text to put ahead
/// of the person's request, and a few words of what it holds, for the info line that shows
/// the person what went to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalContext {
    /// The context, rendered for the prompt (empty: aterm had nothing to give).
    pub prompt: String,
    /// What it holds, e.g. "selection 14 lines · 3 commands".
    pub summary: String,
}

/// Reads this session's terminal context from aterm within the given time (installed by the
/// supervisor); `None` when it cannot (no proven claim, no answer in time).
pub type ContextReader = Box<dyn Fn(Duration) -> Option<TerminalContext> + Send + Sync>;

/// Reads one message of this session's aterm inbox for a typed trigger, within the given time
/// (installed by the supervisor): the row at the offset, which must be of the kind the line
/// names. An error says why nothing was read (no proven claim, no such row, no answer).
pub type MailReader = Box<
    dyn Fn(crate::mail::TriggerKind, u64, Duration) -> Result<crate::mail::InboxMail, String>
        + Send
        + Sync,
>;

/// The session record's write-through (installed by its writer): write the record now, not
/// coalesced; `true` once what the record says is on disk (or when no record is kept).
pub type FlushHook = Box<dyn Fn() -> bool + Send + Sync>;

/// An act whose turn has not started yet ([`Shared::await_turn`]).
#[derive(Clone, Debug)]
struct AwaitedTurn {
    token: u64,
    thread: String,
    deadline_ms: u64,
}

/// The background terminals the displayed chat widget's footer counts
/// ([`Shared::set_background_terminals`]).
#[derive(Clone, Copy, Debug)]
struct TerminalCount {
    /// The count the last frame handed over; `None` before any did.
    count: Option<u32>,
    /// When the count last moved, until that move's hold is ended
    /// ([`Shared::expire_background_terminals_hold`]).
    moved: Option<Instant>,
}

/// How many ended turns are remembered, so a late start of one is not counted as running.
const ENDED_TURNS_KEPT: usize = 64;

/// The turns that run now, by `(thread, turn)` (the autopilot's lifecycle hooks keep it).
#[derive(Default)]
struct TurnBook {
    running: std::collections::BTreeSet<(String, String)>,
    latest: BTreeMap<String, String>,
    stopped_threads: std::collections::BTreeSet<String>,
    ended: std::collections::VecDeque<(String, String)>,
}

/// An act codex-alab is carrying out ([`Shared::begin_act`]): while one stands the record
/// says `busy`, never `idle`. Dropped when the act is done (its turn started, or it failed).
#[must_use = "the act stands only while its guard lives"]
pub struct ActGuard<'a> {
    shared: &'a Shared,
}

impl Drop for ActGuard<'_> {
    fn drop(&mut self) {
        self.shared.acts.fetch_sub(1, Ordering::SeqCst);
        self.shared.mark_changed();
    }
}

/// Why the session is not at rest although its frame may read idle ([`Shared::unrest`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unrest {
    /// A turn runs (the lifecycle hooks saw it start and not yet end).
    TurnRunning,
    /// codex-alab is carrying out an act ([`Shared::begin_act`]).
    Acting,
    /// A posted box answer the TUI has not applied yet.
    BoxAnswer,
    /// A resumed thread owes its carry-on ([`Shared::set_carry_on_owed`]).
    CarryOnOwed,
    /// A user turn was submitted and has not started.
    UserTurnPending,
    /// The TUI holds queued, pending-steer or rejected-steer input.
    QueuedInput,
    /// The live upgrade is under way.
    Upgrading,
}

/// The shared state. [`shared`] is the process's one copy; tests build their own.
pub struct Shared {
    quota: crate::quota::Quota,
    launch: OnceLock<Launch>,
    inherited: OnceLock<Inherited>,
    predecessors: Mutex<Vec<Predecessor>>,
    witness: Mutex<Option<Option<LaunchWitness>>>,
    view: Mutex<View>,
    human_input_ms: AtomicU64,
    draft_empty: AtomicBool,
    queued_input: AtomicBool,
    popup_open: AtomicBool,
    editor_open: AtomicBool,
    overlay_open: AtomicBool,
    pending_user_turn: AtomicBool,
    swept: AtomicBool,
    roots: Mutex<Vec<String>>,
    release: OnceLock<ReleaseHook>,
    context: OnceLock<ContextReader>,
    mail: OnceLock<MailReader>,
    mail_turns: Mutex<Vec<crate::mail::MailTurn>>,
    mail_badge: Mutex<Option<crate::mail::MailBadge>>,
    signal: AtomicI32,
    exec_pending: AtomicBool,
    upgrade_execution: Mutex<Option<UpgradeExecution>>,
    /// The [`Autopilot`] and the [`HarnessWord`] together ([`AUTOPILOT_BITS`]).
    autopilot: AtomicU8,
    /// aterm's `status land_at=` at the supervisor's last read of the word: unix seconds, 0
    /// for none ([`Self::set_land_at`]).
    land_at: AtomicU64,
    /// Each thread's [`TaskFace`] but [`TaskFace::Open`], which leaves none
    /// ([`Self::set_task_face`]).
    task_faces: Mutex<BTreeMap<String, TaskFace>>,
    claim: Mutex<Option<String>>,
    boxes: Mutex<BoxSlot>,
    waker: OnceLock<Callback>,
    leave: OnceLock<Callback>,
    upgrade_leave: OnceLock<UpgradeLeave>,
    /// Native facts and the upgrade's final commit share this lock. Record bookkeeping
    /// has its own generation and must not invalidate an otherwise unchanged handoff.
    upgrade_generation: Mutex<u64>,
    own_turns: Mutex<Vec<String>>,
    /// The TUI is about to start a thread that carries on another's work
    /// ([`Self::continue_work`]); the next root thread taken takes it.
    continues: Mutex<Option<String>>,
    // A prepared thread already exists; only its accepted plan prompt consumes this.
    prepared_continuation: Mutex<BTreeMap<String, (String, String)>>,
    preapprovals: Mutex<Vec<crate::boxes::Preapproval>>,
    resume_lines: AtomicU64,
    carry_on_owed: AtomicBool,
    carried_on: AtomicU32,
    owed_act: Mutex<Option<OwedAct>>,
    assignment: Mutex<Option<crate::assignment::Receipt>>,
    assignment_touched: AtomicBool,
    needs_off: AtomicU64,
    workdirs: Mutex<BTreeMap<String, PathBuf>>,
    /// Each thread's [`ProbeScope`] ([`Self::set_probe_scope`]).
    probe_scopes: Mutex<BTreeMap<String, ProbeScope>>,
    /// The [`ProbeScope`]s the records of the image before this one and of the dead processes
    /// the start swept carried, by session ([`Self::carry_probe_scope`]).
    carried_scopes: Mutex<BTreeMap<String, ProbeScope>>,
    goals: Mutex<BTreeMap<String, String>>,
    /// The background terminals the displayed chat widget's footer counts, and when that count
    /// last moved ([`Self::set_background_terminals`]).
    background_terminals: Mutex<TerminalCount>,
    paces: Mutex<BTreeMap<String, Pace>>,
    /// A person's stops of each thread's goal this process saw ([`Self::person_goal_set`]):
    /// taken after `paces`, never before it.
    person_goal_stops: Mutex<BTreeMap<String, u64>>,
    recovery: Mutex<Option<Recovery>>,
    in_flight_retry: Mutex<Option<InFlightRetry>>,
    relaunch_reserved: AtomicBool,
    relaunch_spent: AtomicBool,
    acts: AtomicU64,
    awaited: Mutex<Vec<AwaitedTurn>>,
    next_await: AtomicU64,
    turns: Mutex<TurnBook>,
    /// THE TURNS THE TUI SAW RUN ([`Self::turn_seen`]), by `(thread, turn)`: every thread its
    /// app-server notifications are routed for (a sub-agent, a side thread, any thread under
    /// a remote app-server), booked for the OSC 7501 report alone
    /// ([`Self::turn_seen_besides`]: such a turn holds a report standing in a turn and begins
    /// none); the record, the acts and the upgrade read the autopilot's book ([`TurnBook`])
    /// only.
    seen: Mutex<std::collections::BTreeSet<(String, String)>>,
    /// THE TURN ENDS THE TUI'S CHAT WIDGET TOOK ([`Self::turn_handled_end`]), live or replayed,
    /// by `(thread, turn)`, the newest [`ENDED_TURNS_KEPT`]: what a landing waits on before it
    /// is recorded ([`Self::tui_handled_end`]).
    handled_ends: Mutex<std::collections::VecDeque<(String, String)>>,
    keyed_since_frame: AtomicBool,
    flush: OnceLock<FlushHook>,
    generation: Mutex<u64>,
    changed: Condvar,
}

/// How long a posted box answer may wait for the TUI's frame to apply it.
pub const BOX_ANSWER_TTL: Duration = Duration::from_secs(10);

/// A TUI action handed to the autopilot (wake a frame, leave like `/exit`).
pub type Callback = Box<dyn Fn() + Send + Sync>;

/// An upgrade has its own conditional event; an explicit user exit stays unconditional.
pub type UpgradeLeave = Box<dyn Fn(UpgradeRequest) + Send + Sync>;
pub use crate::upgrade_completion::UpgradeCompletion;
type UpgradeContext = Box<dyn std::any::Any + Send>;
type UpgradeDecision = Box<dyn FnOnce(bool, Option<UpgradeContext>) -> UpgradeDispatch + Send>;
/// Only Capturing carries a post-commit operation. Ready is the existing plain
/// leave protocol; neither outcome is native SDK or execution authority.
pub enum UpgradeDispatch {
    Refused,
    Ready,
    Capturing(UpgradeCompletion),
}
/// Exact admitted process execution, owned by its committing session until teardown.
pub type UpgradeExecution = Box<dyn FnOnce() -> String + Send + 'static>;

/// A queued TUI handoff. Dropping either end rejects a decision not yet dispatched.
/// Once dispatch takes the decision, the caller's cancellation invalidates its native
/// generation before restoring the prior act, fencing an in-flight commit as well.
pub struct UpgradeRequest {
    decision: std::sync::Arc<Mutex<Option<UpgradeDecision>>>,
    requires_context: bool,
}

pub struct UpgradeCancellation {
    decision: std::sync::Arc<Mutex<Option<UpgradeDecision>>>,
}

impl std::fmt::Debug for UpgradeRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpgradeRequest").finish_non_exhaustive()
    }
}

impl UpgradeRequest {
    pub fn new(decide: impl FnOnce(bool) -> bool + Send + 'static) -> (Self, UpgradeCancellation) {
        let decision: UpgradeDecision = Box::new(move |ready, _| {
            if decide(ready) {
                UpgradeDispatch::Ready
            } else {
                UpgradeDispatch::Refused
            }
        });
        let decision = std::sync::Arc::new(Mutex::new(Some(decision)));
        (
            Self {
                decision: std::sync::Arc::clone(&decision),
                requires_context: false,
            },
            UpgradeCancellation { decision },
        )
    }

    /// The final callback runs only with the exact native context supplied by
    /// the TUI. Missing/wrong context and cancellation drop preparation before
    /// any source effects. The context is a Rust owner, not decoded authority.
    pub fn with_context<T: std::any::Any + Send>(
        decide: impl FnOnce(T) -> Option<UpgradeCompletion> + Send + 'static,
    ) -> (Self, UpgradeCancellation) {
        let decision: UpgradeDecision = Box::new(move |ready, context| {
            if !ready {
                return UpgradeDispatch::Refused;
            }
            let Some(runtime) = UpgradeCompletion::runtime() else {
                return UpgradeDispatch::Refused;
            };
            let Some(context) = context.and_then(|context| context.downcast::<T>().ok()) else {
                return UpgradeDispatch::Refused;
            };
            match decide(*context) {
                Some(completion) => {
                    completion.start_on(runtime);
                    UpgradeDispatch::Capturing(completion)
                }
                None => UpgradeDispatch::Refused,
            }
        });
        let decision = std::sync::Arc::new(Mutex::new(Some(decision)));
        (
            Self {
                decision: decision.clone(),
                requires_context: true,
            },
            UpgradeCancellation { decision },
        )
    }
    pub fn requires_context(&self) -> bool {
        self.requires_context
    }
    pub fn decide_with_context<T: std::any::Any + Send>(
        self,
        native_ready: bool,
        context: T,
    ) -> UpgradeDispatch {
        let decide = { lock(&self.decision).take() };
        decide.map_or(UpgradeDispatch::Refused, |decide| {
            decide(native_ready, Some(Box::new(context)))
        })
    }

    /// Called synchronously at the TUI's actual leave boundary, with current native facts.
    pub fn decide(self, native_ready: bool) -> bool {
        let decide = { lock(&self.decision).take() };
        decide.is_some_and(|decide| matches!(decide(native_ready, None), UpgradeDispatch::Ready))
    }
}

impl Drop for UpgradeRequest {
    fn drop(&mut self) {
        let decide = { lock(&self.decision).take() };
        if let Some(decide) = decide {
            decide(false, None);
        }
    }
}

impl Drop for UpgradeCancellation {
    fn drop(&mut self) {
        let decide = { lock(&self.decision).take() };
        if let Some(decide) = decide {
            decide(false, None);
        }
    }
}

static SHARED: Shared = Shared::new();

/// This process's shared state.
pub fn shared() -> &'static Shared {
    &SHARED
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn replace_if_changed<T: PartialEq + ?Sized + ToOwned>(
    slot: &mut Option<T::Owned>,
    value: Option<&T>,
) -> bool
where
    T::Owned: std::borrow::Borrow<T>,
{
    use std::borrow::Borrow;
    if slot.as_ref().map(Borrow::borrow) == value {
        return false;
    }
    *slot = value.map(ToOwned::to_owned);
    true
}

impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}

impl Shared {
    pub fn quota(&self) -> &crate::quota::Quota {
        &self.quota
    }
    pub const fn new() -> Self {
        Self {
            launch: OnceLock::new(),
            quota: crate::quota::Quota::new(),
            inherited: OnceLock::new(),
            predecessors: Mutex::new(Vec::new()),
            witness: Mutex::new(None),
            view: Mutex::new(View {
                primary_thread: None,
                displayed_thread: None,
                cwd: None,
                model: None,
                codex_home: None,
                embedded: false,
                state: ProgramState::Unknown,
                state_since_ms: 0,
                threads: BTreeMap::new(),
            }),
            human_input_ms: AtomicU64::new(0),
            draft_empty: AtomicBool::new(true),
            queued_input: AtomicBool::new(false),
            popup_open: AtomicBool::new(false),
            editor_open: AtomicBool::new(false),
            overlay_open: AtomicBool::new(false),
            pending_user_turn: AtomicBool::new(false),
            swept: AtomicBool::new(false),
            roots: Mutex::new(Vec::new()),
            release: OnceLock::new(),
            context: OnceLock::new(),
            mail: OnceLock::new(),
            mail_turns: Mutex::new(Vec::new()),
            mail_badge: Mutex::new(None),
            signal: AtomicI32::new(0),
            exec_pending: AtomicBool::new(false),
            upgrade_execution: Mutex::new(None),
            autopilot: AtomicU8::new(0),
            land_at: AtomicU64::new(0),
            task_faces: Mutex::new(BTreeMap::new()),
            claim: Mutex::new(None),
            boxes: Mutex::new(BoxSlot {
                open: None,
                seen: false,
                pending: None,
                pending_ms: 0,
                outcome: None,
                view_at: None,
                views: 0,
                goal_stop: None,
            }),
            waker: OnceLock::new(),
            leave: OnceLock::new(),
            upgrade_leave: OnceLock::new(),
            upgrade_generation: Mutex::new(0),
            own_turns: Mutex::new(Vec::new()),
            continues: Mutex::new(None),
            prepared_continuation: Mutex::new(BTreeMap::new()),
            preapprovals: Mutex::new(Vec::new()),
            resume_lines: AtomicU64::new(0),
            carry_on_owed: AtomicBool::new(false),
            carried_on: AtomicU32::new(0),
            owed_act: Mutex::new(None),
            assignment: Mutex::new(None),
            assignment_touched: AtomicBool::new(false),
            needs_off: AtomicU64::new(0),
            workdirs: Mutex::new(BTreeMap::new()),
            probe_scopes: Mutex::new(BTreeMap::new()),
            carried_scopes: Mutex::new(BTreeMap::new()),
            goals: Mutex::new(BTreeMap::new()),
            background_terminals: Mutex::new(TerminalCount {
                count: None,
                moved: None,
            }),
            paces: Mutex::new(BTreeMap::new()),
            person_goal_stops: Mutex::new(BTreeMap::new()),
            recovery: Mutex::new(None),
            in_flight_retry: Mutex::new(None),
            relaunch_reserved: AtomicBool::new(false),
            relaunch_spent: AtomicBool::new(false),
            awaited: Mutex::new(Vec::new()),
            next_await: AtomicU64::new(0),
            acts: AtomicU64::new(0),
            turns: Mutex::new(TurnBook {
                running: std::collections::BTreeSet::new(),
                latest: BTreeMap::new(),
                stopped_threads: std::collections::BTreeSet::new(),
                ended: std::collections::VecDeque::new(),
            }),
            seen: Mutex::new(std::collections::BTreeSet::new()),
            handled_ends: Mutex::new(std::collections::VecDeque::new()),
            keyed_since_frame: AtomicBool::new(false),
            flush: OnceLock::new(),
            generation: Mutex::new(0),
            changed: Condvar::new(),
        }
    }

    /// Store the launch snapshot; only the first call counts.
    pub fn set_launch(&self, launch: Launch) -> bool {
        self.launch.set(launch).is_ok()
    }

    pub fn launch(&self) -> Option<&Launch> {
        self.launch.get()
    }

    /// Store what a previous image of this pid left; only the first call counts.
    pub fn set_inherited(&self, inherited: Inherited) -> bool {
        self.inherited.set(inherited).is_ok()
    }

    pub fn inherited(&self) -> Option<&Inherited> {
        self.inherited.get()
    }

    /// Note a dead predecessor's record the start-up sweep removed.
    pub fn note_predecessor(&self, predecessor: Predecessor) {
        lock(&self.predecessors).push(predecessor);
    }

    /// The start-up sweep of dead records is done (whatever it found).
    pub fn set_swept(&self) {
        self.swept.store(true, Ordering::SeqCst);
        self.mark_changed();
    }

    /// Whether the start-up sweep is done (until then no predecessor is known).
    pub fn swept(&self) -> bool {
        self.swept.load(Ordering::SeqCst)
    }

    /// Block until the start-up sweep is done, at most `timeout`; whether it is.
    pub fn wait_swept(&self, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut seen = self.generation();
        while !self.swept() {
            let now = std::time::Instant::now();
            if now >= deadline {
                return false;
            }
            seen = self.wait_change(seen, deadline - now);
        }
        true
    }

    /// The launch probe is done: `witness` is what aterm's first `status` read said, `None`
    /// when no read was made (no aterm reached, no answer). Only the first call counts.
    pub fn set_launch_witness(&self, witness: Option<LaunchWitness>) -> bool {
        let set = {
            let mut slot = lock(&self.witness);
            if slot.is_some() {
                false
            } else {
                *slot = Some(witness);
                true
            }
        };
        if set {
            self.mark_changed();
        }
        set
    }

    /// Whether the launch probe is done ([`Self::set_launch_witness`]).
    pub fn launch_probed(&self) -> bool {
        lock(&self.witness).is_some()
    }

    /// What the launch probe read, if it read anything.
    pub fn launch_witness(&self) -> Option<LaunchWitness> {
        lock(&self.witness).clone().flatten()
    }

    /// Block until the launch probe is done, at most `timeout`; whether it is.
    pub fn wait_launch_probed(&self, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut seen = self.generation();
        while !self.launch_probed() {
            let now = std::time::Instant::now();
            if now >= deadline {
                return false;
            }
            seen = self.wait_change(seen, deadline - now);
        }
        true
    }

    /// The unambiguous newest swept record naming `session_id`. Millisecond timestamp
    /// ties cannot prove which process superseded the other, so neither grants authority.
    pub fn predecessor(&self, session_id: &str) -> Option<Predecessor> {
        let predecessors = lock(&self.predecessors);
        let newest = predecessors
            .iter()
            .filter(|p| p.session_id == session_id)
            .map(|p| p.updated_at_ms)
            .max()?;
        let mut candidates = predecessors
            .iter()
            .filter(|p| p.session_id == session_id && p.updated_at_ms == newest);
        let candidate = candidates.next()?;
        candidates.next().is_none().then(|| candidate.clone())
    }

    /// Reserve the process's first resume before its asynchronous evidence wait. This
    /// belongs to Shared so a replacement app-server registry cannot claim it again.
    pub fn reserve_relaunch(&self) -> bool {
        !self.relaunch_reserved.swap(true, Ordering::SeqCst)
    }

    /// Spend this process's one relaunch: `true` to the first caller only. A relaunch is
    /// one fact about this process's start (the swept predecessor, aterm's first `status`
    /// read), so it speaks for one carry-on in the process's life: a later resume of the
    /// same thread (`/resume` away and back) reads the same two facts and must not carry
    /// on again (CONTRACT §5 carry-on).
    pub fn spend_relaunch(&self) -> bool {
        !self.relaunch_spent.swap(true, Ordering::SeqCst)
    }

    /// Whether this process's relaunch is spent ([`Self::spend_relaunch`]).
    pub fn relaunch_spent(&self) -> bool {
        self.relaunch_spent.load(Ordering::SeqCst)
    }

    /// Take in one frame. Cheap when nothing moved (no allocation, no wake-up); any change
    /// bumps the generation the record writer waits on.
    pub fn observe(&self, frame: &Frame<'_>, now_ms: u64) -> bool {
        let mut upgrade = lock(&self.upgrade_generation);
        let mut changed = false;
        {
            let mut view = lock(&self.view);
            let view = &mut *view;
            changed |= replace_if_changed(&mut view.primary_thread, frame.primary_thread);
            changed |= replace_if_changed(&mut view.displayed_thread, frame.displayed_thread);
            changed |= replace_if_changed(&mut view.cwd, Some(frame.cwd));
            changed |= replace_if_changed(&mut view.model, Some(frame.model));
            changed |= replace_if_changed(&mut view.codex_home, Some(frame.codex_home));
            if view.embedded != frame.embedded {
                view.embedded = frame.embedded;
                changed = true;
            }
            if view.state != frame.state {
                view.state = frame.state;
                view.state_since_ms = now_ms;
                changed = true;
            }
            for thread in [frame.primary_thread, frame.displayed_thread]
                .into_iter()
                .flatten()
            {
                if !view.threads.contains_key(thread) {
                    view.threads
                        .insert(thread.to_owned(), ThreadFacts::default());
                    changed = true;
                }
            }
            if let Some(thread) = frame.displayed_thread
                && let Some(facts) = view.threads.get_mut(thread)
            {
                changed |= replace_if_changed(&mut facts.rollout_path, frame.rollout_path);
            }
        }
        for (flag, value) in [
            (&self.draft_empty, frame.draft_empty),
            (&self.queued_input, frame.queued_input),
            (&self.popup_open, frame.popup_open),
            (&self.editor_open, frame.editor_open),
            (&self.pending_user_turn, frame.pending_user_turn),
        ] {
            changed |= flag.swap(value, Ordering::SeqCst) != value;
        }
        // The frame drew whatever a key since the last one did to the composer.
        changed |= self.keyed_since_frame.swap(false, Ordering::SeqCst);
        if changed {
            *upgrade = upgrade.wrapping_add(1);
            self.mark_changed();
        }
        changed
    }

    /// A copy of the latest frame's facts.
    pub fn view(&self) -> View {
        lock(&self.view).clone()
    }

    /// Whether the TUI registered any thread in this process, primary or displayed, at any
    /// frame since it started: a registered thread is never dropped from [`View::threads`],
    /// so this stays true once it is, whatever the primary is now.
    pub fn any_thread_registered(&self) -> bool {
        !lock(&self.view).threads.is_empty()
    }

    /// Whether the TUI registered this thread (only those are the autopilot's).
    pub fn is_registered(&self, thread: &str) -> bool {
        lock(&self.view).threads.contains_key(thread)
    }

    /// The autopilot keeps this thread: a root thread of the TUI's own (never a sub-agent's,
    /// exec's or an app-server client's).
    pub fn note_root_thread(&self, thread: &str) {
        let mut upgrade = lock(&self.upgrade_generation);
        let mut roots = lock(&self.roots);
        if !roots.iter().any(|t| t == thread) {
            roots.push(thread.to_owned());
            *upgrade = upgrade.wrapping_add(1);
        }
    }

    /// Whether `thread` is one the TUI registered AND a root thread the autopilot keeps.
    pub fn is_root_thread(&self, thread: &str) -> bool {
        self.is_registered(thread) && lock(&self.roots).iter().any(|t| t == thread)
    }

    /// Record a registered thread's last turn (the autopilot's `on_turn_stop`).
    pub fn set_last_turn(&self, thread: &str, turn: LastTurn) -> bool {
        let mut view = lock(&self.view);
        let Some(facts) = view.threads.get_mut(thread) else {
            return false;
        };
        facts.last_turn = Some(turn);
        true
    }

    pub fn last_turn(&self, thread: &str) -> Option<LastTurn> {
        lock(&self.view).threads.get(thread)?.last_turn.clone()
    }

    /// A person (or anything typing into the pty) pressed a key, pasted or clicked. Until the
    /// next frame shows what it did to the composer, the record's `draftEmpty` reads `false`
    /// ([`Self::record_draft_empty`]): a person's first keystroke reaches the record before
    /// its echo, never after a reader took the session for at rest.
    pub fn human_input(&self, now_ms: u64) {
        self.stamp_human(now_ms, true);
    }

    /// A person scrolled, dragged or let go of a button: the human grace holds as for a key,
    /// but no draft began, so the record's `draftEmpty` stays what the last frame showed.
    pub fn human_attention(&self, now_ms: u64) {
        self.stamp_human(now_ms, false);
    }

    fn stamp_human(&self, now_ms: u64, keyed: bool) {
        let mut upgrade = lock(&self.upgrade_generation);
        self.human_input_ms.store(now_ms, Ordering::SeqCst);
        if keyed {
            self.keyed_since_frame.store(true, Ordering::SeqCst);
        }
        *upgrade = upgrade.wrapping_add(1);
        self.mark_changed();
    }

    /// `draftEmpty` as the record says it: the composer was empty at the last frame AND no
    /// key, paste or button press came after that frame. Turn readiness uses this same
    /// conservative value, including when the policy's human grace is zero.
    pub fn record_draft_empty(&self) -> bool {
        self.draft_empty() && !self.keyed_since_frame.load(Ordering::SeqCst)
    }

    /// When a person last gave the TUI input (ms since the epoch); `None` if never.
    pub fn last_human_input_ms(&self) -> Option<u64> {
        Some(self.human_input_ms.load(Ordering::SeqCst)).filter(|ms| *ms != 0)
    }

    pub fn draft_empty(&self) -> bool {
        self.draft_empty.load(Ordering::SeqCst)
    }

    pub fn queued_input(&self) -> bool {
        self.queued_input.load(Ordering::SeqCst)
    }

    pub fn popup_open(&self) -> bool {
        self.popup_open.load(Ordering::SeqCst)
    }

    pub fn pending_user_turn(&self) -> bool {
        self.pending_user_turn.load(Ordering::SeqCst)
    }

    /// ALAB(session) hook, each TUI event: a whole-screen overlay (the transcript, a diff, a
    /// backtrack preview) is up. The TUI draws no chat frame under it, so it is told here.
    pub fn set_overlay_open(&self, open: bool) {
        let mut upgrade = lock(&self.upgrade_generation);
        if self.overlay_open.swap(open, Ordering::SeqCst) != open {
            *upgrade = upgrade.wrapping_add(1);
            self.mark_changed();
        }
    }

    /// A whole-screen overlay or the external editor has the screen: a person reads or
    /// writes there, and nothing is done underneath (not even a box's answer).
    pub fn screen_taken(&self) -> bool {
        self.overlay_open.load(Ordering::SeqCst) || self.editor_open.load(Ordering::SeqCst)
    }

    /// The autopilot's way to give its claim back (set once, by the supervisor).
    pub fn set_release(&self, release: ReleaseHook) -> bool {
        self.release.set(release).is_ok()
    }

    /// The exit path, once the record is ended: give the claim back within `within`
    /// (CONTRACT §4: aterm reads the record at that change, so it sees the orderly end).
    /// `true` when nothing is held any more (or nothing ever was).
    pub fn release_claim(&self, within: Duration) -> bool {
        match self.release.get() {
            Some(release) => release(within),
            None => true,
        }
    }

    /// The supervisor's terminal-context reader (set once).
    pub fn set_context_reader(&self, reader: ContextReader) -> bool {
        self.context.set(reader).is_ok()
    }

    /// Whether aterm can give this session's terminal context now: a reader is installed and
    /// a proven claim is held (CONTRACT §2), so `/ide` takes its context from aterm.
    pub fn context_available(&self) -> bool {
        self.context.get().is_some() && self.claim().is_some()
    }

    /// This session's terminal context from aterm, waiting at most `within` (the TUI's submit
    /// path calls it, as upstream's IDE fetch blocks on its socket). `None` when no proven
    /// claim is held or aterm did not answer in time.
    pub fn terminal_context(&self, within: Duration) -> Option<TerminalContext> {
        self.claim()?;
        self.context.get().and_then(|read| read(within))
    }

    /// The supervisor's inbox reader (set once).
    pub fn set_mail_reader(&self, reader: MailReader) -> bool {
        self.mail.set(reader).is_ok()
    }

    /// One message of this session's inbox for a typed trigger (the TUI's submit path calls
    /// it, bounded by `within`): only while a proven claim is held. `Err` says why not.
    pub fn read_mail(
        &self,
        kind: crate::mail::TriggerKind,
        off: u64,
        within: Duration,
    ) -> Result<crate::mail::InboxMail, String> {
        if self.claim().is_none() {
            return Err("no proven aterm claim".to_string());
        }
        match self.mail.get() {
            Some(read) => read(kind, off, within),
            None => Err("no inbox reader".to_string()),
        }
    }

    /// The TUI is about to submit `turn` for a mail: its start is matched by its text, and
    /// its end gives the receipt. At most 8 wait (a turn that never started is forgotten).
    pub fn expect_mail_turn(&self, turn: crate::mail::MailTurn) {
        let mut turns = lock(&self.mail_turns);
        turns.retain(|t| t.id != turn.id);
        if turns.len() >= 8 {
            turns.remove(0);
        }
        turns.push(turn);
    }

    /// Whether `text` is a mail turn the TUI said it is about to submit, without using it up
    /// (`/ide` asks, so a mail turn carries no terminal context nobody asked for).
    pub fn is_mail_turn(&self, text: &str) -> bool {
        lock(&self.mail_turns).iter().any(|t| t.text == text)
    }

    /// The mail turn whose text this started turn has (each counts once).
    pub fn take_mail_turn(&self, text: &str) -> Option<crate::mail::MailTurn> {
        let mut turns = lock(&self.mail_turns);
        let at = turns.iter().position(|t| t.text == text)?;
        Some(turns.remove(at))
    }

    /// The status line's mail badge (display only); `true` when it moved.
    pub fn set_mail_badge(&self, badge: Option<crate::mail::MailBadge>) -> bool {
        let badge = badge.filter(|b| !b.is_empty());
        let mut slot = lock(&self.mail_badge);
        if *slot == badge {
            return false;
        }
        *slot = badge;
        true
    }

    pub fn mail_badge(&self) -> Option<crate::mail::MailBadge> {
        *lock(&self.mail_badge)
    }

    /// Note the terminating signal; the first one wins. `false` if one was already noted.
    pub fn set_signal(&self, signal: i32) -> bool {
        let mut upgrade = lock(&self.upgrade_generation);
        let changed = self
            .signal
            .compare_exchange(0, signal, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();
        if changed {
            if !RELAUNCH_SIGNALS.contains(&signal) {
                lock(&self.in_flight_retry).take();
                self.mark_changed();
            }
            *upgrade = upgrade.wrapping_add(1);
        }
        changed
    }

    /// The signal that ended this process, once one arrived.
    pub fn signal(&self) -> Option<i32> {
        Some(self.signal.load(Ordering::SeqCst)).filter(|signal| *signal != 0)
    }

    /// Whether this process is leaving on a signal aterm may relaunch it after
    /// ([`RELAUNCH_SIGNALS`]): the conversation goes on in that relaunch, which takes the act
    /// the autopilot owed at rest from the record the signal keeps ([`Self::set_owed_act`]).
    pub fn leaving_for_relaunch(&self) -> bool {
        self.signal()
            .is_some_and(|signal| RELAUNCH_SIGNALS.contains(&signal))
    }

    /// Ask the exit path to re-exec (the live upgrade) instead of ending.
    pub fn set_exec_pending(&self, pending: bool) {
        self.exec_pending.store(pending, Ordering::SeqCst);
        self.mark_changed();
    }

    pub fn exec_pending(&self) -> bool {
        self.exec_pending.load(Ordering::SeqCst)
    }

    pub fn set_autopilot(&self, autopilot: Autopilot) {
        self.publish_autopilot(autopilot, None);
    }

    /// [`Self::set_autopilot`] with the [`HarnessWord`] that goes with it, in one store.
    pub fn set_autopilot_with_word(&self, autopilot: Autopilot, word: HarnessWord) {
        self.publish_autopilot(autopilot, Some(word));
    }

    /// The Autopilot, and the word with it or the word standing. Only the Autopilot moves the
    /// generations; a word alone asks the TUI for a frame.
    fn publish_autopilot(&self, autopilot: Autopilot, word: Option<HarnessWord>) {
        let mut upgrade = lock(&self.upgrade_generation);
        let was = self.update_cell(|cell| {
            let word = word.map_or(cell & !AUTOPILOT_BITS, HarnessWord::bits);
            Some(word | autopilot.bits())
        });
        let changed = Autopilot::from_cell(was) != autopilot;
        let cleared = autopilot == Autopilot::Off && lock(&self.recovery).take().is_some();
        if changed || cleared {
            *upgrade = upgrade.wrapping_add(1);
            self.mark_changed();
            let _ = self.wake();
        } else if word.is_some_and(|word| HarnessWord::from_cell(was) != word) {
            let _ = self.wake();
        }
    }

    /// The Autopilot cell moved by `update` (`None` leaves it), answering what it was.
    fn update_cell(&self, mut update: impl FnMut(u8) -> Option<u8>) -> u8 {
        let mut cell = self.autopilot.load(Ordering::SeqCst);
        loop {
            let Some(next) = update(cell) else {
                return cell;
            };
            match self.autopilot.compare_exchange_weak(
                cell,
                next,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(was) => return was,
                Err(now) => cell = now,
            }
        }
    }

    pub fn autopilot(&self) -> Autopilot {
        self.harness().0
    }

    /// Publish the harness's Autopilot word for the TUI's chrome, the [`Autopilot`] standing.
    /// Nothing the record, the upgrade or provider recovery decides reads it, so it moves no
    /// generation; a change asks the TUI for a frame. Where the Autopilot moves with it, the
    /// supervisor publishes both at once ([`Self::set_autopilot_with_word`]).
    pub fn set_harness_word(&self, word: HarnessWord) {
        let was = self.update_cell(|cell| Some((cell & AUTOPILOT_BITS) | word.bits()));
        if HarnessWord::from_cell(was) != word {
            let _ = self.wake();
        }
    }

    /// [`Self::set_harness_word`] only while the Autopilot is [`Autopilot::Active`], decided
    /// in the same store: a mode moved under the acting claim, which never relabels a passive
    /// session that has gone passive meanwhile.
    pub fn set_acting_word(&self, word: HarnessWord) {
        let was = self.update_cell(|cell| {
            (Autopilot::from_cell(cell) == Autopilot::Active)
                .then(|| (cell & AUTOPILOT_BITS) | word.bits())
        });
        if Autopilot::from_cell(was) == Autopilot::Active && HarnessWord::from_cell(was) != word {
            let _ = self.wake();
        }
    }

    pub fn harness_word(&self) -> HarnessWord {
        self.harness().1
    }

    /// The [`Autopilot`] and the [`HarnessWord`] in one load: a pair the supervisor
    /// published, never one half of a change.
    pub fn harness(&self) -> (Autopilot, HarnessWord) {
        let cell = self.autopilot.load(Ordering::SeqCst);
        (Autopilot::from_cell(cell), HarnessWord::from_cell(cell))
    }

    /// Publish aterm's land timer for the TUI's line (`status land_at=`, unix seconds; `None`
    /// for none), as the supervisor's read of the word said it. The TUI's chrome alone reads
    /// it, as it does the word: it moves no generation, and a change asks for a frame.
    pub fn set_land_at(&self, at: Option<u64>) {
        let at = at.unwrap_or(0);
        if self.land_at.swap(at, Ordering::SeqCst) != at {
            let _ = self.wake();
        }
    }

    /// The land timer last published ([`Self::set_land_at`]).
    pub fn land_at(&self) -> Option<u64> {
        Some(self.land_at.load(Ordering::SeqCst)).filter(|&at| at > 0)
    }

    /// Publish `thread`'s [`TaskFace`] for the TUI's line, as its decider holds it now. The
    /// TUI's chrome alone reads it: it moves no generation, and a change asks for a frame.
    /// [`TaskFace::Open`] leaves no entry, so a stopped thread's goes with it.
    pub fn set_task_face(&self, thread: &str, face: TaskFace) {
        let moved = {
            let mut faces = lock(&self.task_faces);
            if face == TaskFace::Open {
                faces.remove(thread).is_some()
            } else {
                faces.insert(thread.to_owned(), face) != Some(face)
            }
        };
        if moved {
            let _ = self.wake();
        }
    }

    /// The primary thread's [`TaskFace`]: the conversation the TUI shows and the record
    /// speaks for. The view's lock is taken first, then the faces'; nothing takes them the
    /// other way round.
    pub fn task_face(&self) -> TaskFace {
        let view = lock(&self.view);
        let Some(primary) = view.primary_thread.as_deref() else {
            return TaskFace::Open;
        };
        lock(&self.task_faces)
            .get(primary)
            .copied()
            .unwrap_or_default()
    }

    /// The supervisor claim codex-alab holds now (`codex-alab@<pid>`), if any.
    pub fn set_claim(&self, claim: Option<String>) {
        let mut upgrade = lock(&self.upgrade_generation);
        let mut slot = lock(&self.claim);
        if *slot != claim {
            *slot = claim;
            *upgrade = upgrade.wrapping_add(1);
            drop(slot);
            self.mark_changed();
            let _ = self.wake();
        }
    }

    pub fn claim(&self) -> Option<String> {
        lock(&self.claim).clone()
    }

    /// The box glue saw its box again this frame: `true` when `id` is the open box and its
    /// `begun` reading is unchanged (nothing is allocated then); `false` asks for a full
    /// [`Shared::report_box`].
    pub fn touch_box(&self, id: &str, begun: Option<&str>) -> bool {
        let mut slot = lock(&self.boxes);
        let same = slot.open.as_ref().is_some_and(|open| {
            open.id == id
                && match &open.kind {
                    crate::boxes::BoxKind::Question { begun: was, .. } => was.as_deref() == begun,
                    crate::boxes::BoxKind::Plan { .. }
                    | crate::boxes::BoxKind::Goal { .. }
                    | crate::boxes::BoxKind::Approval { .. } => true,
                }
        });
        if same {
            slot.seen = true;
        }
        same
    }

    /// The box glue reports the box it draws (its `opened_ms` is kept while its id is).
    pub fn report_box(&self, mut open: OpenBox) {
        let mut upgrade = lock(&self.upgrade_generation);
        let mut slot = lock(&self.boxes);
        slot.seen = true;
        if let Some(was) = &slot.open
            && was.id == open.id
        {
            open.opened_ms = was.opened_ms;
            if *was == open {
                return;
            }
        } else if let Some(covered) = slot.pending.take_if(|p| p.id() != open.id) {
            // Another box covers the one the answer was for: it was never applied, and the
            // poster hears so, or a covered box that comes back under its own id would wait
            // for this answer's outcome for good (review of ca681c4b01).
            if slot.outcome.is_none() {
                slot.outcome = Some(BoxOutcome {
                    id: covered.id().to_owned(),
                    applied: Err("another box covered it before the answer was taken".into()),
                });
            }
        }
        slot.open = Some(open);
        *upgrade = upgrade.wrapping_add(1);
        drop(slot);
        self.mark_changed();
    }

    /// A frame ends: a box no glue reported this frame is closed (with any answer pending
    /// for it). Returns whether one was.
    pub fn end_frame_boxes(&self) -> bool {
        let mut slot = lock(&self.boxes);
        let seen = std::mem::take(&mut slot.seen);
        if seen || slot.open.is_none() {
            return false;
        }
        slot.open = None;
        slot.pending = None;
        slot.view_at = None;
        drop(slot);
        self.mark_changed();
        true
    }

    /// The glue's box `id` took its answer and closed (a queued request that follows is
    /// reported on its own frame).
    pub fn close_box(&self, id: &str) {
        let mut slot = lock(&self.boxes);
        if slot.open.as_ref().is_some_and(|open| open.id == id) {
            slot.open = None;
            slot.seen = false;
            slot.pending = None;
            slot.view_at = None;
            drop(slot);
            self.mark_changed();
        }
    }

    /// The id a selection view at `addr` reports its box by, `kind` naming the box (`plan`,
    /// `goal`): such a view has no id of its own, so it keeps one while it stays the open box
    /// (`addr` is its place in memory, stable while it is boxed on the view stack). A box that
    /// closed forgets its place, so a later view there is a new box with a new id.
    pub fn view_box_id(&self, kind: &str, addr: usize) -> String {
        let mut slot = lock(&self.boxes);
        if slot.view_at != Some(addr) {
            slot.view_at = Some(addr);
            slot.views += 1;
        }
        format!("{kind}-{}", slot.views)
    }

    /// The TUI is about to open "Resume paused goal?" over a goal stopped as `stop`.
    pub fn set_goal_prompt(&self, stop: crate::boxes::GoalStop) {
        lock(&self.boxes).goal_stop = Some(stop);
    }

    /// Why the goal the TUI last opened "Resume paused goal?" over stopped, if it said.
    pub fn goal_prompt(&self) -> Option<crate::boxes::GoalStop> {
        lock(&self.boxes).goal_stop
    }

    /// The box the TUI shows now.
    pub fn open_box(&self) -> Option<OpenBox> {
        lock(&self.boxes).open.clone()
    }

    /// The autopilot's answer for the open box (refused unless it is that box's).
    pub fn post_box_answer(&self, answer: BoxAnswer) -> bool {
        let mut slot = lock(&self.boxes);
        if slot.open.as_ref().is_none_or(|open| open.id != answer.id()) {
            return false;
        }
        slot.pending = Some(answer);
        slot.pending_ms = now_ms();
        drop(slot);
        self.mark_changed();
        true
    }

    /// Take back an answer the glue has not applied yet (the claim was lost).
    pub fn withdraw_box_answer(&self) -> bool {
        let withdrawn = lock(&self.boxes).pending.take().is_some();
        if withdrawn {
            self.mark_changed();
        }
        withdrawn
    }

    /// The glue takes the answer posted for its box `id`, if one is. A person's key, paste or
    /// click since it was posted, or an answer older than [`BOX_ANSWER_TTL`], withdraws it
    /// instead (the box is theirs; the outcome says why).
    pub fn take_box_answer(&self, id: &str) -> Option<BoxAnswer> {
        let mut slot = lock(&self.boxes);
        if !slot.pending.as_ref().is_some_and(|p| p.id() == id) {
            return None;
        }
        let posted = slot.pending_ms;
        let why = if self.last_human_input_ms().is_some_and(|at| at >= posted) {
            Some("a person's input came after the answer was posted")
        } else if now_ms().saturating_sub(posted)
            > u64::try_from(BOX_ANSWER_TTL.as_millis()).unwrap_or(u64::MAX)
        {
            Some("the answer was not applied in time")
        } else {
            None
        };
        let answer = slot.pending.take();
        if let Some(why) = why {
            slot.outcome = Some(BoxOutcome {
                id: id.to_owned(),
                applied: Err(why.to_owned()),
            });
            drop(slot);
            self.mark_changed();
            return None;
        }
        drop(slot);
        self.mark_changed();
        answer
    }

    /// Whether an answer is posted and not yet taken.
    pub fn box_answer_pending(&self) -> bool {
        lock(&self.boxes).pending.is_some()
    }

    /// The glue reports what it did with an answer.
    pub fn set_box_outcome(&self, outcome: BoxOutcome) {
        lock(&self.boxes).outcome = Some(outcome);
        self.mark_changed();
    }

    /// The autopilot takes the last outcome.
    pub fn take_box_outcome(&self) -> Option<BoxOutcome> {
        lock(&self.boxes).outcome.take()
    }

    /// The TUI's frame request (the glue sets it once); only the first call counts.
    pub fn set_waker(&self, waker: Callback) -> bool {
        self.waker.set(waker).is_ok()
    }

    pub fn has_waker(&self) -> bool {
        self.waker.get().is_some()
    }

    /// Ask the TUI for a frame (its glue then applies a posted answer).
    pub fn wake(&self) -> bool {
        match self.waker.get() {
            Some(wake) => {
                wake();
                true
            }
            None => false,
        }
    }

    /// The TUI's `/exit` (`AppEvent::Exit(ExitMode::ShutdownFirst)`); only the first call
    /// counts.
    pub fn set_leave(&self, leave: Callback) -> bool {
        self.leave.set(leave).is_ok()
    }

    /// Leave as `/exit` does (the live upgrade). `false` when no TUI said how.
    pub fn leave(&self) -> bool {
        match self.leave.get() {
            Some(leave) => {
                leave();
                true
            }
            None => false,
        }
    }

    pub fn set_upgrade_leave(&self, leave: UpgradeLeave) -> bool {
        self.upgrade_leave.set(leave).is_ok()
    }

    /// Queue a conditional TUI event. Its handler, rather than this caller, commits leave.
    pub fn queue_upgrade(&self, request: UpgradeRequest) -> bool {
        match self.upgrade_leave.get() {
            Some(leave) => {
                leave(request);
                true
            }
            None => false,
        }
    }

    /// Stamp the native state before collecting asynchronous all-thread facts.
    pub fn upgrade_generation(&self) -> u64 {
        *lock(&self.upgrade_generation)
    }

    /// A loaded-thread lifecycle change outside the displayed TUI invalidates its scan.
    pub fn invalidate_upgrade(&self) {
        let mut generation = lock(&self.upgrade_generation);
        *generation = generation.wrapping_add(1);
    }

    /// The final synchronous TUI commit, serialized with native frame/input/turn changes.
    pub fn commit_upgrade(&self, expected: u64) -> bool {
        let generation = lock(&self.upgrade_generation);
        if *generation != expected
            || !self.record_draft_empty()
            || self.queued_input()
            || self.popup_open()
            || self.screen_taken()
            || self.pending_user_turn()
            || self.turn_running()
            || self.signal().is_some()
            || self.autopilot() != Autopilot::Active
        {
            return false;
        }
        self.set_exec_pending(true);
        true
    }

    /// Own the admitted execution together with this session's successful native
    /// commit. The opaque closure crosses the core/TUI dependency boundary; it
    /// supplies no execution authority. No fallible work follows `decide`.
    pub fn commit_upgrade_execution(
        &self,
        execution: UpgradeExecution,
        decide: impl FnOnce() -> bool,
    ) -> bool {
        let mut pending = lock(&self.upgrade_execution);
        if pending.is_some() || !decide() {
            return false;
        }
        *pending = Some(execution);
        true
    }

    /// Consume only the plan this session committed, after its native teardown.
    pub fn take_upgrade_execution(&self) -> Option<UpgradeExecution> {
        lock(&self.upgrade_execution).take()
    }

    /// Withdraw a rejected/cancelled intent and restore any still-current act while a
    /// native turn start cannot supersede it between the check and timer restoration.
    pub fn cancel_upgrade(&self, restore: impl FnOnce()) -> bool {
        let mut generation = lock(&self.upgrade_generation);
        if self.exec_pending() {
            return false;
        }
        *generation = generation.wrapping_add(1);
        restore();
        true
    }

    /// The TUI is about to submit `text` as a user turn on codex-alab's behalf (the plan
    /// box's yes): the autopilot counts that turn as its own, not a person's.
    pub fn expect_own_turn(&self, text: &str) {
        let mut own = lock(&self.own_turns);
        if own.len() >= 8 {
            own.remove(0);
        }
        own.push(text.to_owned());
    }

    /// Whether `text` is a turn codex-alab said it is about to submit ([`Self::expect_own_turn`]),
    /// without using the expectation up: the TUI's `/ide` path asks, so codex-alab's own turn
    /// carries no context a person did not ask for.
    pub fn is_own_turn(&self, text: &str) -> bool {
        lock(&self.own_turns).iter().any(|t| t == text)
    }

    /// Whether a user turn with `text` was codex-alab's (each expectation counts once). A turn
    /// that left with an IDE context ahead of it (upstream's `/ide` outside aterm) is matched on
    /// the request after the context's marker, so the context does not make it a person's.
    pub fn take_own_turn(&self, text: &str) -> bool {
        let request = text
            .rsplit_once(PROMPT_REQUEST_MARKER)
            .map_or(text, |(_, request)| request);
        let mut own = lock(&self.own_turns);
        match own.iter().position(|t| t == text || t == request) {
            Some(at) => {
                own.remove(at);
                true
            }
            None => false,
        }
    }

    /// A pre-creation provider continuation carries on `source`'s work. The next
    /// root creation takes this mark. Already prepared plan targets instead use
    /// [`Self::continue_prepared_work`] at their accepted input boundary.
    pub fn continue_work(&self, source: &str) {
        *lock(&self.continues) = Some(source.to_owned());
    }

    /// The source thread of the continuation the TUI said it is starting
    /// ([`Self::continue_work`]), used up: the next root thread to start takes it.
    pub fn take_continuation(&self) -> Option<String> {
        lock(&self.continues).take()
    }

    /// A successfully prepared plan continues only this target and accepted prompt.
    /// It cannot classify a later root creation or an edited first message as work.
    pub fn continue_prepared_work(&self, source: &str, target: &str, prompt: &str) {
        lock(&self.prepared_continuation).insert(target.into(), (source.into(), prompt.into()));
    }

    /// Another target cannot consume the mark. Different first input cancels it.
    pub fn take_prepared_continuation(&self, target: &str, prompt: &str) -> Option<String> {
        lock(&self.prepared_continuation)
            .remove(target)
            .and_then(|(source, expected)| (expected == prompt).then_some(source))
    }

    /// A stopped target cannot leave a classification for a later incarnation.
    pub fn discard_prepared_continuation(&self, target: &str) {
        lock(&self.prepared_continuation).remove(target);
    }

    /// The approval contributor's review of a box (its `keys`): `Some(what)` when it would
    /// approve it once but the checks said "not now" ([`crate::boxes::Preapproval`]), `None`
    /// when it would not (a newer review of the same box replaces an older one).
    pub fn set_preapproval(&self, keys: &[&str], what: Option<String>) {
        let mut all = lock(&self.preapprovals);
        all.retain(|p| !p.keys.iter().any(|k| keys.contains(&k.as_str())));
        if let Some(what) = what {
            if all.len() >= 16 {
                all.remove(0);
            }
            all.push(crate::boxes::Preapproval {
                keys: keys.iter().map(|k| (*k).to_owned()).collect(),
                what,
            });
        }
    }

    /// What the reviewed box `key` approves, if the contributor cleared it.
    pub fn preapproval(&self, key: &str) -> Option<String> {
        lock(&self.preapprovals)
            .iter()
            .find(|p| p.keys.iter().any(|k| k == key))
            .map(|p| p.what.clone())
    }

    /// The box `key` was answered (or closed): its review is used up.
    pub fn take_preapproval(&self, key: &str) -> Option<String> {
        let mut all = lock(&self.preapprovals);
        let at = all.iter().position(|p| p.keys.iter().any(|k| k == key))?;
        Some(all.remove(at).what)
    }

    /// The record writer wrote a record whose `resumeArgv` (the line aterm's crash relaunch
    /// would type) differs from the last one written: aterm's copy, read at the claim, is
    /// stale until the claim is renewed (a renewal re-reads the record).
    pub fn note_resume_line(&self) {
        self.resume_lines.fetch_add(1, Ordering::SeqCst);
        self.mark_changed();
    }

    /// A resumed thread owes the carry-on after aterm's crash relaunch and has not typed it
    /// yet: the record says `busy` meanwhile, so a crash before the carry-on is again one
    /// that died working, and the next relaunch still carries on (end-to-end run 2026-10-16:
    /// a kill -9 of the relaunched codex before its act left the task stalled).
    pub fn set_carry_on_owed(&self, owed: bool) {
        if self.carry_on_owed.swap(owed, Ordering::SeqCst) != owed {
            self.mark_changed();
        }
    }

    pub fn carry_on_owed(&self) -> bool {
        self.carry_on_owed.load(Ordering::SeqCst)
    }

    /// The record's `carriedOn`: this process typed the carry-on after aterm's relaunch, the
    /// `n`th in a row with no person's turn and no real work between (its predecessor's count
    /// plus one), or it declined one and kept its predecessor's count; 0 again once a person's
    /// turn starts, a turn the autopilot typed works for real (FOREVER CRUISE never ends a task
    /// on its words, so a done no longer resets it), or Land lands. A relaunch whose predecessor's record says one or more gets
    /// no carry-on: a carry-on led to that death, with no person since (a stall restarted
    /// again), and a loop of stall, restart, continue waits for a person instead.
    pub fn set_carried_on(&self, n: u32) {
        if self.carried_on.swap(n, Ordering::SeqCst) != n {
            self.mark_changed();
        }
    }

    pub fn carried_on(&self) -> u32 {
        self.carried_on.load(Ordering::SeqCst)
    }

    /// The act the autopilot owes ([`OwedAct`]): kept while a paced act waits (the back-off's
    /// wait is rest, so a crash there leaves it to aterm's relaunch), and the one the live
    /// upgrade takes as it leaves, which the record carries across the re-exec; `None` clears
    /// it (the act went or was dropped, an upgrade that did not go). A thread that stops on an
    /// exit aterm may relaunch leaves it set ([`Self::leaving_for_relaunch`]).
    pub fn set_owed_act(&self, owed: Option<OwedAct>) {
        // Native acceptance supersedes this intent. Keep the check and publication
        // under the same turn book as `turn_started` so another root's record sync
        // cannot republish an old act after that turn became busy.
        let turns = lock(&self.turns);
        if owed.as_ref().is_some_and(|act| {
            turns
                .running
                .iter()
                .any(|(thread, _)| *thread == act.thread)
        }) {
            return;
        }
        let mut slot = self
            .owed_act
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // The renewal receipt remains durable until native acceptance is
        // acknowledged. Ordinary timer bookkeeping cannot erase its custody.
        if slot.as_ref().is_some_and(|act| act.renewal.is_some())
            && owed.as_ref().is_none_or(|act| act.renewal.is_none())
        {
            return;
        }
        if *slot != owed {
            *slot = owed;
            drop(slot);
            self.mark_changed();
        }
    }

    pub fn assignment_receipt(&self) -> Option<crate::assignment::Receipt> {
        lock(&self.assignment).clone()
    }
    pub fn seed_assignment_receipt(&self, receipt: Option<crate::assignment::Receipt>) {
        if receipt.as_ref().is_some_and(|receipt| !receipt.valid()) {
            return;
        }
        let mut slot = lock(&self.assignment);
        if slot.is_none() {
            *slot = receipt;
        }
    }
    pub fn set_assignment_receipt(&self, receipt: Option<crate::assignment::Receipt>) -> bool {
        if receipt.as_ref().is_some_and(|receipt| !receipt.valid()) {
            return false;
        }
        let mut upgrade = lock(&self.upgrade_generation);
        self.assignment_touched.store(true, Ordering::SeqCst);
        let mut slot = lock(&self.assignment);
        if slot.as_ref().is_some_and(|existing| {
            !existing.acknowledged
                && receipt
                    .as_ref()
                    .is_some_and(|next| existing.operation != next.operation)
        }) {
            return false;
        }
        if *slot != receipt {
            *slot = receipt;
            *upgrade = upgrade.wrapping_add(1);
            drop(slot);
            self.mark_changed();
        }
        true
    }
    pub fn clear_assignment_receipt(&self, operation: &str) -> bool {
        let mut upgrade = lock(&self.upgrade_generation);
        let mut slot = lock(&self.assignment);
        if slot
            .as_ref()
            .is_none_or(|receipt| receipt.operation != operation)
        {
            return false;
        }
        *slot = None;
        *upgrade = upgrade.wrapping_add(1);
        drop(slot);
        self.mark_changed();
        true
    }
    pub fn has_record_writer(&self) -> bool {
        self.flush.get().is_some()
    }

    pub fn owed_act(&self) -> Option<OwedAct> {
        self.owed_act
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Publish a renewal only at the native point observed before its awaits.
    pub fn publish_renewal(&self, generation: u64, owed: OwedAct) -> bool {
        let current = lock(&self.upgrade_generation);
        if *current != generation || owed.renewal.is_none() {
            return false;
        }
        self.set_owed_act(Some(owed.clone()));
        self.owed_act().as_ref() == Some(&owed)
    }

    /// A verified, durably admitted recovery reuses the original native ID.
    /// This only prepares bookkeeping; it never starts or authorizes input.
    pub fn admit_recovered_turn(&self, thread: &str, turn: &str) -> bool {
        let _generation = lock(&self.upgrade_generation);
        let mut book = lock(&self.turns);
        if book.running.iter().any(|(id, _)| id == thread)
            || book.latest.get(thread).is_some_and(|id| id != turn)
        {
            return false;
        }
        book.ended
            .retain(|ended| ended.0 != thread || ended.1 != turn);
        true
    }

    /// Retire only this acknowledged renewal; a newer intent keeps its custody.
    pub fn discard_renewal(&self, expected: &OwedAct) -> bool {
        let mut owed = lock(&self.owed_act);
        if owed.as_ref() != Some(expected) || expected.renewal.is_none() {
            return false;
        }
        *owed = None;
        drop(owed);
        self.mark_changed();
        true
    }

    pub fn clear_renewal(&self, receipt: &str) -> bool {
        let mut owed = lock(&self.owed_act);
        if !owed
            .as_ref()
            .and_then(|act| act.renewal.as_ref())
            .is_some_and(|renewal| renewal.receipt.as_deref() == Some(receipt))
        {
            return false;
        }
        *owed = None;
        drop(owed);
        self.mark_changed();
        true
    }

    /// THE NEEDS AN OFF TOOK DOWN (review of the forever autopilot, 2026-10-20): `key` is the
    /// NEEDS YOU set a person's or the config's Off took off the person's screen, `0` once a
    /// report raises a set again. An Off is the one end of a need the thread's history does
    /// not show (a person's turn and a `NEEDS YOU: nothing` report are in it), so the record
    /// carries it (`needsOff`), and the image after a re-exec, or a relaunch after a crash,
    /// puts back the needs the history leaves standing unless they are this set.
    pub fn set_needs_off(&self, key: u64) {
        if self.needs_off.swap(key, Ordering::SeqCst) != key {
            self.mark_changed();
        }
    }

    /// [`Self::set_needs_off`]'s key (`0`: no Off took a set down, or a set stands again).
    pub fn needs_off(&self) -> u64 {
        self.needs_off.load(Ordering::SeqCst)
    }

    /// The folder `thread`'s worker ran its last command in (the autopilot's repository probe
    /// reads it). The record carries the primary thread's (`workdir`), so the image after a
    /// re-exec, or a relaunch after a crash, probes where the worker was working.
    pub fn set_workdir(&self, thread: &str, dir: &Path) {
        let mut dirs = lock(&self.workdirs);
        if dirs.get(thread).map(PathBuf::as_path) != Some(dir) {
            dirs.insert(thread.to_owned(), dir.to_path_buf());
            drop(dirs);
            self.mark_changed();
        }
    }

    /// [`Self::set_workdir`]'s folder for `thread`, if a command ran.
    pub fn workdir(&self, thread: &str) -> Option<PathBuf> {
        lock(&self.workdirs).get(thread).cloned()
    }

    /// What the autopilot's repository probe reads for `thread` ([`ProbeScope`]). The record
    /// carries the primary thread's (`probeScope`), so the image after a re-exec, or a relaunch
    /// after a crash, reads what the worker touched before it.
    pub fn set_probe_scope(&self, thread: &str, scope: &ProbeScope) {
        let mut scopes = lock(&self.probe_scopes);
        if scopes.get(thread) != Some(scope) {
            scopes.insert(thread.to_owned(), scope.clone());
            drop(scopes);
            self.mark_changed();
        }
    }

    /// [`Self::set_probe_scope`]'s scope for `thread`, if one was set.
    pub fn probe_scope(&self, thread: &str) -> Option<ProbeScope> {
        lock(&self.probe_scopes).get(thread).cloned()
    }

    /// The scope a record of session `session` carried: the previous image's at a re-exec, or a
    /// swept dead process's. Two for one session are merged ([`ProbeScope::merge`]), so nothing
    /// either touched is left unread.
    pub fn carry_probe_scope(&self, session: &str, scope: &ProbeScope) {
        lock(&self.carried_scopes)
            .entry(session.to_owned())
            .or_default()
            .merge(scope);
    }

    /// [`Self::carry_probe_scope`]'s scope for session `session`, if a record carried one.
    pub fn carried_probe_scope(&self, session: &str) -> Option<ProbeScope> {
        lock(&self.carried_scopes).get(session).cloned()
    }

    /// Thread `thread`'s active goal objective, if one is `active` (`None` clears it). The
    /// record carries the primary thread's (`goal`), bounded to [`GOAL_MAX_BYTES`] UTF-8 bytes
    /// after trimming, so aterm reads the session's standing goal without opening SQLite.
    pub fn set_goal(&self, thread: &str, goal: Option<&str>) {
        if thread.is_empty() {
            return;
        }
        let bounded = goal
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| &text[..text.floor_char_boundary(GOAL_MAX_BYTES)]);
        let mut goals = lock(&self.goals);
        let changed = match bounded {
            Some(text) if goals.get(thread).map(String::as_str) != Some(text) => {
                goals.insert(thread.to_owned(), text.to_owned());
                true
            }
            Some(_) => false,
            None => goals.remove(thread).is_some(),
        };
        drop(goals);
        if changed {
            self.mark_changed();
        }
    }

    /// [`Self::set_goal`]'s active goal objective for `thread`, if one stands.
    pub fn goal(&self, thread: &str) -> Option<String> {
        lock(&self.goals).get(thread).cloned()
    }

    /// The background terminals the TUI's footer counts for the thread it shows (its chat widget's
    /// live unified exec processes, one a running tool call still waits on included), handed over at
    /// each frame at `now`: a moved count marks the record changed and starts that move's hold
    /// ([`Self::background_terminals_held`]); the same count again does neither.
    pub fn set_background_terminals(&self, count: u32, now: Instant) {
        let moved = {
            let mut terminals = lock(&self.background_terminals);
            let moved = terminals.count != Some(count);
            if moved {
                *terminals = TerminalCount {
                    count: Some(count),
                    moved: Some(now),
                };
            }
            moved
        };
        if moved {
            self.mark_changed();
        }
    }

    /// [`Self::set_background_terminals`]'s count; `None` before a frame handed one.
    pub fn background_terminals(&self) -> Option<u32> {
        lock(&self.background_terminals).count
    }

    /// Whether the count's last move, at `now`, has stood less than `hold` (the record writer's
    /// interval) and its hold was not ended: the writer leaves a record that only this move
    /// changes unwritten until then, unless another change writes it first (CONTRACT §4), so a
    /// command that ends within the hold costs no write.
    pub fn background_terminals_held(&self, now: Instant, hold: Duration) -> bool {
        lock(&self.background_terminals)
            .moved
            .is_some_and(|moved| now.saturating_duration_since(moved) < hold)
    }

    /// When the count's last move will have stood `hold`, while its hold was not ended
    /// ([`Self::expire_background_terminals_hold`]): the record writer wakes then.
    pub fn background_terminals_hold_deadline(&self, hold: Duration) -> Option<Instant> {
        lock(&self.background_terminals)
            .moved
            .and_then(|moved| moved.checked_add(hold))
    }

    /// End the hold of the count's last move once it has stood `hold` by `now`; whether one
    /// ended (a change the record writer writes).
    pub fn expire_background_terminals_hold(&self, now: Instant, hold: Duration) -> bool {
        let expired = {
            let mut terminals = lock(&self.background_terminals);
            let expired = terminals
                .moved
                .is_some_and(|moved| now.saturating_duration_since(moved) >= hold);
            if expired {
                terminals.moved = None;
            }
            expired
        };
        if expired {
            self.mark_changed();
        }
        expired
    }

    /// Thread `thread`'s pace now ([`Pace`]): the record carries the primary thread's, so the
    /// image after a re-exec, or a relaunch after a crash, takes it up. A pace at rest is
    /// none. A wall's goal bound before a person's stop this process saw is no wall's
    /// ([`Self::person_goal_set`]): the pace keeps none, so the record never carries it.
    pub fn set_pace(&self, thread: &str, mut pace: Pace) {
        let mut paces = lock(&self.paces);
        if pace
            .wall_goal
            .as_ref()
            .is_some_and(|goal| goal.person_stops != self.person_goal_stops(thread))
        {
            pace.wall_goal = None;
        }
        let changed = if pace.at_rest() {
            paces.remove(thread).is_some()
        } else if paces.get(thread) != Some(&pace) {
            paces.insert(thread.to_owned(), pace);
            true
        } else {
            false
        };
        drop(paces);
        if changed {
            self.mark_changed();
        }
    }

    /// [`Self::set_pace`]'s pace for `thread`; `None` at rest.
    pub fn pace(&self, thread: &str) -> Option<Pace> {
        lock(&self.paces).get(thread).cloned()
    }

    /// A PERSON SET THREAD `thread`'S GOAL, leaving it `stopped` (any status but active; the
    /// app-server notes each set a person makes as it passes, [`crate::goal_stops`]): a stop is
    /// counted, and the goal a wall's turn error blocked is taken out of the thread's pace at once,
    /// so neither the record a re-exec or a relaunch takes up nor a `/resume` back carries it
    /// (review of the forever-persist lane, F2: a stop between the binding and a restart was
    /// forgotten, and the image after resumed the goal the person had stopped). A set that leaves
    /// the goal active counts none, so a wall's stop after a person's resume is still the wall's.
    pub fn person_goal_set(&self, thread: &str, stopped: bool) {
        if !stopped || thread.is_empty() {
            return;
        }
        let changed = {
            let mut paces = lock(&self.paces);
            let mut stops = lock(&self.person_goal_stops);
            let count = stops.entry(thread.to_owned()).or_default();
            *count = count.saturating_add(1);
            drop(stops);
            match paces.get_mut(thread) {
                Some(pace) if pace.wall_goal.is_some() => {
                    pace.wall_goal = None;
                    if pace.at_rest() {
                        paces.remove(thread);
                    }
                    true
                }
                _ => false,
            }
        };
        if changed {
            self.mark_changed();
        }
    }

    /// How many of a person's stops of thread `thread`'s goal this process saw
    /// ([`Self::person_goal_set`]).
    pub fn person_goal_stops(&self, thread: &str) -> u64 {
        lock(&self.person_goal_stops)
            .get(thread)
            .copied()
            .unwrap_or_default()
    }

    /// The primary thread's unresolved failure. Its record carries only the intent and
    /// original deadlines; the next image must prove its handoff and recheck live policy.
    pub fn set_recovery(&self, recovery: Option<Recovery>) {
        let mut current = lock(&self.recovery);
        if *current == recovery {
            return;
        }
        *current = recovery;
        drop(current);
        self.mark_changed();
    }

    pub fn recovery(&self) -> Option<Recovery> {
        lock(&self.recovery).clone()
    }

    /// The exact native turn's pacing fact. Setting it grants no retry or continuation;
    /// the crash relaunch must independently prove its ordinary carry-on authority.
    pub fn set_in_flight_retry(&self, retry: Option<InFlightRetry>) {
        let mut current = lock(&self.in_flight_retry);
        if *current != retry {
            *current = retry;
            drop(current);
            self.mark_changed();
        }
    }

    pub fn in_flight_retry(&self) -> Option<InFlightRetry> {
        lock(&self.in_flight_retry).clone()
    }

    /// A retry hook can publish only for the current primary turn that core still runs.
    /// Passive/bootstrap/Off keep pacing facts; they still authorize no autopilot act.
    pub fn publish_in_flight_retry(&self, retry: InFlightRetry) -> bool {
        let _generation = lock(&self.upgrade_generation);
        if self.signal().is_some() && !self.leaving_for_relaunch() {
            return false;
        }
        let primary = self.view().primary_thread;
        let selected = match primary.as_deref() {
            Some(primary) => primary == retry.thread,
            None => {
                let roots = lock(&self.roots);
                roots.len() == 1 && roots.first() == Some(&retry.thread)
            }
        };
        if !selected {
            return false;
        }
        let turns = lock(&self.turns);
        if turns.latest.get(&retry.thread) != Some(&retry.turn_id)
            || !turns
                .running
                .contains(&(retry.thread.clone(), retry.turn_id.clone()))
        {
            return false;
        }
        self.set_in_flight_retry(Some(retry));
        true
    }

    /// Apply a resume's local state and intent bookkeeping atomically with native
    /// acceptance. The closure follows native -> caller-state lock order and uses
    /// only bookkeeping setters, never another native-boundary method or I/O.
    pub fn apply_resume_at_rest<T>(&self, thread: &str, restore: impl FnOnce() -> T) -> Option<T> {
        let mut generation = lock(&self.upgrade_generation);
        if self.turn_running_on(thread) {
            return None;
        }
        *generation = generation.wrapping_add(1);
        Some(restore())
    }

    /// Startup seeding imports process-wide handoff fields, so any tracked root's
    /// newer native work supersedes it, even after that turn completes or stops.
    /// Return whether native work has been seen, its unique thread, and whether
    /// this process accepted a turn (rather than only stopping during teardown).
    /// A restored frame/root alone is not native acceptance or a stop.
    pub fn native_work(&self) -> (bool, Option<String>, bool) {
        let turns = lock(&self.turns);
        let mut threads = turns
            .latest
            .keys()
            .chain(turns.stopped_threads.iter())
            .chain(turns.ended.iter().map(|(thread, _)| thread));
        let first = threads.next();
        let unique = first
            .filter(|first| threads.all(|thread| thread == *first))
            .cloned();
        (first.is_some(), unique, !turns.latest.is_empty())
    }

    /// Seed only before the native work tracked by [`Self::native_work`].
    /// The closure uses only bookkeeping setters under this boundary.
    pub fn seed_before_native_work(&self, seed: impl FnOnce()) -> bool {
        let _generation = lock(&self.upgrade_generation);
        if self.assignment_touched.load(Ordering::SeqCst) {
            return false;
        }
        let turns = lock(&self.turns);
        if !turns.latest.is_empty() || !turns.stopped_threads.is_empty() || !turns.ended.is_empty()
        {
            return false;
        }
        drop(turns);
        seed();
        true
    }

    /// A completed/cancelled wait cannot clear another turn's selected wait. HUP/TERM
    /// teardown retains the bound; a person's interrupt and ordinary stops discard it.
    pub fn clear_in_flight_retry(&self, thread: &str, turn: &str) -> bool {
        let _generation = lock(&self.upgrade_generation);
        if self.leaving_for_relaunch() {
            return false;
        }
        let mut current = lock(&self.in_flight_retry);
        if current
            .as_ref()
            .is_some_and(|retry| retry.thread == thread && retry.turn_id == turn)
        {
            *current = None;
            drop(current);
            self.mark_changed();
            return true;
        }
        false
    }

    /// Publish a checked failure only while the same native state still holds. A newly
    /// accepted turn either rejects this publication or clears it before write-through.
    /// `validate` checks the decision's state under the native lock; callers must acquire
    /// their decision lock there, rather than hold it while entering this method.
    pub fn publish_recovery(
        &self,
        expected: u64,
        recovery: Recovery,
        validate: impl FnOnce() -> bool,
    ) -> bool {
        let generation = lock(&self.upgrade_generation);
        if *generation != expected
            || self.turn_running_on(&recovery.thread)
            || self.autopilot() != Autopilot::Active
            || !validate()
        {
            return false;
        }
        self.set_recovery(Some(recovery));
        true
    }

    /// codex-alab is about to act (submit a turn, compact, resume a goal): the record says
    /// `busy` until the returned guard drops. Take it BEFORE [`Self::flush_record`] and the
    /// act, and drop it once the act is done (its turn has started, or it failed).
    pub fn begin_act(&self) -> ActGuard<'_> {
        self.acts.fetch_add(1, Ordering::SeqCst);
        self.mark_changed();
        ActGuard { shared: self }
    }

    /// Whether an act stands ([`Self::begin_act`]), or one whose turn has not started yet
    /// ([`Self::await_turn`]).
    pub fn acting(&self) -> bool {
        self.acts.load(Ordering::SeqCst) > 0 || self.awaiting_turn(now_ms())
    }

    /// An act that only queues its turn (`Op::Compact`, a goal resumed) is under way past its
    /// own return: the record stays `busy` until a turn of `thread` starts, the act gives it
    /// up ([`Self::cancel_await`]), or `within` passes. Take it while the act's guard still
    /// stands, BEFORE the act, so a turn that starts before the act returns is seen.
    pub fn await_turn(&self, thread: &str, within: Duration) -> u64 {
        let token = self.next_await.fetch_add(1, Ordering::SeqCst) + 1;
        let within = u64::try_from(within.as_millis()).unwrap_or(u64::MAX);
        lock(&self.awaited).push(AwaitedTurn {
            token,
            thread: thread.to_owned(),
            deadline_ms: now_ms().saturating_add(within),
        });
        self.mark_changed();
        token
    }

    /// The act `token` named failed, or did not go: its turn is no longer awaited.
    pub fn cancel_await(&self, token: u64) {
        let removed = {
            let mut awaited = lock(&self.awaited);
            let before = awaited.len();
            awaited.retain(|a| a.token != token);
            before != awaited.len()
        };
        if removed {
            self.mark_changed();
        }
    }

    /// Whether an act's turn is awaited at `now_ms` ([`Self::await_turn`]).
    pub fn awaiting_turn(&self, now_ms: u64) -> bool {
        lock(&self.awaited).iter().any(|a| a.deadline_ms > now_ms)
    }

    /// When the first awaited turn's wait runs out, if one is awaited (the record writer
    /// looks again then).
    pub fn await_deadline_ms(&self) -> Option<u64> {
        lock(&self.awaited).iter().map(|a| a.deadline_ms).min()
    }

    /// Drop every awaited turn whose wait ran out by `now_ms`; whether any was (a change the
    /// record writer writes).
    pub fn expire_awaited(&self, now_ms: u64) -> bool {
        let removed = {
            let mut awaited = lock(&self.awaited);
            let before = awaited.len();
            awaited.retain(|a| a.deadline_ms > now_ms);
            before != awaited.len()
        };
        if removed {
            self.mark_changed();
        }
        removed
    }

    fn turn_came(&self, thread: &str) -> bool {
        let mut awaited = lock(&self.awaited);
        let before = awaited.len();
        awaited.retain(|a| a.thread != thread);
        before != awaited.len()
    }

    /// A turn of `thread` started (the autopilot's `on_turn_start`). A turn already seen
    /// ending is not counted again. Returns whether it is newly running. A turn newly running,
    /// or one no longer running ([`Self::turn_ended`], [`Self::thread_ended`]), wakes the TUI:
    /// its OSC 7501 report reads a turn of a thread it does not show
    /// ([`Self::turn_running_besides`]), whose events no frame of its own follows.
    pub fn turn_started(&self, thread: &str, turn: &str) -> bool {
        let mut upgrade = lock(&self.upgrade_generation);
        let key = (thread.to_owned(), turn.to_owned());
        let started = {
            let mut book = lock(&self.turns);
            let started = !book.ended.contains(&key) && book.running.insert(key);
            if started {
                book.latest.insert(thread.to_owned(), turn.to_owned());
                let mut recovery = lock(&self.recovery);
                if recovery
                    .as_ref()
                    .is_some_and(|failure| failure.thread == thread)
                {
                    *recovery = None;
                }
                let mut owed = lock(&self.owed_act);
                if owed.as_ref().is_some_and(|act| act.thread == thread) {
                    if let Some(renewal) = owed
                        .as_mut()
                        .and_then(|act| act.renewal.as_mut())
                        .filter(|renewal| renewal.turn.as_deref() == Some(turn))
                    {
                        renewal.accepted = true;
                    } else {
                        *owed = None;
                    }
                }
                let mut retry = lock(&self.in_flight_retry);
                if retry.as_ref().is_some_and(|wait| wait.thread == thread) {
                    *retry = None;
                }
            }
            started
        };
        // Counted as running first, then no longer awaited: the record never reads rest
        // between the two.
        let came = self.turn_came(thread);
        if started || came {
            *upgrade = upgrade.wrapping_add(1);
            self.mark_changed();
        }
        drop(upgrade);
        if started {
            let _ = self.wake();
        }
        started
    }

    /// A turn of `thread` ended (stopped or aborted).
    pub fn turn_ended(&self, thread: &str, turn: &str) {
        let mut upgrade = lock(&self.upgrade_generation);
        let key = (thread.to_owned(), turn.to_owned());
        let removed = {
            let mut book = lock(&self.turns);
            let removed = book.running.remove(&key);
            if !self.leaving_for_relaunch() {
                let mut retry = lock(&self.in_flight_retry);
                if retry
                    .as_ref()
                    .is_some_and(|wait| wait.thread == thread && wait.turn_id == turn)
                {
                    *retry = None;
                }
            }
            if !book.ended.contains(&key) {
                if book.ended.len() >= ENDED_TURNS_KEPT {
                    book.ended.pop_front();
                }
                book.ended.push_back(key);
            }
            removed
        };
        *upgrade = upgrade.wrapping_add(1);
        self.mark_changed();
        drop(upgrade);
        if removed {
            let _ = self.wake();
        }
    }

    /// `thread` stopped: none of its turns runs any more, and none is awaited.
    pub fn thread_ended(&self, thread: &str) {
        let mut upgrade = lock(&self.upgrade_generation);
        let removed = {
            let mut book = lock(&self.turns);
            let before = book.running.len();
            book.running.retain(|(t, _)| t != thread);
            book.stopped_threads.insert(thread.to_owned());
            if !self.leaving_for_relaunch() {
                let mut retry = lock(&self.in_flight_retry);
                if retry.as_ref().is_some_and(|wait| wait.thread == thread) {
                    *retry = None;
                }
            }
            before != book.running.len()
        };
        if removed | self.turn_came(thread) {
            *upgrade = upgrade.wrapping_add(1);
            self.mark_changed();
        }
        drop(upgrade);
        if removed {
            let _ = self.wake();
        }
    }

    /// Whether any turn runs ([`Self::turn_started`]).
    pub fn turn_running(&self) -> bool {
        !lock(&self.turns).running.is_empty()
    }

    /// Whether a turn of `thread` runs ([`Self::turn_started`]): its end, not an older one,
    /// is what the thread stands at next.
    pub fn turn_running_on(&self, thread: &str) -> bool {
        lock(&self.turns).running.iter().any(|(t, _)| t == thread)
    }

    /// Whether a turn of a thread other than `displayed` runs ([`Self::turn_started`]; any
    /// thread's when `None`): the TUI shows another thread, and the OSC 7501 report stays in
    /// a turn while that one rests (`codex_alab_tui::turns`, CONTRACT §6), so a switch away
    /// from a running turn and back begins none. Only the autopilot's threads are booked here;
    /// every other turn the TUI routes is [`Self::turn_seen_besides`].
    pub fn turn_running_besides(&self, displayed: Option<&str>) -> bool {
        lock(&self.turns)
            .running
            .iter()
            .any(|(t, _)| Some(t.as_str()) != displayed)
    }

    /// Whether a turn the TUI saw start ([`Self::turn_seen`]: a sub-agent's, a side thread's,
    /// one under a remote app-server) runs on a thread other than `displayed` (any thread's
    /// when `None`). The OSC 7501 report keeps a turn it stands in while one does
    /// (`codex_alab_tui::turns::Frame::held`), so a switch from a running sub-agent and back
    /// begins none; it never begins one, since no autopilot carries such a turn and a landing
    /// aterm reopened at its begin would be left open with nobody to land it again.
    pub fn turn_seen_besides(&self, displayed: Option<&str>) -> bool {
        lock(&self.seen)
            .iter()
            .any(|(t, _)| Some(t.as_str()) != displayed)
    }

    /// A turn the TUI saw start (`running`) or end on a thread its app-server notifications
    /// are routed for (the `ALAB(turns)` hook in the TUI's routing, before the thread's
    /// store takes it): booked for the OSC 7501 report ([`Self::turn_seen_besides`]),
    /// whichever server runs it, in process or remote, and whatever thread it is (a
    /// sub-agent's, a side thread's, a root's). A change wakes the TUI: another thread's
    /// events draw no frame of their own.
    pub fn turn_seen(&self, thread: &str, turn: &str, running: bool) {
        let key = (thread.to_owned(), turn.to_owned());
        let moved = {
            let mut seen = lock(&self.seen);
            if running {
                seen.insert(key)
            } else {
                seen.remove(&key)
            }
        };
        if moved {
            let _ = self.wake();
        }
    }

    /// The TUI's seen turns kept only where `keep` says so of their thread: `thread/closed`
    /// for one thread, a thread the TUI no longer routes (its notifications, the end among
    /// them, no longer reach the hook), or none at all where the stream lost events (a lag,
    /// a reconnect): a turn whose end may not come is not read as running ([`Self::turn_seen`]).
    /// A change wakes the TUI.
    pub fn turns_seen_keep(&self, keep: impl Fn(&str) -> bool) {
        let moved = {
            let mut seen = lock(&self.seen);
            let before = seen.len();
            seen.retain(|(thread, _)| keep(thread));
            before != seen.len()
        };
        if moved {
            let _ = self.wake();
        }
    }

    /// The TUI's chat widget took `turn/completed` of `turn` for `thread`, live or replayed (the
    /// `ALAB(turns)` hook in its notification arm). The widget takes a thread's notifications
    /// in order and writes a live turn's OSC 7501 begin edge as it takes its `turn/started` (a
    /// replay writes none), so that turn's begin edge, if any, is written by now; the rest word
    /// after it, written at the next frame, begins no turn ([`Self::tui_handled_end`]).
    pub fn turn_handled_end(&self, thread: &str, turn: &str) {
        let key = (thread.to_owned(), turn.to_owned());
        let mut ends = lock(&self.handled_ends);
        if ends.contains(&key) {
            return;
        }
        if ends.len() >= ENDED_TURNS_KEPT {
            ends.pop_front();
        }
        ends.push_back(key);
    }

    /// Whether the TUI's chat widget handled the end of `turn` of `thread`
    /// ([`Self::turn_handled_end`]): aterm reopens a landing at any turn its engine counts
    /// past the one the landing was recorded over, so `landed@v1` waits for this, or a land
    /// turn's begin the TUI writes after it reads as a turn after the landing.
    pub fn tui_handled_end(&self, thread: &str, turn: &str) -> bool {
        lock(&self.handled_ends)
            .iter()
            .any(|(t, u)| t == thread && u == turn)
    }

    /// The turn of `thread` that started last ([`Self::turn_started`]), if any.
    pub fn latest_turn(&self, thread: &str) -> Option<String> {
        lock(&self.turns).latest.get(thread).cloned()
    }

    /// Whether the latest frame showed `thread` ([`View::displayed_thread`]).
    pub fn displays(&self, thread: &str) -> bool {
        lock(&self.view).displayed_thread.as_deref() == Some(thread)
    }

    /// Why the session is not at rest, whatever its frame reads (the record's `idle` is
    /// given only when this is `None`): a turn running, an act being carried out or owed,
    /// a person's turn on its way, or the upgrade. A wait on a timer is rest: the act it
    /// leads to takes [`Self::begin_act`] and writes the record before it goes.
    pub fn unrest(&self) -> Option<Unrest> {
        if self.turn_running() {
            Some(Unrest::TurnRunning)
        } else if self.acting()
            || self
                .assignment_receipt()
                .is_some_and(|receipt| !receipt.acknowledged)
        {
            Some(Unrest::Acting)
        } else if self.box_answer_pending() {
            Some(Unrest::BoxAnswer)
        } else if self.carry_on_owed() {
            Some(Unrest::CarryOnOwed)
        } else if self.pending_user_turn() {
            Some(Unrest::UserTurnPending)
        } else if self.queued_input() {
            Some(Unrest::QueuedInput)
        } else if self.exec_pending() {
            Some(Unrest::Upgrading)
        } else {
            None
        }
    }

    /// The record writer's write-through (set once, by the writer).
    pub fn set_flush(&self, flush: FlushHook) -> bool {
        self.flush.set(flush).is_ok()
    }

    /// Write the session record now, not coalesced (blocking file I/O: call it off the
    /// render and input paths). `true` once the record says what this state says, or when no
    /// record is kept (outside aterm).
    pub fn flush_record(&self) -> bool {
        match self.flush.get() {
            Some(flush) => flush(),
            None => true,
        }
    }

    /// How many times the record's resume line changed ([`Self::note_resume_line`]).
    pub fn resume_lines(&self) -> u64 {
        self.resume_lines.load(Ordering::SeqCst)
    }

    /// Wake whoever waits for a change (the record writer).
    pub fn mark_changed(&self) {
        let mut generation = lock(&self.generation);
        *generation = generation.wrapping_add(1);
        self.changed.notify_all();
    }

    pub fn generation(&self) -> u64 {
        *lock(&self.generation)
    }

    /// Block until the generation differs from `seen` (or `timeout` passes); returns it.
    pub fn wait_change(&self, seen: u64, timeout: Duration) -> u64 {
        let generation = lock(&self.generation);
        let (generation, _) = self
            .changed
            .wait_timeout_while(generation, timeout, |generation| *generation == seen)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *generation
    }
}
