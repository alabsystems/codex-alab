// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A partial reply at a terminal output limit is a wall, not successful work: aterm's
//! Overlong wall, retried at once in smaller pieces.

use super::*;

/// A REPLY PAST ITS OUTPUT TOKEN MAXIMUM end to end: core ends the turn on the incomplete
/// response (it never retries one), and codex-alab types `api-cutoff@v1` at once in aterm's
/// words (`wall_retry_text`'s Overlong case), with the standing rules aterm composed its
/// texts with, as aterm's wall acts carry them; the retry's reply is the next turn. Nothing is
/// left to a person. NEGATIVE CONTROL: the partial reply's own words are never read as the
/// turn's report (no continuation answers them: `continue` is off, and only the wall act is
/// typed).
#[test]
fn a_terminal_incomplete_response_is_retried_at_once_in_smaller_pieces() {
    big_stack(|| async {
        let h = harness_full(
            Mode::Current,
            vec![
                responses::sse_response(responses::sse(vec![
                    responses::ev_response_created("incomplete"),
                    responses::ev_assistant_message("partial", "The change is ready."),
                    serde_json::json!({
                        "type": "response.incomplete",
                        "response": {
                            "id": "incomplete",
                            "incomplete_details": {"reason": "max_output_tokens"}
                        }
                    }),
                ])),
                responses::sse_response(reply("smaller", "Wrote the first part of the file.")),
            ],
            &[
                ("human_grace_s", "0"),
                ("continue", "false"),
                (
                    "continue_text",
                    "keep going (standing rules: Never push to upstream.)",
                ),
            ],
            &[],
            None,
        )
        .await;
        assert!(
            h.fake
                .wait_for(|f| f.claim().is_some_and(|c| c.proven_pid.is_some()))
                .await
        );
        h.test
            .submit_turn("write the long file")
            .await
            .expect("first turn");
        wait_until("the overlong reply's retry is typed and answered", || {
            h.mock.requests().len() == 2
        })
        .await;
        wait_for_event_with_timeout(
            &h.test.codex,
            |event| {
                matches!(event, EventMsg::TurnComplete(done)
                    if done.last_agent_message.as_deref() == Some("Wrote the first part of the file."))
            },
            Duration::from_secs(60),
        )
        .await;
        let typed = h.mock.requests()[1].message_input_texts("user");
        let text = typed.last().expect("a recovery prompt");
        assert!(text.contains("across several smaller replies"));
        assert!(text.ends_with("(standing rules: Never push to upstream.)"));
        assert!(!text.contains("NEEDS YOU"));
        assert_eq!(h.fake.acts(), ["api-cutoff@v1"]);
        assert!(h.fake.attention().is_empty(), "{:?}", h.fake.attention());
        h.test
            .codex
            .shutdown_and_wait()
            .await
            .unwrap_or_else(|err| panic!("shutdown acknowledgement: {err:?}"));
        assert_eq!(h.mock.requests().len(), 2, "only the wall act was typed");
    });
}
