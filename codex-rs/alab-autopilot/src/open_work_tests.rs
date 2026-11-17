// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The open-work readers, ported with their module from aterm `d3e1dd842b`
//! (`supervise/policy/turn_end_tests.rs`: the `open_work` tests and the labelled corpus), plus
//! Land's strict reading and the suffix. Land's defense table and its mutation check are the
//! module's own child (`open_work_land_tests.rs`), as aterm's are `turn_end`'s.

use crate::open_work::*;
use crate::words::FINISHED;

/// The report that ended with the token while its own words listed work
/// left (the owner, 2026-10-16: "stop telling me done if you're not done").
const OPEN_REPORT: &str = "Built and tested the parser; committed on main.\n\
                           Still open: the lexer's e2e suite (not run).\n\
                           ATERM-TASK-DONE";

/// WHAT A FINISHED REPORT LEAVES OPEN, in its own words ([`open_work`]): a
/// head (`Still open:`, `Left:`, `Next steps`, …) with an item that is not
/// negated — one that names its blocker or a decision included (the
/// owner's rule of 2026-10-19: a blocked follow-up is open work) — or a
/// status word (`not run`, `skipped`) that stands undescribed. NEGATIVE
/// CONTROLS: a negation (`Nothing left`, `Left: none`, `0 skipped`), a
/// description (`do not run it twice`, `is skipped`), code in backticks,
/// and a word that only begins like a head (`Left the config`).
#[test]
fn open_work_reads_heads_and_status_words_and_never_a_negation() {
    for (tail, want) in [
        (
            "Committed the parser. Left: the lexer.\nATERM-TASK-DONE",
            &["left"][..],
        ),
        (OPEN_REPORT, &["still open"]),
        (
            "Done:\n- the parser\nStill open:\n- the lexer\n- the docs\nATERM-TASK-DONE",
            &["still open"],
        ),
        (
            "**Remaining work:** the lexer.\nATERM-TASK-DONE",
            &["remaining"],
        ),
        (
            "## Next steps\n1. Port the reader.\nATERM-TASK-DONE",
            &["next steps"],
        ),
        (
            "Follow-ups:\n- delete the old table\nATERM-TASK-DONE",
            &["follow-ups"],
        ),
        ("TODO: wire the flag.\nATERM-TASK-DONE", &["todo"]),
        (
            "Landed the parser; the e2e suite was not run.\nATERM-TASK-DONE",
            &["not run"],
        ),
        (
            "Tests: 12 passed, 2 skipped.\nATERM-TASK-DONE",
            &["skipped"],
        ),
        (
            "Left: nothing. The e2e suite was not run.\nATERM-TASK-DONE",
            &["not run"],
        ),
        // A row Claude Code wrapped carries the line on.
        (
            "Still open: the lexer's e2e suite, which needs a\nrebuild of its fixtures.\nATERM-TASK-DONE",
            &["still open"],
        ),
        (
            "Left: the lexer.\nThe docs were skipped.\nATERM-TASK-DONE",
            &["left", "skipped"],
        ),
        // Blocked, or decided, it is open work all the same.
        (
            "Still open (needs the owner's credential): the release.\nATERM-TASK-DONE",
            &["still open"],
        ),
        (
            "Still open:\n- the release: needs the owner's release credential\n\
             - the GPU bench: no GPU on this host\nATERM-TASK-DONE",
            &["still open"],
        ),
        (
            "Left: the release, blocked on the owner's credential.\nATERM-TASK-DONE",
            &["left"],
        ),
        (
            "Not done (decision): kept the cache at 64 MB; 128 MB costs 40 ms more.\nATERM-TASK-DONE",
            &["not done"],
        ),
    ] {
        assert_eq!(open_work(Some(tail)), want, "{tail:?}");
    }
    for tail in [
        "Nothing left.\nATERM-TASK-DONE",
        "No open items.\nATERM-TASK-DONE",
        "Left: none.\nATERM-TASK-DONE",
        "Still open: nothing.\nATERM-TASK-DONE",
        "Remaining: n/a\nATERM-TASK-DONE",
        "Next steps: none from me.\nATERM-TASK-DONE",
        "Tests: 12 passed, 0 skipped.\nATERM-TASK-DONE",
        "Nothing was skipped.\nATERM-TASK-DONE",
        "Do not run the migration twice.\nATERM-TASK-DONE",
        "A warm cache is skipped, which saves 3 ms.\nATERM-TASK-DONE",
        "Ran `targo --unverified test -- --skipped`; all green.\nATERM-TASK-DONE",
        "Removed the skipped-test registry.\nATERM-TASK-DONE",
        "Left the config as it was; the remaining tests pass.\nATERM-TASK-DONE",
        "Opened the PR and left a note.\nATERM-TASK-DONE",
        "Landed on main; every gate green.\nATERM-TASK-DONE",
        "Hi! What would you like me to work on?\nATERM-TASK-DONE",
        "ATERM-TASK-DONE",
    ] {
        assert!(open_work(Some(tail)).is_empty(), "{tail:?}");
    }
    assert!(open_work(None).is_empty());
}

