// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The session hooks (`ALAB(session)`) and the record writer (CONTRACT §4).
//!
//! `observe` takes each frame's facts into the shared state (`codex_alab_core::session`) and,
//! inside aterm, starts one background writer that keeps `$CODEX_HOME/alab/sessions/<pid>.json`
//! current: at most one write per second, only when something changed, never on the render or
//! input path. Two changes are written through, not coalesced, because aterm's watch-only
//! worker may end the process at the record's rest (`status:"idle"` with `draftEmpty:true`):
//! leaving rest (`idle` to anything else) and a person's first keystroke (`draftEmpty` to
//! `false`). The writer writes those at once; the autopilot's act and every turn's start write
//! the record themselves before they go (`Shared::flush_record`). `human_input` stamps a
//! person's key, paste or click. The exit path ends the record (`finalize`, called from
//! `life`).

use crate::identity;
use crate::record;
use crate::record::Record;
use crate::record::Status;
use crate::resume_argv;
use codex_alab_core::session::Launch;
use codex_alab_core::session::now_ms;
use codex_alab_core::session::shared;
use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

pub use codex_alab_core::session::Autopilot;
pub use codex_alab_core::session::BlockedKind;
pub use codex_alab_core::session::Frame;
pub use codex_alab_core::session::ProgramState;
pub use codex_alab_core::session::Shared;

/// At most one record write per this interval (CONTRACT §4).
pub const WRITE_INTERVAL: Duration = Duration::from_secs(1);

/// How often an unresolved `resumeArgv` (the rollout not yet on disk) is looked at again.
const RESUME_RECHECK: Duration = Duration::from_secs(1);

/// The codex version this binary reports (`codex --version`: the workspace version).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Whether ALab's hooks act in this process (the codex binary's `main` said so).
pub fn active() -> bool {
    codex_alab_core::active()
}

static WRITER: OnceLock<&'static Writer> = OnceLock::new();

/// ALAB(session) hook: one frame's facts (the first line of `App::render_chat_widget_frame`).
pub fn observe(frame: &Frame<'_>) {
    if !active() {
        return;
    }
    let shared = shared();
    shared.observe(frame, now_ms());
    if WRITER.get().is_none()
        && let Some(launch) = shared.launch()
        && inside_aterm(launch)
    {
        let dir = record::sessions_dir(frame.codex_home);
        let _ = WRITER.get_or_init(|| {
            let writer: &'static Writer = Box::leak(Box::new(Writer::new(
                shared,
                dir,
                WRITE_INTERVAL,
                resume_argv::command(),
            )));
            writer.spawn();
            writer.install_write_through();
            writer
        });
    }
}

/// ALAB(session) hook: a key (not a release), a non-empty paste or a click reached the TUI.
pub fn human_input() {
    if active() {
        shared().human_input(now_ms());
    }
}

/// ALAB(session) hook, each TUI event: a whole-screen overlay (the transcript, a diff, a
/// backtrack preview) is up, or not. The TUI draws no chat frame under one.
pub fn overlay_open(open: bool) {
    if active() {
        shared().set_overlay_open(open);
    }
}

/// The record is written only inside aterm (`ATERM_PARENT_SESSION_ID` set at launch).
pub fn inside_aterm(launch: &Launch) -> bool {
    launch
        .var("ATERM_PARENT_SESSION_ID")
        .is_some_and(|value| !value.is_empty())
}

/// How the record ends with the process (CONTRACT §4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ending {
    /// An orderly exit (`/exit`, `/quit`, archive): gone.
    Remove,
    /// A signal (TERM, HUP, INT): kept as the witness with `exit:"signal:<n>"` and the status
    /// it stood at when the signal came (`busy` stays `busy`), so a relaunch knows what was
    /// owed. SIGHUP is the terminal going away (a closed tab, or aterm's own crash), never a
    /// person's `/exit`: kept, so the codex-alab aterm restores carries on.
    Signalled(i32),
    /// Kept as the witness: `status:"exited"` with this `exit` (`fatal`).
    Exited(String),
    /// The live upgrade: kept, `status:"upgrading"`, `execGeneration+1`; the new image rewrites it.
    Upgrading,
}

