// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! ATERM'S FACT RIDES AN ACT CODEX-ALAB TYPES ANYWAY end to end (CONTRACT §5 "aterm fact"; gap 23:
//! a hung background command in a codex tab went unmentioned): a real core thread, a wiremock
//! Responses model and the fake aterm, under Cruise or Land. The fake names a no-progress call
//! as aterm's jobs board does ([`FakeAterm::plant_fact`]) and keeps aterm's whole account (which
//! episode, once per episode, the lease, the policy gate); codex-alab reads it at an act under
//! one of aterm's `FACT_RIDES` rules, types the rider whole after the act's words or not at all,
//! and acknowledges it once the turn started and its act was recorded.
//!
//! Every wait is a one-minute hang detector on the fake's requests, the model's bodies or the
//! journal, never a latency budget. Each test names the mutant it kills (gap 23's §9.7).

use codex_alab_aterm::facts::rider;
use codex_alab_autopilot::words;

use super::*;

/// The sentence aterm's jobs board names a stalled background call with.
const STALLED: &str = "a background call (pid 51240) has made no progress for 6 min";

/// Another stalled call's sentence.
const ALSO_STALLED: &str = "a background call (pid 51377) has made no progress for 9 min";

/// The policy every test starts from: no grace, aterm's facts on.
const FACTS_ON: [(&str, &str); 2] = [("human_grace_s", "0"), ("facts", "true")];

async fn claimed(h: &Harness) {
    assert!(
        h.fake
            .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
            .await,
        "{:?}",
        h.autopilot.supervisor().journal()
    );
}

/// The user text the model's `n`th request ends with: what was typed for it.
fn typed(h: &Harness, n: usize) -> String {
    h.mock.requests()[n]
        .message_input_texts("user")
        .last()
        .cloned()
        .unwrap_or_default()
}

/// Every `harness fact for=…` read's `for=`, in order.
fn reads(h: &Harness) -> Vec<String> {
    h.fake
        .fact_reads()
        .into_iter()
        .map(|(rule, _)| rule)
        .collect()
}

/// The fake's request lines' index of the first that contains `needle`.
fn line_at(h: &Harness, needle: &str) -> Option<usize> {
    h.fake
        .requests()
        .iter()
        .position(|line| line.contains(needle))
}

/// A second root thread of the process, kept by the autopilot as the session's other root
/// thread (it is never displayed and never acts). The caller keeps it live.
async fn side_thread(h: &Harness) -> codex_core::NewThread {
    let side = h
        .test
        .thread_manager
        .start_thread(h.test.start_thread_options().await)
        .await
        .unwrap_or_else(|err| panic!("a second root thread: {err}"));
    let key = side.thread_id.to_string();
    wait_until("the second root thread is the autopilot's", || {
        h.autopilot.thread_rt(&key).is_some()
    })
    .await;
    // The TUI shows it once (a thread switch), then the session's own thread again: both are
    // its registered root threads from here on.
    for displayed in [&key, &h.thread] {
        h.shared.observe(
            &Frame {
                primary_thread: Some(&h.thread),
                displayed_thread: Some(displayed),
                rollout_path: None,
                cwd: h.test.cwd_path(),
                model: "gpt-5",
                codex_home: h.test.codex_home_path(),
                embedded: true,
                state: ProgramState::Idle,
                draft_empty: true,
                queued_input: false,
                popup_open: false,
                editor_open: false,
                pending_user_turn: false,
            },
            now_ms(),
        );
    }
    assert!(h.shared.is_root_thread(&key));
    side
}

/// The settled end of a run: after each of `n` model requests, a done claim's defend prompt
/// waits out the short streak's back-off, so nothing more is typed.
async fn settled(h: &Harness, n: usize) {
    wait_until("the done claim's prompt waits out the back-off", || {
        h.mock.requests().len() == n && journal_has(h, "for continue-done-check@v1")
    })
    .await;
}