/// A historical skipped state is no current open work once that same probe
/// explicitly runs and passes. Land's existing historical reading is a native
/// positive control; the new behavior under test belongs only to Cruise.
#[test]
fn open_work_settles_explicitly_recovered_historical_skips() {
    let mut open = Vec::new();
    let mut land_open = Vec::new();
    for marker in [
        "previously",
        "formerly",
        "once",
        "earlier",
        "initially",
        "originally",
    ] {
        for (subject, predicates) in [
            ("probe", ["now passes", "now runs and passes"]),
            ("test", ["now passes", "now runs and passes"]),
            ("check", ["now passes", "now runs and passes"]),
            ("probes", ["now pass", "now run and pass"]),
            ("tests", ["now pass", "now run and pass"]),
            ("checks", ["now pass", "now run and pass"]),
        ] {
            for predicate in predicates {
                let report = format!("Nothing left. The {marker} skipped {subject} {predicate}.");
                let tail = format!("{report}\n{FINISHED}");
                let land = open_work_strict(Some(&tail));
                let cruise = open_work(Some(&tail));
                if !land.is_empty() {
                    land_open.push((report.clone(), land));
                }
                if !cruise.is_empty() {
                    open.push((report, cruise));
                }
            }
        }
    }
    assert!(
        land_open.is_empty(),
        "existing Land reading changed: {land_open:?}"
    );
    assert!(
        open.is_empty(),
        "completed historical probes read open: {open:?}"
    );
}

/// An old skip alone, a partial subject, a different subject's success, or a
/// conditional passing claim must not erase current work. Nonzero generic
/// skip tallies retain their existing conservative meaning.
#[test]
fn open_work_keeps_unsettled_historical_skips_open() {
    for report in [
        "The previously skipped probe still fails.",
        "The formerly skipped probe needs a re-run.",
        "The initially skipped probe will run after approval.",
        "The previously skipped probe would pass.",
        "The previously skipped probe now runs.",
        "The previously skipped probe now passes if CI passes.",
        "If CI passes, the previously skipped probe now passes.",
        "If CI passed and the previously skipped probe now runs and passes.",
        "The previously skipped probe now passes?",
        "Some previously skipped probes now pass.",
        "Two of three previously skipped probes now pass.",
        "Only one previously skipped probe now passes.",
        "The documentation for the previously skipped probe now passes.",
        "The audit of the previously skipped probe now passes.",
        "The previously skipped probe now passes, except for the final check.",
        "The previously skipped probe now passes, pending approval.",
        "Tests: 12 passed, 2 skipped, no failures.",
    ] {
        let tail = format!("{report}\n{FINISHED}");
        assert!(!open_work(Some(&tail)).is_empty(), "{report:?}");
    }
}

