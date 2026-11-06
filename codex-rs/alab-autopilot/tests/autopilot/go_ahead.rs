// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE OWNER'S STANDING GO-AHEAD END TO END (owner, 2026-10-29: "how dare you block on me for go
//! ahead! fix the cruise harness so that this cannot happen again"; review of the port, finding
//! 14): a real core thread parks on the incident's line, codex-alab types `go-ahead@v1`
//! (`meta set harness_act go-ahead@v1`), and the line it answers never goes on the menu bar
//! (aterm's DESIGN-harness-autopilot §15.6: no `owner=needs` entry, no `harness_needs` set).
//! Where the go-ahead is not typed the line goes up as any need does: with answers off, within
//! the person's hour (`status human_typed_ms=`), and once its bound is spent, where the refusal
//! is told now (`owner=codex-alab`).

use codex_alab_autopilot::go_ahead;

use super::*;

/// The incident's park (2026-10-29), as its agent wrote it.
const INCIDENT: &str = "Nothing has changed. The build is waiting only for your go-ahead.\n\nNEEDS \
                        YOU: say \"rerun the checks\" so I can build and test the upgrade fix \
                        branch, push it to main and remove the worktree.";

/// [`INCIDENT`]'s line, as the menu bar would show it.
const LINE: &str = "say \"rerun the checks\" so I can build and test the upgrade fix branch, push \
                    it to main and remove the worktree.";

/// Whether the fake was ever told a need, by its `harness_needs` set or its `owner=needs`
/// entry.
fn needs_ever_told(h: &Harness) -> bool {
    h.fake.requests().iter().any(|request| {
        request.contains(" meta set harness_needs ")
            || (request.contains(" meta set attention ") && request.contains("owner=needs"))
    })
}

/// The session's first turn, the person's, ends on [`INCIDENT`].
async fn parked(policy: &[(&str, &str)], status: &[(&str, &str)], replies: Vec<String>) -> Harness {
    let mut all = vec![("human_grace_s", "0")];
    all.extend_from_slice(policy);
    let h = harness_with(Mode::Current, replies, &all, status).await;
    assert!(
        h.fake
            .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
            .await
    );
    h.test
        .submit_turn("fix the upgrade and land it")
        .await
        .unwrap_or_else(|err| panic!("the person's turn: {err:?}"));
    h
}

/// THE INCIDENT'S PARK GETS THE GO-AHEAD, AND ITS LINE NEVER GOES UP: the act is typed with the
/// word the line asked for, recorded as `go-ahead@v1`, and no need is ever told (the turn's end
/// foresees the go-ahead before it surfaces the needs, and the decision keeps the line off).
#[test]
fn the_incidents_park_gets_the_go_ahead_and_its_line_never_goes_up() {
    big_stack(|| async {
        let h = parked(
            &[("continue_per_hour", "1")],
            &[],
            vec![
                reply("r1", INCIDENT),
                reply("r2", "Rebuilt, tested and pushed my branch to main."),
            ],
        )
        .await;
        wait_until("the go-ahead's yield waits for the cap", || {
            h.mock.requests().len() == 2 && journal_has(&h, "continue_per_hour=1 reached")
        })
        .await;
        let typed = h.mock.requests()[1].message_input_texts("user");
        let typed = typed.last().map(String::as_str).unwrap_or_default();
        assert!(typed.starts_with(go_ahead::GO_AHEAD_ANSWER), "{typed}");
        assert!(typed.contains("rerun the checks"), "{typed}");
        assert!(
            h.fake
                .acts()
                .iter()
                .any(|act| act == go_ahead::RULE_GO_AHEAD),
            "{:?}",
            h.fake.acts()
        );
        assert!(!needs_ever_told(&h), "{:?}", h.fake.requests());
        assert!(!h.fake.attention().contains_key("needs"));
    });
}

