// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! WHO STOPPED A THREAD'S GOAL (the forever-persist lane, item c; owner 2026-10-19: FOREVER
//! CRUISE, FOREVER LAND, "a person's hand wins"): the goal store keeps a goal's status and no
//! stop event, so a goal a person blocked or paused through the app-server's `thread/goal/set`
//! while a turn ran, and that turn then met a wall codex-alab retries, read as that wall's stop,
//! and the wall's try resumed it over the person. The app-server notes each set a person makes
//! as it passes (its one `ALAB(autopilot)` hook, where the request's origin is `user`; the TUI
//! only pauses, resumes, edits or clears a goal, so a person's block comes from another client),
//! and this process's shared state counts, per thread, the sets that left the goal stopped
//! ([`crate::session::Shared::person_goal_set`]): the autopilot binds a turn error's goal to its
//! wall only where no person's stop came since the turn began, its try resumes that goal only
//! where none came since it was bound, and a stop takes the bound goal out of the pace the
//! record carries, so no image after a re-exec or a relaunch resumes it either. Upstream records
//! the same set in the thread's history (a host-annotated `user.goal` instruction), which the
//! image after reads for the stop a dead process's record never wrote.

/// A PERSON SET THREAD `thread`'S GOAL (the app-server's `thread/goal/set` with a `user` origin),
/// leaving it `stopped` (any status but active): this process's shared state counts the stop
/// ([`crate::session::Shared::person_goal_set`]).
pub fn person_set(thread: &str, stopped: bool) {
    crate::session::shared().person_goal_set(thread, stopped);
}

/// How many of a person's stops of thread `thread`'s goal this process has seen
/// ([`person_set`]).
pub fn person_stops(thread: &str) -> u64 {
    crate::session::shared().person_goal_stops(thread)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Pace;
    use crate::session::Shared;
    use crate::session::WallGoal;

    /// A PERSON'S STOP IS COUNTED PER THREAD; a set that leaves the goal active is not.
    /// NEGATIVE CONTROLS: another thread's count stays, and an unnamed thread counts nothing.
    #[test]
    fn a_persons_stop_is_counted_per_thread() {
        let (thread, other) = ("019a-goal-stops-thread", "019a-goal-stops-other");
        assert_eq!(person_stops(thread), 0);
        person_set(thread, true);
        person_set(thread, false);
        person_set(thread, true);
        assert_eq!(person_stops(thread), 2);
        assert_eq!(person_stops(other), 0);
        person_set("", true);
        assert_eq!(person_stops(""), 0);
    }

    /// A PERSON'S STOP TAKES THE WALL'S GOAL OUT OF THE PACE (review of the forever-persist
    /// lane, F2): the goal a turn error blocked at a wall, bound at the count it was bound at,
    /// leaves the thread's pace when a person stops the goal (the record a re-exec or a relaunch
    /// takes up carries it no more), and a pace written afterwards with that binding keeps none.
    /// NEGATIVE CONTROLS: a set that leaves the goal active takes nothing; another thread's pace
    /// keeps its goal; a binding at the count after the stop is kept; and the rest of the pace
    /// stays.
    #[test]
    fn a_persons_stop_takes_the_walls_goal_out_of_the_pace() {
        let shared: &'static Shared = Box::leak(Box::new(Shared::new()));
        let (thread, other) = ("thread-a", "thread-b");
        let goal = |person_stops| WallGoal {
            goal_id: "goal-1".to_string(),
            objective: "finish the original task".to_string(),
            token_budget: None,
            turn: "turn-1".to_string(),
            person_stops,
        };
        let pace = |wall_goal| Pace {
            short_streak: 2,
            wall_goal,
            ..Pace::default()
        };
        shared.set_pace(thread, pace(Some(goal(0))));
        shared.set_pace(other, pace(Some(goal(0))));
        shared.person_goal_set(thread, false);
        assert_eq!(shared.pace(thread), Some(pace(Some(goal(0)))));
        shared.person_goal_set(thread, true);
        assert_eq!(shared.person_goal_stops(thread), 1);
        assert_eq!(shared.pace(thread), Some(pace(None)), "taken out");
        assert_eq!(shared.pace(other), Some(pace(Some(goal(0)))), "another's");
        // Written again with the binding the stop came after: none is kept.
        shared.set_pace(thread, pace(Some(goal(0))));
        assert_eq!(shared.pace(thread), Some(pace(None)), "never written back");
        // Bound after the stop: kept.
        shared.set_pace(thread, pace(Some(goal(1))));
        assert_eq!(shared.pace(thread), Some(pace(Some(goal(1)))));
        // A pace that held only the goal is at rest once it goes.
        let lone = "thread-c";
        shared.set_pace(
            lone,
            Pace {
                wall_goal: Some(goal(0)),
                ..Pace::default()
            },
        );
        shared.person_goal_set(lone, true);
        assert_eq!(shared.pace(lone), None);
    }
}