impl Ending {
    pub fn of(signal: Option<i32>, exec_pending: bool, fatal: bool) -> Self {
        match signal {
            Some(signal) => Self::Signalled(signal),
            None if fatal => Self::Exited("fatal".to_owned()),
            None if exec_pending => Self::Upgrading,
            None => Self::Remove,
        }
    }
}

/// ALAB(life), as a signal arrives and before the `/exit` teardown it starts: the record's
/// status stands as it is now (the teardown aborts a running turn, which must not turn a
/// busy session's witness into an idle one).
pub fn hold_status_for_signal() {
    if let Some(writer) = WRITER.get() {
        writer.hold_status();
    }
}

/// End this process's record, if a writer keeps one. Synchronous: the process is leaving.
pub fn finalize(ending: &Ending) {
    if let Some(writer) = WRITER.get() {
        let _ = writer.finalize(ending);
    }
}

/// The thread a `resumeArgv` was rebuilt for, and whether its rollout was on disk then.
type ResumeKey = (String, bool);

#[derive(Default)]
struct WriterState {
    last: Option<Record>,
    finalized: bool,
    prepared: bool,
    exec_generation: u64,
    /// The status a signal found ([`Writer::hold_status`]): every later record says it.
    held_status: Option<Status>,
    /// (primary thread, rollout materialized) → the rebuilt argv.
    resume: Option<(ResumeKey, Option<Vec<String>>)>,
}

/// Keeps one process's record current.
pub struct Writer {
    shared: &'static Shared,
    dir: PathBuf,
    interval: Duration,
    command: Option<fn() -> clap::Command>,
    state: Mutex<WriterState>,
    writes: AtomicU64,
}

impl Writer {
    pub fn new(
        shared: &'static Shared,
        dir: PathBuf,
        interval: Duration,
        command: Option<fn() -> clap::Command>,
    ) -> Self {
        Self {
            shared,
            dir,
            interval,
            command,
            state: Mutex::new(WriterState::default()),
            writes: AtomicU64::new(0),
        }
    }

