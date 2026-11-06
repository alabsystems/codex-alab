// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ALAB(life) test glue, compiled into codex-cli's `codex` binary tests as
//! `alab_resume_tests` (one `#[cfg(test)] #[path]` line at the end of `cli/src/main.rs`):
//! the record's `resumeArgv` rewrite against the real `MultitoolCli`, so an upstream change
//! to codex's command line that breaks the rewrite fails here, not in a relaunch.

use super::MultitoolCli;
use clap::CommandFactory;
use pretty_assertions::assert_eq;
use std::ffi::OsString;

const THREAD: &str = "019a0000-0000-7000-8000-0000000000aa";

fn rebuild(args: &[&str]) -> Option<Vec<String>> {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    codex_alab_tui::resume_argv::rebuild(MultitoolCli::command(), &args, "/twin/codex", THREAD)
}

fn resumed(tail: &[&str]) -> Option<Vec<String>> {
    Some(
        std::iter::once("/twin/codex")
            .chain(tail.iter().copied())
            .map(str::to_owned)
            .collect(),
    )
}

fn rebuild_exec(args: &[&str]) -> Option<Vec<String>> {
    let args = args.iter().map(OsString::from).collect::<Vec<_>>();
    codex_alab_tui::resume_argv::rebuild_exec(MultitoolCli::command(), &args, "/twin/codex", THREAD)
}

#[test]
fn exec_resume_preserves_output_contract_without_replaying_input() {
    assert_eq!(
        rebuild_exec(&[
            "codex",
            "-c",
            "model_reasoning_effort=high",
            "exec",
            "--json",
            "--output-schema",
            "/tmp/schema.json",
            "-o",
            "/tmp/answer.txt",
            "--skip-git-repo-check",
            "--color",
            "never",
            "-i",
            "input.png",
            "do this task"
        ]),
        resumed(&[
            "-c",
            "model_reasoning_effort=high",
            "exec",
            "--json",
            "--output-schema",
            "/tmp/schema.json",
            "-o",
            "/tmp/answer.txt",
            "--skip-git-repo-check",
            "--color",
            "never",
            "resume",
            THREAD
        ])
    );
    assert_eq!(
        rebuild_exec(&[
            "codex", "exec", "--json", "resume", "--last", "--all", "continue"
        ]),
        resumed(&["exec", "--json", "resume", THREAD])
    );
}

#[test]
fn exec_resume_rejects_nondurable_and_different_task_modes() {
    for args in [
        vec!["codex", "exec", "--ephemeral", "task"],
        vec!["codex", "exec", "review", "--uncommitted"],
        vec!["codex", "task"],
    ] {
        assert!(rebuild_exec(&args).is_none(), "{args:?}");
    }
}

fn config_scopes(args: &[String]) -> (Vec<String>, Vec<String>) {
    let cli = super::config_override_args::try_parse_from(args).expect("parse scoped overrides");
    let child = match cli.subcommand.expect("session subcommand") {
        super::Subcommand::Resume(command) => command.config_overrides.0.config_overrides,
        super::Subcommand::Fork(command) => command.config_overrides.0.config_overrides,
        _ => panic!("expected a resumable session command"),
    };
    (cli.config_overrides.raw_overrides, child.raw_overrides)
}

fn assert_scoped_overrides_survive(launch: &[&str], expected_tail: &[&str]) {
    let launched: Vec<String> = launch.iter().map(|arg| (*arg).to_owned()).collect();
    let scopes = config_scopes(&launched);
    assert_eq!(scopes.0, ["a=1", "b=2", "a=3", "a=4"]);
    // TuiCli skips its typed config field. The new root parser must retain
    // both scopes, including duplicate keys, rather than only Clap's last scope.
    assert!(scopes.1.is_empty());

    let rebuilt = rebuild(launch).expect("rebuild scoped session command");
    assert_eq!(Some(rebuilt.clone()), resumed(expected_tail));
    assert_eq!(config_scopes(&rebuilt), scopes);

    // Both mutations remain valid argv. Compare the actual typed parse so the
    // control cannot pass merely because formatting or a selector changed.
    let mut dropped = rebuilt.clone();
    let root = dropped
        .iter()
        .position(|arg| arg == "--config=a=1")
        .expect("root override");
    dropped.remove(root);
    assert_ne!(config_scopes(&dropped), scopes);

    let mut reordered = rebuilt;
    let first = reordered
        .iter()
        .position(|arg| arg == "-ca=3")
        .expect("first child override");
    let second = reordered
        .iter()
        .position(|arg| arg == "--config=a=4")
        .expect("last child override");
    reordered.swap(first, second);
    assert_ne!(config_scopes(&reordered), scopes);
}