/// A STALLED CALL RIDES ONE CONTINUATION, AND NOT THE NEXT: the first continuation's user input
/// is the composed act followed by aterm's rider, byte for byte; the ack follows that act's own
/// record (`harness_act`), and aterm's told mark leaves the next continuation bare. The thread is
/// the session's only root thread, so it reads without a background terminal of its own.
/// Kills M20 (no read) and M21 (the rider typed and never acknowledged: the fake would offer the
/// episode again to the same reader, and the next continuation would carry it again).
#[test]
fn a_stalled_call_rides_one_continuation_and_not_the_next() {
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![
                reply("r1", "Stage 1 is in; stage 2 is next."),
                reply("r2", "Stage 2 is in; stage 3 is next."),
                reply("r3", "Stage 3 is in and pushed.\nATERM-TASK-DONE"),
            ],
            &FACTS_ON,
        )
        .await;
        claimed(&h).await;
        let episode = h.fake.plant_fact(STALLED);
        h.test.submit_turn("do the task").await.expect("first turn");
        settled(&h, 3).await;
        assert_eq!(typed(&h, 1), format!("keep going{}", rider(STALLED)));
        assert_eq!(
            typed(&h, 2),
            "keep going",
            "told once: the next act is bare"
        );
        assert_eq!(h.fake.facts_told(), [episode]);
        assert_eq!(reads(&h), [words::RULE_CONTINUE, words::RULE_CONTINUE]);
        assert!(
            h.fake
                .fact_reads()
                .iter()
                .all(|(_, reader)| *reader == h.thread),
            "the reader is the thread: {:?}",
            h.fake.fact_reads()
        );
        let act = line_at(&h, "meta set harness_act continue@v1").expect("the act's record");
        let told = line_at(&h, "harness fact told episode=").expect("the ack");
        assert!(act < told, "the ack follows the act's record");
        assert!(journal_has(&h, "rides continue@v1"));
        assert!(journal_has(&h, "told"));
    });
}

/// A REFUSED ACK LEAVES THE FACT FOR A LATER ACT: aterm refuses the ack (`ERR denied`), so the
/// episode stays untold there, and aterm offers it again to the same reader: the next
/// continuation carries the rider again. Kills M22 (codex-alab keeping a told mark of its own).
#[test]
fn a_refused_ack_leaves_the_fact_for_a_later_act() {
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![
                reply("r1", "Stage 1 is in; stage 2 is next."),
                reply("r2", "Stage 2 is in; stage 3 is next."),
                reply("r3", "Stage 3 is in and pushed.\nATERM-TASK-DONE"),
            ],
            &FACTS_ON,
        )
        .await;
        claimed(&h).await;
        h.fake.refuse_fact_told(true);
        h.fake.fact_lease(Duration::from_secs(1));
        h.fake.plant_fact(STALLED);
        h.test.submit_turn("do the task").await.expect("first turn");
        settled(&h, 3).await;
        let ridden = format!("keep going{}", rider(STALLED));
        assert_eq!(typed(&h, 1), ridden);
        assert_eq!(typed(&h, 2), ridden, "untold in aterm: offered again");
        assert!(h.fake.facts_told().is_empty());
        assert!(journal_has(&h, "not told: Refused"));
    });
}