    fn lock(&self) -> MutexGuard<'_, WriterState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn pid(&self) -> u32 {
        self.shared
            .launch()
            .map_or_else(std::process::id, |launch| launch.pid)
    }

    pub fn path(&self) -> PathBuf {
        record::path_for(&self.dir, self.pid())
    }

    /// How many records this writer has written.
    pub fn writes(&self) -> u64 {
        self.writes.load(Ordering::SeqCst)
    }

    /// A signal came: from now on the record keeps the status it has now.
    pub fn hold_status(&self) {
        let mut state = self.lock();
        if state.finalized || state.held_status.is_some() {
            return;
        }
        let status = self.build(&mut state, now_ms()).status;
        state.held_status = Some(status);
    }

    /// Hand the shared state this writer's write-through ([`Shared::flush_record`]).
    pub fn install_write_through(&'static self) {
        let _ = self
            .shared
            .set_flush(Box::new(move || self.write_now().is_ok()));
    }

    /// Whether the record's next write leaves rest or takes a person's first keystroke
    /// (`idle` to anything else, `draftEmpty` to `false`): such a write is not coalesced.
    pub fn urgent(&self) -> bool {
        let mut state = self.lock();
        if state.finalized {
            return false;
        }
        let next = self.build(&mut state, now_ms());
        state
            .last
            .as_ref()
            .is_none_or(|last| leaves_rest(last, &next))
    }

    /// Start the background task.
    pub fn spawn(&'static self) {
        let _ = std::thread::Builder::new()
            .name("alab-session-record".to_owned())
            .spawn(move || self.run());
    }

    /// Once, before the first write: the directory, the previous image's generation (a
    /// re-exec keeps the pid and the identity), and the sweep of dead identities' records.
    pub fn prepare(&self) {
        let mut state = self.lock();
        if state.prepared {
            return;
        }
        state.prepared = true;
        let _ = record::ensure_dir(&self.dir);
        let launch = self.shared.launch();
        let own = launch.and_then(|launch| launch.identity.as_ref());
        if let Some(previous) = record::read(&self.path())
            && let (Some(own), Some(theirs)) = (own, previous.identity.clone())
            && codex_alab_core::session::ProcessIdentity::from(theirs) == *own
        {
            state.exec_generation = previous.exec_generation;
            // The chain survives the live upgrade's execve: a 0 here would let a later turn
            // nobody asked for start a fresh chain (review of cf084e3b7, finding 4).
            if previous.carried_on > self.shared.carried_on() {
                self.shared.set_carried_on(previous.carried_on);
            }
            self.shared
                .set_inherited(codex_alab_core::session::Inherited {
                    exec_generation: previous.exec_generation,
                    status: serde_json::to_value(previous.status)
                        .ok()
                        .and_then(|status| status.as_str().map(str::to_owned)),
                    session_id: previous.session_id,
                });
        }
        let started = launch.map_or_else(now_ms, |launch| launch.started_at_ms);
        let own_argv = launch.map_or(&[][..], |launch| launch.args.as_slice());
        let horizon = u64::try_from(codex_alab_core::session::RELAUNCH_HORIZON.as_millis())
            .unwrap_or(u64::MAX);
        let shared = self.shared;
        // Only this process's own predecessor is taken: another tab's dead record stands for
        // that tab's relaunch (and keeps its package from the pruner) until it takes it.
        record::sweep_with(
            &self.dir,
            self.pid(),
            started,
            identity::is_live,
            |swept| record::judge(swept, own_argv, started, horizon),
            |pid, swept| {
                // The chain is this process's from its first record on: a relaunch that dies
                // before its carry-on reads the predecessor (`relaunched`, after the launch
                // probe's waits) must not hand the next one chain 0 (review of 363789134,
                // finding 2). Raised only: a person's later turn or a done still resets it.
                if swept.carried_on > shared.carried_on() {
                    shared.set_carried_on(swept.carried_on);
                }
                if let Some(session_id) = &swept.session_id {
                    shared.note_predecessor(codex_alab_core::session::Predecessor {
                        pid,
                        session_id: session_id.clone(),
                        exit: swept.exit.clone(),
                        resume_argv: swept.resume_argv.clone(),
                        status: serde_json::to_value(swept.status)
                            .ok()
                            .and_then(|status| status.as_str().map(str::to_owned)),
                        updated_at_ms: swept.updated_at,
                        carried_on: swept.carried_on,
                    });
                }
            },
        );
        // The autopilot's carry-on waits for this (the crash witnesses are known now).
        shared.set_swept();
    }

    fn run(&self) {
        self.prepare();
        let mut seen = 0;
        let mut last_write: Option<Instant> = None;
        loop {
            if self.lock().finalized {
                return;
            }
            let generation = self.shared.generation();
            let recheck = self.resume_pending();
            if generation == seen && !recheck {
                self.shared.wait_change(seen, self.idle_wait());
                // An awaited act's turn whose wait ran out is a change: the record may rest.
                self.shared.expire_awaited(now_ms());
                continue;
            }
            if generation == seen {
                self.shared.wait_change(seen, RESUME_RECHECK);
            }
            if let Some(at) = last_write
                && !self.urgent()
            {
                // Coalesce, but wake early for a change that may not wait.
                let next = at + self.interval;
                let mut seen_now = self.shared.generation();
                loop {
                    let now = Instant::now();
                    if now >= next {
                        break;
                    }
                    let moved = self.shared.wait_change(seen_now, next - now);
                    if moved != seen_now {
                        seen_now = moved;
                        if self.urgent() {
                            break;
                        }
                    }
                }
            }
            seen = self.shared.generation();
            if let Ok(true) = self.write_now() {
                last_write = Some(Instant::now());
            }
        }
    }

    /// How long the writer waits for a change when nothing is pending: until the first
    /// awaited act's turn runs out of time ([`Shared::await_turn`]), at most an hour.
    fn idle_wait(&self) -> Duration {
        let hour = Duration::from_secs(3600);
        self.shared.await_deadline_ms().map_or(hour, |deadline| {
            Duration::from_millis(deadline.saturating_sub(now_ms()).saturating_add(1)).min(hour)
        })
    }

    /// Whether the record still lacks a `resumeArgv` it could get once the rollout exists.
    fn resume_pending(&self) -> bool {
        let state = self.lock();
        !state.finalized
            && state.last.as_ref().is_some_and(|last| {
                last.session_id.is_some() && last.resume_argv.is_none() && self.command.is_some()
            })
            && self
                .shared
                .view()
                .primary()
                .is_some_and(|facts| facts.rollout_path.is_some())
    }

    /// Write the record now if it changed; `Ok(true)` when a file was written.
    pub fn write_now(&self) -> std::io::Result<bool> {
        let mut state = self.lock();
        if state.finalized {
            return Ok(false);
        }
        let record = self.build(&mut state, now_ms());
        if state
            .last
            .as_ref()
            .is_some_and(|last| last.same_content(&record))
        {
            return Ok(false);
        }
        record::write_atomic(&self.dir, &record)?;
        self.writes.fetch_add(1, Ordering::SeqCst);
        let line_moved = record.resume_argv.is_some()
            && state.last.as_ref().map(|last| &last.resume_argv) != Some(&record.resume_argv);
        state.last = Some(record);
        drop(state);
        if line_moved {
            // aterm read the record at the claim; a line it has not seen is renewed into its
            // copy by the claim's re-confirm (the supervisor watches this).
            self.shared.note_resume_line();
        }
        Ok(true)
    }

    /// End the record (the exit path); nothing is written after this.
    pub fn finalize(&self, ending: &Ending) -> std::io::Result<()> {
        let mut state = self.lock();
        if state.finalized {
            return Ok(());
        }
        state.finalized = true;
        self.shared.mark_changed();
        match ending {
            Ending::Remove => record::remove(&self.dir, self.pid()),
            Ending::Signalled(signal) => {
                if state.held_status.is_none() {
                    state.held_status = Some(self.build(&mut state, now_ms()).status);
                }
                let mut record = self.build(&mut state, now_ms());
                record.exit = Some(format!("signal:{signal}"));
                record::write_atomic(&self.dir, &record)
            }
            Ending::Exited(exit) => {
                let mut record = self.build(&mut state, now_ms());
                record.status = Status::Exited;
                record.exit = Some(exit.clone());
                record::write_atomic(&self.dir, &record)
            }
            Ending::Upgrading => {
                let mut record = self.build(&mut state, now_ms());
                record.status = Status::Upgrading;
                record.exec_generation += 1;
                record::write_atomic(&self.dir, &record)
            }
        }
    }

    fn build(&self, state: &mut WriterState, now: u64) -> Record {
        let shared = self.shared;
        let launch = shared.launch();
        let view = shared.view();
        let (mut status, state_word, blocked_kind) = status_of(view.state);
        // `idle` only at true rest: no turn running, no act being carried out or owed, no
        // person's turn on its way (CONTRACT §4: aterm may end the process at `idle`).
        if status == Status::Idle && shared.unrest().is_some() {
            status = Status::Busy;
        }
        if let Some(held) = state.held_status {
            status = held;
        }
        let primary = view.primary_thread.clone();
        let materialized = view
            .primary()
            .and_then(|facts| facts.rollout_path.as_deref())
            .is_some_and(rollout_materialized);
        let resume_argv = match (&primary, launch) {
            (Some(thread), Some(launch)) => {
                let key = (thread.clone(), materialized);
                match &state.resume {
                    Some((cached, argv)) if *cached == key => argv.clone(),
                    _ => {
                        let argv = materialized
                            .then(|| self.resume_argv(launch, thread))
                            .flatten();
                        state.resume = Some((key, argv.clone()));
                        argv
                    }
                }
            }
            _ => None,
        };
        let mut env = BTreeMap::new();
        if let Some(home) = launch
            .and_then(|launch| launch.var("CODEX_HOME"))
            .and_then(|home| home.to_str())
        {
            env.insert("CODEX_HOME".to_owned(), home.to_owned());
        }
        let path_string = |path: Option<&Path>| path.and_then(Path::to_str).map(str::to_owned);
        Record {
            v: record::SCHEMA_VERSION,
            pid: self.pid(),
            proc_start: launch.and_then(|launch| launch.proc_start.clone()),
            identity: launch
                .and_then(|launch| launch.identity.as_ref())
                .map(record::Identity::from),
            session_id: primary,
            displayed_thread_id: view.displayed_thread.clone(),
            cwd: path_string(view.cwd.as_deref()),
            version: VERSION.to_owned(),
            exe: path_string(launch.and_then(|launch| launch.current_exe.as_deref())),
            launch_argv0: path_string(launch.and_then(|launch| launch.argv0.as_deref())),
            exec_generation: state.exec_generation,
            resume_argv,
            env,
            status,
            exit: None,
            state: state_word,
            blocked_kind,
            draft_empty: shared.record_draft_empty(),
            last_human_input_at: shared.last_human_input_ms(),
            autopilot: match shared.autopilot() {
                codex_alab_core::session::Autopilot::Off => record::Autopilot::Off,
                codex_alab_core::session::Autopilot::Passive => record::Autopilot::Passive,
                codex_alab_core::session::Autopilot::Active => record::Autopilot::Active,
            },
            claim: shared.claim(),
            status_updated_at: view.state_since_ms,
            updated_at: now,
            kind: "interactive".to_owned(),
            entrypoint: "cli".to_owned(),
            carried_on: shared.carried_on(),
        }
    }

    fn resume_argv(&self, launch: &Launch, thread: &str) -> Option<Vec<String>> {
        let command = self.command?;
        let argv0 = launch.argv0.as_deref()?.to_str()?;
        resume_argv::rebuild(command(), &launch.args, argv0, thread)
    }
}

