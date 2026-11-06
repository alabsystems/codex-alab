// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The ported readers answer as aterm's own tests say they do (cases copied from
//! `turn_end_tests.rs` at aterm `3f0b07191`, with the rows aterm main `6ff0b5475a` adds: a
//! force-push however it is spelled, the retired token line read as a done claim), and FOREVER
//! CRUISE / FOREVER LAND's readers (aterm `a355109905`, the spec's §2).

use crate::words::Said;
use crate::words::classify_said;
use crate::words::destructive_word;
use crate::words::says_done;
use pretty_assertions::assert_eq;

/// A line that is the retired token is a done claim (never an end: Cruise owes it its prompt,
/// Land its nudge), Markdown around it allowed (aterm's `a_retired_token_line_is_a_done_claim`).
/// NEGATIVE CONTROL: a token quoted in a sentence that makes it a time to come, or one with
/// more letters to it, claims nothing by itself; nor does a quoted token line, someone else's
/// report passed on ([`crate::words::is_finished_line`]).
#[test]
fn a_retired_token_line_is_a_done_claim() {
    for yes in [
        "ATERM-TASK-DONE",
        "All landed.\n`ATERM-TASK-DONE`",
        "All landed.\n**ATERM-TASK-DONE**\n",
        "All landed.\n  ATERM-TASK-DONE.  ",
    ] {
        assert!(says_done(Some(yes)), "{yes:?}");
    }
    for no in [
        "I will write ATERM-TASK-DONE when the task is finished.",
        "Fixed the parser; ATERM-TASK-DONE-ISH is next.",
        "Their report:\n> ATERM-TASK-DONE",
        "",
    ] {
        assert!(!says_done(Some(no)), "{no:?}");
    }
    assert!(!says_done(None));
}

/// A `Nothing left:` defense is a done claim in either mode, and so is a done phrase beside a
/// NEEDS YOU line; the need's own words never make one. NEGATIVE CONTROLS: a report of
/// progress, and a need alone, claim nothing.
#[test]
fn a_defense_and_a_done_phrase_beside_a_need_are_done_claims() {
    for yes in [
        "Nothing left: it is all merged and pushed.",
        "Merged.\nNothing left: every item is on origin/main.",
        "All done, nothing left to do.\nNEEDS YOU: the signing key password",
    ] {
        assert!(says_done(Some(yes)), "{yes:?}");
    }
    for no in [
        "Stage 2 is in; stage 3 is next.",
        "NEEDS YOU: tell me when nothing is left to do",
        "```\nNothing left: inside a fence\n```",
    ] {
        assert!(!says_done(Some(no)), "{no:?}");
    }
}

/// The retired checks' words echoed back are a done claim like any (aterm `d3e1dd842b`'s
/// `CHECK_ECHO`: `nothing of the job is left`, the owner's rule of 2026-10-19, beside
/// `everything left needs a person` and `finished and verified`), owed its act. NEGATIVE
/// CONTROLS: the words negated just before them, or in a clause that makes them a condition,
/// claim nothing.
#[test]
fn the_retired_checks_words_echoed_are_a_done_claim() {
    for yes in [
        "The task is finished and verified.",
        "Everything left needs a person: the release needs the owner's credential.",
        "Nothing of the job is left.",
    ] {
        assert!(says_done(Some(yes)), "{yes:?}");
    }
    for no in [
        "Not everything left needs a person: the lexer tests are mine.",
        "I'll tag it once nothing of the job is left.",
        "If nothing of the job is left, I'll tag it.",
    ] {
        assert!(!says_done(Some(no)), "{no:?}");
    }
}

