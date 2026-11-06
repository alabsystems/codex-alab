// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The session record, process identity, the resume rewrite and the record's endings.

#[path = "record_recovery_tests.rs"]
mod record_recovery_tests;

#[path = "record_custody_tests.rs"]
mod record_custody_tests;

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
    #[arg(long = "worktree")]
    worktree: bool,
    #[arg(long = "remote")]
    remote: Option<String>,
    #[arg(long = "remote-auth-token-env")]
    remote_auth_token_env: Option<String>,
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
    assert_eq!(
        rebuild(&["codex", "--image=a.png", "resume", "OLD"]),
        resumed(&["resume", THREAD])
    );
    assert_eq!(
        rebuild(&["codex", "-ia.png", "fork", "OLD"]),
        resumed(&["resume", THREAD])
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
    assert_eq!(rebuild(&["codex", "--worktree"]), None);
    assert_eq!(rebuild(&["codex", "fork", "--worktree", "OLD"]), None);
    assert_eq!(rebuild(&["codex", "--remote", "ws://127.0.0.1:8080"]), None);
    assert_eq!(
        rebuild(&["codex", "--remote-auth-token-env", "MY_TOKEN"]),
        None
    );
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
        managed_argv0: None,
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
    let mut captured = launch(4242, true);
    captured.argv0 = captured.current_exe.clone();
    captured.managed_argv0 = Some(PathBuf::from("/twin/codex"));
    let shared = shared_with(captured);
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
        "atermSession",
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
    assert_eq!(
        shared
            .launch()
            .unwrap_or_else(|| panic!("a captured launch"))
            .argv0
            .as_deref(),
        Some(Path::new("/store/b1/bin/codex"))
    );
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
    assert_eq!(value["atermSession"], "fake");
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

    // A path that has become a directory cannot resume the thread, even if it
    // contains a file that looks like a rollout.
    std::fs::remove_file(&rollout).unwrap_or_else(|err| panic!("{err}"));
    std::fs::create_dir(&rollout).unwrap_or_else(|err| panic!("{err}"));
    let entry = rollout.join("unrelated.jsonl");
    std::fs::write(&entry, "{}\n").unwrap_or_else(|err| panic!("{err}"));
    let _ = writer.write_now();
    let record = record::read(&dir.join("4244.json")).unwrap_or_else(|| panic!("parses"));
    assert_eq!(
        record.resume_argv, None,
        "a directory must withdraw the resume line"
    );
    assert_eq!(shared.resume_lines(), 1);

    // Replacing it with the actual non-empty rollout restores resumability and
    // tells the claim to renew the line it had to withdraw.
    std::fs::remove_file(entry).unwrap_or_else(|err| panic!("{err}"));
    std::fs::remove_dir(&rollout).unwrap_or_else(|err| panic!("{err}"));
    std::fs::write(&rollout, "{}\n").unwrap_or_else(|err| panic!("{err}"));
    let _ = writer.write_now();
    let record = record::read(&dir.join("4244.json")).unwrap_or_else(|| panic!("parses"));
    assert_eq!(record.resume_argv, resumed(&["-m", "o3", "resume", THREAD]));
    assert_eq!(shared.resume_lines(), 2);
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
    let mut last = String::new();
    for step in 1..=40u64 {
        // A change that leaves no rest: the shell's directory moving.
        last = format!("/work/{step}");
        let cwd = PathBuf::from(&last);
        shared.observe(
            &Frame {
                cwd: &cwd,
                ..frame(None, ProgramState::Idle, home.path())
            },
            1 + step,
        );
        std::thread::sleep(Duration::from_millis(15));
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(record) = record::read(&dir.join("4245.json"))
            && record.cwd.as_deref() == Some(last.as_str())
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
    // One per interval, plus the first write.
    let allowed = u64::try_from(elapsed.as_millis() / interval.as_millis()).unwrap_or(0) + 1;
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

/// A PERSON'S TYPING AND SCROLLING REWRITE NOTHING (CONTRACT §4): the first key is written,
/// and the stamps after it ride along on a write another change causes; a wheel, a drag or a
/// release begins no draft, so it writes nothing at all. Each write is an atomic replace with
/// a full sync, and typing or scrolling rewrote the record every interval.
#[test]
fn typing_and_scrolling_rewrite_nothing_after_the_first_key() {
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
    let write = || writer.write_now().unwrap_or_else(|err| panic!("{err}"));
    assert!(write(), "the first record");

    // Scrolling the transcript at rest: each wheel step, then the frame it redraws.
    for step in 1..=20 {
        shared.human_attention(10_000 + step);
        assert!(!write(), "a wheel step wrote the record");
        shared.observe(&frame(None, ProgramState::Idle, home.path()), 1 + step);
        assert!(!write(), "a scrolled frame wrote the record");
    }
    let record = record::read(&dir.join("4253.json")).unwrap_or_else(|| panic!("parses"));
    assert!(record.draft_empty);

    // Typing a draft: the first key begins it, and the keys after it write nothing.
    shared.human_input(20_000);
    assert!(write(), "the first key is written");
    let typing = Frame {
        draft_empty: false,
        ..frame(None, ProgramState::Idle, home.path())
    };
    for step in 1..=20 {
        shared.human_input(20_000 + step);
        assert!(!write(), "a key wrote the record");
        shared.observe(&typing, 100 + step);
        assert!(!write(), "a typed frame wrote the record");
    }
    let record = record::read(&dir.join("4253.json")).unwrap_or_else(|| panic!("parses"));
    assert_eq!(record.last_human_input_at, Some(20_000));

    // The turn's start is written, and the latest stamp rides along.
    assert!(shared.turn_started(THREAD, "turn-1"));
    assert!(write(), "the turn's start");
    let record = record::read(&dir.join("4253.json")).unwrap_or_else(|| panic!("parses"));
    assert_eq!(record.last_human_input_at, Some(20_020));
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

/// THE LIVE UPGRADE CARRIES THE AUTOPILOT'S OWED ACT (FOREVER CRUISE, aterm owner direction
/// 2026-10-19: a paced act no longer holds the re-exec): the image that leaves writes the act
/// it owed, and its Land landing, into its record; the new image inherits them. A landing
/// alone writes no `rule`; a done claim's act decided before the repository was read writes
/// the open work it names (`repoOwed`), and only that one; a wall's try that delivers a land
/// act again writes `landing` (parity-3 review, F4), and only that one. NEGATIVE CONTROL: an
/// image that owed nothing writes no `owed` key, and its successor inherits none.
#[test]
fn a_re_exec_inherits_the_act_its_autopilot_owed() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let pid = std::process::id();
    let dir = record::sessions_dir(home.path());
    let owed = codex_alab_core::session::OwedAct {
        renewal: None,
        native_origin: None,
        thread: THREAD.to_string(),
        rule: Some("continue-done-check@v1".to_string()),
        text: Some("Defend it.".to_string()),
        mode: Some("cruise".to_string()),
        not_before_ms: 1_234_567,
        repo_owed: None,
        resume_goal: false,
        landing: false,
        landed: false,
    };
    let nudge = codex_alab_core::session::OwedAct {
        renewal: None,
        native_origin: None,
        thread: THREAD.to_string(),
        rule: Some("land-nudge@v1".to_string()),
        text: Some("Still landing.".to_string()),
        mode: Some("land".to_string()),
        not_before_ms: 1_234_567,
        repo_owed: Some(vec!["not landed".to_string()]),
        resume_goal: false,
        landing: false,
        landed: false,
    };
    let landed = codex_alab_core::session::OwedAct {
        thread: THREAD.to_string(),
        landed: true,
        ..codex_alab_core::session::OwedAct::default()
    };
    let retry = codex_alab_core::session::OwedAct {
        thread: THREAD.to_string(),
        rule: Some("api-retry@v1".to_string()),
        text: Some("Codex reported an API error (500); carry on.".to_string()),
        mode: Some("land".to_string()),
        not_before_ms: 1_234_567,
        landing: true,
        ..codex_alab_core::session::OwedAct::default()
    };
    for carried in [Some(owed), Some(nudge), Some(landed), Some(retry), None] {
        let shared = shared_with(launch(pid, true));
        shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
        shared.set_owed_act(carried.clone());
        let before = Writer::new(shared, dir.clone(), Duration::ZERO, None);
        before.prepare();
        let _ = before.write_now();
        before
            .finalize(&Ending::Upgrading)
            .unwrap_or_else(|err| panic!("{err}"));
        let written = read(&dir.join(format!("{pid}.json")));
        assert_eq!(
            written.get("owed").is_some(),
            carried.is_some(),
            "{written}"
        );
        for key in ["rule", "text", "mode"] {
            assert_eq!(
                written["owed"].get(key).is_some(),
                carried.as_ref().is_some_and(|c| c.rule.is_some()),
                "{key}: {written}"
            );
        }
        assert_eq!(
            written["owed"].get("repoOwed").cloned(),
            carried
                .as_ref()
                .and_then(|c| c.repo_owed.as_ref())
                .map(|open| serde_json::json!(open)),
            "{written}"
        );
        assert_eq!(
            written["owed"].get("landing").cloned(),
            carried
                .as_ref()
                .is_some_and(|c| c.landing)
                .then_some(serde_json::json!(true)),
            "{written}"
        );
        let shared = shared_with(launch(pid, true));
        shared.observe(&frame(None, ProgramState::Idle, home.path()), 2);
        let after = Writer::new(shared, dir.clone(), Duration::ZERO, None);
        after.prepare();
        let inherited = shared.inherited().unwrap_or_else(|| panic!("inherited"));
        assert_eq!(inherited.owed_act, carried);
        assert!(record::remove(&dir, pid).is_ok());
    }
}

/// THE RECORD CARRIES THE NEEDS AN OFF TOOK DOWN AND THE WORKER'S FOLDER (review of the
/// forever autopilot, 2026-10-20): the image after a re-exec inherits `needsOff` (16 hex
/// digits, leading zeros kept) and the primary thread's `workdir`. NEGATIVE CONTROL: an image
/// with neither writes neither key and hands on none (0, no folder), and another thread's
/// folder is not the record's.
#[test]
fn a_re_exec_inherits_the_needs_an_off_took_down_and_the_workers_folder() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let pid = std::process::id();
    let dir = record::sessions_dir(home.path());
    let needs_off = 0x00ab_cdef_0123_4567_u64;
    for (off, workdir) in [(needs_off, Some(PathBuf::from("/work/repo"))), (0, None)] {
        let shared = shared_with(launch(pid, true));
        shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
        shared.set_needs_off(off);
        if let Some(workdir) = &workdir {
            shared.set_workdir(THREAD, workdir);
        }
        shared.set_workdir(
            "019a0000-0000-7000-8000-0000000000ff",
            Path::new("/elsewhere"),
        );
        let before = Writer::new(shared, dir.clone(), Duration::ZERO, None);
        before.prepare();
        let _ = before.write_now();
        before
            .finalize(&Ending::Upgrading)
            .unwrap_or_else(|err| panic!("{err}"));
        let written = read(&dir.join(format!("{pid}.json")));
        assert_eq!(
            written.get("needsOff").and_then(serde_json::Value::as_str),
            (off != 0).then_some("00abcdef01234567"),
            "{written}"
        );
        assert_eq!(
            written.get("workdir").and_then(serde_json::Value::as_str),
            workdir.as_deref().and_then(Path::to_str),
            "{written}"
        );
        let shared = shared_with(launch(pid, true));
        shared.observe(&frame(None, ProgramState::Idle, home.path()), 2);
        let after = Writer::new(shared, dir.clone(), Duration::ZERO, None);
        after.prepare();
        let inherited = shared.inherited().unwrap_or_else(|| panic!("inherited"));
        assert_eq!(inherited.needs_off, off);
        assert_eq!(inherited.workdir, workdir);
        assert!(record::remove(&dir, pid).is_ok());
    }
}

/// And the relaunch after a crash: the dead record's `needsOff`, `workdir` and `owed` (the
/// paced act it owed at rest, which aterm's relaunch hands on) are its predecessor's.
/// NEGATIVE CONTROL: a `needsOff` that is not 16 hex digits names none.
#[test]
fn a_relaunch_finds_the_needs_an_off_took_down_and_the_workers_folder() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let dir = record::sessions_dir(home.path());
    record::ensure_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
    let line = [
        "/abs/bin/codex",
        "resume",
        "0199a000-0000-7000-8000-000000000031",
    ];
    for (word, off) in [
        ("00abcdef01234567", 0x00ab_cdef_0123_4567_u64),
        ("abcdef", 0),
    ] {
        dead_record(&dir, 999_979, line[2], &line, 9_000);
        let path = dir.join("999979.json");
        let mut value = read(&path);
        value["needsOff"] = word.into();
        value["workdir"] = "/work/repo".into();
        value["owed"] = serde_json::json!({
            "thread": line[2],
            "rule": "continue-done-check@v1",
            "text": "Defend it, as decided.",
            "mode": "cruise",
            "notBefore": 9_120_000_u64,
            "landed": false,
        });
        std::fs::write(
            &path,
            serde_json::to_vec(&value).unwrap_or_else(|err| panic!("{err}")),
        )
        .unwrap_or_else(|err| panic!("{err}"));
        let mut own = launch(4291, true);
        own.started_at_ms = 10_000;
        own.args = line.iter().map(OsString::from).collect();
        let shared = shared_with(own);
        let writer = Writer::new(shared, dir.clone(), Duration::ZERO, None);
        writer.prepare();
        let predecessor = shared
            .predecessor(line[2])
            .unwrap_or_else(|| panic!("the dead record is swept: {word}"));
        assert_eq!(predecessor.needs_off, off, "{word}");
        assert_eq!(
            predecessor.workdir.as_deref(),
            Some(Path::new("/work/repo")),
            "{word}"
        );
        assert_eq!(
            predecessor.owed_act,
            Some(codex_alab_core::session::OwedAct {
                renewal: None,
                native_origin: None,
                thread: line[2].to_string(),
                rule: Some("continue-done-check@v1".to_string()),
                text: Some("Defend it, as decided.".to_string()),
                mode: Some("cruise".to_string()),
                not_before_ms: 9_120_000,
                repo_owed: None,
                resume_goal: false,
                landing: false,
                landed: false,
            }),
            "{word}"
        );
        assert_eq!(
            shared.owed_act(),
            predecessor.owed_act,
            "{word}: carried on"
        );
        let _ = record::remove(&dir, 4291);
    }
}

