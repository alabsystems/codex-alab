// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The session record, process identity, the resume rewrite and the record's endings.

use crate::identity;
use crate::record;
use crate::record::Record;
use crate::record::Status;
use crate::resume_argv;
use crate::session;
use crate::session::Ending;
use crate::session::Frame;
use crate::session::ProgramState;
use crate::session::Writer;
use clap::Args;
use clap::CommandFactory;
use clap::Parser;
use clap::Subcommand;
use codex_alab_core::session::BlockedKind;
use codex_alab_core::session::Launch;
use codex_alab_core::session::ProcessIdentity;
use codex_alab_core::session::Shared;
use pretty_assertions::assert_eq;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

const THREAD: &str = "019a0000-0000-7000-8000-000000000001";

// ---- a clap definition shaped like codex's MultitoolCli (the real one: cli/src/main.rs's
// ALAB(life) test glue, codex-rs/alab-tui/glue/cli_resume_tests.rs)

#[derive(Parser)]
#[command(bin_name = "codex", subcommand_negates_reqs = true)]
struct FakeCli {
    #[arg(short = 'c', long = "config", action = clap::ArgAction::Append, global = true)]
    raw_overrides: Vec<String>,
    #[arg(long = "enable", action = clap::ArgAction::Append, global = true)]
    enable: Vec<String>,
    #[clap(flatten)]
    interactive: FakeTui,
    #[command(subcommand)]
    subcommand: Option<FakeSub>,
}

#[derive(Args)]
struct FakeTui {
    #[arg(value_name = "PROMPT")]
    prompt: Option<String>,
    #[arg(long = "image", short = 'i', value_delimiter = ',', num_args = 1..)]
    images: Vec<PathBuf>,
    #[arg(long, short = 'm')]
    model: Option<String>,
    #[arg(long = "dangerously-bypass-approvals-and-sandbox", alias = "yolo")]
    bypass: bool,
    #[arg(long = "cd", short = 'C')]
    cwd: Option<PathBuf>,
    #[arg(long = "search")]
    search: bool,
}

#[derive(Subcommand)]
enum FakeSub {
    Resume(FakeResume),
    Fork(FakeFork),
    Exec { prompt: Option<String> },
    Agents,
}

#[derive(Args)]
struct FakeResume {
    session_id: Option<String>,
    #[arg(long)]
    last: bool,
    #[arg(long)]
    all: bool,
    #[arg(long)]
    include_non_interactive: bool,
    #[clap(flatten)]
    tui: FakeTui,
}

#[derive(Args)]
struct FakeFork {
    session_id: Option<String>,
    #[arg(long)]
    last: bool,
    #[arg(long)]
    all: bool,
    #[clap(flatten)]
    tui: FakeTui,
}

fn rebuild(args: &[&str]) -> Option<Vec<String>> {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    resume_argv::rebuild(FakeCli::command(), &args, "/twin/codex", THREAD)
}

fn resumed(tail: &[&str]) -> Option<Vec<String>> {
    Some(
        std::iter::once("/twin/codex")
            .chain(tail.iter().copied())
            .map(str::to_owned)
            .collect(),
    )
}

#[test]
fn the_prompt_goes_and_root_flags_stay_in_place() {
    assert_eq!(
        rebuild(&["codex", "-m", "o3", "fix it", "--search"]),
        resumed(&["-m", "o3", "--search", "resume", THREAD])
    );
}

#[test]
fn images_go_with_every_value() {
    assert_eq!(
        rebuild(&["codex", "--yolo", "-i", "a.png,b.png", "c.png", "-c", "x=1"]),
        resumed(&["--yolo", "-c", "x=1", "resume", THREAD])
    );
    assert_eq!(
        rebuild(&["codex", "--image=a.png", "--model=o3", "p"]),
        resumed(&["--model=o3", "resume", THREAD])
    );
}

#[test]
fn attached_short_values_stay_verbatim() {
    assert_eq!(
        rebuild(&["codex", "-mo3", "-Csub"]),
        resumed(&["-mo3", "-Csub", "resume", THREAD])
    );
}

#[test]
fn resume_keeps_its_flags_and_drops_its_selectors() {
    assert_eq!(
        rebuild(&[
            "codex", "-c", "a=1", "resume", "--last", "-m", "o3", "--all", "--search",
        ]),
        resumed(&["-c", "a=1", "resume", "-m", "o3", "--search", THREAD])
    );
    assert_eq!(
        rebuild(&[
            "codex",
            "resume",
            "--include-non-interactive",
            "-c",
            "b=2",
            "OLD",
            "hi"
        ]),
        resumed(&["resume", "-c", "b=2", THREAD])
    );
}

#[test]
fn a_fork_resumes_the_forked_thread() {
    assert_eq!(
        rebuild(&["codex", "--enable", "x", "fork", "OLD", "--search", "go on"]),
        resumed(&["--enable", "x", "resume", "--search", THREAD])
    );
}

#[test]
fn everything_after_a_double_dash_is_a_prompt() {
    assert_eq!(
        rebuild(&["codex", "-m", "o3", "--", "--not-a-flag"]),
        resumed(&["-m", "o3", "resume", THREAD])
    );
}

#[test]
fn other_subcommands_and_unparsable_launches_are_refused() {
    assert_eq!(rebuild(&["codex", "exec", "x"]), None);
    assert_eq!(rebuild(&["codex", "agents"]), None);
    assert_eq!(rebuild(&["codex", "--no-such-flag"]), None);
    use std::os::unix::ffi::OsStringExt;
    let args = vec![
        OsString::from("codex"),
        OsString::from_vec(vec![b'-', b'm', 0xff]),
    ];
    assert_eq!(
        resume_argv::rebuild(FakeCli::command(), &args, "/twin/codex", THREAD),
        None
    );
}

// ---- process identity

#[test]
fn this_process_is_live_under_its_own_identity() {
    let own = identity::current().unwrap_or_else(|err| panic!("identity: {err}"));
    assert!(identity::is_live(std::process::id(), Some(&own)));
    let mut other = own;
    match &mut other {
        ProcessIdentity::MacOs { unique_id, .. } => *unique_id += 1,
        ProcessIdentity::Linux { start_ticks, .. } => *start_ticks += 1,
    }
    assert!(
        !identity::is_live(std::process::id(), Some(&other)),
        "a reused pid is not the recorded process"
    );
}