/// A CLEAN DEFENSE (what Land lands on, with the repository clean): a `Nothing left:` line, no
/// need beside it, and no open work in Land's strict reading of the rest. NEGATIVE CONTROLS:
/// the retired token, a done phrase, a defense naming open work, unlanded work or a need; a
/// `NEEDS YOU: nothing` is no need.
#[test]
fn a_clean_defense_is_a_defense_with_nothing_open_and_no_need() {
    use crate::words::clean_defense;
    use crate::words::land_open_words;
    for yes in [
        "Nothing left: the parser and the lexer are merged and pushed to origin/main.",
        "Merged and pushed.\nNothing left: all of it is on origin/main.\nNEEDS YOU: nothing",
        // `alab/smoke/native.py`'s reply to the land instruction.
        "The cache lives in memory now.\nNothing left: the cache work is verified, and there \
         is no repository here to push.\nNEEDS YOU: nothing",
    ] {
        assert!(clean_defense(Some(yes)), "{yes:?}");
        assert!(land_open_words(Some(yes)).is_empty(), "{yes:?}");
    }
    for (no, open) in [
        ("All landed.\nATERM-TASK-DONE", &[][..]),
        ("All done, nothing left to do.", &[]),
        (
            "Nothing left: merged, but the lexer is not landed.",
            &["not landed"],
        ),
        ("Nothing left: the fix is parked on a branch.", &["parked"]),
        (
            "Nothing left: merged and pushed.\nNEEDS YOU: the signing key password",
            &[],
        ),
    ] {
        assert!(!clean_defense(Some(no)), "{no:?}");
        assert_eq!(land_open_words(Some(no)), open, "{no:?}");
    }
    assert!(!clean_defense(None));
}

/// An act's words: the base, then each suffix that is not empty, one space apart, then the
/// standing rules the base arrived composed with (aterm's `act_words`: the suffixes before the
/// act's one copy of the rules). NEGATIVE CONTROL: words that only mention the rules' opening
/// mid-text carry no rules.
#[test]
fn an_acts_words_join_only_what_is_said() {
    use crate::words::act_words;
    assert_eq!(
        act_words("keep going", &["", "Ack.", ""]),
        "keep going Ack."
    );
    assert_eq!(act_words("keep going", &[]), "keep going");
    assert_eq!(
        act_words("keep going (standing rules: be brief)", &["", "Ack."]),
        "keep going Ack. (standing rules: be brief)"
    );
    assert_eq!(
        act_words("keep going (standing rules: be brief) now", &["Ack."]),
        "keep going (standing rules: be brief) now Ack."
    );
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
    // A force-push however it is spelled (aterm a9f89afaa0, 1548dac3bf): a short `-f`
    // after `push`, reordered or clustered, and `--force-with-lease`.
    for said in [
        "Rebased onto main. Should I git push -f origin fix/lock?",
        "Should I git push origin main -f?",
        "Want me to git push -vf origin main?",
        "All green. Want me to push with --force-with-lease to update the PR?",
    ] {
        assert!(
            matches!(classify_said(Some(said)), Said::Irreversible(_)),
            "{said}"
        );
    }
    for (text, w) in [
        ("git push -f origin main", "push -f"),
        ("pushing -f to main", "push -f"),
        ("git push origin main -f", "push -f"),
        ("git push -vf origin main", "push -f"),
        ("git push origin feature.fix -vuf", "push -f"),
        ("git push origin and -f", "push -f"),
        (
            "git push --force-with-lease origin main",
            "force-with-lease",
        ),
        ("push --force-with-lease=main:4f2a1c9", "force-with-lease"),
    ] {
        assert_eq!(destructive_word(text), Some(w), "{text}");
    }
    // NEGATIVE CONTROLS: a plain push, a ref named `f`, an `f` inside a push option's value,
    // and `-f` on another program, its clause cut from the push's.
    for text in [
        "git push origin main",
        "git push origin f",
        "git push origin main -ofoo",
        "git push origin main; tail -f the log",
        "tail -f the log",
        "cp -f a.txt b.txt",
    ] {
        assert_eq!(destructive_word(text), None, "{text}");
    }
}

/// A DONE CLAIM NAMES NO BARE PUSH (aterm `0b4c90d4ec`'s `land_open_words`): a done claim with
/// no `Nothing left:` line never lands, so its words name only the nudge's suffix, and a push
/// with no place is no word of it. NEGATIVE CONTROLS: the same push in a defense names it
/// (a defense lands only where main has the work), and a done claim that names a branch still
/// names it.
#[test]
fn a_done_claim_names_no_bare_push_and_a_defense_does() {
    use crate::words::clean_defense;
    use crate::words::land_open_words;
    assert_eq!(
        land_open_words(Some("All done: I pushed it.")),
        Vec::<&str>::new()
    );
    assert_eq!(
        land_open_words(Some("Nothing left: I pushed it.")),
        ["to a branch"]
    );
    assert!(!clean_defense(Some("Nothing left: I pushed it.")));
    assert_eq!(
        land_open_words(Some("All done: pushed the branch fix-x.")),
        ["to a branch"]
    );
}

