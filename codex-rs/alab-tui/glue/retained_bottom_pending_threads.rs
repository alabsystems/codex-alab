// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use serde::Deserialize;
use serde::Serialize;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    threads: Vec<String>,
}
impl Data {
    pub(crate) fn capture(v: &PendingThreadApprovals) -> Self {
        Self {
            threads: v.threads.clone(),
        }
    }
    pub(crate) fn restore(self) -> PendingThreadApprovals {
        PendingThreadApprovals {
            threads: self.threads,
        }
    }
}
#[cfg(test)]
mod controls {
    use super::*;
    #[test]
    fn original_pending_thread_order_and_update_identity_survive() {
        let mut original = PendingThreadApprovals::new();
        original.set_threads(vec!["original B".into(), "original A".into()]);
        let mut restored = Data::capture(&original).restore();
        assert_eq!(restored.threads, original.threads);
        assert!(!restored.set_threads(original.threads.clone()));
    }
}