#[test]
fn exited_processes_are_dead_zombies_included() {
    let mut child = std::process::Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .spawn()
        .unwrap_or_else(|err| panic!("spawn: {err}"));
    let pid = child.id();
    // Unreaped, it is a zombie: dead to a reader.
    let deadline = Instant::now() + Duration::from_secs(60);
    while identity::of_pid(pid).is_ok() {
        assert!(Instant::now() < deadline, "child never exited");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!identity::is_live(pid, None));
    child.wait().unwrap_or_else(|err| panic!("wait: {err}"));
    assert!(!identity::is_live(pid, None));
}

/// The record's procStart is `ps -o lstart`'s format (aterm parses it). ps itself is setuid,
/// which the gate's sandbox refuses to run, so date(1) renders the same start second in the
/// same format (`%a %b %e %T %Y`, C locale).
#[cfg(target_os = "macos")]
#[test]
fn proc_start_is_ps_lstart() {
    let own = identity::current().unwrap_or_else(|err| panic!("identity: {err}"));
    let ours = identity::proc_start(&own).unwrap_or_else(|| panic!("proc_start"));
    let codex_alab_core::session::ProcessIdentity::MacOs { start_seconds, .. } = own else {
        panic!("a macOS identity: {own:?}");
    };
    let date = std::process::Command::new("/bin/date")
        .args(["-r", &start_seconds.to_string(), "+%a %b %e %T %Y"])
        .env("LC_ALL", "C")
        .output()
        .unwrap_or_else(|err| panic!("date: {err}"));
    let date = String::from_utf8_lossy(&date.stdout);
    let normalize = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(normalize(&ours), normalize(&date));
}

#[test]
fn stat_parsing_survives_parentheses_in_comm() {
    let stat = b"42 (we)ird ) name) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 777 19";
    let (state, start) = identity::parse_stat(stat).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!((state.as_str(), start), ("S", 777));
}

// ---- the record

fn launch(pid: u32, aterm: bool) -> Launch {
    let mut vars = vec![(OsString::from("CODEX_HOME"), OsString::from("/h/.codex"))];
    if aterm {
        vars.push(("ATERM_PARENT_SESSION_ID".into(), "fake".into()));
    }
    Launch {
        args: ["codex", "-m", "o3", "hello"].map(OsString::from).to_vec(),
        vars,
        cwd: Some(PathBuf::from("/work")),
        current_exe: Some(PathBuf::from("/store/b1/bin/codex")),
        exe_id: None,
        argv0: Some(PathBuf::from("/twin/codex")),
        pid,
        identity: identity::current().ok(),
        proc_start: Some("Thu Oct 16 08:00:00 2026".into()),
        started_at_ms: codex_alab_core::session::now_ms(),
    }
}

fn shared_with(launch: Launch) -> &'static Shared {
    let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
    shared.set_launch(launch);
    shared
}

fn frame<'a>(rollout: Option<&'a Path>, state: ProgramState, home: &'a Path) -> Frame<'a> {
    Frame {
        primary_thread: Some(THREAD),
        displayed_thread: Some(THREAD),
        rollout_path: rollout,
        cwd: Path::new("/work"),
        model: "o3",
        codex_home: home,
        embedded: true,
        state,
        draft_empty: true,
        queued_input: false,
        popup_open: false,
        editor_open: false,
        pending_user_turn: false,
    }
}

fn read(path: &Path) -> serde_json::Value {
    let bytes = std::fs::read(path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|err| panic!("{err}"))
}

#[test]
fn the_record_is_schema_v1_in_contract_order() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let rollout = home.path().join("rollout.jsonl");
    std::fs::write(&rollout, "{}\n").unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(launch(4242, true));
    shared.observe(
        &frame(
            Some(&rollout),
            ProgramState::Blocked(BlockedKind::Permission),
            home.path(),
        ),
        1000,
    );
    shared.human_input(1500);
    let dir = record::sessions_dir(home.path());
    let writer = Writer::new(shared, dir.clone(), Duration::ZERO, Some(FakeCli::command));
    writer.prepare();
    assert_eq!(writer.write_now().ok(), Some(true));
    let value = read(&dir.join("4242.json"));
    let raw = std::fs::read_to_string(dir.join("4242.json")).unwrap_or_else(|err| panic!("{err}"));
    let order = [
        "v",
        "pid",
        "procStart",
        "identity",
        "sessionId",
        "displayedThreadId",
        "cwd",
        "version",
        "exe",
        "launchArgv0",
        "execGeneration",
        "resumeArgv",
        "env",
        "status",
        "exit",
        "state",
        "blockedKind",
        "draftEmpty",
        "lastHumanInputAt",
        "autopilot",
        "claim",
        "statusUpdatedAt",
        "updatedAt",
        "kind",
        "entrypoint",
        "carriedOn",
    ];
    assert_eq!(
        value.as_object().map(serde_json::Map::len),
        Some(order.len()),
        "exactly the v1 fields"
    );
    let positions: Vec<usize> = order
        .iter()
        .map(|key| {
            raw.find(&format!("\"{key}\":"))
                .unwrap_or_else(|| panic!("{key} missing"))
        })
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "fields in CONTRACT §4's order: {raw}"
    );
    assert_eq!(value["v"], 1);
    assert_eq!(value["sessionId"], THREAD);
    assert_eq!(value["status"], "waiting");
    assert_eq!(value["state"], "blocked");
    assert_eq!(value["blockedKind"], "permission");
    assert_eq!(value["exe"], "/store/b1/bin/codex");
    assert_eq!(value["launchArgv0"], "/twin/codex");
    assert_eq!(
        value["resumeArgv"],
        serde_json::json!(["/twin/codex", "-m", "o3", "resume", THREAD])
    );
    assert_eq!(value["env"], serde_json::json!({"CODEX_HOME": "/h/.codex"}));
    assert_eq!(value["exit"], serde_json::Value::Null);
    assert_eq!(value["lastHumanInputAt"], 1500);
    assert_eq!(value["statusUpdatedAt"], 1000);
    assert_eq!(value["autopilot"], "off");
    assert_eq!(value["claim"], serde_json::Value::Null);
    assert_eq!(value["kind"], "interactive");
    assert_eq!(value["entrypoint"], "cli");
    assert_eq!(value["carriedOn"], 0);
    let identity = value["identity"]
        .as_object()
        .unwrap_or_else(|| panic!("identity"));
    assert!(identity.contains_key("bootId"));
    assert!(identity.contains_key("uniqueId") || identity.contains_key("startTicks"));
}

