// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Typed operation provenance for policy refusals. A compaction request must never
//! be replayed merely to reach an instruction about independent work.

use std::collections::HashSet;
use std::sync::Mutex;

#[derive(Default)]
pub struct Activity(Mutex<HashSet<String>>);

impl Activity {
    pub fn started(&self, id: String) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id);
    }

    pub fn completed(&self, id: &str) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(id);
    }

    pub fn pending(&self) -> bool {
        !self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::Activity;

    #[test]
    fn only_the_matching_completion_retires_a_pending_compaction() {
        let activity = Activity::default();
        activity.started("current".into());
        activity.completed("older");
        assert!(activity.pending());
        activity.started("second".into());
        activity.completed("current");
        assert!(activity.pending());
        activity.completed("second");
        assert!(!activity.pending());
    }
}
