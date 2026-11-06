// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The OSC 7501 report as aterm counts its turns (`ALAB(turns)`), at the byte level: the
//! words the hooks write, as upstream's writer puts them on the wire, read by aterm's parser
//! and rule (ported, and held to aterm's own rows). Each fixed case is set beside the bytes
//! upstream's report wrote for the same frames, which aterm counted wrong.

use crate::turns;
use crate::turns::Frame;
use crate::turns::Report;
use codex_alab_core::session::BlockedKind;
use codex_alab_core::session::ProgramState;
use pretty_assertions::assert_eq;
use std::io;

const IDLE: ProgramState = ProgramState::Idle;
const WORKING: ProgramState = ProgramState::Working;
const DONE: ProgramState = ProgramState::Done;

/// A frame where upstream's report says `state` for a reason other than MCP startup.
fn shows(state: ProgramState) -> Frame {
    Frame {
        state,
        startup_only: false,
        rest: IDLE,
        submitted: false,
        elsewhere: false,
        held: false,
    }
}

/// A frame where upstream's report says `working` while a turn the TUI submitted waits for
/// the server to start it.
fn submitted() -> Frame {
    Frame {
        submitted: true,
        ..shows(WORKING)
    }
}

/// A frame where upstream's report says `working` only because MCP servers start, the thread
/// standing at `rest`.
fn starting_mcp(rest: ProgramState) -> Frame {
    Frame {
        state: WORKING,
        startup_only: true,
        rest,
        submitted: false,
        elsewhere: false,
        held: false,
    }
}

/// One step of a session, as the two hooks see it.
#[derive(Clone, Copy)]
enum Step {
    /// A frame's report.
    Frame(Frame),
    /// A live turn starts; whether the TUI submitted it.
    Starts { asked: bool },
    /// The handoff's `clear` (Ctrl+Z, an external editor), then taken back.
    Handoff,
}

/// The bytes the hooks write for `steps`, through the public glue functions under a test's
/// scope.
fn written(steps: &[Step]) -> String {
    let scope = turns::scoped();
    for step in steps {
        let result = match *step {
            Step::Frame(frame) => turns::frame(frame, |_| unreachable!("scoped")),
            Step::Starts { asked } => turns::starting(asked, |_| unreachable!("scoped")),
            Step::Handoff => {
                scope.clear();
                Some(Ok(()))
            }
        };
        assert!(matches!(result, Some(Ok(()))), "{result:?}");
    }
    scope.take()
}

/// The bytes upstream's report wrote for the same steps: each frame's state as upstream says
/// it, dropped when it stands already, and nothing at a turn's start.
fn upstream(steps: &[Step]) -> String {
    let mut bytes = String::new();
    let mut cache = None;
    for step in steps {
        match *step {
            Step::Frame(frame) => {
                if cache != Some(frame.state) {
                    bytes.push_str(&turns::sequence(frame.state).unwrap_or_default());
                    cache = Some(frame.state);
                }
            }
            Step::Starts { .. } => {}
            Step::Handoff => {
                bytes.push_str(turns::CLEAR);
                cache = None;
            }
        }
    }
    bytes
}

