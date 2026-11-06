// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! codex-alab's OSC 7501 report as aterm counts its turns (`ALAB(turns)`, CONTRACT §6).
//!
//! aterm's engine counts, as each OSC 7501 report is parsed, the turns the reports begin
//! (`aterm-core` `terminal/program_state.rs`, `ProgramStateReport::turns`, aterm main
//! 4a79c8db07): an in-turn report (`working`, `blocked`) whose report before it was at rest
//! (`idle`, `done`, `error`), or that is the first report taken; a `clear` and a full reset
//! are looked past, never counted from; a report written again begins none. A proven
//! self-claim's landing reopens at a count past the one its `landed@v1` was recorded over
//! (`aterm-gui` `harness_task.rs`, `reopen_self_claim_landing`, e7ab6c9ce4): its holder
//! types nothing after a landing, so a turn begun after it is someone else's. Upstream's
//! report says what the TUI shows, which is not that, in two places:
//!
//! - MCP STARTUP. While the thread's MCP servers start (a launch, a `resume`, `/resume`, a
//!   thread switch, the live upgrade's re-exec) the TUI's running flag stands with no turn,
//!   and upstream reports `working`, then `idle`: aterm counted a turn nobody began, and a
//!   landed task reopened. Here the startup is left out ([`Frame::reported`]): the report
//!   says where the thread stands without it, `idle` at a launch, or the last turn's `done`
//!   or `error` until a key acknowledges it.
//! - A TURN BEGUN WHILE THE REPORT STANDS IN A TURN (a queued message the TUI sends the
//!   moment the last turn ends, a message sent while a background terminal or an active goal
//!   keeps the report at `working`) wrote nothing, so aterm counted none and a landing never
//!   reopened. Here each turn the TUI submitted (a person's prompt, a driver's typed turn, a
//!   queued message, `/compact`, a review, a shell command: upstream's
//!   `user_turn_pending_start` as the server starts it) gets its own rest-to-turn edge as it
//!   starts: `idle`, then `working` ([`Report::starting`]). A turn the TUI did not submit
//!   (a goal's continuation, an autopilot act through the app-server) begun under standing
//!   work writes nothing: that work runs on, as aterm-core counts it, and a landed task's own
//!   wake keeps its landing (aterm's `quiet_wake`, codex-alab's `Rejudge::Wake`). A turn
//!   begun from rest writes `working` as it starts, whoever began it, so one that ends before
//!   the next frame is counted as a frame would have shown it.
//! - A THREAD SWITCH BACK TO A RUNNING TURN. The report says the displayed thread's state,
//!   and a switch replaces the `ChatWidget`: a switch from a thread whose turn runs to one at
//!   rest wrote its rest, and the switch back wrote `working` again, a turn nobody began.
//!   Here, while a turn of one of the autopilot's threads the TUI does not show runs, a
//!   frame at rest says `working` ([`Frame::elsewhere`]): the switch back writes nothing, and
//!   the rest is written once that turn ends. Every other turn the TUI sees start on a thread
//!   it routes the app-server's notifications for ([`seen`]: a sub-agent's, a side thread's,
//!   any under a remote app-server, where no autopilot runs in this process) only HOLDS a
//!   report that stands in a turn ([`Frame::held`]): a switch from a running sub-agent to a
//!   thread at rest and back writes nothing, but a seen turn that starts while the report
//!   rests begins none, since nobody here carries it (the autopilot books, decides and lands
//!   its root threads only), and a landing aterm reopened at that begin would stay open with
//!   nobody to land it again. `Shared` wakes the TUI as any of them starts or ends. A seen
//!   turn's end is booked before any routing return (`thread/closed` and `turn/completed`
//!   from the event stream itself), and one whose end may not reach the TUI is let go: a
//!   thread the TUI stops routing, and a stream that lost events (a lag, a disconnect)
//!   ([`seen_keep`]); a turn the TUI learned of only from a thread's snapshot (no
//!   `turn/started` reached it) is not booked, so a switch back to it after a thread at rest
//!   was shown begins one.
//!
//! The words go through upstream's status writer/cache owner, which drops an ordinary word
//! equal to the last it wrote and re-sends after a handoff, so its cache and the exit's
//! `clear` stay its own. An edge reaches that owner as one batch: under the cache lock it
//! takes stdout, writes the rest prefix and forces the final `working`, then flushes before
//! releasing the writer. A competing status clear cannot interleave between its words.
//! This does not promise that the OS or a terminal reader cannot split a write. This module
//! remembers the last word written, a `clear` looked past as aterm looks past it: what aterm
//! judges the next report against. Only in the codex binary inside aterm ([`enabled`]);
//! elsewhere, iTerm2's OSC 21337 included, upstream's report is unchanged.