/// THE STANDING RULES ARE THE LEAST-CUT COPY THE COMPOSED TEXTS CARRY
/// ([`crate::words::standing_rules`]): every text aterm composed holds the same rules cut to
/// its own room, so the longest copy is read; aterm's longest rules line (its loop's cap and
/// `…`) arrives whole beside `LAND_ANSWER`; `continue_text`'s copy is read only where the act
/// texts' copy is it or its cut. And THE CUT OF A CUT IS THE CUT OF THE WHOLE: rules too long
/// for any copy still compose an act as aterm composes it from the whole where the act's words
/// leave no more room. NEGATIVE CONTROLS: an owner's `continue_text` that spells the rules'
/// opening itself is not read past the act texts' copy, nor at all where aterm sent its act
/// texts bare (no rules stand: parity review finding 3), and no copy is no rules; only where
/// aterm sent no act text is `continue_text`'s copy the one there is.
#[test]
fn the_standing_rules_are_the_least_cut_copy_the_composed_texts_carry() {
    use crate::needs::NEEDS_HINT;
    use crate::words::DONE_CHECK;
    use crate::words::LAND;
    use crate::words::LAND_ANSWER;
    use crate::words::LAND_NUDGE;
    use crate::words::REVERSIBLE_ANSWER;
    use crate::words::RULES_CAP_CHARS;
    use crate::words::act_words_ruled;
    use crate::words::split_rules;
    use crate::words::standing_rules;
    use crate::words::with_rules;
    let longest = format!("{}…", "a".repeat(RULES_CAP_CHARS));
    assert_eq!(
        split_rules(&with_rules(LAND_ANSWER, Some(&longest))).1,
        Some(longest.as_str()),
        "aterm's longest rules line fits whole beside its land answer"
    );
    let rules = "Never push to upstream; keep every change reversible. ".repeat(7);
    let rules = rules.trim();
    let acts = [DONE_CHECK, LAND, LAND_NUDGE, LAND_ANSWER].map(|t| with_rules(t, Some(rules)));
    let fixed = || acts.iter().map(String::as_str);
    assert_ne!(
        split_rules(&acts[1]).1,
        Some(rules),
        "the land instruction's copy is cut"
    );
    assert_eq!(standing_rules(fixed(), None), Some(rules));
    let long_owner = with_rules(&"keep going ".repeat(45), Some(rules));
    assert_eq!(
        standing_rules(fixed(), Some(long_owner.as_str())),
        Some(rules)
    );
    // A short continue_text holds more of them than any act text: its copy is read.
    let emoji = "🙂 ".repeat(250);
    let emoji = emoji.trim();
    let acts = [DONE_CHECK, LAND, LAND_NUDGE, LAND_ANSWER].map(|t| with_rules(t, Some(emoji)));
    let fixed = || acts.iter().map(String::as_str);
    let short_owner = with_rules("keep going", Some(emoji));
    let owner_copy = split_rules(&short_owner).1;
    let least = standing_rules(fixed(), None).unwrap_or_else(|| panic!("a copy"));
    assert!(owner_copy.is_some_and(|o| o.len() > least.len()));
    assert_eq!(
        standing_rules(fixed(), Some(short_owner.as_str())),
        owner_copy
    );
    // The cut of a cut: the reversible answer composed from the land answer's cut copy is the
    // one aterm composes from the whole.
    assert_eq!(
        act_words_ruled(REVERSIBLE_ANSWER, &[NEEDS_HINT], Some(least)),
        act_words_ruled(REVERSIBLE_ANSWER, &[NEEDS_HINT], Some(emoji))
    );
    // NEGATIVE CONTROLS.
    let acts = [DONE_CHECK, LAND, LAND_NUDGE, LAND_ANSWER].map(|t| with_rules(t, Some(rules)));
    let fixed = || acts.iter().map(String::as_str);
    let spelled = with_rules("keep going (standing rules: be brief)", Some(rules));
    assert_eq!(standing_rules(fixed(), Some(spelled.as_str())), Some(rules));
    assert_eq!(
        standing_rules(
            [DONE_CHECK, LAND, LAND_NUDGE, LAND_ANSWER],
            Some("keep going")
        ),
        None
    );
    assert_eq!(
        standing_rules([LAND_ANSWER], Some(long_owner.as_str())),
        None,
        "an act text sent bare: no rules stand"
    );
    assert_eq!(
        standing_rules([LAND_ANSWER], Some("keep going (standing rules: be brief)")),
        None
    );
    assert_eq!(
        standing_rules(std::iter::empty(), Some(long_owner.as_str())),
        split_rules(&long_owner).1,
        "no act text sent: continue_text's copy is the one there is"
    );
}

