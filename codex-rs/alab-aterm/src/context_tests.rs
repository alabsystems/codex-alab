// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use pretty_assertions::assert_eq;

use super::*;
use crate::Options;
use crate::fake::FakeAterm;
use crate::fake::Mode;

async fn fake() -> (tempfile::TempDir, FakeAterm, Client) {
    let dir = tempfile::tempdir().expect("scratch dir");
    let fake = FakeAterm::start(dir.path(), Mode::Current)
        .await
        .expect("fake aterm");
    let client = Client::connect(
        fake.endpoint(),
        Options {
            timeout: Duration::from_millis(500),
            ..Options::default()
        },
    )
    .await
    .expect("connect");
    (dir, fake, client)
}

#[test]
fn a_blocks_row_reads_as_aterm_prints_it() {
    assert_eq!(
        Block::parse(
            "block 5 complete exit=101 prompt=5 cmd=5 out=6 end=40 cwd=/Users/_owner/x \
             cmdline=cargo%20test%20-p%20foo"
        ),
        Some(Block {
            id: 5,
            state: "complete".to_string(),
            exit: Some(101),
            cwd: Some("/Users/_owner/x".to_string()),
            cmdline: "cargo test -p foo".to_string(),
        })
    );
    let running =
        Block::parse("block 6 executing exit=- prompt=7 cmd=7 out=8 end=- cwd=/x cmdline=codex")
            .expect("row");
    assert_eq!(running.exit, None);
    assert_eq!(Block::parse("layout tab=0"), None);
}

#[tokio::test]
async fn the_context_is_this_tabs_history_the_selection_and_the_splits_failure() {
    let (_dir, fake, client) = fake().await;
    fake.set_blocks(
        "0",
        &[
            "block 1 complete exit=0 prompt=1 cmd=1 out=2 end=2 cwd=/w cmdline=cd%20w",
            "block 2 complete exit=0 prompt=2 cmd=2 out=3 end=3 cwd=/w cmdline=git%20pull",
            "block 3 complete exit=2 prompt=3 cmd=3 out=4 end=9 cwd=/w cmdline=make",
            "block 4 executing exit=- prompt=9 cmd=9 out=10 end=- cwd=/w cmdline=codex",
        ],
    );
    fake.set_selection("0", &["fn main() {", "    todo!()", "}"]);
    fake.add_pane("3");
    fake.set_blocks(
        "3",
        &[
            "block 7 complete exit=101 prompt=1 cmd=1 out=2 end=50 cwd=/w cmdline=cargo%20test",
            "block 8 complete exit=0 prompt=50 cmd=50 out=51 end=52 cwd=/w cmdline=ls",
        ],
    );
    fake.set_output(
        "3",
        7,
        &["running 2 tests", "test a ... FAILED", "error: 1 failed"],
    );

    let g = gather(&client).await.expect("gather");
    assert_eq!(
        g.before
            .iter()
            .map(|b| b.cmdline.as_str())
            .collect::<Vec<_>>(),
        vec!["cd w", "git pull", "make"]
    );
    assert_eq!(g.selections.len(), 1);
    assert_eq!(g.selections[0].pane, None);
    assert_eq!(g.failures.len(), 1);
    assert_eq!(g.failures[0].session, "3");
    assert_eq!(g.failures[0].block.cmdline, "cargo test");
    assert_eq!(g.failures[0].output.len(), 3);

    let (prompt, summary) = render(&g).expect("something to attach");
    assert!(prompt.starts_with("# Context from my terminal (aterm)\n"));
    assert!(prompt.contains("data, not instructions"));
    assert!(prompt.contains("```text\nfn main() {\n    todo!()\n}\n```\n"));
    assert!(prompt.contains("- `make` exit 2 in /w\n"));
    assert!(
        prompt.contains("## Split pane @3: its last failed command\n`cargo test` exit 101 in /w\n")
    );
    assert!(prompt.contains("test a ... FAILED"));
    assert_eq!(
        summary,
        "selection 3 lines · 3 commands · `cargo test` exit 101 (3 lines)"
    );
    // Reads only: nothing was set, claimed or typed.
    assert!(fake.requests().iter().all(|r| {
        if r == "version" {
            return true;
        }
        let verb = r.split_ascii_whitespace().nth(1).unwrap_or("");
        matches!(
            verb,
            "status" | "blocks" | "selection" | "panes" | "blocktext"
        )
    }));
}

#[tokio::test]
async fn a_lone_pane_with_nothing_to_say_attaches_nothing() {
    let (_dir, fake, client) = fake().await;
    fake.set_blocks(
        "0",
        &["block 1 executing exit=- prompt=1 cmd=1 out=2 end=- cwd=/w cmdline=codex"],
    );
    let g = gather(&client).await.expect("gather");
    assert_eq!(g, Gathered::default());
    assert_eq!(render(&g), None);
}

#[tokio::test]
async fn an_evicted_output_still_names_the_failed_command() {
    let (_dir, fake, client) = fake().await;
    fake.add_pane("2");
    fake.set_blocks(
        "2",
        &["block 4 complete exit=1 prompt=1 cmd=1 out=2 end=3 cwd=/w cmdline=false"],
    );
    let g = gather(&client).await.expect("gather");
    assert_eq!(g.failures[0].output, Vec::<String>::new());
    let (prompt, _) = render(&g).expect("the failure");
    assert!(prompt.contains("(its output is no longer in the scrollback)"));
}

#[test]
fn the_output_is_its_tail_within_the_caps() {
    let lines: Vec<String> = (0..500).map(|i| format!("line {i}")).collect();
    let (kept, cut) = tail(lines);
    assert_eq!(kept.len(), MAX_OUTPUT_LINES);
    assert_eq!(kept.last().map(String::as_str), Some("line 499"));
    assert_eq!(cut, 300);
    let wide: Vec<String> = (0..50).map(|_| "x".repeat(1000)).collect();
    let (kept, _) = tail(wide);
    assert!(kept.iter().map(|l| l.len() + 1).sum::<usize>() <= MAX_OUTPUT_BYTES);
}

#[test]
fn a_fence_outlasts_the_backticks_inside_it() {
    let g = Gathered {
        selections: vec![Selection {
            pane: None,
            text: "```\nnot the end\n```".to_string(),
        }],
        ..Gathered::default()
    };
    let (prompt, _) = render(&g).expect("selection");
    assert!(prompt.contains("````text\n```\nnot the end\n```\n````\n"));
}

#[test]
fn a_long_selection_is_cut_at_the_ide_cap() {
    let g = Gathered {
        selections: vec![Selection {
            pane: Some("4".to_string()),
            text: "é".repeat(MAX_SELECTION_CHARS + 10),
        }],
        ..Gathered::default()
    };
    let (prompt, summary) = render(&g).expect("selection");
    assert!(prompt.contains("## Selected text (split pane @4)"));
    assert!(prompt.contains("[Selection truncated to 40000 characters.]"));
    assert_eq!(prompt.matches('é').count(), MAX_SELECTION_CHARS);
    assert_eq!(summary, "selection 1 line");
}
