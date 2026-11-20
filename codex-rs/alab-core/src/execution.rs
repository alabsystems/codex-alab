// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Execution policy for consent, input and tool-result recovery.

use crate::active;
use crate::inside_aterm;

/// Run enabled project hooks without another trust decision inside aterm, where the owner has
/// already granted full execution authority. An explicit CLI choice still wins, and disabled
/// hooks remain disabled.
pub fn bypass_hook_trust() -> bool {
    bypass_hook_trust_when(active(), inside_aterm())
}

fn bypass_hook_trust_when(active: bool, inside_aterm: bool) -> bool {
    active && inside_aterm
}

/// Whether full-access MCP servers may surface arbitrary forms to a person. Confirmation-only
/// elicitations already auto-accept; enhanced mode declines the remaining forms immediately so
/// the model can continue without invented answers.
pub fn full_access_form_input() -> bool {
    full_access_form_input_when(active())
}

fn full_access_form_input_when(active: bool) -> bool {
    !active
}

/// Model-visible substitute when a history contains a tool call without its recorded output.
pub fn missing_tool_result_text(upstream: &'static str) -> &'static str {
    if active() {
        "No result was recorded in this history. Check the current state before repeating this action."
    } else {
        upstream
    }
}

/// Model-visible note when an ephemeral sample was taken from a running source thread.
pub fn pending_sample_note(upstream: &'static str) -> &'static str {
    if active() {
        concat!(
            "\nThis snapshot was taken while the source turn was running. ",
            "A tool call that was pending at capture may still finish in the source thread.\n",
        )
    } else {
        upstream
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hook_trust_bypass_requires_the_native_terminal_and_enhanced_mode() {
        assert!(super::bypass_hook_trust_when(true, true));
        for (active, inside) in [(false, true), (true, false), (false, false)] {
            assert!(!super::bypass_hook_trust_when(active, inside));
        }
    }

    #[test]
    fn only_enhanced_mode_declines_unanswerable_forms() {
        assert!(super::full_access_form_input_when(false));
        assert!(!super::full_access_form_input_when(true));
    }
}