/// ATERM'S RULE, AS PORTED, READS ATERM'S OWN ROWS (`aterm-core` `program_state.rs`,
/// `the_turn_count_moves_only_where_a_turn_begins`, aterm main 8221304715), one body at a
/// time, and takes no body aterm does not take. NEGATIVE CONTROLS: every row that begins none.
#[test]
fn aterms_rule_as_ported_reads_aterms_own_rows() {
    let rows: [(&str, u64, &str); 17] = [
        ("state=working:app=codex", 1, "the first report, in a turn"),
        ("state=working:app=codex", 1, "written again: no move"),
        (
            "state=blocked:app=codex:kind=permission",
            1,
            "asked within it",
        ),
        ("state=working:app=codex", 1, "answered: the same turn"),
        ("state=done:app=codex", 1, "the turn ended"),
        ("state=working:app=codex", 2, "a turn after done"),
        ("state=idle:app=codex", 2, "ended"),
        (
            "state=blocked:app=codex:kind=question",
            3,
            "a turn that asked at once",
        ),
        ("state=error:app=codex", 3, "ended on an error"),
        ("state=working:app=codex", 4, "a turn after an error"),
        ("state=clear", 4, "a handoff mid-turn"),
        (
            "state=working:app=codex",
            4,
            "the terminal taken back: resumed",
        ),
        ("state=done:app=codex", 4, "ended"),
        ("state=clear", 4, "a handoff at rest"),
        ("state=done:app=codex", 4, "taken back: still at rest"),
        ("state=clear", 4, "a handoff at rest"),
        ("state=working:app=codex", 5, "a turn begun across it"),
    ];
    let mut bytes = String::new();
    for (body, want, why) in rows {
        bytes.push_str(&format!("\x1b]7501;{body}\x1b\\"));
        assert_eq!(turns::aterm_turns(&bytes), want, "{why}: {body}");
    }
    // Bodies aterm does not take count nothing and move nothing, whatever came before.
    for body in [
        "state=idle:app=claude",
        "state=working:app=claude",
        "state=working",
        "state=working:app=codex;x",
        "state=working:app=codex:app=codex",
        "state=blocked:app=codex:kind=lunch",
        "state=napping:app=codex",
        "app=codex",
    ] {
        let after = format!("\x1b]7501;state=idle:app=codex\x1b\\\x1b]7501;{body}\x1b\\");
        assert_eq!(turns::aterm_turns(&after), 0, "{body}");
        let then = format!("{after}\x1b]7501;state=working:app=codex\x1b\\");
        assert_eq!(turns::aterm_turns(&then), 1, "judged against idle: {body}");
    }
    let long = format!("state=working:app=codex:x={}", "y".repeat(128));
    assert_eq!(turns::aterm_turns(&format!("\x1b]7501;{long}\x1b\\")), 0);
    // BEL ends a body as ST does; a body with no end is not read.
    assert_eq!(
        turns::aterm_turns("\x1b]7501;state=working:app=codex\x07"),
        1
    );
    assert_eq!(turns::aterm_turns("\x1b]7501;state=working:app=codex"), 0);
}