/// AN ACT CUT FROM A COPY aterm ALREADY CUT IS aterm's CUT OF THE WHOLE: where aterm sends a
/// text whose standing rules it had to cut, codex-alab, typing the act's suffixes before them
/// ([`crate::words::fitted_act`]), can only cut them further, and that is exactly aterm's own
/// cut from the whole, since every suffixed act leaves the rules less room than its bare text
/// did (`fit_units` of a cut is the cut of the whole at the smaller room). Land's
/// instruction, the defend prompt and the nudge, each with the reversible clause and the ack,
/// under 377 characters of rules and a line aterm's loop capped. NEGATIVE CONTROL: the
/// received copy alone is no act's: the suffixes cost the rules room.
#[test]
fn an_act_cut_from_a_cut_copy_is_aterms_cut_of_the_whole() {
    use crate::needs::NEEDS_ACK;
    use crate::words::DONE_CHECK;
    use crate::words::DONE_CHECK_REVERSIBLE;
    use crate::words::LAND;
    use crate::words::LAND_NUDGE;
    use crate::words::RULES_CAP_CHARS;
    use crate::words::act_words;
    use crate::words::act_words_ruled;
    use crate::words::split_rules;
    use crate::words::with_rules;
    let short = "Never push to upstream; keep every change reversible. ".repeat(7);
    // A line past aterm's cap as its loop cuts it: 400 characters, `…` last.
    let capped = format!("{}…", "abc ".repeat(RULES_CAP_CHARS / 4).trim_end());
    let suffixes = [DONE_CHECK_REVERSIBLE, NEEDS_ACK];
    let mut cut_copies = 0;
    for rules in [short.trim(), capped.as_str()] {
        for base in [LAND, DONE_CHECK, LAND_NUDGE] {
            let received = with_rules(base, Some(rules));
            let copy = split_rules(&received).1.unwrap_or_else(|| panic!("a copy"));
            cut_copies += usize::from(copy != rules);
            let typed = act_words(&received, &suffixes);
            assert_eq!(
                typed,
                act_words_ruled(base, &suffixes, Some(rules)),
                "{base:.30}: {copy}"
            );
            // NEGATIVE CONTROL.
            assert_ne!(split_rules(&typed).1, Some(copy), "{base:.30}");
        }
    }
    assert!(
        cut_copies >= 2,
        "some received copies were cut: {cut_copies}"
    );
}

/// LAND'S FIRST ACT KEEPS A SHORT STANDING RULE WHOLE with both its clauses (aterm's
/// `lands_first_act_keeps_a_short_standing_rule_beside_both_its_clauses`): the act that says
/// "commit, merge and push it" must still carry a rule like "never push to main", whether the
/// rules arrive composed into aterm's `land_text` or ride codex-alab's own copy. NEGATIVE
/// CONTROL: the no-repository sentence's first, longer wording left the rules 2 units, cut to
/// nothing.
#[test]
fn lands_first_act_keeps_a_short_standing_rule_beside_both_its_clauses() {
    use crate::needs::NEEDS_ACK;
    use crate::words::DONE_CHECK_REVERSIBLE;
    use crate::words::LAND;
    use crate::words::act_words;
    use crate::words::act_words_ruled;
    use crate::words::with_rules;
    let rule = "never push to main; never touch vendor/.";
    let suffixes = [DONE_CHECK_REVERSIBLE, NEEDS_ACK];
    let whole = format!("{NEEDS_ACK} (standing rules: {rule})");
    let ruled = act_words_ruled(LAND, &suffixes, Some(rule));
    assert!(ruled.ends_with(&whole), "{ruled}");
    let received = act_words(&with_rules(LAND, Some(rule)), &suffixes);
    assert!(received.ends_with(&whole), "{received}");
}