use codex_alab_core::session::BlockedKind;
use codex_alab_core::session::ProgramState;
use std::cell::RefCell;
use std::io;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::PoisonError;

/// The rest word an edge writes before a turn's `working`: a report at rest, which never
/// stands (the next word follows at once).
pub const REST_MARK: ProgramState = ProgramState::Idle;

/// One frame's facts for the report (`glue/chat_hand.rs`, each `pre_draw_tick` and overlay
/// event).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    /// What upstream's report says (`desired_program_status`).
    pub state: ProgramState,
    /// Whether `state` is `working` only because MCP servers are starting: no turn runs and
    /// none is submitted, no review, no background terminal, no active goal between turns.
    pub startup_only: bool,
    /// Where the thread stands with no work (upstream's `program_status_after_turn`): `idle`,
    /// or the last turn's `done` or `error` until acknowledged.
    pub rest: ProgramState,
    /// Whether the TUI submitted a turn the server has not started yet (upstream's
    /// `user_turn_pending_start`, or an image being readied for one): a frame that turns the
    /// report from rest to `working` for it shows that turn's begin.
    pub submitted: bool,
    /// Whether a turn of one of the autopilot's threads the TUI does not show runs
    /// ([`running_elsewhere`]): the session works on, so a frame at rest says `working`, and a
    /// thread switch away from a running turn and back begins none.
    pub elsewhere: bool,
    /// Whether a turn the TUI saw start on a thread it does not show runs ([`seen_elsewhere`]:
    /// a sub-agent's, a side thread's, one under a remote app-server): a frame at rest keeps a
    /// report that stands in a turn there ([`Frame::reported_after`]) and begins none.
    pub held: bool,
}

impl Frame {
    /// The state the report says: upstream's, with MCP startup left out, and in a turn while
    /// another thread's turn runs.
    pub fn reported(self) -> ProgramState {
        let state = if self.startup_only && self.state == ProgramState::Working {
            self.rest
        } else {
            self.state
        };
        if self.elsewhere && !in_turn(state) && state != ProgramState::Unknown {
            ProgramState::Working
        } else {
            state
        }
    }

    /// The state the report says after `last`, the last word written ([`Report::last`]):
    /// [`Self::reported`], and a rest kept in its turn while a seen turn runs elsewhere
    /// ([`Self::held`]) when `last` stands in one. From rest, or with no word written, a seen
    /// turn begins none.
    pub fn reported_after(self, last: Option<ProgramState>) -> ProgramState {
        let state = self.reported();
        if self.held
            && !in_turn(state)
            && state != ProgramState::Unknown
            && last.is_some_and(in_turn)
        {
            ProgramState::Working
        } else {
            state
        }
    }
}

/// ALAB(turns) glue: whether a turn of one of the autopilot's threads runs on a thread other
/// than `displayed`, the thread the TUI shows (`Shared::turn_running_besides`, in the codex
/// binary or a test's box scope; never elsewhere).
pub fn running_elsewhere(displayed: Option<&str>) -> bool {
    crate::boxes::tick_shared().is_some_and(|shared| shared.turn_running_besides(displayed))
}

/// ALAB(turns) glue: whether a turn the TUI saw start ([`seen`]) runs on a thread other than
/// `displayed` (`Shared::turn_seen_besides`, in the codex binary or a test's box scope; never
/// elsewhere).
pub fn seen_elsewhere(displayed: Option<&str>) -> bool {
    crate::boxes::tick_shared().is_some_and(|shared| shared.turn_seen_besides(displayed))
}

/// ALAB(turns) glue: the TUI routed `turn/started` (`running`), or its event stream carried
/// `turn/completed`, of `turn` for `thread` (`Shared::turn_seen`, in the codex binary or a
/// test's box scope; never elsewhere): booked for [`seen_elsewhere`], whatever thread and
/// server.
pub fn seen(thread: &str, turn: &str, running: bool) {
    if let Some(shared) = crate::boxes::tick_shared() {
        shared.turn_seen(thread, turn, running);
    }
}

