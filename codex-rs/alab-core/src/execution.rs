// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Execution-first policy for optional upstream consent and input paths.

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