/// THE RECORD CARRIES THE PROBE'S SCOPE (review of the forever probe, land-probe-last-workdir
/// and land-probe-head-only): the folders the primary thread touched, its overflow, its start and
/// the branches it used go into `probeScope`, and the image after a re-exec takes them up as the
/// session's carried scope ([`Shared::carried_probe_scope`]). NEGATIVE CONTROL: another thread's
/// scope is not the record's.
#[test]
fn a_re_exec_carries_the_probe_scope() {
    use codex_alab_core::session::ProbeScope;
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let pid = std::process::id();
    let dir = record::sessions_dir(home.path());
    let scope = ProbeScope {
        dirs: vec![PathBuf::from("/work/repo"), PathBuf::from("/work/lib")],
        overflow: false,
        since: Some(1_700_000_000),
        branches: vec![codex_alab_core::session::UsedBranch::new(
            "fix",
            Some(Path::new("/work/repo")),
        )],
    };
    let shared = shared_with(launch(pid, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    shared.set_workdir(THREAD, Path::new("/work/repo"));
    shared.set_probe_scope(THREAD, &scope);
    // NEGATIVE CONTROL: another thread's.
    shared.set_probe_scope(
        "019a0000-0000-7000-8000-0000000000ff",
        &ProbeScope {
            dirs: vec![PathBuf::from("/elsewhere")],
            ..ProbeScope::default()
        },
    );
    let before = Writer::new(shared, dir.clone(), Duration::ZERO, None);
    before.prepare();
    let _ = before.write_now();
    before
        .finalize(&Ending::Upgrading)
        .unwrap_or_else(|err| panic!("{err}"));
    let written = read(&dir.join(format!("{pid}.json")));
    assert_eq!(
        written.get("probeScope"),
        Some(&serde_json::json!({
            "dirs": ["/work/repo", "/work/lib"],
            "since": 1_700_000_000_u64,
            "branches": [{"name": "fix", "dir": "/work/repo"}],
        })),
        "{written}"
    );
    let shared = shared_with(launch(pid, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 2);
    let after = Writer::new(shared, dir.clone(), Duration::ZERO, None);
    after.prepare();
    assert_eq!(shared.carried_probe_scope(THREAD), Some(scope));
    assert!(record::remove(&dir, pid).is_ok());
}

/// And the relaunch after a crash: the dead record's `probeScope` is its session's carried scope;
/// a form this reader does not know is a scope that lost what it held (`overflow`), never one
/// read clean. NEGATIVE CONTROL: a record with no `probeScope` carries none.
#[test]
fn a_relaunch_carries_the_probe_scope() {
    use codex_alab_core::session::ProbeScope;
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let dir = record::sessions_dir(home.path());
    record::ensure_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
    let line = [
        "/abs/bin/codex",
        "resume",
        "0199a000-0000-7000-8000-000000000032",
    ];
    let carried = ProbeScope {
        dirs: vec![PathBuf::from("/work/repo")],
        overflow: false,
        since: Some(1_700_000_000),
        branches: vec![
            codex_alab_core::session::UsedBranch::new("feat", Some(Path::new("/work/repo"))),
            codex_alab_core::session::UsedBranch::new("any", None),
        ],
    };
    let lost = ProbeScope {
        overflow: true,
        ..ProbeScope::default()
    };
    for (form, expected) in [
        (
            Some(serde_json::json!({
                "dirs": ["/work/repo"],
                "since": 1_700_000_000_u64,
                "branches": [{"name": "feat", "dir": "/work/repo"}, {"name": "any"}],
            })),
            Some(carried),
        ),
        (
            Some(serde_json::json!({"dirs": "/work/repo"})),
            Some(lost.clone()),
        ),
        // A branch with no folder of its own (a name alone) is no form this reader knows.
        (Some(serde_json::json!({"branches": ["feat"]})), Some(lost)),
        // NEGATIVE CONTROL: none written.
        (None, None),
    ] {
        dead_record(&dir, 999_978, line[2], &line, 9_000);
        let path = dir.join("999978.json");
        let mut value = read(&path);
        if let Some(form) = &form {
            value["probeScope"] = form.clone();
        }
        std::fs::write(
            &path,
            serde_json::to_vec(&value).unwrap_or_else(|err| panic!("{err}")),
        )
        .unwrap_or_else(|err| panic!("{err}"));
        let mut own = launch(4292, true);
        own.started_at_ms = 10_000;
        own.args = line.iter().map(OsString::from).collect();
        let shared = shared_with(own);
        let writer = Writer::new(shared, dir.clone(), Duration::ZERO, None);
        writer.prepare();
        assert_eq!(shared.carried_probe_scope(line[2]), expected, "{form:?}");
        let _ = record::remove(&dir, 4292);
        let _ = record::remove(&dir, 999_978);
    }
}

/// THE RECORD CARRIES THE PRIMARY THREAD'S PACE (parity review S2; aterm's loop keeps it
/// across its own restarts): the image after a re-exec takes up the pace the image before
/// wrote, and so does aterm's relaunch from the dead process's record, however old (aterm's
/// loop seeds its streak and ladder from the session's books at every start with no horizon:
/// `short_streak_in`, `seed_retry`; codex-alab dropped a pace written over an hour before);
/// the new process's records carry it on from the first (ruling F's replies included, their
/// marks as hex words, with the reset the newest showed and the held point's words; that a
/// relaunch takes them up into no memory is the autopilot's rule; and Land's hold standing,
/// its `since` and nudges, review of gap 11, finding 1).
/// NEGATIVE CONTROLS: a pace at rest writes no `pace` key, and another thread's is not the
/// record's.
#[test]
fn the_record_carries_the_pace_to_a_re_exec_and_a_relaunch() {
    use codex_alab_core::session::CutAct;
    use codex_alab_core::session::KeptReply;
    use codex_alab_core::session::Pace;
    use codex_alab_core::session::WallMark;
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let pid = std::process::id();
    let dir = record::sessions_dir(home.path());
    let pace = Pace {
        short_streak: 2,
        wall: Some(WallMark {
            class: "server".to_string(),
            attempts: 3,
            first_seen_ms: 1_000,
            last_try_ms: Some(2_000),
            since_ms: Some(2_500),
            answer_owed: true,
        }),
        typed_ms: vec![1_200, 2_000],
        cut: Some(CutAct {
            text: "Defend it.".to_string(),
            mode: "cruise".to_string(),
        }),
        checks: 4,
        // Ruling F's replies: a mark past 2^53, which a JSON number would round, and a
        // wall's with its tries.
        replies: vec![
            KeptReply {
                mark: 0xfedc_ba98_7654_3210,
                answers: 2,
                walled: true,
                tries: 1,
            },
            KeptReply {
                mark: 0xff,
                answers: 1,
                walled: false,
                tries: 0,
            },
        ],
        replies_run: 3,
        replies_at_ms: Some(2_600),
        // The reset the newest answer showed, and the held point's words.
        replies_reset_ms: Some(2_700),
        replies_point: Some(codex_alab_core::session::ReplyPoint {
            text: Some("Still on the parser.".to_string()),
            earlier: Some("Rebuilt.".to_string()),
            questions: true,
        }),
        land_hold: Some(codex_alab_core::session::LandHold {
            since_ms: 1_793_000_000_123,
            nudges: 3,
        }),
        // The person's hour standing and the go-ahead's count on the standing park.
        go_ahead_hour_ms: Some(1_793_000_000_456),
        go_ahead: Some(codex_alab_core::session::GoAheadCount {
            acts: 2,
            first_ms: Some(1_793_000_000_789),
        }),
        // The point left to the person, its exchange the session's opening (edges-5).
        left_point: Some(codex_alab_core::session::LeftPoint {
            point: codex_alab_core::session::ReplyPoint {
                text: Some("It parses foo files.".to_string()),
                earlier: None,
                questions: false,
            },
            opens: true,
        }),
        // A person's Esc over the point, and the goal a turn error blocked at its wall.
        esc: Some(codex_alab_core::session::EscHold {
            at_ms: 1_793_000_000_900,
            turn: "turn-7".to_string(),
        }),
        wall_goal: Some(codex_alab_core::session::WallGoal {
            goal_id: "goal-1".to_string(),
            objective: "finish the original task".to_string(),
            token_budget: Some(1_000),
            turn: "turn-7".to_string(),
            person_stops: 0,
        }),
    };
    let word = serde_json::json!({
        "shortStreak": 2,
        "wall": {
            "class": "server",
            "attempts": 3,
            "firstSeen": 1_000,
            "lastTry": 2_000,
            "since": 2_500,
            "answerOwed": true,
        },
        "typed": [1_200, 2_000],
        "cut": {"text": "Defend it.", "mode": "cruise"},
        "checks": 4,
        "replies": [
            {"mark": "fedcba9876543210", "answers": 2, "walled": true, "tries": 1},
            {"mark": "00000000000000ff", "answers": 1},
        ],
        "repliesRun": 3,
        "repliesAt": 2_600,
        "repliesReset": 2_700,
        "repliesPoint": {"text": "Still on the parser.", "earlier": "Rebuilt.", "questions": true},
        "landHold": {"since": 1_793_000_000_123_u64, "nudges": 3},
        "goAheadHour": 1_793_000_000_456_u64,
        "goAhead": {"acts": 2, "first": 1_793_000_000_789_u64},
        "leftPoint": {"text": "It parses foo files.", "opens": true},
        "esc": {"at": 1_793_000_000_900_u64, "turn": "turn-7"},
        "wallGoal": {"id": "goal-1", "objective": "finish the original task", "budget": 1_000, "turn": "turn-7"},
    });
    for carried in [Some(pace.clone()), None] {
        let shared = shared_with(launch(pid, true));
        shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
        if let Some(pace) = &carried {
            shared.set_pace(THREAD, pace.clone());
        }
        shared.set_pace(
            "019a0000-0000-7000-8000-0000000000ff",
            Pace {
                short_streak: 5,
                ..Pace::default()
            },
        );
        let before = Writer::new(shared, dir.clone(), Duration::ZERO, None);
        before.prepare();
        let _ = before.write_now();
        before
            .finalize(&Ending::Upgrading)
            .unwrap_or_else(|err| panic!("{err}"));
        let written = read(&dir.join(format!("{pid}.json")));
        assert_eq!(
            written.get("pace"),
            carried.as_ref().map(|_| &word),
            "{written}"
        );
        let shared = shared_with(launch(pid, true));
        shared.observe(&frame(None, ProgramState::Idle, home.path()), 2);
        let after = Writer::new(shared, dir.clone(), Duration::ZERO, None);
        after.prepare();
        assert_eq!(shared.pace(THREAD), carried);
        let _ = after.write_now();
        let rewritten = read(&dir.join(format!("{pid}.json")));
        assert_eq!(rewritten.get("pace"), carried.as_ref().map(|_| &word));
        assert!(record::remove(&dir, pid).is_ok());
    }
    // The relaunch after a crash.
    record::ensure_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
    let line = [
        "/abs/bin/codex",
        "resume",
        "0199a000-0000-7000-8000-000000000041",
    ];
    for age in [60_000, 3 * 24 * 3_600_000] {
        let written_at = 1_000_000_000;
        dead_record(&dir, 999_969, line[2], &line, written_at);
        let path = dir.join("999969.json");
        let mut value = read(&path);
        value["pace"] = word.clone();
        std::fs::write(
            &path,
            serde_json::to_vec(&value).unwrap_or_else(|err| panic!("{err}")),
        )
        .unwrap_or_else(|err| panic!("{err}"));
        let mut own = launch(4292, true);
        own.started_at_ms = written_at + age;
        own.args = line.iter().map(OsString::from).collect();
        let shared = shared_with(own);
        let writer = Writer::new(shared, dir.clone(), Duration::ZERO, None);
        writer.prepare();
        assert!(shared.predecessor(line[2]).is_some(), "swept: {age}");
        assert_eq!(shared.pace(line[2]), Some(pace.clone()), "{age}");
        let _ = record::remove(&dir, 4292);
    }
}

/// A PERSON'S ESC AND A WALL'S GOAL IN THE RECORD'S PACE (the forever-persist lane, item a):
/// each alone is carried (a pace at rest but for it), the goal's budget absent where it has
/// none, with the turn whose error blocked it (the image after reads that turn's history for a
/// person's stop: review of the forever-persist lane, F2), and a person's stops are never written
/// (each process counts its own from none). A form this reader does not know, an Esc that names
/// no time or no turn, and a goal that names no id or no turn discard only that field: the rest
/// of the pace, and the record (a crash witness), read as written. NEGATIVE CONTROL: an older
/// record's pace, which names neither, carries neither.
#[test]
fn a_persons_esc_and_a_walls_goal_ride_the_records_pace() {
    use codex_alab_core::session::EscHold;
    use codex_alab_core::session::Pace;
    use codex_alab_core::session::WallGoal;
    let esc = Pace {
        esc: Some(EscHold {
            at_ms: 9_000,
            turn: "turn-1".to_string(),
        }),
        ..Pace::default()
    };
    let goal = Pace {
        wall_goal: Some(WallGoal {
            goal_id: "goal-1".to_string(),
            objective: "ship it".to_string(),
            token_budget: None,
            turn: "turn-2".to_string(),
            person_stops: 4,
        }),
        ..Pace::default()
    };
    assert!(!esc.at_rest() && !goal.at_rest(), "each alone is carried");
    let word = |pace: &Pace| {
        serde_json::to_value(record::Pace::from(pace.clone())).unwrap_or_else(|err| panic!("{err}"))
    };
    assert_eq!(
        word(&esc),
        serde_json::json!({"shortStreak": 0, "esc": {"at": 9_000, "turn": "turn-1"}})
    );
    assert_eq!(
        word(&goal),
        serde_json::json!({"shortStreak": 0, "wallGoal": {"id": "goal-1", "objective": "ship it", "turn": "turn-2"}})
    );
    let read = |value: serde_json::Value| {
        Pace::from(
            serde_json::from_value::<record::Pace>(value).unwrap_or_else(|err| panic!("{err}")),
        )
    };
    assert_eq!(read(word(&esc)), esc);
    assert_eq!(
        read(word(&goal))
            .wall_goal
            .map(|goal| (goal.turn, goal.person_stops)),
        Some(("turn-2".to_string(), 0)),
        "a person's stops are this process's"
    );
    for (esc, wall_goal) in [
        (serde_json::json!("turn-1"), serde_json::json!(["goal-1"])),
        (
            serde_json::json!({"at": 0, "turn": "turn-1"}),
            serde_json::json!({"id": " ", "objective": "ship it"}),
        ),
        (
            serde_json::json!({"at": 9_000, "turn": ""}),
            serde_json::json!({"objective": "ship it"}),
        ),
        (
            serde_json::json!({"at": -1, "turn": "turn-1"}),
            serde_json::json!({"id": "goal-1", "objective": "ship it", "budget": "lots", "turn": "turn-2"}),
        ),
        (
            serde_json::json!({"at": 9_000}),
            serde_json::json!({"id": "goal-1", "objective": "ship it"}),
        ),
        (
            serde_json::json!({"turn": "turn-1"}),
            serde_json::json!({"id": "goal-1", "objective": "ship it", "turn": " "}),
        ),
    ] {
        let unknown = read(serde_json::json!({
            "shortStreak": 3,
            "checks": 2,
            "esc": esc,
            "wallGoal": wall_goal,
        }));
        assert_eq!(
            (
                unknown.short_streak,
                unknown.checks,
                unknown.esc,
                unknown.wall_goal
            ),
            (3, 2, None, None),
            "{esc} {wall_goal}"
        );
    }
    // NEGATIVE CONTROL: an older record's pace.
    let old = read(serde_json::json!({"shortStreak": 3}));
    assert_eq!((old.esc, old.wall_goal), (None, None));
}

/// RULING F'S REPLIES IN THE RECORD'S PACE: an older record's pace, which names none, reads
/// as no reply kept (its streak and checks as written, no reset, no held point and no Land
/// hold), and a pace at rest but for its replies, for the reset the newest showed, or for
/// Land's hold, is carried. NEGATIVE CONTROL: a mark that is no 16 hex digits is no reply kept,
/// the rest read as written.
#[test]
fn an_older_records_pace_reads_as_no_replies_and_an_unreadable_mark_as_none() {
    use codex_alab_core::session::KeptReply;
    use codex_alab_core::session::Pace;
    let old: record::Pace = serde_json::from_value(serde_json::json!({
        "shortStreak": 3,
        "checks": 2,
    }))
    .unwrap_or_else(|err| panic!("{err}"));
    let old = Pace::from(old);
    assert_eq!(
        (
            old.short_streak,
            old.checks,
            old.replies.len(),
            old.replies_run,
            old.replies_at_ms,
            old.replies_reset_ms,
            old.replies_point.is_none(),
            old.land_hold,
        ),
        (3, 2, 0, 0, None, None, true, None)
    );
    // Land's hold alone is carried, its nudges absent at 0.
    let hold = Pace {
        land_hold: Some(codex_alab_core::session::LandHold {
            since_ms: 9_000,
            nudges: 0,
        }),
        ..Pace::default()
    };
    assert!(!hold.at_rest(), "Land's hold alone is carried");
    let word = serde_json::to_value(record::Pace::from(hold.clone()))
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(
        word,
        serde_json::json!({"shortStreak": 0, "landHold": {"since": 9_000}})
    );
    let back: record::Pace = serde_json::from_value(word).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(Pace::from(back), hold);
    // The reset outlives the replies (an Off keeps it): alone, it is carried.
    let reset = Pace {
        replies_reset_ms: Some(9_500),
        ..Pace::default()
    };
    assert!(!reset.at_rest(), "a reset alone is carried");
    let kept = Pace {
        replies: vec![KeptReply {
            mark: 0xfedc_ba98_7654_3210,
            answers: 2,
            walled: false,
            tries: 0,
        }],
        replies_run: 2,
        replies_at_ms: Some(9_000),
        ..Pace::default()
    };
    assert!(!kept.at_rest(), "replies alone are carried");
    let word = serde_json::to_value(record::Pace::from(kept.clone()))
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(
        word,
        serde_json::json!({
            "shortStreak": 0,
            "replies": [{"mark": "fedcba9876543210", "answers": 2}],
            "repliesRun": 2,
            "repliesAt": 9_000,
        })
    );
    let back: record::Pace = serde_json::from_value(word).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(Pace::from(back), kept);
    // NEGATIVE CONTROL: an unreadable mark.
    let bad: record::Pace = serde_json::from_value(serde_json::json!({
        "replies": [{"mark": "12345", "answers": 2}, {"mark": "00000000000000ff", "answers": 1}],
        "repliesRun": 2,
    }))
    .unwrap_or_else(|err| panic!("{err}"));
    let bad = Pace::from(bad);
    assert_eq!(
        bad.replies,
        [KeptReply {
            mark: 0xff,
            answers: 1,
            walled: false,
            tries: 0,
        }]
    );
    assert_eq!(bad.replies_run, 2);
}

/// A DEAD RECORD'S PACE RIDES ON IN ITS OWN ATERM SESSION, OR WHERE ATERM'S RELAUNCH TYPED
/// THE LINE (review of CX-B, CXR-2): aterm's loop keeps a tab's pace in that session's books
/// and takes it up at every start in that tab, however old and however the worker before
/// ended, so a record written in this process's session (`atermSession`) carries its pace on
/// with no relaunch proof. A record from another tab, or one that names no session (an older
/// build), hands its pace on only where the launch probe proves aterm typed this process's
/// line; a person's resume of it starts with none. Before the probe answers, the record is the
/// busy initializer (no line, no pace) and the predecessor's file keeps custody of the pace
/// (CONTRACT §4). The record says the session it was written in.
#[test]
fn a_dead_records_pace_rides_on_in_its_own_aterm_session_or_aterms_relaunch() {
    use codex_alab_core::session::HandPid;
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::LaunchWitness;
    let word = serde_json::json!({"shortStreak": 4});
    let line = ["/twin/codex", "resume", THREAD];
    // (the dead record's aterm session, aterm's relaunch witnessed at this start)
    for (theirs, relaunch) in [
        // NEGATIVE CONTROLS: another tab's, and an older build's with none, at a person's
        // resume.
        (Some("fake"), false),
        (Some("s-another-tab"), false),
        (None, false),
        (Some("s-another-tab"), true),
        (None, true),
    ] {
        let case = format!("{theirs:?} relaunch={relaunch}");
        let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
        let rollout = home.path().join("rollout.jsonl");
        std::fs::write(&rollout, "{}\n").unwrap_or_else(|err| panic!("{err}"));
        let dir = record::sessions_dir(home.path());
        record::ensure_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
        let written_at = 1_000_000_000;
        dead_record(&dir, 999_968, THREAD, &line, written_at);
        let path = dir.join("999968.json");
        let mut value = read(&path);
        value["pace"] = word.clone();
        match theirs {
            Some(sid) => value["atermSession"] = sid.into(),
            None => {
                value
                    .as_object_mut()
                    .map(|fields| fields.remove("atermSession"));
            }
        }
        std::fs::write(
            &path,
            serde_json::to_vec(&value).unwrap_or_else(|err| panic!("{err}")),
        )
        .unwrap_or_else(|err| panic!("{err}"));
        // A person's resume three days on; aterm's relaunch a minute on.
        let mut own = launch(4294, true);
        own.started_at_ms = written_at + if relaunch { 60_000 } else { 3 * 24 * 3_600_000 };
        own.args = line.iter().map(OsString::from).collect();
        let started = own.started_at_ms;
        let shared = shared_with(own);
        let writer = Writer::new(shared, dir, Duration::ZERO, Some(FakeCli::command));
        writer.prepare();
        shared.observe(&frame(Some(&rollout), ProgramState::Idle, home.path()), 2);
        let _ = writer.write_now();
        let unprobed = read(&writer.path());
        let own_session = theirs == Some("fake");
        assert_eq!(
            unprobed.get("pace"),
            None,
            "{case}: before the probe, the initializer carries no pace"
        );
        assert!(
            unprobed
                .get("resumeArgv")
                .is_none_or(serde_json::Value::is_null),
            "{case}: nor a line"
        );
        assert!(
            path.exists(),
            "{case}: the predecessor's file keeps custody"
        );
        assert_eq!(unprobed["atermSession"], "fake", "{case}");
        shared.set_launch_witness(Some(LaunchWitness {
            hand: Some("lease:aterm-harness@812".into()),
            human: if relaunch {
                HumanSeen::Never
            } else {
                HumanSeen::At(started - 1)
            },
            read_at_ms: started + 1,
            server_pid: Some(812),
            hand_pid: HandPid::Absent,
        }));
        let _ = writer.write_now();
        let probed = read(&writer.path());
        assert_eq!(
            probed.get("pace"),
            (own_session || relaunch).then_some(&word),
            "{case}: after the probe"
        );
        assert!(
            probed.get("resumeArgv").is_some_and(|argv| !argv.is_null()),
            "{case}"
        );
        assert!(!path.exists(), "{case}: retired once the line is named");
    }
}

fn stale_record(dir: &Path, pid: u32, identity: Option<&ProcessIdentity>, updated_at: u64) {
    // Building one old record must not sweep the siblings the fixture is assembling.
    let staging = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(launch(pid, true));
    let writer = Writer::new(shared, staging.path().to_path_buf(), Duration::ZERO, None);
    writer.write_now().unwrap_or_else(|err| panic!("{err}"));
    record::ensure_dir(dir).unwrap_or_else(|err| panic!("{err}"));
    let path = dir.join(format!("{pid}.json"));
    let mut value = read(&writer.path());
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

    // Non-canonical filenames (`0.json`, leading zero, empty `.tmp` nonce) are ignored,
    // and a record whose JSON `pid` disagrees with its filename is removed without `on_taken`.
    for ignored in ["0.json", "0999996.json", "999996.json..tmp"] {
        std::fs::write(dir.join(ignored), b"{}").unwrap_or_else(|err| panic!("{err}"));
    }
    stale_record(&dir, 999_995, None, started - 1);
    let mismatched_path = dir.join("999995.json");
    let mut mismatched = read(&mismatched_path);
    mismatched["pid"] = 999_994.into();
    std::fs::write(
        &mismatched_path,
        serde_json::to_vec(&mismatched).unwrap_or_else(|err| panic!("{err}")),
    )
    .unwrap_or_else(|err| panic!("{err}"));
    let later = codex_alab_core::session::now_ms() + 60_000;
    let mut mismatched_seen = Vec::new();
    let removed_mismatched = record::sweep_with(
        &dir,
        std::process::id(),
        later,
        identity::is_live,
        |_| record::Sweep::Take,
        |pid, _| mismatched_seen.push(pid),
    );
    assert_eq!(removed_mismatched, 1);
    assert!(mismatched_seen.is_empty());
    assert!(!mismatched_path.exists());
    for ignored in ["0.json", "0999996.json", "999996.json..tmp"] {
        assert!(dir.join(ignored).exists(), "{ignored}");
    }
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
        let rollout = home.path().join("rollout.jsonl");
        std::fs::write(&rollout, "{}\n").unwrap_or_else(|err| panic!("{err}"));
        let mut current = frame(Some(&rollout), ProgramState::Working, home.path());
        current.primary_thread = Some(own_line[2]);
        current.displayed_thread = Some(own_line[2]);
        shared.observe(&current, 1);
        let writer = Writer::new(shared, dir.clone(), Duration::ZERO, Some(FakeCli::command));
        writer.prepare();
        let _ = writer.write_now();
        let first =
            record::read(&dir.join(format!("{pid}.json"))).unwrap_or_else(|| panic!("kept"));
        if shared.predecessor(own_line[2]).is_some() {
            assert!(
                dir.join("999981.json").exists(),
                "the first unprobed record keeps custody"
            );
            assert!(first.resume_argv.is_none());
        }
        shared.set_launch_witness(Some(codex_alab_core::session::LaunchWitness {
            hand: Some("lease:aterm-harness@812".into()),
            human: codex_alab_core::session::HumanSeen::Never,
            read_at_ms: 10_001,
            server_pid: Some(812),
            hand_pid: codex_alab_core::session::HandPid::Pid(812),
        }));
        writer.write_now().unwrap_or_else(|err| panic!("{err}"));
        assert!(record::read(&writer.path()).is_some_and(|record| record.resume_argv.is_some()));
        (shared.carried_on(), first.carried_on)
    };
    for chain in [1, 0] {
        chained(999_981, &own_line, chain);
        assert_eq!(start(4271 + chain), (chain, chain), "{chain}");
        assert!(
            !dir.join("999981.json").exists(),
            "its own predecessor: retired after concrete publication"
        );
        assert!(record::remove(&dir, 4271 + chain).is_ok());
    }
    // Negative control: another tab's chain is that tab's.
    chained(999_982, &other_line, 1);
    assert_eq!(start(4273), (0, 0));
    assert!(dir.join("999982.json").exists(), "another tab's: left");
}

/// THE ACT A CRASH AT REST OWED OUTLIVES A KILL OF THE RELAUNCH BEFORE ITS RECORD CARRIES IT
/// (CX-B B1, seen live in parity-3's land8: the relaunch's sweep removed the dead record, the
/// relaunch's first record withheld the act its launch probe had not proven yet and named the
/// line, a SIGKILL 2 ms after its claim beat the coalesced write that would have carried the
/// act, and the next relaunch, finding that record its predecessor, decided nothing: the done
/// check never came). The sweep holds the dead record (its file stays); until the launch probe
/// answers, every record the relaunch writes is the busy initializer, which names no line, so
/// the next start removes it as naming no relaunch and takes the dead record, act and all. The
/// probe's answer is written through (no coalescing: it commits the replacement), the record
/// then carries the act and names the line, and the dead record is retired after it. A probe
/// that proves no relaunch (a person's resume) settles the act void: nothing is laundered into
/// a later relaunch. Each case ends the relaunch with no finalize (a SIGKILL). NEGATIVE
/// CONTROL: the old ordering (the sweep removes the dead record, the first record names the
/// line) leaves the next start nothing.
#[test]
fn an_owed_act_outlives_a_kill_of_the_relaunch_before_its_record_carries_it() {
    use codex_alab_core::session::HandPid;
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::LaunchWitness;
    use codex_alab_core::session::NativeOrigin;
    use codex_alab_core::session::NativePhase;
    use codex_alab_core::session::OwedAct;
    let witness = |started: u64, person: bool| LaunchWitness {
        hand: Some("lease:aterm-harness@812".into()),
        human: if person {
            HumanSeen::At(started - 1)
        } else {
            HumanSeen::Never
        },
        read_at_ms: started + 1,
        server_pid: Some(812),
        hand_pid: HandPid::Absent,
    };
    for case in ["killed-unprobed", "killed-after-proof", "person"] {
        let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
        let dir = record::sessions_dir(home.path());
        let rollout = home.path().join("rollout.jsonl");
        std::fs::write(&rollout, "{}\n").unwrap_or_else(|err| panic!("{err}"));
        // The process that owed the done check at rest, its wait 240 s, then SIGKILLed.
        let dead = shared_with(launch(1_000_000_101, true));
        dead.observe(&frame(Some(&rollout), ProgramState::Idle, home.path()), 1);
        let act = OwedAct {
            renewal: None,
            thread: THREAD.into(),
            native_origin: Some(NativeOrigin {
                turn_id: "the-done-claims-turn".into(),
                phase: NativePhase::Complete,
            }),
            rule: Some("continue-done-check@v1".into()),
            text: Some("Defend it.".into()),
            mode: Some("cruise".into()),
            not_before_ms: codex_alab_core::session::now_ms() + 240_000,
            repo_owed: Some(Vec::new()),
            resume_goal: false,
            landing: false,
            landed: false,
        };
        dead.set_owed_act(Some(act.clone()));
        let writer = Writer::new(dead, dir.clone(), Duration::ZERO, Some(FakeCli::command));
        writer.write_now().unwrap_or_else(|err| panic!("{err}"));
        let source = record::read(&writer.path()).unwrap_or_else(|| panic!("the dead record"));
        assert_eq!(source.owed.clone().map(OwedAct::from), Some(act.clone()));
        let line = source
            .resume_argv
            .clone()
            .unwrap_or_else(|| panic!("its line"));
        let dead_path = writer.path();
        // aterm's relaunch of it: swept, framed, its first record written, then killed.
        let started = source.updated_at + 1;
        let mut first = launch(1_000_000_102, true);
        first.args = line.iter().map(OsString::from).collect();
        first.started_at_ms = started;
        let relaunch = shared_with(first);
        let relaunch_writer = Writer::new(
            relaunch,
            dir.clone(),
            session::WRITE_INTERVAL,
            Some(FakeCli::command),
        );
        relaunch_writer.prepare();
        relaunch.observe(&frame(Some(&rollout), ProgramState::Idle, home.path()), 2);
        relaunch_writer
            .write_now()
            .unwrap_or_else(|err| panic!("{err}"));
        let unprobed = record::read(&relaunch_writer.path()).unwrap_or_else(|| panic!("written"));
        assert_eq!(
            (unprobed.owed.is_some(), unprobed.resume_argv.is_some()),
            (false, false),
            "{case}: unproven, it withholds the act and so the line"
        );
        assert!(dead_path.exists(), "{case}: the dead record stays");
        let killed_at = if case == "killed-unprobed" {
            unprobed.updated_at
        } else {
            relaunch.set_launch_witness(Some(witness(started, case == "person")));
            assert!(
                relaunch_writer.urgent(),
                "{case}: the probe settles the handoff, written through"
            );
            relaunch_writer
                .write_now()
                .unwrap_or_else(|err| panic!("{err}"));
            let settled =
                record::read(&relaunch_writer.path()).unwrap_or_else(|| panic!("written"));
            assert_eq!(settled.resume_argv.as_ref(), Some(&line), "{case}");
            assert_eq!(
                settled.owed.clone().map(OwedAct::from),
                (case != "person").then(|| act.clone()),
                "{case}: carried where the probe proved aterm's relaunch"
            );
            assert!(!dead_path.exists(), "{case}: the dead record went after it");
            settled.updated_at
        };
        // The next relaunch, after the kill.
        let started = killed_at + 1;
        let mut second = launch(1_000_000_103, true);
        second.args = line.iter().map(OsString::from).collect();
        second.started_at_ms = started;
        let next = shared_with(second);
        next.set_launch_witness(Some(witness(started, false)));
        let next_writer = Writer::new(next, dir, Duration::ZERO, Some(FakeCli::command));
        next_writer.prepare();
        assert_eq!(
            next.owed_act(),
            (case != "person").then(|| act.clone()),
            "{case}: the next relaunch is handed the act"
        );
        next_writer
            .write_now()
            .unwrap_or_else(|err| panic!("{err}"));
        let carried = record::read(&next_writer.path()).unwrap_or_else(|| panic!("written"));
        assert_eq!(
            carried.owed.map(OwedAct::from),
            (case != "person").then(|| act.clone()),
            "{case}: and carries it on"
        );
        assert!(
            !dead_path.exists() && !relaunch_writer.path().exists(),
            "{case}: both predecessors' records are gone"
        );
    }
}

/// A PREDECESSOR KILLED BETWEEN ITS TEMPORARY'S SYNC AND ITS RENAME is retired by the
/// snapshot the sweep read (review of CX-B, CXR-4): its complete `<pid>.json.<nonce>.tmp` is
/// read and held like a record, and once this process's record carries the act every file of
/// that pid holding the snapshot goes, so a later start is handed nothing. Removed by pid,
/// only `<pid>.json` went, and the temporary handed the same act to every start after. Also
/// where the kill left both, the record and a newer temporary. NEGATIVE CONTROL: before the
/// probe answers, the temporary stands.
#[test]
fn a_predecessors_stray_temporary_goes_once_its_handoff_settles() {
    use codex_alab_core::session::HandPid;
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::LaunchWitness;
    use codex_alab_core::session::OwedAct;
    let witness = |started: u64| LaunchWitness {
        hand: Some("lease:aterm-harness@812".into()),
        human: HumanSeen::Never,
        read_at_ms: started + 1,
        server_pid: Some(812),
        hand_pid: HandPid::Absent,
    };
    for both in [false, true] {
        let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
        let dir = record::sessions_dir(home.path());
        let rollout = home.path().join("rollout.jsonl");
        std::fs::write(&rollout, "{}\n").unwrap_or_else(|err| panic!("{err}"));
        // The process that owed an act at rest, killed as it wrote its record again.
        let dead = shared_with(launch(1_000_000_201, true));
        dead.observe(&frame(Some(&rollout), ProgramState::Idle, home.path()), 1);
        let act = OwedAct {
            thread: THREAD.into(),
            rule: Some("continue-done-check@v1".into()),
            text: Some("Defend it.".into()),
            mode: Some("cruise".into()),
            not_before_ms: codex_alab_core::session::now_ms() + 240_000,
            ..Default::default()
        };
        dead.set_owed_act(Some(act.clone()));
        let writer = Writer::new(dead, dir.clone(), Duration::ZERO, Some(FakeCli::command));
        writer.write_now().unwrap_or_else(|err| panic!("{err}"));
        let source = record::read(&writer.path()).unwrap_or_else(|| panic!("the dead record"));
        let line = source
            .resume_argv
            .clone()
            .unwrap_or_else(|| panic!("its line"));
        let temporary = dir.join("1000000201.json.19a0deadbeef.tmp");
        if both {
            // The kill came at the next write: its temporary is the newer record.
            let mut newer = read(&writer.path());
            newer["updatedAt"] = (source.updated_at + 1).into();
            std::fs::write(
                &temporary,
                serde_json::to_vec(&newer).unwrap_or_else(|err| panic!("{err}")),
            )
            .unwrap_or_else(|err| panic!("{err}"));
        } else {
            std::fs::rename(writer.path(), &temporary).unwrap_or_else(|err| panic!("{err}"));
        }
        // aterm's relaunch of it.
        let started = source.updated_at + 2;
        let mut first = launch(1_000_000_202, true);
        first.args = line.iter().map(OsString::from).collect();
        first.started_at_ms = started;
        let relaunch = shared_with(first);
        let relaunch_writer = Writer::new(
            relaunch,
            dir.clone(),
            Duration::ZERO,
            Some(FakeCli::command),
        );
        relaunch_writer.prepare();
        assert_eq!(
            relaunch.owed_act(),
            Some(act.clone()),
            "both={both}: handed on"
        );
        relaunch.observe(&frame(Some(&rollout), ProgramState::Idle, home.path()), 2);
        relaunch_writer
            .write_now()
            .unwrap_or_else(|err| panic!("{err}"));
        assert!(
            temporary.exists(),
            "both={both}: unsettled, the temporary stands"
        );
        relaunch.set_launch_witness(Some(witness(started)));
        relaunch_writer
            .write_now()
            .unwrap_or_else(|err| panic!("{err}"));
        let settled = record::read(&relaunch_writer.path()).unwrap_or_else(|| panic!("written"));
        assert_eq!(settled.owed.map(OwedAct::from), Some(act));
        assert!(
            !temporary.exists() && !writer.path().exists(),
            "both={both}: settled, every file of the predecessor goes"
        );
        // The relaunch's orderly end, then another start of the same line.
        relaunch_writer
            .finalize(&Ending::Remove)
            .unwrap_or_else(|err| panic!("{err}"));
        let later = started + 10;
        let mut second = launch(1_000_000_203, true);
        second.args = line.iter().map(OsString::from).collect();
        second.started_at_ms = later;
        let next = shared_with(second);
        next.set_launch_witness(Some(witness(later)));
        let next_writer = Writer::new(next, dir, Duration::ZERO, Some(FakeCli::command));
        next_writer.prepare();
        assert_eq!(
            next.owed_act(),
            None,
            "both={both}: a later start is handed nothing"
        );
    }
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

    // Negative control: a change that leaves no rest waits for the interval: the next
    // keystroke (no first one: the draft already reads begun) and the frame it draws, whose
    // directory moved. The stamp alone would write nothing at all (it rides along).
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
    shared.observe(
        &Frame {
            cwd: Path::new("/work/next"),
            draft_empty: false,
            ..frame(None, ProgramState::Idle, home.path())
        },
        2,
    );
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(writer.writes(), writes, "coalesced: nothing written yet");
    let record = record::read(&dir.join("4252.json")).unwrap_or_else(|| panic!("parses"));
    assert_eq!(record.last_human_input_at, Some(10_000));
    assert_eq!(record.cwd.as_deref(), Some("/work"));

    // Idle to busy: the turn's start is written at once (the coalesced changes ride along).
    assert!(shared.turn_started(THREAD, "turn-1"));
    let busy = wait_record(&dir, 4252, |r| r.status == Status::Busy);
    assert_eq!(busy.last_human_input_at, Some(10_001));
    assert_eq!(busy.cwd.as_deref(), Some("/work/next"));
    writer
        .finalize(&Ending::Remove)
        .unwrap_or_else(|err| panic!("{err}"));
}

/// THE ACT OWED AND THE PACE ARE WRITTEN THROUGH (CONTRACT §4; B1, B3): an act decided,
/// typed or dropped and a pace that moved (an act typed, a wall's rung climbed) make the
/// record urgent, never coalesced, so a crash within the coalescing second loses neither, as
/// aterm writes its books at each act. NEGATIVE CONTROLS: the same act re-timed alone (a
/// deadline read again through another clock) and the same pace set again wait for the
/// interval; with nothing moved, nothing is urgent.
#[test]
fn the_owed_act_and_the_pace_are_written_through() {
    use codex_alab_core::session::OwedAct;
    use codex_alab_core::session::Pace;
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(launch(4254, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    let dir = record::sessions_dir(home.path());
    let writer = Writer::new(shared, dir, Duration::from_secs(3600), None);
    writer.prepare();
    writer.write_now().unwrap_or_else(|err| panic!("{err}"));
    assert!(!writer.urgent(), "nothing moved");
    let pace = |typed: Vec<u64>| Pace {
        short_streak: 1,
        typed_ms: typed,
        ..Pace::default()
    };
    shared.set_pace(THREAD, pace(vec![1_000]));
    assert!(writer.urgent(), "an act typed moved the pace");
    writer.write_now().unwrap_or_else(|err| panic!("{err}"));
    let written = record::read(&writer.path()).unwrap_or_else(|| panic!("written"));
    assert_eq!(written.pace.map(Pace::from), Some(pace(vec![1_000])));
    // NEGATIVE CONTROL: the same pace again moves nothing.
    shared.set_pace(THREAD, pace(vec![1_000]));
    assert!(!writer.urgent(), "the same pace");
    shared.set_pace(THREAD, pace(vec![1_000, 2_000]));
    assert!(writer.urgent(), "another act typed");
    writer.write_now().unwrap_or_else(|err| panic!("{err}"));
    let act = |not_before_ms: u64| OwedAct {
        renewal: None,
        thread: THREAD.into(),
        native_origin: None,
        rule: Some("continue-done-check@v1".into()),
        text: Some("Defend it.".into()),
        mode: Some("cruise".into()),
        not_before_ms,
        repo_owed: None,
        resume_goal: false,
        landing: false,
        landed: false,
    };
    shared.set_owed_act(Some(act(5_000)));
    assert!(writer.urgent(), "an act decided");
    writer.write_now().unwrap_or_else(|err| panic!("{err}"));
    // NEGATIVE CONTROL: re-timed alone, the act waits for the interval.
    shared.set_owed_act(Some(act(5_007)));
    assert!(!writer.urgent(), "the same act, re-timed");
    shared.set_owed_act(None);
    assert!(writer.urgent(), "the act typed or dropped");
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

#[test]
fn startup_sweeps_the_configured_home_before_any_frame() {
    codex_alab_core::activate();
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let dir = record::sessions_dir(home.path());
    record::ensure_dir(&dir).unwrap_or_else(|err| panic!("{err}"));
    let line = ["/twin/codex", "resume", THREAD];
    let mut own = launch(4280, true);
    own.args = line.iter().map(OsString::from).collect();
    dead_record(&dir, 999_980, THREAD, &line, own.started_at_ms - 1);
    let owed = codex_alab_core::session::OwedAct {
        thread: THREAD.into(),
        rule: Some("continue@v1".into()),
        text: Some("Keep going.".into()),
        mode: Some("cruise".into()),
        not_before_ms: own.started_at_ms + 60_000,
        ..Default::default()
    };
    let old_path = dir.join("999980.json");
    let mut source = read(&old_path);
    source["owed"] = serde_json::to_value(record::Owed::from(owed.clone()))
        .unwrap_or_else(|err| panic!("{err}"));
    std::fs::write(
        &old_path,
        serde_json::to_vec(&source).unwrap_or_else(|err| panic!("{err}")),
    )
    .unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(own);
    let slot = std::sync::OnceLock::new();
    assert!(
        shared.view().primary_thread.is_none(),
        "no frame was observed"
    );
    assert!(session::start_with(shared, home.path(), &slot));
    assert!(
        shared.wait_swept(Duration::from_secs(60)),
        "startup sweep hang detector"
    );
    let predecessor = shared
        .predecessor(THREAD)
        .unwrap_or_else(|| panic!("swept"));
    assert_eq!(predecessor.pid, 999_980);
    assert_eq!(predecessor.status.as_deref(), Some("busy"));
    assert_eq!(predecessor.owed_act, Some(owed.clone()));
    assert_eq!(shared.owed_act(), Some(owed));
    assert!(
        old_path.exists(),
        "the configured home's predecessor remains durable until its replacement is written"
    );
    assert!(
        shared.view().primary_thread.is_none(),
        "startup needs no frame"
    );
    let writer = slot.get().unwrap_or_else(|| panic!("writer admitted"));
    assert_eq!(writer.path(), dir.join("4280.json"));
    assert!(session::start_with(shared, home.path(), &slot));
    assert!(std::ptr::eq(
        *writer,
        *slot.get().unwrap_or_else(|| panic!("writer"))
    ));
    writer
        .finalize(&Ending::Remove)
        .unwrap_or_else(|err| panic!("{err}"));
}

#[test]
fn startup_without_an_aterm_launch_settles_without_touching_the_home() {
    codex_alab_core::activate();
    for own in [None, Some(launch(4281, false))] {
        let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        if let Some(own) = own {
            shared.set_launch(own);
        }
        let slot = std::sync::OnceLock::new();
        assert!(!session::start_with(shared, home.path(), &slot));
        assert!(shared.swept(), "no writer is a settled absence of evidence");
        assert!(slot.get().is_none());
        assert!(shared.predecessor(THREAD).is_none());
        assert!(
            std::fs::read_dir(home.path())
                .unwrap_or_else(|err| panic!("{err}"))
                .next()
                .is_none()
        );
    }
}

#[test]
fn an_occupied_startup_slot_keeps_its_shared_state_and_home() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let other_home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(launch(4282, true));
    let original: &'static Writer = Box::leak(Box::new(Writer::new(
        shared,
        record::sessions_dir(home.path()),
        Duration::ZERO,
        None,
    )));
    let slot = std::sync::OnceLock::new();
    slot.set(original).unwrap_or_else(|_| panic!("empty slot"));
    assert!(!shared.swept(), "the original writer has not prepared");

    let foreign = shared_with(launch(4283, true));
    assert!(!session::start_with(foreign, home.path(), &slot));
    assert!(foreign.swept(), "foreign startup settles its own absence");
    assert!(
        !shared.swept(),
        "another caller cannot finish the owner's sweep"
    );

    assert!(!session::start_with(shared, other_home.path(), &slot));
    assert!(
        !shared.swept(),
        "a refused home cannot finish the owner's sweep"
    );
    assert!(std::ptr::eq(
        original,
        *slot.get().unwrap_or_else(|| panic!("original writer"))
    ));
    assert_eq!(original.writes(), 0);
    assert!(shared.predecessor(THREAD).is_none());

    // The public root-thread hook cannot initialize an unrelated ambient writer.
    codex_alab_core::activate();
    let injected = shared_with(launch(4284, true));
    assert!(!session::start(injected, other_home.path()));
    assert!(injected.swept());
    assert!(!shared.swept());
    for dir in [home.path(), other_home.path()] {
        assert!(
            std::fs::read_dir(dir)
                .unwrap_or_else(|err| panic!("{err}"))
                .next()
                .is_none(),
            "refused startup writes no files"
        );
    }
}

#[test]
fn a_failed_writer_spawn_cannot_later_sweep_or_overwrite_its_source() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let pid = std::process::id();
    let dir = record::sessions_dir(home.path());
    let before = shared_with(launch(pid, true));
    before.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    let old = Writer::new(before, dir.clone(), Duration::ZERO, None);
    old.prepare();
    old.finalize(&Ending::Upgrading)
        .unwrap_or_else(|err| panic!("{err}"));
    let path = old.path();
    let original = std::fs::read(&path).unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(launch(pid, true));
    let writer: &'static Writer =
        Box::leak(Box::new(Writer::new(shared, dir, Duration::ZERO, None)));
    writer.install_write_through();
    let _ = writer.spawn_with(|_| Err(std::io::Error::other("injected writer spawn refusal")));
    assert!(
        shared.swept(),
        "a failed launch cannot strand resume readiness"
    );
    assert!(shared.inherited().is_none());
    assert!(
        !shared.flush_record(),
        "failed write-through grants no publication"
    );
    writer.prepare();
    writer.hold_status();
    assert!(writer.write_now().is_err());
    assert!(writer.finalize(&Ending::Signalled(libc::SIGTERM)).is_err());
    assert!(
        shared.inherited().is_none(),
        "later paths cannot revive failed startup"
    );
    assert_eq!(
        std::fs::read(path).unwrap_or_else(|err| panic!("{err}")),
        original
    );
    assert_eq!(writer.writes(), 0);
}