/// WHERE THE GO-AHEAD IS NOT TYPED, THE LINE GOES UP: with answers off (codex-alab tells the
/// question), and within the person's hour (their key 5 s ago: Cruise's continuation goes over a
/// park of a NEEDS YOU line alone, without its sentence that the go-ahead is no need). NEGATIVE
/// CONTROL of each: no `go-ahead@v1` is recorded.
#[test]
fn the_line_goes_up_where_the_go_ahead_is_not_typed() {
    big_stack(|| async {
        let alone =
            "say \"rerun the checks\" so I can build and test my branch and push it to main.";
        // The fake percent-encodes what it sends.
        let continuation = format!("Keep going. {}", go_ahead::NEVER_A_NEED);
        // Answers off type nothing; within the hour the continuation goes once
        // (`continue_per_hour=1`).
        for (what, policy, status, park, line, turns) in [
            (
                "answers off",
                vec![("answer_questions", "false")],
                vec![],
                INCIDENT.to_string(),
                LINE,
                1,
            ),
            (
                "the person's hour",
                vec![
                    ("continue_per_hour", "1"),
                    ("continue_text", continuation.as_str()),
                ],
                vec![("human_typed_ms", "5000")],
                format!("NEEDS YOU: {alone}"),
                alone,
                2,
            ),
        ] {
            let replies = [
                reply("r1", &park),
                reply("r2", "Rebuilt, tested and pushed my branch to main."),
            ];
            let h = parked(&policy, &status, replies[..turns].to_vec()).await;
            wait_until("the turns ran", || h.mock.requests().len() == turns).await;
            assert!(
                h.fake
                    .wait_for(|f| f.needs_told() == Some(vec![line.to_string()]))
                    .await,
                "{what}: {:?}",
                h.fake.requests()
            );
            assert!(
                h.fake.attention().contains_key("needs"),
                "{what}: {:?}",
                h.fake.attention()
            );
            assert!(
                !h.fake
                    .acts()
                    .iter()
                    .any(|act| act == go_ahead::RULE_GO_AHEAD),
                "{what}: {:?}",
                h.fake.acts()
            );
            if turns == 2 {
                let typed = h.mock.requests()[1].message_input_texts("user");
                let typed = typed.last().map(String::as_str).unwrap_or_default();
                assert!(typed.starts_with("Keep going."), "{what}: {typed}");
                assert!(
                    !typed.contains(go_ahead::NEVER_A_NEED),
                    "{what}: the continuation, cut: {typed}"
                );
            }
        }
    });
}

/// THE BOUND: three go-aheads on the standing park, then the fourth park types nothing, the
/// refusal is told now under codex-alab's own entry, and the line goes back on the menu bar.
/// NEGATIVE CONTROL: up to the third, no need was told.
#[test]
fn three_go_aheads_then_the_refusal_is_told_and_the_line_goes_up() {
    big_stack(|| async {
        let parks = [
            "NEEDS YOU: say \"rerun the checks\" so I can rebuild my branch.",
            "NEEDS YOU: say \"go\" so I can rebuild my branch and push it to main.",
            "The rebuild of my branch is waiting for your go-ahead.",
        ];
        let h = parked(
            &[],
            &[],
            vec![
                reply("r1", parks[0]),
                reply("r2", parks[1]),
                reply("r3", parks[2]),
                reply("r4", parks[0]),
            ],
        )
        .await;
        wait_until("three go-aheads", || {
            h.fake
                .acts()
                .iter()
                .filter(|act| *act == go_ahead::RULE_GO_AHEAD)
                .count()
                == 3
        })
        .await;
        assert!(
            h.fake
                .wait_for(|f| f
                    .attention()
                    .get("codex-alab")
                    .is_some_and(|told| told.contains(go_ahead::REFUSES_GO_AHEAD)))
                .await,
            "{:?}",
            h.fake.attention()
        );
        assert!(
            h.fake
                .wait_for(|f| f.needs_told()
                    == Some(vec![
                        "say \"rerun the checks\" so I can rebuild my branch.".to_string()
                    ]))
                .await,
            "{:?}",
            h.fake.requests()
        );
        assert_eq!(
            h.mock.requests().len(),
            4,
            "nothing typed after the fourth park"
        );
        // NEGATIVE CONTROL: no need was told before the third go-ahead was recorded.
        let requests = h.fake.requests();
        let third = requests
            .iter()
            .enumerate()
            .filter(|(_, r)| r.ends_with(" meta set harness_act go-ahead@v1"))
            .nth(2)
            .map(|(at, _)| at)
            .expect("the third go-ahead's record");
        assert!(
            requests[..third]
                .iter()
                .all(|r| !r.contains(" meta set harness_needs ")),
            "{requests:?}"
        );
        assert_eq!(
            h.fake
                .acts()
                .iter()
                .filter(|act| *act == go_ahead::RULE_GO_AHEAD)
                .count(),
            3
        );
    });
}
