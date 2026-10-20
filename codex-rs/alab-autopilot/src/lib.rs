// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! codex-alab's native autopilot (CONTRACT §5): what aterm's screen-scraping Codex lane did
//! (retired at aterm 83bd43c206), done inside codex from Codex's own events, while
//! codex-alab holds a PROVEN supervisor claim on its aterm session.
//!
//! An app-server extension, installed by one `ALAB(autopilot)` line in
//! `app-server/src/extensions.rs` ([`install`]). It acts only:
//! - in the codex binary ([`codex_alab_core::active`]; upstream's tests never set it, so
//!   [`install`] registers nothing there);
//! - for root threads of the TUI (`SessionSource::Cli`) that the TUI registered in the shared
//!   state (`codex_alab_core::session`), never exec, app-server clients or sub-agents;
//! - while a proven claim is held and it is not passive under it ([`supervisor`]), after the
//!   before-every-act checks.
//!
//! - [`words`]: aterm's word lists and texts, verbatim.
//! - [`decide`]: the pure turn-end decider.
//! - [`answer`]: the question overlay's and the plan box's answer.
//! - [`approval`]: the approval UNLESS list.
//! - [`supervisor`]: the claim, the policy, the checks, attention and visibility.
//! - [`story`]: what each act, wait and escalation tells aterm's presence band.
//! - [`describe`]: the goal and plan step the session's aterm description says.
//! - [`upgrade`]: the live upgrade's preconditions.
//! - [`extension`]: the contributors, the per-thread timers and the acts.

use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::Weak;

use codex_core::ThreadManager;
use codex_core::config::Config;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_goal_extension::GoalService;

pub mod answer;
pub mod approval;
pub mod decide;
pub mod describe;
pub mod extension;
pub mod story;
pub mod supervisor;
pub mod upgrade;
pub mod words;

pub use extension::Autopilot;
pub use supervisor::Supervisor;
pub use supervisor::SupervisorOptions;

/// The ALAB(autopilot) hook: register the autopilot on `builder`. Nothing outside the codex
/// binary. The claim is the process's: every registry this process builds shares one
/// [`Supervisor`].
pub fn install(
    builder: &mut ExtensionRegistryBuilder<Config>,
    thread_manager: Weak<ThreadManager>,
    goal_service: Arc<GoalService>,
) {
    if !codex_alab_core::active() {
        return;
    }
    static SUPERVISOR: OnceLock<Arc<Supervisor>> = OnceLock::new();
    let supervisor = SUPERVISOR
        .get_or_init(|| Supervisor::new(SupervisorOptions::from_env()))
        .clone();
    let autopilot = Autopilot::new(
        supervisor,
        builder.event_sink(),
        Some(thread_manager),
        Some(goal_service),
        decide::Timing::default(),
        extension::tui_threads,
    );
    autopilot.register(builder);
}

#[cfg(test)]
mod answer_tests;
#[cfg(test)]
mod approval_tests;
#[cfg(test)]
mod classify_tests;
#[cfg(test)]
mod decide_tests;
#[cfg(test)]
mod passive_tests;
#[cfg(test)]
mod rest_tests;
#[cfg(test)]
mod resume_tests;
#[cfg(test)]
mod timer_tests;
#[cfg(test)]
mod upgrade_tests;
#[cfg(test)]
mod words_tests;