/// Whether `next` leaves the rest `last` stood at: `idle` becomes anything else, or an empty
/// draft becomes a person's (CONTRACT §4: these are written through).
pub fn leaves_rest(last: &Record, next: &Record) -> bool {
    (last.status == Status::Idle && next.status != Status::Idle)
        || (last.draft_empty && !next.draft_empty)
}

/// A fresh thread's rollout is written only from its first user message on; until then
/// `codex resume <id>` cannot find it (`resumable_thread` needs a non-empty file).
pub(crate) fn rollout_materialized(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0)
}

fn status_of(state: ProgramState) -> (Status, Option<record::State>, Option<record::BlockedKind>) {
    match state {
        ProgramState::Unknown => (Status::Busy, None, None),
        ProgramState::Idle => (Status::Idle, Some(record::State::Idle), None),
        ProgramState::Working => (Status::Busy, Some(record::State::Working), None),
        ProgramState::Blocked(kind) => (
            Status::Waiting,
            Some(record::State::Blocked),
            Some(match kind {
                BlockedKind::Permission => record::BlockedKind::Permission,
                BlockedKind::Question => record::BlockedKind::Question,
                BlockedKind::Auth => record::BlockedKind::Auth,
            }),
        ),
        ProgramState::Done => (Status::Idle, Some(record::State::Done), None),
        ProgramState::Error => (Status::Idle, Some(record::State::Error), None),
    }
}
