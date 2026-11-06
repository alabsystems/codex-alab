// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::ThreadStore;
use crate::local::test_support::test_config;
use crate::local::test_support::write_session_file_with_history_mode;
use codex_protocol::ThreadId;
use codex_protocol::protocol::HistoryPosition;
use codex_rollout::CompactedItem;
use codex_rollout::RolloutLine;
use serde_json::json;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use tempfile::TempDir;
use uuid::Uuid;

fn event(value: serde_json::Value) -> RolloutItem {
    RolloutItem::EventMsg(serde_json::from_value(value).expect("native event"))
}

fn start(turn: &str) -> RolloutItem {
    event(json!({"type":"task_started", "turn_id":turn,
        "model_context_window":258400, "collaboration_mode_kind":"default"}))
}

fn input() -> RolloutItem {
    event(json!({"type":"user_message", "message":"original request",
        "images":null, "local_images":[], "text_elements":[]}))
}

fn complete(turn: &str) -> RolloutItem {
    event(json!({"type":"task_complete", "turn_id":turn, "last_agent_message":"done"}))
}

fn compacted() -> RolloutItem {
    RolloutItem::Compacted(Box::new(CompactedItem {
        message: "shorter model context".into(),
        replacement_history: Some(Vec::new()),
        guardian_history: None,
        retained_context: None,
        mcp_resource_origins: None,
        window_number: Some(1),
        first_window_id: None,
        previous_window_id: None,
        window_id: None,
        compaction_response_id: None,
        latest_token_usage_record: None,
        resume_metadata: None,
    }))
}

fn append(path: &Path, ordinal: u64, item: RolloutItem) {
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .expect("open rollout");
    let line = RolloutLine {
        timestamp: "2025-01-03T13:00:01Z".into(),
        ordinal: Some(ordinal),
        item,
    };
    writeln!(
        file,
        "{}",
        serde_json::to_string(&line).expect("encode record")
    )
    .expect("append record");
}

fn fixture(home: &Path, id: u128) -> (ThreadId, PathBuf) {
    let uuid = Uuid::from_u128(id);
    let path = write_session_file_with_history_mode(
        home,
        "2025-01-03T13-00-00",
        uuid,
        ThreadHistoryMode::Paginated,
    )
    .expect("rollout");
    (
        ThreadId::from_string(&uuid.to_string()).expect("thread ID"),
        path,
    )
}

fn params(thread_id: ThreadId) -> LoadThreadHistoryParams {
    LoadThreadHistoryParams {
        thread_id,
        include_archived: false,
    }
}

