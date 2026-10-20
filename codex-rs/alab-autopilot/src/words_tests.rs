// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The ported readers answer as aterm's own tests say they do (cases copied from
//! `turn_end_tests.rs` at aterm `3f0b07191`).

use crate::words::FINISHED;
use crate::words::Said;
use crate::words::classify_said;
use crate::words::destructive_word;
use crate::words::says_done;
use crate::words::says_finished;
use pretty_assertions::assert_eq;

#[test]
fn finished_is_only_the_token_on_the_last_line() {
    for yes in [
        "ATERM-TASK-DONE",
        "All landed.\n`ATERM-TASK-DONE`",
        "All landed.\n**ATERM-TASK-DONE**\n",
        "All landed.\n  ATERM-TASK-DONE.  ",
    ] {
        assert!(says_finished(Some(yes)), "{yes:?}");
        assert!(says_done(Some(yes)), "{yes:?}");
    }
    for no in [
        "I will write ATERM-TASK-DONE when the task is finished.",
        "ATERM-TASK-DONE\nbut one test still fails, fixing it now",
        "ATERM-TASK-DONE-ISH",
        "Done.",
        "",
    ] {
        assert!(!says_finished(Some(no)), "{no:?}");
    }
    assert!(!says_finished(None));
    assert_eq!(FINISHED, "ATERM-TASK-DONE");
}

#[test]
fn whats_next_after_finished_work_is_a_done_report() {
    for said in [
        "Done. I've created hello.txt with the greeting. What's next?",
        "The migration is in and the suite is green. Anything else?",
        "What would you like me to work on?",
    ] {
        assert!(says_done(Some(said)), "{said}");
    }
    assert!(!says_done(Some("Should I also update the docs?")));
    assert!(!says_done(Some(
        "Next I will check what's next in the plan."
    )));
    assert!(!says_done(Some("Stage 3 is next.")));
}

#[test]
fn questions_offers_and_stops_read_as_aterm_reads_them() {
    assert_eq!(
        classify_said(Some(
            "Should I rename the module?\n(I have not touched it yet.)"
        )),
        Said::Question
    );
    assert_eq!(
        classify_said(Some("Proceed with stage 2 (y/n)")),
        Said::Question
    );
    let said = "Two ways forward:\n1. rewrite the parser\n2. patch the lexer\nI recommend 1. \
                Which one do you want?";
    assert_eq!(classify_said(Some(said)), Said::Question);
    let said = "Two ways forward:\n1. rewrite the parser\n2. patch the lexer\nI recommend 1; \
                shall I start?";
    assert_eq!(classify_said(Some(said)), Said::Offer);
    assert!(matches!(
        classify_said(Some("I need your decision on the schema.")),
        Said::Stop(_)
    ));
    assert_eq!(
        classify_said(Some("Fixed the bug; tests pass.")),
        Said::Plain
    );
    assert_eq!(classify_said(None), Said::Plain);
}

#[test]
fn a_named_irreversible_act_is_never_a_plain_offer() {
    assert!(matches!(
        classify_said(Some("Want me to force-push the branch?")),
        Said::Irreversible(_)
    ));
    assert!(matches!(
        classify_said(Some("Should I delete the old tables?")),
        Said::Irreversible(_)
    ));
    // Negative control: `push it when green` is a continuation, not a destructive act.
    assert_eq!(destructive_word("push it when green"), None);
    assert_eq!(destructive_word("i deleted it"), Some("delete"));
}
