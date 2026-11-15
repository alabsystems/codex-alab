// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Keep startup and foreground work moving without optional human input.

use codex_alab_core::active;

/// Accept a mandatory move off a retired model. The old model cannot execute another turn, so
/// the existing migration acceptance path records the selection and startup continues.
pub fn accept_forced_model_migration() -> bool {
    accept_forced_model_migration_when(active())
}

fn accept_forced_model_migration_when(active: bool) -> bool {
    active
}

/// Whether the first user message starts a hidden thread and model request to invent a title.
/// Explicit rename suggestions retain upstream behavior.
pub fn automatic_thread_title_model_calls() -> bool {
    automatic_thread_title_model_calls_when(active())
}

fn automatic_thread_title_model_calls_when(active: bool) -> bool {
    !active
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_enhanced_mode_accepts_required_migrations() {
        assert!(super::accept_forced_model_migration_when(true));
        assert!(!super::accept_forced_model_migration_when(false));
    }

    #[test]
    fn only_enhanced_mode_skips_automatic_title_model_calls() {
        assert!(super::automatic_thread_title_model_calls_when(false));
        assert!(!super::automatic_thread_title_model_calls_when(true));
    }
}