/// A recovered historical probe settles no independent check or reported work.
#[test]
fn open_work_retains_independent_work_after_recovered_historical_skips() {
    for (report, expected) in [
        (
            "The previously skipped probe now runs and passes. The audit was not verified.",
            &["not verified"][..],
        ),
        (
            "The formerly skipped probe now passes; still open: finish the independent review.",
            &["still open"],
        ),
        (
            "The initially skipped probe now passes. Another probe was skipped.",
            &["skipped"],
        ),
    ] {
        let tail = format!("{report}\n{FINISHED}");
        assert_eq!(open_work(Some(&tail)), expected, "{report:?}");
    }
}

/// Current pending work remains open independently of a completed historical
/// probe. Collect every result before asserting, retaining actual Land data in
/// a failing diagnostic without prescribing a new Land reading.
#[test]
fn open_work_keeps_explicit_current_pending_work_open() {
    let mut mismatches = Vec::new();
    for (report, expected) in [
        ("The audit remains pending.", &["remains pending"][..]),
        (
            "The previously skipped probe now passes. The audit remains pending.",
            &["remains pending"],
        ),
        (
            "The review remains pending until approval.",
            &["remains pending"],
        ),
        ("The audit remains open.", &["remains open"]),
        (
            "The audit remains pending and is not verified.",
            &["not verified"],
        ),
    ] {
        let tail = format!("{report}\n{FINISHED}");
        let land = open_work_strict(Some(&tail));
        let found = open_work(Some(&tail));
        if found != expected {
            mismatches.push((report, expected, found, land));
        }
    }
    assert!(
        mismatches.is_empty(),
        "current pending work was lost (report, expected, Cruise, Land): {mismatches:?}"
    );
}

/// Reuse the current status reader's negation, quotation and no-work reason
/// guards; a formerly pending state alone is not current pending work.
#[test]
fn open_work_keeps_negated_quoted_and_past_pending_work_clear() {
    let mut unexpected = Vec::new();
    for report in [
        "No audit remains pending.",
        "The phrase \"remains pending\" is documentation.",
        "The phrase “remains pending” is documentation.",
        "The audit previously remained pending and is now complete.",
        "The audit remains pending, because no contract was added.",
        "The previously skipped probe now passes. No audit remains pending.",
    ] {
        let tail = format!("{report}\n{FINISHED}");
        let land = open_work_strict(Some(&tail));
        let found = open_work(Some(&tail));
        if !found.is_empty() {
            unexpected.push((report, found, land));
        }
    }
    assert!(
        unexpected.is_empty(),
        "noncurrent pending wording read open (report, Cruise, Land): {unexpected:?}"
    );
}

#[test]
fn open_work_accepts_the_completed_follow_ups_report() {
    for report in [
        "Available follow-ups were already complete.",
        "Nothing left: available follow-ups were already complete; unavailable native work remains \
         deferred. Fixed workflow false-success exits and landed at `610696ddc1`.",
        "Completed follow-ups.",
    ] {
        let tail = format!("{report}\n{FINISHED}");
        assert!(open_work(Some(&tail)).is_empty(), "{report:?}");
    }
}

#[test]
fn open_work_settles_follow_ups_only_when_their_own_clause_is_complete() {
    for report in [
        "I completed the follow-up.",
        "I completed the follow-ups.",
        "We completed all follow-ups.",
        "The follow-up is complete.",
        "The follow-ups are complete.",
        "The follow-up was completed.",
        "The follow-ups were completed.",
        "The follow-up has been completed.",
        "The follow-ups have been completed.",
        "The follow-ups are now done.",
        "The follow-up is finished.",
        "We addressed the follow-up.",
        "The follow-ups are resolved.",
        "Both follow-ups are complete.",
        "The follow-up was completed after CI passed.",
    ] {
        let tail = format!("{report}\n{FINISHED}");
        assert!(open_work(Some(&tail)).is_empty(), "{report:?}");
    }
}