/// AN UNREAD PROBE'S WORDS OUTLAST THE OPEN WORDS ([`crate::words::fitted_act`], aterm main
/// `e32ba59da2`): over every base that leaves [`crate::words::REPO_UNREAD`] room only beside
/// fewer open words, the act types it whole and names fewer; it is left out only once no open
/// word is named. NEGATIVE CONTROL: some base lengths keep it beside fewer open words, so the
/// rule is reached (a loop that stopped at the first count of open words that fit, as a clean
/// repository's does, typed every open word there and left the unread words out).
#[test]
fn an_unread_probes_words_outlast_the_open_words() {
    use crate::open_work::OPEN_NAMED;
    use crate::repo::RepoRead;
    use crate::words::REPO_UNREAD;
    use crate::words::Suffix;
    use crate::words::VERBATIM_UNITS;
    use crate::words::fitted_act;
    let open = [
        "the lexer",
        "the docs",
        "the e2e suite",
        "port the reader",
        "the bench",
    ];
    let most = open.len().min(OPEN_NAMED);
    let mut fewer = 0;
    for len in 400..VERBATIM_UNITS {
        let base = "w".repeat(len);
        let act = fitted_act(
            &base,
            &[
                Suffix::Open(&open),
                Suffix::Repo(RepoRead::Unread, crate::words::RepoAsk::All),
            ],
        );
        let named = open.iter().filter(|w| act.contains(*w)).count();
        if act.contains(REPO_UNREAD) {
            fewer += usize::from(named < most);
        } else {
            assert_eq!(named, 0, "a base of {len}: {act}");
        }
    }
    assert!(
        fewer > 0,
        "no base made the open words fewer beside the unread words"
    );
}

/// A FACT RIDES WHOLE OR NOT AT ALL (aterm's `fact_rides`; CONTRACT §5 "aterm fact", gap 23): an
/// act's words and its standing rules, whole, with aterm's rider after them ride at exactly
/// [`crate::words::VERBATIM_UNITS`] UTF-16 units, the act and its rules unchanged; a unit more
/// (a character, or a character outside the BMP counting its two units) and the rider is left off
/// whole, neither cut nor bought with fewer rules. NEGATIVE CONTROL: the same act a unit under
/// the bound rides too.
#[test]
fn a_fact_rides_whole_or_not_at_all() {
    use crate::words::DONE_CHECK;
    use crate::words::VERBATIM_UNITS;
    use crate::words::fitted_act;
    use crate::words::with_fact;
    use crate::words::with_rules;
    let units = |s: &str| s.encode_utf16().count();
    let rules = "record each decision and its reason ".repeat(8);
    let rules = rules.trim_end();
    let said = fitted_act(&with_rules(DONE_CHECK, Some(rules)), &[]);
    let whole_rules = format!(" (standing rules: {rules})");
    assert!(
        said.ends_with(&whole_rules),
        "the act carries its rules whole: {said}"
    );
    let stalled = "a background call (pid 51240) has made no progress for 6 min";
    let room = VERBATIM_UNITS - units(&said) - units(" (aterm: )");
    assert!(room > units(stalled) + 1, "{room}");
    let text = format!("{stalled}{}", "z".repeat(room - units(stalled)));
    let rider = |text: &str| format!(" (aterm: {text})");
    let ridden = with_fact(&said, &rider(&text)).expect("exactly the bound rides");
    assert_eq!(units(&ridden), VERBATIM_UNITS);
    assert_eq!(ridden, format!("{said}{}", rider(&text)));
    assert!(
        ridden.contains(&whole_rules),
        "the rules are not cut for it"
    );
    let under = &text[..text.len() - 1];
    assert_eq!(
        with_fact(&said, &rider(under)),
        Some(format!("{said}{}", rider(under)))
    );
    assert_eq!(with_fact(&said, &rider(&format!("{text}z"))), None);
    let wide = format!("{under}\u{1d11e}");
    assert_eq!(wide.chars().count(), text.chars().count());
    assert_eq!(
        with_fact(&said, &rider(&wide)),
        None,
        "a two-unit character"
    );
}
