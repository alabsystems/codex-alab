// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! What codex-alab tells aterm's presence band (`@self story <verb> [<text>]`), so its own acts
//! show there as `aterm drive watch`'s decisions do (aterm `supervise/run.rs` `story_of`): a
//! three-second flash in the phase slot, the tab's story dot until the person looks, and the
//! approvals and choices counted in the band's `◇ quiet` summary. Without it a self-supervised
//! codex looked idle on the band while it approved and answered (`harness_act` records a
//! timeline row only).
//!
//! aterm's verbs are a closed set (`approved dismissed reconnected timeout exit compacted
//! warned chose`), so each act maps onto the nearest one:
//!
//! | codex-alab | story |
//! | --- | --- |
//! | an approval (`approve=all`) | `approved` (bare: never the command) |
//! | the plan box's yes | `approved plan` |
//! | a question answered (overlay or prose) | `chose <policy word>` |
//! | a continuation, the done check | `chose continue`, `chose done-check`: the harness chose by policy |
//! | a retry after an API error or a cut-off reply, the resume at a usage reset, a re-exec onto a new build | `reconnected <what>`: an outage ridden out |
//! | a compaction | `compacted` |
//! | a wait for a usage reset or a retry | `warned <the wait's words>` |
//! | a point left to a person (attention) | `warned needs you: <why>` |
//!
//! The finished task, a host-typed turn and the login wall tell nothing of their own: the
//! last two raise attention, which is told.

use crate::answer;
use crate::extension::RULE_APPROVE;
use crate::upgrade::RULE_UPGRADE;
use crate::words;

/// The story an act recorded under `rule` tells: `(verb, text)`, or `None`.
pub fn for_act(rule: &str) -> Option<(&'static str, &'static str)> {
    Some(match rule {
        RULE_APPROVE => ("approved", ""),
        answer::RULE_PLAN => ("approved", "plan"),
        answer::RULE_ANSWER_RECOMMENDED => ("chose", "recommended"),
        answer::RULE_ANSWER_LONG_TERM | words::RULE_PROSE_LONG_TERM => ("chose", "long-term"),
        answer::RULE_ANSWER_SHORT_TERM | words::RULE_PROSE_SHORT_TERM => ("chose", "short-term"),
        words::RULE_ANSWER => ("chose", "answered"),
        words::RULE_CONTINUE => ("chose", "continue"),
        words::RULE_DONE_CHECK => ("chose", "done-check"),
        words::RULE_API_RETRY => ("reconnected", "api retry"),
        words::RULE_API_CUTOFF => ("reconnected", "cut-off reply"),
        words::RULE_LIMIT_RESUME => ("reconnected", "usage reset"),
        RULE_UPGRADE => ("reconnected", "new build"),
        words::RULE_COMPACT => ("compacted", ""),
        _ => return None,
    })
}

/// The story a shown wait tells: `warned`, with the wait's own words.
pub fn for_wait(words: &str) -> (&'static str, String) {
    ("warned", words.to_string())
}

/// The story a point left to a person tells (`kind: why`, as attention shows it).
pub fn for_attention(text: &str) -> (&'static str, String) {
    ("warned", format!("needs you: {text}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn every_act_tells_a_verb_aterm_takes() {
        for rule in [
            RULE_APPROVE,
            answer::RULE_PLAN,
            answer::RULE_ANSWER_RECOMMENDED,
            answer::RULE_ANSWER_LONG_TERM,
            answer::RULE_ANSWER_SHORT_TERM,
            words::RULE_ANSWER,
            words::RULE_PROSE_LONG_TERM,
            words::RULE_PROSE_SHORT_TERM,
            words::RULE_CONTINUE,
            words::RULE_DONE_CHECK,
            words::RULE_API_RETRY,
            words::RULE_API_CUTOFF,
            words::RULE_LIMIT_RESUME,
            RULE_UPGRADE,
            words::RULE_COMPACT,
        ] {
            let (verb, text) = for_act(rule).unwrap_or_else(|| panic!("{rule} tells nothing"));
            assert!(
                codex_alab_aterm::STORY_VERBS.contains(&verb),
                "{rule}: {verb}"
            );
            assert!(text.len() <= codex_alab_aterm::STORY_MAX_BYTES, "{rule}");
        }
        for rule in [
            words::RULE_FINISHED,
            words::RULE_HOST_TURN,
            words::RULE_LOGIN,
        ] {
            assert_eq!(for_act(rule), None, "{rule}");
        }
        assert!(codex_alab_aterm::STORY_VERBS.contains(&for_wait("x").0));
        assert!(codex_alab_aterm::STORY_VERBS.contains(&for_attention("x").0));
    }

    #[test]
    fn an_approval_is_told_bare_as_drive_watch_tells_it() {
        assert_eq!(for_act(RULE_APPROVE), Some(("approved", "")));
        assert_eq!(
            for_act(answer::RULE_ANSWER_RECOMMENDED),
            Some(("chose", "recommended"))
        );
        assert_eq!(
            for_attention("question: a destructive option"),
            (
                "warned",
                "needs you: question: a destructive option".to_string()
            )
        );
    }
}