/// ALAB(turns) glue: the chat widget took `turn/completed` of `turn` for `thread`
/// (`Shared::turn_handled_end`, in the codex binary or a test's box scope; never elsewhere):
/// that turn's begin edge, if any, is written, since the widget took its `turn/started` first.
pub fn handled_end(thread: &str, turn: &str) {
    if let Some(shared) = crate::boxes::tick_shared() {
        shared.turn_handled_end(thread, turn);
    }
}

/// ALAB(turns) glue: the seen turns kept only on threads `keep` names
/// (`Shared::turns_seen_keep`): a closed thread's go, a thread the TUI no longer routes, and
/// every one where the notification stream lost events.
pub fn seen_keep(keep: impl Fn(&str) -> bool) {
    if let Some(shared) = crate::boxes::tick_shared() {
        shared.turns_seen_keep(keep);
    }
}

/// Whether a state is in a turn as aterm counts it (`working`, `blocked`).
pub fn in_turn(state: ProgramState) -> bool {
    matches!(state, ProgramState::Working | ProgramState::Blocked(_))
}

/// The report's memory: the last word written, and whether a frame already showed the begin
/// of a submitted turn that has not started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The state of the last word this process wrote, a `clear` looked past (aterm's `last`):
    /// `None` before the first.
    last: Option<ProgramState>,
    /// A frame turned the report from rest to a turn while the TUI's submission waited for
    /// the server: that submitted turn's begin is shown, and its start writes no second edge.
    /// Dropped at a report at rest and at a submitted turn's start.
    submitted_shown: bool,
}

impl Report {
    pub const fn new() -> Self {
        Self {
            last: None,
            submitted_shown: false,
        }
    }

    /// The last word written, if any.
    pub fn last(&self) -> Option<ProgramState> {
        self.last
    }

    /// The words a live turn's start writes, before the frame after it: under a report that
    /// stands in a turn, `idle` then `working` for a turn the TUI submitted (`asked`) whose
    /// begin no frame showed yet, and nothing for any other; from rest, `working`. Before this
    /// process wrote any report aterm's last word is unknown (a predecessor's), so the start
    /// writes `idle` first.
    pub fn starting_words(&self, asked: bool) -> Vec<ProgramState> {
        match self.last {
            Some(last) if in_turn(last) => {
                if asked && !self.submitted_shown {
                    vec![REST_MARK, ProgramState::Working]
                } else {
                    Vec::new()
                }
            }
            Some(_) => vec![ProgramState::Working],
            None => vec![REST_MARK, ProgramState::Working],
        }
    }

    /// Writes `words` through `write` as one batch, in order (an edge's rest word and the
    /// `working` after it share a writer critical section and flush: the module doc),
    /// remembering the last. A failed write leaves the last word unknown (`None`): aterm may
    /// have read the batch, a part of it or none, so the next turn's start writes its edge whole.
    pub fn write(
        &mut self,
        words: Vec<ProgramState>,
        write: impl FnOnce(&[ProgramState]) -> io::Result<()>,
    ) -> io::Result<()> {
        let Some(&last) = words.last() else {
            return Ok(());
        };
        match write(&words) {
            Ok(()) => {
                self.last = Some(last);
                Ok(())
            }
            Err(err) => {
                self.last = None;
                Err(err)
            }
        }
    }

    /// One frame: its reported state ([`Frame::reported_after`] the last word) written,
    /// always (upstream's writer drops it when it stands already, and re-sends it after a
    /// handoff).
    pub fn frame(
        &mut self,
        frame: Frame,
        write: impl FnOnce(&[ProgramState]) -> io::Result<()>,
    ) -> io::Result<()> {
        let state = frame.reported_after(self.last);
        if state == ProgramState::Unknown {
            return Ok(());
        }
        let begins = in_turn(state) && !self.last.is_some_and(in_turn);
        self.write(vec![state], write)?;
        if !in_turn(state) {
            self.submitted_shown = false;
        } else if begins {
            self.submitted_shown = frame.submitted;
        }
        Ok(())
    }

    /// A live turn's start: its edge written ([`Self::starting_words`]); a submitted turn
    /// takes up the begin a frame showed for it.
    pub fn starting(
        &mut self,
        asked: bool,
        write: impl FnOnce(&[ProgramState]) -> io::Result<()>,
    ) -> io::Result<()> {
        let words = self.starting_words(asked);
        if asked {
            self.submitted_shown = false;
        }
        self.write(words, write)
    }
}