/// ONLY ATERM'S FOUR ACTS READ, AND ATERM MAY NARROW THEM. Under Land, the instruction
/// (`land@v1`) carries the fact; the nudge (`land-nudge@v1`) reads, and an aterm whose loop lets
/// no fact ride it answers `why=rule`, so the nudge is typed bare though another call stands
/// untold. Under Cruise, a usage window's resume (`usage-resume@v1`, no `FACT_RIDES` rule) reads
/// nothing and types bare, and the continuation after it reads and carries the fact. (The
/// continuation's and the done check's reads are the other tests'; a wall's retry and a
/// question's answer read nothing in [`a_walls_retry_and_a_questions_answer_read_no_fact`], and a
/// compaction's retyped act in [`a_retyped_act_carries_no_fact_told_once`].) Kills M23 (every
/// act reads: the resume's read) and M23'' (codex-alab typing a fact aterm did not give for that
/// act).
#[test]
fn only_aterms_four_acts_read_and_aterm_may_narrow_them() {
    big_stack(|| async {
        let h = harness_with(
            Mode::Current,
            vec![
                reply("r1", "Stage 1 is in."),
                reply("r2", "Stage 2 is in."),
                reply(
                    "r3",
                    "Nothing left: every change is merged and pushed to origin/main.",
                ),
            ],
            &FACTS_ON,
            &[("autopilot", "land")],
        )
        .await;
        claimed(&h).await;
        let first = h.fake.plant_fact(STALLED);
        h.fake.plant_fact(ALSO_STALLED);
        h.fake.narrow_fact_rides(words::RULE_LAND_NUDGE);
        h.test.submit_turn("do the task").await.expect("first turn");
        assert!(
            h.fake
                .wait_for(|f| f.acts().iter().any(|a| a == words::RULE_LANDED))
                .await,
            "{:?} {:?}",
            h.fake.acts(),
            h.autopilot.supervisor().journal()
        );
        assert_eq!(h.mock.requests().len(), 3);
        assert_eq!(typed(&h, 1), format!("{}{}", words::LAND, rider(STALLED)));
        let nudge = typed(&h, 2);
        assert!(
            nudge.starts_with(words::LAND_NUDGE)
                && !nudge.contains(codex_alab_aterm::facts::FACT_OPEN),
            "the narrowed nudge is typed bare: {nudge}"
        );
        assert!(h.fake.acts().iter().any(|a| a == words::RULE_LAND_NUDGE));
        assert_eq!(reads(&h), [words::RULE_LAND, words::RULE_LAND_NUDGE]);
        assert_eq!(h.fake.facts_told(), [first], "the other call stays untold");
        assert!(journal_has(&h, "fact: rule (land-nudge@v1)"));
    });
    big_stack(|| async {
        let resets_at = now_ms() / 1000 - 1;
        let limit = wiremock::ResponseTemplate::new(429).set_body_json(serde_json::json!({
            "error": {
                "type": "usage_limit_reached",
                "message": "limit reached",
                "resets_at": resets_at,
                "plan_type": "pro"
            }
        }));
        let resumed = |said: &str| responses::sse_response(reply("resumed", said));
        let h = harness_timed(
            Mode::Current,
            vec![
                limit,
                resumed("Resumed after the reset."),
                resumed("Carried on with the task."),
            ],
            &[
                ("human_grace_s", "0"),
                ("facts", "true"),
                ("resume_limits", "true"),
                ("limit_wait", "true"),
            ],
            &[],
            Some(0),
            None,
            Timing {
                reset_grace: Duration::ZERO,
                reset_jitter: Duration::ZERO,
                ..Timing::default()
            },
        )
        .await;
        claimed(&h).await;
        let episode = h.fake.plant_fact(STALLED);
        h.test.submit_turn("do the task").await.expect("first turn");
        wait_until(
            "the continuation's short yield backs the next act off",
            || journal_has(&h, "s for continue@v1 (continue@v1 after 1 short turn(s)"),
        )
        .await;
        assert_eq!(h.mock.requests().len(), 3);
        assert_eq!(
            typed(&h, 1),
            words::LIMIT_CLEARED,
            "the resume reads nothing: its cleared line goes bare"
        );
        assert_eq!(typed(&h, 2), format!("keep going{}", rider(STALLED)));
        assert_eq!(reads(&h), [words::RULE_CONTINUE]);
        assert_eq!(h.fake.facts_told(), [episode]);
        let acts = h.fake.acts();
        let at = |act: &str| acts.iter().position(|a| a == act);
        assert!(
            at(words::RULE_LIMIT_RESUME) < at(words::RULE_CONTINUE),
            "{acts:?}"
        );
    });
}