#[test]
fn the_prompt_and_images_go_and_every_other_root_flag_stays() {
    assert_eq!(
        rebuild(&[
            "codex",
            "-m",
            "gpt-5.1",
            "--yolo",
            "-c",
            "model_reasoning_effort=high",
            "--enable",
            "goals",
            "-i",
            "a.png,b.png",
            "--search",
            "--no-alt-screen",
            "fix the build",
        ]),
        resumed(&[
            "-m",
            "gpt-5.1",
            "--yolo",
            "-c",
            "model_reasoning_effort=high",
            "--enable",
            "goals",
            "--search",
            "--no-alt-screen",
            "resume",
            THREAD,
        ])
    );
}

#[test]
fn equals_forms_and_short_clusters_stay_verbatim() {
    assert_eq!(
        rebuild(&[
            "codex",
            "--image=x.png",
            "-mo3",
            "-a",
            "never",
            "--sandbox=read-only",
            "hi",
        ]),
        resumed(&[
            "-mo3",
            "-a",
            "never",
            "--sandbox=read-only",
            "resume",
            THREAD
        ])
    );
}

#[test]
fn resume_keeps_its_own_flags_and_drops_its_selectors() {
    assert_eq!(
        rebuild(&[
            "codex",
            "-c",
            "a=1",
            "resume",
            "--last",
            "--all",
            "--include-non-interactive",
            "-m",
            "o3",
            "--search",
        ]),
        resumed(&["-c", "a=1", "resume", "-m", "o3", "--search", THREAD])
    );
    assert_eq!(
        rebuild(&[
            "codex",
            "resume",
            "019a0000-0000-7000-8000-000000000001",
            "go on"
        ]),
        resumed(&["resume", THREAD])
    );
    assert_eq!(
        rebuild(&[
            "codex",
            "--image=a.png",
            "resume",
            "019a0000-0000-7000-8000-000000000001",
        ]),
        resumed(&["resume", THREAD])
    );
    assert_eq!(
        rebuild(&[
            "codex",
            "-ia.png",
            "fork",
            "019a0000-0000-7000-8000-000000000001",
        ]),
        resumed(&["resume", THREAD])
    );
    assert_scoped_overrides_survive(
        &[
            "codex",
            "--config=a=1",
            "-c",
            "b=2",
            "resume",
            "-ca=3",
            "--config=a=4",
            "019a0000-0000-7000-8000-000000000001",
            "go on",
        ],
        &[
            "--config=a=1",
            "-c",
            "b=2",
            "resume",
            "-ca=3",
            "--config=a=4",
            THREAD,
        ],
    );
}

#[test]
fn a_fork_resumes_the_forked_thread() {
    assert_eq!(
        rebuild(&[
            "codex",
            "fork",
            "019a0000-0000-7000-8000-000000000001",
            "-m",
            "o3",
            "try again",
        ]),
        resumed(&["resume", "-m", "o3", THREAD])
    );
    assert_scoped_overrides_survive(
        &[
            "codex",
            "--config=a=1",
            "-c",
            "b=2",
            "fork",
            "-ca=3",
            "--config=a=4",
            "019a0000-0000-7000-8000-000000000001",
            "try again",
        ],
        &[
            "--config=a=1",
            "-c",
            "b=2",
            "resume",
            "-ca=3",
            "--config=a=4",
            THREAD,
        ],
    );
}

#[test]
fn agents_exec_and_the_rest_are_refused() {
    assert_eq!(rebuild(&["codex", "agents"]), None);
    assert_eq!(rebuild(&["codex", "exec", "hello"]), None);
    assert_eq!(rebuild(&["codex", "e", "hello"]), None);
    assert_eq!(rebuild(&["codex", "review"]), None);
    assert_eq!(rebuild(&["codex", "--worktree"]), None);
    assert_eq!(
        rebuild(&[
            "codex",
            "fork",
            "--worktree",
            "019a0000-0000-7000-8000-000000000001"
        ]),
        None
    );
    assert_eq!(rebuild(&["codex", "--remote", "ws://127.0.0.1:8080"]), None);
    assert_eq!(
        rebuild(&["codex", "--remote-auth-token-env", "MY_TOKEN"]),
        None
    );
    assert_eq!(rebuild(&["codex", "--definitely-not-a-flag"]), None);
    assert_eq!(
        rebuild(&["codex", "--model=o3", "-m", "o4"]),
        None,
        "clap refuses it too"
    );
}
