// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Backend provenance for observed turns and active goals, independent of the displayed widget.
//! Only routed starts and active goal observations allocate entries. Completion, close, stream
//! loss and routing retirement remove seen turns; goal clear and close remove owned goals.
//! This ephemeral App state owns no request, transport or authority and is not
//! serialized: a new App begins with the same empty seen book after process handoff.

use super::App;
use crate::app_server_session::AppServerSession;
use crate::app_server_session::AppServerSessionEvent;
use crate::backend_id::BackendId;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadGoalStatus;
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;

type SeenTurn = (String, String);

struct OwnedGoal {
    owner: BackendId,
    objective: String,
}

#[derive(Default)]
pub(super) struct State {
    seen: RefCell<HashMap<BackendId, HashSet<SeenTurn>>>,
    goals: RefCell<HashMap<String, Vec<OwnedGoal>>>,
    displayed: Cell<Option<BackendId>>,
}

impl State {
    pub(super) fn new(owner: BackendId) -> Self {
        Self {
            displayed: Cell::new(Some(owner)),
            ..Self::default()
        }
    }

    pub(super) fn displayed(&self, owner: BackendId) {
        self.displayed.set(Some(owner));
    }

    pub(super) fn displayed_goal(&self, thread: &str, goal: Option<&str>) {
        if let Some(owner) = self.displayed.get() {
            self.goal(owner, thread, goal);
        } else {
            // Legacy manually constructed App fixtures have no backend until their first event.
            codex_alab_tui::session::set_goal(thread, goal);
        }
    }

    pub(super) fn goal(&self, owner: BackendId, thread: &str, goal: Option<&str>) {
        self.change_goal(owner, thread, goal, /*admitted*/ true);
    }

    fn change_goal(&self, owner: BackendId, thread: &str, goal: Option<&str>, admitted: bool) {
        let shared = codex_alab_tui::boxes::tick_shared()
            .or_else(|| codex_alab_tui::session::active().then(codex_alab_tui::session::shared));
        let Some(shared) = shared else {
            return;
        };
        if thread.is_empty() {
            return;
        }
        let objective = goal
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| {
                &text[..text.floor_char_boundary(codex_alab_core::session::GOAL_MAX_BYTES)]
            });
        let published = {
            let mut goals = self.goals.borrow_mut();
            if (!admitted || objective.is_none())
                && !goals
                    .get(thread)
                    .is_some_and(|entries| entries.iter().any(|entry| entry.owner == owner))
            {
                return;
            }
            let entries = goals.entry(thread.to_owned()).or_default();
            if let Some(objective) = objective.filter(|_| !admitted) {
                // A parked update refreshes an already observed contribution in place;
                // it cannot replace the displayed owner's priority for a colliding key.
                if let Some(entry) = entries.iter_mut().find(|entry| entry.owner == owner) {
                    objective.clone_into(&mut entry.objective);
                }
            } else {
                entries.retain(|entry| entry.owner != owner);
                if let Some(objective) = objective {
                    entries.push(OwnedGoal {
                        owner,
                        objective: objective.to_owned(),
                    });
                }
            }
            let published = entries.last().map(|entry| entry.objective.clone());
            if entries.is_empty() {
                goals.remove(thread);
            }
            published
        };
        // Publish with no RefCell borrow held. A close/clear restores another active owner
        // of this same native key instead of erasing that backend's current objective.
        shared.set_goal(thread, published.as_deref());
    }

    fn start(&self, owner: BackendId, thread: &str, turn: &str) {
        if codex_alab_tui::boxes::tick_shared().is_some() {
            self.seen
                .borrow_mut()
                .entry(owner)
                .or_default()
                .insert((thread.to_owned(), turn.to_owned()));
        }
    }

    fn release(&self, owner: BackendId, matches: impl Fn(&SeenTurn) -> bool) {
        let released = {
            let mut seen = self.seen.borrow_mut();
            let mut released = Vec::new();
            if let Some(turns) = seen.get_mut(&owner) {
                turns.retain(|key| {
                    if matches(key) {
                        released.push(key.clone());
                        false
                    } else {
                        true
                    }
                });
            }
            seen.retain(|_, turns| !turns.is_empty());
            // Backends can reuse native thread and turn IDs. Its other observed owner still
            // holds the shared OSC key even when this backend completed or lost its stream.
            released.retain(|key| !seen.values().any(|turns| turns.contains(key)));
            released
        };
        // Waking a frame holds no borrow of App bookkeeping.
        for (thread, turn) in released {
            codex_alab_tui::turns::seen(&thread, &turn, false);
        }
    }

    pub(super) fn retain(&self, keep: impl Fn(&str) -> bool) {
        let mut seen = self.seen.borrow_mut();
        for turns in seen.values_mut() {
            turns.retain(|(thread, _)| keep(thread));
        }
        seen.retain(|_, turns| !turns.is_empty());
    }

    pub(super) fn stream(&self, owner: BackendId, event: &AppServerEvent) {
        match event {
            AppServerEvent::Lagged { .. } | AppServerEvent::Disconnected { .. } => {
                self.release(owner, |_| true);
            }
            AppServerEvent::ServerNotification(notification) => match notification.as_ref() {
                ServerNotification::TurnCompleted(completed) => {
                    self.release(owner, |(thread, turn)| {
                        thread == &completed.thread_id && turn == &completed.turn.id
                    });
                }
                ServerNotification::ThreadClosed(closed) => {
                    self.release(owner, |(thread, _)| thread == &closed.thread_id);
                    self.goal(owner, &closed.thread_id, None);
                }
                ServerNotification::ThreadGoalCleared(cleared) => {
                    self.change_goal(owner, &cleared.thread_id, None, /*admitted*/ false);
                }
                ServerNotification::ThreadGoalUpdated(updated) => {
                    let goal = (updated.goal.status == ThreadGoalStatus::Active)
                        .then_some(updated.goal.objective.as_str());
                    self.change_goal(owner, &updated.thread_id, goal, /*admitted*/ false);
                }
                _ => {}
            },
            AppServerEvent::ServerRequest(_) => {}
        }
    }
}

/// Observe a resumed or live widget's goal under the backend selected by startup/navigation.
pub(super) fn observe_goal(app: &App) {
    if let Some(thread) = app.chat_widget.thread_id() {
        app.alab_backend_bookkeeping
            .displayed_goal(&thread.to_string(), app.chat_widget.alab_goal());
    }
}

/// Called only after the normal thread-routing admission accepted this notification.
pub(super) fn routed(app: &App, owner: BackendId, notification: &ServerNotification) {
    if let ServerNotification::TurnStarted(started) = notification {
        app.alab_backend_bookkeeping
            .start(owner, &started.thread_id, &started.turn.id);
    }
    super::alab_session::routed(app, owner, notification);
}

/// Parked events may retire their own bookkeeping and invalidate a pending native scan, but
/// never reach the displayed backend's UI. Verify the actual session owner before either.
pub(super) fn parked(
    app: &App,
    session: &AppServerSession,
    displayed: BackendId,
    event: &AppServerSessionEvent,
) {
    if event.backend_id != session.backend_id() {
        return;
    }
    app.alab_backend_bookkeeping.displayed(displayed);
    if event.backend_id == displayed {
        return;
    }
    codex_alab_tui::upgrade::native_event();
    app.alab_backend_bookkeeping
        .stream(event.backend_id, &event.event);
}

#[cfg(test)]
#[path = "backend_bookkeeping_tests.rs"]
mod tests;