#[test]
fn the_record_is_private_and_written_whole() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(launch(4243, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    let dir = record::sessions_dir(home.path());
    let writer = Writer::new(shared, dir.clone(), Duration::ZERO, None);
    writer.prepare();
    assert_eq!(writer.write_now().ok(), Some(true));
    assert_eq!(
        writer.write_now().ok(),
        Some(false),
        "nothing new, nothing written"
    );
    let mode = |path: &Path| {
        std::fs::metadata(path)
            .unwrap_or_else(|err| panic!("{err}"))
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&dir.join("4243.json")), 0o600);
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("{err}"))
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["4243.json"], "no temporary left behind");
    let record = record::read(&dir.join("4243.json")).unwrap_or_else(|| panic!("parses"));
    assert_eq!(record.status, Status::Idle);
    assert_eq!(record.resume_argv, None, "no rollout yet, no resume");
}

#[test]
fn resume_argv_waits_for_the_rollout() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let rollout = home.path().join("rollout.jsonl");
    std::fs::write(&rollout, "").unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(launch(4244, true));
    shared.observe(
        &frame(Some(&rollout), ProgramState::Working, home.path()),
        1,
    );
    let dir = record::sessions_dir(home.path());
    let writer = Writer::new(shared, dir.clone(), Duration::ZERO, Some(FakeCli::command));
    writer.prepare();
    let _ = writer.write_now();
    let record = record::read(&dir.join("4244.json")).unwrap_or_else(|| panic!("parses"));
    assert_eq!(
        record.resume_argv, None,
        "an empty rollout cannot be resumed"
    );
    // Negative control: a record without a line tells the claim nothing.
    assert_eq!(shared.resume_lines(), 0);
    std::fs::write(&rollout, "{}\n").unwrap_or_else(|err| panic!("{err}"));
    let _ = writer.write_now();
    let record = record::read(&dir.join("4244.json")).unwrap_or_else(|| panic!("parses"));
    assert_eq!(record.resume_argv, resumed(&["-m", "o3", "resume", THREAD]));
    // The line aterm's copy (read at the claim) lacks: the claim is to be renewed.
    assert_eq!(shared.resume_lines(), 1);
    // A later write of the same line is no news.
    shared.observe(&frame(Some(&rollout), ProgramState::Idle, home.path()), 2);
    let _ = writer.write_now();
    assert_eq!(shared.resume_lines(), 1);
}