/// A WALL'S RETRY AND A QUESTION'S ANSWER READ NO FACT: a server's 500 is retried
/// (`api-retry@v1`) and a reply that asks a question is answered (`answer@v1`); neither is one of
/// aterm's `FACT_RIDES` rules, so neither reads, though a stalled call stands untold, and both are
/// typed bare. Kills a `FACT_RIDES` that gains either rule (its act would read).
#[test]
fn a_walls_retry_and_a_questions_answer_read_no_fact() {
    big_stack(|| async {
        let h = harness_timed(
            Mode::Current,
            vec![
                wiremock::ResponseTemplate::new(500).set_body_json(serde_json::json!({
                    "error": {"message": "Internal server error."}
                })),
                responses::sse_response(reply("r2", "Should I rename the module?")),
                responses::sse_response(reply("r3", "Renamed the module; stage 2 is next.")),
            ],
            &[
                ("human_grace_s", "0"),
                ("facts", "true"),
                ("continue", "false"),
            ],
            &[],
            Some(0),
            None,
            Timing {
                min_work: Duration::ZERO,
                retry_backoff: vec![Duration::ZERO],
                ..Timing::default()
            },
        )
        .await;
        claimed(&h).await;
        h.fake.plant_fact(STALLED);
        h.test.submit_turn("do the task").await.expect("first turn");
        wait_until("the answer's turn ends", || {
            h.mock.requests().len() == 3 && journal_has(&h, "nothing (continue is off)")
        })
        .await;
        let acts = h.fake.acts();
        assert!(
            acts.iter().any(|a| a == words::RULE_API_RETRY)
                && acts.iter().any(|a| a == words::RULE_ANSWER),
            "{acts:?}"
        );
        assert!(
            h.fake.fact_reads().is_empty(),
            "no read: {:?}",
            h.fake.fact_reads()
        );
        for n in 1..3 {
            let said = typed(&h, n);
            assert!(
                !said.contains(codex_alab_aterm::facts::FACT_OPEN),
                "typed bare: {said}"
            );
        }
        assert!(h.fake.facts_told().is_empty());
    });
}

/// A LONG ACT'S NOTICE KEEPS ITS FACT WITHIN ATERM'S 512 BYTES: aterm refuses an `appnotice` text
/// past 512 bytes whole (the fake as aterm does), and Land's instruction alone passes it, so the
/// act's record in the message log gives up the middle of its typed words to `…` and keeps the
/// act's head, the whole rider and its why and turn: one notice carries the fact, the fact's one
/// Messages entry, while the journal keeps the words whole. Kills the client's old cut at 1024
/// bytes (aterm refused every Land notice whole, and the fact was told with no entry).
#[test]
fn a_long_acts_notice_keeps_its_fact_within_aterms_512_bytes() {
    big_stack(|| async {
        let h = harness_with(
            Mode::Current,
            vec![
                reply("r1", "Stage 1 is in."),
                reply("r2", "Stage 2 is in."),
                reply(
                    "r3",
                    "Nothing left: every change is merged and pushed to origin/main.",
                ),
            ],
            &FACTS_ON,
            &[("autopilot", "land")],
        )
        .await;
        claimed(&h).await;
        let episode = h.fake.plant_fact(STALLED);
        h.test.submit_turn("do the task").await.expect("first turn");
        assert!(
            h.fake
                .wait_for(|f| f.acts().iter().any(|a| a == words::RULE_LANDED))
                .await,
            "{:?} {:?}",
            h.fake.acts(),
            h.autopilot.supervisor().journal()
        );
        assert_eq!(h.mock.requests().len(), 3);
        let ridden = rider(STALLED);
        assert_eq!(typed(&h, 1), format!("{}{ridden}", words::LAND));
        assert_eq!(h.fake.facts_told(), [episode]);
        let notices = h.fake.notices();
        let carrying: Vec<&String> = notices.iter().filter(|n| n.contains(&ridden)).collect();
        assert_eq!(carrying.len(), 1, "{notices:?}");
        let notice = carrying[0];
        assert!(
            notice.len() <= codex_alab_aterm::NOTICE_MAX_BYTES,
            "{} bytes: {notice}",
            notice.len()
        );
        let opening = codex_alab_aterm::wire::single_line(words::LAND, 40);
        assert!(
            notice.starts_with("codex-alab @") && notice.contains(&format!(": typed \"{opening}")),
            "the act's head: {notice}"
        );
        let elided = format!("{}{ridden}\" (", codex_alab_aterm::wire::ELISION);
        let at = notice.find(&elided).unwrap_or_else(|| panic!("{notice}"));
        let why = &notice[at + elided.len()..];
        assert!(
            why.contains("; turn ") && why.ends_with(')') && !why.contains('"'),
            "the why and the turn close it: {notice}"
        );
        assert!(
            journal_has(&h, &format!("typed \"{}{ridden}\"", words::LAND)),
            "the journal keeps the words whole"
        );
    });
}

