// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A resumed thread's history as the carry-on reads it, from a rollout in the paginated
//! store's form (whose `load_history` the store refuses): the lines are the end-to-end
//! run's own (2026-10-16), the turn ids shortened.

use pretty_assertions::assert_eq;

use crate::extension::scan_history;

const PERSON: &str = r#"{"timestamp":"2026-10-16T22:06:56.447Z","ordinal":1,"type":"event_msg","payload":{"type":"task_started","turn_attribution":{"turn_id":"t1","turn_trigger":"user","parent_turn_id":null,"initiating_agent_path":null,"root_turn_id":"t1"},"turn_id":"t1","root_turn_id":"t1","started_at":1792188416,"model_context_window":258400,"collaboration_mode_kind":"default"}}"#;
const DONE: &str = r#"{"timestamp":"2026-10-16T22:06:56.463Z","ordinal":13,"type":"event_msg","payload":{"type":"task_complete","root_turn_id":"t1","turn_id":"t1","last_agent_message":"Here is step one of the plan. I will continue when asked.","started_at":1792188416,"completed_at":1792188416,"duration_ms":30,"time_to_first_token_ms":28}}"#;
const OURS: &str = r#"{"timestamp":"2026-10-16T22:07:01.441Z","ordinal":14,"type":"event_msg","payload":{"type":"task_started","turn_attribution":{"turn_id":"t2","turn_trigger":"alab_autopilot","parent_turn_id":null,"initiating_agent_path":null,"root_turn_id":"t2"},"turn_id":"t2","root_turn_id":"t2","started_at":1792188421,"model_context_window":258400,"collaboration_mode_kind":"default"}}"#;

async fn scan(lines: &[&str]) -> (bool, Option<String>) {
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("scratch dir: {err}"));
    let path = dir.path().join("rollout.jsonl");
    std::fs::write(&path, lines.join("\n") + "\n").unwrap_or_else(|err| panic!("{err}"));
    let (items, _, _) = codex_core::RolloutRecorder::load_rollout_items(&path)
        .await
        .unwrap_or_else(|err| panic!("rollout: {err}"));
    scan_history(&items)
}

#[tokio::test]
async fn a_paginated_rollout_names_the_task_and_the_last_reply() {
    assert_eq!(
        scan(&[PERSON, DONE, OURS]).await,
        (
            true,
            Some("Here is step one of the plan. I will continue when asked.".to_string())
        )
    );
    // Negative control: only codex-alab's own turns are no task.
    assert_eq!(scan(&[OURS]).await, (false, None));
}
