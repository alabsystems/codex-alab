// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The one in-process state codex-alab's pieces share (CONTRACT §4-§5).
//!
//! The TUI hooks (`codex-rs/alab-tui`) write it: the launch snapshot taken first thing in
//! `main`, what each frame shows (threads, cwd, model, program state, the draft and the
//! person's queued input), the human-input stamp, and the exit's signal or exec request.
//! The session-record writer and, later, the autopilot read it. It is one `static` in one
//! crate, so the TUI and the in-process app-server extension see the same copy (tui-life §6).
//! Plain `std` only: no codex crate, no I/O.

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
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

/// Upstream's line between an `/ide` context and the person's request
/// (`tui/src/ide_context/prompt.rs`, `PROMPT_REQUEST_BEGIN`).
const PROMPT_REQUEST_MARKER: &str = "\n## My request for Codex:\n";

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
    pub pid: u32,
    pub identity: Option<ProcessIdentity>,
    /// The start time in `ps -o lstart` form, as Claude Code's records carry it.
    pub proc_start: Option<String>,
    pub started_at_ms: u64,
}

impl Launch {
    /// A launch-time environment variable.
    pub fn var(&self, name: &str) -> Option<&OsStr> {
        self.vars
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_os_str())
    }
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

/// What the previous image of this pid left in its record (a re-exec), read once at start.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inherited {
    pub exec_generation: u64,
    pub status: Option<String>,
    pub session_id: Option<String>,
}

/// How long a dead process's record can still be a relaunch aterm owes: one written longer
/// before a start is a person's later resume, never aterm's relaunch (the carry-on's window),
/// and the start-up sweep removes it. A younger one is left for the process it names.
pub const RELAUNCH_HORIZON: Duration = Duration::from_secs(24 * 3600);

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
    /// person's turn and no done since ([`Shared::set_carried_on`]); 0 for a record that has
    /// none.
    pub carried_on: u32,
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
            Some(raw) => raw
                .parse::<u32>()
                .ok()
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

/// How many ended turns are remembered, so a late start of one is not counted as running.
const ENDED_TURNS_KEPT: usize = 64;

/// The turns that run now, by `(thread, turn)` (the autopilot's lifecycle hooks keep it).
#[derive(Default)]
struct TurnBook {
    running: std::collections::BTreeSet<(String, String)>,
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
    autopilot: AtomicU8,
    claim: Mutex<Option<String>>,
    boxes: Mutex<BoxSlot>,
    waker: OnceLock<Callback>,
    leave: OnceLock<Callback>,
    own_turns: Mutex<Vec<String>>,
    preapprovals: Mutex<Vec<crate::boxes::Preapproval>>,
    resume_lines: AtomicU64,
    carry_on_owed: AtomicBool,
    carried_on: AtomicU32,
    relaunch_spent: AtomicBool,
    acts: AtomicU64,
    awaited: Mutex<Vec<AwaitedTurn>>,
    next_await: AtomicU64,
    turns: Mutex<TurnBook>,
    keyed_since_frame: AtomicBool,
    flush: OnceLock<FlushHook>,
    generation: Mutex<u64>,
    changed: Condvar,
}

/// How long a posted box answer may wait for the TUI's frame to apply it.
pub const BOX_ANSWER_TTL: Duration = Duration::from_secs(10);