/// A FACT THAT WOULD NOT FIT IS LEFT OFF, AND UNTOLD: with a 790-unit `continue_text` the rider
/// would pass the act's 800 units, so the act is typed as it is, nothing is acknowledged, and
/// aterm keeps the episode for a later act. Kills M24 (a cut fact, or an ack with no rider).
#[test]
fn a_fact_that_would_not_fit_is_left_off_and_untold() {
    big_stack(|| async {
        let mut text = String::from("keep going");
        while text.len() < 790 {
            text.push_str(" and record each decision");
        }
        text.truncate(790);
        let text = text.trim_end().to_string();
        let h = harness(
            Mode::Current,
            vec![
                reply("r1", "Stage 1 is in; stage 2 is next."),
                reply("r2", "Stage 2 is in and pushed.\nATERM-TASK-DONE"),
            ],
            &[
                ("human_grace_s", "0"),
                ("facts", "true"),
                ("continue_text", &text),
            ],
        )
        .await;
        claimed(&h).await;
        h.fake.plant_fact(STALLED);
        h.test.submit_turn("do the task").await.expect("first turn");
        settled(&h, 2).await;
        assert_eq!(typed(&h, 1), text, "typed as it is, with nothing cut");
        assert_eq!(reads(&h), [words::RULE_CONTINUE]);
        assert!(h.fake.facts_told().is_empty());
        assert!(line_at(&h, "harness fact told").is_none(), "no ack");
        assert!(journal_has(
            &h,
            "would pass 800 units: continue@v1 typed without it"
        ));
    });
}

/// A FACT GONE STALE BEFORE THE START IS LEFT OFF: aterm's reading was fresh when aterm answered
/// (the fake serves a pinned reading as is), but at the last step before the turn request it is
/// more than `FACT_FRESH_MS` old on this host's clock, so the act goes bare and nothing is
/// acknowledged. Kills M40 (no last-step freshness check).
#[test]
fn a_fact_gone_stale_before_the_start_is_left_off() {
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![
                reply("r1", "Stage 1 is in; stage 2 is next."),
                reply("r2", "Stage 2 is in and pushed.\nATERM-TASK-DONE"),
            ],
            &FACTS_ON,
        )
        .await;
        claimed(&h).await;
        let six_s_ago = i64::try_from(now_ms()).unwrap_or(i64::MAX) - 6_000;
        h.fake.plant_fact_as_of(STALLED, six_s_ago);
        h.test.submit_turn("do the task").await.expect("first turn");
        settled(&h, 2).await;
        assert_eq!(typed(&h, 1), "keep going");
        assert_eq!(reads(&h), [words::RULE_CONTINUE]);
        assert!(h.fake.facts_told().is_empty());
        assert!(journal_has(
            &h,
            "gone stale before the start: continue@v1 typed without it"
        ));
    });
}