#[test]
fn writes_and_endings_before_spawn_capture_same_identity_inheritance_first() {
    for entry in ["write", "finalize", "signal"] {
        let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
        let pid = std::process::id();
        let dir = record::sessions_dir(home.path());
        let owed = codex_alab_core::session::OwedAct {
            thread: THREAD.into(),
            rule: Some("continue@v1".into()),
            text: Some("Keep going.".into()),
            mode: Some("cruise".into()),
            not_before_ms: 123_456,
            ..Default::default()
        };
        let before = shared_with(launch(pid, true));
        before.observe(&frame(None, ProgramState::Idle, home.path()), 1);
        before.set_owed_act(Some(owed.clone()));
        before.set_carried_on(1);
        let old = Writer::new(before, dir.clone(), Duration::ZERO, None);
        old.prepare();
        old.finalize(&Ending::Upgrading)
            .unwrap_or_else(|err| panic!("{err}"));
        let shared = shared_with(launch(pid, true));
        let writer = Writer::new(shared, dir, Duration::ZERO, None);
        match entry {
            "write" => assert_eq!(writer.write_now().ok(), Some(true)),
            "finalize" => writer
                .finalize(&Ending::Signalled(libc::SIGTERM))
                .unwrap_or_else(|err| panic!("{err}")),
            "signal" => {
                writer.hold_status();
                assert_eq!(writer.write_now().ok(), Some(true));
            }
            _ => unreachable!(),
        }
        let inherited = shared
            .inherited()
            .unwrap_or_else(|| panic!("{entry}: inherited before overwrite"));
        assert_eq!(inherited.exec_generation, 1, "{entry}");
        assert_eq!(inherited.owed_act, Some(owed), "{entry}");
        assert_eq!(shared.carried_on(), 1, "{entry}");
        let written = record::read(&writer.path()).unwrap_or_else(|| panic!("{entry}: written"));
        assert_eq!(written.exec_generation, 1, "{entry}");
    }
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
        let writer = Writer::new(shared, dir.clone(), Duration::ZERO, Some(FakeCli::command));
        writer.prepare();
        (shared, writer)
    };
    let publish = |shared: &Shared, writer: &Writer, line: &[&str]| {
        let rollout = dir.join(format!(
            "rollout-{}.jsonl",
            shared.launch().unwrap_or_else(|| panic!("launch")).pid
        ));
        std::fs::write(&rollout, "{}\n").unwrap_or_else(|err| panic!("{err}"));
        let mut current = frame(Some(&rollout), ProgramState::Idle, home.path());
        current.primary_thread = Some(line[2]);
        current.displayed_thread = Some(line[2]);
        shared.observe(&current, now + 1);
        shared.set_launch_witness(Some(codex_alab_core::session::LaunchWitness {
            hand: Some("lease:aterm-harness@812".into()),
            human: codex_alab_core::session::HumanSeen::Never,
            read_at_ms: now + 1,
            server_pid: Some(812),
            hand_pid: codex_alab_core::session::HandPid::Pid(812),
        }));
        writer.write_now().unwrap_or_else(|err| panic!("{err}"));
        assert!(record::read(&writer.path()).is_some_and(|record| record.resume_argv.is_some()));
    };
    let (first, first_writer) = start(4261, &a);
    assert_eq!(first.predecessor(a[2]).map(|p| p.pid), Some(999_990));
    assert_eq!(
        first.predecessor(b[2]),
        None,
        "another tab's record is not this one's"
    );
    assert!(
        dir.join("999990.json").exists(),
        "its own: held until concrete publication"
    );
    assert!(
        dir.join("999991.json").exists(),
        "the other tab's record stands for its own relaunch"
    );
    assert!(
        !dir.join("999992.json").exists(),
        "past the horizon: removed"
    );
    assert!(!dir.join("999993.json").exists(), "no resume line: removed");
    publish(first, &first_writer, &a);
    assert!(!dir.join("999990.json").exists());
    assert!(
        dir.join("999991.json").exists(),
        "publishing the first leaves the second's custody untouched"
    );
    let (second, second_writer) = start(4262, &b);
    assert_eq!(second.predecessor(b[2]).map(|p| p.pid), Some(999_991));
    assert!(
        dir.join("999991.json").exists(),
        "its own startup keeps custody"
    );
    publish(second, &second_writer, &b);
    assert!(
        !dir.join("999991.json").exists(),
        "retired only after its own publication"
    );
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

