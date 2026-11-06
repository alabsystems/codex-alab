// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Failure context for bounded native test scenarios, without extending their deadlines.

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

#[derive(Clone)]
pub(super) struct Progress(Arc<Mutex<State>>);

struct State {
    case: String,
    phase: &'static str,
    started: Instant,
    case_started: Instant,
    phase_started: Instant,
    has_case: bool,
    completed_cases: Vec<(String, Duration)>,
    phase_totals: BTreeMap<&'static str, Duration>,
    slow_phases: Vec<(Duration, String, &'static str)>,
}

impl Default for Progress {
    fn default() -> Self {
        let now = Instant::now();
        Self(Arc::new(Mutex::new(State {
            case: "not started".to_owned(),
            phase: "create test future",
            started: now,
            case_started: now,
            phase_started: now,
            has_case: false,
            completed_cases: Vec::new(),
            phase_totals: BTreeMap::new(),
            slow_phases: Vec::new(),
        })))
    }
}

impl Progress {
    pub(super) fn case(&self, case: impl Into<String>) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        state.finish_phase(now);
        if state.has_case {
            let completed = (state.case.clone(), now.duration_since(state.case_started));
            state.completed_cases.push(completed);
        }
        state.case = case.into();
        state.phase = "begin case";
        state.case_started = now;
        state.phase_started = now;
        state.has_case = true;
    }

    pub(super) fn phase(&self, phase: &'static str) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        state.finish_phase(now);
        state.phase = phase;
        state.phase_started = now;
    }

    pub(super) fn describe(&self) -> String {
        let state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        let phase_elapsed = now.duration_since(state.phase_started);
        let mut phase_totals = state.phase_totals.clone();
        let mut slow_phases = state.slow_phases.clone();
        if state.has_case {
            *phase_totals.entry(state.phase).or_default() += phase_elapsed;
            slow_phases.push((phase_elapsed, state.case.clone(), state.phase));
        }
        let mut phase_totals = phase_totals.into_iter().collect::<Vec<_>>();
        phase_totals.sort_unstable_by_key(|entry| Reverse(entry.1));
        slow_phases.sort_unstable_by_key(|entry| Reverse(entry.0));
        slow_phases.truncate(5);
        format!(
            "case {:?}, phase {:?}, phase elapsed {:?}, case elapsed {:?}, total elapsed {:?}; completed cases {:?}; cumulative phases {:?}; slow phases (elapsed, case, phase) {:?}",
            state.case,
            state.phase,
            phase_elapsed,
            now.duration_since(state.case_started),
            now.duration_since(state.started),
            state.completed_cases,
            phase_totals,
            slow_phases,
        )
    }
}

impl State {
    fn finish_phase(&mut self, now: Instant) {
        if !self.has_case {
            return;
        }
        let elapsed = now.duration_since(self.phase_started);
        *self.phase_totals.entry(self.phase).or_default() += elapsed;
        self.slow_phases
            .push((elapsed, self.case.clone(), self.phase));
        self.slow_phases
            .sort_unstable_by_key(|entry| Reverse(entry.0));
        self.slow_phases.truncate(5);
    }
}