/// FACTS OFF READS NOTHING: aterm's `[harness] facts` is a limit; with it off (explicitly, and
/// absent, as an aterm before the key sends the policy) codex-alab asks aterm for no fact.
/// Kills M25 (the limit ignored).
#[test]
fn facts_off_reads_nothing() {
    for policy in [
        &[("human_grace_s", "0"), ("facts", "false")][..],
        &[("human_grace_s", "0")][..],
    ] {
        big_stack(move || async move {
            let h = harness(
                Mode::Current,
                vec![
                    reply("r1", "Stage 1 is in; stage 2 is next."),
                    reply("r2", "Stage 2 is in and pushed.\nATERM-TASK-DONE"),
                ],
                policy,
            )
            .await;
            claimed(&h).await;
            h.fake.plant_fact(STALLED);
            h.test.submit_turn("do the task").await.expect("first turn");
            settled(&h, 2).await;
            assert_eq!(typed(&h, 1), "keep going");
            assert!(h.fake.fact_reads().is_empty(), "{:?}", h.fake.fact_reads());
        });
    }
}

/// AN ATERM BEFORE THE VERB IS ASKED ONCE PER CLAIM: its `harness` usage names no `fact` form,
/// so the first read is the claim's last, the act is typed as it is today, and the next act
/// reads nothing. (A new claim asks once again: `passive_tests`' `fact_read_tests`.) Kills M26
/// (asking at every act).
#[test]
fn an_aterm_before_the_verb_is_asked_once_per_claim() {
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![
                reply("r1", "Stage 1 is in; stage 2 is next."),
                reply("r2", "Stage 2 is in; stage 3 is next."),
                reply("r3", "Stage 3 is in and pushed.\nATERM-TASK-DONE"),
            ],
            &FACTS_ON,
        )
        .await;
        claimed(&h).await;
        h.fake.support_harness_fact(false);
        h.fake.plant_fact(STALLED);
        h.test.submit_turn("do the task").await.expect("first turn");
        settled(&h, 3).await;
        assert_eq!(typed(&h, 1), "keep going");
        assert_eq!(typed(&h, 2), "keep going");
        assert_eq!(reads(&h), [words::RULE_CONTINUE], "asked once");
        assert!(journal_has(&h, "has no harness fact"));
        assert!(journal_has(&h, "not asked again under this claim"));
    });
}

/// A BUSY OR FAILED READ TYPES THE ACT WITHOUT ITS FACT, AND ASKS AGAIN: aterm's `why=busy` (its
/// main thread did not answer in time) and a refusal (`ERR aterm is showing a dialog`) are no
/// fact this time only; the third continuation reads and carries the fact. Kills M38 (a latch on
/// any `ERR`).
#[test]
fn a_busy_or_failed_read_types_the_act_without_its_fact_and_asks_again() {
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![
                reply("r1", "Wrote the parser for the new config section."),
                reply("r2", "Added tests for the parser's error paths."),
                reply("r3", "Fixed the lint those tests turned up."),
                reply("r4", "Pushed it all to main.\nATERM-TASK-DONE"),
            ],
            &FACTS_ON,
        )
        .await;
        claimed(&h).await;
        h.fake.fact_answer_once("OK fact=- why=busy");
        h.fake.fact_answer_once("ERR aterm is showing a dialog");
        let episode = h.fake.plant_fact(STALLED);
        h.test.submit_turn("do the task").await.expect("first turn");
        settled(&h, 4).await;
        assert_eq!(typed(&h, 1), "keep going");
        assert_eq!(typed(&h, 2), "keep going");
        assert_eq!(typed(&h, 3), format!("keep going{}", rider(STALLED)));
        assert_eq!(reads(&h).len(), 3);
        assert_eq!(h.fake.facts_told(), [episode]);
        assert!(journal_has(&h, "fact: busy (continue@v1)"));
        assert!(journal_has(
            &h,
            "fact: ERR aterm is showing a dialog (continue@v1)"
        ));
    });
}