#[test]
fn the_record_preserves_an_admitted_auth_path_in_env() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let auth = home.path().join("pool").join("acct-1").join("auth.json");
    let auth_str = auth.to_str().unwrap_or_else(|| panic!("utf8")).to_owned();

    let mut valid = launch(4290, true);
    valid
        .vars
        .push(("CODEX_AUTH_PATH".into(), OsString::from(&auth_str)));
    let shared = shared_with(valid);
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 10);
    let dir = record::sessions_dir(home.path());
    let writer = Writer::new(shared, dir.clone(), Duration::ZERO, None);
    writer.prepare();
    assert_eq!(writer.write_now().ok(), Some(true));
    let value = read(&dir.join("4290.json"));
    assert_eq!(
        value["env"],
        serde_json::json!({
            "CODEX_AUTH_PATH": auth_str,
            "CODEX_HOME": "/h/.codex",
        })
    );

    // Negative controls: relative paths, directories and empty strings never enter `env`.
    for (pid, rejected) in [
        (4291, OsString::from("relative/auth.json")),
        (4292, OsString::new()),
        (4293, home.path().as_os_str().to_os_string()),
    ] {
        let mut bad = launch(pid, true);
        bad.vars.push(("CODEX_AUTH_PATH".into(), rejected.clone()));
        let bad_shared = shared_with(bad);
        bad_shared.observe(&frame(None, ProgramState::Idle, home.path()), 10);
        let bad_writer = Writer::new(bad_shared, dir.clone(), Duration::ZERO, None);
        bad_writer.prepare();
        assert_eq!(bad_writer.write_now().ok(), Some(true));
        let bad_value = read(&dir.join(format!("{pid}.json")));
        assert_eq!(
            bad_value["env"],
            serde_json::json!({"CODEX_HOME": "/h/.codex"}),
            "rejected CODEX_AUTH_PATH={rejected:?}"
        );
    }
}

