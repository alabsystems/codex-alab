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
}

#[test]
fn agents_exec_and_the_rest_are_refused() {
    assert_eq!(rebuild(&["codex", "agents"]), None);
    assert_eq!(rebuild(&["codex", "exec", "hello"]), None);
    assert_eq!(rebuild(&["codex", "e", "hello"]), None);
    assert_eq!(rebuild(&["codex", "review"]), None);
    assert_eq!(rebuild(&["codex", "--definitely-not-a-flag"]), None);
    assert_eq!(
        rebuild(&["codex", "--model=o3", "-m", "o4"]),
        None,
        "clap refuses it too"
    );
}
