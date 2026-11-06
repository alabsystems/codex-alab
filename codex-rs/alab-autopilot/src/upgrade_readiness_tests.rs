// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! aterm's own cases for its reading of an answer to an upgrade notice
//! (`harness::upgrade_readiness`'s tests, aterm main `433715d26d`), byte for byte, and its
//! marker line (`upgrade::says_ready`), and Codex where aterm's reader names Claude Code.

use crate::upgrade_readiness::says_ready;
use crate::upgrade_readiness::says_ready_to;

const MARKER: &str = "ATERM-UPGRADE-READY-01234567";

#[test]
fn ordinary_answers_need_no_prescribed_output() {
    for text in [
        "Ready for the restart.",
        "Everything is saved. I'm ready to restart now.",
        "Nothing of mine is running. You can restart this session now.",
        "No background jobs are running; this session can safely be restarted.",
        "It’s safe to restart the session.",
        "**I'm ready for the upgrade.**",
        "All work is saved, and I am ready for this restart.",
        "Saved `parser.rs`. I'm ready to restart now.",
        "```text\nExample: not ready for restart.\n```\nI'm ready to restart now.",
    ] {
        assert!(says_ready(text), "{text}");
        assert!(says_ready_to(text, MARKER), "{text}");
    }
}

#[test]
fn a_caveat_or_example_is_not_readiness() {
    for text in [
        "I'm not ready for the restart.",
        "I'll be ready once the build finishes.",
        "Ready for the restart?",
        "Ready for the restart, but the build is still running.",
        "Ready for the restart. The build is pending.",
        "Ready for the restart. The tests are still in progress.",
        "Ready for the restart. The tests are in progress.",
        "Ready for the restart. I still need to save my work.",
        "Ready for the restart. I need to save my work.",
        "Ready for the restart. I have unsaved changes.",
        "I'm ready to restart. I haven't saved my work.",
        "Ready for the restart. The result hasn't been saved.",
        "Ready for the restart. The work is not done.",
        "Ready for the restart. Not all changes are saved.",
        "Ready for the restart. Nothing is running except the release watcher.",
        "Ready for the restart. No tests are running other than the release watcher.",
        "Ready for the restart. The build is not running and the deploy is pending.",
        "Ready for the restart. Wait for the upload to finish.",
        "Ready for the restart. Hold off for a moment.",
        "Ready for the restart. The upload isn't complete.",
        "Ready for the restart. Do not restart yet.",
        "If everything is saved, you can restart this session.",
        "Once I say ready, you can restart this session.",
        "> Ready for the restart.",
        "```text\nReady for the restart.\n```",
        "Example: `I'm ready to restart now.`",
        "Example:\n    Ready for the restart.",
        "\tReady for the restart.",
        "The agent said: \"Ready for the restart.\"",
        "All tests passed.",
        "````text\nAn example follows:\n```\nReady for the restart.\n````",
        "```text\n~~~\nReady for the restart.\n```",
        "`Example response:\nReady for the restart.\n`",
        "``Example with a ` inside:\nReady for the restart.\n``",
        "I'm `not` ready for the restart.",
        "Ready for the restart. I'm `not` ready for the restart.",
        "Ready for the restart. `An unfinished example",
        "Ready for the restart.\n````text\nAn unfinished example",
    ] {
        assert!(!says_ready(text), "{text}");
        assert!(!says_ready_to(text, MARKER), "{text}");
    }
}

/// aterm's legacy marker reply: the notice's reference alone on a line answers it ready,
/// fenced in backticks too. NEGATIVE CONTROLS: another notice's reference, the reference inside
/// other words, and no reference at all (an empty one never matches).
#[test]
fn the_reference_alone_on_a_line_is_readiness() {
    assert!(says_ready_to(MARKER, MARKER));
    assert!(says_ready_to(
        &format!("Saved my work.\n`{MARKER}`"),
        MARKER
    ));
    assert!(!says_ready_to("ATERM-UPGRADE-READY-76543210", MARKER));
    assert!(!says_ready_to(
        &format!("Not yet: {MARKER} after the build."),
        MARKER
    ));
    assert!(!says_ready_to("Ready for the restart.", ""));
}

/// THE PROGRAM CODEX-ALAB'S NOTICE NAMES (review of gap 27, finding 13): the notice says aterm
/// "will restart this Codex", and an answer that names Codex back is readiness, as one naming
/// Claude Code is under aterm's. NEGATIVE CONTROLS: another program, and a caveat beside Codex.
#[test]
fn an_answer_naming_codex_is_readiness() {
    for text in [
        "You can restart Codex now.",
        "It's safe to restart Codex.",
        "It is safe to restart codex now.",
    ] {
        assert!(says_ready(text), "{text}");
    }
    for text in [
        "You can restart Vim now.",
        "You can restart Codex once the build finishes.",
    ] {
        assert!(!says_ready(text), "{text}");
    }
}