/// AN UNANSWERED FACT READ IS PASSIVE (fail closed, CONTRACT §5): a read is a call like any
/// other, so at its deadline the claim's connection closes, the claim ends with it, and the act
/// whose read it was is not typed; nothing is acknowledged. Kills M28 (typing after the claim
/// ended).
#[test]
fn an_unanswered_fact_read_is_passive() {
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![reply("r1", "Stage 1 is in; stage 2 is next.")],
            &FACTS_ON,
        )
        .await;
        claimed(&h).await;
        h.fake.plant_fact(STALLED);
        h.fake.stall_fact_reply(true);
        h.test.submit_turn("do the task").await.expect("first turn");
        // The read's deadline closes the claim's connection; codex-alab claims again on a new
        // one and decides the owed act afresh, whose read stalls too. By that second read the
        // first act's evaluation has ended: had it typed after its claim ended, the model would
        // have its request (and the thread its turn) by now.
        let received = || async {
            responses::received_responses_requests(&h._server)
                .await
                .len()
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        while h.fake.fact_reads().len() < 2 && received().await == 1 {
            assert!(
                tokio::time::Instant::now() < deadline,
                "never happened: the act decided again under a new claim"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(received().await, 1, "nothing typed after the claim ended");
        assert!(journal_has(&h, "fact: "), "the read's failure is journaled");
        assert!(!journal_has(&h, "typed \"keep going"));
        assert!(!h.fake.acts().iter().any(|a| a == words::RULE_CONTINUE));
        assert!(h.fake.facts_told().is_empty());
        assert!(line_at(&h, "harness fact told").is_none());
    });
}

/// A RETYPED ACT CARRIES NO FACT, TOLD ONCE: the done check carries the fact; its turn meets a
/// full context, codex-alab compacts (`context-compact@v1`), and the compaction's continuation
/// types the act the full context cut again, verbatim, from the thread's `cut`, which keeps the
/// decision's words without the rider: bare, and with no read (`context-compact@v1` is no
/// `FACT_RIDES` rule). Kills M30 (the ridden text kept in `ThreadState::cut`).
#[test]
fn a_retyped_act_carries_no_fact_told_once() {
    big_stack(|| async {
        let h = harness_configured(
            Mode::Current,
            [
                reply("r1", "All of it is in.\nATERM-TASK-DONE"),
                responses::sse_failed(
                    "context-full",
                    "context_length_exceeded",
                    "The context window is full.",
                ),
                reply("compacted", "A summary of the work so far."),
                reply("r4", "Nothing left: it is all merged and pushed."),
            ]
            .into_iter()
            .map(responses::sse_response)
            .collect(),
            &FACTS_ON,
            &[],
            None,
            None,
            Timing {
                min_work: Duration::ZERO,
                ..Timing::default()
            },
            |config| {
                // Core's own compaction: the model's reply is the summary.
                config.model_provider.capabilities = Some(
                    serde_json::from_value(serde_json::json!({"remote_compaction": "unsupported"}))
                        .unwrap_or_else(|err| panic!("compaction capability: {err:?}")),
                );
            },
        )
        .await;
        claimed(&h).await;
        let episode = h.fake.plant_fact(STALLED);
        h.test.submit_turn("do the task").await.expect("first turn");
        wait_until("the compaction's continuation is typed", || {
            h.mock.requests().len() >= 4
                && h.fake
                    .acts()
                    .iter()
                    .filter(|a| *a == words::RULE_COMPACT)
                    .count()
                    == 2
        })
        .await;
        let check = typed(&h, 1);
        assert!(
            check.starts_with(words::DONE_CHECK) && check.ends_with(&rider(STALLED)),
            "{check}"
        );
        let bare = check
            .strip_suffix(&rider(STALLED))
            .expect("the rider ends the check");
        assert_eq!(typed(&h, 3), bare, "the cut words, typed again bare");
        assert_eq!(reads(&h)[0], words::RULE_DONE_CHECK);
        assert!(
            !reads(&h).iter().any(|rule| rule == words::RULE_COMPACT),
            "{:?}",
            reads(&h)
        );
        assert_eq!(h.fake.facts_told(), [episode]);
    });
}