#[test]
fn the_record_publishes_only_the_primary_threads_active_goal() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let dir = record::sessions_dir(home.path());
    let shared = shared_with(launch(4295, true));
    let writer = Writer::new(shared, dir.clone(), Duration::ZERO, None);
    writer.prepare();

    shared.observe(&frame(None, ProgramState::Idle, home.path()), 10);
    assert_eq!(writer.write_now().ok(), Some(true));
    assert!(
        read(&dir.join("4295.json")).get("goal").is_none(),
        "absent before an active goal is set"
    );

    // Trimmed, UTF-8 char-boundary bounded, and preserved when displaying a sub-agent thread.
    let long = format!("  ship native codex-alab {}🚀  ", "x".repeat(9000));
    shared.set_goal(THREAD, Some(&long));
    shared.set_goal("subagent-thread", Some("subagent objective"));
    let mut subagent_frame = frame(None, ProgramState::Working, home.path());
    subagent_frame.displayed_thread = Some("subagent-thread");
    shared.observe(&subagent_frame, 20);
    assert_eq!(writer.write_now().ok(), Some(true));
    let published = read(&dir.join("4295.json"));
    let goal = published["goal"].as_str().expect("active primary goal");
    assert!(goal.starts_with("ship native codex-alab "));
    assert!(goal.len() <= codex_alab_core::session::GOAL_MAX_BYTES);

    // Clearing or pausing (`None`) removes `goal` from the serialized record; empty/whitespace is ignored as None.
    shared.set_goal(THREAD, Some("   \n\t  "));
    assert_eq!(writer.write_now().ok(), Some(true));
    assert!(
        read(&dir.join("4295.json")).get("goal").is_none(),
        "cleared/non-active goal is omitted"
    );
}

