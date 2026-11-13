// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

// Core runtime types form a recursive graph during auto-trait evaluation.
#![recursion_limit = "256"]

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
//! - [`needs`]: the NEEDS YOU and `Nothing left:` lines, aterm's readers verbatim.
//! - [`go_ahead`]: a park on a person's go-ahead for the agent's own reversible work, and the
//!   owner's standing go-ahead that answers it (`go-ahead@v1`), aterm's reader verbatim.
//! - [`open_work`]: what a report still names open, Cruise's reading and Land's strict one.
//! - [`repo`]: what the thread's repository holds that no remote main has.
//! - [`decide`]: the pure turn-end decider (FOREVER CRUISE / FOREVER LAND).
//! - [`replies`]: ruling F's typing stop, the replies the short streak keeps.
//! - [`answer`]: the question overlay's and the plan box's answer.
//! - [`approval`]: the approval UNLESS list.
//! - [`supervisor`]: the claim, the policy, the checks, attention and visibility.
//! - [`story`]: the one act aterm is told of, a question box answered (`story chose`).
//! - [`describe`]: the goal and plan step the session's aterm description says.
//! - [`netwatch`]: the API's reach, measured on a network wall's own route.
//! - [`login`]: a lost login's return, read through codex's own `AuthManager`.
//! - [`upgrade`]: the live upgrade's preconditions.
//! - [`upgrade_ask`]: the live upgrade's readiness notices under aterm's `upgrade_ask`.
//! - [`upgrade_readiness`]: whether an answer says ready to restart, aterm's reader verbatim.
//! - [`extension`]: the contributors, the per-thread timers and the acts.

#[cfg(unix)]
pub use codex_alab_aterm as aterm;
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
pub mod go_ahead;
mod in_flight_retry;
pub mod login;
pub mod mounts;
pub mod needs;
pub mod netwatch;
pub mod open_work;
mod owed_origin;
#[cfg(test)]
mod owed_origin_tests;
mod recovery;
pub mod replies;
pub mod repo;
mod resume_history;
pub use resume_history::unfinished_recovery_turn;
pub mod story;
pub mod supervisor;
pub mod upgrade;
pub mod upgrade_goals;
#[cfg(unix)]
pub mod upgrade_source;
#[cfg(unix)]
pub mod upgrade_state;
#[cfg(unix)]
pub mod upgrade_destination;
pub mod upgrade_ask;
mod upgrade_monitor;
pub mod upgrade_readiness;
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
    if !codex_alab_core::is_alab_binary() {
        return;
    }
    static SUPERVISOR: OnceLock<Arc<Supervisor>> = OnceLock::new();
    let supervisor = SUPERVISOR
        .get_or_init(|| Supervisor::new(SupervisorOptions::from_env()))
        .clone();
    let autopilot = Autopilot::new(
        supervisor,
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
mod capacity_crash_tests;
#[cfg(test)]
mod classify_tests;
#[cfg(test)]
mod decide_tests;
#[cfg(test)]
mod mounts_tests;
#[cfg(test)]
mod netwatch_tests;
#[cfg(test)]
mod open_work_tests;
#[cfg(test)]
mod passive_tests;
#[cfg(test)]
mod probe_scope_tests;
#[cfg(test)]
mod replies_tests;
#[cfg(test)]
mod repo_tests;
#[cfg(test)]
mod rest_tests;
#[cfg(test)]
mod resume_tests;
#[cfg(test)]
mod same_reply_tests;
#[cfg(test)]
mod timer_tests;
#[cfg(test)]
mod upgrade_ask_tests;
#[cfg(test)]
mod upgrade_readiness_tests;
#[cfg(test)]
mod upgrade_tests;
#[cfg(test)]
mod words_tests;