#[test]
fn open_work_keeps_incomplete_and_conditional_follow_ups_open() {
    for report in [
        "The follow-up is pending.",
        "The follow-ups remain pending.",
        "The follow-up is deferred.",
        "The follow-ups are deferred.",
        "The follow-up is partially complete.",
        "The follow-ups are partly completed.",
        "The follow-up is not yet complete.",
        "The follow-ups have not yet been completed.",
        "Only one of three follow-ups is completed.",
        "Some follow-ups are complete.",
        "Several follow-ups are complete.",
        "Many follow-ups are complete.",
        "Two of three follow-ups are completed.",
        "Both of the three follow-ups are complete.",
        "The follow-ups are complete except docs.",
        "The follow-ups are complete, except docs.",
        "The follow-ups are green.",
        "The follow-up passes.",
        "The follow-ups will be completed tomorrow.",
        "The follow-up is complete if CI passes.",
        "The follow-up is complete after CI passes.",
        "The follow-ups are complete pending approval.",
        "The follow-ups are done once approval arrives.",
        "We plan to complete the follow-ups.",
        "The follow-up can be completed after review.",
        "The follow-ups are complete?",
        "Completed follow-ups?",
        "If CI passed and I completed the follow-up.",
        "The audit then completed the follow-up.",
        "I will verify CI and completed the follow-up.",
    ] {
        let tail = format!("{report}\n{FINISHED}");
        assert!(!open_work(Some(&tail)).is_empty(), "{report:?}");
    }
}

#[test]
fn open_work_does_not_settle_follow_ups_with_another_clause_or_subject() {
    for report in [
        "The measurement is completed; the follow-ups are pending.",
        "The follow-up is pending; the measurement is completed.",
        "The follow-ups are deferred, but the measurement is complete.",
        "The follow-up is pending and the measurement is completed.",
        "The follow-up remains pending. The audit is complete.",
        "The report is complete. The follow-ups are still pending.",
        "The follow-ups documentation is complete.",
        "The audit of the follow-ups is complete.",
    ] {
        let tail = format!("{report}\n{FINISHED}");
        assert!(!open_work(Some(&tail)).is_empty(), "{report:?}");
    }
}

#[test]
fn open_work_retains_genuine_work_after_completed_follow_ups() {
    for (report, expected) in [
        (
            "I completed the follow-up. The tests were not run.",
            &["not run"][..],
        ),
        (
            "The follow-ups are complete; the audit was not verified.",
            &["not verified"],
        ),
        (
            "We completed the follow-up. Still open: finish the independent review.",
            &["still open"],
        ),
        (
            "I completed the follow-up; another follow-up is pending.",
            &["follow-up"],
        ),
        (
            "The follow-ups are completed. Remaining work: finish the migration.",
            &["remaining"],
        ),
    ] {
        let tail = format!("{report}\n{FINISHED}");
        assert_eq!(open_work(Some(&tail)), expected, "{report:?}");
    }
}