/// The record's `backgroundTerminals` is the count the last frame handed over: absent before
/// one, written both ways (a rise and a fall to 0) and never written through, and the same
/// count again is no write. (A writer with no interval holds no move.)
#[test]
fn the_record_counts_the_background_terminals_the_footer_shows() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let dir = record::sessions_dir(home.path());
    let shared = shared_with(launch(4296, true));
    let writer = Writer::new(shared, dir.clone(), Duration::ZERO, None);
    writer.prepare();

    shared.observe(&frame(None, ProgramState::Idle, home.path()), 10);
    assert_eq!(writer.write_now().ok(), Some(true));
    assert!(
        read(&dir.join("4296.json"))
            .get("backgroundTerminals")
            .is_none(),
        "absent before a frame handed a count"
    );

    shared.set_background_terminals(2, Instant::now());
    assert!(!writer.urgent(), "a count is never written through");
    assert_eq!(writer.write_now().ok(), Some(true));
    assert_eq!(
        read(&dir.join("4296.json"))["backgroundTerminals"],
        serde_json::json!(2)
    );

    shared.set_background_terminals(0, Instant::now());
    assert!(!writer.urgent(), "nor is its fall");
    assert_eq!(writer.write_now().ok(), Some(true), "a fall is a write");
    assert_eq!(
        read(&dir.join("4296.json"))["backgroundTerminals"],
        serde_json::json!(0)
    );

    shared.set_background_terminals(0, Instant::now());
    assert_eq!(
        writer.write_now().ok(),
        Some(false),
        "the same count is none"
    );
}