static REPORT: Mutex<Report> = Mutex::new(Report::new());

/// Whether codex-alab writes the report with aterm's turn edges: the codex binary
/// ([`codex_alab_core::active`]), inside aterm ([`codex_alab_core::inside_aterm`]).
pub fn enabled() -> bool {
    static INSIDE_ATERM: OnceLock<bool> = OnceLock::new();
    codex_alab_core::active() && *INSIDE_ATERM.get_or_init(codex_alab_core::inside_aterm)
}

/// ALAB(turns) glue, each frame's report (`refresh_terminal_program_status`): `None` where
/// upstream writes it (outside aterm, or another binary); else its words written through
/// `write` as one batch (upstream's status owner, an edge's words under one writer guard)
/// and how that went.
pub fn frame(
    frame: Frame,
    write: impl FnOnce(&[ProgramState]) -> io::Result<()>,
) -> Option<io::Result<()>> {
    if let Some(result) = with_scope(|tape| tape.frame(frame)) {
        return Some(result);
    }
    enabled().then(|| lock().frame(frame, write))
}

/// ALAB(turns) glue, a live `TurnStarted` before upstream's `on_task_started` (which clears
/// the submission): its edge written through `write`, where codex-alab writes the report.
/// `asked`: the TUI submitted the turn.
pub fn starting(
    asked: bool,
    write: impl FnOnce(&[ProgramState]) -> io::Result<()>,
) -> Option<io::Result<()>> {
    if let Some(result) = with_scope(|tape| tape.starting(asked)) {
        return Some(result);
    }
    enabled().then(|| lock().starting(asked, write))
}

fn lock() -> std::sync::MutexGuard<'static, Report> {
    REPORT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The exact bytes upstream's writer puts on the wire for a word inside aterm (no OSC 21337
/// there): `ESC ] 7501 ; state=<word> : app=codex [: kind=<kind>] ESC \`
/// (`tui/src/terminal_program_status.rs`, `SetTerminalProgramStatus`); `None` for `Unknown`,
/// which is never written.
pub fn sequence(state: ProgramState) -> Option<String> {
    let (word, kind) = match state {
        ProgramState::Unknown => return None,
        ProgramState::Idle => ("idle", None),
        ProgramState::Working => ("working", None),
        ProgramState::Blocked(kind) => (
            "blocked",
            Some(match kind {
                BlockedKind::Permission => "permission",
                BlockedKind::Question => "question",
                BlockedKind::Auth => "auth",
            }),
        ),
        ProgramState::Done => ("done", None),
        ProgramState::Error => ("error", None),
    };
    Some(match kind {
        Some(kind) => format!("\x1b]7501;state={word}:app=codex:kind={kind}\x1b\\"),
        None => format!("\x1b]7501;state={word}:app=codex\x1b\\"),
    })
}

/// upstream's `state=clear` report (`ClearTerminalProgramStatus`, inside aterm).
pub const CLEAR: &str = "\x1b]7501;state=clear\x1b\\";

/// HOW MANY TURNS ATERM'S ENGINE COUNTS IN `bytes`: its parser and rule
/// (`aterm-core` `terminal/program_state.rs`, aterm main 8221304715), ported for the checks
/// on both sides of the hooks. Each `ESC ] 7501 ; <body>` ended by `ESC \` or BEL whose body
/// it takes: at most 128 bytes, no `;`, every `:`-separated item a `key=value`, `state`,
/// `app` and `kind` each at most once, `state=clear`, or `app=codex` with a known state word
/// and, on `blocked`, a known kind when one is named. A report in a turn begins one where the
/// latest report taken before it, a `clear` looked past, was not in a turn, or where it is the
/// first.
pub fn aterm_turns(bytes: &str) -> u64 {
    const START: &str = "\x1b]7501;";
    let mut turns = 0;
    let mut last_in_turn: Option<bool> = None;
    let mut rest = bytes;
    while let Some(at) = rest.find(START) {
        let tail = &rest[at + START.len()..];
        let Some(end) = tail.find(['\x1b', '\x07']) else {
            break;
        };
        // A clear, or a body aterm does not take, moves nothing.
        if let Some(Some(in_turn)) = aterm_body(&tail[..end]) {
            if in_turn && last_in_turn != Some(true) {
                turns += 1;
            }
            last_in_turn = Some(in_turn);
        }
        rest = &tail[end..];
    }
    turns
}