/// THE WORDS ARE UPSTREAM'S BYTES: each word's sequence is the 7501 part of upstream's own
/// snapshot of its writer (`tui/src/snapshots/…iterm_session_status_fields.snap`, the
/// OSC 21337 prefix aterm never gets left aside), and the clear is upstream's generic clear
/// (`tui/src/iterm_session_status_tests.rs`).
#[test]
fn the_words_are_upstreams_bytes() {
    let snapshot = include_str!(
        "../../tui/src/snapshots/codex_tui__terminal_program_status__tests__iterm_session_status_fields.snap"
    );
    let words = [
        IDLE,
        WORKING,
        ProgramState::Blocked(BlockedKind::Permission),
        ProgramState::Blocked(BlockedKind::Question),
        ProgramState::Blocked(BlockedKind::Auth),
        DONE,
        ProgramState::Error,
    ];
    let lines: Vec<&str> = snapshot.lines().filter(|l| l.contains("7501")).collect();
    assert_eq!(lines.len(), words.len(), "{snapshot}");
    for (word, line) in words.into_iter().zip(lines) {
        let sequence = turns::sequence(word).expect("a written word");
        let escaped = sequence.escape_debug().to_string();
        assert!(line.ends_with(&escaped), "{word:?}: {escaped} in {line}");
    }
    assert_eq!(turns::sequence(ProgramState::Unknown), None);
    let tests = include_str!("../../tui/src/iterm_session_status_tests.rs");
    assert!(tests.contains(r#"assert_eq!(generic, "\x1b]7501;state=clear\x1b\\");"#));
    assert_eq!(turns::CLEAR, "\x1b]7501;state=clear\x1b\\");
}

/// O1: MCP STARTUP AT A LAUNCH BEGINS NO TURN. Upstream reported `working` while the servers
/// started and `idle` after, which aterm counted as a turn (RED: the upstream bytes); the
/// report now says `idle` throughout. Then the person's first turn is one.
#[test]
fn mcp_startup_at_a_launch_begins_no_turn() {
    let launch = [
        Step::Frame(starting_mcp(IDLE)),
        Step::Frame(starting_mcp(IDLE)),
        Step::Frame(shows(IDLE)),
    ];
    assert_eq!(turns::aterm_turns(&upstream(&launch)), 1, "RED: upstream");
    let bytes = written(&launch);
    assert_eq!(bytes, turns::sequence(IDLE).unwrap());
    assert_eq!(turns::aterm_turns(&bytes), 0);

    let mut then = launch.to_vec();
    then.extend([
        Step::Starts { asked: true },
        Step::Frame(shows(WORKING)),
        Step::Frame(shows(DONE)),
    ]);
    assert_eq!(turns::aterm_turns(&written(&then)), 1, "CONTROL: a turn");
}

/// O1 AT A THREAD SWITCH OR `/resume` after a turn: the servers start while the last turn's
/// `done` stands, and the report keeps `done` (upstream's `working`, then `done`, counted a
/// second turn: RED). The live upgrade's re-exec at rest after a landing is the same start.
#[test]
fn mcp_startup_after_a_turn_keeps_the_count() {
    let steps = [
        Step::Frame(shows(IDLE)),
        Step::Starts { asked: true },
        Step::Frame(shows(WORKING)),
        Step::Frame(shows(DONE)),
        Step::Frame(starting_mcp(DONE)),
        Step::Frame(starting_mcp(DONE)),
        Step::Frame(shows(DONE)),
        Step::Frame(shows(IDLE)),
    ];
    assert_eq!(turns::aterm_turns(&upstream(&steps)), 2, "RED: upstream");
    assert_eq!(turns::aterm_turns(&written(&steps)), 1);
}

/// A frame where upstream's report says `state` for the displayed thread while a turn of a
/// thread the TUI does not show runs.
fn elsewhere(state: ProgramState) -> Frame {
    Frame {
        elsewhere: true,
        ..shows(state)
    }
}

/// A THREAD SWITCH BACK TO A RUNNING TURN: thread A's turn runs, the TUI shows thread B at its
/// last turn's `done`, then A again with its turn still running, then B once more until A's
/// turn ends. Upstream's report (B's `done`, then A's `working`) counted a turn nobody began
/// (RED); here B's frames stand in a turn while A's runs, and B's rest is written once A's
/// turn ends.
#[test]
fn a_switch_back_to_a_running_turn_begins_none() {
    let steps = [
        Step::Frame(shows(IDLE)),
        Step::Starts { asked: true },
        Step::Frame(shows(WORKING)),
        Step::Frame(elsewhere(DONE)),
        Step::Frame(elsewhere(DONE)),
        Step::Frame(shows(WORKING)),
        Step::Frame(elsewhere(DONE)),
        Step::Frame(shows(DONE)),
    ];
    assert_eq!(turns::aterm_turns(&upstream(&steps)), 2, "RED: upstream");
    let bytes = written(&steps);
    assert_eq!(turns::aterm_turns(&bytes), 1, "{bytes:?}");
    let expect = [IDLE, WORKING, DONE].map(|w| turns::sequence(w).unwrap());
    assert_eq!(bytes, expect.concat(), "B's rest only once A's turn ended");
}

/// NEGATIVE CONTROL for the switch: a turn the person begins on thread B while A's runs is a
/// turn (its own edge under the report standing in A's), and a box on B is reported as it is.
#[test]
fn a_turn_begun_beside_a_running_one_is_a_turn() {
    let steps = [
        Step::Frame(shows(IDLE)),
        Step::Starts { asked: true },
        Step::Frame(shows(WORKING)),
        Step::Frame(elsewhere(IDLE)),
        Step::Frame(Frame {
            submitted: true,
            ..elsewhere(WORKING)
        }),
        Step::Starts { asked: true },
        Step::Frame(elsewhere(WORKING)),
        Step::Frame(shows(DONE)),
    ];
    assert_eq!(turns::aterm_turns(&written(&steps)), 2);
    let blocked = ProgramState::Blocked(BlockedKind::Question);
    assert_eq!(elsewhere(blocked).reported(), blocked);
    assert_eq!(elsewhere(IDLE).reported(), WORKING);
    assert_eq!(elsewhere(ProgramState::Error).reported(), WORKING);
    assert_eq!(
        Frame {
            elsewhere: true,
            ..starting_mcp(DONE)
        }
        .reported(),
        WORKING
    );
    assert_eq!(
        elsewhere(ProgramState::Unknown).reported(),
        ProgramState::Unknown
    );
}

/// A frame where upstream's report says `state` for the displayed thread while a turn the TUI
/// saw start (a sub-agent's, one under a remote app-server) runs on a thread it does not show.
fn held(state: ProgramState) -> Frame {
    Frame {
        held: true,
        ..shows(state)
    }
}

/// A SEEN TURN HOLDS A TURN AND BEGINS NONE (parity-5 review, T-subagent-turn-reopens-landing):
/// a sub-agent's turn that starts while the shown thread rests writes nothing, since nobody in
/// this process carries it (the autopilot decides and lands its root threads only) and aterm
/// would reopen a landing at the begin, which nobody would then land again. RED with a seen
/// turn read as the autopilot's own (`elsewhere`): the rest turned `working`, a turn begun. A
/// switch from a running sub-agent to the thread at rest and back still begins none, and the
/// rest comes once its turn ends. NEGATIVE CONTROL: a turn the person begins on the shown
/// thread under a held report writes its own edge.
#[test]
fn a_seen_turn_holds_a_turn_and_begins_none() {
    let quiet = [
        Step::Frame(shows(DONE)),
        Step::Frame(held(DONE)),
        Step::Frame(held(DONE)),
        Step::Frame(shows(DONE)),
    ];
    let bytes = written(&quiet);
    assert_eq!(turns::aterm_turns(&bytes), 0, "{bytes:?}");
    assert_eq!(bytes, turns::sequence(DONE).unwrap());
    let as_elsewhere = quiet.map(|step| match step {
        Step::Frame(frame) if frame.held => Step::Frame(Frame {
            held: false,
            ..elsewhere(frame.state)
        }),
        step => step,
    });
    assert_eq!(
        turns::aterm_turns(&written(&as_elsewhere)),
        1,
        "RED: a seen turn begun as the autopilot's own"
    );

    // The person looks at the running sub-agent (its frame is `working`), back at the
    // thread at rest, at the sub-agent again, and back, until the sub-agent's turn ends.
    let switching = [
        Step::Frame(shows(DONE)),
        Step::Frame(shows(WORKING)),
        Step::Frame(held(DONE)),
        Step::Frame(shows(WORKING)),
        Step::Frame(held(DONE)),
        Step::Frame(shows(DONE)),
    ];
    assert_eq!(
        turns::aterm_turns(&upstream(&switching)),
        2,
        "RED: upstream"
    );
    let bytes = written(&switching);
    assert_eq!(turns::aterm_turns(&bytes), 1, "{bytes:?}");
    let expect = [DONE, WORKING, DONE].map(|w| turns::sequence(w).unwrap());
    assert_eq!(bytes, expect.concat(), "the rest only once its turn ended");

    // NEGATIVE CONTROL: the person's own turn on the shown thread, under the held report.
    let person = [
        Step::Frame(shows(WORKING)),
        Step::Frame(held(IDLE)),
        Step::Frame(Frame {
            submitted: true,
            ..held(WORKING)
        }),
        Step::Starts { asked: true },
        Step::Frame(held(WORKING)),
        Step::Frame(shows(DONE)),
    ];
    assert_eq!(turns::aterm_turns(&written(&person)), 2);

    let blocked = ProgramState::Blocked(BlockedKind::Question);
    assert_eq!(held(IDLE).reported_after(Some(WORKING)), WORKING);
    assert_eq!(held(DONE).reported_after(Some(blocked)), WORKING);
    assert_eq!(held(IDLE).reported_after(Some(DONE)), IDLE, "from rest");
    assert_eq!(held(IDLE).reported_after(None), IDLE, "no word yet");
    assert_eq!(held(blocked).reported_after(Some(WORKING)), blocked);
    assert_eq!(
        held(ProgramState::Unknown).reported_after(Some(WORKING)),
        ProgramState::Unknown
    );
    assert_eq!(held(IDLE).reported(), IDLE, "alone it says the frame");
}

/// The glue's reading of a turn the TUI saw (`Shared`'s seen book, here a test's box scope):
/// it is held ([`turns::seen_elsewhere`]), never read as the autopilot's own
/// ([`turns::running_elsewhere`]), and the displayed thread's own is left out.
#[test]
fn a_seen_turn_is_read_from_the_seen_book_alone() {
    use codex_alab_core::session::Shared;
    assert!(!turns::seen_elsewhere(None), "no shared state, no book");
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let _scope = crate::boxes::scoped(shared);
    turns::seen("sub", "s1", true);
    assert!(turns::seen_elsewhere(Some("root")));
    assert!(turns::seen_elsewhere(None));
    assert!(!turns::seen_elsewhere(Some("sub")), "its own turn");
    assert!(
        !turns::running_elsewhere(Some("root")),
        "never the autopilot's book"
    );
    turns::seen("sub", "s1", false);
    assert!(!turns::seen_elsewhere(None));
}

/// The glue's reading of another thread's turn: `Shared`'s book (here a test's box scope), the
/// displayed thread's own turn left out; none outside the codex binary and a scope.
#[test]
fn another_threads_turn_is_read_from_the_shared_book() {
    use codex_alab_core::session::Shared;
    assert!(!turns::running_elsewhere(None), "no shared state, no book");
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    let _scope = crate::boxes::scoped(shared);
    assert!(!turns::running_elsewhere(Some("a")));
    assert!(shared.turn_started("a", "t1"));
    assert!(!turns::running_elsewhere(Some("a")), "its own turn");
    assert!(turns::running_elsewhere(Some("b")));
    assert!(turns::running_elsewhere(None));
    shared.turn_ended("a", "t1");
    assert!(!turns::running_elsewhere(Some("b")));
    assert!(shared.turn_started("a", "t2"));
    shared.thread_ended("a");
    assert!(
        !turns::running_elsewhere(Some("b")),
        "a stopped thread runs nothing"
    );
}

/// NEGATIVE CONTROL for O1: a turn that runs while the servers still start is work, not the
/// startup (`startup_only` false), and is reported and counted.
#[test]
fn a_turn_during_mcp_startup_is_a_turn() {
    let steps = [
        Step::Frame(starting_mcp(IDLE)),
        Step::Starts { asked: true },
        Step::Frame(shows(WORKING)),
        Step::Frame(shows(DONE)),
    ];
    let bytes = written(&steps);
    assert_eq!(turns::aterm_turns(&bytes), 1);
    assert!(bytes.ends_with(&turns::sequence(DONE).unwrap()));
}

/// O2: A QUEUED MESSAGE SENT THE MOMENT THE LAST TURN ENDS begins its own turn. Upstream's
/// report stood at `working` across both turns and wrote nothing at the second's start (RED:
/// one turn counted); its start now writes `idle`, then `working`.
#[test]
fn a_queued_turn_sent_as_the_last_ends_begins_its_own() {
    let steps = [
        Step::Frame(shows(IDLE)),
        Step::Starts { asked: true },
        Step::Frame(shows(WORKING)),
        // The first turn ends and the queued message is sent; a frame shows it waiting.
        Step::Frame(submitted()),
        Step::Starts { asked: true },
        Step::Frame(shows(WORKING)),
        Step::Frame(shows(DONE)),
    ];
    assert_eq!(turns::aterm_turns(&upstream(&steps)), 1, "RED: upstream");
    let bytes = written(&steps);
    assert_eq!(turns::aterm_turns(&bytes), 2);
    let edge = [IDLE, WORKING]
        .map(|w| turns::sequence(w).unwrap())
        .concat();
    assert!(bytes.contains(&format!("{}{edge}", turns::sequence(WORKING).unwrap())));
}

/// ONE BEGIN PER SUBMITTED TURN: a frame drawn between the person's submission and the
/// server's start already turns the report from rest to `working` (that turn's begin), so the
/// start writes no second edge; without a frame between, the start writes the one edge.
/// NEGATIVE CONTROL for the start's edge: counted twice, the person's one turn would read as
/// two.
#[test]
fn a_frame_between_a_submission_and_its_start_shows_its_one_begin() {
    let with_frame = [
        Step::Frame(shows(IDLE)),
        Step::Frame(submitted()),
        Step::Starts { asked: true },
        Step::Frame(shows(WORKING)),
        Step::Frame(shows(DONE)),
    ];
    let bytes = written(&with_frame);
    assert_eq!(turns::aterm_turns(&bytes), 1, "{bytes:?}");
    assert_eq!(
        turns::aterm_turns(&upstream(&with_frame)),
        1,
        "upstream counted this one right, and still does"
    );
    let without = [
        Step::Frame(shows(IDLE)),
        Step::Starts { asked: true },
        Step::Frame(shows(WORKING)),
        Step::Frame(shows(DONE)),
    ];
    assert_eq!(turns::aterm_turns(&written(&without)), 1);
    // The begin a frame showed is taken by the submitted turn only: one the core began first
    // (a goal's continuation) leaves it to the submitted turn, which writes nothing more.
    let core_first = [
        Step::Frame(shows(IDLE)),
        Step::Frame(submitted()),
        Step::Starts { asked: false },
        Step::Starts { asked: true },
        Step::Frame(shows(WORKING)),
    ];
    assert_eq!(turns::aterm_turns(&written(&core_first)), 1);
    // A submission whose turn never started, back at rest, leaves nothing standing: the next
    // submitted turn under standing work writes its own edge.
    let never = [
        Step::Frame(shows(IDLE)),
        Step::Frame(submitted()),
        Step::Frame(shows(IDLE)),
        Step::Starts { asked: false },
        Step::Frame(shows(WORKING)),
        Step::Starts { asked: true },
        Step::Frame(shows(WORKING)),
    ];
    assert_eq!(turns::aterm_turns(&written(&never)), 3);
}

/// O2 under work running on: a message sent while a background terminal (or an active goal
/// between turns) keeps the report at `working` begins its own turn (RED: upstream).
#[test]
fn a_message_sent_under_standing_work_begins_its_own() {
    let steps = [
        Step::Frame(shows(IDLE)),
        Step::Starts { asked: true },
        Step::Frame(shows(WORKING)),
        // The turn ended; a background terminal keeps the report at `working`.
        Step::Frame(shows(WORKING)),
        Step::Starts { asked: true },
        Step::Frame(shows(WORKING)),
    ];
    assert_eq!(turns::aterm_turns(&upstream(&steps)), 1, "RED: upstream");
    assert_eq!(turns::aterm_turns(&written(&steps)), 2);
}

/// NEGATIVE CONTROLS for O2: a turn the TUI did not submit begun under standing work (a goal's
/// continuation, an autopilot act through the app-server) writes nothing: the work runs on,
/// and a landed task's own wake keeps its landing. A handoff mid-turn (`clear`, the report
/// sent again) begins none either, and the frame after it does send the report again.
#[test]
fn work_running_on_and_a_handoff_begin_none() {
    let steps = [
        Step::Frame(shows(IDLE)),
        Step::Starts { asked: true },
        Step::Frame(shows(WORKING)),
        Step::Starts { asked: false },
        Step::Frame(shows(WORKING)),
        Step::Handoff,
        Step::Frame(shows(WORKING)),
        Step::Frame(shows(DONE)),
    ];
    let bytes = written(&steps);
    assert_eq!(turns::aterm_turns(&bytes), 1);
    let working = turns::sequence(WORKING).unwrap();
    assert!(
        bytes.contains(&format!("{}{working}", turns::CLEAR)),
        "sent again after the handoff: {bytes:?}"
    );
    assert_eq!(bytes.matches(&working).count(), 2, "{bytes:?}");
}

/// A TURN BEGUN AND ENDED BETWEEN TWO FRAMES (a message that fails at once) is counted: its
/// start writes `working`, whoever began it, as a frame would have shown it (RED: upstream's
/// frames saw `done`, then `error`, and nothing between).
#[test]
fn a_turn_begun_and_ended_between_frames_is_counted() {
    let steps = [
        Step::Frame(shows(IDLE)),
        Step::Starts { asked: true },
        Step::Frame(shows(WORKING)),
        Step::Frame(shows(DONE)),
        Step::Starts { asked: false },
        Step::Frame(shows(ProgramState::Error)),
    ];
    assert_eq!(turns::aterm_turns(&upstream(&steps)), 1, "RED: upstream");
    assert_eq!(turns::aterm_turns(&written(&steps)), 2);
}

/// THE FIRST WORD OF A PROCESS: before it wrote a report, aterm's last word is a
/// predecessor's (one killed mid-turn left `working`, its exit's `clear` looked past), so a
/// turn's start writes `idle` first, and the turn is counted either way.
#[test]
fn a_turn_a_process_starts_before_any_report_is_counted() {
    let predecessor = format!("{}{}", turns::sequence(WORKING).unwrap(), turns::CLEAR);
    let ours = written(&[Step::Starts { asked: true }, Step::Frame(shows(WORKING))]);
    assert_eq!(
        ours,
        [IDLE, WORKING]
            .map(|w| turns::sequence(w).unwrap())
            .concat()
    );
    let before = turns::aterm_turns(&predecessor);
    assert_eq!(
        turns::aterm_turns(&format!("{predecessor}{ours}")),
        before + 1
    );
}

/// AN EDGE GOES TO THE TERMINAL IN ONE WRITE (parity-3 review, F5): a turn begun under a
/// standing `working` writes `idle` then `working` as one batch, so aterm's reader parses both
/// in one read and no sweep between them shows `idle` alone (a turn end, its dot, a `program
/// state=idle` mark, for a turn that runs on); a turn begun from rest writes its one word, and
/// so does each frame. NEGATIVE CONTROL: a work running on writes no batch at all.
#[test]
fn an_edge_goes_to_the_terminal_in_one_write() {
    let mut report = Report::new();
    let mut batches: Vec<Vec<ProgramState>> = Vec::new();
    let mut put = |words: &[ProgramState]| -> io::Result<()> {
        batches.push(words.to_vec());
        Ok(())
    };
    report.frame(shows(IDLE), &mut put).expect("written");
    report.starting(/*asked*/ true, &mut put).expect("written");
    report.frame(shows(WORKING), &mut put).expect("written");
    report.starting(/*asked*/ true, &mut put).expect("written");
    report.starting(/*asked*/ false, &mut put).expect("written");
    assert_eq!(
        batches,
        vec![
            vec![IDLE],
            vec![WORKING],
            vec![WORKING],
            vec![IDLE, WORKING],
        ]
    );
    // Through the tape, as upstream's writer and the glue put it on the wire.
    let wire = written(&[
        Step::Frame(shows(WORKING)),
        Step::Starts { asked: true },
        Step::Frame(shows(WORKING)),
    ]);
    let edge = format!(
        "{}{}",
        turns::sequence(IDLE).unwrap(),
        turns::sequence(WORKING).unwrap()
    );
    assert_eq!(wire, format!("{}{edge}", turns::sequence(WORKING).unwrap()));
    assert_eq!(turns::aterm_turns(&wire), 2);
}

/// A FAILED WRITE leaves the last word unknown (aterm may have read the batch, a part of it
/// or none), so the next turn's start writes its edge whole, and aterm counts that turn.
#[test]
fn a_failed_write_leaves_the_last_word_unknown() {
    let mut report = Report::new();
    let mut wire = String::new();
    let mut put = |words: &[ProgramState]| -> io::Result<()> {
        for word in words {
            wire.push_str(&turns::sequence(*word).unwrap());
        }
        Ok(())
    };
    report.frame(shows(WORKING), &mut put).expect("written");
    let refused = report.starting(/*asked*/ true, |_| Err(io::Error::other("refused")));
    assert!(refused.is_err());
    assert_eq!(report.last(), None);
    report.starting(/*asked*/ true, &mut put).expect("written");
    assert_eq!(report.last(), Some(WORKING));
    assert_eq!(turns::aterm_turns(&wire), 2);
}

/// The startup is left out only where it is the one reason upstream says `working`, and a
/// box (`blocked`) is reported as it is.
#[test]
fn only_the_startup_is_left_out() {
    assert_eq!(starting_mcp(IDLE).reported(), IDLE);
    assert_eq!(starting_mcp(DONE).reported(), DONE);
    assert_eq!(shows(WORKING).reported(), WORKING);
    let blocked = ProgramState::Blocked(BlockedKind::Permission);
    assert_eq!(
        Frame {
            state: blocked,
            startup_only: true,
            rest: IDLE,
            submitted: false,
            elsewhere: false,
            held: false,
        }
        .reported(),
        blocked
    );
    assert!(turns::in_turn(blocked) && turns::in_turn(WORKING));
    assert!(
        ![IDLE, DONE, ProgramState::Error]
            .into_iter()
            .any(turns::in_turn)
    );
}