/// UNDER A CYBERSECURITY REFUSAL'S EXCLUSION NO FACT IS READ (the owner's standing rule: every
/// automatic prompt there keeps the CONTRACT's `POLICY_BOUNDARY` suffix, with nothing after it):
/// the policy continuation (no `FACT_RIDES` rule) and the continuation after its independent work
/// both go bare, and aterm is asked for nothing. Kills the mutant that drops the exclusion (the
/// continuation would end on the rider, after the boundary).
#[test]
fn a_session_under_a_policy_refusal_reads_no_fact() {
    big_stack(|| async {
        let refusal = responses::sse_response(responses::sse_failed(
            "refused",
            "cyber_policy",
            "This action was refused by the cybersecurity access policy.",
        ));
        let h = harness_timed(
            Mode::Current,
            vec![
                refusal,
                responses::sse_response(reply(
                    "independent",
                    "Reviewed independently authorized tests.",
                )),
                responses::sse_response(reply(
                    "more",
                    "Reviewed more independently authorized tests.\nATERM-TASK-DONE",
                )),
            ],
            &FACTS_ON,
            &[],
            None,
            None,
            Timing {
                min_work: Duration::ZERO,
                ..Timing::default()
            },
        )
        .await;
        claimed(&h).await;
        h.fake.plant_fact(STALLED);
        h.test
            .submit_turn("the initial request")
            .await
            .expect("first turn");
        settled(&h, 3).await;
        assert_eq!(typed(&h, 1), words::POLICY_CONTINUE);
        let next = typed(&h, 2);
        assert!(next.ends_with(words::POLICY_BOUNDARY), "{next}");
        assert!(h.fake.fact_reads().is_empty(), "{:?}", h.fake.fact_reads());
    });
}

/// A THREAD WITHOUT A BACKGROUND TERMINAL BESIDE ANOTHER ROOT THREAD READS NO FACT: aterm's jobs
/// board keys calls by tab, so with a second root thread live in the process only a thread that
/// runs a background terminal may carry the tab's fact. Without one, the continuation goes bare
/// and aterm is asked nothing; with one (the person's turn started `sleep` in the background),
/// the continuation reads and carries it. Alone, a thread with none reads
/// ([`a_stalled_call_rides_one_continuation_and_not_the_next`]). Kills M41 (the thread filter
/// missing, or applied to a lone thread).
#[test]
fn a_thread_without_a_background_terminal_beside_another_reads_no_fact() {
    big_stack(|| async {
        let h = harness(
            Mode::Current,
            vec![
                reply("r1", "Stage 1 is in; stage 2 is next."),
                reply("r2", "Stage 2 is in and pushed.\nATERM-TASK-DONE"),
            ],
            &FACTS_ON,
        )
        .await;
        claimed(&h).await;
        let _side = side_thread(&h).await;
        h.fake.plant_fact(STALLED);
        h.test.submit_turn("do the task").await.expect("first turn");
        settled(&h, 2).await;
        assert_eq!(typed(&h, 1), "keep going");
        assert!(h.fake.fact_reads().is_empty(), "{:?}", h.fake.fact_reads());
        assert!(journal_has(
            &h,
            "fact: no background terminal beside another root thread (continue@v1)"
        ));
    });
    big_stack(|| async {
        let start = serde_json::json!({"cmd": "sleep 60", "yield_time_ms": 250});
        let h = harness_full(
            Mode::Current,
            vec![
                responses::sse_response(responses::sse(vec![
                    responses::ev_response_created("call"),
                    responses::ev_function_call("bg-1", "exec_command", &start.to_string()),
                    responses::ev_completed("call"),
                ])),
                responses::sse_response(reply("r1", "The build runs in the background.")),
                responses::sse_response(reply("r2", "The build is in.\nATERM-TASK-DONE")),
            ],
            &FACTS_ON,
            &[],
            None,
        )
        .await;
        claimed(&h).await;
        let _side = side_thread(&h).await;
        let episode = h.fake.plant_fact(STALLED);
        h.test.submit_turn("do the task").await.expect("first turn");
        settled(&h, 3).await;
        assert!(
            h.test.codex.list_background_terminals().await.len() == 1,
            "the person's turn left its command running"
        );
        assert_eq!(typed(&h, 2), format!("keep going{}", rider(STALLED)));
        assert_eq!(reads(&h), [words::RULE_CONTINUE]);
        assert_eq!(h.fake.facts_told(), [episode]);
    });
}
