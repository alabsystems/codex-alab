// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The session hooks (`ALAB(session)`) and the record writer (CONTRACT §4).
//!
//! `start` initializes the background sweep before native restore; `observe` takes each
//! frame's facts into shared state. Inside aterm, one writer keeps `$CODEX_HOME/alab/sessions/<pid>.json`
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
pub use codex_alab_core::session::shared;
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
pub use codex_alab_core::session::HarnessWord;
pub use codex_alab_core::session::ProgramState;
pub use codex_alab_core::session::Shared;

/// At most one record write per this interval (CONTRACT §4).
pub const WRITE_INTERVAL: Duration = Duration::from_secs(1);

/// How often an unresolved `resumeArgv` (the rollout not yet on disk) is looked at again.
const RESUME_RECHECK: Duration = Duration::from_secs(1);

/// The full version captured by this executable as codex-alab reports itself (`codex
/// --version`, Decision 8): never the client version OpenAI's servers see.
pub fn version() -> &'static str {
    codex_alab_core::version::product_version(env!("CARGO_PKG_VERSION"))
}

/// Managed-process bookkeeping stays on in vanilla mode: a mode switch must not
/// leave an idle record behind while a person writes a draft or continues a turn.
/// Rendering and automatic actions use `codex_alab_core::active` separately.
pub fn active() -> bool {
    codex_alab_core::is_alab_binary()
}

static WRITER: OnceLock<&'static Writer> = OnceLock::new();

/// Start the sweep from the effective TUI configuration, before native restore needs it.
pub fn start(current: &'static Shared, home: &Path) -> bool {
    if !active() || !std::ptr::eq(current, shared()) {
        current.set_swept();
        return false;
    }
    start_with(current, home, &WRITER)
}

pub(crate) fn start_with(
    shared: &'static Shared,
    home: &Path,
    slot: &OnceLock<&'static Writer>,
) -> bool {
    if !shared.launch().is_some_and(inside_aterm) {
        shared.set_swept();
        return false;
    }
    let dir = record::sessions_dir(home);
    let writer = slot.get_or_init(|| {
        let writer: &'static Writer = Box::leak(Box::new(Writer::new(
            shared,
            dir.clone(),
            WRITE_INTERVAL,
            resume_argv::command(),
        )));
        writer.spawn();
        writer.install_write_through();
        writer
    });
    if !std::ptr::eq(shared, writer.shared) {
        shared.set_swept();
        return false;
    }
    // The admitted writer owns this Shared's readiness. A later mismatched home must
    // neither replace it nor make its still-pending sweep appear complete.
    writer.dir == dir
}

/// ALAB(session) hook: one frame's facts (the first line of `App::render_chat_widget_frame`).
pub fn observe(frame: &Frame<'_>) {
    if !active() {
        return;
    }
    let shared = shared();
    shared.observe(frame, now_ms());
    start(shared, frame.codex_home);
}

/// ALAB(session) hook: record or clear `thread`'s active goal objective (`None` when paused,
/// stopped, complete or cleared).
pub fn set_goal(thread: &str, goal: Option<&str>) {
    if active() {
        shared().set_goal(thread, goal);
    }
}

/// ALAB(session) hook: the background terminals the displayed chat widget's footer counts,
/// handed over at each frame before [`observe`].
pub fn set_background_terminals(count: u32) {
    if active() {
        shared().set_background_terminals(count, Instant::now());
    }
}

/// ALAB(session) hook: a key (not a release), a non-empty paste or a mouse event other than a
/// move reached the TUI. A key, a paste or a button press `may_edit` the composer, so the
/// record's `draftEmpty` reads `false` until the next frame; a wheel, a drag or a release only
/// stamps the human grace ([`Shared::human_attention`]).
pub fn human_input(may_edit: bool) {
    if active() {
        if may_edit {
            shared().human_input(now_ms());
        } else {
            shared().human_attention(now_ms());
        }
    }
}