#[test]
fn open_work_keeps_fenced_examples_out_of_the_report() {
    for tail in [
        "A sample report:\n```text\nStill open: finish the tests.\n```\nATERM-TASK-DONE",
        "A sample report:\n````text\n```\nTests were not run.\n```\n````\nATERM-TASK-DONE",
        "A sample report:\n~~~text\nStill open: finish the tests.\n~~~\nATERM-TASK-DONE",
        "A sample report:\n```text\nStill open: finish the tests.\nATERM-TASK-DONE",
        "Still open:\n```text\nAn example, not a reported item.\n```\nAll checks passed.\nATERM-TASK-DONE",
    ] {
        assert!(open_work(Some(tail)).is_empty(), "{tail:?}");
    }
    for fence in ["```", "~~~~"] {
        let tail = format!(
            "A sample report:\n{fence}text\nLeft: the example only.\n{fence}\n\
             Still open: finish the actual tests.\nATERM-TASK-DONE"
        );
        assert_eq!(open_work(Some(&tail)), &["still open"], "{tail:?}");
    }
    assert_eq!(
        open_work(Some(
            "All work completed\n```text\nAn example.\n```\nstill open: the actual tests.\nATERM-TASK-DONE"
        )),
        &["still open"]
    );

    // The done reader and the open-work reader agree on every block's
    // actual close. A complete done phrase inside code is no own claim.
    for (open, inner, close) in [
        ("~~~~markdown", "```", "~~~~"),
        ("````markdown", "~~~", "````"),
        ("````markdown", "```", "````"),
        ("~~~~markdown", "~~~", "~~~~"),
        ("```markdown", "``` not a close", "```"),
        ("~~~markdown", "~~~ not a close", "~~~"),
        ("```markdown", "~~~", "````"),
    ] {
        let tail = format!("{open}\n{inner}\nAll done.\n{close}");
        assert!(!crate::words::says_done(Some(&tail)), "{tail:?}");
        assert!(open_work(Some(&tail)).is_empty(), "{tail:?}");
        assert!(crate::words::says_done(Some(&format!("{tail}\nAll done."))));
        assert_eq!(
            open_work(Some(&format!(
                "{tail}\nStill open: finish the actual tests."
            ))),
            &["still open"]
        );
    }
}

#[test]
fn open_work_reads_balanced_underscore_emphasis_without_splitting_identifiers() {
    for (tail, want) in [
        (
            "__Still open__: finish the tests.\nATERM-TASK-DONE",
            "still open",
        ),
        (
            "_Remaining work_: finish the tests.\nATERM-TASK-DONE",
            "remaining",
        ),
        ("The tests were _not run_.\nATERM-TASK-DONE", "not run"),
        ("The checks were __skipped__.\nATERM-TASK-DONE", "skipped"),
    ] {
        assert_eq!(open_work(Some(tail)), &[want], "{tail:?}");
    }
    for tail in [
        "Removed the skipped_test registry.\nATERM-TASK-DONE",
        "Removed the _skipped_test_ registry.\nATERM-TASK-DONE",
        "__Still open: an unmatched example marker.\nATERM-TASK-DONE",
        "Ran `_not run_` as the example string.\nATERM-TASK-DONE",
    ] {
        assert!(open_work(Some(tail)).is_empty(), "{tail:?}");
    }
}

/// A BLOCKED ITEM AND THE SENTENCES AFTER IT ([`open_work`]): the item that
/// names its blocker is open (the owner's rule of 2026-10-19: a blocked
/// follow-up is open work), and each later sentence is read on its own.
/// NEGATIVE CONTROL: a negated later sentence adds nothing.
#[test]
fn open_work_reads_a_blocked_item_and_the_sentences_after_it() {
    for (tail, want) in [
        (
            "Still open: the release needs a credential. Tests were not run.\nATERM-TASK-DONE",
            &["still open", "not run"][..],
        ),
        (
            "Left (blocked on the owner): the release. The tests were not run.\nATERM-TASK-DONE",
            &["left", "not run"],
        ),
        (
            "Still open: the release needs a credential; the tests were not run.\nATERM-TASK-DONE",
            &["still open", "not run"],
        ),
        (
            "Still open: the release tests were not run without the owner's credential.\nATERM-TASK-DONE",
            &["still open"],
        ),
        (
            "Left (blocked on the owner): the release. No tests were skipped.\nATERM-TASK-DONE",
            &["left"],
        ),
        (
            "Left: fix the tests. Tests were skipped.\nATERM-TASK-DONE",
            &["left", "skipped"],
        ),
    ] {
        assert_eq!(open_work(Some(tail)), want, "{tail:?}");
    }
}