#[test]
fn writes_coalesce_to_one_per_interval() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(launch(4245, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    let dir = record::sessions_dir(home.path());
    let interval = Duration::from_millis(200);
    let writer: &'static Writer =
        Box::leak(Box::new(Writer::new(shared, dir.clone(), interval, None)));
    let started = Instant::now();
    writer.spawn();
    let mut last = 0;
    for stamp in 1..=40u64 {
        shared.human_input(10_000 + stamp);
        last = 10_000 + stamp;
        std::thread::sleep(Duration::from_millis(15));
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(record) = record::read(&dir.join("4245.json"))
            && record.last_human_input_at == Some(last)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the last change never reached the record"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let elapsed = started.elapsed();
    // One per interval, plus the first write and the first key's write-through (the draft
    // a key may have begun is not coalesced).
    let allowed = u64::try_from(elapsed.as_millis() / interval.as_millis()).unwrap_or(0) + 2;
    assert!(
        writer.writes() <= allowed,
        "{} writes in {elapsed:?} at one per {interval:?}",
        writer.writes()
    );
    assert!(writer.writes() >= 2, "it wrote more than the first change");
    writer
        .finalize(&Ending::Remove)
        .unwrap_or_else(|err| panic!("{err}"));
}

#[test]
fn endings_follow_the_contract() {
    // The terminal going away is no person's /exit: kept, as every signal is.
    assert_eq!(
        Ending::of(Some(libc::SIGHUP), false, false),
        Ending::Signalled(libc::SIGHUP)
    );
    assert_eq!(
        Ending::of(Some(libc::SIGTERM), false, false),
        Ending::Signalled(libc::SIGTERM)
    );
    assert_eq!(
        Ending::of(Some(libc::SIGINT), true, true),
        Ending::Signalled(libc::SIGINT)
    );
    assert_eq!(Ending::of(None, true, false), Ending::Upgrading);
    assert_eq!(
        Ending::of(None, false, true),
        Ending::Exited("fatal".into())
    );
    assert_eq!(Ending::of(None, false, false), Ending::Remove);
    // A fatal exit during an upgrade's teardown is a crash, never an exec.
    assert_eq!(Ending::of(None, true, true), Ending::Exited("fatal".into()));
}

fn finalized(pid: u32, ending: &Ending) -> (tempfile::TempDir, PathBuf, Option<Record>, u64) {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(launch(pid, true));
    shared.observe(&frame(None, ProgramState::Working, home.path()), 1);
    let dir = record::sessions_dir(home.path());
    let writer = Writer::new(shared, dir.clone(), Duration::ZERO, None);
    writer.prepare();
    let _ = writer.write_now();
    writer
        .finalize(ending)
        .unwrap_or_else(|err| panic!("{err}"));
    shared.human_input(99);
    let after = writer.write_now().ok().map_or(0, u64::from);
    let path = dir.join(format!("{pid}.json"));
    let record = record::read(&path);
    (home, path, record, after)
}

#[test]
fn an_orderly_exit_removes_the_record() {
    let (_home, path, record, after) = finalized(4246, &Ending::Remove);
    assert_eq!(record, None);
    assert!(!path.exists());
    assert_eq!(after, 0, "nothing is written after the end");
}

#[test]
fn a_signal_keeps_the_record_with_the_status_it_found() {
    for signal in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
        let (_home, _, record, after) = finalized(4247, &Ending::Signalled(signal));
        let record = record.unwrap_or_else(|| panic!("kept"));
        assert_eq!(record.status, Status::Busy, "it died working");
        assert_eq!(record.exit, Some(format!("signal:{signal}")));
        assert_eq!(after, 0);
    }
    // A fatal exit is no signal: `exited`, as before.
    let (_home, _, record, _) = finalized(4249, &Ending::Exited("fatal".into()));
    let record = record.unwrap_or_else(|| panic!("kept"));
    assert_eq!(record.status, Status::Exited);
    assert_eq!(record.exit.as_deref(), Some("fatal"));
}

#[test]
fn a_hangup_mid_turn_keeps_the_record_busy_through_the_teardown() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(launch(4254, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    let dir = record::sessions_dir(home.path());
    let writer = Writer::new(shared, dir.clone(), Duration::ZERO, None);
    writer.prepare();
    assert!(shared.turn_started(THREAD, "turn-1"));
    let _ = writer.write_now();
    // aterm went away: the signal comes while the turn runs ...
    writer.hold_status();
    // ... and the /exit teardown aborts that turn before the record is ended.
    shared.turn_ended(THREAD, "turn-1");
    let _ = writer.write_now();
    writer
        .finalize(&Ending::Signalled(libc::SIGHUP))
        .unwrap_or_else(|err| panic!("{err}"));
    let record = record::read(&dir.join("4254.json")).unwrap_or_else(|| panic!("kept"));
    assert_eq!(
        record.status,
        Status::Busy,
        "the witness of a session that died working"
    );
    assert_eq!(record.exit.as_deref(), Some("signal:1"));

    // Negative control: a session at rest when the terminal went away stays at rest (a
    // relaunch owes it nothing).
    let shared = shared_with(launch(4255, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    let writer = Writer::new(shared, dir.clone(), Duration::ZERO, None);
    writer.prepare();
    writer.hold_status();
    assert!(shared.turn_started(THREAD, "turn-late"));
    writer
        .finalize(&Ending::Signalled(libc::SIGHUP))
        .unwrap_or_else(|err| panic!("{err}"));
    let record = record::read(&dir.join("4255.json")).unwrap_or_else(|| panic!("kept"));
    assert_eq!(record.status, Status::Idle);
    assert_eq!(record.exit.as_deref(), Some("signal:1"));
}

#[test]
fn the_upgrade_keeps_the_record_one_generation_on() {
    let (_home, _, record, _) = finalized(4248, &Ending::Upgrading);
    let record = record.unwrap_or_else(|| panic!("kept"));
    assert_eq!(record.status, Status::Upgrading);
    assert_eq!(record.exec_generation, 1);
    assert_eq!(record.exit, None);
}

#[test]
fn a_re_exec_inherits_its_generation_from_its_own_record() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let pid = std::process::id();
    let shared = shared_with(launch(pid, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    let dir = record::sessions_dir(home.path());
    let before = Writer::new(shared, dir.clone(), Duration::ZERO, None);
    before.prepare();
    let _ = before.write_now();
    before
        .finalize(&Ending::Upgrading)
        .unwrap_or_else(|err| panic!("{err}"));
    // The new image: same pid, same identity.
    let shared = shared_with(launch(pid, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 2);
    let after = Writer::new(shared, dir.clone(), Duration::ZERO, None);
    after.prepare();
    let _ = after.write_now();
    let record = record::read(&dir.join(format!("{pid}.json"))).unwrap_or_else(|| panic!("kept"));
    assert_eq!(record.exec_generation, 1);
    assert_eq!(record.status, Status::Idle);
    let inherited = shared.inherited().unwrap_or_else(|| panic!("inherited"));
    assert_eq!(inherited.status.as_deref(), Some("upgrading"));
    assert_eq!(inherited.exec_generation, 1);
}

/// Finding 4 (review of cf084e3b7): the live upgrade's execve is the same process, so its
/// carry-on chain goes on: the new image starts from the old one's `carriedOn`, not 0.
#[test]
fn a_re_exec_keeps_its_carry_on_chain() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let pid = std::process::id();
    let dir = record::sessions_dir(home.path());
    for chain in [1, 0] {
        let shared = shared_with(launch(pid, true));
        shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
        shared.set_carried_on(chain);
        let before = Writer::new(shared, dir.clone(), Duration::ZERO, None);
        before.prepare();
        let _ = before.write_now();
        before
            .finalize(&Ending::Upgrading)
            .unwrap_or_else(|err| panic!("{err}"));
        // The new image: same pid, same identity, a fresh shared state.
        let shared = shared_with(launch(pid, true));
        shared.observe(&frame(None, ProgramState::Idle, home.path()), 2);
        let after = Writer::new(shared, dir.clone(), Duration::ZERO, None);
        after.prepare();
        assert_eq!(shared.carried_on(), chain, "{chain}");
        let _ = after.write_now();
        let record =
            record::read(&dir.join(format!("{pid}.json"))).unwrap_or_else(|| panic!("kept"));
        assert_eq!(record.carried_on, chain, "{chain}");
        assert!(record::remove(&dir, pid).is_ok());
    }
    // Negative control: another process's record under this pid (another identity) is no
    // previous image of this one, and hands no chain on.
    let shared = shared_with(launch(pid, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    shared.set_carried_on(1);
    let before = Writer::new(shared, dir.clone(), Duration::ZERO, None);
    before.prepare();
    let _ = before.write_now();
    let path = dir.join(format!("{pid}.json"));
    let mut value = read(&path);
    value["identity"] = serde_json::Value::Null;
    std::fs::write(
        &path,
        serde_json::to_vec(&value).unwrap_or_else(|err| panic!("{err}")),
    )
    .unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(launch(pid, true));
    let after = Writer::new(shared, dir, Duration::ZERO, None);
    after.prepare();
    assert_eq!(shared.carried_on(), 0);
    assert!(shared.inherited().is_none());
}

fn stale_record(dir: &Path, pid: u32, identity: Option<&ProcessIdentity>, updated_at: u64) {
    let shared = shared_with(launch(pid, true));
    let writer = Writer::new(shared, dir.to_path_buf(), Duration::ZERO, None);
    let _ = writer.write_now();
    let path = dir.join(format!("{pid}.json"));
    let mut value = read(&path);
    value["updatedAt"] = updated_at.into();
    value["identity"] = match identity {
        Some(identity) => serde_json::to_value(record::Identity::from(identity))
            .unwrap_or_else(|err| panic!("{err}")),
        None => serde_json::Value::Null,
    };
    std::fs::write(
        &path,
        serde_json::to_vec(&value).unwrap_or_else(|err| panic!("{err}")),
    )
    .unwrap_or_else(|err| panic!("{err}"));
}

#[test]
fn the_start_sweep_removes_only_dead_identities_from_before() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let dir = record::sessions_dir(home.path());
    record::ensure_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
    let mut child = std::process::Command::new("/bin/sleep")
        .arg("60")
        .spawn()
        .unwrap_or_else(|err| panic!("spawn: {err}"));
    let live = child.id();
    let live_identity = identity::of_pid(live).unwrap_or_else(|err| panic!("{err}"));
    let mut reused = live_identity.clone();
    match &mut reused {
        ProcessIdentity::MacOs { unique_id, .. } => *unique_id += 1,
        ProcessIdentity::Linux { start_ticks, .. } => *start_ticks += 1,
    }
    let started = 10_000;
    // pid 999_999 does not exist (above macOS's pid_max of 99_998).
    stale_record(&dir, 999_999, None, started - 1);
    stale_record(&dir, 999_998, None, started + 1);
    stale_record(&dir, live, Some(&live_identity), started - 1);
    std::fs::write(dir.join("999997.json.abc.tmp"), "partial")
        .unwrap_or_else(|err| panic!("{err}"));
    std::fs::write(dir.join("notes.txt"), "keep").unwrap_or_else(|err| panic!("{err}"));
    let own = std::process::id();
    stale_record(&dir, own, None, started - 1);

    // The temporary's mtime is now, so it is swept only by a later start.
    let removed = record::sweep(&dir, own, started, identity::is_live);
    assert_eq!(removed, 1);
    assert!(!dir.join("999999.json").exists(), "dead and older: removed");
    assert!(
        dir.join("999998.json").exists(),
        "written after this start: kept"
    );
    assert!(
        dir.join(format!("{live}.json")).exists(),
        "live identity: kept"
    );
    assert!(
        dir.join(format!("{own}.json")).exists(),
        "own pid: never swept"
    );
    assert!(dir.join("notes.txt").exists());

    // The same live pid under another identity is a reused pid: dead.
    stale_record(&dir, live, Some(&reused), started - 1);
    let later = codex_alab_core::session::now_ms() + 60_000;
    let removed = record::sweep(&dir, own, later, identity::is_live);
    assert!(
        !dir.join(format!("{live}.json")).exists(),
        "reused pid: removed"
    );
    assert!(
        !dir.join("999997.json.abc.tmp").exists(),
        "stray temporary: removed"
    );
    assert!(!dir.join("999998.json").exists());
    assert_eq!(removed, 3);
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn outside_aterm_there_is_no_record() {
    assert!(!session::inside_aterm(&launch(1, false)));
    assert!(session::inside_aterm(&launch(1, true)));
    let mut empty = launch(1, false);
    empty
        .vars
        .push(("ATERM_PARENT_SESSION_ID".into(), "".into()));
    assert!(!session::inside_aterm(&empty));
    // This test binary never captured a launch: the global hook keeps no record.
    codex_alab_core::activate();
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    session::observe(&frame(None, ProgramState::Idle, home.path()));
    assert!(!home.path().join("alab").exists());
}

#[test]
fn the_start_sweep_hands_on_each_dead_record_it_removed() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let dir = record::sessions_dir(home.path());
    record::ensure_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
    let started = 10_000;
    // pid 999_996 does not exist (above macOS's pid_max of 99_998).
    stale_record(&dir, 999_996, None, started - 1);
    let path = dir.join("999996.json");
    let mut value = read(&path);
    value["sessionId"] = "0199a000-0000-7000-8000-000000000001".into();
    value["exit"] = "signal:15".into();
    std::fs::write(
        &path,
        serde_json::to_vec(&value).unwrap_or_else(|err| panic!("{err}")),
    )
    .unwrap_or_else(|err| panic!("{err}"));
    let mut seen = Vec::new();
    let removed = record::sweep_with(
        &dir,
        std::process::id(),
        started,
        identity::is_live,
        |_| record::Sweep::Take,
        |pid, swept| seen.push((pid, swept.session_id.clone(), swept.exit.clone())),
    );
    assert_eq!(removed, 1);
    assert_eq!(
        seen,
        vec![(
            999_996,
            Some("0199a000-0000-7000-8000-000000000001".to_string()),
            Some("signal:15".to_string())
        )]
    );
}

#[test]
fn an_owed_carry_on_keeps_the_record_busy() {
    // A kill -9 of a relaunched codex before its carry-on must read as one that died
    // working, so the next relaunch still carries on.
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(launch(4247, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    let dir = record::sessions_dir(home.path());
    let writer = Writer::new(shared, dir.clone(), Duration::ZERO, None);
    writer.prepare();
    shared.set_carry_on_owed(true);
    let _ = writer.write_now();
    let record = record::read(&dir.join("4247.json")).unwrap_or_else(|| panic!("parses"));
    assert_eq!(record.status, Status::Busy);
    // Negative control: once the carry-on is typed the idle reads idle again.
    shared.set_carry_on_owed(false);
    let _ = writer.write_now();
    let record = record::read(&dir.join("4247.json")).unwrap_or_else(|| panic!("parses"));
    assert_eq!(record.status, Status::Idle);
}

#[test]
fn the_carry_on_chain_rides_the_record_to_the_next_relaunch() {
    // F1b: the count a carry-on set is written, and the next start's sweep hands it on.
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let dir = record::sessions_dir(home.path());
    record::ensure_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
    let started = 10_000;
    // pid 999_995 does not exist (above macOS's pid_max of 99_998).
    stale_record(&dir, 999_995, None, started - 1);
    let path = dir.join("999995.json");
    let mut value = read(&path);
    value["sessionId"] = "0199a000-0000-7000-8000-000000000002".into();
    value["status"] = "busy".into();
    value["exit"] = "signal:15".into();
    value["carriedOn"] = 1.into();
    // The line this process was started with: its own predecessor's (the sweep takes only
    // the record whose resume line is this process's argv).
    value["resumeArgv"] = serde_json::json!(["codex", "-m", "o3", "hello"]);
    std::fs::write(
        &path,
        serde_json::to_vec(&value).unwrap_or_else(|err| panic!("{err}")),
    )
    .unwrap_or_else(|err| panic!("{err}"));
    // A record from before the field reads 0 (a v1 record stays a v1 record).
    let old = dir.join("999994.json");
    let mut before = value.clone();
    before["pid"] = 999_994.into();
    before["sessionId"] = "0199a000-0000-7000-8000-000000000003".into();
    before
        .as_object_mut()
        .unwrap_or_else(|| panic!("object"))
        .remove("carriedOn");
    std::fs::write(
        &old,
        serde_json::to_vec(&before).unwrap_or_else(|err| panic!("{err}")),
    )
    .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(record::read(&old).map(|r| r.carried_on), Some(0));
    let mut own = launch(4251, true);
    own.started_at_ms = started;
    let shared = shared_with(own);
    let writer = Writer::new(shared, dir.clone(), Duration::ZERO, None);
    writer.prepare();
    let carried = |session: &str| shared.predecessor(session).map(|p| p.carried_on);
    assert_eq!(carried("0199a000-0000-7000-8000-000000000002"), Some(1));
    assert_eq!(carried("0199a000-0000-7000-8000-000000000003"), Some(0));
    // This process's own count is written.
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    shared.set_carried_on(1);
    let _ = writer.write_now();
    let mine = record::read(&dir.join("4251.json")).unwrap_or_else(|| panic!("parses"));
    assert_eq!(mine.carried_on, 1);
}

/// Finding 2 (review of 363789134): a relaunch's record carries its predecessor's chain from
/// its first write, before the carry-on reads the predecessor (after the launch probe's
/// waits), so a relaunch that dies in between hands the next one the chain, never 0. Another
/// tab's record (left for its own start) hands this process nothing.
#[test]
fn a_relaunch_carries_its_predecessors_chain_from_its_first_record() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let dir = record::sessions_dir(home.path());
    record::ensure_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
    let own_line = [
        "/abs/bin/codex",
        "resume",
        "0199a000-0000-7000-8000-000000000021",
    ];
    let other_line = [
        "/abs/bin/codex",
        "resume",
        "0199a000-0000-7000-8000-000000000022",
    ];
    let chained = |pid: u32, line: &[&str], chain: u32| {
        dead_record(&dir, pid, line[2], line, 9_000);
        let path = dir.join(format!("{pid}.json"));
        let mut value = read(&path);
        value["carriedOn"] = chain.into();
        std::fs::write(
            &path,
            serde_json::to_vec(&value).unwrap_or_else(|err| panic!("{err}")),
        )
        .unwrap_or_else(|err| panic!("{err}"));
    };
    let start = |pid: u32| {
        let mut own = launch(pid, true);
        own.started_at_ms = 10_000;
        own.args = own_line.iter().map(OsString::from).collect();
        let shared = shared_with(own);
        shared.observe(&frame(None, ProgramState::Working, home.path()), 1);
        let writer = Writer::new(shared, dir.clone(), Duration::ZERO, None);
        writer.prepare();
        let _ = writer.write_now();
        let first =
            record::read(&dir.join(format!("{pid}.json"))).unwrap_or_else(|| panic!("kept"));
        (shared.carried_on(), first.carried_on)
    };
    for chain in [1, 0] {
        chained(999_981, &own_line, chain);
        assert_eq!(start(4271 + chain), (chain, chain), "{chain}");
        assert!(
            !dir.join("999981.json").exists(),
            "its own predecessor: taken"
        );
        assert!(record::remove(&dir, 4271 + chain).is_ok());
    }
    // Negative control: another tab's chain is that tab's.
    chained(999_982, &other_line, 1);
    assert_eq!(start(4273), (0, 0));
    assert!(dir.join("999982.json").exists(), "another tab's: left");
}

#[test]
fn an_awaited_turn_keeps_the_record_busy_until_its_wait_runs_out() {
    // G1: an act that only queued its turn keeps the record busy; one whose turn never came
    // lets it rest when its wait runs out, with no other change to wake the writer.
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(launch(4252, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    let dir = record::sessions_dir(home.path());
    let _awaited = shared.await_turn(THREAD, Duration::from_secs(2));
    let writer: &'static Writer = Box::leak(Box::new(Writer::new(
        shared,
        dir.clone(),
        Duration::ZERO,
        None,
    )));
    writer.prepare();
    let _ = writer.write_now();
    let first = record::read(&dir.join("4252.json")).unwrap_or_else(|| panic!("parses"));
    assert_eq!(first.status, Status::Busy, "awaited: busy");
    writer.spawn();
    let rested = wait_record(&dir, 4252, |record| record.status == Status::Idle);
    assert_eq!(rested.status, Status::Idle);
    let _ = writer.finalize(&Ending::Remove);
}

/// Wait (a one-minute hang detector) until `pid`'s record satisfies `done`.
fn wait_record(dir: &Path, pid: u32, done: impl Fn(&Record) -> bool) -> Record {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(record) = record::read(&dir.join(format!("{pid}.json")))
            && done(&record)
        {
            return record;
        }
        assert!(Instant::now() < deadline, "the record never got there");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn draft_empty_is_a_json_boolean() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(launch(4250, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    let dir = record::sessions_dir(home.path());
    let writer = Writer::new(shared, dir.clone(), Duration::ZERO, None);
    writer.prepare();
    let _ = writer.write_now();
    let path = dir.join("4250.json");
    assert_eq!(read(&path)["draftEmpty"], serde_json::Value::Bool(true));
    let mut typed = frame(None, ProgramState::Idle, home.path());
    typed.draft_empty = false;
    shared.observe(&typed, 2);
    let _ = writer.write_now();
    assert_eq!(read(&path)["draftEmpty"], serde_json::Value::Bool(false));
    // Negative control: the schema takes nothing else there (aterm reads a non-boolean as
    // "unknown", never at rest).
    let mut value = read(&path);
    value["draftEmpty"] = "true".into();
    std::fs::write(
        &path,
        serde_json::to_vec(&value).unwrap_or_else(|err| panic!("{err}")),
    )
    .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(record::read(&path), None);
}

#[test]
fn idle_is_written_only_at_true_rest() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(launch(4251, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    let dir = record::sessions_dir(home.path());
    let writer = Writer::new(shared, dir.clone(), Duration::ZERO, None);
    writer.prepare();
    let status = || {
        let _ = writer.write_now();
        record::read(&dir.join("4251.json"))
            .unwrap_or_else(|| panic!("parses"))
            .status
    };
    // Negative control: an idle frame with nothing standing is idle.
    assert_eq!(status(), Status::Idle);
    // A turn running although the frame still draws idle (the lifecycle hook came first).
    assert!(shared.turn_started(THREAD, "turn-1"));
    assert_eq!(status(), Status::Busy);
    shared.turn_ended(THREAD, "turn-1");
    assert_eq!(status(), Status::Idle);
    // An act codex-alab is carrying out.
    let act = shared.begin_act();
    assert_eq!(status(), Status::Busy);
    drop(act);
    assert_eq!(status(), Status::Idle);
    // A person's turn on its way, and the TUI's queued input.
    let mut pending = frame(None, ProgramState::Idle, home.path());
    pending.pending_user_turn = true;
    shared.observe(&pending, 2);
    assert_eq!(status(), Status::Busy);
    let mut queued = frame(None, ProgramState::Idle, home.path());
    queued.queued_input = true;
    shared.observe(&queued, 3);
    assert_eq!(status(), Status::Busy);
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 4);
    assert_eq!(status(), Status::Idle);
    // A done or failed turn's idle is rest too, unless something still stands.
    shared.observe(&frame(None, ProgramState::Done, home.path()), 5);
    assert_eq!(status(), Status::Idle);
    shared.set_exec_pending(true);
    assert_eq!(status(), Status::Busy);
    shared.set_exec_pending(false);
    // A box keeps its own word.
    shared.observe(
        &frame(
            None,
            ProgramState::Blocked(BlockedKind::Question),
            home.path(),
        ),
        6,
    );
    assert_eq!(status(), Status::Waiting);
}

#[test]
fn leaving_rest_and_a_first_key_are_written_through() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(launch(4252, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    let dir = record::sessions_dir(home.path());
    // An hour between coalesced writes: only a write-through lands within the test.
    let writer: &'static Writer = Box::leak(Box::new(Writer::new(
        shared,
        dir.clone(),
        Duration::from_secs(3600),
        None,
    )));
    writer.spawn();
    let first = wait_record(&dir, 4252, |r| r.status == Status::Idle);
    assert!(first.draft_empty);

    // A person's first keystroke: draftEmpty false, written now, before its frame.
    shared.human_input(10_000);
    let keyed = wait_record(&dir, 4252, |r| !r.draft_empty);
    assert_eq!(keyed.last_human_input_at, Some(10_000));

    // Negative control: a change that leaves no rest waits for the interval. The next
    // keystroke is no first one (the draft already reads begun).
    // The file is renamed into place before the writer counts it: wait for the count of the
    // two writes so far (the first and the keystroke's), a one-minute hang detector.
    let deadline = Instant::now() + Duration::from_secs(60);
    while writer.writes() < 2 {
        assert!(
            Instant::now() < deadline,
            "the keystroke's write was never counted"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let writes = writer.writes();
    shared.human_input(10_001);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(writer.writes(), writes, "coalesced: nothing written yet");
    let record = record::read(&dir.join("4252.json")).unwrap_or_else(|| panic!("parses"));
    assert_eq!(record.last_human_input_at, Some(10_000));

    // Idle to busy: the turn's start is written at once (the coalesced stamp rides along).
    assert!(shared.turn_started(THREAD, "turn-1"));
    let busy = wait_record(&dir, 4252, |r| r.status == Status::Busy);
    assert_eq!(busy.last_human_input_at, Some(10_001));
    writer
        .finalize(&Ending::Remove)
        .unwrap_or_else(|err| panic!("{err}"));
}

#[test]
fn the_write_through_writes_before_it_returns() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(launch(4253, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    let dir = record::sessions_dir(home.path());
    let writer: &'static Writer = Box::leak(Box::new(Writer::new(
        shared,
        dir.clone(),
        Duration::from_secs(3600),
        None,
    )));
    writer.prepare();
    let _ = writer.write_now();
    let path = dir.join("4253.json");
    let _act = shared.begin_act();
    // Negative control: with no write-through installed the record still says idle.
    assert!(
        shared.flush_record(),
        "nothing installed: nothing to hold an act back"
    );
    assert_eq!(record::read(&path).map(|r| r.status), Some(Status::Idle));
    writer.install_write_through();
    assert!(shared.flush_record());
    assert_eq!(
        record::read(&path).map(|r| r.status),
        Some(Status::Busy),
        "on disk when the flush returns"
    );
}

/// A dead record whose `resumeArgv` is `line`, for `session`, last written at `updated_at`.
fn dead_record(dir: &Path, pid: u32, session: &str, line: &[&str], updated_at: u64) {
    stale_record(dir, pid, None, updated_at);
    let path = dir.join(format!("{pid}.json"));
    let mut value = read(&path);
    value["sessionId"] = session.into();
    value["status"] = "busy".into();
    value["exit"] = "signal:1".into();
    value["resumeArgv"] = serde_json::json!(line);
    std::fs::write(
        &path,
        serde_json::to_vec(&value).unwrap_or_else(|err| panic!("{err}")),
    )
    .unwrap_or_else(|err| panic!("{err}"));
}

/// Finding 2 (review of 148a9fedb): aterm relaunches two codex-alab tabs together (its own
/// crash took both). Each start takes only the dead record whose resume line is its own argv,
/// so the second start still finds its predecessor; a record no relaunch can owe any more (a
/// day old, or naming no line) is removed.
#[test]
fn two_tabs_relaunched_together_each_find_their_own_predecessor() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let dir = record::sessions_dir(home.path());
    record::ensure_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
    let now = codex_alab_core::session::now_ms();
    let horizon =
        u64::try_from(codex_alab_core::session::RELAUNCH_HORIZON.as_millis()).unwrap_or(0);
    let a = [
        "/abs/bin/codex",
        "resume",
        "0199a000-0000-7000-8000-00000000000a",
    ];
    let b = [
        "/abs/bin/codex",
        "resume",
        "0199a000-0000-7000-8000-00000000000b",
    ];
    let c = [
        "/abs/bin/codex",
        "resume",
        "0199a000-0000-7000-8000-00000000000c",
    ];
    // pids above macOS's pid_max of 99_998: none exists.
    dead_record(&dir, 999_990, a[2], &a, now - 1_000);
    dead_record(&dir, 999_991, b[2], &b, now - 1_000);
    dead_record(&dir, 999_992, c[2], &c, now - horizon - 1_000);
    stale_record(&dir, 999_993, None, now - 1_000);
    let start = |pid: u32, line: &[&str]| {
        let mut own = launch(pid, true);
        own.args = line.iter().map(OsString::from).collect();
        own.started_at_ms = now;
        let shared = shared_with(own);
        Writer::new(shared, dir.clone(), Duration::ZERO, None).prepare();
        shared
    };
    let first = start(4261, &a);
    assert_eq!(first.predecessor(a[2]).map(|p| p.pid), Some(999_990));
    assert_eq!(
        first.predecessor(b[2]),
        None,
        "another tab's record is not this one's"
    );
    assert!(!dir.join("999990.json").exists(), "its own: taken");
    assert!(
        dir.join("999991.json").exists(),
        "the other tab's record stands for its own relaunch"
    );
    assert!(
        !dir.join("999992.json").exists(),
        "past the horizon: removed"
    );
    assert!(!dir.join("999993.json").exists(), "no resume line: removed");
    let second = start(4262, &b);
    assert_eq!(second.predecessor(b[2]).map(|p| p.pid), Some(999_991));
    assert!(!dir.join("999991.json").exists(), "taken by its own start");
}

#[test]
fn the_sweep_judges_a_record_by_its_resume_line_and_its_age() {
    use record::Sweep;
    let line = ["/abs/bin/codex", "resume", "t1"];
    let own: Vec<OsString> = line.iter().map(OsString::from).collect();
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let dir = record::sessions_dir(home.path());
    record::ensure_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
    dead_record(&dir, 999_989, "t1", &line, 1_000);
    let mut swept = record::read(&dir.join("999989.json")).unwrap_or_else(|| panic!("parses"));
    assert_eq!(record::judge(&swept, &own, 2_000, 10_000), Sweep::Take);
    // Its own line is taken however old (the relaunch window is the carry-on's to judge).
    assert_eq!(record::judge(&swept, &own, 100_000, 10_000), Sweep::Take);
    let other: Vec<OsString> = ["/abs/bin/codex", "resume", "t2"]
        .iter()
        .map(OsString::from)
        .collect();
    assert_eq!(record::judge(&swept, &other, 2_000, 10_000), Sweep::Leave);
    assert_eq!(record::judge(&swept, &other, 11_001, 10_000), Sweep::Remove);
    // A prefix of the line is not the line.
    assert_eq!(
        record::judge(&swept, &own[..2], 2_000, 10_000),
        Sweep::Leave
    );
    swept.resume_argv = None;
    assert_eq!(record::judge(&swept, &own, 2_000, 10_000), Sweep::Remove);
}

/// Finding 5 (review of cf084e3b7): another tab's record whose end aterm reads as orderly
/// (a person's SIGINT, a signal aterm never sends, an `exited` status) owes no relaunch, so
/// the sweep removes it at once instead of keeping it, and its package, a day. One aterm may
/// still relaunch stays, idle or already in a carry-on chain alike: aterm types its line all
/// the same (its memory remedy at an idle point, its restore after its crash). A `fatal`
/// exit's goes too, whatever its status: the review of 363789134 kept it for aterm's Codex
/// lane, which read its status 1 as a crash, and that lane is retired (aterm 83bd43c206).
#[test]
fn the_sweep_removes_another_tabs_record_that_owes_no_relaunch() {
    use record::Sweep;
    let line = ["/abs/bin/codex", "resume", "t1"];
    let other: Vec<OsString> = ["/abs/bin/codex", "resume", "t2"]
        .iter()
        .map(OsString::from)
        .collect();
    let own: Vec<OsString> = line.iter().map(OsString::from).collect();
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let dir = record::sessions_dir(home.path());
    record::ensure_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
    dead_record(&dir, 999_988, "t1", &line, 1_000);
    let base = record::read(&dir.join("999988.json")).unwrap_or_else(|| panic!("parses"));
    let with = |exit: Option<&str>, status: Status, carried_on: u32| record::Record {
        exit: exit.map(str::to_string),
        status,
        carried_on,
        ..base.clone()
    };
    for (exit, status) in [
        (Some("signal:2"), Status::Busy),
        (Some("signal:9"), Status::Busy),
        (None, Status::Exited),
        (Some("signal:1"), Status::Exited),
        (Some("fatal"), Status::Exited),
        (Some("fatal"), Status::Busy),
        (Some("fatal"), Status::Idle),
    ] {
        let orderly = with(exit, status, 0);
        assert!(!record::relaunch_owed(&orderly), "{exit:?} {status:?}");
        assert_eq!(
            record::judge(&orderly, &other, 2_000, 10_000),
            Sweep::Remove,
            "{exit:?} {status:?}"
        );
        // Its own start still takes it (the carry-on refuses it there).
        assert_eq!(record::judge(&orderly, &own, 2_000, 10_000), Sweep::Take);
    }
    // Negative control: a relaunch aterm may still type is left for its own start, whatever
    // its status or its chain.
    for exit in [None, Some("signal:15"), Some("signal:1")] {
        for status in [
            Status::Busy,
            Status::Idle,
            Status::Waiting,
            Status::Upgrading,
        ] {
            for carried_on in [0, 1] {
                let owed = with(exit, status, carried_on);
                assert!(record::relaunch_owed(&owed), "{exit:?} {status:?}");
                assert_eq!(
                    record::judge(&owed, &other, 2_000, 10_000),
                    Sweep::Leave,
                    "{exit:?} {status:?} {carried_on}"
                );
            }
        }
    }
    // And through the sweep itself: the orderly record goes, the owed one stays.
    dead_record(
        &dir,
        999_987,
        "t3",
        &["/abs/bin/codex", "resume", "t3"],
        1_000,
    );
    let path = dir.join("999987.json");
    let mut value = read(&path);
    value["exit"] = "signal:2".into();
    std::fs::write(
        &path,
        serde_json::to_vec(&value).unwrap_or_else(|err| panic!("{err}")),
    )
    .unwrap_or_else(|err| panic!("{err}"));
    // A fatal exit's record, as codex-alab writes it (`exited`, `fatal`): removed too.
    dead_record(
        &dir,
        999_986,
        "t4",
        &["/abs/bin/codex", "resume", "t4"],
        1_000,
    );
    let fatal = dir.join("999986.json");
    let mut value = read(&fatal);
    value["exit"] = "fatal".into();
    value["status"] = "exited".into();
    std::fs::write(
        &fatal,
        serde_json::to_vec(&value).unwrap_or_else(|err| panic!("{err}")),
    )
    .unwrap_or_else(|err| panic!("{err}"));
    let removed = record::sweep_with(
        &dir,
        4263,
        2_000,
        |_, _| false,
        |swept| record::judge(swept, &other, 2_000, 10_000),
        |_, _| {},
    );
    assert_eq!(removed, 2);
    assert!(!path.exists(), "the person's SIGINT: removed");
    assert!(dir.join("999988.json").exists(), "a crash witness: left");
    assert!(
        !fatal.exists(),
        "a fatal exit nothing relaunches any more: removed"
    );
}