/// ALAB(session) hook: the TUI is about to start a thread that carries on `source`'s work
/// (plan mode's "clear context and implement", a provider error's continuation), so the
/// autopilot takes it past its session's opening ([`Shared::continue_work`]; edges-3).
pub fn continues_work(source: Option<impl std::fmt::Display>) {
    if let Some(source) = source.filter(|_| active()) {
        shared().continue_work(&source.to_string());
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

fn recovery_source(source: &Record, retry: &record::Recovery) -> bool {
    source.session_id.as_deref() == Some(&retry.thread)
        && source.recovery.as_ref().is_some_and(|original| {
            original.thread == retry.thread && original.turn_id == retry.turn_id
        })
}

fn in_flight_source(source: &Record, retry: &record::InFlightRetry) -> bool {
    source.session_id.as_deref() == Some(&retry.thread)
        && source.in_flight_retry.as_ref().is_some_and(|original| {
            original.thread == retry.thread && original.turn_id == retry.turn_id
        })
}

fn owed_source(source: &Record, owed: &record::Owed) -> bool {
    source.session_id.as_deref() == Some(&owed.thread)
        && source.owed.as_ref().is_some_and(|original| {
            let mut rounded = original.clone();
            rounded.not_before = owed.not_before;
            original.not_before <= owed.not_before && rounded == *owed
        })
}

/// A remote native receipt is only custody here. The host still has to prove
/// Committed, the actual successor, current profile and admission authority.
fn native_pending_owed(source: &Record) -> Option<&record::Owed> {
    let owed = source.owed.as_ref()?;
    let (renewal, purpose) = owed.handoff()?;
    let origin = owed.native_origin.as_ref()?;
    (source.session_id.as_deref() == Some(owed.thread.as_str())
        && owed.rule.as_deref() == Some(purpose.rule())
        && renewal.openai_scope
        && !renewal.local
        && renewal.receipt.is_some()
        && renewal.starts > 0
        && origin.phase == record::NativePhase::Complete)
        .then_some(owed)
}

/// Accepted same-ID work advances `turn` and `accepted`; it has not consumed
/// its receipt yet. Everything binding original custody must remain unchanged.
fn same_native_custody(source: &Record, current: &record::Owed) -> bool {
    let Some(original) = native_pending_owed(source) else {
        return false;
    };
    let (Some((old, old_purpose)), Some((now, purpose))) = (original.handoff(), current.handoff())
    else {
        return false;
    };
    original.thread == current.thread
        && original.native_origin == current.native_origin
        && purpose == old_purpose
        && current.rule.as_deref() == Some(purpose.rule())
        && !current.landed
        && (!now.accepted || now.turn.is_some())
        && old.openai_scope == now.openai_scope
        && old.local == now.local
        && old.account == now.account
        && old.user == now.user
        && old.destination == now.destination
        && old.receipt == now.receipt
        && old.starts == now.starts
        && old.goal_id == now.goal_id
        && (!old.accepted || now.accepted)
        && now.turn.as_ref().is_none_or(|turn| {
            original
                .native_origin
                .as_ref()
                .is_some_and(|origin| turn == &origin.turn_id)
        })
}

#[derive(Default)]
struct WriterState {
    last: Option<Record>,
    finalized: bool,
    prepared: bool,
    start_failed: bool,
    exec_generation: u64,
    /// The status a signal found ([`Writer::hold_status`]): every later record says it.
    held_status: Option<Status>,
    /// (primary thread, rollout materialized) → the rebuilt argv.
    resume: Option<(ResumeKey, Option<Vec<String>>)>,
    /// The unique newest dead-record source; emitting its fields needs launch proof.
    pending_handoff: Option<Record>,
    /// A same-identity predecessor is already authoritative, including before a frame.
    inherited_handoff: Option<Record>,
    /// Sources remain on disk through an initializer or failed successor write.
    held_sources: Vec<Record>,
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
        self.prepare();
        let mut state = self.lock();
        if state.start_failed || state.finalized || state.held_status.is_some() {
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
    /// (`idle` to anything else, `draftEmpty` to `false`), moves the act owed or the pace, or
    /// settles a handoff (it commits the concrete replacement that retires the predecessors'
    /// records this start holds): such a write is not coalesced ([`written_through`]).
    pub fn urgent(&self) -> bool {
        let mut state = self.lock();
        if state.finalized {
            return false;
        }
        let next = self.build(&mut state, now_ms());
        let settles = state
            .held_sources
            .iter()
            .any(|source| !self.retain_native_source(&state, source))
            && (next.resume_argv.is_some() || !record::relaunch_owed(&next));
        settles
            || state
                .last
                .as_ref()
                .is_none_or(|last| written_through(last, &next))
    }

    /// Whether `source` was written in this process's own aterm session
    /// ([`codex_alab_core::session::same_aterm_session`]): its `pace` is this start's to carry
    /// on, as aterm's loop takes up the tab's books, with no relaunch proof.
    fn same_session(&self, source: &Record) -> bool {
        self.shared.launch().is_some_and(|launch| {
            codex_alab_core::session::same_aterm_session(source.aterm_session.as_deref(), launch)
        })
    }

    fn native_custody_source(&self, source: &Record) -> bool {
        native_pending_owed(source).is_some()
            && source.identity.is_some()
            && self.same_session(source)
            && self.shared.launch().is_some_and(|launch| {
                let horizon = u64::try_from(codex_alab_core::session::RELAUNCH_HORIZON.as_millis())
                    .unwrap_or(u64::MAX);
                source.pid != launch.pid
                    && source.updated_at < launch.started_at_ms
                    && launch.started_at_ms.saturating_sub(source.updated_at) <= horizon
                    && source.resume_argv.as_ref().is_some_and(|argv| {
                        argv.len() == launch.args.len()
                            && argv
                                .iter()
                                .zip(&launch.args)
                                .all(|(want, got)| got.to_str() == Some(want.as_str()))
                    })
            })
    }

    fn native_owed_handoff(&self, state: &WriterState, owed: &record::Owed) -> bool {
        state.pending_handoff.as_ref().is_some_and(|source| {
            self.native_custody_source(source) && same_native_custody(source, owed)
        })
    }

    fn retain_native_source(&self, state: &WriterState, source: &Record) -> bool {
        if !self.native_custody_source(source) {
            return false;
        }
        if self
            .shared
            .owed_act()
            .map(record::Owed::from)
            .as_ref()
            .is_some_and(|owed| same_native_custody(source, owed))
        {
            return true;
        }
        // A selected carrier's absent/mismatched marker is an explicit clear,
        // Consume acknowledgment or superseding work, never ordinary timer None.
        if state
            .pending_handoff
            .as_ref()
            .and_then(|pending| pending.owed.as_ref())
            .is_some_and(|selected| same_native_custody(source, selected))
        {
            return false;
        }
        // Ambiguous/unselected native records are not ours to erase without
        // actual newer work. In particular, absence cannot manufacture intent.
        let Some(owed) = native_pending_owed(source) else {
            return false;
        };
        self.shared.latest_turn(&owed.thread).is_none()
    }

    /// Start the background task.
    pub fn spawn(&'static self) {
        self.spawn_with(|writer| {
            std::thread::Builder::new()
                .name("alab-session-record".to_owned())
                .spawn(move || writer.run())
                .map(|_| ())
        });
    }

    pub(crate) fn spawn_with(
        &'static self,
        spawn: impl FnOnce(&'static Self) -> std::io::Result<()>,
    ) -> bool {
        if spawn(self).is_err() {
            let mut state = self.lock();
            state.start_failed = true;
            state.prepared = true;
            // No writer can supply evidence. Settle safely, without later reading or
            // overwriting a previous image's record from an observer's fallback path.
            self.shared.set_swept();
            return false;
        }
        true
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
            if let Some(thread) = &previous.session_id {
                let seeded = self.shared.seed_before_native_work(|| {
                    if previous.carried_on > self.shared.carried_on() {
                        self.shared.set_carried_on(previous.carried_on);
                    }
                    if let Some(pace) = &previous.pace {
                        self.shared.set_pace(thread, pace.clone().into());
                    }
                    self.shared.seed_assignment_receipt(
                        previous
                            .assignment
                            .clone()
                            .and_then(super::assignment_record::Receipt::into_core),
                    );
                    if self.shared.owed_act().is_none() {
                        self.shared
                            .set_owed_act(previous.owed.clone().map(Into::into));
                    }
                    if self.shared.recovery().is_none() {
                        self.shared
                            .set_recovery(previous.recovery.clone().map(Into::into));
                    }
                    if self.shared.in_flight_retry().is_none() {
                        self.shared
                            .set_in_flight_retry(previous.in_flight_retry.clone().map(Into::into));
                    }
                    self.shared
                        .set_inherited(codex_alab_core::session::Inherited {
                            exec_generation: previous.exec_generation,
                            status: serde_json::to_value(previous.status)
                                .ok()
                                .and_then(|status| status.as_str().map(str::to_owned)),
                            session_id: previous.session_id.clone(),
                            owed_act: previous.owed.clone().map(Into::into),
                            recovery: previous.recovery.clone().map(Into::into),
                            in_flight_retry: previous.in_flight_retry.clone().map(Into::into),
                            needs_off: previous
                                .needs_off
                                .as_deref()
                                .and_then(record::needs_key_of)
                                .unwrap_or(0),
                            workdir: previous.workdir.clone().map(PathBuf::from),
                        });
                    if let (Some(session), Some(scope)) =
                        (&previous.session_id, &previous.probe_scope)
                    {
                        self.shared
                            .carry_probe_scope(session, &scope.clone().into());
                    }
                });
                if seeded {
                    state.inherited_handoff = Some(previous);
                }
            }
        }
        let started = launch.map_or_else(now_ms, |launch| launch.started_at_ms);
        let own_argv = launch.map_or(&[][..], |launch| launch.args.as_slice());
        let horizon = u64::try_from(codex_alab_core::session::RELAUNCH_HORIZON.as_millis())
            .unwrap_or(u64::MAX);
        let shared = self.shared;
        let mut swept_handoff: Option<(u64, Record, bool, u32)> = None;
        let mut held_sources = Vec::new();
        // Only this process's own predecessor is taken: another tab's dead record stands for
        // that tab's relaunch (and keeps its package from the pruner) until it takes it.
        record::sweep_with(
            &self.dir,
            self.pid(),
            started,
            identity::is_live,
            |swept| {
                if native_pending_owed(swept).is_some()
                    && !self.same_session(swept)
                    && started.saturating_sub(swept.updated_at) <= horizon
                {
                    return record::Sweep::Leave;
                }
                match record::judge(swept, own_argv, started, horizon) {
                    record::Sweep::Take => record::Sweep::Hold,
                    other => other,
                }
            },
            |pid, swept| {
                // A restore interrupted after hard-linking can leave the exact
                // same record at its canonical and temporary names. It is one
                // snapshot, unlike genuinely different equal-time sources.
                if held_sources.iter().any(|source| source == swept) {
                    return;
                }
                held_sources.push(swept.clone());
                // Every carried field comes from one unique newest record, including
                // explicit empty fields and a chain reset. A tie grants no authority;
                // keep its chain bounded even if another early image crashes before scan.
                match &mut swept_handoff {
                    Some((updated, _, unique, chain)) if *updated == swept.updated_at => {
                        *unique = false;
                        *chain = (*chain)
                            .max(swept.carried_on)
                            .max(codex_alab_core::session::MAX_CARRY_ONS);
                    }
                    Some((updated, _, _, _)) if *updated > swept.updated_at => {}
                    _ => {
                        swept_handoff =
                            Some((swept.updated_at, swept.clone(), true, swept.carried_on));
                    }
                }
                if let Some(session_id) = &swept.session_id {
                    if let Some(scope) = &swept.probe_scope {
                        shared.carry_probe_scope(session_id, &scope.clone().into());
                    }
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
                        needs_off: swept
                            .needs_off
                            .as_deref()
                            .and_then(record::needs_key_of)
                            .unwrap_or(0),
                        workdir: swept.workdir.as_deref().map(PathBuf::from),
                        owed_act: swept.owed.clone().map(Into::into),
                        recovery: swept.recovery.clone().map(Into::into),
                        in_flight_retry: swept.in_flight_retry.clone().map(Into::into),
                        pace: swept.pace.clone().map(Into::into),
                        aterm_session: swept.aterm_session.clone(),
                    });
                }
            },
        );
        state.held_sources = held_sources;
        // Seed once, so a later native/manual clearing never gets filled back in.
        // A same-identity image supersedes every other dead source, including its empties.
        if shared.inherited().is_none()
            && let Some((_, mut source, unique, chain)) = swept_handoff
            && let Some(thread) = source.session_id.clone()
        {
            if !unique {
                shared.seed_before_native_work(|| {
                    shared.set_carried_on(chain.max(shared.carried_on()));
                });
            } else {
                let eligible = record::relaunch_owed(&source)
                    && started.saturating_sub(source.updated_at) <= horizon;
                let native_custody = self.native_custody_source(&source);
                source.owed = source.owed.filter(|owed| {
                    (eligible || native_custody)
                        && source.session_id.as_deref() == Some(&owed.thread)
                });
                source.recovery = source.recovery.filter(|retry| {
                    eligible && source.session_id.as_deref() == Some(&retry.thread)
                });
                source.in_flight_retry = source.in_flight_retry.filter(|retry| {
                    eligible
                        && source.session_id.as_deref() == Some(&retry.thread)
                        && source.status == Status::Busy
                        && source.carried_on < codex_alab_core::session::MAX_CARRY_ONS
                });
                // THE PACE, HOWEVER OLD (aterm's loop seeds its streak and its retry ladder
                // from the session's books at every start, `short_streak_in`, `seed_retry`,
                // with no horizon): the acts the budget counts fall out of its window at the
                // seed ([`codex_alab_core::session::Pace::typed_ms`]), nothing else does. Held
                // here for custody only: the record carries it, and the autopilot takes it up,
                // where it is this tab's own (the same aterm session, [`Self::same_session`])
                // or aterm's relaunch typed this line ([`Self::handoff_allows`]).
                let seeded = shared.seed_before_native_work(|| {
                    shared.set_carried_on(chain.max(shared.carried_on()));
                    shared.seed_assignment_receipt(
                        source
                            .assignment
                            .clone()
                            .and_then(super::assignment_record::Receipt::into_core),
                    );
                    if shared.owed_act().is_none() {
                        shared.set_owed_act(source.owed.clone().map(Into::into));
                    }
                    if shared.recovery().is_none() {
                        shared.set_recovery(source.recovery.clone().map(Into::into));
                    }
                    if shared.in_flight_retry().is_none() {
                        shared.set_in_flight_retry(source.in_flight_retry.clone().map(Into::into));
                    }
                    if let Some(pace) = &source.pace
                        && shared.pace(&thread).is_none()
                    {
                        shared.set_pace(&thread, pace.clone().into());
                    }
                });
                if seeded {
                    state.pending_handoff = Some(source);
                }
            }
        }
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
                // So is a held count that has stood the interval: it is written now.
                self.shared
                    .expire_background_terminals_hold(Instant::now(), self.interval);
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
    /// awaited act's turn runs out of time ([`Shared::await_turn`]) or a held count has stood
    /// the interval ([`Shared::background_terminals_hold_deadline`]), at most an hour.
    fn idle_wait(&self) -> Duration {
        let hour = Duration::from_secs(3600);
        let awaited = self.shared.await_deadline_ms().map(|deadline| {
            Duration::from_millis(deadline.saturating_sub(now_ms()).saturating_add(1))
        });
        let held = self
            .shared
            .background_terminals_hold_deadline(self.interval)
            .map(|deadline| {
                deadline
                    .saturating_duration_since(Instant::now())
                    .saturating_add(Duration::from_millis(1))
            });
        awaited
            .into_iter()
            .chain(held)
            .min()
            .unwrap_or(hour)
            .min(hour)
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
        self.write_now_with(|| {})
    }

    /// The callback observes the completed atomic write, before source retirement.
    pub(crate) fn write_now_with(&self, after_write: impl FnOnce()) -> std::io::Result<bool> {
        self.prepare();
        let mut state = self.lock();
        if state.start_failed {
            return Err(std::io::Error::other("session writer did not start"));
        }
        if state.finalized {
            return Ok(false);
        }
        let record = self.build(&mut state, now_ms());
        // A held move of the count alone is no write yet: it is written once it has stood
        // the interval, or rides along on the write another change causes (CONTRACT §4), so
        // a command that ends within the interval costs no full sync.
        let held = self
            .shared
            .background_terminals_held(Instant::now(), self.interval);
        if state.last.as_ref().is_some_and(|last| {
            last.same_content(&record) || (held && last.same_but_background_terminals(&record))
        }) {
            self.retire_sources(&mut state, &record);
            return Ok(false);
        }
        record::write_atomic(&self.dir, &record)?;
        after_write();
        self.retire_sources(&mut state, &record);
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

    fn retire_sources(&self, state: &mut WriterState, current: &Record) {
        if current.resume_argv.is_some() || !record::relaunch_owed(current) {
            let retained: Vec<_> = state
                .held_sources
                .iter()
                .filter(|source| self.retain_native_source(state, source))
                .cloned()
                .collect();
            let _ = self.remove_held_sources_if(state, |source| !retained.contains(source));
        }
    }

    fn remove_held_sources(&self, state: &mut WriterState) -> std::io::Result<()> {
        self.remove_held_sources_if(state, |_| true)
    }

    fn remove_held_sources_if(
        &self,
        state: &mut WriterState,
        remove: impl Fn(&Record) -> bool,
    ) -> std::io::Result<()> {
        let mut failure = None;
        state.held_sources.retain(|source| {
            if !remove(source) {
                return true;
            }
            match record::retire_held_source(&self.dir, source, identity::is_live) {
                Ok(()) => false,
                Err(err) => {
                    failure = Some(err);
                    true
                }
            }
        });
        failure.map_or(Ok(()), Err)
    }

    /// End the record (the exit path); nothing is written after this.
    pub fn finalize(&self, ending: &Ending) -> std::io::Result<()> {
        self.finalize_with(ending, || Ok(()))
    }

    pub(crate) fn finalize_with(
        &self,
        ending: &Ending,
        before_retire: impl FnOnce() -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        self.prepare();
        let mut state = self.lock();
        if state.start_failed {
            return Err(std::io::Error::other("session writer did not start"));
        }
        if state.finalized {
            return Ok(());
        }
        state.finalized = true;
        self.shared.mark_changed();
        match ending {
            Ending::Remove => {
                if !state.held_sources.is_empty() {
                    // Commit the clearing before retiring either side of custody.
                    // A crash or cleanup error leaves this newer terminal record,
                    // which another tab must keep until old authority expires.
                    let mut tombstone = self.build(&mut state, now_ms());
                    // Revoke the held launch lineage, even if the current UI has
                    // since moved to another root. All held resume lines exactly
                    // matched this process's original argv at the startup census.
                    let source = state.pending_handoff.as_ref().or_else(|| {
                        state
                            .held_sources
                            .iter()
                            .find(|source| source.session_id.is_some())
                    });
                    tombstone.session_id = source.and_then(|source| source.session_id.clone());
                    tombstone.resume_argv = source.and_then(|source| source.resume_argv.clone());
                    tombstone.status = Status::Exited;
                    tombstone.exit = Some(record::HANDOFF_CLEARED.into());
                    tombstone.owed = None;
                    tombstone.recovery = None;
                    tombstone.in_flight_retry = None;
                    tombstone.pace = None;
                    tombstone.carried_on = tombstone
                        .carried_on
                        .max(codex_alab_core::session::MAX_CARRY_ONS);
                    record::write_atomic(&self.dir, &tombstone)?;
                    before_retire()?;
                    self.remove_held_sources(&mut state)?;
                }
                record::remove(&self.dir, self.pid())?;
                Ok(())
            }
            Ending::Signalled(signal) => {
                if state.held_status.is_none() {
                    state.held_status = Some(self.build(&mut state, now_ms()).status);
                }
                let mut record = self.build(&mut state, now_ms());
                record.exit = Some(format!("signal:{signal}"));
                record::write_atomic(&self.dir, &record)?;
                self.retire_sources(&mut state, &record);
                Ok(())
            }
            Ending::Exited(exit) => {
                let mut record = self.build(&mut state, now_ms());
                record.status = Status::Exited;
                record.exit = Some(exit.clone());
                record::write_atomic(&self.dir, &record)?;
                self.retire_sources(&mut state, &record);
                Ok(())
            }
            Ending::Upgrading => {
                let mut record = self.build(&mut state, now_ms());
                record.status = Status::Upgrading;
                record.exec_generation += 1;
                record::write_atomic(&self.dir, &record)?;
                self.retire_sources(&mut state, &record);
                Ok(())
            }
        }
    }

    fn build(&self, state: &mut WriterState, now: u64) -> Record {
        let shared = self.shared;
        let launch = shared.launch();
        let view = shared.view();
        let (native_seen, native_thread, native_accepted) = shared.native_work();
        let native_superseded = native_seen && (!shared.leaving_for_relaunch() || native_accepted);
        let (mut status, state_word, blocked_kind) = status_of(view.state);
        // `idle` only at true rest: no turn running, no act being carried out or owed, no
        // person's turn on its way (CONTRACT §4: aterm may end the process at `idle`).
        if status == Status::Idle && shared.unrest().is_some() {
            status = Status::Busy;
        }
        if let Some(held) = state.held_status {
            status = held;
        }
        // A frame can arrive before the launch probe. Seeing the thread on screen does
        // not authorize copying an unverified predecessor into this process's record.
        let recovery = shared
            .recovery()
            .map(record::Recovery::from)
            .filter(|retry| self.handoff_allows(state, |source| recovery_source(source, retry)));
        let in_flight_retry = shared
            .in_flight_retry()
            .map(record::InFlightRetry::from)
            .filter(|retry| self.handoff_allows(state, |source| in_flight_source(source, retry)));
        let owed = shared.owed_act().map(record::Owed::from).filter(|owed| {
            self.handoff_allows(state, |source| owed_source(source, owed))
                || self.native_owed_handoff(state, owed)
        });
        let carried_thread = recovery.as_ref().and_then(|retry| {
            Self::carried_handoff(state, |source| recovery_source(source, retry))
                .then(|| retry.thread.clone())
        });
        let ambiguous_native =
            native_superseded && native_thread.is_none() && view.primary_thread.is_none();
        let bootstrap_thread = || {
            state
                .inherited_handoff
                .as_ref()
                .or(state.pending_handoff.as_ref())
                .and_then(|source| source.session_id.clone())
        };
        let provisional_thread = if native_superseded {
            native_thread.or_else(bootstrap_thread)
        } else {
            carried_thread
                .or_else(|| {
                    in_flight_retry.as_ref().and_then(|retry| {
                        Self::carried_handoff(state, |source| in_flight_source(source, retry))
                            .then(|| retry.thread.clone())
                    })
                })
                .or_else(|| {
                    owed.as_ref().and_then(|owed| {
                        Self::carried_handoff(state, |source| owed_source(source, owed))
                            .then(|| owed.thread.clone())
                    })
                })
                .or_else(|| {
                    // The bootstrap's known thread is independent of an act. A settled
                    // empty handoff must survive process death too; it must not revive
                    // the old act before any native widget has produced its first frame.
                    state
                        .inherited_handoff
                        .as_ref()
                        .or_else(|| {
                            shared.launch_probed().then_some(())?;
                            state.pending_handoff.as_ref()
                        })
                        .and_then(|source| source.session_id.clone())
                })
        };
        let provisional = view.primary_thread.is_none() && provisional_thread.is_some();
        let bootstrap = provisional && !native_superseded;
        if provisional && native_superseded && state.held_status.is_none() {
            status = if shared.unrest().is_some() {
                Status::Busy
            } else {
                Status::Idle
            };
        }
        if provisional
            && state.held_status.is_none()
            && in_flight_retry
                .as_ref()
                .is_some_and(|retry| provisional_thread.as_deref() == Some(&retry.thread))
        {
            // An early second crash still owes the original busy carry-on. Retaining a
            // pacing fact must not turn that busy predecessor into an idle one.
            status = Status::Busy;
        }
        let primary = view.primary_thread.clone().or(provisional_thread);
        let workdir = primary.as_deref().and_then(|thread| shared.workdir(thread));
        let probe_scope = primary
            .as_deref()
            .and_then(|thread| shared.probe_scope(thread))
            .map(record::ProbeScope::from);
        let goal = (!ambiguous_native)
            .then(|| primary.as_deref().and_then(|thread| shared.goal(thread)))
            .flatten();
        let pace = (!ambiguous_native)
            .then(|| {
                primary.as_deref().and_then(|thread| {
                    shared.pace(thread).map(record::Pace::from).filter(|pace| {
                        let from_source = |source: &Record| {
                            source.session_id.as_deref() == Some(thread)
                                && source.pace.as_ref() == Some(pace)
                        };
                        // A pace from this process's own aterm session is carried with no
                        // proof, as aterm's books for the tab are (CXR-2).
                        self.handoff_allows(state, from_source)
                            || state.pending_handoff.as_ref().is_some_and(|source| {
                                from_source(source) && self.same_session(source)
                            })
                    })
                })
            })
            .flatten();
        let pending_probe =
            state.pending_handoff.is_some() && !shared.launch_probed() && !native_superseded;
        let known_materialized = primary.as_deref().is_some_and(|thread| {
            let matches = |source: &Record| {
                source.session_id.as_deref() == Some(thread) && source.resume_argv.is_some()
            };
            state.inherited_handoff.as_ref().is_some_and(matches)
                || (shared.launch_probed() && state.pending_handoff.as_ref().is_some_and(matches))
        });
        let materialized = !pending_probe
            && (view
                .primary()
                .and_then(|facts| facts.rollout_path.as_deref())
                .is_some_and(rollout_materialized)
                || provisional
                || known_materialized);
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
        if let Some(auth_path) = codex_alab_core::home::admitted_auth_path_of(
            true,
            launch.and_then(|launch| launch.var(codex_alab_core::home::AUTH_PATH_ENV)),
        ) && let Some(auth_str) = auth_path.to_str()
        {
            env.insert(
                codex_alab_core::home::AUTH_PATH_ENV.to_owned(),
                auth_str.to_owned(),
            );
        }
        let path_string = |path: Option<&Path>| path.and_then(Path::to_str).map(str::to_owned);
        let recovery = recovery.filter(|recovery| {
            !ambiguous_native
                && Some(&recovery.thread) == primary.as_ref()
                && (!bootstrap
                    || Self::carried_handoff(state, |source| recovery_source(source, recovery)))
        });
        let in_flight_retry = in_flight_retry.filter(|retry| {
            !ambiguous_native
                && Some(&retry.thread) == primary.as_ref()
                && (!bootstrap
                    || Self::carried_handoff(state, |source| in_flight_source(source, retry)))
        });
        let owed = owed.filter(|owed| {
            !ambiguous_native
                && Some(&owed.thread) == primary.as_ref()
                && (!bootstrap
                    || Self::carried_handoff(state, |source| owed_source(source, owed))
                    || self.native_owed_handoff(state, owed))
        });
        let bootstrap_source = state
            .inherited_handoff
            .as_ref()
            .or(state.pending_handoff.as_ref())
            .filter(|source| source.session_id.as_ref() == primary.as_ref());
        // Unknown before the first Frame is conservatively busy. That must not
        // mint a continuation from an empty idle/waiting/upgrade handoff. Only
        // the exact authenticated busy crash predecessor already had that right.
        let busy_predecessor = state.pending_handoff.as_ref().is_some_and(|source| {
            source.session_id.as_ref() == primary.as_ref() && source.status == Status::Busy
        }) && self.handoff_allows(state, |_| true);
        let empty_bootstrap = bootstrap
            && bootstrap_source.is_some()
            && shared.unrest().is_none()
            && shared.record_draft_empty()
            && owed.is_none()
            && recovery.is_none()
            && in_flight_retry.is_none()
            && !busy_predecessor;
        let mut record = Record {
            assignment: shared.assignment_receipt().as_ref().map(Into::into),
            v: record::SCHEMA_VERSION,
            pid: self.pid(),
            proc_start: launch.and_then(|launch| launch.proc_start.clone()),
            identity: launch
                .and_then(|launch| launch.identity.as_ref())
                .map(record::Identity::from),
            session_id: primary,
            displayed_thread_id: view.displayed_thread.clone(),
            cwd: path_string(view.cwd.as_deref()),
            version: version().to_owned(),
            exe: path_string(launch.and_then(|launch| launch.current_exe.as_deref())),
            launch_argv0: path_string(launch.and_then(Launch::upgrade_path)),
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
            // An empty bootstrap revocation, including several accepted roots
            // without an identifiable primary, grants no generic busy carry-on.
            carried_on: if ambiguous_native || empty_bootstrap {
                shared
                    .carried_on()
                    .max(codex_alab_core::session::MAX_CARRY_ONS)
            } else {
                shared.carried_on()
            },
            owed,
            needs_off: Some(shared.needs_off())
                .filter(|key| *key != 0)
                .map(record::needs_key_word),
            workdir: path_string(workdir.as_deref()),
            probe_scope,
            pace,
            recovery,
            in_flight_retry,
            aterm_session: launch.and_then(Launch::aterm_session).map(str::to_owned),
            goal,
            background_terminals: shared.background_terminals(),
        };
        if pending_probe {
            // aterm inherits its exact typed line only from this complete
            // initializer shape. A restored Frame is kept internally, but
            // publishing half its identity would revoke that safe fallback
            // before the proven handoff can be written as a concrete record.
            record.session_id = None;
            record.displayed_thread_id = None;
            record.resume_argv = None;
            record.cwd = None;
            record.state = None;
            record.blocked_kind = None;
            record.status = Status::Busy;
            record.status_updated_at = 0;
            record.workdir = None;
            record.probe_scope = None;
            record.owed = None;
            record.recovery = None;
            record.in_flight_retry = None;
            record.pace = None;
            record.goal = None;
            // A count is a frame fact: the busy initializer does not carry it.
            record.background_terminals = None;
            record.draft_empty =
                record.draft_empty && !shared.queued_input() && !shared.pending_user_turn();
        }
        record
    }

    fn carried_handoff(state: &WriterState, matches: impl Fn(&Record) -> bool) -> bool {
        state.pending_handoff.as_ref().is_some_and(&matches)
            || state.inherited_handoff.as_ref().is_some_and(matches)
    }

    fn handoff_allows(&self, state: &WriterState, matches: impl Fn(&Record) -> bool) -> bool {
        let shared = self.shared;
        if state.inherited_handoff.as_ref().is_some_and(&matches)
            || !state.pending_handoff.as_ref().is_some_and(matches)
        {
            return true;
        }
        let Some(source) = &state.pending_handoff else {
            return false;
        };
        let Some(thread) = &source.session_id else {
            return false;
        };
        let horizon = u64::try_from(codex_alab_core::session::RELAUNCH_HORIZON.as_millis())
            .unwrap_or(u64::MAX);
        shared.predecessor(thread).is_some_and(|old| {
            old.pid == source.pid
                && old.updated_at_ms == source.updated_at
                && codex_alab_core::session::exit_relaunchable(old.exit.as_deref())
                && shared.launch().is_some_and(|launch| {
                    launch.started_at_ms.saturating_sub(old.updated_at_ms) <= horizon
                        && old.resume_argv.as_ref().is_some_and(|argv| {
                            argv.len() == launch.args.len()
                                && argv
                                    .iter()
                                    .zip(&launch.args)
                                    .all(|(want, got)| got.to_str() == Some(want.as_str()))
                        })
                        && shared
                            .launch_witness()
                            .is_some_and(|witness| witness.proves_relaunch(launch.started_at_ms))
                })
        })
    }

    fn resume_argv(&self, launch: &Launch, thread: &str) -> Option<Vec<String>> {
        let command = self.command?;
        let argv0 = launch.upgrade_path()?.to_str()?;
        resume_argv::rebuild(command(), &launch.args, argv0, thread)
    }
}

/// Whether `next` leaves the rest `last` stood at: `idle` becomes anything else, or an empty
/// draft becomes a person's (CONTRACT §4: these are written through).
pub fn leaves_rest(last: &Record, next: &Record) -> bool {
    (last.status == Status::Idle && next.status != Status::Idle)
        || (last.draft_empty && !next.draft_empty)
}

/// THE CHANGES WRITTEN THROUGH, never coalesced (CONTRACT §4): leaving rest ([`leaves_rest`]);
/// the act owed moving (decided, typed or dropped; a new time alone, which only re-reads the
/// same deadline through another clock, waits for the next write), since a crash at rest
/// hands the record's act to aterm's relaunch and nothing else holds it; and the pace moving
/// (an act typed, a wall's rung climbed), as aterm writes its books at each act (B3: the
/// pace reached the record only at the next coalesced write, so a crash within it lost an
/// act from the hour's budget and a rung from the ladder).
pub fn written_through(last: &Record, next: &Record) -> bool {
    let act = |record: &Record| {
        record.owed.clone().map(|mut owed| {
            owed.not_before = 0;
            owed
        })
    };
    leaves_rest(last, next) || act(last) != act(next) || last.pace != next.pace
}

/// A fresh thread's rollout is written only from its first user message on; until then
/// `codex resume <id>` cannot find it (`resumable_thread` needs a non-empty file).
pub(crate) fn rollout_materialized(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
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