fn same(actual: &[RolloutItem], expected: &[RolloutItem]) {
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

#[tokio::test]
async fn exec_recovery_history_retains_turn_bound_agent_input() {
    let home = TempDir::new().unwrap();
    let (id, path) = fixture(home.path(), 7110);
    let mail = RolloutItem::ResponseItem(codex_rollout::ResponseItemEnvelope {
        item: serde_json::from_value(
            json!({"type":"agent_message", "author":"/root", "recipient":"/root/child",
            "content":[{"type":"encrypted_content", "encrypted_content":"original task"}],
            "internal_chat_message_metadata_passthrough":{"turn_id":"original"}}),
        )
        .unwrap(),
        metadata: None,
    });
    append(&path, 1, start("original"));
    append(&path, 2, mail.clone());
    append(&path, 3, compacted());
    let store = LocalThreadStore::new(test_config(home.path()), None);
    same(
        &store
            .load_latest_turn_history(params(id))
            .await
            .unwrap()
            .items,
        &[start("original"), mail.clone()],
    );
    append(&path, 4, complete("original"));
    same(
        &store
            .load_latest_turn_history(params(id))
            .await
            .unwrap()
            .items,
        &[start("original"), mail, complete("original")],
    );
    append(&path, 5, start("later"));
    same(
        &store
            .load_latest_turn_history(params(id))
            .await
            .unwrap()
            .items,
        &[start("later")],
    );
}

#[tokio::test]
async fn exec_recovery_history_crosses_compaction_and_retains_terminal_events() {
    let home = TempDir::new().unwrap();
    let (id, path) = fixture(home.path(), 7101);
    append(&path, 1, start("original"));
    append(&path, 2, input());
    append(&path, 3, compacted());
    let store = LocalThreadStore::new(test_config(home.path()), None);
    let model = store.load_latest_model_context(params(id)).await.unwrap();
    assert!(
        !model
            .items
            .iter()
            .any(|item| matches!(item, RolloutItem::EventMsg(EventMsg::TurnStarted(_))))
    );
    let history = store.load_latest_turn_history(params(id)).await.unwrap();
    same(&history.items, &[start("original"), input()]);
    assert_eq!(history.thread_id, id);
    assert!(history.revision.is_some());

    append(&path, 4, complete("original"));
    same(
        &store
            .load_latest_turn_history(params(id))
            .await
            .unwrap()
            .items,
        &[start("original"), input(), complete("original")],
    );
    append(&path, 5, start("later"));
    same(
        &store
            .load_latest_turn_history(params(id))
            .await
            .unwrap()
            .items,
        &[start("later")],
    );
}

#[tokio::test]
async fn exec_recovery_history_retains_input_and_terminals_across_repeated_starts() {
    let home = TempDir::new().unwrap();
    let (id, path) = fixture(home.path(), 7105);
    append(&path, 1, start("older"));
    append(&path, 2, input());
    append(&path, 3, complete("older"));
    append(&path, 4, start("original"));
    append(&path, 5, input());
    append(&path, 6, compacted());
    append(&path, 7, start("original"));
    let store = LocalThreadStore::new(test_config(home.path()), None);
    same(
        &store
            .load_latest_turn_history(params(id))
            .await
            .unwrap()
            .items,
        &[start("original"), input(), start("original")],
    );
    append(&path, 8, complete("original"));
    append(&path, 9, start("original"));
    same(
        &store
            .load_latest_turn_history(params(id))
            .await
            .unwrap()
            .items,
        &[
            start("original"),
            input(),
            start("original"),
            complete("original"),
            start("original"),
        ],
    );
}

#[tokio::test]
async fn exec_recovery_history_obeys_frozen_lineage_and_compressed_ancestors() {
    let home = TempDir::new().unwrap();
    let (parent, parent_path) = fixture(home.path(), 7102);
    append(&parent_path, 1, start("inherited"));
    append(&parent_path, 2, input());
    let frozen = std::fs::metadata(&parent_path).unwrap().len();
    append(&parent_path, 3, complete("inherited"));
    let (child, child_path) = fixture(home.path(), 7103);
    let contents = std::fs::read_to_string(&child_path).unwrap();
    let mut meta: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
    meta["payload"]["history_base"] = serde_json::to_value(HistoryPosition {
        thread_id: parent,
        end_ordinal_exclusive: 3,
        end_byte_offset: frozen,
    })
    .unwrap();
    std::fs::write(&child_path, format!("{meta}\n")).unwrap();
    append(&child_path, 4, compacted());
    let store = LocalThreadStore::new(test_config(home.path()), None);
    same(
        &store
            .load_latest_turn_history(params(child))
            .await
            .unwrap()
            .items,
        &[start("inherited"), input()],
    );

    zstd::stream::copy_encode(
        std::fs::File::open(&parent_path).unwrap(),
        std::fs::File::create(parent_path.with_extension("jsonl.zst")).unwrap(),
        3,
    )
    .unwrap();
    std::fs::remove_file(&parent_path).unwrap();
    same(
        &store
            .load_latest_turn_history(params(child))
            .await
            .unwrap()
            .items,
        &[start("inherited"), input()],
    );
    assert!(
        !parent_path.exists(),
        "reading must not publish decoded ancestor files"
    );
}

#[tokio::test]
async fn exec_recovery_history_refuses_unreadable_terminal_records() {
    let home = TempDir::new().unwrap();
    let (id, path) = fixture(home.path(), 7104);
    append(&path, 1, start("original"));
    append(&path, 2, input());
    append(&path, 3, compacted());
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(file, "{{broken terminal record").unwrap();
    let store = LocalThreadStore::new(test_config(home.path()), None);
    assert!(store.load_latest_turn_history(params(id)).await.is_err());
}

#[tokio::test]
async fn exec_recovery_history_keeps_review_parent_across_forwarded_starts() {
    let home = TempDir::new().unwrap();
    let (id, path) = fixture(home.path(), 7120);
    let entered = event(
        json!({"type":"item_completed","thread_id":id,"turn_id":"parent", "item":{"type":"EnteredReviewMode","id":"entry","target":{"type":"custom","instructions":"review"},"user_facing_hint":"Review"}}),
    );
    let exited = event(
        json!({"type":"item_completed","thread_id":id,"turn_id":"parent", "item":{"type":"ExitedReviewMode","id":"exit","review_output":null}}),
    );
    let store = LocalThreadStore::new(test_config(home.path()), None);
    append(&path, 1, start("previous"));
    append(&path, 2, complete("previous"));
    append(&path, 3, entered.clone());
    append(&path, 4, start("delegate"));
    let mut expected = vec![entered, start("delegate")];
    same(
        &store
            .load_latest_turn_history(params(id))
            .await
            .unwrap()
            .items,
        &expected,
    );
    append(&path, 5, exited.clone());
    append(&path, 6, complete("parent"));
    expected.extend([exited, complete("parent")]);
    same(
        &store
            .load_latest_turn_history(params(id))
            .await
            .unwrap()
            .items,
        &expected,
    );
    append(&path, 7, start("later"));
    append(&path, 8, input());
    same(
        &store
            .load_latest_turn_history(params(id))
            .await
            .unwrap()
            .items,
        &[start("later"), input()],
    );
}

#[tokio::test]
async fn exec_recovery_history_retains_review_across_parent_restart_and_later_abort() {
    let home = TempDir::new().unwrap();
    let (id, path) = fixture(home.path(), 7121);
    let entered = event(
        json!({"type":"item_completed","thread_id":id,"turn_id":"parent", "item":{"type":"EnteredReviewMode","id":"entry","target":{"type":"custom","instructions":"review"},"user_facing_hint":"Review"}}),
    );
    let store = LocalThreadStore::new(test_config(home.path()), None);
    append(&path, 1, entered.clone());
    append(&path, 2, start("delegate"));
    append(&path, 3, start("parent"));
    same(
        &store
            .load_latest_turn_history(params(id))
            .await
            .unwrap()
            .items,
        &[entered.clone(), start("delegate"), start("parent")],
    );
    let aborted = event(json!({"type":"turn_aborted","turn_id":"parent","reason":"interrupted"}));
    append(&path, 4, aborted.clone());
    same(
        &store
            .load_latest_turn_history(params(id))
            .await
            .unwrap()
            .items,
        &[entered, start("delegate"), start("parent"), aborted],
    );
    append(&path, 5, start("later"));
    append(&path, 6, input());
    same(
        &store
            .load_latest_turn_history(params(id))
            .await
            .unwrap()
            .items,
        &[start("later"), input()],
    );
}
