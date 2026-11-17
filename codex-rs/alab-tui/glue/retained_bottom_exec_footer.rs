// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
use super::*;
use serde::Deserialize;
use serde::Serialize;
/// Original native process-summary strings; no process/task resource is created.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    processes: Vec<String>,
}
impl Data {
    pub(crate) fn capture(v: &UnifiedExecFooter) -> Self {
        Self {
            processes: v.processes.clone(),
        }
    }
    pub(crate) fn restore(self) -> UnifiedExecFooter {
        UnifiedExecFooter {
            processes: self.processes,
        }
    }
}
#[cfg(test)]
mod controls {
    use super::*;
    #[test]
    fn original_summary_setter_continues_from_exact_processes() {
        let mut original = UnifiedExecFooter::new();
        original.set_processes(vec!["first actual command".into(), "second".into()]);
        let mut restored = Data::capture(&original).restore();
        assert_eq!(restored.processes, original.processes);
        assert_eq!(restored.summary_text(), original.summary_text());
        assert!(!restored.set_processes(original.processes.clone()));
        assert!(restored.set_processes(vec!["actual later summary".into()]));
    }
}
