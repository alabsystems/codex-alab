// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native bounded agent-status presentation, without creating agents, stores or subscriptions.
use super::*;
use crate::terminal_hyperlinks::alab_retained_text as native;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Data {
    entries: Vec<Entry>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    agent_path: String,
    activity: Vec<String>,
}
impl Data {
    pub(crate) fn capture(actual: &dyn HistoryCell) -> io::Result<Option<Self>> {
        let Some(actual) = actual.as_any().downcast_ref::<AgentStatusHistoryCell>() else {
            return Ok(None);
        };
        if actual.entries.len() > native::MAX_NODES {
            return Err(native::invalid("native agent-status capacity"));
        }
        Ok(Some(Self {
            entries: actual
                .entries
                .iter()
                .map(|entry| Entry {
                    agent_path: entry.agent_path.clone(),
                    activity: entry.activity.clone(),
                })
                .collect(),
        }))
    }
    pub(crate) fn restore(self) -> io::Result<Box<dyn HistoryCell>> {
        if self.entries.len() > native::MAX_NODES {
            return Err(native::invalid("native agent-status capacity"));
        }
        Ok(Box::new(AgentStatusHistoryCell {
            entries: self
                .entries
                .into_iter()
                .map(|entry| AgentStatusThreadPreview {
                    agent_path: entry.agent_path,
                    activity: entry.activity,
                })
                .collect(),
        }))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_agent_status_retains_complete_paths_activity_and_empty_previews() -> io::Result<()> {
        let original = AgentStatusHistoryCell::new(vec![
            AgentStatusThreadPreview {
                agent_path: "actual/root/worker".into(),
                activity: vec![
                    "exact first preview\ncontinuation".into(),
                    "second original preview".into(),
                ],
            },
            AgentStatusThreadPreview::empty("actual/root/empty".into()),
        ]);
        let data = Data::capture(&original)?
            .ok_or_else(|| native::invalid("native agent-status variant missing"))?;
        let restored =
            serde_json::from_slice::<Data>(&serde_json::to_vec(&data).map_err(native::invalid)?)
                .map_err(native::invalid)?
                .restore()?;
        let actual = restored
            .as_any()
            .downcast_ref::<AgentStatusHistoryCell>()
            .ok_or_else(|| native::invalid("native agent-status type changed"))?;
        assert_eq!(actual.entries[0].agent_path, original.entries[0].agent_path);
        assert_eq!(actual.entries[0].activity, original.entries[0].activity);
        assert_eq!(actual.entries[1].agent_path, original.entries[1].agent_path);
        assert!(actual.entries[1].activity.is_empty());
        for width in [0, 5, 24, 97] {
            assert_eq!(original.display_lines(width), restored.display_lines(width));
        }
        assert_eq!(original.raw_lines(), restored.raw_lines());
        Ok(())
    }
}