/// A MOVE OF THE COUNT ALONE WAITS OUT THE INTERVAL (CONTRACT §4): a command's start and its
/// end each move the footer's count, and the writer's leading edge wrote both, a full sync
/// each. A move that alone changes the record is written once the count has stood the
/// interval, or rides along on the write another change causes, so a command that ends within
/// the interval costs no write.
#[test]
fn a_count_that_alone_moves_the_record_waits_out_the_interval_or_rides_along() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(launch(4297, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    let dir = record::sessions_dir(home.path());
    let hour = Duration::from_secs(3600);
    let writer = Writer::new(shared, dir.clone(), hour, None);
    writer.prepare();
    let write = || writer.write_now().unwrap_or_else(|err| panic!("{err}"));
    let path = dir.join("4297.json");
    assert!(write(), "the first record");

    shared.set_background_terminals(1, Instant::now());
    assert!(!write(), "a command's start alone is held");
    shared.set_background_terminals(0, Instant::now());
    assert!(
        !write(),
        "and its end within the interval writes nothing either"
    );
    assert!(read(&path).get("backgroundTerminals").is_none());

    shared.set_background_terminals(2, Instant::now());
    let cwd = PathBuf::from("/work/next");
    shared.observe(
        &Frame {
            cwd: &cwd,
            ..frame(None, ProgramState::Idle, home.path())
        },
        2,
    );
    assert!(write(), "another change writes the record");
    assert_eq!(
        read(&path)["backgroundTerminals"],
        serde_json::json!(2),
        "and the held count rides along"
    );

    shared.set_background_terminals(0, Instant::now());
    assert!(!write(), "held");
    assert!(shared.expire_background_terminals_hold(Instant::now() + hour, hour));
    assert!(write(), "a count that has stood the interval is written");
    assert_eq!(read(&path)["backgroundTerminals"], serde_json::json!(0));
}

/// The writer wakes for a held count by itself: with nothing else to write, a move is on disk
/// once it has stood the interval.
#[test]
fn the_writer_wakes_to_write_a_held_count() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let shared = shared_with(launch(4298, true));
    shared.observe(&frame(None, ProgramState::Idle, home.path()), 1);
    let dir = record::sessions_dir(home.path());
    let writer: &'static Writer = Box::leak(Box::new(Writer::new(
        shared,
        dir.clone(),
        Duration::from_millis(50),
        None,
    )));
    writer.spawn();
    let first = wait_record(&dir, 4298, |_| true);
    assert_eq!(first.background_terminals, None);
    shared.set_background_terminals(2, Instant::now());
    let counted = wait_record(&dir, 4298, |record| record.background_terminals.is_some());
    assert_eq!(counted.background_terminals, Some(2));
    let _ = writer.finalize(&Ending::Remove);
}