/// aterm's reading of one body: `None` not taken, `Some(None)` a clear, `Some(Some(t))` a
/// report, `t` whether it is in a turn.
fn aterm_body(body: &str) -> Option<Option<bool>> {
    if body.len() > 128 || body.contains(';') {
        return None;
    }
    let (mut state, mut app, mut kind) = (None, None, None);
    for item in body.split(':') {
        let (key, value) = item.split_once('=')?;
        match key {
            "state" if state.is_none() => state = Some(value),
            "app" if app.is_none() => app = Some(value),
            "kind" if kind.is_none() => kind = Some(value),
            "state" | "app" | "kind" => return None,
            _ => {}
        }
    }
    let state = state?;
    if state == "clear" {
        return Some(None);
    }
    if app != Some("codex") {
        return None;
    }
    let in_turn = match state {
        "working" => true,
        "blocked" => {
            if let Some(kind) = kind
                && !matches!(kind, "permission" | "question" | "auth")
            {
                return None;
            }
            true
        }
        "idle" | "done" | "error" => false,
        _ => return None,
    };
    Some(Some(in_turn))
}

/// A test's own report for this thread ([`scoped`]): its memory, the bytes its words would
/// put on the wire, and what upstream's writer last wrote (its cache: `None` after a
/// `clear`).
#[derive(Debug, Default)]
struct Tape {
    report: Report,
    bytes: String,
    cache: Option<ProgramState>,
}

impl Tape {
    fn frame(&mut self, frame: Frame) -> io::Result<()> {
        let Self {
            report,
            bytes,
            cache,
        } = self;
        report.frame(frame, upstream_writer(bytes, cache))
    }

    fn starting(&mut self, asked: bool) -> io::Result<()> {
        let Self {
            report,
            bytes,
            cache,
        } = self;
        report.starting(asked, upstream_writer(bytes, cache))
    }
}

/// Upstream's writer, onto a tape, as the glue drives it (`alab_write_program_states`): one
/// word is written as upstream writes it, dropped where it equals the one it last wrote and
/// written whatever it is after a `clear`; an edge's prefix forces its final word. The tape
/// forgets its cached last word to model the owner's forced batch emission.
fn upstream_writer<'a>(
    bytes: &'a mut String,
    cache: &'a mut Option<ProgramState>,
) -> impl FnOnce(&[ProgramState]) -> io::Result<()> + 'a {
    move |words| {
        let Some((&last, before)) = words.split_last() else {
            return Ok(());
        };
        if !before.is_empty() {
            for word in before {
                bytes.push_str(&sequence(*word).unwrap_or_default());
            }
            *cache = None;
        }
        if *cache != Some(last) {
            bytes.push_str(&sequence(last).unwrap_or_default());
            *cache = Some(last);
        }
        Ok(())
    }
}

thread_local! {
    static SCOPED: RefCell<Option<Tape>> = const { RefCell::new(None) };
}

fn with_scope(act: impl FnOnce(&mut Tape) -> io::Result<()>) -> Option<io::Result<()>> {
    SCOPED.with(|scoped| scoped.borrow_mut().as_mut().map(act))
}

/// A test's report for this thread, while the guard lives: the hooks write its words, as the
/// bytes upstream's writer would put on the wire ([`sequence`]; a word equal to the one it
/// last wrote dropped, as its cache drops it), into the guard's tape instead of the terminal,
/// whatever [`enabled`] says.
pub fn scoped() -> Scope {
    Scope(SCOPED.with(|scoped| scoped.replace(Some(Tape::default()))))
}

/// The guard [`scoped`] returns; restores the previous scope when dropped.
pub struct Scope(Option<Tape>);

impl Scope {
    /// The bytes written since the last take.
    pub fn take(&self) -> String {
        SCOPED.with(|scoped| {
            scoped
                .borrow_mut()
                .as_mut()
                .map(|tape| std::mem::take(&mut tape.bytes))
                .unwrap_or_default()
        })
    }

    /// The handoff's `clear` (an external editor, Ctrl+Z, the exit), as upstream's
    /// `clear_terminal_program_status` writes it: on the wire, looked past by the memory.
    pub fn clear(&self) {
        SCOPED.with(|scoped| {
            if let Some(tape) = scoped.borrow_mut().as_mut() {
                tape.bytes.push_str(CLEAR);
                tape.cache = None;
            }
        });
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        let previous = self.0.take();
        SCOPED.with(|scoped| *scoped.borrow_mut() = previous);
    }
}
