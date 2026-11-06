// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! What codex-alab tells aterm (`@self story chose [<text>]`): a question box it answered, as
//! aterm's own approval loop tells one (`story chose <policy word>`, only for a dialog's
//! answer). aterm counts it as one story point on the tab's presence band and writes one story
//! mark to the session log (`stories N` in the brief); the text is checked, not kept.
//!
//! aterm's verbs are a closed set of one since its owner surfaces (aterm main `189ec7bc0d`):
//! `chose`. The seven others (`approved dismissed reconnected timeout exit compacted warned`)
//! are refused, so only a dialog's answer is told:
//!
//! | codex-alab | story |
//! | --- | --- |
//! | a question box answered | `chose <policy word>` |
//!
//! Every other act (an approval, the plan box's yes, a prose question answered in the
//! composer, a continuation, the owner's standing go-ahead, Land's acts, a wall's try, a
//! compaction, a re-exec), a wait, a point left to a person and the worker's NEEDS YOU tell
//! nothing here: each is a `harness_act` timeline row, an `appnotice` or keyed attention
//! already, as aterm's own loop tells none of them.

use crate::answer;

/// The story an act recorded under `rule` tells: `(verb, text)`, or `None`.
pub fn for_act(rule: &str) -> Option<(&'static str, &'static str)> {
    Some(match rule {
        answer::RULE_ANSWER_RECOMMENDED => ("chose", "recommended"),
        answer::RULE_ANSWER_LONG_TERM => ("chose", "long-term"),
        answer::RULE_ANSWER_SHORT_TERM => ("chose", "short-term"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::RULE_APPROVE;
    use crate::upgrade::RULE_UPGRADE;
    use crate::words;
    use pretty_assertions::assert_eq;

    #[test]
    fn only_a_dialogs_answer_tells_a_story_and_its_verb_is_one_aterm_takes() {
        for rule in [
            answer::RULE_ANSWER_RECOMMENDED,
            answer::RULE_ANSWER_LONG_TERM,
            answer::RULE_ANSWER_SHORT_TERM,
        ] {
            let (verb, text) = for_act(rule).unwrap_or_else(|| panic!("{rule} tells nothing"));
            assert!(
                codex_alab_aterm::STORY_VERBS.contains(&verb),
                "{rule}: {verb}"
            );
            assert!(text.len() <= codex_alab_aterm::STORY_MAX_BYTES, "{rule}");
        }
        // NEGATIVE CONTROL: aterm's own loop tells none of these (a prose answer included).
        for rule in [
            words::RULE_ANSWER,
            words::RULE_PROSE_LONG_TERM,
            words::RULE_PROSE_SHORT_TERM,
            RULE_APPROVE,
            answer::RULE_PLAN,
            answer::RULE_GOAL_LEAVE,
            words::RULE_CONTINUE,
            words::RULE_POLICY_CONTINUE,
            words::RULE_DONE_CHECK,
            words::RULE_LAND,
            words::RULE_LAND_NUDGE,
            words::RULE_LAND_ANSWER,
            words::RULE_API_RETRY,
            words::RULE_API_BACK,
            words::RULE_API_CUTOFF,
            words::RULE_LIMIT_RESUME,
            RULE_UPGRADE,
            words::RULE_COMPACT,
            words::RULE_LANDED,
            words::RULE_LEFT,
            words::RULE_FINISHED,
            words::RULE_HOST_TURN,
            words::RULE_LOGIN,
            words::RULE_LOGIN_BACK,
            crate::go_ahead::RULE_GO_AHEAD,
        ] {
            assert_eq!(for_act(rule), None, "{rule}");
        }
    }

    #[test]
    fn an_answer_is_told_by_its_policy_word_as_aterms_loop_tells_it() {
        assert_eq!(
            for_act(answer::RULE_ANSWER_RECOMMENDED),
            Some(("chose", "recommended"))
        );
        assert_eq!(
            for_act(answer::RULE_ANSWER_LONG_TERM),
            Some(("chose", "long-term"))
        );
    }
}
