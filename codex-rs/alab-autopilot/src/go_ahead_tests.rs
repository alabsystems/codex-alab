// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The go-ahead reader against `fixtures/go-ahead-table.txt`, and the park it hands the answer.
//!
//! PORTED VERBATIM from aterm's `crates/aterm-agent/src/supervise/policy/go_ahead_tests.rs` at
//! aterm main `79002d08ce` (its owner-classification test from `718835900b` and `dc6fd84782`
//! included), only its paths changed (aterm's `turn_end` is [`crate::words`], its
//! `guard_base_tests.rs` helpers are [`escaped`], [`unescaped`] and [`SAME`] here, and a hidden
//! character is what [`crate::needs::sanitized`] drops), with aterm's
//! `the_guards_cut_takes_no_go_ahead_join` (`needs_tests.rs`) and the guard base's reader
//! (`guard_base`, `turn_end_go_ahead_tests.rs`) beside them.

use super::*;
use crate::words::Said;

/// Whether `c` is a character [`crate::needs::sanitized`] drops (aterm's `is_hidden_char`).
fn hidden(c: char) -> bool {
    c.is_control() || codex_alab_aterm::wire::is_format_char(c)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Row {
    Park,
    Person,
    Plain,
}

/// `go-ahead-table.txt`'s rows, in order: each one's section and its reply.
pub(crate) fn table() -> Vec<(Row, String)> {
    let table = include_str!("fixtures/go-ahead-table.txt");
    let mut rows = Vec::new();
    let mut section = None;
    for line in table.lines() {
        match line {
            "== park" => section = Some(Row::Park),
            "== person" => section = Some(Row::Person),
            "== plain" => section = Some(Row::Plain),
            _ if line.starts_with('#') || line.trim().is_empty() => {}
            _ => rows.push((
                section.unwrap_or_else(|| panic!("a row before its section: {line:?}")),
                line.replace("\\n", "\n"),
            )),
        }
    }
    rows
}

/// `reply` as a screen of `columns` draws it: each line wrapped at a word behind a 2-column
/// gutter, so a sentence straddles rows.
pub(crate) fn wrapped(reply: &str, columns: usize) -> String {
    let width = columns - 2;
    let mut rows = Vec::new();
    for line in reply.lines() {
        let mut row = String::new();
        for word in line.split(' ') {
            if !row.is_empty() && row.chars().count() + 1 + word.chars().count() > width {
                rows.push(format!("  {row}"));
                row.clear();
            }
            if !row.is_empty() {
                row.push(' ');
            }
            row.push_str(word);
        }
        rows.push(if row.is_empty() {
            String::new()
        } else {
            format!("  {row}")
        });
    }
    rows.join("\n")
}

/// The widths a table row is read at: as written, and wrapped at 80, 100, 101 and 120 columns.
pub(crate) const DRAWN: [Option<usize>; 5] = [None, Some(80), Some(100), Some(101), Some(120)];

/// `reply` as drawn at `columns` ([`DRAWN`]).
pub(crate) fn drawn(reply: &str, columns: Option<usize>) -> String {
    columns.map_or_else(|| reply.to_string(), |c| wrapped(reply, c))
}

/// `reply` as Claude Code renders it: inline code and bold drawn bare, with no backticks and no
/// `**`, so a code span's words read as prose.
pub(crate) fn rendered(reply: &str) -> String {
    reply.replace('`', "").replace("**", "")
}

/// The incident's turn of 2026-10-26T23:17Z..23:25Z (transcript `65ef503b…`): its earlier
/// messages, which name code (`plan()`, `tap_of`, `for_launch`), and its last reply.
pub(crate) const INCIDENT_TURN: (&[&str], &str) = (
    &[
        "`note_wait` also restarts the clock on a person's or a limit's wait word, and the watch \
         calls it, so the doc sentence I wrote overstates. Correcting the doc and the commit \
         message:",
        "Reflowing that paragraph, and making line 240 match the new Cruise guard rule:",
        "Since the tell now runs `plan()` on each visit where a tell is due, checking that `plan` \
         writes nothing (e.g. through `tap_of` or `for_launch`):",
        "`plan` only reads. Reviewing this round's drive diff while the window test finishes:",
        "The Tell arm repeats the Announce arm's plan construction (~30 lines). Checking whether \
         `mode` and `model` are used elsewhere in the Announce arm, so one shared closure can \
         serve both:",
        "Factoring the plan into one closure that both arms call:",
        "Main's change touches `crates/aterm-cli/src/ship/release.rs`, which my branch doesn't.",
        "That was my own edit. Now queuing the agent crate's tests behind the window build (the \
         shared cache's lock runs them one after the other, not side by side):",
    ],
    "Two builds are queued: the window's tell test, then the agent crate's tell and notice tests \
     after the shared-plan refactor. I'll pick up when they report.\n\nWhere this round stands \
     (not committed or pushed yet):\n- **Fixed, review finding 6:** the \"Upgrade waiting\" line \
     is no longer typed for a tab aterm couldn't actually relaunch, such as one under a dash \
     shell or a HOME the relaunch line can't carry. It now runs the move's own relaunch plan \
     first.\n- **Fixed, review finding 5:** the line names `aterm ctl @self restart` only when \
     the new build is proved to start, which is what that restart itself requires.\n- \
     **Changed, Cruise:** a manager's wait or the person's own exchange no longer holds the \
     line, matching main's Cruise continuation. Only a person actually at the tab holds it. \
     Land and Off keep every guard.\n- **Text:** the help, changelog, design doc and SPECS no \
     longer overclaim: \"at a turn end\" rather than \"first\", the `/goal` exception, and the \
     count is per agent process.\n- **Tests:** the agent crate's tell and quiet tests pass \
     (194) with the two new drive cases. Each new gate, removed on its own, makes its case \
     fail.\n\nNEEDS YOU: say \"rerun the checks\" when the Mac has memory to spare. The full \
     verify on the merged branch was stopped for low memory, and I'm not restarting it on my \
     own.",
);

/// THE INCIDENT'S TURN PARKS AS THE SCREEN DRAWS IT (2026-10-29 review 5): Claude Code draws
/// inline code bare, so `for_launch` and `ship/release.rs` in an earlier message read as prose;
/// an identifier or a path names no outward act the go-ahead would be read as covering, so the
/// turn parks, as written and drawn bare at every width.
#[test]
fn the_incidents_turn_drawn_bare_parks_whatever_code_its_messages_name() {
    let (earlier, tail) = INCIDENT_TURN;
    let mut wrong = Vec::new();
    for bare in [false, true] {
        let draw = |t: &str| if bare { rendered(t) } else { t.to_string() };
        for columns in DRAWN {
            let said: Vec<String> = earlier.iter().map(|m| drawn(&draw(m), columns)).collect();
            let park = parked(&said.join("\n\n"), &drawn(&draw(tail), columns));
            if park.as_ref().and_then(|p| p.token.as_deref()) != Some("rerun the checks") {
                wrong.push(format!("bare {bare} at {columns:?}: {park:?}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    // An outward act an earlier message names in its words, not in an identifier, still parks
    // nothing (the control).
    let plan = drawn(
        &rendered("Next I'll tag the release and launch it."),
        Some(80),
    );
    assert!(parked(&plan, tail).is_none());
}

/// The rows whose reading differs from their section, at every width of [`DRAWN`], as written
/// and, for a park or a person's need, as Claude Code renders it ([`rendered`]): its code drawn
/// bare. A plain row that quotes a park in code reads as that park once drawn bare
/// (ATERM-KI-1020-2), and gets the go-ahead, which limits itself.
fn misread() -> Vec<String> {
    let mut wrong = Vec::new();
    for (row, reply) in table() {
        let bares: &[bool] = if row == Row::Plain {
            &[false]
        } else {
            &[false, true]
        };
        for &bare in bares {
            let reply = if bare {
                rendered(&reply)
            } else {
                reply.clone()
            };
            for columns in DRAWN {
                let text = drawn(&reply, columns);
                if parked("", &text).is_some() != (row == Row::Park) {
                    wrong.push(format!("{row:?} at {columns:?}, bare {bare}: {reply:?}"));
                }
            }
        }
    }
    wrong
}

#[test]
fn every_go_ahead_row_reads_as_its_section_however_it_wraps() {
    let rows = table();
    for row in [Row::Park, Row::Person, Row::Plain] {
        assert!(
            rows.iter().filter(|(r, _)| *r == row).count() >= 10,
            "the table keeps ten {row:?} rows at least"
        );
    }
    let wrong = misread();
    assert!(
        wrong.is_empty(),
        "{} misread:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn uppercase_owners_keep_their_work_outside_the_standing_go_ahead() {
    // Establish own work, acronym/deadline prose and the existing S2/S7 boundaries before
    // the regression cases. These run through the real reader without typing any answer.
    for reply in [
        "NEEDS YOU: say \"go\" so I can rebuild my branch.",
        "NEEDS YOU: say \"go\" so I can rebuild the branch I made.",
        "WAITING FOR YOUR APPROVAL TO MERGE MY BRANCH TO MAIN.",
        "NEEDS YOU: the go-ahead by EOD so I can merge my branch into main.",
        "NEEDS YOU: your go-ahead by Friday so I can merge my branch into main.",
        "NEEDS YOU: the go-ahead by May so I can merge my branch into main.",
        "NEEDS YOU: the go-ahead by MAY so I can rebuild my branch.",
        "NEEDS YOU: your go-ahead by Monday so I can rebuild my branch.",
        "Waiting for your go-ahead from Slack before I merge my branch to main.",
        "The census check now flags a token in a log line.\n\nNEEDS YOU: say \"go\" so I can rerun the checks and commit.",
    ] {
        assert!(parked("", reply).is_some(), "own work still parks: {reply}");
    }
    for reply in [
        "NEEDS YOU: approval from the security team so I can merge my branch into main.",
        "The build needs your GitHub token, which I don't have.\n\nNEEDS YOU: say \"go\" so I can rebuild the branch.",
    ] {
        assert!(
            parked("", reply).is_none(),
            "the boundary still holds: {reply}"
        );
    }

    let mut wrong = Vec::new();
    for reply in [
        "NEEDS YOU: say \"go\" so I can rebuild ALICE's branch.",
        "NEEDS YOU: say \"go\" so I can rebuild the branch ALICE made.",
        "NEEDS YOU: ALICE's approval so I can rebuild my branch.",
        "NEEDS YOU: say \"go\" so I can rebuild MAY's branch.",
        "NEEDS YOU: say \"go\" so I can rebuild the branch MAY made.",
    ] {
        for columns in DRAWN {
            let text = drawn(reply, columns);
            if let Some(park) = parked("", &text) {
                wrong.push(format!("{columns:?}: {text:?} became {park:?}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

fn dropping<T>(part: Part, read: impl FnOnce() -> T) -> T {
    DROPPED.with(|d| d.set(part as u16));
    let out = read();
    DROPPED.with(|d| d.set(0));
    out
}

#[test]
fn every_family_and_exclusion_holds_some_row() {
    let families = [
        Part::Consent,
        Part::Says,
        Part::Fixed,
        Part::Slots,
        Part::OwnInitiative,
        Part::StopNote,
    ];
    let exclusions = [
        Part::PersonOnly,
        Part::PersonFirst,
        Part::Outward,
        Part::SomeoneElse,
        Part::Declined,
        Part::Chooses,
        Part::Echoes,
        Part::Attributed,
    ];
    let rows = table();
    for part in families {
        let missed = dropping(part, || {
            rows.iter()
                .any(|(row, reply)| *row == Row::Park && parked("", reply).is_none())
        });
        assert!(
            missed,
            "dropping family {} leaves every park row parked",
            part as u16
        );
    }
    for part in exclusions {
        let parks = dropping(part, || {
            rows.iter()
                .any(|(row, reply)| *row != Row::Park && parked("", reply).is_some())
        });
        assert!(
            parks,
            "dropping exclusion {} parks no person or plain row",
            part as u16
        );
    }
}

/// The incident's last reply (2026-10-29), the one the owner read.
const INCIDENT: &str = "Nothing has changed. The build is waiting only for your go-ahead.\n\nNEEDS YOU: \
                        say \"rerun the checks\" so I can build and test the upgrade fix branch, push \
                        it to main and remove the worktree.";

#[test]
fn the_incident_reply_parks_on_the_word_it_asked_for() {
    let park = parked("", INCIDENT).expect("the incident parks");
    assert_eq!(park.token.as_deref(), Some("rerun the checks"));
    assert!(park.need && !park.others && !park.waits_room, "{park:?}");
    assert!(
        park.quote.starts_with("say \"rerun the checks\""),
        "{park:?}"
    );
    assert_eq!(
        park.said(800),
        format!("{TOKEN_OPEN}rerun the checks{TOKEN_CLOSE}")
    );
}

#[test]
fn a_park_in_an_earlier_message_of_the_turn_parks() {
    let earlier = "NEEDS YOU: say \"rerun the checks\" so I can build and test the branch.";
    let park = parked(earlier, "Checked the branch again; nothing moved.").expect("parks");
    assert_eq!(park.token.as_deref(), Some("rerun the checks"));
    assert!(parked("", "Checked the branch again; nothing moved.").is_none());
}

#[test]
fn a_park_beside_a_persons_needs_keeps_them_and_names_room() {
    let first = "The fix isn't on main yet.\n\nNEEDS YOU: say \"rerun the checks\" when the Mac has \
                 memory to spare (the run on the merged branch was stopped for low memory and I'm \
                 not restarting it on my own).\nNEEDS YOU: sign agy and Muse in on this Mac \
                 (browser OAuth; run `aterm ai doctor` at a terminal).\nNEEDS YOU: decide on \
                 public hosting of the signed fleet document (outward-facing publish).";
    let park = parked("", first).expect("the go-ahead need parks");
    assert!(park.need && park.others && park.waits_room, "{park:?}");
    let prose = parked(
        "",
        "The build was stopped for low memory, so I won't start it again unless you ask.",
    )
    .expect("parks");
    assert!(
        prose.waits_room && !prose.need && !prose.others,
        "{prose:?}"
    );
    assert_eq!(prose.token, None);
}

#[test]
fn a_park_is_quoted_without_hidden_characters_and_fits_its_room() {
    let reply =
        "NEEDS YOU: say \"rerun\u{200B} the\u{202E} checks 42\" so I can rebuild the branch.";
    let park = parked("", reply).expect("parks");
    let token = park.token.as_deref().expect("a token");
    assert!(!token.chars().any(hidden), "{token:?}");
    assert!(!park.quote.chars().any(hidden));
    let long = format!(
        "The build is waiting for your go-ahead, {}.",
        "and the rebuild after it ".repeat(12)
    );
    let park = parked("", &long).expect("parks");
    assert!(park.quote.encode_utf16().count() <= QUOTE_UNITS && park.quote.ends_with('…'));
    for room in [0, 10, 40, 80, 200] {
        let said = park.said(room);
        assert!(said.encode_utf16().count() <= room, "{room}: {said:?}");
    }
    assert!(park.said(17).is_empty());
    assert!(
        park.said(60)
            .starts_with("You parked on: \"The build is waiting")
    );
}

#[test]
fn the_answer_parks_nothing() {
    for text in [
        GO_AHEAD_ANSWER,
        GO_AHEAD_ROOM,
        GO_AHEAD_DROP,
        GO_AHEAD_OTHERS,
    ] {
        assert!(parked("", text).is_none(), "{text}");
        assert!(!parks(text), "{text}");
    }
}

#[test]
fn an_outward_plan_anywhere_in_the_turn_parks_nothing() {
    let plan = "Plan: delete the stale release tags on the remote, then push my branch to main. \
                Tests are green.";
    let ask = "Waiting for your go-ahead to push.";
    assert!(parked(plan, ask).is_none());
    assert!(parked("Tests are green.", ask).is_some());
    let tagged = "Tagged v1.4.0; the next push runs the crates.io publish.";
    assert!(parked(tagged, "NEEDS YOU: say go so I can push.").is_none());
}

/// Every review's repros (2026-10-29, rounds 1 to 5): the replies each review found misread,
/// in the base guard's corpus ([`no_reading_is_weaker_than_the_base_guard`]).
pub(crate) const REPROS: &[&str] = &[
    "Nothing has changed. The build is waiting only for your go-ahead.",
    "I'll keep waiting until I have your go-ahead to delete the old release tags.",
    "I won't delete the stale release tags until I have your go-ahead.",
    "I've built the installer and with your go-ahead I'll upload it to the App Store.",
    "The [aterm harness] go-ahead covers my branch only, so deleting the old release tags still \
     needs your go-ahead.",
    "As I said, \"deleting the old release tags needs your go-ahead.\"",
    "Tagged v1.4.0 for the crates.io release.\nNEEDS YOU: say go so I can push.",
    "The cleanup job deletes the stale release tags on the remote once you say go.\n\nNEEDS YOU: \
     say go so I can rebuild and push my branch to main.",
    "CI uploads every green build of main to TestFlight. The suite is green. Waiting for your \
     go-ahead to push my branch to main.",
    "The branch is ready for your review. Once you say go, I'll push it to main.",
    "Once you've had a look, just say go and I'll push my branch to main.",
    "After you take a look, say go and I'll push it to main.",
    "I'll merge the Dependabot PR into main once you say so.",
    "I'll merge the three open PRs into main once you say so.",
    "I'll merge PR #412 into main once you say so.",
    "Canary is healthy; I'll continue the rollout to the remaining hosts once you give the \
     go-ahead.",
    "I'll resume the backfill on the orders table once you give the go-ahead.",
    "NEEDS YOU: say go so I can rerun the checks.\nNEEDS YOU: say go so I can delete the stale \
     release tags on the remote.",
    "Say \"go\" so I can rerun the checks. Say \"go\" so I can delete the stale release tags on \
     the remote.",
    "Give me the go-ahead and I'll force-push the branch.",
    "With your go-ahead I'll delete the old releases.",
    "I'll wait for your go-ahead before force-pushing.",
    "I need your go-ahead to delete the stale release tags; the standing go-ahead covers only my \
     own reversible work.",
    "I have the go-ahead for my branch, but deleting the old release tags still needs your \
     go-ahead.",
    "Status:\n│ delete old release tags │ waiting for your go-ahead │",
    "I won't delete the old release tags without your approval.",
    "The release is built. I won't publish it without your approval.",
    "I won't deploy without your go-ahead.",
    "I can't force-push the branch without your go-ahead.",
    "I'll deploy without waiting for you.",
    "Rebuilt and tested. I need your approval to push:\n\n```\ngit push --force origin main\n```",
    "I won't merge without your sign-off.",
    "The old reply said:\n```\nI'm blocked on your review.\n```",
    "I won't push my branch without your go-ahead. Pushing it also deletes the stale release tags \
     on the remote.",
    "Give me the go-ahead and I'll rotate the production API keys.",
    "On your go-ahead I'll email the customers.",
    "With your go-ahead I'll open the tracking issue.",
    "I'll push the merged branch with your go-ahead.",
    "No force-push and nothing published. I'm still waiting for your go-ahead before rebuilding.",
    // Review 5: a `NEEDS YOU items …` row the base cut as a need under a choice and a question.
    "Two ways to clear the stale tags:\n1. Delete the stale release tags and re-tag.\n2. Leave \
     them in place.\nWhich one?\nNEEDS YOU items stay on the menu bar until you answer.",
    "Should I rebase onto main first?\nNEEDS YOU items stay on the menu bar until you answer.",
];

/// What an agent writes once it has the go-ahead, or of it: echoes, reports and paraphrases.
pub(crate) const ECHOES: &[&str] = &[
    "Rebuilt and pushed with your go-ahead; the suite is green and the worktree is gone.",
    "Rebuilt and pushed to main with your go-ahead.",
    "I no longer need your go-ahead to rebuild, so I'm rebuilding now.",
    "Since the go-ahead stands I'm not waiting; rebuilding the branch now.",
    "Since the go-ahead stands I'm not waiting: I merged main, built and tested the branch, pushed \
     it to main and removed the worktree.",
    "The harness gave the owner's standing go-ahead, so I'm rerunning the checks.",
    "I rebuilt without waiting for your go-ahead, per the ruling.",
    "Got the go-ahead, so I deleted the stale worktree I made and rebuilt.",
    "The rebuild that was waiting for your go-ahead is done: built, tested and pushed to main.",
    "Proceeding with my own reversible work under the owner's standing go-ahead: rerunning the \
     checks.",
    "This adds a go-ahead reader; its rows are in the table.",
    "This fix adds a go-ahead reader; its table holds rows such as:\n```\nI won't restart it \
     unless you ask.\nThe build is waiting for your go-ahead.\n```\nEach of them parks.",
];

/// THE GUARD'S CORPUS: every table row at every width a tab draws it ([`DRAWN`]), every review's
/// repro and every echo. `fixtures/go-ahead-guard-base.txt` holds the base's guard for each.
pub(crate) fn guard_corpus() -> Vec<String> {
    let mut texts: Vec<String> = Vec::new();
    for (_, reply) in table() {
        texts.extend(DRAWN.iter().map(|&c| drawn(&reply, c)));
    }
    texts.extend(REPROS.iter().chain(ECHOES).map(|t| (*t).to_string()));
    texts
}

/// WRITE THE GUARD'S CORPUS for `guard_base_tests.rs` to read at 246d1ddc2f: one escaped text a
/// line, into the file `GA_CORPUS` names.
#[test]
#[ignore = "writes the corpus guard_base_tests.rs reads at 246d1ddc2f"]
fn write_guard_corpus() {
    let path = std::env::var("GA_CORPUS").expect("GA_CORPUS is set");
    let lines: Vec<String> = guard_corpus().iter().map(|t| escaped(t)).collect();
    std::fs::write(path, lines.join("\n") + "\n").expect("the corpus writes");
}

/// A reading's strength in the irreversible guard: an irreversible ask over a stop over the rest.
pub(crate) fn strength(said: &Said) -> u8 {
    match said {
        Said::Irreversible(_) => 2,
        Said::Stop(_) => 1,
        Said::Question | Said::Offer | Said::Plain => 0,
    }
}

/// THE IRREVERSIBLE GUARD IS NEVER WEAKER THAN CODEX-ALAB'S BASE (aterm's 2026-10-29 review 3,
/// read against codex-alab's own guard): every table row at every width, every review's repro and
/// every echo is cut and read as the base cut and read it (`fixtures/go-ahead-guard-base.txt`,
/// aterm's guard at 246d1ddc2f, which codex-alab's `classify_said` over `needs::without_needs`
/// equalled at origin/main 5b7a00447a), or reads stronger, whatever the go-ahead reader makes of
/// it. aterm's own form of this test ranks the base by a stop phrase and a destructive word
/// anywhere in the text; codex-alab's guard has cut the NEEDS YOU lines before reading since
/// before the go-ahead (`classify_said`), so its base is that cut's reading, recorded per text.
#[test]
fn no_reading_is_weaker_than_the_base_guard() {
    use crate::words::classify_said;
    let base = guard_base();
    let mut weaker = Vec::new();
    let mut cut_moved = Vec::new();
    let mut kinds = [0usize; 3];
    for text in guard_corpus() {
        let Some((cut, said)) = base.get(&text) else {
            weaker.push(format!("no base record: {text:?}"));
            continue;
        };
        kinds[usize::from(strength(said))] += 1;
        let read = classify_said(Some(&text));
        if strength(&read) < strength(said) {
            weaker.push(format!("{text:?}: {read:?} under the base's {said:?}"));
        }
        if crate::needs::without_needs(&text) != *cut {
            cut_moved.push(format!("{text:?}"));
        }
    }
    assert!(weaker.is_empty(), "{}", weaker.join("\n"));
    // The guard cuts what the base cut: a need joined further would hide its rows from it.
    assert!(cut_moved.is_empty(), "{}", cut_moved.join("\n"));
    // The base reads each kind: the control that the corpus holds irreversible asks and stops.
    assert!(kinds[1] > 0 && kinds[2] > 0, "{kinds:?}");
}

/// THE ECHOES PARK NOTHING: an agent acting on the go-ahead, or reporting it, gets no second one.
#[test]
fn an_echo_or_a_report_of_the_go_ahead_parks_nothing() {
    for text in ECHOES {
        assert!(parked("", text).is_none(), "parks: {text}");
    }
}

/// A WORD ANOTHER NEED ASKS FOR AN ACT THE GO-AHEAD DOES NOT COVER IS NEVER SAID (2026-10-29
/// review 3): the reply's own reversible need parks, and the answer quotes it rather than say
/// `go`, which the other need asks for a deletion on the remote.
#[test]
fn a_word_another_need_asks_for_an_excluded_act_is_never_said() {
    let two = "NEEDS YOU: say go so I can rerun the checks.\nNEEDS YOU: say go so I can delete the \
               stale release tags on the remote.";
    let park = parked("", two).expect("the reversible need parks");
    assert_eq!(park.token, None, "{park:?}");
    assert!(park.others, "{park:?}");
    let said = park.said(800);
    assert!(
        said.starts_with(QUOTE_OPEN) && !said.contains(TOKEN_CLOSE),
        "{said}"
    );
    // Alone, the reversible need's word is said.
    let one = parked("", "NEEDS YOU: say go so I can rerun the checks.").expect("parks");
    assert_eq!(one.token.as_deref(), Some("go"));
    assert!(one.said(800).contains(TOKEN_CLOSE));
}

/// THE LINES A PARK'S GO-AHEAD ANSWERS ARE ITS OWN, AS THE MENU BAR NAMES THEM (§15.6): the
/// table's first park reply names the go-ahead's line beside a sign-in and a publish decision,
/// and only the go-ahead's line is the park's, cleaned as `needs_you` writes it; the incident's
/// reply is its one line at every width; a park in prose answers no line; a turn that plans an
/// outward act parks nothing. Where a parking need took rows only the go-ahead's view joins (a
/// text of the guard's corpus wrapped mid-clause), the line every other reader reads (`needs_you`
/// of the drawn rows, the need's words before those rows) is the park's too.
#[test]
fn a_parks_needs_are_its_go_ahead_lines_as_the_menu_bar_writes_them() {
    let first = table()
        .into_iter()
        .find(|(row, reply)| *row == Row::Park && reply.contains("sign agy and Muse in"))
        .map(|(_, reply)| reply)
        .expect("the table's first park reply");
    let lines = needs::needs_you(&first);
    assert_eq!(lines.len(), 3, "{lines:#?}");
    let park = parked("", &first).expect("parks");
    assert_eq!(park.needs, [lines[0].clone()], "{park:?}");
    assert!(
        lines[0].starts_with("say \"rerun the checks\""),
        "{lines:#?}"
    );
    let park = parked("", INCIDENT).expect("the incident parks");
    assert_eq!(park.needs, needs::needs_you(INCIDENT));
    let prose = parked(
        "",
        "The build was stopped for low memory, so I won't start it again unless you ask.",
    )
    .expect("parks");
    assert!(prose.needs.is_empty(), "{prose:?}");
    assert!(
        parked(
            "Plan: delete the stale release tags on the remote, then push my branch to main.",
            "NEEDS YOU: say \"rerun the checks\" so I can push my branch to main.",
        )
        .is_none()
    );
    for columns in DRAWN {
        let text = drawn(INCIDENT, columns);
        let park = parked("", &text).expect("parks at every width");
        assert_eq!(park.needs, needs::needs_you(&text), "{columns:?}");
    }
    let mut moved = 0;
    for text in guard_corpus() {
        let Some(park) = parked("", &text) else {
            continue;
        };
        let lines = needs::needs_you(&text);
        for m in needs::go_ahead_view(&text).moved {
            if !park.needs.contains(&needs::sanitized(&m.need)) {
                continue;
            }
            moved += 1;
            let base = needs::sanitized(&m.base);
            assert!(
                park.needs.contains(&base) && lines.contains(&base),
                "{text:?}: {base:?} {park:?} {lines:#?}"
            );
        }
    }
    assert!(
        moved > 0,
        "some parking need takes a row only the go-ahead's view joins"
    );
}

/// A PERSON'S NEED THAT CLEANS TO A LINE THE PARK'S NEED CLEANS TO IS NEVER ANSWERED (2026-10-30
/// review 9): the menu bar matches lines by their cleaned words, so where `needs_you` reads two
/// needs as one line, that line is a person's too. Two ways: the agent's own newline after a
/// clause's `so` (the base reads each need's first row alone, the go-ahead's view joins the rest),
/// and two needs past `NEED_CHARS` that differ only after its cut. Either way the reversible need
/// still parks, and its own joined words stay the park's.
#[test]
fn a_line_a_persons_need_cleans_to_is_never_answered() {
    let apart = "NEEDS YOU: say \"go\" when you're back so\nI can rebuild and push my branch to \
                 main.\nNEEDS YOU: say \"go\" when you're back so\nI can use your App Store \
                 Connect key for the upload.";
    let head = "say \"go\" when you're back so".to_string();
    assert_eq!(needs::needs_you(apart), std::slice::from_ref(&head));
    let park = parked("", apart).expect("the reversible need parks");
    assert!(park.others, "{park:?}");
    assert!(!park.needs.contains(&head), "{park:?}");
    assert!(
        park.needs.contains(
            &"say \"go\" when you're back so I can rebuild and push my branch to main.".to_string()
        ),
        "{park:?}"
    );
    let words = "say \"rerun the checks\" so I can build and test the parser branch after the \
                 rebase, compare its timings with the ones on main, rebuild the docs and the \
                 changelog from it, and then";
    let long = format!(
        "NEEDS YOU: {words} push the branch to main.\nNEEDS YOU: {words} sign the build with your \
         Apple ID password."
    );
    let cut = needs::needs_you(&long);
    assert_eq!(cut.len(), 1, "one line past the cut: {cut:#?}");
    let park = parked("", &long).expect("the reversible need parks");
    assert!(park.others, "{park:?}");
    assert!(!park.needs.contains(&cut[0]), "{park:?}");
}

/// NO PERSON'S LINE IS EVER ONE THE GO-AHEAD ANSWERS (§15.6): every park row of the table that
/// names a `NEEDS YOU:` line, joined with every person row that names one, as written and drawn
/// at every width: where the joined reply parks, none of the person's lines is the park's, and
/// as written each of the park's lines is one the menu bar names. The control: with the
/// person-only exclusion dropped, some person's line is the park's.
#[test]
fn no_persons_line_is_ever_answered() {
    let rows = table();
    let needing = |want: Row| -> Vec<String> {
        rows.iter()
            .filter(|(row, reply)| *row == want && reply.contains(needs::NEEDS_YOU))
            .map(|(_, reply)| reply.clone())
            .collect()
    };
    let (parks, persons) = (needing(Row::Park), needing(Row::Person));
    let sweep = || {
        let (mut answered, mut joined_parks, mut wrong) = (0, 0, Vec::new());
        for park in &parks {
            for person in &persons {
                for columns in DRAWN {
                    let joined = drawn(&format!("{park}\n{person}"), columns);
                    let Some(p) = parked("", &joined) else {
                        continue;
                    };
                    joined_parks += 1;
                    for line in needs::needs_you(&drawn(person, columns)) {
                        if p.needs.contains(&line) {
                            answered += 1;
                            wrong.push(format!("{columns:?}: {line:?} in {joined:?}"));
                        }
                    }
                    let named = needs::needs_you(&joined);
                    if columns.is_none() && !p.needs.iter().all(|l| named.contains(l)) {
                        wrong.push(format!("not on the menu bar: {:?} of {joined:?}", p.needs));
                    }
                }
            }
        }
        (answered, joined_parks, wrong)
    };
    let (_, joined_parks, wrong) = sweep();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert!(joined_parks >= 100, "the sweep parks: {joined_parks}");
    let (answered, ..) = dropping(Part::PersonOnly, sweep);
    assert!(
        answered > 0,
        "NEGATIVE CONTROL: without the person-only exclusion some person's line is answered"
    );
}

/// `text` with its backslashes, line breaks and tabs escaped, one field of a guard-base record
/// (aterm's `guard_base_tests::escaped`).
pub(crate) fn escaped(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}

/// One field of a record read back ([`escaped`]'s inverse; aterm's `guard_base_tests::unescaped`).
pub(crate) fn unescaped(field: &str) -> String {
    let mut out = String::new();
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        out.push(match chars.next() {
            Some('n') => '\n',
            Some('t') => '\t',
            Some(other) => other,
            None => '\\',
        });
    }
    out
}

/// The cut field of a record whose cut is its text whole: no escaped text reads so (aterm's
/// `guard_base_tests::SAME`).
pub(crate) const SAME: &str = "\\=";

/// The base's guard (aterm 246d1ddc2f) for each text of the corpus, as
/// `fixtures/go-ahead-guard-base.txt` holds it: its cut, and the kind of its reading (aterm's
/// `turn_end_go_ahead_tests::guard_base`).
pub(crate) fn guard_base() -> std::collections::HashMap<String, (String, Said)> {
    include_str!("fixtures/go-ahead-guard-base.txt")
        .lines()
        .filter(|l| l.contains('\t'))
        .map(|line| {
            let mut fields = line.split('\t');
            let mut field = || fields.next().expect("three fields");
            let text = unescaped(field());
            let cut = match field() {
                SAME => text.clone(),
                cut => unescaped(cut),
            };
            let reading = unescaped(field());
            let said = match reading.split_once(':') {
                Some(("stop", why)) => Said::Stop(why.to_string()),
                Some(("irreversible", why)) => Said::Irreversible(why.to_string()),
                _ => match reading.as_str() {
                    "plain" => Said::Plain,
                    "offer" => Said::Offer,
                    "question" => Said::Question,
                    other => panic!("a reading: {other}"),
                },
            };
            (text, (cut, said))
        })
        .collect()
}
