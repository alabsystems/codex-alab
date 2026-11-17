// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Original parsed CLI intent only. Existing UI lookup/picker policy chooses
//! the actual target; this never turns an unresolved request into fresh START.
use super::Cli;
use codex_app_server_client::InitialSessionKind;

pub(super) fn kind(cli: &Cli) -> InitialSessionKind {
    if cli.agents_overview {
        InitialSessionKind::Selection
    } else if cli.fork_picker || cli.fork_last || cli.fork_session_id.is_some() {
        if cli.fork_session_id.is_some() {
            InitialSessionKind::Fork
        } else {
            // Original --last may return StartFresh when no eligible session
            // exists. Picker may choose any original action. Freeze only when
            // that real UI action sends its claim-bearing full request.
            InitialSessionKind::Selection
        }
    } else if cli.resume_session_id.is_some() {
        InitialSessionKind::Resume
    } else if cli.resume_last || cli.resume_picker {
        InitialSessionKind::Selection
    } else {
        InitialSessionKind::Start
    }
}

#[cfg(test)]
#[path = "owned_local_initial_plan_tests.rs"]
mod tests;
