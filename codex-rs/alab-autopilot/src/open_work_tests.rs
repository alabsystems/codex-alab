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