/// Native auth restart has its own sealed host authority, not the generic
/// crash relaunch's Drive lease. The writer carries only the existing inert
/// marker and keeps its genuine predecessor until the host Consume is acked.
#[test]
fn native_renewal_custody_survives_a_cold_restart_without_a_drive_lease() {
    use codex_alab_core::account_handoff::Purpose;
    for purpose in [Purpose::Login, Purpose::WeeklyQuota] {
        native_account_custody_survives_restart(purpose);
    }
}

fn native_account_custody_survives_restart(purpose: codex_alab_core::account_handoff::Purpose) {
    use codex_alab_core::session::AccountHandoff;
    use codex_alab_core::session::HandPid;
    use codex_alab_core::session::HumanSeen;
    use codex_alab_core::session::LaunchWitness;
    use codex_alab_core::session::NativeOrigin;
    use codex_alab_core::session::NativePhase;
    use codex_alab_core::session::OwedAct;
    let home = tempfile::tempdir().unwrap();
    let dir = record::sessions_dir(home.path());
    let rollout = home.path().join("rollout.jsonl");
    std::fs::write(&rollout, "{}\n").unwrap();
    let receipt = "c".repeat(32);
    let wall = "019a0000-0000-7000-8000-000000000002";
    let mut original_launch = launch(1_000_000_301, true);
    original_launch
        .vars
        .push(("CODEX_AUTH_PATH".into(), "/old-profile/auth.json".into()));
    let original = shared_with(original_launch);
    original.observe(&frame(Some(&rollout), ProgramState::Idle, home.path()), 1);
    let act = OwedAct {
        thread: THREAD.into(),
        native_origin: Some(NativeOrigin {
            turn_id: wall.into(),
            phase: NativePhase::Complete,
        }),
        renewal: Some(AccountHandoff {
            purpose,
            destination: (purpose == codex_alab_core::account_handoff::Purpose::WeeklyQuota).then(
                || codex_alab_core::account_handoff::Destination {
                    account: "d".repeat(64),
                    user: "e".repeat(64),
                },
            ),
            openai_scope: true,
            local: false,
            account: "a".repeat(64),
            user: "b".repeat(64),
            receipt: Some(receipt.clone()),
            turn: None,
            accepted: false,
            starts: 1,
            goal_id: None,
        }),
        rule: Some(purpose.rule().into()),
        text: Some("The existing login return marker.".into()),
        mode: Some("land".into()),
        ..Default::default()
    };
    assert!(original.publish_renewal(original.upgrade_generation(), act.clone()));
    let before = Writer::new(
        original,
        dir.clone(),
        Duration::ZERO,
        Some(FakeCli::command),
    );
    before.write_now().unwrap();
    before.finalize(&Ending::Signalled(15)).unwrap();
    let source = record::read(&before.path()).unwrap();
    assert_eq!(source.owed.clone().map(OwedAct::from), Some(act.clone()));
    let line = source.resume_argv.clone().unwrap();
    let mut next_launch = launch(1_000_000_302, true);
    next_launch.args = line.iter().map(OsString::from).collect();
    next_launch.started_at_ms = source.updated_at + 1;
    next_launch
        .vars
        .push(("CODEX_AUTH_PATH".into(), "/new-profile/auth.json".into()));
    let next = shared_with(next_launch);
    let no_drive = LaunchWitness {
        hand: None,
        human: HumanSeen::Never,
        read_at_ms: source.updated_at + 2,
        server_pid: Some(812),
        hand_pid: HandPid::Absent,
    };
    assert!(!no_drive.proves_relaunch(source.updated_at + 1));
    next.set_launch_witness(Some(no_drive));
    let after = Writer::new(next, dir, Duration::ZERO, Some(FakeCli::command));
    after.prepare();
    assert_eq!(
        next.owed_act(),
        Some(act.clone()),
        "existing marker retained in memory"
    );
    next.observe(&frame(Some(&rollout), ProgramState::Idle, home.path()), 2);
    after.write_now().unwrap();
    let current = record::read(&after.path()).unwrap();
    assert_eq!(
        current.owed.map(OwedAct::from),
        Some(act.clone()),
        "inert native custody is durably published without a generic Drive grant"
    );
    assert!(
        before.path().exists(),
        "native RecordWitness still owns the original pending marker"
    );
    let mut reserved = act;
    reserved.renewal.as_mut().unwrap().turn = Some(wall.into());
    assert!(next.publish_renewal(next.upgrade_generation(), reserved));
    assert!(next.admit_recovered_turn(THREAD, wall));
    assert!(next.turn_started(THREAD, wall));
    next.turn_ended(THREAD, wall);
    after.write_now().unwrap();
    assert!(
        before.path().exists(),
        "accepted same-ID work still awaits native Consume acknowledgment"
    );
    assert!(
        next.clear_renewal(&receipt),
        "existing clear API follows verified native Consume"
    );
    after.write_now().unwrap();
    assert!(
        !before.path().exists(),
        "only acknowledged custody can retire its predecessor"
    );
}

#[test]
fn native_custody_rejects_other_contexts_and_retires_only_known_supersession() {
    use codex_alab_core::session::AccountHandoff;
    use codex_alab_core::session::NativeOrigin;
    use codex_alab_core::session::NativePhase;
    use codex_alab_core::session::OwedAct;
    for (index, case) in [
        "other-sid",
        "other-argv",
        "no-receipt",
        "local-only",
        "missing-source",
        "new-native",
        "new-goal",
        "new-profile",
    ]
    .into_iter()
    .enumerate()
    {
        let home = tempfile::tempdir().unwrap();
        let dir = record::sessions_dir(home.path());
        let rollout = home.path().join("rollout.jsonl");
        std::fs::write(&rollout, "{}\n").unwrap();
        let original = shared_with(launch(1_000_000_400 + index as u32 * 2, true));
        original.observe(&frame(Some(&rollout), ProgramState::Idle, home.path()), 1);
        let act = OwedAct {
            thread: THREAD.into(),
            native_origin: Some(NativeOrigin {
                turn_id: "original-wall".into(),
                phase: NativePhase::Complete,
            }),
            renewal: Some(AccountHandoff {
                purpose: codex_alab_core::account_handoff::Purpose::Login,
                destination: None,
                openai_scope: true,
                local: case == "local-only",
                account: "a".repeat(64),
                user: "b".repeat(64),
                receipt: (case != "no-receipt").then(|| "c".repeat(32)),
                turn: None,
                accepted: false,
                starts: 1,
                goal_id: Some("original-goal".into()),
            }),
            rule: Some("login-back@v1".into()),
            text: Some("Existing inert marker.".into()),
            mode: Some("land".into()),
            ..Default::default()
        };
        assert!(original.publish_renewal(original.upgrade_generation(), act.clone()));
        let before = Writer::new(
            original,
            dir.clone(),
            Duration::ZERO,
            Some(FakeCli::command),
        );
        before.write_now().unwrap();
        let mut source = record::read(&before.path()).unwrap();
        if case == "missing-source" {
            before.finalize(&Ending::Remove).unwrap();
        } else {
            before.finalize(&Ending::Signalled(15)).unwrap();
            source = record::read(&before.path()).unwrap();
        }
        let mut next_launch = launch(1_000_000_401 + index as u32 * 2, true);
        next_launch.args = source
            .resume_argv
            .as_ref()
            .unwrap()
            .iter()
            .map(OsString::from)
            .collect();
        next_launch.started_at_ms = source.updated_at + 1;
        if case == "other-sid" {
            next_launch
                .vars
                .iter_mut()
                .find(|(key, _)| key == "ATERM_PARENT_SESSION_ID")
                .unwrap()
                .1 = "another-tab".into();
        }
        if case == "other-argv" {
            next_launch.args[2] = "another-model".into();
        }
        let next = shared_with(next_launch);
        next.set_launch_witness(None);
        let after = Writer::new(next, dir, Duration::ZERO, Some(FakeCli::command));
        after.prepare();
        next.observe(&frame(Some(&rollout), ProgramState::Idle, home.path()), 2);
        after.write_now().unwrap();
        let mut explicitly_published = None;
        if matches!(case, "new-native" | "new-goal" | "new-profile") {
            assert!(
                record::read(&after.path()).unwrap().owed.is_some(),
                "{case}: begin with actual inert custody"
            );
            assert!(before.path().exists());
            if case == "new-native" {
                assert!(next.turn_started(THREAD, "a-genuinely-new-native-id"));
            } else {
                let mut changed = act;
                let renewal = changed.renewal.as_mut().unwrap();
                if case == "new-goal" {
                    renewal.goal_id = Some("human-new-goal".into());
                } else {
                    renewal.account = "d".repeat(64);
                }
                assert!(next.publish_renewal(next.upgrade_generation(), changed.clone()));
                explicitly_published = Some(changed);
            }
            after.write_now().unwrap();
            assert!(
                !before.path().exists(),
                "{case}: superseded custody does not retain old authority"
            );
        }
        assert_eq!(
            record::read(&after.path()).unwrap().owed.map(OwedAct::from),
            explicitly_published,
            "{case}: old custody cannot replace an explicitly published new intent"
        );
        if matches!(case, "other-sid" | "other-argv") {
            assert!(
                before.path().exists(),
                "{case}: another owner's genuine source is left intact"
            );
        }
        if case != "new-native" {
            assert!(
                next.latest_turn(THREAD).is_none(),
                "{case}: custody never starts model work"
            );
        }
    }
}

#[test]
fn rollout_materialized_and_ensure_dir_reject_symlinks() {
    let home = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let real_rollout = home.path().join("rollout.jsonl");
    std::fs::write(&real_rollout, "{}\n").unwrap_or_else(|err| panic!("{err}"));
    let link_rollout = home.path().join("rollout-link.jsonl");
    std::os::unix::fs::symlink(&real_rollout, &link_rollout).unwrap_or_else(|err| panic!("{err}"));
    assert!(crate::session::rollout_materialized(&real_rollout));
    assert!(!crate::session::rollout_materialized(&link_rollout));

    let real_dir = home.path().join("real-sessions");
    std::fs::create_dir(&real_dir).unwrap_or_else(|err| panic!("{err}"));
    let link_dir = home.path().join("link-sessions");
    std::os::unix::fs::symlink(&real_dir, &link_dir).unwrap_or_else(|err| panic!("{err}"));
    assert!(record::ensure_dir(&link_dir).is_err());
}