/// A TUI action handed to the autopilot (wake a frame, leave like `/exit`).
pub type Callback = Box<dyn Fn() + Send + Sync>;

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
    pub const fn new() -> Self {
        Self {
            launch: OnceLock::new(),
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
            autopilot: AtomicU8::new(0),
            claim: Mutex::new(None),
            boxes: Mutex::new(BoxSlot {
                open: None,
                seen: false,
                pending: None,
                pending_ms: 0,
                outcome: None,
            }),
            waker: OnceLock::new(),
            leave: OnceLock::new(),
            own_turns: Mutex::new(Vec::new()),
            preapprovals: Mutex::new(Vec::new()),
            resume_lines: AtomicU64::new(0),
            carry_on_owed: AtomicBool::new(false),
            carried_on: AtomicU32::new(0),
            relaunch_spent: AtomicBool::new(false),
            awaited: Mutex::new(Vec::new()),
            next_await: AtomicU64::new(0),
            acts: AtomicU64::new(0),
            turns: Mutex::new(TurnBook {
                running: std::collections::BTreeSet::new(),
                ended: std::collections::VecDeque::new(),
            }),
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

    /// The swept record that named `session_id`, if any (the last written of them).
    pub fn predecessor(&self, session_id: &str) -> Option<Predecessor> {
        lock(&self.predecessors)
            .iter()
            .filter(|p| p.session_id == session_id)
            .max_by_key(|p| p.updated_at_ms)
            .cloned()
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
            self.mark_changed();
        }
        changed
    }

    /// A copy of the latest frame's facts.
    pub fn view(&self) -> View {
        lock(&self.view).clone()
    }

    /// Whether the TUI registered this thread (only those are the autopilot's).
    pub fn is_registered(&self, thread: &str) -> bool {
        lock(&self.view).threads.contains_key(thread)
    }

    /// The autopilot keeps this thread: a root thread of the TUI's own (never a sub-agent's,
    /// exec's or an app-server client's).
    pub fn note_root_thread(&self, thread: &str) {
        let mut roots = lock(&self.roots);
        if !roots.iter().any(|t| t == thread) {
            roots.push(thread.to_owned());
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
        self.human_input_ms.store(now_ms, Ordering::SeqCst);
        self.keyed_since_frame.store(true, Ordering::SeqCst);
        self.mark_changed();
    }

    /// `draftEmpty` as the record says it: the composer was empty at the last frame AND no
    /// key, paste or click came after that frame. The before-act checks read
    /// [`Self::draft_empty`] (the human grace already holds them after a key).
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
        if self.overlay_open.swap(open, Ordering::SeqCst) != open {
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
        self.signal
            .compare_exchange(0, signal, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// The signal that ended this process, once one arrived.
    pub fn signal(&self) -> Option<i32> {
        Some(self.signal.load(Ordering::SeqCst)).filter(|signal| *signal != 0)
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
        let value = match autopilot {
            Autopilot::Off => 0,
            Autopilot::Passive => 1,
            Autopilot::Active => 2,
        };
        if self.autopilot.swap(value, Ordering::SeqCst) != value {
            self.mark_changed();
            let _ = self.wake();
        }
    }

    pub fn autopilot(&self) -> Autopilot {
        match self.autopilot.load(Ordering::SeqCst) {
            1 => Autopilot::Passive,
            2 => Autopilot::Active,
            _ => Autopilot::Off,
        }
    }

    /// The supervisor claim codex-alab holds now (`codex-alab@<pid>`), if any.
    pub fn set_claim(&self, claim: Option<String>) {
        let mut slot = lock(&self.claim);
        if *slot != claim {
            *slot = claim;
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
                    crate::boxes::BoxKind::Plan { .. } | crate::boxes::BoxKind::Approval { .. } => {
                        true
                    }
                }
        });
        if same {
            slot.seen = true;
        }
        same
    }

    /// The box glue reports the box it draws (its `opened_ms` is kept while its id is).
    pub fn report_box(&self, mut open: OpenBox) {
        let mut slot = lock(&self.boxes);
        slot.seen = true;
        if let Some(was) = &slot.open
            && was.id == open.id
        {
            open.opened_ms = was.opened_ms;
            if *was == open {
                return;
            }
        } else if slot.pending.as_ref().is_some_and(|p| p.id() != open.id) {
            slot.pending = None;
        }
        slot.open = Some(open);
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
            drop(slot);
            self.mark_changed();
        }
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
    /// `n`th in a row with no person's turn and no done between (its predecessor's count plus
    /// one), or it declined one and kept its predecessor's count; 0 again once a person's turn
    /// starts or a turn ends done. A relaunch whose predecessor's record says one or more gets
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
    /// ending is not counted again. Returns whether it is newly running.
    pub fn turn_started(&self, thread: &str, turn: &str) -> bool {
        let key = (thread.to_owned(), turn.to_owned());
        let started = {
            let mut book = lock(&self.turns);
            !book.ended.contains(&key) && book.running.insert(key)
        };
        // Counted as running first, then no longer awaited: the record never reads rest
        // between the two.
        let came = self.turn_came(thread);
        if started || came {
            self.mark_changed();
        }
        started
    }

    /// A turn of `thread` ended (stopped or aborted).
    pub fn turn_ended(&self, thread: &str, turn: &str) {
        let key = (thread.to_owned(), turn.to_owned());
        {
            let mut book = lock(&self.turns);
            book.running.remove(&key);
            if !book.ended.contains(&key) {
                if book.ended.len() >= ENDED_TURNS_KEPT {
                    book.ended.pop_front();
                }
                book.ended.push_back(key);
            }
        }
        self.mark_changed();
    }

    /// `thread` stopped: none of its turns runs any more, and none is awaited.
    pub fn thread_ended(&self, thread: &str) {
        let removed = {
            let mut book = lock(&self.turns);
            let before = book.running.len();
            book.running.retain(|(t, _)| t != thread);
            before != book.running.len()
        };
        if removed | self.turn_came(thread) {
            self.mark_changed();
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

    /// Why the session is not at rest, whatever its frame reads (the record's `idle` is
    /// given only when this is `None`): a turn running, an act being carried out or owed,
    /// a person's turn on its way, or the upgrade. A wait on a timer is rest: the act it
    /// leads to takes [`Self::begin_act`] and writes the record before it goes.
    pub fn unrest(&self) -> Option<Unrest> {
        if self.turn_running() {
            Some(Unrest::TurnRunning)
        } else if self.acting() {
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