/// OPEN WORK SAID MID-SENTENCE, AND A BLOCKER NAMED AFTER IT ([`open_work`]):
/// `The lexer is still open.`, `I did not find the cause.`, `didn't fix`,
/// `not yet run`, a head with a tail before its colon (`Still open from
/// AU-1:`), a Markdown heading that opens with a head (`## Not measured, and
/// why`). WHATEVER BLOCKS IT: a blocker named after the item (`Not compiled.
/// Blocker: …`), in a sub-item, a stated decision, another track's step, a
/// reason that says only the work takes long or a negated one that names a
/// lack (`no mount is available`, `could not run on this host`) leaves it
/// open (the owner's rule of 2026-10-19). A reason the words around it give that nothing needs doing
/// only some readers can tell (`aterm-gui is not modified`, `the brief forbids
/// it`): it reads open, the safe side, and the worker's next report, naming
/// nothing left, clears it. NEGATIVE CONTROLS: a phrase in quotation marks, a
/// possessive (`this host's load`), a reason that says there was nothing to
/// do (`not run, because no contract was added`, `not done, because main
/// already has it`).
#[test]
fn open_work_reads_work_left_mid_sentence_and_a_blocker_named_after_it() {
    for (tail, want) in [
        ("The lexer is still open.", &["still open"][..]),
        ("I did not find the cause.", &["did not find"]),
        (
            "One spike at load 18: observed, not explained.",
            &["not explained"],
        ),
        (
            "I traced it but didn't fix it, because a fix is a code change.",
            &["did not fix"],
        ),
        ("Both helpers are not yet run at once.", &["not run"]),
        (
            "Still open from AU-1:\n- the boot apply's second half",
            &["still open"],
        ),
        (
            "Open items, all out of scope:\n- the held pane's check",
            &["open items"],
        ),
        (
            "Follow-ups for the owner: bound the vendor DNS time.",
            &["follow-ups"],
        ),
        (
            "## Not measured, and why\n- **The index hint:** not passed in the replay.",
            &["not measured"],
        ),
        (
            "Left open, each with its blocker:\n- the primer: the window rewrites it at its next spawn",
            &["left open"],
        ),
        (
            "Another chain's trust run needs a re-run.",
            &["needs a re-run"],
        ),
        (
            "At this host's load the spike was not explained.",
            &["not explained"],
        ),
        (
            "Tests were not run; the release needs a credential.",
            &["not run"],
        ),
        (
            "Not run: the e2e suite. The docs built. Lint passed. The release needs the owner's credential.",
            &["not run"],
        ),
        ("Tests: 12 passed, 2 skipped, no failures.", &["skipped"]),
        ("The memory leak is still open.", &["still open"]),
        (
            "Still open: the verifier rejected the bundle's signature.",
            &["still open"],
        ),
        ("Left: the reviewer's race finding in spawn.rs.", &["left"]),
        (
            "- The write was not allowed (EPERM) in the sandbox; not fixed.",
            &["not fixed"],
        ),
        (
            "- The release-mode tests: not run, because there was no time.",
            &["not run"],
        ),
    ] {
        assert_eq!(
            open_work(Some(&format!("{tail}\n{FINISHED}"))),
            want,
            "{tail:?}"
        );
    }
    // Blocked, decided or another's step: open all the same.
    for (tail, want) in [
        (
            "- Windows build: not compiled. Blocker: no Windows std on this host.",
            &["not compiled"][..],
        ),
        (
            "- **Windowed checks:** not run.\n  - Blocker: no windows on the owner's desktop.",
            &["not run"],
        ),
        (
            "- Moving the checks: not done, per the reviewer's correction.",
            &["not done"],
        ),
        (
            "- au-4: not done; its review verdict rejected it.",
            &["not done"],
        ),
        (
            "- Segmented downloads: not done on purpose; 8 streams measured slower.",
            &["not done"],
        ),
        (
            "- Trust for atpkg: skipped for memory, as round 1 measured.",
            &["skipped"],
        ),
        (
            "- GUI side: not done here; that is TC-3 per the plan.",
            &["not done"],
        ),
        // A negated blocker is a blocker: it names what is lacking.
        (
            "- The stalled-mount case: not reproduced: no SMB mount is available.",
            &["not reproduced"],
        ),
        (
            "- Windowed Cmd-T was not measured (no windows allowed).",
            &["not measured"],
        ),
        (
            "- The trust floor check could not run on this host.",
            &["not run"],
        ),
        // The safe side: a reason only the words around it give.
        (
            "- aterm-gui's tests were not run. aterm-gui is not modified.",
            &["not run"],
        ),
        (
            "- I did not run `aterm agents install`: the brief forbids touching ~/.claude.",
            &["not run"],
        ),
    ] {
        assert_eq!(
            open_work(Some(&format!("{tail}\n{FINISHED}"))),
            want,
            "{tail:?}"
        );
    }
    for tail in [
        "- contract-probe: not run, because no requires/ensures contract was added.",
        "- contract-probe was not run because the diff adds no contract.",
        "- contract-probe: not run. No contract was added.",
        "- Headless QoS: not done, because main already has it.",
        "A Window place skips the \"window still open\" check.",
    ] {
        assert!(
            open_work(Some(&format!("{tail}\n{FINISHED}"))).is_empty(),
            "{tail:?}"
        );
    }
}

/// THREE ROUNDS OF REAL DONE REPORTS (`fixtures/open-work-corpus.txt`): the
/// reports this repository's own agents ended with ATERM-TASK-DONE, trimmed
/// to excerpts each labelled by the owner's rule, open or clear. Every
/// excerpt, and every report its excerpts make up, reads as its label says,
/// or as its mark says where the reading is known to miss or to flag, so a
/// change that mends one shows here as plainly as one that breaks one. The
/// round-1 and round-2 reports that ended with the token while listing open
/// work must be found, and so must the round-3 integration report, whose
/// every leftover names its blocker: a blocked leftover is open work (the
/// owner's rule of 2026-10-19). NEGATIVE CONTROLS: the clear excerpts,
/// which leave nothing to do.
#[test]
fn open_work_reads_three_rounds_of_real_reports_as_labelled() {
    let corpus = include_str!("fixtures/open-work-corpus.txt");
    let mut reports: Vec<(&str, Vec<(bool, String)>)> = Vec::new();
    let mut excerpt: Option<(bool, String)> = None;
    for line in corpus.lines().skip_while(|l| !l.starts_with("=== ")) {
        if (line.starts_with("=== ") || line.starts_with("--- "))
            && let (Some(done), Some((_, excerpts))) = (excerpt.take(), reports.last_mut())
        {
            excerpts.push(done);
        }
        if let Some(name) = line.strip_prefix("=== ") {
            reports.push((name, Vec::new()));
        } else if let Some(mark) = line.strip_prefix("--- ") {
            let (label, known) = mark.split_once(", ").unwrap_or((mark, ""));
            let open = match label {
                "open" => true,
                "clear" => false,
                _ => panic!("no label: {mark:?}"),
            };
            let reads_open = match known.split_once(':').map(|(k, _)| k) {
                None => open,
                Some("missed") if open => false,
                Some("flagged") if !open => true,
                _ => panic!("a mark that contradicts its label: {mark:?}"),
            };
            excerpt = Some((reads_open, String::new()));
        } else if let Some((_, text)) = excerpt.as_mut() {
            text.push_str(line);
            text.push('\n');
        }
    }
    if let (Some(done), Some((_, excerpts))) = (excerpt, reports.last_mut()) {
        excerpts.push(done);
    }
    let reads = |text: &str| !open_work(Some(&format!("{text}{FINISHED}"))).is_empty();
    // Every excerpt that reads otherwise than its label, named at once.
    let mut wrong: Vec<String> = Vec::new();
    for (name, excerpts) in &reports {
        for (reads_open, text) in excerpts {
            if reads(text) != *reads_open {
                wrong.push(format!(
                    "{name}: reads {}, labelled {}:\n{text}",
                    reads(text),
                    reads_open
                ));
            }
        }
        let joined: String = excerpts
            .iter()
            .map(|(_, text)| format!("{text}\n"))
            .collect();
        let any = excerpts.iter().any(|(reads_open, _)| *reads_open);
        if reads(&joined) != any {
            wrong.push(format!(
                "{name}: the joined report reads {}",
                reads(&joined)
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n----\n"));
    let report = |name: &str| {
        let (_, excerpts) = reports.iter().find(|(n, _)| *n == name).expect(name);
        let joined: String = excerpts
            .iter()
            .map(|(_, text)| format!("{text}\n"))
            .collect();
        reads(&joined)
    };
    for name in [
        "round 1, AU-1 review",
        "round 1, measurement report",
        "round 2, measurement report",
        "round 2, TAB-1 implementer",
    ] {
        assert!(report(name), "{name}: ended with the token over open work");
    }
    assert!(
        report("round 3, integration notes"),
        "its leftovers are blocked, and open"
    );
    assert!(reports.len() > 60, "{} reports", reports.len());
}

/// LAND'S STRICT READING ([`open_work_strict`]): work not on the remote main branch counts
/// open wherever it stands (`not landed`, `parked`, `WIP`, `for later`, …), and a blocked item
/// is open as under Cruise, its `Blocker:` a head of open work of its own (aterm `0b4c90d4ec`'s
/// Land heads: a blocker that is not none is work left). NEGATIVE CONTROLS: Cruise's reading
/// reads no land word; a negated land word (`nothing is parked or left for later`) is clean
/// under Land too.
#[test]
fn land_reads_strictly_and_counts_unlanded_work() {
    for (tail, want) in [
        ("The lexer is not landed.", &["not landed"][..]),
        ("The refactor is parked on a branch.", &["parked"]),
        ("The WIP refactor stays where it is.", &["wip"]),
        ("The docs pass is for later.", &["for later"]),
        ("Two commits are unpushed.", &["unpushed"]),
        (
            "Still open: the release tag (blocked on the owner's credential).",
            &["still open"],
        ),
        (
            "- Windows build: not compiled. Blocker: no Windows std on this host.",
            &["not compiled", "blocker"],
        ),
    ] {
        assert_eq!(open_work_strict(Some(tail)), want, "{tail:?}");
    }
    for tail in [
        "The lexer is not landed.",
        "The refactor is parked on a branch.",
        "The docs pass is for later.",
        "Two commits are unpushed.",
    ] {
        assert!(open_work(Some(tail)).is_empty(), "Cruise: {tail:?}");
    }
    for tail in [
        "Nothing is parked or left for later.",
        "No commit is unpushed; every branch is merged into main.",
        "Merged the parser and the lexer into main and pushed it.",
    ] {
        assert!(open_work_strict(Some(tail)).is_empty(), "{tail:?}");
    }
    assert!(open_work_strict(None).is_empty());
}

/// THE SUFFIX names at most [`OPEN_NAMED`] words, in the tables' own spelling, `…` after the
/// rest; nothing found, no suffix.
#[test]
fn the_open_work_suffix_names_at_most_four_words() {
    assert_eq!(open_work_suffix(&[]), None);
    assert_eq!(
        open_work_suffix(&["still open", "not run"]).as_deref(),
        Some("Your report still names open work (still open, not run): finish and land it first.")
    );
    let many = ["left", "todo", "skipped", "not run", "parked"];
    let suffix = open_work_suffix(&many).unwrap_or_default();
    assert!(
        suffix.contains("(left, todo, skipped, not run, …)"),
        "{suffix}"
    );
    assert!(!suffix.contains("parked"), "{suffix}");
}
